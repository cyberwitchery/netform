use netform_dialect_iosxe::parse_iosxe;
use netform_ir::{Path, Span, parse_generic};

use super::{
    Diff, DiffLine, Edit, EditAnchor, NormalizationStep, NormalizeOptions, OrderPolicy,
    OrderPolicyConfig, PlanAction, PlanLineEditKind, build_comparison_view, build_plan,
    diff_documents,
};

#[test]
fn detects_replace_edit() {
    let a = parse_generic("interface Ethernet1\n  description old\n");
    let b = parse_generic("interface Ethernet1\n  description new\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    assert_eq!(diff.edits.len(), 1);
    assert!(matches!(diff.edits[0], Edit::Replace { .. }));
}

#[test]
fn ignores_comments_when_configured() {
    let a = parse_generic("! generated\ninterface Ethernet1\n");
    let b = parse_generic("! changed comment\ninterface Ethernet1\n");

    let diff = diff_documents(
        &a,
        &b,
        NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]),
    );

    assert!(diff.edits.is_empty());
}

#[test]
fn records_applied_normalization_steps() {
    let a = parse_generic("line  a\n");
    let b = parse_generic("line a\n");
    let options = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);

    let diff = diff_documents(&a, &b, options);
    assert_eq!(
        diff.normalization_steps,
        vec![NormalizationStep::CollapseInternalWhitespace]
    );
}

#[test]
fn block_aware_diff_only_reports_changed_children() {
    let a = parse_generic("interface Ethernet1\n  description old\n  mtu 9000\n");
    let b = parse_generic("interface Ethernet1\n  description new\n  mtu 9000\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
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

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    assert!(!diff.has_changes);
    assert!(!diff.findings.is_empty());
    assert!(
        diff.findings
            .iter()
            .any(|f| f.message.contains("ambiguous content key"))
    );
}

#[test]
fn reports_has_changes_for_drift() {
    let a = parse_generic("hostname old\n");
    let b = parse_generic("hostname new\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    assert!(diff.has_changes);
}

#[test]
fn ordered_policy_reports_reordered_block_children_as_change() {
    let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n");
    let b = parse_generic("interface Ethernet1\n  mtu 9000\n  description uplink\n");

    let diff = diff_documents(
        &a,
        &b,
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: Vec::new(),
        }),
    );

    assert!(diff.has_changes);
}

#[test]
fn unordered_policy_ignores_reordered_block_children() {
    let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n");
    let b = parse_generic("interface Ethernet1\n  mtu 9000\n  description uplink\n");

    let diff = diff_documents(
        &a,
        &b,
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: OrderPolicy::Unordered,
            overrides: Vec::new(),
        }),
    );

    assert!(!diff.has_changes);
}

#[test]
fn keyed_stable_policy_ignores_reordered_block_children() {
    let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n");
    let b = parse_generic("interface Ethernet1\n  mtu 9000\n  description uplink\n");

    let diff = diff_documents(
        &a,
        &b,
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: OrderPolicy::KeyedStable,
            overrides: Vec::new(),
        }),
    );

    assert!(!diff.has_changes);
}

#[test]
fn fallback_alignment_emits_finding() {
    let a = parse_generic("interface Ethernet1\n  description one\n");
    let b = parse_generic("router bgp 65000\n  neighbor 10.0.0.1 remote-as 65001\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    assert!(
        diff.findings
            .iter()
            .any(|f| f.message.contains("fallback segment alignment"))
    );
}

#[test]
fn parse_uncertainty_is_exposed_as_finding() {
    let a = parse_generic("  orphan-line\n");
    let b = parse_generic("  orphan-line\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    assert!(
        diff.findings
            .iter()
            .any(|f| f.code == "unknown_unparsed_construct")
    );
}

#[test]
fn build_plan_emits_missing_anchor_finding_when_anchor_is_absent() {
    let diff = Diff {
        edits: vec![Edit::Insert {
            at_key: None,
            left_anchor: None,
            right_anchor: None,
            lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "set system host-name edge-1".to_string(),
                path: Path(vec![0]),
                span: Span {
                    line: 1,
                    start_byte: 0,
                    end_byte: 27,
                },
            }],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert!(plan.actions.is_empty());
    assert!(
        plan.findings
            .iter()
            .any(|f| f.code == "missing_anchor" && f.message.contains("insert"))
    );
}

#[test]
fn build_plan_creates_insert_and_delete_line_actions_with_anchor_context() {
    let delete_anchor = EditAnchor {
        path: Path(vec![0, 2]),
        span: Span {
            line: 3,
            start_byte: 20,
            end_byte: 36,
        },
    };
    let insert_anchor = EditAnchor {
        path: Path(vec![0, 1]),
        span: Span {
            line: 2,
            start_byte: 10,
            end_byte: 28,
        },
    };

    let diff = Diff {
        edits: vec![
            Edit::Delete {
                at_key: Some(11),
                left_anchor: Some(delete_anchor),
                right_anchor: None,
                lines: vec![DiffLine {
                    content_key: 11,
                    occurrence_key: 11,
                    text: "  no shutdown".to_string(),
                    path: Path(vec![0, 2]),
                    span: Span {
                        line: 3,
                        start_byte: 20,
                        end_byte: 32,
                    },
                }],
            },
            Edit::Insert {
                at_key: Some(22),
                left_anchor: None,
                right_anchor: Some(insert_anchor),
                lines: vec![DiffLine {
                    content_key: 22,
                    occurrence_key: 22,
                    text: "  shutdown".to_string(),
                    path: Path(vec![0, 1]),
                    span: Span {
                        line: 2,
                        start_byte: 10,
                        end_byte: 20,
                    },
                }],
            },
        ],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.findings.len(), 0);

    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext {
            context_path,
            line_edits,
        } => {
            assert_eq!(context_path.0, vec![0]);
            assert_eq!(line_edits[0].kind, PlanLineEditKind::Delete);
            assert_eq!(line_edits[1].kind, PlanLineEditKind::Insert);
        }
        _ => panic!("expected delete line-edit action"),
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
    );

    assert!(diff.findings.iter().any(|f| {
        f.code == "ambiguous_key_match" && f.message.contains("ambiguous extracted key")
    }));
}
