use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use netform_diff::{
    Edit, Finding, FindingLevel, NormalizationStep, OrderPolicy, PlanFinding, PlanLineEditKind,
    finding_code,
};
use netform_ir::{Path, Span};
use serde::Serialize;
use serde_json::Value;

/// every variant of a fieldless enum; leaving one out fails to compile.
macro_rules! every_variant {
    ($enum:ident { $($variant:ident),* $(,)? }) => {{
        let _exhaustive = |value: $enum| match value {
            $($enum::$variant => ()),*
        };
        [$($enum::$variant),*]
    }};
}

fn schemas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../schemas")
}

fn schema(name: &str) -> Value {
    let path = schemas_dir().join(name);
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn strings(value: &Value) -> BTreeSet<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

fn schema_enum(name: &str, pointer: &str) -> BTreeSet<String> {
    let document = schema(name);
    let values = document
        .pointer(pointer)
        .unwrap_or_else(|| panic!("{name} has no {pointer}"));
    strings(values)
}

fn serde_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value).expect("serialize") {
        Value::String(name) => name,
        Value::Object(fields) => fields["type"].as_str().expect("type tag").to_string(),
        other => panic!("unexpected serialization: {other}"),
    }
}

fn serde_names<T: Serialize>(values: &[T]) -> BTreeSet<String> {
    values.iter().map(serde_name).collect()
}

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().expect("object").keys().cloned().collect()
}

fn set_of(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn fixture_dialects_are_generic_and_every_registered_vendor() {
    let mut expected = set_of(&["generic"]);
    expected.extend(netform_dialects::names().map(String::from));

    assert_eq!(
        schema_enum("fixture.schema.json", "/properties/dialect/enum"),
        expected
    );
}

#[test]
fn normalization_steps_match_their_serde_names() {
    let steps = every_variant!(NormalizationStep {
        IgnoreComments,
        IgnoreBlankLines,
        TrimTrailingWhitespace,
        NormalizeLeadingWhitespace,
        CollapseInternalWhitespace,
    });

    assert_eq!(
        schema_enum("normalization-pipeline.schema.json", "/$defs/step/enum"),
        serde_names(&steps)
    );
}

#[test]
fn order_policy_modes_match_their_serde_names() {
    let modes = every_variant!(OrderPolicy {
        Ordered,
        Unordered,
        KeyedStable
    });

    assert_eq!(
        schema_enum("order-policy.schema.json", "/$defs/mode/enum"),
        serde_names(&modes)
    );
}

#[test]
fn finding_levels_match_their_serde_names() {
    let levels = every_variant!(FindingLevel { Warning, Info });

    assert_eq!(
        schema_enum("diff.schema.json", "/$defs/finding_level/enum"),
        serde_names(&levels)
    );
}

#[test]
fn finding_codes_match_the_codes_each_output_emits() {
    assert_eq!(
        schema_enum("diff.schema.json", "/$defs/finding_code/enum"),
        set_of(&[
            finding_code::AMBIGUOUS_KEY_MATCH,
            finding_code::UNKNOWN_UNPARSED_CONSTRUCT,
            finding_code::DIFF_UNRELIABLE_REGION,
        ])
    );
    assert_eq!(
        schema_enum("plan.schema.json", "/$defs/finding_code/enum"),
        set_of(&[finding_code::MISSING_ANCHOR])
    );
}

#[test]
fn fixture_edit_types_match_the_edit_tags() {
    let edits = [
        Edit::Insert {
            at_key: None,
            left_anchor: None,
            right_anchor: None,
            lines: Vec::new(),
        },
        Edit::Delete {
            at_key: None,
            left_anchor: None,
            right_anchor: None,
            lines: Vec::new(),
        },
        Edit::Replace {
            old_at_key: None,
            new_at_key: None,
            left_anchor: None,
            right_anchor: None,
            old_lines: Vec::new(),
            new_lines: Vec::new(),
        },
    ];
    for edit in &edits {
        match edit {
            Edit::Insert { .. } | Edit::Delete { .. } | Edit::Replace { .. } => {}
        }
    }

    assert_eq!(
        schema_enum(
            "fixture.schema.json",
            "/properties/expected/properties/edit_types/items/enum"
        ),
        serde_names(&edits)
    );
}

#[test]
fn plan_line_edit_kinds_match_their_serde_names() {
    let kinds = every_variant!(PlanLineEditKind {
        Insert,
        Delete,
        Replace
    });

    assert_eq!(
        schema_enum(
            "plan.schema.json",
            "/$defs/apply_line_edits_action/properties/line_edits/items/properties/kind/enum"
        ),
        serde_names(&kinds)
    );
}

#[test]
fn finding_fields_match_what_each_finding_serializes() {
    let span = Span {
        line: 1,
        start_byte: 0,
        end_byte: 4,
    };
    let finding = Finding {
        code: finding_code::AMBIGUOUS_KEY_MATCH.to_string(),
        level: FindingLevel::Warning,
        message: String::new(),
        path: None,
        span: None,
    };
    let bare = PlanFinding {
        code: finding_code::MISSING_ANCHOR.to_string(),
        message: String::new(),
        level: None,
        path: None,
        span: None,
    };
    let full = PlanFinding {
        level: Some(FindingLevel::Warning),
        path: Some(Path(vec![0])),
        span: Some(span),
        ..bare.clone()
    };

    let finding = keys(&serde_json::to_value(&finding).expect("serialize"));
    let diff = &schema("diff.schema.json")["$defs"]["finding"];
    assert_eq!(keys(&diff["properties"]), finding);
    assert_eq!(strings(&diff["required"]), finding);

    let bare = keys(&serde_json::to_value(&bare).expect("serialize"));
    let full = keys(&serde_json::to_value(&full).expect("serialize"));
    let plan = &schema("plan.schema.json")["$defs"]["finding"];
    assert_eq!(keys(&plan["properties"]), full);
    assert_eq!(strings(&plan["required"]), bare);
}

fn collect_refs<'a>(value: &'a Value, refs: &mut Vec<&'a str>) {
    match value {
        Value::Object(fields) => {
            if let Some(Value::String(reference)) = fields.get("$ref") {
                refs.push(reference);
            }
            fields.values().for_each(|field| collect_refs(field, refs));
        }
        Value::Array(items) => items.iter().for_each(|item| collect_refs(item, refs)),
        _ => {}
    }
}

#[test]
fn every_schema_ref_resolves() {
    let mut checked = 0;
    for entry in fs::read_dir(schemas_dir()).expect("read schemas/") {
        let name = entry.expect("dir entry").file_name();
        let name = name.to_str().expect("utf-8 file name");
        let document = schema(name);
        let mut refs = Vec::new();
        collect_refs(&document, &mut refs);
        for reference in refs {
            let (file, pointer) = reference.split_once('#').unwrap_or((reference, ""));
            let target = if file.is_empty() {
                document.clone()
            } else {
                schema(file)
            };
            assert!(
                target.pointer(pointer).is_some(),
                "{name}: unresolved $ref {reference}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0);
}
