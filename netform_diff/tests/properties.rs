use netform_diff::{NormalizationStep, NormalizeOptions, diff_documents};
use netform_ir::{parse_generic, parse_with_dialect};
use proptest::prelude::*;

fn text_strategy() -> impl Strategy<Value = String> {
    let line = prop::string::string_regex("[ -~]{0,40}").expect("valid regex");
    prop::collection::vec(line, 0..40).prop_map(|lines| {
        if lines.is_empty() {
            String::new()
        } else {
            lines.join("\n")
        }
    })
}

/// Generate IOS-like config snippets with realistic structure.
fn ios_like_strategy() -> impl Strategy<Value = String> {
    let iface_name = prop::sample::select(vec![
        "Ethernet1",
        "Ethernet2",
        "Loopback0",
        "Vlan100",
        "mgmt0",
    ]);
    let iface_block = iface_name
        .prop_map(|name| format!("interface {name}\n  description link\n  no shutdown\n"));
    prop::collection::vec(iface_block, 1..5).prop_map(|blocks| blocks.join(""))
}

/// Generate FortiOS config snippets with realistic structure.
fn fortios_strategy() -> impl Strategy<Value = String> {
    let hostname = prop::sample::select(vec!["FGT-A", "FGT-B", "FGT-C"]);
    hostname.prop_map(|name| {
        format!("config system global\n    set hostname \"{name}\"\n    set timezone 04\nend\n")
    })
}

/// Generate Junos config snippets with realistic structure.
fn junos_strategy() -> impl Strategy<Value = String> {
    let iface_name = prop::sample::select(vec!["ge-0/0/0", "ge-0/0/1", "lo0", "ae0"]);
    iface_name.prop_map(|name| {
        format!("interfaces {{\n    {name} {{\n        description \"link\";\n    }}\n}}\n")
    })
}

proptest! {
    #[test]
    fn diff_is_deterministic(a in text_strategy(), b in text_strategy()) {
        let doc_a = parse_generic(&a);
        let doc_b = parse_generic(&b);

        let one = diff_documents(&doc_a, &doc_b, NormalizeOptions::default());
        let two = diff_documents(&doc_a, &doc_b, NormalizeOptions::default());

        prop_assert_eq!(one, two);
    }

    #[test]
    fn roundtrip_survives_random_inputs(input in text_strategy()) {
        let doc = parse_generic(&input);
        prop_assert_eq!(doc.render(), input);
    }

    // -- self-diff invariant --

    #[test]
    fn self_diff_has_no_changes_generic(input in text_strategy()) {
        let doc = parse_generic(&input);
        let diff = diff_documents(&doc, &doc, NormalizeOptions::default());
        prop_assert!(!diff.has_changes, "self-diff should report no changes");
        prop_assert!(diff.edits.is_empty(), "self-diff should have no edits");
        prop_assert_eq!(diff.stats.inserts, 0);
        prop_assert_eq!(diff.stats.deletes, 0);
        prop_assert_eq!(diff.stats.replaces, 0);
    }

    #[test]
    fn self_diff_has_no_changes_eos(input in ios_like_strategy()) {
        let dialect = netform_dialect_eos::EOS_DIALECT;
        let doc = parse_with_dialect(&input, &dialect);
        let diff = diff_documents(&doc, &doc, NormalizeOptions::default());
        prop_assert!(!diff.has_changes, "EOS self-diff should report no changes");
    }

    #[test]
    fn self_diff_has_no_changes_nxos(input in ios_like_strategy()) {
        let dialect = netform_dialect_nxos::NXOS_DIALECT;
        let doc = parse_with_dialect(&input, &dialect);
        let diff = diff_documents(&doc, &doc, NormalizeOptions::default());
        prop_assert!(!diff.has_changes, "NX-OS self-diff should report no changes");
    }

    #[test]
    fn self_diff_has_no_changes_iosxe(input in ios_like_strategy()) {
        let dialect = netform_dialect_iosxe::IOSXE_DIALECT;
        let doc = parse_with_dialect(&input, &dialect);
        let diff = diff_documents(&doc, &doc, NormalizeOptions::default());
        prop_assert!(!diff.has_changes, "IOS-XE self-diff should report no changes");
    }

    #[test]
    fn self_diff_has_no_changes_fortios(input in fortios_strategy()) {
        let dialect = netform_dialect_fortios::FortiosDialect;
        let doc = parse_with_dialect(&input, &dialect);
        let diff = diff_documents(&doc, &doc, NormalizeOptions::default());
        prop_assert!(!diff.has_changes, "FortiOS self-diff should report no changes");
    }

    #[test]
    fn self_diff_has_no_changes_junos(input in junos_strategy()) {
        let dialect = netform_dialect_junos::JunosDialect;
        let doc = parse_with_dialect(&input, &dialect);
        let diff = diff_documents(&doc, &doc, NormalizeOptions::default());
        prop_assert!(!diff.has_changes, "Junos self-diff should report no changes");
    }

    // -- normalization idempotency --

    #[test]
    fn normalization_idempotent_ignore_comments(input in text_strategy()) {
        let opts = NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]);
        let doc = parse_generic(&input);
        let diff1 = diff_documents(&doc, &doc, opts.clone());
        let diff2 = diff_documents(&doc, &doc, opts);
        prop_assert_eq!(diff1, diff2, "normalization should be idempotent");
    }

    #[test]
    fn normalization_idempotent_ignore_blanks(input in text_strategy()) {
        let opts = NormalizeOptions::new(vec![NormalizationStep::IgnoreBlankLines]);
        let doc = parse_generic(&input);
        let diff1 = diff_documents(&doc, &doc, opts.clone());
        let diff2 = diff_documents(&doc, &doc, opts);
        prop_assert_eq!(diff1, diff2, "normalization should be idempotent");
    }

    #[test]
    fn normalization_idempotent_all_steps(input in text_strategy()) {
        let opts = NormalizeOptions::new(vec![
            NormalizationStep::IgnoreComments,
            NormalizationStep::IgnoreBlankLines,
            NormalizationStep::TrimTrailingWhitespace,
            NormalizationStep::NormalizeLeadingWhitespace,
            NormalizationStep::CollapseInternalWhitespace,
        ]);
        let doc = parse_generic(&input);
        let diff1 = diff_documents(&doc, &doc, opts.clone());
        let diff2 = diff_documents(&doc, &doc, opts);
        prop_assert_eq!(diff1, diff2, "all-step normalization should be idempotent");
    }

    // -- dialect round-trips --

    #[test]
    fn eos_roundtrip(input in ios_like_strategy()) {
        let doc = parse_with_dialect(&input, &netform_dialect_eos::EOS_DIALECT);
        prop_assert_eq!(doc.render(), input, "EOS round-trip should be lossless");
    }

    #[test]
    fn nxos_roundtrip(input in ios_like_strategy()) {
        let doc = parse_with_dialect(&input, &netform_dialect_nxos::NXOS_DIALECT);
        prop_assert_eq!(doc.render(), input, "NX-OS round-trip should be lossless");
    }

    #[test]
    fn iosxe_roundtrip(input in ios_like_strategy()) {
        let doc = netform_dialect_iosxe::parse_iosxe(&input);
        prop_assert_eq!(doc.render(), input, "IOS-XE round-trip should be lossless");
    }

    #[test]
    fn fortios_roundtrip(input in fortios_strategy()) {
        let doc = netform_dialect_fortios::parse_fortios(&input);
        prop_assert_eq!(doc.render(), input, "FortiOS round-trip should be lossless");
    }

    #[test]
    fn junos_roundtrip(input in junos_strategy()) {
        let doc = netform_dialect_junos::parse_junos(&input);
        prop_assert_eq!(doc.render(), input, "Junos round-trip should be lossless");
    }
}
