use std::collections::BTreeSet;
use std::path::Path;

use netform_diff::{NormalizeOptions, diff_documents};
use netform_ir::parse_generic;
use serde_json::Value;

fn diff_schema() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../schemas/diff.schema.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

fn keys(value: &Value) -> BTreeSet<String> {
    value.as_object().expect("object").keys().cloned().collect()
}

fn required(value: &Value) -> BTreeSet<String> {
    value["required"]
        .as_array()
        .expect("required array")
        .iter()
        .map(|item| item.as_str().expect("string").to_string())
        .collect()
}

/// diff.json carries exactly the fields diff.schema.json declares, at every
/// level, for a nested and a top-level value change.
#[test]
fn diff_json_carries_exactly_the_schema_fields() {
    let schema = diff_schema();
    let defs = &schema["$defs"];
    for (intended, actual) in [
        (
            "interface Ethernet1\n  description intended\n",
            "interface Ethernet1\n  description actual\n",
        ),
        ("set system host-name a\n", "set system host-name b\n"),
    ] {
        let diff = diff_documents(
            &parse_generic(intended),
            &parse_generic(actual),
            NormalizeOptions::default(),
        )
        .unwrap();
        let value = serde_json::to_value(&diff).expect("serialize diff");

        assert_eq!(keys(&value), keys(&schema["properties"]), "{intended:?}");
        assert_eq!(keys(&value), required(&schema), "{intended:?}");
        assert_eq!(value["has_changes"], Value::Bool(true), "{intended:?}");
        assert_eq!(
            keys(&value["stats"]),
            required(&defs["stats"]),
            "{intended:?}"
        );

        let edits = value["edits"].as_array().expect("edits array");
        assert_eq!(edits.len(), 1, "{intended:?}");
        for edit in edits {
            assert_eq!(edit["type"], "Replace", "{intended:?}");
            let def = &defs["replace_edit"];
            assert_eq!(keys(edit), keys(&def["properties"]), "{intended:?}");
            assert_eq!(keys(edit), required(def), "{intended:?}");
            for side in ["old_lines", "new_lines"] {
                for line in edit[side].as_array().expect("lines array") {
                    assert_eq!(keys(line), required(&defs["diff_line"]), "{intended:?}");
                }
            }
        }
        for finding in value["findings"].as_array().expect("findings array") {
            assert_eq!(keys(finding), required(&defs["finding"]), "{intended:?}");
        }
    }
}
