use netform_dialects::fortios::parse as parse_fortios;
use netform_dialects::iosxe::parse as parse_iosxe;
use netform_dialects::iosxr::parse as parse_iosxr;
use netform_dialects::junos::parse as parse_junos;
use netform_ir::{Path, Span, parse_generic};

use super::{
    Diff, DiffLine, Edit, EditAnchor, NormalizationStep, NormalizeOptions, OrderPolicy,
    OrderPolicyConfig, OrderPolicyOverride, PlanAction, PlanLineEditKind, build_comparison_view,
    build_plan, diff_documents,
};

#[test]
fn ignores_comments_when_configured() {
    let a = parse_generic("! generated\ninterface Ethernet1\n");
    let b = parse_generic("! changed comment\ninterface Ethernet1\n");

    let diff = diff_documents(
        &a,
        &b,
        NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]),
    )
    .unwrap();

    assert!(diff.edits.is_empty());
}

#[test]
fn records_applied_normalization_steps() {
    let a = parse_generic("line  a\n");
    let b = parse_generic("line a\n");
    let options = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);

    let diff = diff_documents(&a, &b, options).unwrap();
    assert_eq!(
        diff.normalization_steps,
        vec![NormalizationStep::CollapseInternalWhitespace]
    );
}

#[test]
fn collapse_internal_whitespace_ignores_single_tab_separator() {
    let a = parse_generic("interface\tGi0/1\n");
    let b = parse_generic("interface Gi0/1\n");
    let options = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);

    let diff = diff_documents(&a, &b, options).unwrap();
    assert!(!diff.has_changes);
    assert!(diff.edits.is_empty());
}

#[test]
fn block_aware_diff_only_reports_changed_children() {
    let a = parse_generic("interface Ethernet1\n  description old\n  mtu 9000\n");
    let b = parse_generic("interface Ethernet1\n  description new\n  mtu 9000\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    assert_eq!(diff.edits.len(), 1);

    match &diff.edits[0] {
        Edit::Replace {
            old_lines,
            new_lines,
            ..
        } => {
            assert_eq!(old_lines.len(), 1);
            assert_eq!(new_lines.len(), 1);
            assert_eq!(old_lines[0].text, "  description old");
            assert_eq!(new_lines[0].text, "  description new");
        }
        _ => panic!("expected a replace edit"),
    }
}

#[test]
fn ambiguous_duplicate_lines_create_findings() {
    let a = parse_generic("line\nline\nline\n");
    let b = parse_generic("line\nline\nline\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    assert!(!diff.has_changes);
    let [finding] = &diff.findings[..] else {
        panic!("expected one finding, got {:?}", diff.findings);
    };
    assert_eq!(finding.code, crate::finding_code::AMBIGUOUS_KEY_MATCH);
    assert!(
        finding
            .message
            .ends_with("appears 3x on left and 3x on right"),
        "{}",
        finding.message
    );
}

#[test]
fn fallback_alignment_emits_finding() {
    let a = parse_generic("interface Ethernet1\n  description one\n");
    let b = parse_generic("router bgp 65000\n  neighbor 10.0.0.1 remote-as 65001\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let unreliable = diff
        .findings
        .iter()
        .filter(|f| f.code == crate::finding_code::DIFF_UNRELIABLE_REGION)
        .map(|f| f.path.clone())
        .collect::<Vec<_>>();
    assert_eq!(unreliable, [Some(Path(vec![0]))]);
}

#[test]
fn parse_uncertainty_is_exposed_as_finding() {
    let a = parse_generic("  orphan-line\n");
    let b = parse_generic("  orphan-line\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    assert!(
        diff.findings
            .iter()
            .any(|f| f.code == "unknown_unparsed_construct")
    );
}

#[test]
fn build_plan_replace_of_sibling_lines_deletes_them_and_inserts_under_their_context() {
    let anchor = EditAnchor {
        path: Path(vec![0, 1]),
        span: Span {
            line: 2,
            start_byte: 10,
            end_byte: 27,
        },
    };
    let diff = Diff {
        edits: vec![Edit::Replace {
            old_at_key: Some(1),
            new_at_key: Some(2),
            left_anchor: Some(anchor),
            right_anchor: None,
            old_lines: vec![
                DiffLine {
                    content_key: 1,
                    occurrence_key: 1,
                    text: "  description old-a".to_string(),
                    path: Path(vec![0, 1]),
                    span: Span {
                        line: 2,
                        start_byte: 10,
                        end_byte: 29,
                    },
                },
                DiffLine {
                    content_key: 11,
                    occurrence_key: 11,
                    text: "  description old-b".to_string(),
                    path: Path(vec![0, 2]),
                    span: Span {
                        line: 3,
                        start_byte: 30,
                        end_byte: 49,
                    },
                },
            ],
            new_lines: vec![DiffLine {
                content_key: 2,
                occurrence_key: 2,
                text: "  description merged".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 30,
                },
            }],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert!(plan.findings.is_empty());
    assert_eq!(plan.actions.len(), 1);
    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext {
            context_path,
            line_edits,
        } => {
            assert_eq!(context_path.0, vec![0]);
            let edits = line_edits
                .iter()
                .map(|edit| (edit.kind.clone(), edit.text.as_str()))
                .collect::<Vec<_>>();
            assert_eq!(
                edits,
                [
                    (PlanLineEditKind::Delete, "  description old-a"),
                    (PlanLineEditKind::Delete, "  description old-b"),
                    (PlanLineEditKind::Insert, "  description merged"),
                ]
            );
        }
        other => panic!("expected line edits under [0], got {other:?}"),
    }
}

#[test]
fn build_plan_mixed_edit_types_group_under_same_context() {
    let mk_anchor = |idx: usize| EditAnchor {
        path: Path(vec![0, idx]),
        span: Span {
            line: idx + 1,
            start_byte: idx * 20,
            end_byte: idx * 20 + 15,
        },
    };
    let mk_line = |key: u64, idx: usize, text: &str| DiffLine {
        content_key: key,
        occurrence_key: key,
        text: text.to_string(),
        path: Path(vec![0, idx]),
        span: Span {
            line: idx + 1,
            start_byte: idx * 20,
            end_byte: idx * 20 + text.len(),
        },
    };

    let diff = Diff {
        edits: vec![
            Edit::Delete {
                at_key: Some(1),
                left_anchor: Some(mk_anchor(1)),
                right_anchor: None,
                lines: vec![mk_line(1, 1, "  no shutdown")],
            },
            Edit::Insert {
                at_key: Some(2),
                left_anchor: Some(EditAnchor {
                    path: Path(vec![0]),
                    span: Span {
                        line: 1,
                        start_byte: 0,
                        end_byte: 9,
                    },
                }),
                right_anchor: Some(mk_anchor(2)),
                lines: vec![mk_line(2, 2, "  shutdown")],
            },
            Edit::Replace {
                old_at_key: Some(3),
                new_at_key: Some(4),
                left_anchor: Some(mk_anchor(3)),
                right_anchor: None,
                old_lines: vec![mk_line(3, 3, "  mtu 9000")],
                new_lines: vec![mk_line(4, 3, "  mtu 9216")],
            },
        ],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.actions.len(), 1);
    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext {
            context_path,
            line_edits,
        } => {
            assert_eq!(context_path.0, vec![0]);
            let edits = line_edits
                .iter()
                .map(|edit| (edit.kind.clone(), edit.text.as_str()))
                .collect::<Vec<_>>();
            assert_eq!(
                edits,
                [
                    (PlanLineEditKind::Delete, "  no shutdown"),
                    (PlanLineEditKind::Replace, "  mtu 9216"),
                    (PlanLineEditKind::Insert, "  shutdown"),
                ]
            );
        }
        _ => panic!("expected single grouped line-edit action"),
    }
}

#[test]
fn receives_key_hints_from_dialect_documents() {
    let doc = parse_iosxe("interface Ethernet1\n  description uplink\n");
    let view = build_comparison_view(&doc, &NormalizeOptions::default());
    let first = view.lines.first().expect("first comparison line");
    assert_eq!(first.key_hint.as_deref(), Some("interface:Ethernet1"));
}

#[test]
fn emits_finding_for_ambiguous_extracted_stanza_keys() {
    let a =
        parse_iosxe("interface Ethernet1\n  description a\ninterface Ethernet1\n  description b\n");
    let b =
        parse_iosxe("interface Ethernet1\n  description a\ninterface Ethernet1\n  description c\n");

    let diff = diff_documents(
        &a,
        &b,
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: OrderPolicy::KeyedStable,
            overrides: Vec::new(),
        }),
    )
    .unwrap();

    assert!(diff.findings.iter().any(|f| {
        f.code == "ambiguous_key_match" && f.message.contains("ambiguous extracted key")
    }));
}

// three-policy comparison tests on parsed documents; see the view-level
// counterpart section in engine.rs.

#[test]
fn reordered_block_children_only_changed_under_ordered_policy() {
    // identical block children in swapped order.  Ordered treats this as
    // drift; Unordered and KeyedStable do not.
    let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n  no shutdown\n");
    let b = parse_generic("interface Ethernet1\n  no shutdown\n  description uplink\n  mtu 9000\n");

    let opts = |policy| {
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: policy,
            overrides: Vec::new(),
        })
    };

    let ordered = diff_documents(&a, &b, opts(OrderPolicy::Ordered)).unwrap();
    let unordered = diff_documents(&a, &b, opts(OrderPolicy::Unordered)).unwrap();
    let keyed = diff_documents(&a, &b, opts(OrderPolicy::KeyedStable)).unwrap();

    assert!(ordered.has_changes, "Ordered detects reordering as drift");
    assert!(!ordered.edits.is_empty());

    assert!(!unordered.has_changes, "Unordered ignores child reordering");
    assert!(unordered.edits.is_empty());

    assert!(!keyed.has_changes, "KeyedStable ignores child reordering");
    assert!(keyed.edits.is_empty());
}

#[test]
fn fortios_set_value_change_is_a_replace_unless_unordered() {
    // FortiOS `set` commands produce key_hints (e.g. `set:hostname`), which
    // give lines a stable content_key independent of the actual value.
    // nothing is reordered, so the changed value is the only drift to find
    // (0.8.0 regression for `set hostname`).
    //
    //   Ordered, KeyedStable → pair the two lines → Replace
    //   Unordered            → hashes normalized text → different buckets → Delete + Insert
    let a = parse_fortios("config system global\n    set hostname \"edge-1\"\nend\n");
    let b = parse_fortios("config system global\n    set hostname \"edge-2\"\nend\n");
    let replace = vec![(
        "replace",
        vec!["set hostname \"edge-1\"", "set hostname \"edge-2\""],
    )];
    let delete_insert = vec![
        ("delete", vec!["set hostname \"edge-1\""]),
        ("insert", vec!["set hostname \"edge-2\""]),
    ];

    for (options, expected) in [
        (NormalizeOptions::default(), &replace),
        (with_policy(OrderPolicy::Ordered, Vec::new()), &replace),
        (with_policy(OrderPolicy::KeyedStable, Vec::new()), &replace),
        (
            with_policy(OrderPolicy::Unordered, Vec::new()),
            &delete_insert,
        ),
    ] {
        let policy = options.order_policy.default;
        let diff = diff_documents(&a, &b, options).unwrap();
        assert!(diff.has_changes, "{policy:?}");
        assert_eq!(&edit_shapes(&diff), expected, "{policy:?}");
    }
}

#[test]
fn junos_set_value_change_reported_under_default_policy() {
    // Junos keys `set` lines by path, so this pair matches at the top level.
    let a = parse_junos("set system host-name edge-1\n");
    let b = parse_junos("set system host-name edge-2\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    assert!(diff.has_changes);

    let [
        Edit::Replace {
            old_lines,
            new_lines,
            ..
        },
    ] = &diff.edits[..]
    else {
        panic!("expected a single Replace, got {:?}", diff.edits);
    };
    assert_eq!(old_lines[0].text, "set system host-name edge-1");
    assert_eq!(new_lines[0].text, "set system host-name edge-2");
}

#[test]
fn fortios_reorder_plus_value_change_three_way_contrast() {
    // two `set` fields under the same block: reordered AND one value changes.
    //
    // Ordered  → reports the moved hostname line as Delete + Insert.
    // Unordered → ignores reorder; sees value change as Delete + Insert.
    // KeyedStable → ignores reorder; sees value change as Replace.
    let a = parse_fortios(
        "config system global\n    set hostname \"edge-1\"\n    set timezone \"UTC\"\nend\n",
    );
    let b = parse_fortios(
        "config system global\n    set timezone \"UTC\"\n    set hostname \"edge-2\"\nend\n",
    );
    let delete_insert = vec![
        ("delete", vec!["set hostname \"edge-1\""]),
        ("insert", vec!["set hostname \"edge-2\""]),
    ];
    let replace = vec![(
        "replace",
        vec!["set hostname \"edge-1\"", "set hostname \"edge-2\""],
    )];

    for (policy, expected) in [
        (OrderPolicy::Ordered, &delete_insert),
        (OrderPolicy::Unordered, &delete_insert),
        (OrderPolicy::KeyedStable, &replace),
    ] {
        let diff = diff_documents(&a, &b, with_policy(policy, Vec::new())).unwrap();
        assert!(diff.has_changes, "{policy:?}");
        assert_eq!(&edit_shapes(&diff), expected, "{policy:?}");
    }
}

#[test]
fn fortios_block_footer_participates_in_comparison_and_diff() {
    // FortiOS `end` is captured as the config block's footer, so
    // it flows through the flattener as a KeyKind::BlockFooter line (keyed under
    // the block header, at path [block, children_len]) and takes part in diffs.
    let closed = parse_fortios("config system global\n    set hostname \"x\"\nend\n");

    let view = build_comparison_view(&closed, &NormalizeOptions::default());
    let footer = view.lines.last().expect("comparison view is non-empty");
    assert_eq!(footer.normalized, "end");
    // footer path is [config-block-index, children_len] == [0, 1].
    assert_eq!(footer.path, Path(vec![0, 1]));

    // dropping the `end` (leaving the block open) is a real, detected change.
    let open = parse_fortios("config system global\n    set hostname \"x\"\n");
    let diff = diff_documents(&closed, &open, NormalizeOptions::default()).unwrap();
    assert!(
        diff.has_changes,
        "the footer terminator should participate in the diff",
    );
    let mentions_end = diff.edits.iter().any(|e| match e {
        Edit::Delete { lines, .. } | Edit::Insert { lines, .. } => {
            lines.iter().any(|l| l.text == "end")
        }
        Edit::Replace {
            old_lines,
            new_lines,
            ..
        } => old_lines.iter().chain(new_lines).any(|l| l.text == "end"),
    });
    assert!(
        mentions_end,
        "the `end` footer should appear in the diff edits",
    );
}

fn with_policy(policy: OrderPolicy, overrides: Vec<OrderPolicyOverride>) -> NormalizeOptions {
    NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
        default: policy,
        overrides,
    })
}

fn edited_texts(diff: &Diff) -> Vec<&str> {
    let mut texts = diff
        .edits
        .iter()
        .flat_map(|edit| match edit {
            Edit::Delete { lines, .. } | Edit::Insert { lines, .. } => lines.iter().collect(),
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => old_lines.iter().chain(new_lines).collect::<Vec<_>>(),
        })
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    texts.sort_unstable();
    texts
}

const SAME_HEADER_SIBLINGS: &str = "\
router bgp 65000
 address-family ipv4
  neighbor 10.0.0.1 activate
 address-family ipv4
  neighbor 10.0.0.2 activate
";

#[test]
fn reordered_same_header_siblings_cancel_under_unordered_and_keyed_stable() {
    let a = parse_generic(SAME_HEADER_SIBLINGS);
    let b = parse_generic(
        "router bgp 65000\n address-family ipv4\n  neighbor 10.0.0.2 activate\n address-family ipv4\n  neighbor 10.0.0.1 activate\n",
    );

    for policy in [OrderPolicy::Unordered, OrderPolicy::KeyedStable] {
        let diff = diff_documents(&a, &b, with_policy(policy, Vec::new())).unwrap();
        assert!(diff.edits.is_empty(), "{policy:?}: {:?}", diff.edits);
    }
}

#[test]
fn swapped_route_policy_statements_cancel_under_unordered_and_keyed_stable() {
    let gold = "  if community matches-any C-GOLD then\n    set local-preference 200\n  else\n    set local-preference 100\n  endif\n";
    let bogons = "  if destination in PS-BOGONS then\n    drop\n  else\n    pass\n  endif\n";
    let a = parse_iosxr(&format!("route-policy RP-IN\n{gold}{bogons}end-policy\n"));
    let b = parse_iosxr(&format!("route-policy RP-IN\n{bogons}{gold}end-policy\n"));

    let ordered = diff_documents(&a, &b, with_policy(OrderPolicy::Ordered, Vec::new())).unwrap();
    assert!(ordered.has_changes, "the swap is drift under ordered");
    for policy in [OrderPolicy::Unordered, OrderPolicy::KeyedStable] {
        let diff = diff_documents(&a, &b, with_policy(policy, Vec::new())).unwrap();
        assert!(diff.edits.is_empty(), "{policy:?}: {:?}", diff.edits);
    }
}

#[test]
fn same_header_siblings_swapped_and_reordered_inside_cancel() {
    let a = parse_generic(
        "router bgp 65000\n address-family ipv4\n  neighbor 10.0.0.1 activate\n  neighbor 10.0.0.2 activate\n address-family ipv4\n  neighbor 10.0.0.3 activate\n  neighbor 10.0.0.4 activate\n",
    );
    let b = parse_generic(
        "router bgp 65000\n address-family ipv4\n  neighbor 10.0.0.4 activate\n  neighbor 10.0.0.3 activate\n address-family ipv4\n  neighbor 10.0.0.2 activate\n  neighbor 10.0.0.1 activate\n",
    );

    for policy in [OrderPolicy::Unordered, OrderPolicy::KeyedStable] {
        let diff = diff_documents(&a, &b, with_policy(policy, Vec::new())).unwrap();
        assert!(diff.edits.is_empty(), "{policy:?}: {:?}", diff.edits);
    }
}

#[test]
fn same_header_siblings_with_new_lines_pair_in_document_order() {
    let a = parse_generic(SAME_HEADER_SIBLINGS);
    let b = parse_generic(
        "router bgp 65000\n address-family ipv4\n  neighbor 10.0.0.1 activate\n  neighbor 10.0.0.3 activate\n address-family ipv4\n  neighbor 10.0.0.2 activate\n  neighbor 10.0.0.4 activate\n",
    );

    for policy in [OrderPolicy::Unordered, OrderPolicy::KeyedStable] {
        let diff = diff_documents(&a, &b, with_policy(policy, Vec::new())).unwrap();
        assert_eq!(
            edited_texts(&diff),
            vec![
                "  neighbor 10.0.0.3 activate",
                "  neighbor 10.0.0.4 activate"
            ],
            "{policy:?}: {:?}",
            diff.edits
        );
    }
}

#[test]
fn actions_swapped_between_fortios_policies_report_four_edits_under_unordered() {
    let policies = |first: &str, second: &str| {
        parse_fortios(&format!(
            "config firewall policy\n    edit 1\n        set action {first}\n    next\n    edit 2\n        set action {second}\n    next\nend\n"
        ))
    };
    let a = policies("accept", "deny");
    let b = policies("deny", "accept");

    let diff = diff_documents(&a, &b, with_policy(OrderPolicy::Unordered, Vec::new())).unwrap();
    assert_eq!(diff.edits.len(), 4, "{:?}", diff.edits);
    assert_eq!(
        edited_texts(&diff),
        [
            "        set action accept",
            "        set action accept",
            "        set action deny",
            "        set action deny",
        ]
    );
}

const REPATCHED_A: &str = "hostname r1\ninterface Ethernet1\n description srv1\n switchport access vlan 10\ninterface Ethernet2\n";
const REPATCHED_B: &str = "hostname r1\ninterface Ethernet1\ninterface Ethernet2\n description srv1\n switchport access vlan 10\n";

fn in_vrf(config: &str) -> String {
    let body = config.lines().skip(1).map(|line| format!(" {line}\n"));
    format!(
        "hostname r1\nvrf definition X\n{}",
        body.collect::<String>()
    )
}

fn planned_line_edits(diff: &Diff) -> Vec<String> {
    build_plan(diff)
        .actions
        .iter()
        .flat_map(|action| match action {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => line_edits
                .iter()
                .map(|edit| format!("{:?} {:?} {}", context_path.0, edit.kind, edit.text))
                .collect::<Vec<_>>(),
            other => panic!("expected line edits, got {other:?}"),
        })
        .collect()
}

#[test]
fn children_moved_to_an_empty_sibling_are_drift_under_every_policy() {
    let shapes = [
        (REPATCHED_A.to_string(), REPATCHED_B.to_string()),
        (in_vrf(REPATCHED_A), in_vrf(REPATCHED_B)),
    ];
    for (a, b) in &shapes {
        for parse in [parse_iosxe, parse_generic] {
            for policy in [
                OrderPolicy::Ordered,
                OrderPolicy::Unordered,
                OrderPolicy::KeyedStable,
            ] {
                let diff =
                    diff_documents(&parse(a), &parse(b), with_policy(policy, Vec::new())).unwrap();
                assert!(diff.has_changes, "{policy:?}:\n{a}");
                let headers_reported = edited_texts(&diff)
                    .iter()
                    .any(|text| text.trim() == "interface Ethernet1");
                assert_eq!(
                    headers_reported,
                    policy != OrderPolicy::Unordered,
                    "{policy:?}: {:?}",
                    diff.edits
                );
            }
        }
    }
}

#[test]
fn moved_children_are_planned_under_the_interfaces_they_left_and_joined() {
    let diff = diff_documents(
        &parse_iosxe(REPATCHED_A),
        &parse_iosxe(REPATCHED_B),
        with_policy(OrderPolicy::Unordered, Vec::new()),
    )
    .unwrap();

    assert_eq!(
        planned_line_edits(&diff),
        [
            "[1] Delete  description srv1",
            "[1] Delete  switchport access vlan 10",
            "[2] Insert  description srv1",
            "[2] Insert  switchport access vlan 10",
        ]
    );
}

#[test]
fn a_renamed_interface_is_planned_whole_under_unordered() {
    let diff = diff_documents(
        &parse_iosxe("hostname r1\ninterface Ethernet1\n shutdown\n"),
        &parse_iosxe("hostname r1\ninterface Ethernet10\n shutdown\n"),
        with_policy(OrderPolicy::Unordered, Vec::new()),
    )
    .unwrap();

    assert_eq!(
        planned_line_edits(&diff),
        [
            "[] Delete interface Ethernet1",
            "[] Delete  shutdown",
            "[] Insert interface Ethernet10",
            "[] Insert  shutdown",
        ]
    );
}

#[test]
fn override_on_a_moved_block_follows_its_left_path() {
    let a = parse_generic("hostname r1\npolicy-map WAN\n class VOICE\n  police 8000\n  priority\n");
    let b = parse_generic("policy-map WAN\n class VOICE\n  priority\n  police 8000\nhostname r1\n");
    let options = with_policy(
        OrderPolicy::Unordered,
        vec![OrderPolicyOverride {
            context_prefix: vec![1, 0],
            policy: OrderPolicy::Ordered,
        }],
    );

    let diff = diff_documents(&a, &b, options).unwrap();
    assert_eq!(
        edited_texts(&diff),
        vec!["  police 8000", "  police 8000"],
        "{:?}",
        diff.edits
    );
}

#[test]
fn block_whose_partner_paired_by_contents_takes_the_next_one() {
    let a = parse_generic(
        "router bgp 65000\n address-family ipv4\n  neighbor 10.0.0.1 activate\n  neighbor 10.0.0.2 activate\n  neighbor 10.0.0.3 activate\n address-family ipv4\n  neighbor 10.0.0.9 activate\n",
    );
    let b = parse_generic(
        "router bgp 65000\n address-family ipv4\n  neighbor 10.0.0.9 activate\n address-family ipv4\n  neighbor 10.0.0.2 activate\n  neighbor 10.0.0.1 activate\n",
    );
    let options = with_policy(
        OrderPolicy::Unordered,
        vec![OrderPolicyOverride {
            context_prefix: vec![0, 0],
            policy: OrderPolicy::Ordered,
        }],
    );

    let diff = diff_documents(&a, &b, options).unwrap();
    assert_eq!(
        edited_texts(&diff),
        vec![
            "  neighbor 10.0.0.1 activate",
            "  neighbor 10.0.0.1 activate",
            "  neighbor 10.0.0.3 activate",
        ],
        "the override must apply to the block: {:?}",
        diff.edits
    );
}

const ROUTE_1: &str = "ip route 10.0.0.0 255.0.0.0 192.0.2.1";
const ROUTE_2: &str = "ip route 10.0.0.0 255.0.0.0 192.0.2.2";
const ROUTE_3: &str = "ip route 10.0.0.0 255.0.0.0 192.0.2.3";
const SEQ_5: &str = "ip prefix-list PL seq 5 permit 10.0.0.0/8";
const SEQ_5_DENY: &str = "ip prefix-list PL seq 5 deny 10.0.0.0/8";
const SEQ_10: &str = "ip prefix-list PL seq 10 permit 192.168.0.0/16";

type Parse = fn(&str) -> netform_ir::Document;

const IOS_LIKE: [(&str, Parse); 3] = [
    ("iosxe", parse_iosxe),
    ("eos", netform_dialects::eos::parse),
    ("nxos", netform_dialects::nxos::parse),
];

/// `hostname r1` then `lines`, at the top level and under `vrf definition X`.
fn top_level_and_nested(lines: &[&str]) -> (String, String) {
    let top = lines.iter().map(|line| format!("{line}\n"));
    let nested = lines.iter().map(|line| format!(" {line}\n"));
    (
        format!("hostname r1\n{}", top.collect::<String>()),
        format!(
            "hostname r1\nvrf definition X\n{}",
            nested.collect::<String>()
        ),
    )
}

fn unordered_top_level_diff(parse: Parse, a: &[&str], b: &[&str]) -> Diff {
    let (a, _) = top_level_and_nested(a);
    let (b, _) = top_level_and_nested(b);
    diff_documents(
        &parse(&a),
        &parse(&b),
        with_policy(OrderPolicy::Unordered, Vec::new()),
    )
    .unwrap()
}

#[test]
fn swapped_top_level_entries_cancel_under_unordered() {
    for (first, second) in [(ROUTE_1, ROUTE_2), (SEQ_5, SEQ_10)] {
        for (name, parse) in IOS_LIKE {
            let diff = unordered_top_level_diff(parse, &[first, second], &[second, first]);
            assert!(diff.edits.is_empty(), "{name}: {first}: {:?}", diff.edits);
        }
    }
}

#[test]
fn top_level_value_change_is_still_reported_under_unordered() {
    for (name, parse) in IOS_LIKE {
        for (old, new) in [(ROUTE_1, ROUTE_3), (SEQ_5, SEQ_5_DENY)] {
            let diff = unordered_top_level_diff(parse, &[old, SEQ_10], &[SEQ_10, new]);
            assert!(diff.has_changes, "{name}: {old} -> {new}");
            let mut expected = vec![old, new];
            expected.sort_unstable();
            assert_eq!(edited_texts(&diff), expected, "{name}: {:?}", diff.edits);
        }
    }
}

/// each edit's kind and its lines' trimmed text, sorted.
fn edit_shapes(diff: &Diff) -> Vec<(&'static str, Vec<&str>)> {
    fn texts(lines: &[DiffLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.trim()).collect()
    }
    let mut shapes = diff
        .edits
        .iter()
        .map(|edit| match edit {
            Edit::Insert { lines, .. } => ("insert", texts(lines)),
            Edit::Delete { lines, .. } => ("delete", texts(lines)),
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => ("replace", [texts(old_lines), texts(new_lines)].concat()),
        })
        .collect::<Vec<_>>();
    shapes.sort_unstable();
    shapes
}

#[test]
fn unordered_top_level_and_nested_level_report_the_same_edits() {
    let cases: [(&[&str], &[&str]); 4] = [
        (&[ROUTE_1, ROUTE_2], &[ROUTE_2, ROUTE_1]),
        (&[SEQ_5, SEQ_10], &[SEQ_10, SEQ_5]),
        (&[ROUTE_1, ROUTE_2], &[ROUTE_2, ROUTE_3]),
        (&[SEQ_5, SEQ_10], &[SEQ_10, SEQ_5_DENY]),
    ];
    let options = || with_policy(OrderPolicy::Unordered, Vec::new());

    for (name, parse) in IOS_LIKE {
        for (left, right) in cases {
            let (a_top, a_nested) = top_level_and_nested(left);
            let (b_top, b_nested) = top_level_and_nested(right);
            let top = diff_documents(&parse(&a_top), &parse(&b_top), options()).unwrap();
            let nested = diff_documents(&parse(&a_nested), &parse(&b_nested), options()).unwrap();
            assert_eq!(
                edit_shapes(&top),
                edit_shapes(&nested),
                "{name}: {left:?} -> {right:?}"
            );
        }
    }
}

fn unreliable_regions(diff: &Diff) -> Vec<Path> {
    diff.findings
        .iter()
        .filter(|finding| finding.code == crate::finding_code::DIFF_UNRELIABLE_REGION)
        .filter_map(|finding| finding.path.clone())
        .collect()
}

#[test]
fn unordered_top_level_flags_the_regions_the_generic_dialect_flags() {
    let cases: [(&[&str], &[&str], Vec<Path>); 3] = [
        (&[ROUTE_1, ROUTE_2], &[ROUTE_2, ROUTE_1], Vec::new()),
        (&[ROUTE_1, SEQ_10], &[ROUTE_3, SEQ_10], vec![Path(vec![1])]),
        (
            &[ROUTE_1, ROUTE_2, SEQ_10],
            &[ROUTE_2, ROUTE_1, SEQ_5],
            vec![Path(vec![1]), Path(vec![3])],
        ),
    ];

    for (name, parse) in IOS_LIKE {
        for (left, right, expected) in &cases {
            let keyed = unordered_top_level_diff(parse, left, right);
            let generic = unordered_top_level_diff(parse_generic, left, right);
            assert_eq!(
                unreliable_regions(&keyed),
                *expected,
                "{name}: {left:?} -> {right:?}"
            );
            assert_eq!(
                unreliable_regions(&generic),
                *expected,
                "generic: {left:?} -> {right:?}"
            );
        }
    }
}

#[test]
fn ordered_override_on_a_top_level_block_holds_under_an_unordered_top_level() {
    let acl = |first: &str, second: &str| {
        format!(
            "ip access-list extended ACL\n permit tcp any any eq {first}\n permit tcp any any eq {second}\n"
        )
    };
    let a = parse_iosxe(&format!(
        "hostname r1\n{ROUTE_1}\n{ROUTE_2}\n{}",
        acl("443", "80")
    ));
    let b = parse_iosxe(&format!(
        "{}{ROUTE_2}\n{ROUTE_1}\nhostname r1\n",
        acl("80", "443")
    ));

    let silent = diff_documents(&a, &b, with_policy(OrderPolicy::Unordered, Vec::new())).unwrap();
    assert!(silent.edits.is_empty(), "{:?}", silent.edits);

    let acl_ordered = vec![OrderPolicyOverride {
        context_prefix: vec![3],
        policy: OrderPolicy::Ordered,
    }];
    let diff = diff_documents(&a, &b, with_policy(OrderPolicy::Unordered, acl_ordered)).unwrap();
    assert_eq!(
        edited_texts(&diff),
        vec![" permit tcp any any eq 443", " permit tcp any any eq 443"],
        "{:?}",
        diff.edits
    );
}

#[test]
fn inserts_into_a_block_only_the_right_side_has_get_no_left_anchor() {
    let a = parse_generic("router bgp 1\n  neighbor 1.1.1.1\n");
    let b = parse_generic(
        "router bgp 1\n  neighbor 1.1.1.1\n  vrf A\n    address-family ipv4\n      network 10.0.0.0/8\n",
    );

    let diff = diff_documents(&a, &b, with_policy(OrderPolicy::KeyedStable, Vec::new())).unwrap();
    let mut anchors = diff
        .edits
        .iter()
        .map(|edit| match edit {
            Edit::Insert {
                left_anchor, lines, ..
            } => (
                lines[0].text.as_str(),
                left_anchor.as_ref().map(|anchor| anchor.path.0.clone()),
            ),
            other => panic!("expected only inserts, got {other:?}"),
        })
        .collect::<Vec<_>>();
    anchors.sort();
    assert_eq!(
        anchors,
        [
            ("      network 10.0.0.0/8", None),
            ("    address-family ipv4", None),
            ("  vrf A", Some(vec![0])),
        ]
    );
}
