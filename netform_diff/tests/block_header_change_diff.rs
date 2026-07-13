//! Integration coverage for the block-header silent-miss root-cause fix.
//!
//! A block header's key hint doubles as its match key, and the diff engine's
//! `Equal` branch used to re-diff only a matched block's *children* — never the
//! two headers themselves.  So two textually different headers that collided on
//! the same (lossy) key hint had their header change silently dropped, yielding
//! an empty diff for a config that genuinely changed.
//!
//! These cases exercise collisions the per-construct key_hint patches (#91, #92)
//! do not cover, proving the engine-level fix catches the whole family:
//!   * `class-map match-any VOICE` vs `class-map match-all VOICE` collapse to
//!     `class-map:VOICE`, discarding the OR->AND matching semantics.
//!   * `router ospfv3 1` vs `router ospfv3 2` collapse to `router:ospfv3`,
//!     discarding the process id (ospfv3 is not a keyed router protocol).

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
    // match-any (OR) -> match-all (AND) is a security-relevant matching change,
    // but both headers key to `class-map:VOICE`.  The child is unchanged, so the
    // header edit is the only signal that anything changed.
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
    // ospfv3 is not one of the keyed router protocols, so `router ospfv3 1` and
    // `router ospfv3 2` both collapse to `router:ospfv3`, dropping the id.
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
