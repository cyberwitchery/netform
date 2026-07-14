//! Integration coverage for the block-header silent-miss root-cause fix.
//!
//! A block header's key hint doubles as its match key, and the diff engine's
//! `Equal` branch only re-diffs a matched block's *children* — never the two
//! headers themselves.  So two textually different headers that collided on the
//! same key hint had their header change silently dropped, yielding an empty
//! diff for a config that genuinely changed.
//!
//! These cases go beyond the per-construct patches (#91, #92): `class-map
//! match-any VOICE` vs `match-all VOICE` both key to `class-map:VOICE`, and
//! `router ospfv3 1` vs `2` both key to `router:ospfv3`.  Each fails on the
//! pre-fix code.

use netform_diff::{Diff, Edit, NormalizeOptions, diff_documents};

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

#[test]
fn iosxe_class_map_match_type_change_is_detected() {
    let before = "class-map match-any VOICE\n  match dscp ef\n";
    let after = "class-map match-all VOICE\n  match dscp ef\n";

    let left = netform_dialect_iosxe::parse_iosxe(before);
    let right = netform_dialect_iosxe::parse_iosxe(after);
    let diff = diff_documents(&left, &right, NormalizeOptions::default()).unwrap();

    assert!(
        diff.has_changes,
        "changing the class-map match type must be a detected change, not a silent no-op"
    );
    assert!(
        diff.stats.replaces >= 1,
        "the header change should surface as a Replace: {:?}",
        diff.stats
    );

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("match-any VOICE")),
        "old class-map header should appear in the diff: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("match-all VOICE")),
        "new class-map header should appear in the diff: {texts:?}"
    );
}

#[test]
fn iosxe_ospfv3_process_id_change_is_detected() {
    let before = "router ospfv3 1\n  router-id 10.0.0.1\n";
    let after = "router ospfv3 2\n  router-id 10.0.0.1\n";

    let left = netform_dialect_iosxe::parse_iosxe(before);
    let right = netform_dialect_iosxe::parse_iosxe(after);
    let diff = diff_documents(&left, &right, NormalizeOptions::default()).unwrap();

    assert!(
        diff.has_changes,
        "changing the OSPFv3 process id must be a detected change, not a silent no-op"
    );
    assert!(
        diff.stats.replaces >= 1,
        "the header change should surface as a Replace: {:?}",
        diff.stats
    );

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("ospfv3 1")),
        "old OSPFv3 header should appear in the diff: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("ospfv3 2")),
        "new OSPFv3 header should appear in the diff: {texts:?}"
    );
}
