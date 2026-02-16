use std::path::Path;

use netform_diff::{NormalizeOptions, diff_documents};
use netform_ir::parse_generic;
use serde_json::Value;

#[test]
fn schema_files_exist_at_repo_root() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");

    let required = [
        "schemas/diff.schema.json",
        "schemas/plan.schema.json",
        "schemas/normalization-pipeline.schema.json",
        "schemas/order-policy.schema.json",
        "schemas/fixture.schema.json",
    ];

    for relative in required {
        let path = repo_root.join(relative);
        assert!(path.exists(), "missing schema file: {}", path.display());
    }
}

#[test]
fn diff_json_shape_contract() {
    let intended = parse_generic("interface Ethernet1\n  description intended\n");
    let actual = parse_generic("interface Ethernet1\n  description actual\n");

    let diff = diff_documents(&intended, &actual, NormalizeOptions::default());
    let value = serde_json::to_value(&diff).expect("serialize diff");

    let obj = value.as_object().expect("diff should be object");
    assert!(obj.contains_key("normalization_steps"));
    assert!(obj.contains_key("order_policy"));
    assert!(obj.contains_key("has_changes"));
    assert!(obj.contains_key("edits"));
    assert!(obj.contains_key("stats"));
    assert!(obj.contains_key("findings"));
    let findings = obj
        .get("findings")
        .and_then(Value::as_array)
        .expect("findings should be array");
    for finding in findings {
        let finding_obj = finding.as_object().expect("finding object");
        assert!(finding_obj.contains_key("code"));
    }

    let edits = obj
        .get("edits")
        .and_then(Value::as_array)
        .expect("edits should be array");
    assert!(!edits.is_empty());

    for edit in edits {
        let edit_obj = edit.as_object().expect("edit should be object");
        assert!(edit_obj.contains_key("type"));
        assert!(!edit_obj.contains_key("confidence"));

        match edit_obj
            .get("type")
            .and_then(Value::as_str)
            .expect("type string")
        {
            "Insert" => {
                assert!(edit_obj.contains_key("at_key"));
                assert!(edit_obj.contains_key("left_anchor"));
                assert!(edit_obj.contains_key("right_anchor"));
                assert!(edit_obj.contains_key("lines"));
            }
            "Delete" => {
                assert!(edit_obj.contains_key("at_key"));
                assert!(edit_obj.contains_key("left_anchor"));
                assert!(edit_obj.contains_key("right_anchor"));
                assert!(edit_obj.contains_key("lines"));
            }
            "Replace" => {
                assert!(edit_obj.contains_key("old_at_key"));
                assert!(edit_obj.contains_key("new_at_key"));
                assert!(edit_obj.contains_key("left_anchor"));
                assert!(edit_obj.contains_key("right_anchor"));
                assert!(edit_obj.contains_key("old_lines"));
                assert!(edit_obj.contains_key("new_lines"));
            }
            other => panic!("unexpected edit type: {other}"),
        }
    }
}

#[test]
fn diff_json_is_byte_stable_across_runs() {
    let intended = parse_generic("line a\nline b\nline c\n");
    let actual = parse_generic("line a\nline x\nline c\n");

    let one = diff_documents(&intended, &actual, NormalizeOptions::default());
    let two = diff_documents(&intended, &actual, NormalizeOptions::default());

    let one_json = serde_json::to_string_pretty(&one).expect("serialize first");
    let two_json = serde_json::to_string_pretty(&two).expect("serialize second");

    assert_eq!(one_json, two_json);
}
