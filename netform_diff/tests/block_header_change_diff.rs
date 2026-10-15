//! a block header's key hint doubles as its match key, so textually different
//! headers can collide on one key; the engine compares matched headers
//! directly (see the engine's `diff_matched_segment`).  `class-map
//! match-any/match-all VOICE` and `router ospfv3 1/2` are such collisions.
//! tagged `router eigrp <as>` (NX-OS, EOS) and `router isis <tag>` (IOS
//! family) key on their instance id.  every case below differs only in the
//! header line.

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

/// (case, dialect parser, header before, header after, unchanged body,
/// headers collide on one key)
type Case = (
    &'static str,
    fn(&str) -> netform_ir::Document,
    &'static str,
    &'static str,
    &'static str,
    bool,
);

#[test]
fn a_header_only_change_is_one_replace_of_the_header() {
    // `collides` marks headers that share one key: they must surface as one
    // replace of the header line alone.
    let cases: &[Case] = &[
        (
            "iosxe class-map match type",
            netform_dialects::iosxe::parse,
            "class-map match-any VOICE",
            "class-map match-all VOICE",
            "\n  match dscp ef\n",
            true,
        ),
        (
            "iosxe ospfv3 process id",
            netform_dialects::iosxe::parse,
            "router ospfv3 1",
            "router ospfv3 2",
            "\n  router-id 10.0.0.1\n",
            true,
        ),
        (
            "nxos eigrp as",
            netform_dialects::nxos::parse,
            "router eigrp 100",
            "router eigrp 200",
            "\n  router-id 10.0.0.1\n",
            false,
        ),
        (
            "eos eigrp as",
            netform_dialects::eos::parse,
            "router eigrp 100",
            "router eigrp 200",
            "\n  router-id 10.0.0.1\n",
            false,
        ),
        (
            "iosxe isis tag",
            netform_dialects::iosxe::parse,
            "router isis AREA-A",
            "router isis AREA-B",
            "\n  net 49.0001.0000.0000.0001.00\n",
            false,
        ),
    ];
    for &(name, parse, before, after, body, collides) in cases {
        let left = parse(&format!("{before}{body}"));
        let right = parse(&format!("{after}{body}"));
        let diff = diff_documents(&left, &right, NormalizeOptions::default()).unwrap();

        let texts = edit_texts(&diff);
        if collides {
            assert_eq!(texts, vec![before, after], "{name}");
            assert_eq!(diff.stats.replaces, 1, "{name}: {:?}", diff.stats);
        } else {
            assert!(diff.has_changes, "{name}");
            assert!(
                texts.iter().any(|t| t.contains(before)),
                "{name}: {texts:?}"
            );
            assert!(texts.iter().any(|t| t.contains(after)), "{name}: {texts:?}");
        }
    }
}
