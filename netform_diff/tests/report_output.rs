use netform_diff::{
    DEFAULT_CONTEXT_LINES, NormalizeOptions, diff_documents, format_markdown_report,
    format_unified_diff,
};
use netform_ir::parse_generic;

#[test]
fn markdown_report_mentions_keyed_replace() {
    let a = parse_generic("interface Ethernet1\n  description old\n");
    let b = parse_generic("interface Ethernet1\n  description new\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "left.cfg", "right.cfg", DEFAULT_CONTEXT_LINES);

    assert!(diff.has_changes);
    assert!(report.contains("# Config Diff Report"));
    assert!(report.contains("Replaces: 1 (1 -> 1 lines)"));
    assert!(report.contains("Replace 1 line(s) at key 0x"));
    // line content is now shown under each edit, prefixed with source line number
    assert!(
        report.contains("description old"),
        "report should show removed line text"
    );
    assert!(
        report.contains("description new"),
        "report should show added line text"
    );
    // verify line numbers are present in diff lines
    assert!(
        report.contains("- L"),
        "removed lines should include source line number"
    );
    assert!(
        report.contains("+ L"),
        "added lines should include source line number"
    );
}

#[test]
fn markdown_report_shows_added_line_in_replace() {
    // adding a child line to a block produces a Replace at the block level
    let a = parse_generic("interface Ethernet1\n");
    let b = parse_generic("interface Ethernet1\n  description added\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "left.cfg", "right.cfg", DEFAULT_CONTEXT_LINES);

    assert!(diff.has_changes);
    assert!(
        report.contains("description added"),
        "report should show the added line text:\n{}",
        report
    );
}

#[test]
fn markdown_report_shows_removed_line_in_replace() {
    // removing a child line from a block produces a Replace at the block level
    let a = parse_generic("interface Ethernet1\n  description removed\n");
    let b = parse_generic("interface Ethernet1\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "left.cfg", "right.cfg", DEFAULT_CONTEXT_LINES);

    assert!(diff.has_changes);
    assert!(
        report.contains("description removed"),
        "report should show the removed line text:\n{}",
        report
    );
}

// -- format_markdown_report unit tests --

#[test]
fn markdown_report_no_changes_says_no_changes() {
    let a = parse_generic("hostname router\n");
    let diff = diff_documents(&a, &a, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "same.cfg", "same.cfg", DEFAULT_CONTEXT_LINES);

    assert!(report.contains("# Config Diff Report"));
    assert!(report.contains("No changes detected."));
    assert!(report.contains("Inserts: 0 (0 lines)"));
    assert!(report.contains("Deletes: 0 (0 lines)"));
    assert!(report.contains("Replaces: 0 (0 -> 0 lines)"));
}

#[test]
fn markdown_report_shows_labels() {
    let a = parse_generic("hostname old\n");
    let b = parse_generic("hostname new\n");
    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "before.cfg", "after.cfg", DEFAULT_CONTEXT_LINES);

    assert!(report.contains("- Left: `before.cfg`"));
    assert!(report.contains("- Right: `after.cfg`"));
}

#[test]
fn markdown_report_insert_shows_added_lines() {
    let a = parse_generic("");
    let b = parse_generic("hostname new\n");
    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "empty.cfg", "new.cfg", DEFAULT_CONTEXT_LINES);

    assert!(diff.has_changes);
    assert!(report.contains("Insert"));
    assert!(report.contains("hostname new"));
}

#[test]
fn markdown_report_delete_shows_removed_lines() {
    let a = parse_generic("hostname old\n");
    let b = parse_generic("");
    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "old.cfg", "empty.cfg", DEFAULT_CONTEXT_LINES);

    assert!(diff.has_changes);
    assert!(report.contains("Delete"));
    assert!(report.contains("hostname old"));
}

#[test]
fn markdown_report_truncates_long_edits() {
    // create a diff with many lines to test truncation
    let a_lines: Vec<String> = (0..20).map(|i| format!("line-a-{i}")).collect();
    let b_lines: Vec<String> = (0..20).map(|i| format!("line-b-{i}")).collect();
    let a = parse_generic(&a_lines.join("\n"));
    let b = parse_generic(&b_lines.join("\n"));
    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();

    // use max_lines_shown=3 to force truncation
    let report = format_markdown_report(&diff, "a.cfg", "b.cfg", 3);

    assert!(diff.has_changes);
    assert!(
        report.contains("more"),
        "report should truncate with 'more' message:\n{report}"
    );
}

#[test]
fn markdown_report_stats_are_correct_for_multiple_edits() {
    let a = parse_generic("line1\nline2\nline3\n");
    let b = parse_generic("line1\nchanged\nline3\nnew-line\n");
    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let report = format_markdown_report(&diff, "a.cfg", "b.cfg", DEFAULT_CONTEXT_LINES);

    assert!(diff.has_changes);
    // the report should have the Stats section with non-zero values
    assert!(report.contains("## Stats"));
    assert!(report.contains("## Edits"));
}

// -- format_unified_diff tests --

#[test]
fn unified_diff_empty_for_no_changes() {
    let a = parse_generic("hostname router\n");
    let diff = diff_documents(&a, &a, NormalizeOptions::default()).unwrap();
    // suppress ANSI colors for test
    owo_colors::set_override(false);
    let output = format_unified_diff(&diff, "a.cfg", "a.cfg", DEFAULT_CONTEXT_LINES);
    owo_colors::set_override(true);

    assert!(
        output.is_empty(),
        "unified diff should be empty when no changes"
    );
}

#[test]
fn unified_diff_shows_headers_and_hunks() {
    let a = parse_generic("hostname old\n");
    let b = parse_generic("hostname new\n");
    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    owo_colors::set_override(false);
    let output = format_unified_diff(&diff, "left.cfg", "right.cfg", DEFAULT_CONTEXT_LINES);
    owo_colors::set_override(true);

    assert!(output.contains("--- left.cfg"), "should contain --- header");
    assert!(
        output.contains("+++ right.cfg"),
        "should contain +++ header"
    );
    assert!(output.contains("@@"), "should contain @@ hunk header");
    assert!(
        output.contains("- hostname old"),
        "should contain deleted line"
    );
    assert!(
        output.contains("+ hostname new"),
        "should contain inserted line"
    );
}

#[test]
fn json_output_is_stable_shape() {
    let a = parse_generic("set system host-name a\n");
    let b = parse_generic("set system host-name b\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let json = serde_json::to_string_pretty(&diff).expect("serialize diff");

    assert!(diff.has_changes);
    assert!(json.contains("\"edits\""));
    assert!(json.contains("\"has_changes\": true"));
    assert!(json.contains("\"stats\""));
    assert!(json.contains("\"old_at_key\""));
    assert!(json.contains("\"occurrence_key\""));
}
