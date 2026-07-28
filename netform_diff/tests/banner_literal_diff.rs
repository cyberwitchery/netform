//! Integration coverage for IOS-family banner bodies as literal regions.
//!
//! Both symptoms below reproduce on the pre-fix code: banner text was classified,
//! tokenized and key-hinted as if it were configuration, so a `!`-prefixed
//! banner line was dropped by `--ignore-comments` and a banner line reading
//! `interface …` collided with the identity of the real interface.

use netform_dialect_iosxe::parse_iosxe;
use netform_diff::{
    Diff, Edit, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    diff_documents, finding_code,
};

fn diff(a: &str, b: &str, options: NormalizeOptions) -> Diff {
    diff_documents(&parse_iosxe(a), &parse_iosxe(b), options).unwrap()
}

fn ignore_comments() -> NormalizeOptions {
    NormalizeOptions::new(vec![NormalizationStep::IgnoreComments])
}

fn keyed_stable() -> NormalizeOptions {
    NormalizeOptions {
        order_policy: OrderPolicyConfig {
            default: OrderPolicy::KeyedStable,
            overrides: Vec::new(),
        },
        ..NormalizeOptions::default()
    }
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

const BANNER_WITH_BANG: &str = "\
hostname edge-1
!
banner motd ^C
Authorized use only
! contact noc@example.net
^C
!
interface GigabitEthernet0/0/0
  description WAN uplink
";

#[test]
fn banner_text_change_survives_ignore_comments() {
    let after = BANNER_WITH_BANG.replace("! contact noc@example.net", "! contact soc@example.net");

    let diff = diff(BANNER_WITH_BANG, &after, ignore_comments());

    assert!(diff.has_changes, "banner text change must be visible");
    assert_eq!(
        edit_texts(&diff),
        vec!["! contact noc@example.net", "! contact soc@example.net"],
    );
}

#[test]
fn real_comments_are_still_dropped_by_ignore_comments() {
    let after = BANNER_WITH_BANG.replace("hostname edge-1\n!\n", "hostname edge-1\n! new note\n");

    let diff = diff(BANNER_WITH_BANG, &after, ignore_comments());

    assert!(!diff.has_changes);
}

#[test]
fn identical_configs_with_banners_report_no_changes() {
    let diff = diff(BANNER_WITH_BANG, BANNER_WITH_BANG, ignore_comments());
    assert!(!diff.has_changes);
}

const BANNER_WITH_GLUED_DELIMITER: &str = "\
banner motd #Warning restricted
! Authorized use only
#
interface GigabitEthernet0/0/0
  description WAN uplink
";

#[test]
fn glued_single_character_delimiter_banner_survives_ignore_comments() {
    let after = BANNER_WITH_GLUED_DELIMITER.replace("Authorized use only", "CHANGED banner text");

    let diff = diff(BANNER_WITH_GLUED_DELIMITER, &after, ignore_comments());

    assert!(diff.has_changes, "banner text change must be visible");
    assert_eq!(
        edit_texts(&diff),
        vec!["! Authorized use only", "! CHANGED banner text"],
    );
}

const BANNER_SHADOWING_AN_INTERFACE: &str = "\
banner motd ^C
Notice to operators:
interface GigabitEthernet0/0/0
  is the WAN port, do not shut
^C
interface GigabitEthernet0/0/0
  description WAN uplink
";

const NO_BANNER: &str = "\
interface GigabitEthernet0/0/0
  description WAN uplink
";

#[test]
fn banner_text_naming_an_interface_does_not_collide_with_it() {
    let after = BANNER_SHADOWING_AN_INTERFACE.replace("WAN uplink", "WAN backup");

    let shadowed = diff(BANNER_SHADOWING_AN_INTERFACE, &after, keyed_stable());

    assert!(
        !shadowed
            .findings
            .iter()
            .any(|finding| finding.code == finding_code::AMBIGUOUS_KEY_MATCH),
        "banner body must not claim the real interface's identity: {:?}",
        shadowed.findings,
    );

    // the real interface's edits are exactly what the same change produces with
    // no banner in the file at all.
    let baseline = diff(
        NO_BANNER,
        &NO_BANNER.replace("WAN uplink", "WAN backup"),
        keyed_stable(),
    );
    assert_eq!(edit_texts(&shadowed), edit_texts(&baseline));
    assert_eq!(shadowed.findings, baseline.findings);
}

#[test]
fn banner_body_change_is_reported_under_keyed_stable() {
    let after = BANNER_SHADOWING_AN_INTERFACE.replace("do not shut", "do not disable");

    let shadowed = diff(BANNER_SHADOWING_AN_INTERFACE, &after, keyed_stable());

    assert!(shadowed.has_changes);
    assert!(
        edit_texts(&shadowed)
            .iter()
            .any(|text| text.contains("do not disable")),
        "changed banner line must appear in the diff: {:?}",
        edit_texts(&shadowed),
    );
}

#[test]
fn whitespace_normalization_does_not_mask_banner_text_changes() {
    // a tab and four spaces normalize to the same indent, and a doubled inner
    // space collapses — inside a banner all three are real text changes.
    let before = "banner motd ^C\n\tascii  art\n^C\n";
    let after = "banner motd ^C\n    ascii art \n^C\n";

    let options = NormalizeOptions::new(vec![
        NormalizationStep::NormalizeLeadingWhitespace,
        NormalizationStep::CollapseInternalWhitespace,
        NormalizationStep::TrimTrailingWhitespace,
    ]);

    assert!(diff(before, after, options).has_changes);
}

#[test]
fn ignore_blank_lines_keeps_a_blank_banner_line() {
    let before = "banner motd ^C\ntop\n\nbottom\n^C\n";
    let after = "banner motd ^C\ntop\nbottom\n^C\n";

    let options = NormalizeOptions::new(vec![NormalizationStep::IgnoreBlankLines]);

    assert!(diff(before, after, options).has_changes);
}
