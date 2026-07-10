//! Integration coverage for the tagged router-instance silent-miss fix.
//!
//! A block header's key hint doubles as its match key, and the diff engine's
//! `Equal` branch only re-diffs a matched block's *children* — never the two
//! headers themselves.  So two textually different headers that collided on the
//! same key hint had their header change silently dropped, yielding an empty
//! diff for a config that genuinely changed.
//!
//! Tagged `router eigrp <as>` (NX-OS, EOS) and `router isis <tag>` (IOS family)
//! used to collapse to `router:eigrp` / `router:isis`, discarding the instance
//! id.  These tests parse configs that differ only in that id and assert the
//! diff is non-empty; each fails on the pre-fix code.

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
fn nxos_eigrp_as_change_is_detected() {
    let before = "router eigrp 100\n  router-id 10.0.0.1\n";
    let after = "router eigrp 200\n  router-id 10.0.0.1\n";

    let left = netform_dialect_nxos::parse_nxos(before);
    let right = netform_dialect_nxos::parse_nxos(after);
    let diff = diff_documents(&left, &right, NormalizeOptions::default()).unwrap();

    assert!(
        diff.has_changes,
        "changing the EIGRP AS must be a detected change, not a silent no-op"
    );

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("eigrp 100")),
        "old EIGRP header should appear in the diff: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("eigrp 200")),
        "new EIGRP header should appear in the diff: {texts:?}"
    );
}

#[test]
fn eos_eigrp_as_change_is_detected() {
    let before = "router eigrp 100\n  router-id 10.0.0.1\n";
    let after = "router eigrp 200\n  router-id 10.0.0.1\n";

    let left = netform_dialect_eos::parse_eos(before);
    let right = netform_dialect_eos::parse_eos(after);
    let diff = diff_documents(&left, &right, NormalizeOptions::default()).unwrap();

    assert!(
        diff.has_changes,
        "changing the EIGRP AS must be a detected change, not a silent no-op"
    );

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("eigrp 100")),
        "old EIGRP header should appear in the diff: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("eigrp 200")),
        "new EIGRP header should appear in the diff: {texts:?}"
    );
}

#[test]
fn iosxe_isis_tag_change_is_detected() {
    let before = "router isis AREA-A\n  net 49.0001.0000.0000.0001.00\n";
    let after = "router isis AREA-B\n  net 49.0001.0000.0000.0001.00\n";

    let left = netform_dialect_iosxe::parse_iosxe(before);
    let right = netform_dialect_iosxe::parse_iosxe(after);
    let diff = diff_documents(&left, &right, NormalizeOptions::default()).unwrap();

    assert!(
        diff.has_changes,
        "changing the IS-IS tag must be a detected change, not a silent no-op"
    );

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("isis AREA-A")),
        "old IS-IS header should appear in the diff: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("isis AREA-B")),
        "new IS-IS header should appear in the diff: {texts:?}"
    );
}
