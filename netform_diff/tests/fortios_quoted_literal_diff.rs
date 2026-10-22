//! a FortiOS certificate rotation is a value change inside one `edit` entry,
//! not structural churn across the file (see `fortios_literal_region` in
//! netform_dialect_fortios).

use netform_dialects::fortios::parse;
use netform_diff::{
    Diff, Edit, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    diff_documents, finding_code,
};
use netform_ir::{Dialect, Path, parse_with_dialect};

fn diff(a: &str, b: &str, options: NormalizeOptions) -> Diff {
    diff_documents(&parse(a), &parse(b), options).unwrap()
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
    // scoped to the key body line under the certificate's edit entry; the
    // firewall policy is root 1 and stays untouched.
    assert_eq!(edit_paths(&diff), vec![Path(vec![0, 0, 2]); 2]);
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

    // the certificate does not shift the root index of the sections below it
    assert_eq!(
        edit_paths(&with_certificate),
        vec![Path(vec![1, 0, 2]), Path(vec![1, 0, 2])]
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
    let options = NormalizeOptions::new(vec![
        NormalizationStep::NormalizeLeadingWhitespace,
        NormalizationStep::CollapseInternalWhitespace,
        NormalizationStep::TrimTrailingWhitespace,
        NormalizationStep::IgnoreComments,
        NormalizationStep::IgnoreBlankLines,
    ]);
    let config = |body: &str| format!("config system global\n{body}\nend\n");

    for (before, after, reported) in [
        (
            "    set comment \"top\n\thard  wrapped\nbottom\"",
            "    set comment \"top\n    hard wrapped \nbottom\"",
            &["\thard  wrapped", "    hard wrapped "][..],
        ),
        (
            "    set comments \"allow  web\"",
            "    set comments \"allow web\"",
            &[
                "    set comments \"allow  web\"",
                "    set comments \"allow web\"",
            ],
        ),
        (
            "    set comment \"top  x\nbottom\"",
            "    set comment \"top x\nbottom\"",
            &["    set comment \"top  x", "    set comment \"top x"],
        ),
        (
            "    set comment \"top   \nbottom\"",
            "    set comment \"top\nbottom\"",
            &["    set comment \"top   ", "    set comment \"top"],
        ),
        (
            "    set comments \"19\\\" rack  B\"",
            "    set comments \"19\\\" rack B\"",
            &[
                "    set comments \"19\\\" rack  B\"",
                "    set comments \"19\\\" rack B\"",
            ],
        ),
        (
            "    set member\t\"a\"   \"b\"  ",
            "    set  member \"a\" \"b\"",
            &[],
        ),
    ] {
        assert_eq!(
            edit_texts(&diff(&config(before), &config(after), options.clone())),
            reported,
            "{before:?}",
        );
    }
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
fn the_ios_family_dialects_read_a_quote_as_plain_text() {
    let collapse = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);

    for (name, dialect) in [
        ("iosxe", &netform_dialects::iosxe::DIALECT),
        ("eos", &netform_dialects::eos::DIALECT),
        ("nxos", &netform_dialects::nxos::DIALECT),
    ] {
        for quoted in [
            // FortiOS
            "    set private-key \"-----BEGIN ENCRYPTED PRIVATE KEY-----",
            // Junos
            "                certificate \"-----BEGIN CERTIFICATE-----",
        ] {
            assert_eq!(dialect.literal_region(quoted), None, "{name}: {quoted}");
        }

        let before = parse_with_dialect("interface Gi0/1\n description \"to  core\"\n", dialect);
        let after = parse_with_dialect("interface Gi0/1\n description \"to core\"\n", dialect);
        let diff = diff_documents(&before, &after, collapse.clone()).unwrap();
        assert!(!diff.has_changes, "{name}");
    }
}
