use std::borrow::Cow;
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
///
/// configuration text and both labels are escaped, so markdown syntax in a
/// config renders as the literal text the device holds.
pub fn format_markdown_report(
    diff: &Diff,
    left_label: &str,
    right_label: &str,
    max_lines_shown: usize,
) -> String {
    let mut out = String::new();
    out.push_str("# Config Diff Report\n\n");
    writeln!(out, "- Left: {}", code_span(left_label)).unwrap();
    writeln!(out, "- Right: {}\n", code_span(right_label)).unwrap();

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
                walk_single(
                    &mut out,
                    &ColoredRenderer,
                    Side::New,
                    lines,
                    max_lines_shown,
                );
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
                walk_single(
                    &mut out,
                    &ColoredRenderer,
                    Side::Old,
                    lines,
                    max_lines_shown,
                );
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
                walk_replace(
                    &mut out,
                    &ColoredRenderer,
                    old_lines,
                    new_lines,
                    max_lines_shown,
                );
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

/// which side of an edit a rendered line belongs to: old lines carry the `-`
/// marker (red when colored), new lines carry `+` (green when colored).
#[derive(Clone, Copy)]
enum Side {
    Old,
    New,
}

/// per-format strategy for emitting a single diff line.
///
/// the walkers ([`walk_single`], [`walk_replace`]) own the shared control
/// skeleton — truncation counting and the paired/unpaired split — while an
/// implementation decides only how one line is formatted.
trait LineRenderer {
    /// emit a paired line with token-level inline highlighting.
    fn inline_line(&self, out: &mut String, side: Side, line: &DiffLine, spans: &[TokenSpan]);
    /// emit a line verbatim (a single-sided insert/delete, or the extra lines
    /// on the longer side of a replace).
    fn plain_line(&self, out: &mut String, side: Side, line: &DiffLine);
    /// emit the "... and N more" truncation marker for one side.
    fn truncation(&self, out: &mut String, side: Side, remaining: usize);
}

/// renders lines for the colored unified-diff report.
struct ColoredRenderer;

impl LineRenderer for ColoredRenderer {
    fn inline_line(&self, out: &mut String, side: Side, _line: &DiffLine, spans: &[TokenSpan]) {
        match side {
            Side::Old => {
                write!(out, "{}", "- ".red()).unwrap();
                for span in spans {
                    if span.changed {
                        write!(out, "{}", span.text.red().bold().underline()).unwrap();
                    } else {
                        write!(out, "{}", span.text.red()).unwrap();
                    }
                }
            }
            Side::New => {
                write!(out, "{}", "+ ".green()).unwrap();
                for span in spans {
                    if span.changed {
                        write!(out, "{}", span.text.green().bold().underline()).unwrap();
                    } else {
                        write!(out, "{}", span.text.green()).unwrap();
                    }
                }
            }
        }
        writeln!(out).unwrap();
    }

    fn plain_line(&self, out: &mut String, side: Side, line: &DiffLine) {
        match side {
            Side::Old => writeln!(out, "{}", format_args!("- {}", line.text).red()).unwrap(),
            Side::New => writeln!(out, "{}", format_args!("+ {}", line.text).green()).unwrap(),
        }
    }

    fn truncation(&self, out: &mut String, side: Side, remaining: usize) {
        let marker = match side {
            Side::Old => "-",
            Side::New => "+",
        };
        writeln!(
            out,
            "{}",
            format_args!("{marker} ... and {remaining} more").dimmed()
        )
        .unwrap();
    }
}

/// markdown openers that are significant anywhere in a line, not just at its start.
const INLINE_MARKDOWN: &[char] = &['\\', '`', '*', '_', '[', ']', '<', '&', '~'];

/// backslash-escape the inline markdown syntax in a run of configuration text.
fn escape_markdown(text: &str) -> Cow<'_, str> {
    if !text.contains(INLINE_MARKDOWN) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if INLINE_MARKDOWN.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    Cow::Owned(out)
}

/// wrap text in a code span, widening the backtick fence past the longest run
/// inside it and padding when the content would otherwise merge with the fence.
///
/// empty text yields an empty string: CommonMark has no empty code span.
fn code_span(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let longest_run = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest_run + 1);
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && !text.trim().is_empty());
    if pad {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

/// renders lines for the markdown report.
struct MarkdownRenderer;

impl LineRenderer for MarkdownRenderer {
    fn inline_line(&self, out: &mut String, side: Side, line: &DiffLine, spans: &[TokenSpan]) {
        let marker = match side {
            Side::Old => "-",
            Side::New => "+",
        };
        write!(out, "\n   {marker} L{}: ", line.span.line).unwrap();
        for span in spans {
            let text = escape_markdown(span.text);
            match (span.changed, span.text.trim().is_empty()) {
                // `**` around whitespace is not emphasis under CommonMark's flanking rules.
                (true, true) => write!(out, "{}", code_span(span.text)),
                (true, false) => write!(out, "**{text}**"),
                (false, _) => write!(out, "{text}"),
            }
            .unwrap();
        }
    }

    fn plain_line(&self, out: &mut String, side: Side, line: &DiffLine) {
        let marker = match side {
            Side::Old => "-",
            Side::New => "+",
        };
        write!(
            out,
            "\n   {marker} L{}: {}",
            line.span.line,
            escape_markdown(&line.text)
        )
        .unwrap();
    }

    fn truncation(&self, out: &mut String, side: Side, remaining: usize) {
        let marker = match side {
            Side::Old => "-",
            Side::New => "+",
        };
        write!(out, "\n   {marker} ... and {remaining} more").unwrap();
    }
}

/// walk a single-sided (insert or delete) edit, emitting each shown line then
/// a truncation marker once `max_lines_shown` is exceeded.
fn walk_single<R: LineRenderer>(
    out: &mut String,
    renderer: &R,
    side: Side,
    lines: &[DiffLine],
    max_lines_shown: usize,
) {
    let show = lines.len().min(max_lines_shown);
    for line in &lines[..show] {
        renderer.plain_line(out, side, line);
    }
    let remaining = lines.len().saturating_sub(max_lines_shown);
    if remaining > 0 {
        renderer.truncation(out, side, remaining);
    }
}

/// walk a replace edit: pair old/new lines for inline highlighting up to the
/// shorter length, render the extra lines on the longer side verbatim, and
/// emit a per-side truncation marker once `max_lines_shown` is exceeded.
fn walk_replace<R: LineRenderer>(
    out: &mut String,
    renderer: &R,
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
            renderer.inline_line(out, Side::Old, &old_lines[i], &diffs[i].0);
        } else {
            renderer.plain_line(out, Side::Old, &old_lines[i]);
        }
    }
    let old_remaining = old_lines.len().saturating_sub(max_lines_shown);
    if old_remaining > 0 {
        renderer.truncation(out, Side::Old, old_remaining);
    }

    for i in 0..new_show {
        if i < pair_count {
            renderer.inline_line(out, Side::New, &new_lines[i], &diffs[i].1);
        } else {
            renderer.plain_line(out, Side::New, &new_lines[i]);
        }
    }
    let new_remaining = new_lines.len().saturating_sub(max_lines_shown);
    if new_remaining > 0 {
        renderer.truncation(out, Side::New, new_remaining);
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
                escape_markdown(&crate::util::key_label(*at_key)),
            )
            .unwrap();
            walk_single(
                &mut out,
                &MarkdownRenderer,
                Side::New,
                lines,
                max_lines_shown,
            );
        }
        Edit::Delete { at_key, lines, .. } => {
            write!(
                out,
                "Delete {} line(s) at key {}",
                lines.len(),
                escape_markdown(&crate::util::key_label(*at_key)),
            )
            .unwrap();
            walk_single(
                &mut out,
                &MarkdownRenderer,
                Side::Old,
                lines,
                max_lines_shown,
            );
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
                escape_markdown(&crate::util::key_label(*old_at_key)),
                new_lines.len(),
                escape_markdown(&crate::util::key_label(*new_at_key)),
            )
            .unwrap();
            walk_replace(
                &mut out,
                &MarkdownRenderer,
                old_lines,
                new_lines,
                max_lines_shown,
            );
        }
    }
    out
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
        assert!(report.contains(r"at key \<unknown>"));
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
    fn unified_delete_truncation() {
        owo_colors::set_override(false);
        let lines: Vec<&str> = (0..5).map(|_| "gone").collect();
        let diff = Diff {
            edits: vec![delete_edit(&lines)],
            ..Default::default()
        };
        let result = strip_ansi(&format_unified_diff(&diff, "a", "b", 2));
        assert!(result.contains("- gone"));
        assert!(result.contains("- ... and 3 more"));
    }

    #[test]
    fn unified_replace_old_longer_shows_unpaired_and_truncates() {
        owo_colors::set_override(false);
        // paired line shares the "set mtu" prefix (unchanged inline spans); the
        // surplus old lines render as plain lines, then the old side truncates.
        let old = ["set mtu 1500", "extra one", "extra two", "extra three"];
        let new = ["set mtu 9000"];
        let diff = Diff {
            edits: vec![replace_edit(&old, &new)],
            ..Default::default()
        };
        let result = strip_ansi(&format_unified_diff(&diff, "a", "b", 2));
        assert!(result.contains("- set mtu 1500"));
        assert!(result.contains("+ set mtu 9000"));
        assert!(result.contains("- extra one"));
        assert!(result.contains("- ... and 2 more"));
    }

    #[test]
    fn unified_replace_new_longer_shows_unpaired_additions() {
        owo_colors::set_override(false);
        let old = ["base"];
        let new = ["base", "added one", "added two"];
        let diff = Diff {
            edits: vec![replace_edit(&old, &new)],
            ..Default::default()
        };
        let result = strip_ansi(&format_unified_diff(&diff, "a", "b", 10));
        assert!(result.contains("+ added one"));
        assert!(result.contains("+ added two"));
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

    #[test]
    fn escape_markdown_borrows_text_without_inline_syntax() {
        assert!(matches!(
            escape_markdown("set mtu 1500 # comment | pipe"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn escape_markdown_covers_every_inline_opener() {
        assert_eq!(
            escape_markdown(r"a\b`c*d_e[f]g<h&i~j"),
            r"a\\b\`c\*d\_e\[f\]g\<h\&i\~j"
        );
    }

    #[test]
    fn code_span_widens_fence_past_longest_backtick_run() {
        assert_eq!(code_span("a``b`c"), "```a``b`c```");
    }

    #[test]
    fn code_span_pads_when_content_touches_the_fence() {
        assert_eq!(code_span("`x`"), "`` `x` ``");
        assert_eq!(code_span(" x "), "`  x  `");
    }

    #[test]
    fn code_span_leaves_all_whitespace_content_unpadded() {
        assert_eq!(code_span(" "), "` `");
        assert_eq!(code_span("  "), "`  `");
    }

    #[test]
    fn code_span_of_empty_text_is_empty() {
        assert_eq!(code_span(""), "");
    }

    #[test]
    fn markdown_escapes_inline_syntax_in_plain_lines() {
        let diff = Diff {
            edits: vec![insert_edit(&["banner motd *** <b>halt</b> `now` ***"])],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains(r"+ L1: banner motd \*\*\* \<b>halt\</b> \`now\` \*\*\*"));
    }

    #[test]
    fn markdown_escapes_inline_syntax_in_changed_and_unchanged_spans() {
        let diff = Diff {
            edits: vec![replace_edit(
                &["set banner *old* [x]"],
                &["set banner *new* [x]"],
            )],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains(r"- L1: set banner **\*old\*** \[x\]"));
        assert!(report.contains(r"+ L1: set banner **\*new\*** \[x\]"));
    }

    #[test]
    fn markdown_whitespace_only_change_renders_as_code_span() {
        let diff = Diff {
            edits: vec![replace_edit(&["set mtu  1500"], &["set mtu 1500"])],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("- L1: set mtu`  `1500"));
        assert!(report.contains("+ L1: set mtu` `1500"));
    }

    #[test]
    fn markdown_labels_survive_a_backtick() {
        let diff = empty_diff();
        let report = format_markdown_report(&diff, "a`b.cfg", "`c.cfg", 10);

        assert!(report.contains("- Left: ``a`b.cfg``"));
        assert!(report.contains("- Right: `` `c.cfg ``"));
    }

    #[test]
    fn unified_leaves_config_text_and_labels_unescaped() {
        owo_colors::set_override(false);
        let diff = Diff {
            edits: vec![
                insert_edit(&["banner motd *** <b>halt</b> ***"]),
                replace_edit(&["set mtu  1500"], &["set mtu 1500"]),
            ],
            ..Default::default()
        };
        let result = strip_ansi(&format_unified_diff(&diff, "a`b.cfg", "*c.cfg", 10));

        assert!(result.contains("--- a`b.cfg"));
        assert!(result.contains("+++ *c.cfg"));
        assert!(result.contains("+ banner motd *** <b>halt</b> ***"));
        assert!(result.contains("- set mtu  1500"));
    }
}
