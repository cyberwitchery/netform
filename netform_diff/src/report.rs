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
