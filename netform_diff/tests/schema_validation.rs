//! the json netform reads and writes validates against `schemas/`: every replay
//! fixture against fixture.schema.json, and the diff `config-diff --json` writes
//! and the plan `--plan-json` writes against diff.schema.json and
//! plan.schema.json.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Registry, Validator};
use netform_diff::{
    NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig, build_plan,
    diff_documents, finding_code,
};
use netform_ir::{Document, parse_generic};
use serde_json::Value;

fn json_files(dir: &str) -> Vec<(PathBuf, Value)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(dir);
    let mut paths = fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect::<Vec<_>>();
    paths.sort();
    assert!(!paths.is_empty(), "no json files in {}", dir.display());
    paths
        .into_iter()
        .map(|path| {
            let text =
                fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let value = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            (path, value)
        })
        .collect()
}

struct Schemas {
    diff: Validator,
    plan: Validator,
    fixture: Validator,
}

impl Schemas {
    /// every file in `schemas/`, registered under its `$id` so the `$ref`s
    /// between them resolve without retrieving anything.
    fn load() -> Self {
        let schemas = json_files("schemas");
        let resources = schemas.iter().map(|(path, schema)| {
            let id = schema["$id"]
                .as_str()
                .unwrap_or_else(|| panic!("{}: no $id", path.display()));
            (id, schema)
        });
        let registry = Registry::new()
            .extend(resources)
            .and_then(|builder| builder.prepare())
            .expect("register schemas");
        let validator = |name: &str| {
            let (path, schema) = schemas
                .iter()
                .find(|(path, _)| path.ends_with(name))
                .unwrap_or_else(|| panic!("no schemas/{name}"));
            jsonschema::options()
                .with_registry(&registry)
                .offline()
                .build(schema)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
        };
        Self {
            diff: validator("diff.schema.json"),
            plan: validator("plan.schema.json"),
            fixture: validator("fixture.schema.json"),
        }
    }
}

fn assert_valid(validator: &Validator, instance: &Value, what: &str) {
    let errors = validator
        .iter_errors(instance)
        .map(|err| format!("  at {:?}: {err}", err.instance_path().as_str()))
        .collect::<Vec<_>>();
    assert!(errors.is_empty(), "{what}:\n{}", errors.join("\n"));
}

/// the diff and the plan of two documents, as `--json` and `--plan-json` write
/// them, each validated against its schema.
fn validated_outputs(
    schemas: &Schemas,
    (a, b): (&Document, &Document),
    options: NormalizeOptions,
    what: &str,
) -> (Value, Value) {
    let diff = diff_documents(a, b, options).unwrap_or_else(|err| panic!("{what}: {err}"));
    let plan = serde_json::to_value(build_plan(&diff)).expect("serialize plan");
    let diff = serde_json::to_value(&diff).expect("serialize diff");
    assert_valid(&schemas.diff, &diff, &format!("{what}: diff"));
    assert_valid(&schemas.plan, &plan, &format!("{what}: plan"));
    (diff, plan)
}

#[test]
fn every_fixture_validates_against_the_fixture_schema() {
    let schemas = Schemas::load();
    for (path, fixture) in json_files("fixtures") {
        assert_valid(&schemas.fixture, &fixture, &path.display().to_string());
    }
}

#[test]
fn every_fixture_diff_and_plan_validate_under_each_order_policy() {
    let schemas = Schemas::load();
    for (path, fixture) in json_files("fixtures") {
        let name = path.display();
        let field = |key: &str| fixture[key].clone();
        let dialect = fixture["dialect"]
            .as_str()
            .unwrap_or(netform_dialects::GENERIC.name);
        let parse = netform_dialects::find(dialect)
            .unwrap_or_else(|| panic!("{name}: unknown dialect {dialect}"))
            .parse;
        let text = |key: &str| field(key).as_str().map(parse).expect("config text");
        let (intended, actual) = (text("intended"), text("actual"));
        let steps: Vec<NormalizationStep> =
            serde_json::from_value(field("normalization_steps")).expect("normalization steps");
        let policy: OrderPolicyConfig =
            serde_json::from_value(field("order_policy")).expect("order policy");

        for default in [
            OrderPolicy::Ordered,
            OrderPolicy::Unordered,
            OrderPolicy::KeyedStable,
        ] {
            let options =
                NormalizeOptions::new(steps.clone()).with_order_policy(OrderPolicyConfig {
                    default,
                    overrides: policy.overrides.clone(),
                });
            validated_outputs(
                &schemas,
                (&intended, &actual),
                options,
                &format!("{name} under {default:?}"),
            );
        }
    }
}

/// `leaves` under `depth` nested blocks.
fn nested(depth: usize, leaves: &[&str]) -> String {
    let headers = (0..depth).map(|level| format!("{}level{level}\n", " ".repeat(level)));
    let leaves = leaves
        .iter()
        .map(|leaf| format!("{}{leaf}\n", " ".repeat(depth)));
    headers.chain(leaves).collect()
}

fn tags<'a>(items: &'a Value, key: &'a str) -> impl Iterator<Item = &'a str> {
    items
        .as_array()
        .expect("array")
        .iter()
        .map(move |item| item[key].as_str().expect("string tag"))
}

/// between them these produce every edit type, plan action, plan line edit and
/// finding code the engine emits for parsed configuration. `missing_anchor` is
/// not among them: the engine anchors each edit on its own first lines, and a
/// nested insert on its parent's left header, so no parsed input is known to
/// reach it.
#[test]
fn every_edit_action_and_finding_kind_validates() {
    let deep = |leaves| nested(126, leaves);
    let cases = [
        (
            "replace",
            "hostname a\n".to_string(),
            "hostname b\n".to_string(),
        ),
        (
            "delete",
            "hostname a\nntp server 1\n".into(),
            "hostname a\n".into(),
        ),
        (
            "insert",
            "hostname a\n".into(),
            "hostname a\nntp server 1\n".into(),
        ),
        (
            "renamed block",
            "interface Ethernet1\n description a\n".into(),
            "interface Ethernet2\n description b\n".into(),
        ),
        (
            "repeated line",
            "logging host 1\nlogging host 1\nhostname a\n".into(),
            "logging host 1\nlogging host 1\nhostname b\n".into(),
        ),
        (
            "orphan line",
            "  orphan-line\n".into(),
            "  orphan-line\n".into(),
        ),
        (
            "past the nesting limit",
            deep(&["sub", " old"]),
            deep(&["sub", " new"]),
        ),
    ];

    let schemas = Schemas::load();
    let mut seen = BTreeSet::new();
    for (case, intended, actual) in &cases {
        let (diff, plan) = validated_outputs(
            &schemas,
            (&parse_generic(intended), &parse_generic(actual)),
            NormalizeOptions::default(),
            case,
        );
        seen.extend(tags(&diff["edits"], "type").map(|tag| format!("edit {tag}")));
        seen.extend(tags(&diff["findings"], "code").map(|code| format!("diff finding {code}")));
        seen.extend(tags(&plan["findings"], "code").map(|code| format!("plan finding {code}")));
        for action in plan["actions"].as_array().expect("actions") {
            seen.insert(format!("action {}", action["type"].as_str().expect("type")));
            if let Some(line_edits) = action.get("line_edits") {
                seen.extend(tags(line_edits, "kind").map(|kind| format!("line edit {kind}")));
            }
        }
    }

    let expected = [
        "edit Insert",
        "edit Delete",
        "edit Replace",
        "action replace_block",
        "action apply_line_edits_under_context",
        "line edit insert",
        "line edit delete",
        "line edit replace",
    ]
    .into_iter()
    .map(String::from)
    .chain(
        [
            finding_code::AMBIGUOUS_KEY_MATCH,
            finding_code::UNKNOWN_UNPARSED_CONSTRUCT,
            finding_code::DIFF_UNRELIABLE_REGION,
        ]
        .map(|code| format!("diff finding {code}")),
    )
    .chain([format!(
        "plan finding {}",
        finding_code::DIFF_UNRELIABLE_REGION
    )])
    .collect::<BTreeSet<_>>();
    assert_eq!(seen, expected);
}
