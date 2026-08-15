//! a Junos `/* … */` comment body is a comment for normalization too, so
//! `--ignore-comments` drops all of it (see `junos_comment_region`).

use netform_dialect_junos::parse_junos;
use netform_diff::{Diff, Edit, NormalizationStep, NormalizeOptions, diff_documents};
use netform_ir::Path;

fn diff(a: &str, b: &str, options: NormalizeOptions) -> Diff {
    diff_documents(&parse_junos(a), &parse_junos(b), options).unwrap()
}

fn ignore_comments() -> NormalizeOptions {
    NormalizeOptions::new(vec![NormalizationStep::IgnoreComments])
}

fn edit_texts(diff: &Diff) -> Vec<String> {
    let mut out = Vec::new();
    for edit in &diff.edits {
        match edit {
            Edit::Insert { lines, .. } | Edit::Delete { lines, .. } => {
                out.extend(lines.iter().map(|line| line.text.clone()));
            }
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                out.extend(old_lines.iter().map(|line| line.text.clone()));
                out.extend(new_lines.iter().map(|line| line.text.clone()));
            }
        }
    }
    out
}

fn edit_paths(diff: &Diff) -> Vec<Path> {
    let mut out = Vec::new();
    for edit in &diff.edits {
        match edit {
            Edit::Insert { lines, .. } | Edit::Delete { lines, .. } => {
                out.extend(lines.iter().map(|line| line.path.clone()));
            }
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                out.extend(old_lines.iter().map(|line| line.path.clone()));
                out.extend(new_lines.iter().map(|line| line.path.clone()));
            }
        }
    }
    out
}

const BEFORE: &str = "\
/* site notes
   Rack 4, 19\" cabinet, row B
   contact ops@example.com
 */
system {
    host-name router-1;
}
interfaces {
    ge-0/0/0 {
        description \"uplink to core\";
    }
}
";

#[test]
fn ignore_comments_drops_a_rewritten_comment_body() {
    let after = BEFORE.replace("ops@example.com", "noc@example.com");

    assert!(diff(BEFORE, &after, NormalizeOptions::default()).has_changes);
    assert!(!diff(BEFORE, &after, ignore_comments()).has_changes);
}

#[test]
fn ignore_comments_drops_a_whole_added_comment() {
    let after = BEFORE.replace(
        " */\n",
        "   patched to panel 3\n   and cabled 2026-08-10\n */\n",
    );

    assert!(diff(BEFORE, &after, NormalizeOptions::default()).has_changes);
    assert!(!diff(BEFORE, &after, ignore_comments()).has_changes);
}

#[test]
fn ignore_comments_still_reports_a_configuration_change_under_a_comment() {
    let after = BEFORE.replace("uplink to core", "uplink to spine");

    let diff = diff(BEFORE, &after, ignore_comments());
    assert!(diff.has_changes);
    assert_eq!(
        edit_texts(&diff),
        vec![
            "        description \"uplink to core\";",
            "        description \"uplink to spine\";",
        ],
    );
}

#[test]
fn an_inch_mark_in_comment_prose_does_not_hide_a_configuration_change() {
    let plain = BEFORE.replace("19\" cabinet", "19in cabinet");
    let changed = |cfg: &str| cfg.replace("host-name router-1", "host-name router-2");

    let with_inch_mark = diff(BEFORE, &changed(BEFORE), NormalizeOptions::default());
    let control = diff(&plain, &changed(&plain), NormalizeOptions::default());

    assert!(with_inch_mark.has_changes);
    assert_eq!(edit_texts(&with_inch_mark), edit_texts(&control));
    assert_eq!(
        edit_texts(&with_inch_mark),
        vec!["    host-name router-1;", "    host-name router-2;"],
    );
    assert_eq!(
        edit_paths(&with_inch_mark),
        edit_paths(&control),
        "the change is reported inside `system`, not at the root",
    );
    assert_eq!(edit_paths(&with_inch_mark), vec![Path(vec![4, 0]); 2]);
}
