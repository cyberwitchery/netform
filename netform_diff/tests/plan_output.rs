use netform_diff::{
    Diff, DiffLine, Edit, EditAnchor, NormalizeOptions, PlanAction, PlanLineEditKind, build_plan,
    diff_documents,
};
use netform_ir::{Path, Span, parse_generic};

#[test]
fn generates_line_edit_plan_for_single_line_replace() {
    let a = parse_generic("interface Ethernet1\n  description old\n");
    let b = parse_generic("interface Ethernet1\n  description new\n");

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
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

    let diff = diff_documents(&a, &b, NormalizeOptions::default()).unwrap();
    let plan = build_plan(&diff);

    assert_eq!(
        plan.actions,
        vec![PlanAction::ReplaceBlock {
            target_path: Path(vec![0]),
            target_span: Span {
                line: 1,
                start_byte: 0,
                end_byte: 19,
            },
            intended_lines: vec![
                "router bgp 65000".to_string(),
                "  neighbor 10.0.0.1 remote-as 65001".to_string(),
            ],
        }]
    );
    assert!(plan.findings.is_empty());
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
