use crate::model::{Diff, Edit, Plan, PlanAction, PlanFinding, PlanLineEdit, PlanLineEditKind};

/// Convert a [`Diff`] into a transport-neutral action plan.
pub fn build_plan(diff: &Diff) -> Plan {
    let mut actions = Vec::new();
    let mut grouped_line_action_indices: Vec<(netform_ir::Path, usize)> = Vec::new();
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
                    findings.push(PlanFinding {
                        code: "missing_anchor".to_string(),
                        message: "cannot create plan action for replace edit without left anchor"
                            .to_string(),
                    });
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
                    findings.push(PlanFinding {
                        code: "missing_anchor".to_string(),
                        message: "cannot create plan action for insert edit without right anchor"
                            .to_string(),
                    });
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
                    findings.push(PlanFinding {
                        code: "missing_anchor".to_string(),
                        message: "cannot create plan action for delete edit without left anchor"
                            .to_string(),
                    });
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

fn push_or_append_grouped_line_action(
    actions: &mut Vec<PlanAction>,
    grouped_indices: &mut Vec<(netform_ir::Path, usize)>,
    context_path: netform_ir::Path,
    mut line_edits: Vec<PlanLineEdit>,
) {
    if let Some((_, idx)) = grouped_indices
        .iter()
        .find(|(path, _)| *path == context_path)
        .cloned()
    {
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
    grouped_indices.push((context_path.clone(), idx));
    actions.push(PlanAction::ApplyLineEditsUnderContext {
        context_path,
        line_edits,
    });
}
