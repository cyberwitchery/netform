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
    out.push_str(&format!("- Left: `{left_label}`\n"));
    out.push_str(&format!("- Right: `{right_label}`\n\n"));

    out.push_str("## Stats\n\n");
    out.push_str(&format!(
        "- Inserts: {} ({} lines)\n",
        diff.stats.inserts, diff.stats.inserted_lines
    ));
    out.push_str(&format!(
        "- Deletes: {} ({} lines)\n",
        diff.stats.deletes, diff.stats.deleted_lines
    ));
    out.push_str(&format!(
        "- Replaces: {} ({} -> {} lines)\n\n",
        diff.stats.replaces, diff.stats.replaced_old_lines, diff.stats.replaced_new_lines
    ));

    out.push_str("## Edits\n\n");
    if diff.edits.is_empty() {
        out.push_str("No changes detected.\n");
    } else {
        for (idx, edit) in diff.edits.iter().enumerate() {
            out.push_str(&format!(
                "{}. {}\n",
                idx + 1,
                describe_edit(edit, max_lines_shown)
            ));
        }
    }

    if !diff.findings.is_empty() {
        out.push_str("\n## Findings\n\n");
        for finding in &diff.findings {
            out.push_str(&format!(
                "- {} [{}]: {}\n",
                finding.level, finding.code, finding.message
            ));
        }
    }

    out
}

/// Format a colored unified-diff-style report from a diff result.
///
/// Uses ANSI colors when enabled via the `colored` crate's global controls:
/// - `---`/`+++` file headers: bold
/// - `@@` hunk headers: cyan
/// - `-` deletion lines: red
/// - `+` insertion lines: green
///
/// Call `colored::control::set_override(false)` before invoking this function
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

    out.push_str(&format!("{}\n", format!("--- {left_label}").bold()));
    out.push_str(&format!("{}\n", format!("+++ {right_label}").bold()));

    for edit in &diff.edits {
        match edit {
            Edit::Insert { at_key, lines, .. } => {
                out.push_str(&format!(
                    "{}\n",
                    format!(
                        "@@ insert {} line(s) at key {} @@",
                        lines.len(),
                        crate::util::key_label(*at_key),
                    )
                    .cyan()
                ));
                append_colored_lines(&mut out, "+", lines, max_lines_shown);
            }
            Edit::Delete { at_key, lines, .. } => {
                out.push_str(&format!(
                    "{}\n",
                    format!(
                        "@@ delete {} line(s) at key {} @@",
                        lines.len(),
                        crate::util::key_label(*at_key),
                    )
                    .cyan()
                ));
                append_colored_lines(&mut out, "-", lines, max_lines_shown);
            }
            Edit::Replace {
                old_at_key,
                new_at_key,
                old_lines,
                new_lines,
                ..
            } => {
                out.push_str(&format!(
                    "{}\n",
                    format!(
                        "@@ replace {} line(s) at key {} -> {} line(s) at key {} @@",
                        old_lines.len(),
                        crate::util::key_label(*old_at_key),
                        new_lines.len(),
                        crate::util::key_label(*new_at_key),
                    )
                    .cyan()
                ));
                append_colored_lines(&mut out, "-", old_lines, max_lines_shown);
                append_colored_lines(&mut out, "+", new_lines, max_lines_shown);
            }
        }
    }

    if !diff.findings.is_empty() {
        out.push('\n');
        for finding in &diff.findings {
            out.push_str(&format!(
                "{}\n",
                format!("{} [{}]: {}", finding.level, finding.code, finding.message).yellow()
            ));
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
            out.push_str(&format!("{}\n", formatted.green()));
        } else {
            out.push_str(&format!("{}\n", formatted.red()));
        }
    }
    let remaining = lines.len().saturating_sub(max_lines_shown);
    if remaining > 0 {
        out.push_str(&format!(
            "{}\n",
            format!("{prefix} ... and {remaining} more").dimmed()
        ));
    }
}

fn describe_edit(edit: &Edit, max_lines_shown: usize) -> String {
    let mut out = String::new();
    match edit {
        Edit::Insert { at_key, lines, .. } => {
            out.push_str(&format!(
                "Insert {} line(s) at key {}",
                lines.len(),
                crate::util::key_label(*at_key),
            ));
            append_diff_lines(&mut out, "+", lines, max_lines_shown);
        }
        Edit::Delete { at_key, lines, .. } => {
            out.push_str(&format!(
                "Delete {} line(s) at key {}",
                lines.len(),
                crate::util::key_label(*at_key),
            ));
            append_diff_lines(&mut out, "-", lines, max_lines_shown);
        }
        Edit::Replace {
            old_at_key,
            new_at_key,
            old_lines,
            new_lines,
            ..
        } => {
            out.push_str(&format!(
                "Replace {} line(s) at key {} with {} line(s) at key {}",
                old_lines.len(),
                crate::util::key_label(*old_at_key),
                new_lines.len(),
                crate::util::key_label(*new_at_key),
            ));
            append_diff_lines(&mut out, "-", old_lines, max_lines_shown);
            append_diff_lines(&mut out, "+", new_lines, max_lines_shown);
        }
    }
    out
}

fn append_diff_lines(out: &mut String, prefix: &str, lines: &[DiffLine], max_lines_shown: usize) {
    let show = lines.len().min(max_lines_shown);
    for line in &lines[..show] {
        out.push_str(&format!("\n   {} {}", prefix, line.text));
    }
    let remaining = lines.len().saturating_sub(max_lines_shown);
    if remaining > 0 {
        out.push_str(&format!("\n   {} ... and {} more", prefix, remaining));
    }
}
