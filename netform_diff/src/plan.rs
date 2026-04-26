use std::collections::HashMap;

use crate::model::{
    Diff, Edit, FindingLevel, Plan, PlanAction, PlanFinding, PlanLineEdit, PlanLineEditKind,
    finding_code,
};

/// Convert a [`Diff`] into a transport-neutral action plan.
pub fn build_plan(diff: &Diff) -> Plan {
    let mut actions = Vec::new();
    let mut grouped_line_action_indices: HashMap<netform_ir::Path, usize> = HashMap::new();
    let mut findings = Vec::new();

    for edit in &diff.edits {
        match edit {
            Edit::Replace {
                left_anchor,
                old_lines,
                new_lines,
                ..
            } => {
                if let Some(anchor) = left_anchor {
                    if old_lines.len() > 1 || new_lines.len() > 1 {
                        actions.push(PlanAction::ReplaceBlock {
                            target_path: anchor.path.clone(),
                            target_span: anchor.span.clone(),
                            intended_lines: new_lines.iter().map(|l| l.text.clone()).collect(),
                        });
                    } else {
                        let context_path = crate::util::parent_path(&anchor.path);
                        push_or_append_grouped_line_action(
                            &mut actions,
                            &mut grouped_line_action_indices,
                            context_path,
                            new_lines
                                .iter()
                                .map(|line| PlanLineEdit {
                                    kind: PlanLineEditKind::Replace,
                                    text: line.text.clone(),
                                })
                                .collect(),
                        );
                    }
                } else {
                    findings.push(missing_anchor_finding("replace", "left"));
                }
            }
            Edit::Insert {
                right_anchor,
                lines,
                ..
            } => {
                if let Some(anchor) = right_anchor {
                    let context_path = crate::util::parent_path(&anchor.path);
                    push_or_append_grouped_line_action(
                        &mut actions,
                        &mut grouped_line_action_indices,
                        context_path,
                        lines
                            .iter()
                            .map(|line| PlanLineEdit {
                                kind: PlanLineEditKind::Insert,
                                text: line.text.clone(),
                            })
                            .collect(),
                    );
                } else {
                    findings.push(missing_anchor_finding("insert", "right"));
                }
            }
            Edit::Delete {
                left_anchor, lines, ..
            } => {
                if let Some(anchor) = left_anchor {
                    let context_path = crate::util::parent_path(&anchor.path);
                    push_or_append_grouped_line_action(
                        &mut actions,
                        &mut grouped_line_action_indices,
                        context_path,
                        lines
                            .iter()
                            .map(|line| PlanLineEdit {
                                kind: PlanLineEditKind::Delete,
                                text: line.text.clone(),
                            })
                            .collect(),
                    );
                } else {
                    findings.push(missing_anchor_finding("delete", "left"));
                }
            }
        }
    }

    Plan {
        version: "v1".to_string(),
        actions,
        findings,
    }
}

fn missing_anchor_finding(edit_kind: &str, anchor_side: &str) -> PlanFinding {
    PlanFinding {
        code: finding_code::MISSING_ANCHOR.to_string(),
        message: format!(
            "cannot create plan action for {edit_kind} edit without {anchor_side} anchor"
        ),
        level: Some(FindingLevel::Warning),
        path: None,
        span: None,
    }
}

fn push_or_append_grouped_line_action(
    actions: &mut Vec<PlanAction>,
    grouped_indices: &mut HashMap<netform_ir::Path, usize>,
    context_path: netform_ir::Path,
    mut line_edits: Vec<PlanLineEdit>,
) {
    if let Some(&idx) = grouped_indices.get(&context_path) {
        if let Some(PlanAction::ApplyLineEditsUnderContext {
            line_edits: existing,
            ..
        }) = actions.get_mut(idx)
        {
            existing.append(&mut line_edits);
        }
        return;
    }

    let idx = actions.len();
    grouped_indices.insert(context_path.clone(), idx);
    actions.push(PlanAction::ApplyLineEditsUnderContext {
        context_path,
        line_edits,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DiffLine, EditAnchor};
    use netform_ir::{Path, Span};

    fn span(line: usize) -> Span {
        Span {
            line,
            start_byte: 0,
            end_byte: 10,
        }
    }

    fn anchor(path: Vec<usize>, line: usize) -> EditAnchor {
        EditAnchor {
            path: Path(path),
            span: span(line),
        }
    }

    fn diff_line(text: &str, path: Vec<usize>, line: usize) -> DiffLine {
        DiffLine {
            content_key: 0,
            occurrence_key: 0,
            text: text.to_string(),
            path: Path(path),
            span: span(line),
        }
    }

    fn diff_with_edits(edits: Vec<Edit>) -> Diff {
        Diff {
            edits,
            ..Default::default()
        }
    }

    #[test]
    fn single_line_replace_produces_line_edit() {
        let diff = diff_with_edits(vec![Edit::Replace {
            old_at_key: None,
            new_at_key: None,
            left_anchor: Some(anchor(vec![0, 1], 2)),
            right_anchor: None,
            old_lines: vec![diff_line("old", vec![0, 1], 2)],
            new_lines: vec![diff_line("new", vec![0, 1], 2)],
        }]);

        let plan = build_plan(&diff);

        assert_eq!(plan.version, "v1");
        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => {
                assert_eq!(context_path, &Path(vec![0]));
                assert_eq!(line_edits.len(), 1);
                assert_eq!(line_edits[0].kind, PlanLineEditKind::Replace);
                assert_eq!(line_edits[0].text, "new");
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }

    #[test]
    fn multi_line_replace_produces_replace_block() {
        let diff = diff_with_edits(vec![Edit::Replace {
            old_at_key: None,
            new_at_key: None,
            left_anchor: Some(anchor(vec![0, 1], 2)),
            right_anchor: None,
            old_lines: vec![
                diff_line("old1", vec![0, 1], 2),
                diff_line("old2", vec![0, 1], 3),
            ],
            new_lines: vec![
                diff_line("new1", vec![0, 1], 2),
                diff_line("new2", vec![0, 1], 3),
            ],
        }]);

        let plan = build_plan(&diff);

        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            PlanAction::ReplaceBlock {
                target_path,
                target_span,
                intended_lines,
            } => {
                assert_eq!(target_path, &Path(vec![0, 1]));
                assert_eq!(target_span, &span(2));
                assert_eq!(intended_lines, &["new1", "new2"]);
            }
            other => panic!("expected ReplaceBlock, got {other:?}"),
        }
    }

    #[test]
    fn replace_one_old_many_new_produces_replace_block() {
        let diff = diff_with_edits(vec![Edit::Replace {
            old_at_key: None,
            new_at_key: None,
            left_anchor: Some(anchor(vec![0, 1], 2)),
            right_anchor: None,
            old_lines: vec![diff_line("old", vec![0, 1], 2)],
            new_lines: vec![
                diff_line("new1", vec![0, 1], 2),
                diff_line("new2", vec![0, 1], 3),
            ],
        }]);

        let plan = build_plan(&diff);

        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], PlanAction::ReplaceBlock { .. }));
    }

    #[test]
    fn insert_with_anchor_produces_line_edit() {
        let diff = diff_with_edits(vec![Edit::Insert {
            at_key: None,
            left_anchor: None,
            right_anchor: Some(anchor(vec![0, 1, 2], 5)),
            lines: vec![diff_line("inserted line", vec![0, 1, 2], 5)],
        }]);

        let plan = build_plan(&diff);

        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => {
                assert_eq!(context_path, &Path(vec![0, 1]));
                assert_eq!(line_edits.len(), 1);
                assert_eq!(line_edits[0].kind, PlanLineEditKind::Insert);
                assert_eq!(line_edits[0].text, "inserted line");
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }

    #[test]
    fn delete_with_anchor_produces_line_edit() {
        let diff = diff_with_edits(vec![Edit::Delete {
            at_key: None,
            left_anchor: Some(anchor(vec![0, 3], 10)),
            right_anchor: None,
            lines: vec![diff_line("deleted line", vec![0, 3], 10)],
        }]);

        let plan = build_plan(&diff);

        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => {
                assert_eq!(context_path, &Path(vec![0]));
                assert_eq!(line_edits.len(), 1);
                assert_eq!(line_edits[0].kind, PlanLineEditKind::Delete);
                assert_eq!(line_edits[0].text, "deleted line");
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }

    #[test]
    fn replace_without_anchor_produces_finding() {
        let diff = diff_with_edits(vec![Edit::Replace {
            old_at_key: None,
            new_at_key: None,
            left_anchor: None,
            right_anchor: None,
            old_lines: vec![diff_line("old", vec![0], 1)],
            new_lines: vec![diff_line("new", vec![0], 1)],
        }]);

        let plan = build_plan(&diff);

        assert!(plan.actions.is_empty());
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].code, finding_code::MISSING_ANCHOR);
        assert!(plan.findings[0].message.contains("replace"));
    }

    #[test]
    fn insert_without_anchor_produces_finding() {
        let diff = diff_with_edits(vec![Edit::Insert {
            at_key: None,
            left_anchor: None,
            right_anchor: None,
            lines: vec![diff_line("new", vec![0], 1)],
        }]);

        let plan = build_plan(&diff);

        assert!(plan.actions.is_empty());
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].code, finding_code::MISSING_ANCHOR);
        assert!(plan.findings[0].message.contains("insert"));
    }

    #[test]
    fn delete_without_anchor_produces_finding() {
        let diff = diff_with_edits(vec![Edit::Delete {
            at_key: None,
            left_anchor: None,
            right_anchor: None,
            lines: vec![diff_line("gone", vec![0], 1)],
        }]);

        let plan = build_plan(&diff);

        assert!(plan.actions.is_empty());
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].code, finding_code::MISSING_ANCHOR);
        assert!(plan.findings[0].message.contains("delete"));
    }

    #[test]
    fn edits_under_same_context_are_grouped() {
        let diff = diff_with_edits(vec![
            Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: Some(anchor(vec![0, 1, 2], 3)),
                lines: vec![diff_line("line a", vec![0, 1, 2], 3)],
            },
            Edit::Delete {
                at_key: None,
                left_anchor: Some(anchor(vec![0, 1, 5], 7)),
                right_anchor: None,
                lines: vec![diff_line("line b", vec![0, 1, 5], 7)],
            },
        ]);

        let plan = build_plan(&diff);

        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => {
                assert_eq!(context_path, &Path(vec![0, 1]));
                assert_eq!(line_edits.len(), 2);
                assert_eq!(line_edits[0].kind, PlanLineEditKind::Insert);
                assert_eq!(line_edits[0].text, "line a");
                assert_eq!(line_edits[1].kind, PlanLineEditKind::Delete);
                assert_eq!(line_edits[1].text, "line b");
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }

    #[test]
    fn edits_under_different_contexts_stay_separate() {
        let diff = diff_with_edits(vec![
            Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: Some(anchor(vec![0, 1, 2], 3)),
                lines: vec![diff_line("line a", vec![0, 1, 2], 3)],
            },
            Edit::Delete {
                at_key: None,
                left_anchor: Some(anchor(vec![0, 2, 5], 7)),
                right_anchor: None,
                lines: vec![diff_line("line b", vec![0, 2, 5], 7)],
            },
        ]);

        let plan = build_plan(&diff);

        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 2);
        match &plan.actions[0] {
            PlanAction::ApplyLineEditsUnderContext { context_path, .. } => {
                assert_eq!(context_path, &Path(vec![0, 1]));
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
        match &plan.actions[1] {
            PlanAction::ApplyLineEditsUnderContext { context_path, .. } => {
                assert_eq!(context_path, &Path(vec![0, 2]));
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }

    #[test]
    fn mixed_actions_and_findings() {
        let diff = diff_with_edits(vec![
            Edit::Replace {
                old_at_key: None,
                new_at_key: None,
                left_anchor: Some(anchor(vec![0, 1], 2)),
                right_anchor: None,
                old_lines: vec![diff_line("old", vec![0, 1], 2)],
                new_lines: vec![diff_line("new", vec![0, 1], 2)],
            },
            Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: None,
                lines: vec![diff_line("orphan", vec![0], 5)],
            },
        ]);

        let plan = build_plan(&diff);

        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].code, finding_code::MISSING_ANCHOR);
    }

    #[test]
    fn empty_diff_produces_empty_plan() {
        let diff = diff_with_edits(vec![]);

        let plan = build_plan(&diff);

        assert_eq!(plan.version, "v1");
        assert!(plan.actions.is_empty());
        assert!(plan.findings.is_empty());
    }

    #[test]
    fn replace_block_uses_anchor_path_not_parent() {
        let diff = diff_with_edits(vec![Edit::Replace {
            old_at_key: None,
            new_at_key: None,
            left_anchor: Some(anchor(vec![0, 1, 2], 5)),
            right_anchor: None,
            old_lines: vec![
                diff_line("a", vec![0, 1, 2], 5),
                diff_line("b", vec![0, 1, 2], 6),
            ],
            new_lines: vec![diff_line("c", vec![0, 1, 2], 5)],
        }]);

        let plan = build_plan(&diff);

        match &plan.actions[0] {
            PlanAction::ReplaceBlock { target_path, .. } => {
                assert_eq!(target_path, &Path(vec![0, 1, 2]));
            }
            other => panic!("expected ReplaceBlock, got {other:?}"),
        }
    }

    #[test]
    fn multiple_inserts_same_context_accumulate() {
        let diff = diff_with_edits(vec![
            Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: Some(anchor(vec![0, 1, 2], 3)),
                lines: vec![diff_line("first", vec![0, 1, 2], 3)],
            },
            Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: Some(anchor(vec![0, 1, 4], 5)),
                lines: vec![
                    diff_line("second", vec![0, 1, 4], 5),
                    diff_line("third", vec![0, 1, 4], 6),
                ],
            },
        ]);

        let plan = build_plan(&diff);

        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            PlanAction::ApplyLineEditsUnderContext { line_edits, .. } => {
                assert_eq!(line_edits.len(), 3);
                assert_eq!(line_edits[0].text, "first");
                assert_eq!(line_edits[1].text, "second");
                assert_eq!(line_edits[2].text, "third");
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }
}
