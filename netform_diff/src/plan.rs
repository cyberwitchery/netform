use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use netform_ir::Path;

use crate::model::{
    Diff, DiffLine, Edit, EditAnchor, FindingLevel, Plan, PlanAction, PlanFinding, PlanLineEdit,
    PlanLineEditKind, finding_code,
};
use crate::util::{parent_path, path_starts_with};

/// convert a [`Diff`] into a transport-neutral action plan.
///
/// every `context_path` and `target_path` in the plan names a node of the
/// original left-hand document, the one the plan is applied to, and actions
/// come deepest first.
pub fn build_plan(diff: &Diff) -> Plan {
    let mut lines = PlanLines::default();
    let mut findings = Vec::new();
    for (order, edit) in diff.edits.iter().enumerate() {
        lines.add(order, edit, &mut findings);
    }
    let actions = lines.into_actions(&mut findings);

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

/// where an inserted line goes unless it sits inside another inserted line.
#[derive(Clone)]
enum Destination {
    /// under this left-side context, or nowhere when the context has no left-side counterpart.
    Context(Option<Path>),
    /// into the intended lines of the `replace_block` action at this index.
    Block(usize),
}

struct Placed<'a> {
    order: usize,
    line: &'a DiffLine,
}

struct ReplacedLine<'a> {
    order: usize,
    left: &'a Path,
    context: Path,
    new_text: &'a str,
}

struct Block<'a> {
    order: usize,
    target: &'a EditAnchor,
    intended_lines: Vec<String>,
}

#[derive(Default)]
struct ContextEdits<'a> {
    order: usize,
    deleted: Vec<Vec<&'a DiffLine>>,
    replaced: Vec<(&'a Path, &'a str)>,
    inserted: Vec<Vec<&'a DiffLine>>,
}

#[derive(Default)]
struct Contexts<'a> {
    entries: Vec<(Path, ContextEdits<'a>)>,
    index: HashMap<Path, usize>,
}

impl<'a> Contexts<'a> {
    fn at(&mut self, path: Path, order: usize) -> &mut ContextEdits<'a> {
        let index = match self.index.get(&path) {
            Some(&index) => index,
            None => {
                self.index.insert(path.clone(), self.entries.len());
                self.entries.push((
                    path,
                    ContextEdits {
                        order,
                        ..Default::default()
                    },
                ));
                self.entries.len() - 1
            }
        };
        let edits = &mut self.entries[index].1;
        edits.order = edits.order.min(order);
        edits
    }
}

#[derive(Default)]
struct PlanLines<'a> {
    deleted: Vec<Placed<'a>>,
    replaced: Vec<ReplacedLine<'a>>,
    inserted: Vec<(Placed<'a>, Destination)>,
    blocks: Vec<Block<'a>>,
}

impl<'a> PlanLines<'a> {
    fn add(&mut self, order: usize, edit: &'a Edit, findings: &mut Vec<PlanFinding>) {
        match edit {
            Edit::Replace {
                left_anchor,
                old_lines,
                new_lines,
                ..
            } => {
                let Some(anchor) = left_anchor else {
                    findings.push(missing_anchor_finding("replace", "left"));
                    return;
                };
                let context = parent_path(&anchor.path);
                if let ([_], [new_line]) = (old_lines.as_slice(), new_lines.as_slice()) {
                    self.replaced.push(ReplacedLine {
                        order,
                        left: &anchor.path,
                        context,
                        new_text: &new_line.text,
                    });
                } else if old_lines
                    .iter()
                    .all(|line| path_starts_with(&line.path.0, &anchor.path.0))
                {
                    self.blocks.push(Block {
                        order,
                        target: anchor,
                        intended_lines: Vec::new(),
                    });
                    let block = Destination::Block(self.blocks.len() - 1);
                    self.insert(order, new_lines, block);
                } else {
                    self.delete(order, old_lines);
                    self.insert(order, new_lines, Destination::Context(Some(context)));
                }
            }
            Edit::Insert {
                left_anchor,
                right_anchor,
                lines,
                ..
            } => {
                let Some(right_anchor) = right_anchor else {
                    findings.push(missing_anchor_finding("insert", "right"));
                    return;
                };
                let context = if right_anchor.path.0.len() == 1 {
                    Some(Path(Vec::new()))
                } else {
                    left_anchor.as_ref().map(|anchor| anchor.path.clone())
                };
                self.insert(order, lines, Destination::Context(context));
            }
            Edit::Delete {
                left_anchor, lines, ..
            } => {
                if left_anchor.is_some() {
                    self.delete(order, lines);
                } else {
                    findings.push(missing_anchor_finding("delete", "left"));
                }
            }
        }
    }

    fn delete(&mut self, order: usize, lines: &'a [DiffLine]) {
        self.deleted
            .extend(lines.iter().map(|line| Placed { order, line }));
    }

    /// records `lines`, whose first line sits directly under the context `destination` names.
    fn insert(&mut self, order: usize, lines: &'a [DiffLine], destination: Destination) {
        let Some(first) = lines.first() else {
            return;
        };
        let level = parent_path(&first.path);
        for line in lines {
            let destination = if parent_path(&line.path) == level {
                destination.clone()
            } else {
                Destination::Context(None)
            };
            self.inserted.push((Placed { order, line }, destination));
        }
    }

    fn into_actions(self, findings: &mut Vec<PlanFinding>) -> Vec<PlanAction> {
        let block_targets = self
            .blocks
            .iter()
            .map(|block| block.target.path.0.as_slice())
            .collect::<HashSet<_>>();
        let deleted = self
            .deleted
            .into_iter()
            .filter(|placed| {
                let path = &placed.line.path.0;
                !(0..=path.len()).any(|len| block_targets.contains(&path[..len]))
            })
            .map(|placed| (placed, ()))
            .collect();
        let mut blocks = self.blocks;
        let mut contexts = Contexts::default();

        for (order, fragment, ()) in subtrees(deleted) {
            let context = parent_path(&fragment[0].path);
            contexts.at(context, order).deleted.push(fragment);
        }
        for replaced in self.replaced {
            contexts
                .at(replaced.context, replaced.order)
                .replaced
                .push((replaced.left, replaced.new_text));
        }
        for (order, fragment, destination) in subtrees(self.inserted) {
            match destination {
                Destination::Context(Some(context)) => {
                    contexts.at(context, order).inserted.push(fragment);
                }
                Destination::Block(index) => blocks[index]
                    .intended_lines
                    .extend(fragment.iter().map(|line| line.text.clone())),
                Destination::Context(None) => {
                    findings.push(missing_anchor_finding("insert", "left"));
                }
            }
        }

        let mut actions = blocks
            .into_iter()
            .map(|block| {
                (
                    block.order,
                    PlanAction::ReplaceBlock {
                        target_path: block.target.path.clone(),
                        target_span: block.target.span.clone(),
                        intended_lines: block.intended_lines,
                    },
                )
            })
            .chain(contexts.entries.into_iter().map(|(context_path, edits)| {
                (
                    edits.order,
                    PlanAction::ApplyLineEditsUnderContext {
                        context_path,
                        line_edits: edits.into_line_edits(),
                    },
                )
            }))
            .collect::<Vec<_>>();
        actions.sort_by_key(|(order, action)| {
            let depth = match action {
                PlanAction::ReplaceBlock { target_path, .. } => target_path.0.len(),
                PlanAction::ApplyLineEditsUnderContext { context_path, .. } => context_path.0.len(),
            };
            (Reverse(depth), *order)
        });
        actions.into_iter().map(|(_, action)| action).collect()
    }
}

impl ContextEdits<'_> {
    /// deletes, then replaces, then inserts, each subtree whole and in document order.
    fn into_line_edits(mut self) -> Vec<PlanLineEdit> {
        self.deleted.sort_by(|a, b| a[0].path.0.cmp(&b[0].path.0));
        self.replaced.sort_by(|a, b| a.0.0.cmp(&b.0.0));
        self.inserted.sort_by(|a, b| a[0].path.0.cmp(&b[0].path.0));

        let line_edit = |kind: PlanLineEditKind, text: &str| PlanLineEdit {
            kind,
            text: text.to_string(),
        };
        let deleted = self
            .deleted
            .iter()
            .flatten()
            .map(|line| line_edit(PlanLineEditKind::Delete, &line.text));
        let replaced = self
            .replaced
            .iter()
            .map(|(_, text)| line_edit(PlanLineEditKind::Replace, text));
        let inserted = self
            .inserted
            .iter()
            .flatten()
            .map(|line| line_edit(PlanLineEditKind::Insert, &line.text));
        deleted.chain(replaced).chain(inserted).collect()
    }
}

/// splits lines into subtrees in document order, each a line whose parent is not among `lines`
/// followed by its descendants, with the earliest order among them and the root's tag.
fn subtrees<'a, T>(mut lines: Vec<(Placed<'a>, T)>) -> Vec<(usize, Vec<&'a DiffLine>, T)> {
    lines.sort_by(|a, b| a.0.line.path.0.cmp(&b.0.line.path.0));
    let members = lines
        .iter()
        .map(|(placed, _)| placed.line.path.clone())
        .collect::<HashSet<_>>();

    let mut out: Vec<(usize, Vec<&'a DiffLine>, T)> = Vec::new();
    for (placed, tag) in lines {
        let line = placed.line;
        match out.last_mut() {
            Some((order, fragment, _))
                if members.contains(&parent_path(&line.path))
                    && path_starts_with(&line.path.0, &fragment[0].path.0) =>
            {
                *order = (*order).min(placed.order);
                fragment.push(line);
            }
            _ => out.push((placed.order, vec![line], tag)),
        }
    }
    out
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
    fn insert_is_planned_under_its_left_anchor() {
        let diff = diff_with_edits(vec![Edit::Insert {
            at_key: None,
            left_anchor: Some(anchor(vec![0, 4], 3)),
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
                assert_eq!(context_path, &Path(vec![0, 4]));
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
                left_anchor: Some(anchor(vec![0, 1], 2)),
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
                assert_eq!(line_edits[0].kind, PlanLineEditKind::Delete);
                assert_eq!(line_edits[0].text, "line b");
                assert_eq!(line_edits[1].kind, PlanLineEditKind::Insert);
                assert_eq!(line_edits[1].text, "line a");
            }
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }

    #[test]
    fn edits_under_different_contexts_stay_separate() {
        let diff = diff_with_edits(vec![
            Edit::Insert {
                at_key: None,
                left_anchor: Some(anchor(vec![0, 1], 2)),
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
                left_anchor: Some(anchor(vec![0, 1], 2)),
                right_anchor: Some(anchor(vec![0, 1, 2], 3)),
                lines: vec![diff_line("first", vec![0, 1, 2], 3)],
            },
            Edit::Insert {
                at_key: None,
                left_anchor: Some(anchor(vec![0, 1], 2)),
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

    fn line_edits(action: &PlanAction) -> (&Path, Vec<(PlanLineEditKind, &str)>) {
        match action {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => (
                context_path,
                line_edits
                    .iter()
                    .map(|edit| (edit.kind.clone(), edit.text.as_str()))
                    .collect(),
            ),
            other => panic!("expected ApplyLineEditsUnderContext, got {other:?}"),
        }
    }

    #[test]
    fn delete_and_insert_sharing_a_path_on_different_sides_stay_separate() {
        let diff = diff_with_edits(vec![
            Edit::Delete {
                at_key: None,
                left_anchor: Some(anchor(vec![1, 0], 3)),
                right_anchor: None,
                lines: vec![diff_line(" description srv1", vec![1, 0], 3)],
            },
            Edit::Insert {
                at_key: None,
                left_anchor: Some(anchor(vec![2], 5)),
                right_anchor: Some(anchor(vec![1, 1], 4)),
                lines: vec![diff_line(" description srv1", vec![1, 1], 4)],
            },
        ]);

        let plan = build_plan(&diff);

        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(
            line_edits(&plan.actions[0]),
            (
                &Path(vec![1]),
                vec![(PlanLineEditKind::Delete, " description srv1")]
            )
        );
        assert_eq!(
            line_edits(&plan.actions[1]),
            (
                &Path(vec![2]),
                vec![(PlanLineEditKind::Insert, " description srv1")]
            )
        );
    }

    #[test]
    fn lines_inside_a_new_block_follow_its_header() {
        let diff = diff_with_edits(vec![
            Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: Some(anchor(vec![0, 1, 0], 3)),
                lines: vec![diff_line("  network 10.0.0.0/8", vec![0, 1, 0], 3)],
            },
            Edit::Insert {
                at_key: None,
                left_anchor: Some(anchor(vec![0], 1)),
                right_anchor: Some(anchor(vec![0, 1], 2)),
                lines: vec![diff_line(" address-family ipv4", vec![0, 1], 2)],
            },
            Edit::Insert {
                at_key: None,
                left_anchor: Some(anchor(vec![0], 1)),
                right_anchor: Some(anchor(vec![0, 2], 4)),
                lines: vec![diff_line(" neighbor 1.1.1.1", vec![0, 2], 4)],
            },
        ]);

        let plan = build_plan(&diff);

        assert!(plan.findings.is_empty());
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            line_edits(&plan.actions[0]),
            (
                &Path(vec![0]),
                vec![
                    (PlanLineEditKind::Insert, " address-family ipv4"),
                    (PlanLineEditKind::Insert, "  network 10.0.0.0/8"),
                    (PlanLineEditKind::Insert, " neighbor 1.1.1.1"),
                ]
            )
        );
    }

    #[test]
    fn lines_inside_a_deleted_block_follow_its_header() {
        let diff = diff_with_edits(vec![
            Edit::Delete {
                at_key: None,
                left_anchor: Some(anchor(vec![1, 0], 3)),
                right_anchor: None,
                lines: vec![diff_line(" mtu 9000", vec![1, 0], 3)],
            },
            Edit::Delete {
                at_key: None,
                left_anchor: Some(anchor(vec![0], 1)),
                right_anchor: None,
                lines: vec![diff_line("hostname x", vec![0], 1)],
            },
            Edit::Delete {
                at_key: None,
                left_anchor: Some(anchor(vec![1], 2)),
                right_anchor: None,
                lines: vec![diff_line("interface Ethernet1", vec![1], 2)],
            },
        ]);

        let plan = build_plan(&diff);

        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            line_edits(&plan.actions[0]),
            (
                &Path(vec![]),
                vec![
                    (PlanLineEditKind::Delete, "hostname x"),
                    (PlanLineEditKind::Delete, "interface Ethernet1"),
                    (PlanLineEditKind::Delete, " mtu 9000"),
                ]
            )
        );
    }

    #[test]
    fn nested_insert_without_left_context_produces_finding() {
        let diff = diff_with_edits(vec![Edit::Insert {
            at_key: None,
            left_anchor: None,
            right_anchor: Some(anchor(vec![0, 1, 2], 5)),
            lines: vec![diff_line("orphan", vec![0, 1, 2], 5)],
        }]);

        let plan = build_plan(&diff);

        assert!(plan.actions.is_empty());
        assert_eq!(plan.findings.len(), 1);
        assert_eq!(plan.findings[0].code, finding_code::MISSING_ANCHOR);
        assert!(plan.findings[0].message.contains("left"));
    }
}
