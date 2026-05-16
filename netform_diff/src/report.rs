use std::fmt::Write;

use owo_colors::OwoColorize;

use crate::model::{Diff, DiffLine, Edit};

/// Default maximum number of lines shown per side of an edit before truncating.
pub const DEFAULT_CONTEXT_LINES: usize = 10;

/// Format a markdown-oriented human report from a diff result.
///
/// `max_lines_shown` controls how many lines are printed per side of each edit
/// before the output is truncated with an "and N more" message.
pub fn format_markdown_report(
    diff: &Diff,
    left_label: &str,
    right_label: &str,
    max_lines_shown: usize,
) -> String {
    let mut out = String::new();
    out.push_str("# Config Diff Report\n\n");
    writeln!(out, "- Left: `{left_label}`").unwrap();
    writeln!(out, "- Right: `{right_label}`\n").unwrap();

    out.push_str("## Stats\n\n");
    writeln!(
        out,
        "- Inserts: {} ({} lines)",
        diff.stats.inserts, diff.stats.inserted_lines
    )
    .unwrap();
    writeln!(
        out,
        "- Deletes: {} ({} lines)",
        diff.stats.deletes, diff.stats.deleted_lines
    )
    .unwrap();
    writeln!(
        out,
        "- Replaces: {} ({} -> {} lines)\n",
        diff.stats.replaces, diff.stats.replaced_old_lines, diff.stats.replaced_new_lines
    )
    .unwrap();

    out.push_str("## Edits\n\n");
    if diff.edits.is_empty() {
        out.push_str("No changes detected.\n");
    } else {
        for (idx, edit) in diff.edits.iter().enumerate() {
            writeln!(out, "{}. {}", idx + 1, describe_edit(edit, max_lines_shown)).unwrap();
        }
    }

    if !diff.findings.is_empty() {
        out.push_str("\n## Findings\n\n");
        for finding in &diff.findings {
            writeln!(
                out,
                "- {} [{}]: {}",
                finding.level, finding.code, finding.message
            )
            .unwrap();
        }
    }

    out
}

/// Format a colored unified-diff-style report from a diff result.
///
/// Uses ANSI colors when enabled via `owo_colors`:
/// - `---`/`+++` file headers: bold
/// - `@@` hunk headers: cyan
/// - `-` deletion lines: red
/// - `+` insertion lines: green
///
/// Call `owo_colors::set_override(false)` before invoking this function
/// to suppress ANSI escapes (e.g. when stdout is not a TTY).
pub fn format_unified_diff(
    diff: &Diff,
    left_label: &str,
    right_label: &str,
    max_lines_shown: usize,
) -> String {
    let mut out = String::new();
    if diff.edits.is_empty() {
        return out;
    }

    writeln!(out, "{}", format_args!("--- {left_label}").bold()).unwrap();
    writeln!(out, "{}", format_args!("+++ {right_label}").bold()).unwrap();

    for edit in &diff.edits {
        match edit {
            Edit::Insert { at_key, lines, .. } => {
                writeln!(
                    out,
                    "{}",
                    format_args!(
                        "@@ insert {} line(s) at key {} @@",
                        lines.len(),
                        crate::util::key_label(*at_key),
                    )
                    .cyan()
                )
                .unwrap();
                append_colored_lines(&mut out, "+", lines, max_lines_shown);
            }
            Edit::Delete { at_key, lines, .. } => {
                writeln!(
                    out,
                    "{}",
                    format_args!(
                        "@@ delete {} line(s) at key {} @@",
                        lines.len(),
                        crate::util::key_label(*at_key),
                    )
                    .cyan()
                )
                .unwrap();
                append_colored_lines(&mut out, "-", lines, max_lines_shown);
            }
            Edit::Replace {
                old_at_key,
                new_at_key,
                old_lines,
                new_lines,
                ..
            } => {
                writeln!(
                    out,
                    "{}",
                    format_args!(
                        "@@ replace {} line(s) at key {} -> {} line(s) at key {} @@",
                        old_lines.len(),
                        crate::util::key_label(*old_at_key),
                        new_lines.len(),
                        crate::util::key_label(*new_at_key),
                    )
                    .cyan()
                )
                .unwrap();
                append_colored_lines(&mut out, "-", old_lines, max_lines_shown);
                append_colored_lines(&mut out, "+", new_lines, max_lines_shown);
            }
        }
    }

    if !diff.findings.is_empty() {
        out.push('\n');
        for finding in &diff.findings {
            writeln!(
                out,
                "{}",
                format_args!("{} [{}]: {}", finding.level, finding.code, finding.message).yellow()
            )
            .unwrap();
        }
    }

    out
}

fn append_colored_lines(
    out: &mut String,
    prefix: &str,
    lines: &[DiffLine],
    max_lines_shown: usize,
) {
    let show = lines.len().min(max_lines_shown);
    for line in &lines[..show] {
        let formatted = format!("{prefix} {}", line.text);
        if prefix == "+" {
            writeln!(out, "{}", formatted.green()).unwrap();
        } else {
            writeln!(out, "{}", formatted.red()).unwrap();
        }
    }
    let remaining = lines.len().saturating_sub(max_lines_shown);
    if remaining > 0 {
        writeln!(
            out,
            "{}",
            format_args!("{prefix} ... and {remaining} more").dimmed()
        )
        .unwrap();
    }
}

fn describe_edit(edit: &Edit, max_lines_shown: usize) -> String {
    let mut out = String::new();
    match edit {
        Edit::Insert { at_key, lines, .. } => {
            write!(
                out,
                "Insert {} line(s) at key {}",
                lines.len(),
                crate::util::key_label(*at_key),
            )
            .unwrap();
            append_diff_lines(&mut out, "+", lines, max_lines_shown);
        }
        Edit::Delete { at_key, lines, .. } => {
            write!(
                out,
                "Delete {} line(s) at key {}",
                lines.len(),
                crate::util::key_label(*at_key),
            )
            .unwrap();
            append_diff_lines(&mut out, "-", lines, max_lines_shown);
        }
        Edit::Replace {
            old_at_key,
            new_at_key,
            old_lines,
            new_lines,
            ..
        } => {
            write!(
                out,
                "Replace {} line(s) at key {} with {} line(s) at key {}",
                old_lines.len(),
                crate::util::key_label(*old_at_key),
                new_lines.len(),
                crate::util::key_label(*new_at_key),
            )
            .unwrap();
            append_diff_lines(&mut out, "-", old_lines, max_lines_shown);
            append_diff_lines(&mut out, "+", new_lines, max_lines_shown);
        }
    }
    out
}

fn append_diff_lines(out: &mut String, prefix: &str, lines: &[DiffLine], max_lines_shown: usize) {
    let show = lines.len().min(max_lines_shown);
    for line in &lines[..show] {
        write!(out, "\n   {prefix} {}", line.text).unwrap();
    }
    let remaining = lines.len().saturating_sub(max_lines_shown);
    if remaining > 0 {
        write!(out, "\n   {prefix} ... and {remaining} more").unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DiffStats, Finding, FindingLevel};
    use netform_ir::{Path, Span};

    fn make_diff_line(text: &str) -> DiffLine {
        DiffLine {
            content_key: 0,
            occurrence_key: 0,
            text: text.to_string(),
            path: Path(vec![0]),
            span: Span {
                line: 0,
                start_byte: 0,
                end_byte: 0,
            },
        }
    }

    fn empty_diff() -> Diff {
        Diff::default()
    }

    fn insert_edit(lines: &[&str]) -> Edit {
        Edit::Insert {
            at_key: Some(42),
            left_anchor: None,
            right_anchor: None,
            lines: lines.iter().map(|t| make_diff_line(t)).collect(),
        }
    }

    fn delete_edit(lines: &[&str]) -> Edit {
        Edit::Delete {
            at_key: Some(99),
            left_anchor: None,
            right_anchor: None,
            lines: lines.iter().map(|t| make_diff_line(t)).collect(),
        }
    }

    fn replace_edit(old: &[&str], new: &[&str]) -> Edit {
        Edit::Replace {
            old_at_key: Some(10),
            new_at_key: Some(20),
            left_anchor: None,
            right_anchor: None,
            old_lines: old.iter().map(|t| make_diff_line(t)).collect(),
            new_lines: new.iter().map(|t| make_diff_line(t)).collect(),
        }
    }

    fn make_finding(level: FindingLevel, code: &str, message: &str) -> Finding {
        Finding {
            code: code.to_string(),
            level,
            message: message.to_string(),
            path: None,
            span: None,
        }
    }

    // --- format_markdown_report ---

    #[test]
    fn markdown_empty_diff_shows_no_changes() {
        let diff = empty_diff();
        let report = format_markdown_report(&diff, "left.cfg", "right.cfg", 10);

        assert!(report.contains("# Config Diff Report"));
        assert!(report.contains("- Left: `left.cfg`"));
        assert!(report.contains("- Right: `right.cfg`"));
        assert!(report.contains("- Inserts: 0 (0 lines)"));
        assert!(report.contains("- Deletes: 0 (0 lines)"));
        assert!(report.contains("- Replaces: 0 (0 -> 0 lines)"));
        assert!(report.contains("No changes detected."));
        assert!(!report.contains("## Findings"));
    }

    #[test]
    fn markdown_insert_edit() {
        let diff = Diff {
            edits: vec![insert_edit(&["permit any"])],
            stats: DiffStats {
                inserts: 1,
                inserted_lines: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("- Inserts: 1 (1 lines)"));
        assert!(report.contains("1. Insert 1 line(s) at key 0x000000000000002a"));
        assert!(report.contains("+ permit any"));
        assert!(!report.contains("No changes detected."));
    }

    #[test]
    fn markdown_delete_edit() {
        let diff = Diff {
            edits: vec![delete_edit(&["deny all"])],
            stats: DiffStats {
                deletes: 1,
                deleted_lines: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("- Deletes: 1 (1 lines)"));
        assert!(report.contains("1. Delete 1 line(s) at key 0x0000000000000063"));
        assert!(report.contains("- deny all"));
    }

    #[test]
    fn markdown_replace_edit() {
        let diff = Diff {
            edits: vec![replace_edit(&["old line"], &["new line"])],
            stats: DiffStats {
                replaces: 1,
                replaced_old_lines: 1,
                replaced_new_lines: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("- Replaces: 1 (1 -> 1 lines)"));
        assert!(report.contains(
            "Replace 1 line(s) at key 0x000000000000000a with 1 line(s) at key 0x0000000000000014"
        ));
        assert!(report.contains("- old line"));
        assert!(report.contains("+ new line"));
    }

    #[test]
    fn markdown_multiple_edits_numbered() {
        let diff = Diff {
            edits: vec![insert_edit(&["line a"]), delete_edit(&["line b"])],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("1. Insert"));
        assert!(report.contains("2. Delete"));
    }

    #[test]
    fn markdown_truncation_with_max_lines() {
        let lines: Vec<&str> = (0..5).map(|_| "line").collect();
        let diff = Diff {
            edits: vec![insert_edit(&lines)],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 2);

        // Should show 2 lines then truncation message
        assert!(report.contains("... and 3 more"));
    }

    #[test]
    fn markdown_no_truncation_when_within_limit() {
        let diff = Diff {
            edits: vec![insert_edit(&["line1", "line2"])],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 5);

        assert!(!report.contains("... and"));
        assert!(report.contains("+ line1"));
        assert!(report.contains("+ line2"));
    }

    #[test]
    fn markdown_findings_section() {
        let diff = Diff {
            findings: vec![
                make_finding(FindingLevel::Warning, "missing_anchor", "anchor not found"),
                make_finding(
                    FindingLevel::Info,
                    "ambiguous_key_match",
                    "multiple matches",
                ),
            ],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("## Findings"));
        assert!(report.contains("- warning [missing_anchor]: anchor not found"));
        assert!(report.contains("- info [ambiguous_key_match]: multiple matches"));
    }

    #[test]
    fn markdown_no_findings_section_when_empty() {
        let diff = empty_diff();
        let report = format_markdown_report(&diff, "a", "b", 10);
        assert!(!report.contains("## Findings"));
    }

    #[test]
    fn markdown_insert_with_none_key() {
        let diff = Diff {
            edits: vec![Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: None,
                lines: vec![make_diff_line("new line")],
            }],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);
        assert!(report.contains("at key <unknown>"));
    }

    // --- format_unified_diff ---

    #[test]
    fn unified_empty_diff_returns_empty_string() {
        let diff = empty_diff();
        let result = format_unified_diff(&diff, "a", "b", 10);
        assert!(result.is_empty());
    }

    #[test]
    fn unified_insert_edit() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![insert_edit(&["permit any"])],
            ..Default::default()
        };
        let result = format_unified_diff(&diff, "left.cfg", "right.cfg", 10);

        assert!(result.contains("--- left.cfg"));
        assert!(result.contains("+++ right.cfg"));
        assert!(result.contains("@@ insert 1 line(s) at key 0x000000000000002a @@"));
        assert!(result.contains("+ permit any"));
    }

    #[test]
    fn unified_delete_edit() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![delete_edit(&["deny all"])],
            ..Default::default()
        };
        let result = format_unified_diff(&diff, "a", "b", 10);

        assert!(result.contains("@@ delete 1 line(s) at key 0x0000000000000063 @@"));
        assert!(result.contains("- deny all"));
    }

    #[test]
    fn unified_replace_edit() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![replace_edit(&["old"], &["new"])],
            ..Default::default()
        };
        let result = format_unified_diff(&diff, "a", "b", 10);

        assert!(result.contains("@@ replace 1 line(s) at key"));
        assert!(result.contains("- old"));
        assert!(result.contains("+ new"));
    }

    #[test]
    fn unified_truncation() {
        owo_colors::set_override(false);
        let lines: Vec<&str> = (0..5).map(|_| "line").collect();
        let diff = Diff {
            edits: vec![insert_edit(&lines)],
            ..Default::default()
        };
        let result = format_unified_diff(&diff, "a", "b", 2);

        assert!(result.contains("... and 3 more"));
    }

    #[test]
    fn unified_no_truncation_within_limit() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![insert_edit(&["a", "b"])],
            ..Default::default()
        };
        let result = format_unified_diff(&diff, "a", "b", 5);

        assert!(!result.contains("... and"));
    }

    #[test]
    fn unified_findings_appended() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![insert_edit(&["x"])],
            findings: vec![make_finding(
                FindingLevel::Warning,
                "test_code",
                "something happened",
            )],
            ..Default::default()
        };
        let result = format_unified_diff(&diff, "a", "b", 10);

        assert!(result.contains("warning [test_code]: something happened"));
    }

    #[test]
    fn unified_no_findings_when_empty() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![insert_edit(&["x"])],
            ..Default::default()
        };
        let result = format_unified_diff(&diff, "a", "b", 10);

        // Should not have the extra newline that precedes findings
        let lines: Vec<&str> = result.lines().collect();
        let last = lines.last().unwrap();
        assert!(!last.is_empty()); // no trailing blank from findings block
    }

    #[test]
    fn unified_max_lines_exactly_at_boundary() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![insert_edit(&["a", "b", "c"])],
            ..Default::default()
        };
        // max_lines_shown == lines.len(): all shown, no truncation
        let result = format_unified_diff(&diff, "a", "b", 3);
        assert!(!result.contains("... and"));
        assert!(result.contains("+ a"));
        assert!(result.contains("+ b"));
        assert!(result.contains("+ c"));
    }

    #[test]
    fn unified_max_lines_one_over_boundary() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![insert_edit(&["a", "b", "c"])],
            ..Default::default()
        };
        // max_lines_shown == lines.len() - 1: should truncate with "and 1 more"
        let result = format_unified_diff(&diff, "a", "b", 2);
        assert!(result.contains("... and 1 more"));
    }

    // --- describe_edit (tested through format_markdown_report) ---

    #[test]
    fn markdown_replace_truncation_both_sides() {
        let old: Vec<&str> = (0..4).map(|_| "old").collect();
        let new: Vec<&str> = (0..6).map(|_| "new").collect();
        let diff = Diff {
            edits: vec![replace_edit(&old, &new)],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 2);

        assert!(report.contains("- ... and 2 more"));
        assert!(report.contains("+ ... and 4 more"));
    }

    // --- DEFAULT_CONTEXT_LINES ---

    #[test]
    fn default_context_lines_is_ten() {
        assert_eq!(DEFAULT_CONTEXT_LINES, 10);
    }
}
