//! Integration coverage for the numbered-ACL silent-miss fix.
//!
//! A numbered `access-list N ...` rule is an ordered sequence entry whose
//! identity is its full text, not the shared ACL number.  Before the fix these
//! lines were keyed by the ACL number alone, so two rules under the same ACL
//! shared a content key: changing a rule body left the key sequence unchanged
//! and the diff reported no change under the default (Ordered) policy.
//!
//! These tests parse real IOS XE configs and exercise the public
//! `diff_documents` entry point end to end; all three fail on the pre-fix code.

use netform_dialect_iosxe::parse_iosxe;
use netform_diff::{Diff, Edit, NormalizeOptions, diff_documents};

fn diff(a: &str, b: &str) -> Diff {
    let left = parse_iosxe(a);
    let right = parse_iosxe(b);
    // NormalizeOptions::default() uses the Ordered order policy — the default
    // path where the silent miss occurred.
    diff_documents(&left, &right, NormalizeOptions::default()).unwrap()
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

#[test]
fn numbered_acl_body_change_is_detected_under_default_policy() {
    let before = "access-list 100 permit ip 10.0.0.0 0.0.0.255 any\n\
                  access-list 100 permit tcp any any eq 22\n";
    let after = "access-list 100 permit ip 10.1.0.0 0.0.0.255 any\n\
                 access-list 100 permit tcp any any eq 22\n";

    let diff = diff(before, after);

    assert!(
        diff.has_changes,
        "changing a numbered ACL rule body must be a detected change, not a silent no-op"
    );
    assert_eq!(
        diff.stats.replaces, 1,
        "the body change should be one Replace"
    );

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("10.0.0.0")),
        "old rule body should appear in the diff: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("10.1.0.0")),
        "new rule body should appear in the diff: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("eq 22")),
        "the unchanged sibling rule must not appear in the diff: {texts:?}"
    );
}

#[test]
fn adding_a_numbered_acl_rule_is_a_single_insert() {
    let before = "access-list 100 permit ip 10.0.0.0 0.0.0.255 any\n\
                  access-list 100 deny ip any any\n";
    let after = "access-list 100 permit ip 10.0.0.0 0.0.0.255 any\n\
                 access-list 100 permit tcp any any eq 443\n\
                 access-list 100 deny ip any any\n";

    let diff = diff(before, after);

    assert!(diff.has_changes);
    assert_eq!(
        diff.edits.len(),
        1,
        "adding a rule should be exactly one edit, got {:?}",
        diff.edits
    );
    assert_eq!(diff.stats.inserts, 1, "expected a single Insert");
    assert_eq!(diff.stats.deletes, 0, "no cascade of deletes");
    assert_eq!(diff.stats.replaces, 0, "no cascade of replaces");
    assert_eq!(diff.stats.inserted_lines, 1);

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("eq 443")),
        "the actually-added rule should be the inserted line: {texts:?}"
    );
}

#[test]
fn removing_a_numbered_acl_rule_is_a_single_delete() {
    let before = "access-list 100 permit ip 10.0.0.0 0.0.0.255 any\n\
                  access-list 100 permit tcp any any eq 443\n\
                  access-list 100 deny ip any any\n";
    let after = "access-list 100 permit ip 10.0.0.0 0.0.0.255 any\n\
                 access-list 100 deny ip any any\n";

    let diff = diff(before, after);

    assert!(diff.has_changes);
    assert_eq!(
        diff.edits.len(),
        1,
        "removing a rule should be exactly one edit, got {:?}",
        diff.edits
    );
    assert_eq!(diff.stats.deletes, 1, "expected a single Delete");
    assert_eq!(diff.stats.inserts, 0, "no cascade of inserts");
    assert_eq!(diff.stats.replaces, 0, "no cascade of replaces");
    assert_eq!(diff.stats.deleted_lines, 1);

    let texts = edit_texts(&diff);
    assert!(
        texts.iter().any(|t| t.contains("eq 443")),
        "the actually-removed rule should be the deleted line: {texts:?}"
    );
}
