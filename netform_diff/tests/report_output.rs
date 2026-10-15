use netform_diff::{
    DEFAULT_CONTEXT_LINES, NormalizeOptions, diff_documents, format_markdown_report,
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
    // inline diff wraps changed tokens in **bold** markers
    assert!(
        report.contains("description **old**"),
        "report should show removed line text with inline highlighting"
    );
    assert!(
        report.contains("description **new**"),
        "report should show added line text with inline highlighting"
    );
    // verify line numbers are present in diff lines
    assert!(
        report.contains("- `-` L"),
        "removed lines should include source line number"
    );
    assert!(
        report.contains("- `+` L"),
        "added lines should include source line number"
    );
}

/// the rendered `## Edits` section with the hash keys masked.
fn edits_section(report: &str) -> String {
    let start = report.find("## Edits\n\n").expect("edits section") + "## Edits\n\n".len();
    let end = report[start..]
        .find("\n\n")
        .map_or(report.len(), |end| start + end);
    let mut out = String::new();
    let mut rest = &report[start..end];
    while let Some(at) = rest.find("0x") {
        out.push_str(&rest[..at + 2]);
        out.push('#');
        rest = rest[at + 2..].trim_start_matches(|c: char| c.is_ascii_hexdigit());
    }
    out.push_str(rest);
    out
}

#[test]
fn markdown_report_renders_each_edit_kind() {
    for (left, right, expected) in [
        // a child added to a block replaces the block
        (
            "interface Ethernet1\n",
            "interface Ethernet1\n  description added\n",
            "1. Replace 1 line(s) at key 0x# with 2 line(s) at key 0x#\n   - `-` L1: interface Ethernet1\n   - `+` L1: interface Ethernet1\n   - `+` L2:   description added",
        ),
        // a child removed from a block replaces the block
        (
            "interface Ethernet1\n  description removed\n",
            "interface Ethernet1\n",
            "1. Replace 2 line(s) at key 0x# with 1 line(s) at key 0x#\n   - `-` L1: interface Ethernet1\n   - `-` L2:   description removed\n   - `+` L1: interface Ethernet1",
        ),
        (
            "",
            "hostname new\n",
            "1. Insert 1 line(s) at key 0x#\n   - `+` L1: hostname new",
        ),
        (
            "hostname old\n",
            "",
            "1. Delete 1 line(s) at key 0x#\n   - `-` L1: hostname old",
        ),
    ] {
        let diff = diff_documents(
            &parse_generic(left),
            &parse_generic(right),
            NormalizeOptions::default(),
        )
        .unwrap();
        let report = format_markdown_report(&diff, "left.cfg", "right.cfg", DEFAULT_CONTEXT_LINES);
        assert_eq!(edits_section(&report), expected, "{left:?} -> {right:?}");
    }
}
