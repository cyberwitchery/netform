use std::borrow::Cow;
use std::fmt::Write;

use owo_colors::{OwoColorize, Style};

use crate::inline::TokenSpan;
use crate::model::{Diff, DiffLine, Edit};

/// default maximum number of lines shown per side of an edit before truncating.
pub const DEFAULT_CONTEXT_LINES: usize = 10;

/// whether a rendered report carries ANSI escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    /// style the report with ANSI escapes.
    Always,
    /// emit plain text.
    Never,
}

/// the ANSI style of each role in the unified-diff report; a plain [`Style`]
/// writes no escapes, so color-off needs no second code path.
#[derive(Clone, Copy, Default)]
struct Palette {
    header: Style,
    hunk: Style,
    del: Style,
    del_changed: Style,
    ins: Style,
    ins_changed: Style,
    truncation: Style,
    finding: Style,
}

impl Palette {
    fn new(color: ColorChoice) -> Self {
        match color {
            ColorChoice::Always => Self {
                header: Style::new().bold(),
                hunk: Style::new().cyan(),
                del: Style::new().red(),
                del_changed: Style::new().red().bold().underline(),
                ins: Style::new().green(),
                ins_changed: Style::new().green().bold().underline(),
                truncation: Style::new().dimmed(),
                finding: Style::new().yellow(),
            },
            ColorChoice::Never => Self::default(),
        }
    }
}

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
            let ordinal = idx + 1;
            writeln!(
                out,
                "{ordinal}. {}",
                describe_edit(edit, ordinal, max_lines_shown)
            )
            .unwrap();
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
/// renders with ANSI escapes; call [`format_unified_diff_with_color`] to
/// choose.
pub fn format_unified_diff(
    diff: &Diff,
    left_label: &str,
    right_label: &str,
    max_lines_shown: usize,
) -> String {
    format_unified_diff_with_color(
        diff,
        left_label,
        right_label,
        max_lines_shown,
        ColorChoice::Always,
    )
}

/// format a unified-diff-style report, choosing whether it carries ANSI escapes.
///
/// `color` alone decides: nothing here consults the terminal or the
/// environment, and [`ColorChoice::Never`] output is byte-for-byte the
/// [`ColorChoice::Always`] output with every escape removed.
///
/// the styled roles are:
/// - `---`/`+++` file headers: bold
/// - `@@` hunk headers: cyan
/// - `-` deletion lines: red
/// - `+` insertion lines: green
/// - the tokens that differ within a replaced line: also bold and underlined
/// - `... and N more` truncation markers: dimmed
/// - findings: yellow
pub fn format_unified_diff_with_color(
    diff: &Diff,
    left_label: &str,
    right_label: &str,
    max_lines_shown: usize,
    color: ColorChoice,
) -> String {
    let mut out = String::new();
    if diff.edits.is_empty() {
        return out;
    }

    let palette = Palette::new(color);
    let renderer = ColoredRenderer { palette };

    writeln!(
        out,
        "{}",
        format_args!("--- {left_label}").style(palette.header)
    )
    .unwrap();
    writeln!(
        out,
        "{}",
        format_args!("+++ {right_label}").style(palette.header)
    )
    .unwrap();

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
                    .style(palette.hunk)
                )
                .unwrap();
                walk_single(&mut out, &renderer, Side::New, lines, max_lines_shown);
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
                    .style(palette.hunk)
                )
                .unwrap();
                walk_single(&mut out, &renderer, Side::Old, lines, max_lines_shown);
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
                    .style(palette.hunk)
                )
                .unwrap();
                walk_replace(&mut out, &renderer, old_lines, new_lines, max_lines_shown);
            }
        }
    }

    if !diff.findings.is_empty() {
        out.push('\n');
        for finding in &diff.findings {
            writeln!(
                out,
                "{}",
                format_args!("{} [{}]: {}", finding.level, finding.code, finding.message)
                    .style(palette.finding)
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
struct ColoredRenderer {
    palette: Palette,
}

impl ColoredRenderer {
    /// the marker and the unchanged/changed styles one side draws with.
    fn side_styles(&self, side: Side) -> (char, Style, Style) {
        match side {
            Side::Old => ('-', self.palette.del, self.palette.del_changed),
            Side::New => ('+', self.palette.ins, self.palette.ins_changed),
        }
    }
}

impl LineRenderer for ColoredRenderer {
    fn inline_line(&self, out: &mut String, side: Side, _line: &DiffLine, spans: &[TokenSpan]) {
        let (marker, plain, changed) = self.side_styles(side);
        write!(out, "{}", format_args!("{marker} ").style(plain)).unwrap();
        for span in spans {
            let style = if span.changed { changed } else { plain };
            write!(out, "{}", span.text.style(style)).unwrap();
        }
        writeln!(out).unwrap();
    }

    fn plain_line(&self, out: &mut String, side: Side, line: &DiffLine) {
        let (marker, plain, _) = self.side_styles(side);
        writeln!(
            out,
            "{}",
            format_args!("{marker} {}", line.text).style(plain)
        )
        .unwrap();
    }

    fn truncation(&self, out: &mut String, side: Side, remaining: usize) {
        let (marker, _, _) = self.side_styles(side);
        writeln!(
            out,
            "{}",
            format_args!("{marker} ... and {remaining} more").style(self.palette.truncation)
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

/// renders lines for the markdown report, nested under one numbered edit.
struct MarkdownRenderer {
    indent: usize,
}

impl MarkdownRenderer {
    /// a list item's content column is the width of its marker: `1. ` is 3, `10. ` is 4.
    fn for_ordinal(ordinal: usize) -> Self {
        Self {
            indent: ordinal.to_string().len() + 2,
        }
    }

    /// open a nested list item for one diff line; a bare `-`/`+` here would be
    /// eaten as the list marker, so the side marker rides in a code span.
    fn bullet(&self, side: Side) -> String {
        let marker = match side {
            Side::Old => '-',
            Side::New => '+',
        };
        format!("\n{:width$}- `{marker}`", "", width = self.indent)
    }
}

impl LineRenderer for MarkdownRenderer {
    fn inline_line(&self, out: &mut String, side: Side, line: &DiffLine, spans: &[TokenSpan]) {
        write!(out, "{} L{}: ", self.bullet(side), line.span.line).unwrap();
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
        write!(
            out,
            "{} L{}: {}",
            self.bullet(side),
            line.span.line,
            escape_markdown(&line.text)
        )
        .unwrap();
    }

    fn truncation(&self, out: &mut String, side: Side, remaining: usize) {
        write!(out, "{} ... and {remaining} more", self.bullet(side)).unwrap();
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

fn describe_edit(edit: &Edit, ordinal: usize, max_lines_shown: usize) -> String {
    let renderer = MarkdownRenderer::for_ordinal(ordinal);
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
            walk_single(&mut out, &renderer, Side::New, lines, max_lines_shown);
        }
        Edit::Delete { at_key, lines, .. } => {
            write!(
                out,
                "Delete {} line(s) at key {}",
                lines.len(),
                escape_markdown(&crate::util::key_label(*at_key)),
            )
            .unwrap();
            walk_single(&mut out, &renderer, Side::Old, lines, max_lines_shown);
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
            walk_replace(&mut out, &renderer, old_lines, new_lines, max_lines_shown);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DiffStats, Finding, FindingLevel};
    use netform_ir::{Path, Span};

    fn plain(diff: &Diff, left_label: &str, right_label: &str, max_lines_shown: usize) -> String {
        format_unified_diff_with_color(
            diff,
            left_label,
            right_label,
            max_lines_shown,
            ColorChoice::Never,
        )
    }

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

    fn numbered_inserts(count: usize) -> Vec<Edit> {
        (1..=count)
            .map(|n| insert_edit(&[&format!("edit-{n}")]))
            .collect()
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
        assert!(report.contains("   - `+` L1: permit any"));
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
        assert!(report.contains("   - `-` L1: deny all"));
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
        assert!(report.contains("   - `-` L1: **old** line"));
        assert!(report.contains("   - `+` L1: **new** line"));
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
        assert!(report.contains("   - `+` ... and 3 more"));
    }

    #[test]
    fn markdown_no_truncation_when_within_limit() {
        let diff = Diff {
            edits: vec![insert_edit(&["line1", "line2"])],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 5);

        assert!(!report.contains("... and"));
        assert!(report.contains("   - `+` L1: line1"));
        assert!(report.contains("   - `+` L2: line2"));
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
        let result = plain(&diff, "a", "b", 10);
        assert!(result.is_empty());
    }

    #[test]
    fn unified_insert_edit() {
        let diff = Diff {
            edits: vec![insert_edit(&["permit any"])],
            ..Default::default()
        };
        let result = plain(&diff, "left.cfg", "right.cfg", 10);

        assert!(result.contains("--- left.cfg"));
        assert!(result.contains("+++ right.cfg"));
        assert!(result.contains("@@ insert 1 line(s) at key 0x000000000000002a @@"));
        assert!(result.contains("+ permit any"));
    }

    #[test]
    fn unified_delete_edit() {
        let diff = Diff {
            edits: vec![delete_edit(&["deny all"])],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 10);

        assert!(result.contains("@@ delete 1 line(s) at key 0x0000000000000063 @@"));
        assert!(result.contains("- deny all"));
    }

    #[test]
    fn unified_replace_edit() {
        let diff = Diff {
            edits: vec![replace_edit(&["old"], &["new"])],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 10);

        assert!(result.contains("@@ replace 1 line(s) at key"));
        assert!(result.contains("- old"));
        assert!(result.contains("+ new"));
    }

    #[test]
    fn unified_truncation() {
        let lines: Vec<&str> = (0..5).map(|_| "line").collect();
        let diff = Diff {
            edits: vec![insert_edit(&lines)],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 2);

        assert!(result.contains("... and 3 more"));
    }

    #[test]
    fn unified_no_truncation_within_limit() {
        let diff = Diff {
            edits: vec![insert_edit(&["a", "b"])],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 5);

        assert!(!result.contains("... and"));
    }

    #[test]
    fn unified_findings_appended() {
        let diff = Diff {
            edits: vec![insert_edit(&["x"])],
            findings: vec![make_finding(
                FindingLevel::Warning,
                "test_code",
                "something happened",
            )],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 10);

        assert!(result.contains("warning [test_code]: something happened"));
    }

    #[test]
    fn unified_no_findings_when_empty() {
        let diff = Diff {
            edits: vec![insert_edit(&["x"])],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 10);

        // should not have the extra newline that precedes findings
        let lines: Vec<&str> = result.lines().collect();
        let last = lines.last().unwrap();
        assert!(!last.is_empty()); // no trailing blank from findings block
    }

    #[test]
    fn unified_max_lines_exactly_at_boundary() {
        let diff = Diff {
            edits: vec![insert_edit(&["a", "b", "c"])],
            ..Default::default()
        };
        // max_lines_shown == lines.len(): all shown, no truncation
        let result = plain(&diff, "a", "b", 3);
        assert!(!result.contains("... and"));
        assert!(result.contains("+ a"));
        assert!(result.contains("+ b"));
        assert!(result.contains("+ c"));
    }

    #[test]
    fn unified_max_lines_one_over_boundary() {
        let diff = Diff {
            edits: vec![insert_edit(&["a", "b", "c"])],
            ..Default::default()
        };
        // max_lines_shown == lines.len() - 1: should truncate with "and 1 more"
        let result = plain(&diff, "a", "b", 2);
        assert!(result.contains("... and 1 more"));
    }

    #[test]
    fn unified_delete_truncation() {
        let lines: Vec<&str> = (0..5).map(|_| "gone").collect();
        let diff = Diff {
            edits: vec![delete_edit(&lines)],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 2);
        assert!(result.contains("- gone"));
        assert!(result.contains("- ... and 3 more"));
    }

    #[test]
    fn unified_replace_old_longer_shows_unpaired_and_truncates() {
        // paired line shares the "set mtu" prefix (unchanged inline spans); the
        // surplus old lines render as plain lines, then the old side truncates.
        let old = ["set mtu 1500", "extra one", "extra two", "extra three"];
        let new = ["set mtu 9000"];
        let diff = Diff {
            edits: vec![replace_edit(&old, &new)],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 2);
        assert!(result.contains("- set mtu 1500"));
        assert!(result.contains("+ set mtu 9000"));
        assert!(result.contains("- extra one"));
        assert!(result.contains("- ... and 2 more"));
    }

    #[test]
    fn unified_replace_new_longer_shows_unpaired_additions() {
        let old = ["base"];
        let new = ["base", "added one", "added two"];
        let diff = Diff {
            edits: vec![replace_edit(&old, &new)],
            ..Default::default()
        };
        let result = plain(&diff, "a", "b", 10);
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

        assert!(report.contains("   - `-` ... and 2 more"));
        assert!(report.contains("   - `+` ... and 4 more"));
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

        assert!(report.contains("   - `+` L10: first"));
        assert!(report.contains("   - `+` L11: second"));
        assert!(report.contains("   - `+` L12: third"));
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

        assert!(report.contains(r"   - `+` L1: banner motd \*\*\* \<b>halt\</b> \`now\` \*\*\*"));
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

        assert!(report.contains(r"   - `-` L1: set banner **\*old\*** \[x\]"));
        assert!(report.contains(r"   - `+` L1: set banner **\*new\*** \[x\]"));
    }

    #[test]
    fn markdown_whitespace_only_change_renders_as_code_span() {
        let diff = Diff {
            edits: vec![replace_edit(&["set mtu  1500"], &["set mtu 1500"])],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.contains("   - `-` L1: set mtu`  `1500"));
        assert!(report.contains("   - `+` L1: set mtu` `1500"));
    }

    #[test]
    fn markdown_labels_survive_a_backtick() {
        let diff = empty_diff();
        let report = format_markdown_report(&diff, "a`b.cfg", "`c.cfg", 10);

        assert!(report.contains("- Left: ``a`b.cfg``"));
        assert!(report.contains("- Right: `` `c.cfg ``"));
    }

    #[test]
    fn markdown_edit_lines_keep_side_markers() {
        let diff = Diff {
            edits: vec![insert_edit(&["permit any"]), delete_edit(&["deny all"])],
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        assert!(report.ends_with(concat!(
            "## Edits\n\n",
            "1. Insert 1 line(s) at key 0x000000000000002a\n",
            "   - `+` L1: permit any\n",
            "2. Delete 1 line(s) at key 0x0000000000000063\n",
            "   - `-` L1: deny all\n",
        )));
    }

    #[test]
    fn markdown_diff_line_indent_widens_with_the_ordinal() {
        let diff = Diff {
            edits: numbered_inserts(100),
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 10);

        let key = "0x000000000000002a";
        for (ordinal, indent) in [(9, "   "), (10, "    "), (99, "    "), (100, "     ")] {
            let expected = format!(
                "\n{ordinal}. Insert 1 line(s) at key {key}\n{indent}- `+` L1: edit-{ordinal}\n"
            );
            assert!(
                report.contains(&expected),
                "edit {ordinal} should nest at the width of its own marker: {expected:?}"
            );
        }
    }

    #[test]
    fn markdown_no_diff_line_opens_with_a_bare_side_marker() {
        let mut edits = numbered_inserts(100);
        edits[9] = insert_edit(&["permit any", "permit tcp"]);
        edits[98] = delete_edit(&["deny all"]);
        edits[99] = replace_edit(&["set mtu 1500"], &["set mtu 9000"]);
        let diff = Diff {
            edits,
            ..Default::default()
        };
        let report = format_markdown_report(&diff, "a", "b", 1);

        let edits = report.split_once("## Edits\n\n").unwrap().1;
        let mut ordinal = String::new();
        for line in edits.lines().filter(|l| !l.is_empty()) {
            if let Some((n, _)) = line.split_once(". ")
                && !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
            {
                ordinal = n.to_string();
                continue;
            }
            let indent = " ".repeat(ordinal.len() + 2);
            assert!(
                line.starts_with(&format!("{indent}- `-` "))
                    || line.starts_with(&format!("{indent}- `+` ")),
                "edit {ordinal} does not hold this line: {line:?}"
            );
        }
        assert!(edits.contains("\n    - `+` ... and 1 more\n"));
        assert!(edits.contains("\n    - `-` L1: deny all\n"));
        assert!(edits.contains("\n     - `-` L1: set mtu **1500**\n"));
        assert!(edits.contains("\n     - `+` L1: set mtu **9000**\n"));
    }

    #[test]
    fn unified_leaves_config_text_and_labels_unescaped() {
        let diff = Diff {
            edits: vec![
                insert_edit(&["banner motd *** <b>halt</b> ***"]),
                replace_edit(&["set mtu  1500"], &["set mtu 1500"]),
            ],
            ..Default::default()
        };
        let result = plain(&diff, "a`b.cfg", "*c.cfg", 10);

        assert!(result.contains("--- a`b.cfg"));
        assert!(result.contains("+++ *c.cfg"));
        assert!(result.contains("+ banner motd *** <b>halt</b> ***"));
        assert!(result.contains("- set mtu  1500"));
    }

    /// a diff that reaches every styled role, including both truncation markers.
    fn every_role_diff() -> Diff {
        Diff {
            edits: vec![
                insert_edit(&["permit any", "permit all", "permit some"]),
                delete_edit(&["deny all", "deny some", "deny any"]),
                replace_edit(&["set mtu 1500"], &["set mtu 9000"]),
            ],
            findings: vec![Finding {
                code: "unterminated-literal-region".to_string(),
                level: FindingLevel::Warning,
                message: "banner has no closing delimiter".to_string(),
                path: None,
                span: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn unified_color_off_is_color_on_minus_the_escapes() {
        let diff = every_role_diff();
        let colored = format_unified_diff_with_color(&diff, "a", "b", 2, ColorChoice::Always);
        let uncolored = plain(&diff, "a", "b", 2);

        assert!(colored.contains('\x1b'), "color on should emit escapes");
        assert!(
            !uncolored.contains('\x1b'),
            "color off should emit no escapes: {uncolored:?}"
        );
        assert_eq!(strip_ansi(&colored), uncolored);
    }

    #[test]
    fn unified_color_on_styles_every_role() {
        let diff = every_role_diff();
        let colored = format_unified_diff_with_color(&diff, "a", "b", 2, ColorChoice::Always);

        for (role, escape) in [
            ("bold file header", "\x1b[1m--- a\x1b[0m"),
            ("cyan hunk header", "\x1b[36m@@ insert"),
            ("red deletion", "\x1b[31m- deny all\x1b[0m"),
            ("green insertion", "\x1b[32m+ permit any\x1b[0m"),
            ("underlined changed deletion", "\x1b[31;1;4m1500\x1b[0m"),
            ("underlined changed insertion", "\x1b[32;1;4m9000\x1b[0m"),
            ("dimmed old truncation", "\x1b[2m- ... and 1 more\x1b[0m"),
            ("dimmed new truncation", "\x1b[2m+ ... and 1 more\x1b[0m"),
            ("yellow finding", "\x1b[33mwarning ["),
        ] {
            assert!(
                colored.contains(escape),
                "{role} should render as {escape:?}: {colored:?}"
            );
        }
    }

    #[test]
    fn unified_empty_diff_is_empty_under_either_color_choice() {
        let diff = empty_diff();
        assert!(
            format_unified_diff_with_color(&diff, "a", "b", 10, ColorChoice::Always).is_empty()
        );
        assert!(plain(&diff, "a", "b", 10).is_empty());
    }

    #[test]
    fn format_unified_diff_keeps_rendering_with_color() {
        let diff = every_role_diff();
        assert_eq!(
            format_unified_diff(&diff, "a", "b", 2),
            format_unified_diff_with_color(&diff, "a", "b", 2, ColorChoice::Always)
        );
    }
}
