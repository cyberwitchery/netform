use netform_diff::{
    Diff, DiffLine, Edit, EditAnchor, NormalizeOptions, PlanAction, PlanLineEditKind, build_plan,
    diff_documents,
};
use netform_ir::{Path, Span, parse_generic};

#[test]
fn generates_line_edit_plan_for_single_line_replace() {
    let a = parse_generic("interface Ethernet1\n  description old\n");
    let b = parse_generic("interface Ethernet1\n  description new\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    let plan = build_plan(&diff);

    assert_eq!(plan.version, "v1");
    assert_eq!(plan.findings.len(), 0);
    assert_eq!(plan.actions.len(), 1);

    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext {
            context_path,
            line_edits,
        } => {
            assert_eq!(context_path.0, vec![0]);
            assert_eq!(line_edits.len(), 1);
            assert_eq!(line_edits[0].kind, PlanLineEditKind::Replace);
            assert_eq!(line_edits[0].text, "  description new");
        }
        _ => panic!("expected line-edit action"),
    }
}

#[test]
fn generates_replace_block_plan_for_multi_line_replace() {
    let a = parse_generic("interface Ethernet1\n  description old\n  mtu 9000\n");
    let b = parse_generic("router bgp 65000\n  neighbor 10.0.0.1 remote-as 65001\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default());
    let plan = build_plan(&diff);

    assert!(!plan.actions.is_empty());
    assert!(
        plan.actions
            .iter()
            .any(|a| matches!(a, PlanAction::ReplaceBlock { .. }))
    );

    let json = serde_json::to_string_pretty(&plan).expect("serialize plan");
    assert!(json.contains("\"version\""));
    assert!(json.contains("\"actions\""));
    assert!(json.contains("\"findings\""));
}

#[test]
fn groups_multiple_line_edits_under_same_context() {
    let anchor_a = EditAnchor {
        path: Path(vec![0, 1]),
        span: Span {
            line: 2,
            start_byte: 10,
            end_byte: 28,
        },
    };
    let anchor_b = EditAnchor {
        path: Path(vec![0, 2]),
        span: Span {
            line: 3,
            start_byte: 29,
            end_byte: 39,
        },
    };
    let diff = Diff {
        edits: vec![
            Edit::Replace {
                old_at_key: Some(11),
                new_at_key: Some(12),
                left_anchor: Some(anchor_a.clone()),
                right_anchor: Some(anchor_a.clone()),
                old_lines: vec![DiffLine {
                    content_key: 11,
                    occurrence_key: 11,
                    text: "  description old".to_string(),
                    path: Path(vec![0, 1]),
                    span: anchor_a.span.clone(),
                }],
                new_lines: vec![DiffLine {
                    content_key: 12,
                    occurrence_key: 12,
                    text: "  description new".to_string(),
                    path: Path(vec![0, 1]),
                    span: anchor_a.span.clone(),
                }],
            },
            Edit::Replace {
                old_at_key: Some(21),
                new_at_key: Some(22),
                left_anchor: Some(anchor_b.clone()),
                right_anchor: Some(anchor_b.clone()),
                old_lines: vec![DiffLine {
                    content_key: 21,
                    occurrence_key: 21,
                    text: "  mtu 9000".to_string(),
                    path: Path(vec![0, 2]),
                    span: anchor_b.span.clone(),
                }],
                new_lines: vec![DiffLine {
                    content_key: 22,
                    occurrence_key: 22,
                    text: "  mtu 9216".to_string(),
                    path: Path(vec![0, 2]),
                    span: anchor_b.span.clone(),
                }],
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
            assert_eq!(line_edits.len(), 2);
            assert_eq!(line_edits[0].kind, PlanLineEditKind::Replace);
            assert_eq!(line_edits[1].kind, PlanLineEditKind::Replace);
        }
        _ => panic!("expected grouped line-edit action"),
    }
}

#[test]
fn preserves_first_seen_action_order_when_grouping_line_actions() {
    let line_anchor = EditAnchor {
        path: Path(vec![0, 1]),
        span: Span {
            line: 2,
            start_byte: 10,
            end_byte: 28,
        },
    };
    let block_anchor = EditAnchor {
        path: Path(vec![1]),
        span: Span {
            line: 10,
            start_byte: 100,
            end_byte: 120,
        },
    };

    let diff = Diff {
        edits: vec![
            Edit::Replace {
                old_at_key: Some(1),
                new_at_key: Some(2),
                left_anchor: Some(line_anchor.clone()),
                right_anchor: Some(line_anchor.clone()),
                old_lines: vec![DiffLine {
                    content_key: 1,
                    occurrence_key: 1,
                    text: "  description old".to_string(),
                    path: Path(vec![0, 1]),
                    span: line_anchor.span.clone(),
                }],
                new_lines: vec![DiffLine {
                    content_key: 2,
                    occurrence_key: 2,
                    text: "  description new".to_string(),
                    path: Path(vec![0, 1]),
                    span: line_anchor.span.clone(),
                }],
            },
            Edit::Replace {
                old_at_key: Some(3),
                new_at_key: Some(4),
                left_anchor: Some(block_anchor.clone()),
                right_anchor: Some(block_anchor),
                old_lines: vec![
                    DiffLine {
                        content_key: 3,
                        occurrence_key: 3,
                        text: "router bgp 65000".to_string(),
                        path: Path(vec![1]),
                        span: Span {
                            line: 10,
                            start_byte: 100,
                            end_byte: 114,
                        },
                    },
                    DiffLine {
                        content_key: 31,
                        occurrence_key: 31,
                        text: "  neighbor 192.0.2.1 remote-as 65100".to_string(),
                        path: Path(vec![1, 0]),
                        span: Span {
                            line: 11,
                            start_byte: 115,
                            end_byte: 149,
                        },
                    },
                ],
                new_lines: vec![
                    DiffLine {
                        content_key: 4,
                        occurrence_key: 4,
                        text: "router bgp 65000".to_string(),
                        path: Path(vec![1]),
                        span: Span {
                            line: 10,
                            start_byte: 100,
                            end_byte: 114,
                        },
                    },
                    DiffLine {
                        content_key: 41,
                        occurrence_key: 41,
                        text: "  neighbor 192.0.2.2 remote-as 65101".to_string(),
                        path: Path(vec![1, 0]),
                        span: Span {
                            line: 11,
                            start_byte: 115,
                            end_byte: 149,
                        },
                    },
                ],
            },
        ],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.actions.len(), 2);
    assert!(matches!(
        plan.actions[0],
        PlanAction::ApplyLineEditsUnderContext { .. }
    ));
    assert!(matches!(plan.actions[1], PlanAction::ReplaceBlock { .. }));
}
