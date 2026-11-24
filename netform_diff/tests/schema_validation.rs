//! the json netform reads and writes validates against `schemas/`: every replay
//! fixture against fixture.schema.json, and the diff `config-diff --json` writes
//! and the plan `--plan-json` writes against diff.schema.json and
//! plan.schema.json.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use netform_diff::{
    NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig, build_plan,
    diff_documents, finding_code,
};
use netform_ir::{Document, parse_generic};
use serde_json::{Value, json};

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

const BASE: &str = "https://netform.local/schemas/";

/// every file in `schemas/`, by `$id`, and a validator for the json schema
/// keywords they use. it panics on any other keyword, so a schema that starts
/// using one fails these tests instead of going unchecked.
struct Schemas(HashMap<String, Value>);

impl Schemas {
    fn load() -> Self {
        let schemas = json_files("schemas").into_iter().map(|(path, schema)| {
            let id = schema["$id"]
                .as_str()
                .unwrap_or_else(|| panic!("{}: no $id", path.display()));
            (id.to_string(), schema)
        });
        Self(schemas.collect())
    }

    /// the `$id` of the file `reference` names, relative to the `$id` `base`,
    /// and the schema at its fragment.
    fn resolve(&self, base: &str, reference: &str) -> (&str, &Value) {
        let (file, pointer) = reference.split_once('#').unwrap_or((reference, ""));
        let id = match file {
            "" => base.to_string(),
            file => format!("{}{file}", &base[..=base.rfind('/').expect("base")]),
        };
        self.0
            .get_key_value(&id)
            .and_then(|(id, root)| Some((id.as_str(), root.pointer(pointer)?)))
            .unwrap_or_else(|| panic!("{base}: $ref {reference} resolves to nothing"))
    }

    /// where and why `instance` breaks the schema `reference` names.
    fn errors(&self, reference: &str, instance: &Value) -> Vec<String> {
        let (base, schema) = self.resolve(BASE, reference);
        self.check(base, schema, instance, "")
    }

    fn check(&self, base: &str, schema: &Value, instance: &Value, at: &str) -> Vec<String> {
        let schema = schema
            .as_object()
            .unwrap_or_else(|| panic!("{base}: {schema} is not a schema object"));
        let object = instance.as_object();
        let (mut reasons, mut nested) = (Vec::new(), Vec::new());
        for (keyword, value) in schema {
            match keyword.as_str() {
                "$schema" => assert_eq!(value, "https://json-schema.org/draft/2020-12/schema"),
                "$id" => assert_eq!(value, base, "a nested $id is not implemented"),
                "title" | "description" | "default" | "$defs" => {}
                "$ref" => {
                    let (base, target) = self.resolve(base, value.as_str().expect("$ref"));
                    nested.extend(self.check(base, target, instance, at));
                }
                "type" => {
                    let names = value
                        .as_array()
                        .map_or(vec![value], |names| names.iter().collect());
                    if !names.into_iter().any(|name| has_type(instance, name)) {
                        reasons.push(format!("{instance} is not of type {value}"));
                    }
                }
                "enum" => {
                    if !value.as_array().expect("enum").contains(instance) {
                        reasons.push(format!("{instance} is not one of {value}"));
                    }
                }
                "const" => {
                    if instance != value {
                        reasons.push(format!("{instance} is not {value}"));
                    }
                }
                "minimum" => {
                    if instance
                        .as_f64()
                        .is_some_and(|n| n < value.as_f64().expect("minimum"))
                    {
                        reasons.push(format!("{instance} is less than {value}"));
                    }
                }
                "required" => {
                    for name in value.as_array().expect("required") {
                        let name = name.as_str().expect("required");
                        if object.is_some_and(|object| !object.contains_key(name)) {
                            reasons.push(format!("{name:?} is a required property"));
                        }
                    }
                }
                "properties" => {
                    for (name, property) in value.as_object().expect("properties") {
                        if let Some(value) = object.and_then(|object| object.get(name)) {
                            nested.extend(self.check(
                                base,
                                property,
                                value,
                                &format!("{at}/{name}"),
                            ));
                        }
                    }
                }
                "additionalProperties" => {
                    assert_eq!(value, &Value::Bool(false), "only `false` is implemented");
                    let declared = schema.get("properties").and_then(Value::as_object);
                    for name in object.into_iter().flat_map(|object| object.keys()) {
                        if !declared.is_some_and(|declared| declared.contains_key(name)) {
                            reasons.push(format!("{name:?} is not a declared property"));
                        }
                    }
                }
                "items" => {
                    for (index, item) in instance.as_array().into_iter().flatten().enumerate() {
                        nested.extend(self.check(base, value, item, &format!("{at}/{index}")));
                    }
                }
                "oneOf" => {
                    let branches = value.as_array().expect("oneOf");
                    let matches = branches
                        .iter()
                        .filter(|branch| self.check(base, branch, instance, at).is_empty())
                        .count();
                    if matches != 1 {
                        reasons.push(format!(
                            "{instance} matches {matches} of the {} oneOf schemas, not one",
                            branches.len()
                        ));
                    }
                }
                other => panic!("{base}: the {other} keyword is not implemented"),
            }
        }
        let reasons = reasons
            .into_iter()
            .map(|reason| format!("  at {at:?}: {reason}"));
        reasons.chain(nested).collect()
    }

    fn assert_valid(&self, reference: &str, instance: &Value, what: &str) {
        let errors = self.errors(reference, instance);
        assert!(errors.is_empty(), "{what}:\n{}", errors.join("\n"));
    }
}

/// whether `instance` is of the json schema type `name`. an integer is a json
/// integer, not a float with no fraction.
fn has_type(instance: &Value, name: &Value) -> bool {
    match name.as_str().expect("type name") {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "integer" => instance.is_i64() || instance.is_u64(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        other => panic!("type {other} is not implemented"),
    }
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
    schemas.assert_valid("diff.schema.json", &diff, &format!("{what}: diff"));
    schemas.assert_valid("plan.schema.json", &plan, &format!("{what}: plan"));
    (diff, plan)
}

#[test]
fn every_fixture_validates_against_the_fixture_schema() {
    let schemas = Schemas::load();
    for (path, fixture) in json_files("fixtures") {
        schemas.assert_valid("fixture.schema.json", &fixture, &path.display().to_string());
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

    // each breaks the replace diff in one place, and the validator must reject it
    let (_, intended, actual) = &cases[0];
    let (diff, _) = validated_outputs(
        &schemas,
        (&parse_generic(intended), &parse_generic(actual)),
        NormalizeOptions::default(),
        "replace",
    );
    let breaks: [fn(&mut Value); 5] = [
        |diff| diff["order_policy"]["default"] = json!("sorted"),
        |diff| diff["stats"]["inserts"] = json!(-1),
        |diff| diff["stats"]["extra"] = json!(0),
        |diff| diff["edits"][0]["old_lines"][0]["text"] = json!(1),
        |diff| {
            drop(
                diff["stats"]
                    .as_object_mut()
                    .expect("stats")
                    .remove("inserts"),
            )
        },
    ];
    for break_diff in breaks {
        let mut broken = diff.clone();
        break_diff(&mut broken);
        let errors = schemas.errors("diff.schema.json", &broken);
        assert!(!errors.is_empty(), "accepted {broken}");
    }
}
