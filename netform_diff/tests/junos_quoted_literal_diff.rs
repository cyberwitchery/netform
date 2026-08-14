//! a Junos certificate rotation is a value change inside the block that holds
//! it, not structural churn across the file (see `junos_literal_region` in
//! netform_dialect_junos).

use netform_dialect_junos::parse_junos;
use netform_diff::{
    Diff, Edit, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    diff_documents, finding_code,
};
use netform_ir::{Dialect, Path};

fn diff(a: &str, b: &str, options: NormalizeOptions) -> Diff {
    diff_documents(&parse_junos(a), &parse_junos(b), options).unwrap()
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

const CERTIFICATE_AND_INTERFACES: &str = "\
security {
    certificates {
        local {
            SSL-CERT {
                certificate \"-----BEGIN CERTIFICATE-----
MIIDXTCCAkWgAwIBAgIJAKL0UG+mRkSP
hkiG9w0BBQwwDgQIabcd+/EFGH==
-----END CERTIFICATE-----\";
            }
        }
    }
}
interfaces {
    ge-0/0/0 {
        description \"uplink to core\";
        unit 0 {
            family inet {
                address 10.0.0.1/30;
            }
        }
    }
}
";

#[test]
fn a_rotated_certificate_line_is_the_only_change_reported() {
    let after = CERTIFICATE_AND_INTERFACES.replace(
        "hkiG9w0BBQwwDgQIabcd+/EFGH==",
        "hkiG9w0BBQwwDgQIzyxw9/8VUTS=",
    );

    let diff = diff(CERTIFICATE_AND_INTERFACES, &after, keyed_stable());

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
fn the_change_is_scoped_to_the_block_holding_the_certificate() {
    let after = CERTIFICATE_AND_INTERFACES.replace(
        "hkiG9w0BBQwwDgQIabcd+/EFGH==",
        "hkiG9w0BBQwwDgQIzyxw9/8VUTS=",
    );

    let paths = edit_paths(&diff(CERTIFICATE_AND_INTERFACES, &after, keyed_stable()));

    assert!(!paths.is_empty());
    for path in &paths {
        assert_eq!(
            path.0.first(),
            Some(&0),
            "`interfaces` is root 1 and must not be touched: {paths:?}",
        );
        assert!(
            path.0.len() >= 5,
            "a certificate body line sits under `SSL-CERT`, not at the root: {paths:?}",
        );
    }
}

#[test]
fn a_longer_certificate_adds_one_contained_line() {
    let after = CERTIFICATE_AND_INTERFACES.replace(
        "-----END CERTIFICATE-----\";",
        "QklTM1RyYWlsaW5nQmxvY2s9PQ==\n-----END CERTIFICATE-----\";",
    );

    let diff = diff(CERTIFICATE_AND_INTERFACES, &after, keyed_stable());

    assert_eq!(edit_texts(&diff), vec!["QklTM1RyYWlsaW5nQmxvY2s9PQ=="]);
    assert_eq!(edit_paths(&diff), vec![Path(vec![0, 0, 0, 0, 3])]);
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
fn an_unchanged_certificate_reports_no_changes() {
    assert!(
        !diff(
            CERTIFICATE_AND_INTERFACES,
            CERTIFICATE_AND_INTERFACES,
            keyed_stable(),
        )
        .has_changes
    );
}

#[test]
fn an_edit_below_a_certificate_diffs_as_it_would_without_one() {
    const NO_CERTIFICATE: &str = "\
interfaces {
    ge-0/0/0 {
        description \"uplink to core\";
        unit 0 {
            family inet {
                address 10.0.0.1/30;
            }
        }
    }
}
";

    let with_certificate = diff(
        CERTIFICATE_AND_INTERFACES,
        &CERTIFICATE_AND_INTERFACES.replace("10.0.0.1/30", "10.0.0.5/30"),
        keyed_stable(),
    );
    let baseline = diff(
        NO_CERTIFICATE,
        &NO_CERTIFICATE.replace("10.0.0.1/30", "10.0.0.5/30"),
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
fn normalization_never_masks_a_change_inside_a_quoted_value() {
    let before =
        "system {\n    login {\n        announcement \"top\n\thard  wrapped\nbottom\";\n    }\n}\n";
    let after = "system {\n    login {\n        announcement \"top\n    hard wrapped \nbottom\";\n    }\n}\n";

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
    let before = "system {\n    login {\n        announcement \"Authorized use only\n## contact ops@example.com\n\";\n    }\n}\n";
    let after = before.replace("ops@example.com", "noc@example.com");

    let options = NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]);

    let diff = diff(before, &after, options);
    assert!(diff.has_changes);
    assert_eq!(
        edit_texts(&diff),
        vec!["## contact ops@example.com", "## contact noc@example.com"],
    );
}

#[test]
fn the_ios_family_dialects_open_no_region_on_a_junos_quoted_line() {
    let quoted = "                certificate \"-----BEGIN CERTIFICATE-----";

    assert!(
        netform_dialect_junos::JunosDialect
            .literal_region(quoted)
            .is_some()
    );
    assert_eq!(
        netform_dialect_iosxe::IOSXE_DIALECT.literal_region(quoted),
        None,
    );
    assert_eq!(
        netform_dialect_eos::EOS_DIALECT.literal_region(quoted),
        None
    );
    assert_eq!(
        netform_dialect_nxos::NXOS_DIALECT.literal_region(quoted),
        None,
    );
}
