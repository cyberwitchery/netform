use crate::model::{Diff, DiffLine, Edit};

/// Maximum number of lines shown per side of an edit before truncating.
const MAX_LINES_SHOWN: usize = 10;

/// Format a markdown-oriented human report from a diff result.
pub fn format_markdown_report(diff: &Diff, left_label: &str, right_label: &str) -> String {
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
            out.push_str(&format!("{}. {}\n", idx + 1, describe_edit(edit)));
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

fn describe_edit(edit: &Edit) -> String {
    let mut out = String::new();
    match edit {
        Edit::Insert { at_key, lines, .. } => {
            out.push_str(&format!(
                "Insert {} line(s) at key {}",
                lines.len(),
                crate::util::key_label(*at_key),
            ));
            append_diff_lines(&mut out, "+", lines);
        }
        Edit::Delete { at_key, lines, .. } => {
            out.push_str(&format!(
                "Delete {} line(s) at key {}",
                lines.len(),
                crate::util::key_label(*at_key),
            ));
            append_diff_lines(&mut out, "-", lines);
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
            append_diff_lines(&mut out, "-", old_lines);
            append_diff_lines(&mut out, "+", new_lines);
        }
    }
    out
}

fn append_diff_lines(out: &mut String, prefix: &str, lines: &[DiffLine]) {
    let show = lines.len().min(MAX_LINES_SHOWN);
    for line in &lines[..show] {
        out.push_str(&format!("\n   {} {}", prefix, line.text));
    }
    let remaining = lines.len().saturating_sub(MAX_LINES_SHOWN);
    if remaining > 0 {
        out.push_str(&format!("\n   {} ... and {} more", prefix, remaining));
    }
}
