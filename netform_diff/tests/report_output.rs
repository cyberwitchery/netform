use netform_diff::{NormalizeOptions, diff_documents, format_markdown_report};
use netform_ir::parse_generic;

#[test]
fn markdown_report_mentions_keyed_replace() {
    let a = parse_generic("interface Ethernet1\n  description old\n");
    let b = parse_generic("interface Ethernet1\n  description new\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    let report = format_markdown_report(&diff, "left.cfg", "right.cfg");

    assert!(diff.has_changes);
    assert!(report.contains("# Config Diff Report"));
    assert!(report.contains("Replaces: 1 (1 -> 1 lines)"));
    assert!(report.contains("Replace 1 line(s) at key 0x"));
}

#[test]
fn json_output_is_stable_shape() {
    let a = parse_generic("set system host-name a\n");
    let b = parse_generic("set system host-name b\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    let json = serde_json::to_string_pretty(&diff).expect("serialize diff");

    assert!(diff.has_changes);
    assert!(json.contains("\"edits\""));
    assert!(json.contains("\"has_changes\": true"));
    assert!(json.contains("\"stats\""));
    assert!(json.contains("\"old_at_key\""));
    assert!(json.contains("\"occurrence_key\""));
}
