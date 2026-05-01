use netform_dialect_fortios::parse_fortios;
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
fn build_plan_empty_diff_produces_empty_plan() {
    let diff = Diff::default();
    let plan = build_plan(&diff);
    assert_eq!(plan.version, "v1");
    assert!(plan.actions.is_empty());
    assert!(plan.findings.is_empty());
}

#[test]
fn build_plan_replace_without_left_anchor_emits_finding() {
    let diff = Diff {
        edits: vec![Edit::Replace {
            old_at_key: Some(1),
            new_at_key: Some(2),
            left_anchor: None,
            right_anchor: None,
            old_lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "  description old".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 27,
                },
            }],
            new_lines: vec![DiffLine {
                content_key: 2,
                occurrence_key: 2,
                text: "  description new".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 27,
                },
            }],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert!(plan.actions.is_empty());
    assert_eq!(plan.findings.len(), 1);
    assert_eq!(plan.findings[0].code, "missing_anchor");
    assert!(plan.findings[0].message.contains("replace"));
}

#[test]
fn build_plan_delete_without_left_anchor_emits_finding() {
    let diff = Diff {
        edits: vec![Edit::Delete {
            at_key: Some(1),
            left_anchor: None,
            right_anchor: None,
            lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "  no shutdown".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 23,
                },
            }],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert!(plan.actions.is_empty());
    assert_eq!(plan.findings.len(), 1);
    assert_eq!(plan.findings[0].code, "missing_anchor");
    assert!(plan.findings[0].message.contains("delete"));
}

#[test]
fn build_plan_single_line_replace_creates_replace_line_edit() {
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
            old_lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "  description old".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 27,
                },
            }],
            new_lines: vec![DiffLine {
                content_key: 2,
                occurrence_key: 2,
                text: "  description new".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 27,
                },
            }],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
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
fn build_plan_multi_line_replace_creates_replace_block() {
    let anchor = EditAnchor {
        path: Path(vec![1]),
        span: Span {
            line: 4,
            start_byte: 40,
            end_byte: 55,
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
                    text: "router bgp 65000".to_string(),
                    path: Path(vec![1]),
                    span: Span {
                        line: 4,
                        start_byte: 40,
                        end_byte: 55,
                    },
                },
                DiffLine {
                    content_key: 11,
                    occurrence_key: 11,
                    text: "  neighbor 10.0.0.1 remote-as 65001".to_string(),
                    path: Path(vec![1, 0]),
                    span: Span {
                        line: 5,
                        start_byte: 56,
                        end_byte: 90,
                    },
                },
            ],
            new_lines: vec![
                DiffLine {
                    content_key: 2,
                    occurrence_key: 2,
                    text: "router bgp 65100".to_string(),
                    path: Path(vec![1]),
                    span: Span {
                        line: 4,
                        start_byte: 40,
                        end_byte: 55,
                    },
                },
                DiffLine {
                    content_key: 21,
                    occurrence_key: 21,
                    text: "  neighbor 10.0.0.2 remote-as 65002".to_string(),
                    path: Path(vec![1, 0]),
                    span: Span {
                        line: 5,
                        start_byte: 56,
                        end_byte: 90,
                    },
                },
            ],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.findings.len(), 0);
    assert_eq!(plan.actions.len(), 1);
    match &plan.actions[0] {
        PlanAction::ReplaceBlock {
            target_path,
            target_span,
            intended_lines,
        } => {
            assert_eq!(target_path.0, vec![1]);
            assert_eq!(target_span.line, 4);
            assert_eq!(intended_lines.len(), 2);
            assert_eq!(intended_lines[0], "router bgp 65100");
            assert_eq!(intended_lines[1], "  neighbor 10.0.0.2 remote-as 65002");
        }
        _ => panic!("expected replace-block action"),
    }
}

#[test]
fn build_plan_replace_with_single_old_multi_new_creates_replace_block() {
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
            old_lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "  description old".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 27,
                },
            }],
            new_lines: vec![
                DiffLine {
                    content_key: 2,
                    occurrence_key: 2,
                    text: "  description new-a".to_string(),
                    path: Path(vec![0, 1]),
                    span: Span {
                        line: 2,
                        start_byte: 10,
                        end_byte: 29,
                    },
                },
                DiffLine {
                    content_key: 3,
                    occurrence_key: 3,
                    text: "  description new-b".to_string(),
                    path: Path(vec![0, 2]),
                    span: Span {
                        line: 3,
                        start_byte: 30,
                        end_byte: 49,
                    },
                },
            ],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(plan.actions[0], PlanAction::ReplaceBlock { .. }));
}

#[test]
fn build_plan_replace_with_multi_old_single_new_creates_replace_block() {
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
    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(plan.actions[0], PlanAction::ReplaceBlock { .. }));
}

#[test]
fn build_plan_insert_with_anchor_creates_insert_line_edit() {
    let anchor = EditAnchor {
        path: Path(vec![0, 1]),
        span: Span {
            line: 2,
            start_byte: 10,
            end_byte: 28,
        },
    };
    let diff = Diff {
        edits: vec![Edit::Insert {
            at_key: Some(1),
            left_anchor: None,
            right_anchor: Some(anchor),
            lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "  shutdown".to_string(),
                path: Path(vec![0, 1]),
                span: Span {
                    line: 2,
                    start_byte: 10,
                    end_byte: 20,
                },
            }],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.findings.len(), 0);
    assert_eq!(plan.actions.len(), 1);
    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext {
            context_path,
            line_edits,
        } => {
            assert_eq!(context_path.0, vec![0]);
            assert_eq!(line_edits.len(), 1);
            assert_eq!(line_edits[0].kind, PlanLineEditKind::Insert);
            assert_eq!(line_edits[0].text, "  shutdown");
        }
        _ => panic!("expected insert line-edit action"),
    }
}

#[test]
fn build_plan_delete_with_anchor_creates_delete_line_edit() {
    let anchor = EditAnchor {
        path: Path(vec![0, 2]),
        span: Span {
            line: 3,
            start_byte: 20,
            end_byte: 36,
        },
    };
    let diff = Diff {
        edits: vec![Edit::Delete {
            at_key: Some(1),
            left_anchor: Some(anchor),
            right_anchor: None,
            lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "  no shutdown".to_string(),
                path: Path(vec![0, 2]),
                span: Span {
                    line: 3,
                    start_byte: 20,
                    end_byte: 33,
                },
            }],
        }],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.findings.len(), 0);
    assert_eq!(plan.actions.len(), 1);
    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext {
            context_path,
            line_edits,
        } => {
            assert_eq!(context_path.0, vec![0]);
            assert_eq!(line_edits.len(), 1);
            assert_eq!(line_edits[0].kind, PlanLineEditKind::Delete);
            assert_eq!(line_edits[0].text, "  no shutdown");
        }
        _ => panic!("expected delete line-edit action"),
    }
}

#[test]
fn build_plan_edits_under_different_contexts_create_separate_actions() {
    let anchor_a = EditAnchor {
        path: Path(vec![0, 1]),
        span: Span {
            line: 2,
            start_byte: 10,
            end_byte: 27,
        },
    };
    let anchor_b = EditAnchor {
        path: Path(vec![1, 0]),
        span: Span {
            line: 5,
            start_byte: 50,
            end_byte: 65,
        },
    };
    let diff = Diff {
        edits: vec![
            Edit::Insert {
                at_key: Some(1),
                left_anchor: None,
                right_anchor: Some(anchor_a),
                lines: vec![DiffLine {
                    content_key: 1,
                    occurrence_key: 1,
                    text: "  description first".to_string(),
                    path: Path(vec![0, 1]),
                    span: Span {
                        line: 2,
                        start_byte: 10,
                        end_byte: 29,
                    },
                }],
            },
            Edit::Insert {
                at_key: Some(2),
                left_anchor: None,
                right_anchor: Some(anchor_b),
                lines: vec![DiffLine {
                    content_key: 2,
                    occurrence_key: 2,
                    text: "  description second".to_string(),
                    path: Path(vec![1, 0]),
                    span: Span {
                        line: 5,
                        start_byte: 50,
                        end_byte: 70,
                    },
                }],
            },
        ],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert_eq!(plan.actions.len(), 2);
    match &plan.actions[0] {
        PlanAction::ApplyLineEditsUnderContext { context_path, .. } => {
            assert_eq!(context_path.0, vec![0]);
        }
        _ => panic!("expected first action under context [0]"),
    }
    match &plan.actions[1] {
        PlanAction::ApplyLineEditsUnderContext { context_path, .. } => {
            assert_eq!(context_path.0, vec![1]);
        }
        _ => panic!("expected second action under context [1]"),
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
                left_anchor: None,
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
            assert_eq!(line_edits.len(), 3);
            assert_eq!(line_edits[0].kind, PlanLineEditKind::Delete);
            assert_eq!(line_edits[1].kind, PlanLineEditKind::Insert);
            assert_eq!(line_edits[2].kind, PlanLineEditKind::Replace);
        }
        _ => panic!("expected single grouped line-edit action"),
    }
}

#[test]
fn build_plan_multiple_missing_anchors_produce_multiple_findings() {
    let diff = Diff {
        edits: vec![
            Edit::Replace {
                old_at_key: None,
                new_at_key: None,
                left_anchor: None,
                right_anchor: None,
                old_lines: vec![],
                new_lines: vec![],
            },
            Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: None,
                lines: vec![],
            },
            Edit::Delete {
                at_key: None,
                left_anchor: None,
                right_anchor: None,
                lines: vec![],
            },
        ],
        ..Diff::default()
    };

    let plan = build_plan(&diff);
    assert!(plan.actions.is_empty());
    assert_eq!(plan.findings.len(), 3);
    assert!(plan.findings.iter().all(|f| f.code == "missing_anchor"));
    assert!(plan.findings.iter().any(|f| f.message.contains("replace")));
    assert!(plan.findings.iter().any(|f| f.message.contains("insert")));
    assert!(plan.findings.iter().any(|f| f.message.contains("delete")));
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

// ── order policy behavioral contrasts (diff_documents level) ──
//
// These tests run the same parsed documents through all three policies and
// assert the different outcomes, documenting when and how the policies diverge.

#[test]
fn reordered_block_children_only_changed_under_ordered_policy() {
    // Identical block children in swapped order.  Ordered treats this as
    // drift; Unordered and KeyedStable do not.
    let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n  no shutdown\n");
    let b = parse_generic("interface Ethernet1\n  no shutdown\n  description uplink\n  mtu 9000\n");

    let opts = |policy| {
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: policy,
            overrides: Vec::new(),
        })
    };

    let ordered = diff_documents(&a, &b, opts(OrderPolicy::Ordered));
    let unordered = diff_documents(&a, &b, opts(OrderPolicy::Unordered));
    let keyed = diff_documents(&a, &b, opts(OrderPolicy::KeyedStable));

    assert!(ordered.has_changes, "Ordered detects reordering as drift");
    assert!(!ordered.edits.is_empty());

    assert!(!unordered.has_changes, "Unordered ignores child reordering");
    assert!(unordered.edits.is_empty());

    assert!(!keyed.has_changes, "KeyedStable ignores child reordering");
    assert!(keyed.edits.is_empty());
}

#[test]
fn fortios_set_value_change_keyed_stable_emits_replace_unordered_emits_delete_insert() {
    // FortiOS `set` commands produce key_hints (e.g. `set:hostname`), which
    // give lines a stable content_key independent of the actual value.
    //
    // When a value changes:
    //   KeyedStable → pairs by content_key → Replace
    //   Unordered   → hashes normalized text → different buckets → Delete + Insert
    let a = parse_fortios("config system global\n    set hostname \"edge-1\"\nend\n");
    let b = parse_fortios("config system global\n    set hostname \"edge-2\"\nend\n");

    let opts = |policy| {
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: policy,
            overrides: Vec::new(),
        })
    };

    let keyed = diff_documents(&a, &b, opts(OrderPolicy::KeyedStable));
    let unordered = diff_documents(&a, &b, opts(OrderPolicy::Unordered));

    assert!(keyed.has_changes);
    assert!(unordered.has_changes);

    // KeyedStable pairs the two `set hostname` lines by their shared
    // content_key and emits a single Replace.
    assert!(
        keyed
            .edits
            .iter()
            .any(|e| matches!(e, Edit::Replace { .. })),
        "KeyedStable should emit Replace for value change on a keyed line"
    );
    assert!(
        !keyed
            .edits
            .iter()
            .any(|e| matches!(e, Edit::Delete { .. } | Edit::Insert { .. })),
        "KeyedStable should not emit separate Delete/Insert for a keyed value change"
    );

    // Unordered sees two distinct text hashes — the old value disappears,
    // the new one appears.  No pairing, so Delete + Insert, not Replace.
    assert!(
        !unordered
            .edits
            .iter()
            .any(|e| matches!(e, Edit::Replace { .. })),
        "Unordered should not emit Replace (no key-based pairing)"
    );
    assert!(
        unordered
            .edits
            .iter()
            .any(|e| matches!(e, Edit::Delete { .. })),
        "Unordered should emit Delete for the old text"
    );
    assert!(
        unordered
            .edits
            .iter()
            .any(|e| matches!(e, Edit::Insert { .. })),
        "Unordered should emit Insert for the new text"
    );
}

#[test]
fn fortios_reorder_plus_value_change_three_way_contrast() {
    // Two `set` fields under the same block: reordered AND one value changes.
    //
    // Ordered  → sees both the reorder and the value change.
    // Unordered → ignores reorder; sees value change as Delete + Insert.
    // KeyedStable → ignores reorder; sees value change as Replace.
    let a = parse_fortios(
        "config system global\n    set hostname \"edge-1\"\n    set timezone \"UTC\"\nend\n",
    );
    let b = parse_fortios(
        "config system global\n    set timezone \"UTC\"\n    set hostname \"edge-2\"\nend\n",
    );

    let opts = |policy| {
        NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
            default: policy,
            overrides: Vec::new(),
        })
    };

    let ordered = diff_documents(&a, &b, opts(OrderPolicy::Ordered));
    let unordered = diff_documents(&a, &b, opts(OrderPolicy::Unordered));
    let keyed = diff_documents(&a, &b, opts(OrderPolicy::KeyedStable));

    // All three detect changes (the hostname value changed).
    assert!(ordered.has_changes);
    assert!(unordered.has_changes);
    assert!(keyed.has_changes);

    // Ordered produces more edits because it also reports the reordering.
    assert!(
        ordered.edits.len() > keyed.edits.len(),
        "Ordered reports reorder edits that KeyedStable suppresses"
    );

    // Unordered emits Delete + Insert for the hostname change.
    assert!(
        unordered
            .edits
            .iter()
            .any(|e| matches!(e, Edit::Delete { .. })),
    );
    assert!(
        unordered
            .edits
            .iter()
            .any(|e| matches!(e, Edit::Insert { .. })),
    );

    // KeyedStable emits a single Replace for the hostname change.
    assert_eq!(
        keyed.edits.len(),
        1,
        "KeyedStable should emit exactly one edit for the value change"
    );
    assert!(matches!(keyed.edits[0], Edit::Replace { .. }));
}
