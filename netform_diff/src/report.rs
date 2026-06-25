use std::fmt::Write;

use owo_colors::OwoColorize;

use crate::inline::TokenSpan;
use crate::model::{Diff, DiffLine, Edit};

/// default maximum number of lines shown per side of an edit before truncating.
pub const DEFAULT_CONTEXT_LINES: usize = 10;

/// format a markdown-oriented human report from a diff result.
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
            if let Some(span) = &finding.span {
                writeln!(
                    out,
                    "- {} [{}] (line {}): {}",
                    finding.level, finding.code, span.line, finding.message
                )
                .unwrap();
            } else {
                writeln!(
                    out,
                    "- {} [{}]: {}",
                    finding.level, finding.code, finding.message
                )
                .unwrap();
            }
        }
    }

    out
}

/// format a colored unified-diff-style report from a diff result.
///
/// uses ANSI colors when enabled via `owo_colors`:
/// - `---`/`+++` file headers: bold
/// - `@@` hunk headers: cyan
/// - `-` deletion lines: red
/// - `+` insertion lines: green
///
/// call `owo_colors::set_override(false)` before invoking this function
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
                append_replace_colored(&mut out, old_lines, new_lines, max_lines_shown);
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

fn append_replace_colored(
    out: &mut String,
    old_lines: &[DiffLine],
    new_lines: &[DiffLine],
    max_lines_shown: usize,
) {
    let pair_count = old_lines.len().min(new_lines.len());
    let old_show = old_lines.len().min(max_lines_shown);
    let new_show = new_lines.len().min(max_lines_shown);

    let diff_count = pair_count.min(old_show.max(new_show));
    let diffs: Vec<_> = (0..diff_count)
        .map(|i| crate::inline::inline_diff(&old_lines[i].text, &new_lines[i].text))
        .collect();

    for i in 0..old_show {
        if i < pair_count {
            append_inline_spans(out, "- ", &diffs[i].0, true);
        } else {
            writeln!(out, "{}", format_args!("- {}", old_lines[i].text).red()).unwrap();
        }
    }
    let old_remaining = old_lines.len().saturating_sub(max_lines_shown);
    if old_remaining > 0 {
        writeln!(
            out,
            "{}",
            format_args!("- ... and {old_remaining} more").dimmed()
        )
        .unwrap();
    }

    for i in 0..new_show {
        if i < pair_count {
            append_inline_spans(out, "+ ", &diffs[i].1, false);
        } else {
            writeln!(out, "{}", format_args!("+ {}", new_lines[i].text).green()).unwrap();
        }
    }
    let new_remaining = new_lines.len().saturating_sub(max_lines_shown);
    if new_remaining > 0 {
        writeln!(
            out,
            "{}",
            format_args!("+ ... and {new_remaining} more").dimmed()
        )
        .unwrap();
    }
}

fn append_inline_spans(out: &mut String, prefix: &str, spans: &[TokenSpan], is_delete: bool) {
    if is_delete {
        write!(out, "{}", prefix.red()).unwrap();
        for span in spans {
            if span.changed {
                write!(out, "{}", span.text.red().bold().underline()).unwrap();
            } else {
                write!(out, "{}", span.text.red()).unwrap();
            }
        }
    } else {
        write!(out, "{}", prefix.green()).unwrap();
        for span in spans {
            if span.changed {
                write!(out, "{}", span.text.green().bold().underline()).unwrap();
            } else {
                write!(out, "{}", span.text.green()).unwrap();
            }
        }
    }
    writeln!(out).unwrap();
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
            append_replace_diff_lines(&mut out, old_lines, new_lines, max_lines_shown);
        }
    }
    out
}

fn append_replace_diff_lines(
    out: &mut String,
    old_lines: &[DiffLine],
    new_lines: &[DiffLine],
    max_lines_shown: usize,
) {
    let pair_count = old_lines.len().min(new_lines.len());
    let old_show = old_lines.len().min(max_lines_shown);
    let new_show = new_lines.len().min(max_lines_shown);

    let diff_count = pair_count.min(old_show.max(new_show));
    let diffs: Vec<_> = (0..diff_count)
        .map(|i| crate::inline::inline_diff(&old_lines[i].text, &new_lines[i].text))
        .collect();

    for i in 0..old_show {
        if i < pair_count {
            write!(out, "\n   - L{}: ", old_lines[i].span.line).unwrap();
            append_markdown_spans(out, &diffs[i].0);
        } else {
            write!(
                out,
                "\n   - L{}: {}",
                old_lines[i].span.line, old_lines[i].text
            )
            .unwrap();
        }
    }
    let old_remaining = old_lines.len().saturating_sub(max_lines_shown);
    if old_remaining > 0 {
        write!(out, "\n   - ... and {old_remaining} more").unwrap();
    }

    for i in 0..new_show {
        if i < pair_count {
            write!(out, "\n   + L{}: ", new_lines[i].span.line).unwrap();
            append_markdown_spans(out, &diffs[i].1);
        } else {
            write!(
                out,
                "\n   + L{}: {}",
                new_lines[i].span.line, new_lines[i].text
            )
            .unwrap();
        }
    }
    let new_remaining = new_lines.len().saturating_sub(max_lines_shown);
    if new_remaining > 0 {
        write!(out, "\n   + ... and {new_remaining} more").unwrap();
    }
}

fn append_markdown_spans(out: &mut String, spans: &[TokenSpan]) {
    for span in spans {
        if span.changed {
            write!(out, "**{}**", span.text).unwrap();
        } else {
            write!(out, "{}", span.text).unwrap();
        }
    }
}

fn append_diff_lines(out: &mut String, prefix: &str, lines: &[DiffLine], max_lines_shown: usize) {
    let show = lines.len().min(max_lines_shown);
    for line in &lines[..show] {
        write!(out, "\n   {prefix} L{}: {}", line.span.line, line.text).unwrap();
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

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for c in s.chars() {
            if c == '\x1b' {
                in_escape = true;
            } else if in_escape {
                if c == 'm' {
                    in_escape = false;
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn make_diff_line_at(text: &str, line: usize) -> DiffLine {
        DiffLine {
            content_key: 0,
            occurrence_key: 0,
            text: text.to_string(),
            path: Path(vec![0]),
            span: Span {
                line,
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
            lines: lines
                .iter()
                .enumerate()
                .map(|(i, t)| make_diff_line_at(t, i + 1))
                .collect(),
        }
    }

    fn delete_edit(lines: &[&str]) -> Edit {
        Edit::Delete {
            at_key: Some(99),
            left_anchor: None,
            right_anchor: None,
            lines: lines
                .iter()
                .enumerate()
                .map(|(i, t)| make_diff_line_at(t, i + 1))
                .collect(),
        }
    }

    fn replace_edit(old: &[&str], new: &[&str]) -> Edit {
        Edit::Replace {
            old_at_key: Some(10),
            new_at_key: Some(20),
            left_anchor: None,
            right_anchor: None,
            old_lines: old
                .iter()
                .enumerate()
                .map(|(i, t)| make_diff_line_at(t, i + 1))
                .collect(),
            new_lines: new
                .iter()
                .enumerate()
                .map(|(i, t)| make_diff_line_at(t, i + 1))
                .collect(),
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
        assert!(report.contains("+ L1: permit any"));
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
        assert!(report.contains("- L1: deny all"));
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
        assert!(report.contains("- L1: **old** line"));
        assert!(report.contains("+ L1: **new** line"));
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

        // should show 2 lines then truncation message
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
        assert!(report.contains("+ L1: line1"));
        assert!(report.contains("+ L2: line2"));
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
                lines: vec![make_diff_line_at("new line", 1)],
            }],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);
        assert!(report.contains("at key <unknown>"));
    }

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

        let plain = strip_ansi(&result);
        assert!(plain.contains("@@ replace 1 line(s) at key"));
        assert!(plain.contains("- old"));
        assert!(plain.contains("+ new"));
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

        // should not have the extra newline that precedes findings
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

    #[test]
    fn markdown_diff_lines_show_source_line_numbers() {
        let diff = Diff {
            edits: vec![Edit::Insert {
                at_key: Some(1),
                left_anchor: None,
                right_anchor: None,
                lines: vec![
                    make_diff_line_at("first", 10),
                    make_diff_line_at("second", 11),
                    make_diff_line_at("third", 12),
                ],
            }],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("+ L10: first"));
        assert!(report.contains("+ L11: second"));
        assert!(report.contains("+ L12: third"));
    }

    #[test]
    fn markdown_findings_with_span_show_line_number() {
        let diff = Diff {
            findings: vec![Finding {
                code: "test_code".to_string(),
                level: FindingLevel::Warning,
                message: "something broke".to_string(),
                path: None,
                span: Some(Span {
                    line: 42,
                    start_byte: 0,
                    end_byte: 10,
                }),
            }],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("- warning [test_code] (line 42): something broke"));
    }

    #[test]
    fn markdown_findings_without_span_omit_line_number() {
        let diff = Diff {
            findings: vec![make_finding(FindingLevel::Info, "no_span", "no location")],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("- info [no_span]: no location"));
        assert!(!report.contains("(line"));
    }

    #[test]
    fn default_context_lines_is_ten() {
        assert_eq!(DEFAULT_CONTEXT_LINES, 10);
    }
}
