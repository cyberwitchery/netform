//! a FortiOS certificate rotation is a value change inside one `edit` entry,
//! not structural churn across the file (see `fortios_literal_region` in
//! netform_dialect_fortios).

use netform_dialect_fortios::parse_fortios;
use netform_diff::{
    Diff, Edit, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    diff_documents, finding_code,
};
use netform_ir::{Dialect, LiteralTerminator, Path};

fn diff(a: &str, b: &str, options: NormalizeOptions) -> Diff {
    diff_documents(&parse_fortios(a), &parse_fortios(b), options).unwrap()
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

const CERTIFICATE_AND_POLICY: &str = "\
config vpn certificate local
    edit \"Fortinet_CA\"
        set private-key \"-----BEGIN ENCRYPTED PRIVATE KEY-----
MIIFDjBABgkqhkiG9w0BBQ0wMzAbBgkq
hkiG9w0BBQwwDgQIabcd+/EFGH==
-----END ENCRYPTED PRIVATE KEY-----\"
        set range global
    next
end
config firewall policy
    edit 1
        set srcintf \"port1\"
        set dstintf \"port2\"
        set action accept
    next
end
";

#[test]
fn a_rotated_key_line_is_the_only_change_reported() {
    let after = CERTIFICATE_AND_POLICY.replace(
        "hkiG9w0BBQwwDgQIabcd+/EFGH==",
        "hkiG9w0BBQwwDgQIzyxw9/8VUTS=",
    );

    let diff = diff(CERTIFICATE_AND_POLICY, &after, keyed_stable());

    assert!(diff.has_changes);
    let mut texts = edit_texts(&diff);
    texts.sort();
    assert_eq!(
        texts,
        vec![
            "hkiG9w0BBQwwDgQIabcd+/EFGH==",
            "hkiG9w0BBQwwDgQIzyxw9/8VUTS=",
        ],
    );
    assert!(diff.findings.is_empty(), "{:?}", diff.findings);
}

#[test]
fn the_change_is_scoped_to_the_certificate_entry() {
    let after = CERTIFICATE_AND_POLICY.replace(
        "hkiG9w0BBQwwDgQIabcd+/EFGH==",
        "hkiG9w0BBQwwDgQIzyxw9/8VUTS=",
    );

    let diff = diff(CERTIFICATE_AND_POLICY, &after, keyed_stable());

    let paths = edit_paths(&diff);
    assert!(!paths.is_empty());
    for path in &paths {
        assert_eq!(
            path.0.first(),
            Some(&0),
            "the firewall policy is root 1 and must not be touched: {paths:?}",
        );
        assert!(
            path.0.len() >= 3,
            "a key body line is nested under the edit entry, not a root sibling: {paths:?}",
        );
    }
}

#[test]
fn a_longer_key_adds_one_contained_line() {
    let after = CERTIFICATE_AND_POLICY.replace(
        "-----END ENCRYPTED PRIVATE KEY-----\"",
        "QklTM1RyYWlsaW5nQmxvY2s9PQ==\n-----END ENCRYPTED PRIVATE KEY-----\"",
    );

    let diff = diff(CERTIFICATE_AND_POLICY, &after, keyed_stable());

    assert_eq!(edit_texts(&diff), vec!["QklTM1RyYWlsaW5nQmxvY2s9PQ=="]);
    assert_eq!(edit_paths(&diff), vec![Path(vec![0, 0, 3])]);
    assert!(
        !diff
            .findings
            .iter()
            .any(|finding| finding.code == finding_code::DIFF_UNRELIABLE_REGION),
        "{:?}",
        diff.findings,
    );
}

#[test]
fn a_certificate_does_not_shift_the_root_index_of_the_sections_below_it() {
    let with_certificate = diff(
        CERTIFICATE_AND_POLICY,
        &CERTIFICATE_AND_POLICY.replace("set action accept", "set action deny"),
        keyed_stable(),
    );

    assert_eq!(
        edit_paths(&with_certificate),
        vec![Path(vec![1, 0, 2]), Path(vec![1, 0, 2]),]
    );
}

#[test]
fn an_unrelated_edit_below_the_certificate_diffs_as_it_would_without_one() {
    const NO_CERTIFICATE: &str = "\
config firewall policy
    edit 1
        set srcintf \"port1\"
        set dstintf \"port2\"
        set action accept
    next
end
";

    let with_certificate = diff(
        CERTIFICATE_AND_POLICY,
        &CERTIFICATE_AND_POLICY.replace("set action accept", "set action deny"),
        keyed_stable(),
    );
    let baseline = diff(
        NO_CERTIFICATE,
        &NO_CERTIFICATE.replace("set action accept", "set action deny"),
        keyed_stable(),
    );

    assert_eq!(edit_texts(&with_certificate), edit_texts(&baseline));
    assert!(
        !with_certificate
            .findings
            .iter()
            .any(|finding| finding.code == finding_code::AMBIGUOUS_KEY_MATCH),
        "{:?}",
        with_certificate.findings,
    );
}

#[test]
fn identical_configs_with_a_certificate_report_no_changes() {
    assert!(
        !diff(
            CERTIFICATE_AND_POLICY,
            CERTIFICATE_AND_POLICY,
            keyed_stable()
        )
        .has_changes
    );
}

#[test]
fn normalization_never_masks_a_change_inside_a_quoted_value() {
    let before = "config system global\n    set comment \"top\n\thard  wrapped\nbottom\"\nend\n";
    let after = "config system global\n    set comment \"top\n    hard wrapped \nbottom\"\nend\n";

    let options = NormalizeOptions::new(vec![
        NormalizationStep::NormalizeLeadingWhitespace,
        NormalizationStep::CollapseInternalWhitespace,
        NormalizationStep::TrimTrailingWhitespace,
        NormalizationStep::IgnoreComments,
        NormalizationStep::IgnoreBlankLines,
    ]);

    assert!(diff(before, after, options).has_changes);
}

#[test]
fn a_hash_line_inside_a_quoted_value_survives_ignore_comments() {
    let before = "config system replacemsg webproxy \"deny\"\n    set buffer \"<style>\n#banner { color: red; }\n</style>\"\nend\n";
    let after = before.replace("color: red", "color: blue");

    let options = NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]);

    let diff = diff(before, &after, options);
    assert!(diff.has_changes);
    assert_eq!(
        edit_texts(&diff),
        vec!["#banner { color: red; }", "#banner { color: blue; }"],
    );
}

#[test]
fn the_other_dialects_open_no_region_on_a_quoted_line() {
    let quoted = "    set private-key \"-----BEGIN ENCRYPTED PRIVATE KEY-----";
    let dialects: Vec<(&str, Option<LiteralTerminator>)> = vec![
        (
            "junos",
            netform_dialect_junos::JunosDialect.literal_region(quoted),
        ),
        (
            "iosxe",
            netform_dialect_iosxe::IOSXE_DIALECT.literal_region(quoted),
        ),
        (
            "eos",
            netform_dialect_eos::EOS_DIALECT.literal_region(quoted),
        ),
        (
            "nxos",
            netform_dialect_nxos::NXOS_DIALECT.literal_region(quoted),
        ),
    ];

    for (name, region) in dialects {
        assert_eq!(region, None, "{name}");
    }
}
