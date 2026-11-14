use std::collections::{HashMap, HashSet};
use std::ops::Range;

use netform_ir::{Document, Node, NodeId, Path};

use crate::engine::{
    pair_in_order, pair_same_contents_first, sequence_hash, sibling_identity, tree_hash,
};
use crate::flatten::{
    Cut, MAX_NESTING_DEPTH, Truncation, content_counts, cut_fingerprint, extracted_key_counts,
    tree_fingerprints,
};
use crate::model::{
    ComparisonLine, ComparisonView, Finding, FindingLevel, NormalizeOptions, OrderPolicy,
    finding_code,
};

#[derive(Debug)]
pub(crate) struct DiffContext {
    ambiguous_content_keys: HashMap<u64, (usize, usize)>,
    ambiguous_extracted_keys: HashMap<String, (usize, usize)>,
    content_key_first_index: HashMap<u64, usize>,
    extracted_key_first_index: HashMap<String, usize>,
    a_span_line_first_index: HashMap<usize, usize>,
    b_span_line_first_index: HashMap<usize, usize>,
}

impl DiffContext {
    pub(crate) fn from_views(a: &ComparisonView, b: &ComparisonView) -> Self {
        let a_counts = content_counts(a);
        let b_counts = content_counts(b);
        let a_extracted_counts = extracted_key_counts(a);
        let b_extracted_counts = extracted_key_counts(b);

        let mut ambiguous_content_keys = HashMap::new();
        for (key, a_count) in &a_counts {
            if *a_count > 1
                && let Some(b_count) = b_counts.get(key)
                && *b_count > 1
            {
                ambiguous_content_keys.insert(*key, (*a_count, *b_count));
            }
        }

        let mut ambiguous_extracted_keys = HashMap::new();
        for (key, a_count) in &a_extracted_counts {
            if *a_count > 1
                && let Some(b_count) = b_extracted_counts.get(key)
                && *b_count > 1
            {
                ambiguous_extracted_keys.insert(key.clone(), (*a_count, *b_count));
            }
        }

        let mut content_key_first_index: HashMap<u64, usize> = HashMap::new();
        let mut extracted_key_first_index: HashMap<String, usize> = HashMap::new();
        let mut a_span_line_first_index: HashMap<usize, usize> = HashMap::new();
        for (idx, line) in a.lines.iter().enumerate() {
            content_key_first_index
                .entry(line.content_key)
                .or_insert(idx);
            if let Some(hint) = &line.key_hint {
                extracted_key_first_index.entry(hint.clone()).or_insert(idx);
            }
            a_span_line_first_index.entry(line.span.line).or_insert(idx);
        }

        let mut b_span_line_first_index: HashMap<usize, usize> = HashMap::new();
        for (idx, line) in b.lines.iter().enumerate() {
            b_span_line_first_index.entry(line.span.line).or_insert(idx);
        }

        Self {
            ambiguous_content_keys,
            ambiguous_extracted_keys,
            content_key_first_index,
            extracted_key_first_index,
            a_span_line_first_index,
            b_span_line_first_index,
        }
    }
}

/// a block whose children nest too deep to flatten, and whether they differ from the other side's.
pub(crate) struct TruncatedRegion<'a> {
    side: &'static str,
    header: &'a ComparisonLine,
    pub(crate) differs: bool,
}

fn node_at(doc: &Document, path: &[usize]) -> Option<NodeId> {
    let (&first, rest) = path.split_first()?;
    rest.iter()
        .try_fold(*doc.roots.get(first)?, |id, &index| match doc.node(id)? {
            Node::Block(block) => block.children.get(index).copied(),
            Node::Line(_) => None,
        })
}

/// each line's `tree_fingerprint` and `sequence_hash` over the lines nested under it in `view`.
fn visible_hashes(view: &ComparisonView) -> Vec<(u64, u64)> {
    let mut hashes = vec![(0, 0); view.lines.len()];
    let mut pending: Vec<Vec<(u64, u64)>> = Vec::new();
    for (index, line) in view.lines.iter().enumerate().rev() {
        let len = line.path.0.len();
        if pending.len() < len + 2 {
            pending.resize_with(len + 2, Vec::new);
        }
        let mut children = std::mem::take(&mut pending[len + 1]);
        children.reverse();
        let sequence = children.iter().map(|child| child.1).collect::<Vec<_>>();
        let tree = children.into_iter().map(|child| child.0).collect();
        hashes[index] = (
            tree_hash(&line.normalized, tree),
            sequence_hash(&line.normalized, &sequence),
        );
        pending[len].push(hashes[index]);
    }
    hashes
}

/// one side of the diff as `truncated_regions` walks it.
struct Side<'a> {
    doc: &'a Document,
    view: &'a ComparisonView,
    visible: Vec<(u64, u64)>,
    /// the truncated blocks by header line, and the header line of every block holding one.
    cuts: HashMap<usize, NodeId>,
    holds: HashSet<usize>,
    trees: HashMap<NodeId, (u64, u64)>,
}

/// a sibling the walk pairs.
struct Child {
    lines: Range<usize>,
    deep: (u64, u64),
    visible: (u64, u64),
    /// whether it is a block with, then without, the lines below the cut.
    block: (bool, bool),
}

impl<'a> Side<'a> {
    fn new(
        doc: &'a Document,
        view: &'a ComparisonView,
        truncations: &[Truncation],
        options: &NormalizeOptions,
    ) -> Self {
        let mut trees = HashMap::new();
        let mut holds = HashSet::new();
        for truncation in truncations {
            tree_fingerprints(doc, truncation.block, options, &mut trees);
            let mut depth = view.lines[truncation.line].path.0.len();
            holds.insert(truncation.line);
            for line in (0..truncation.line).rev() {
                let len = view.lines[line].path.0.len();
                if len < depth {
                    if !holds.insert(line) {
                        break;
                    }
                    depth = len;
                }
            }
        }
        Side {
            doc,
            view,
            visible: visible_hashes(view),
            cuts: truncations
                .iter()
                .map(|truncation| (truncation.line, truncation.block))
                .collect(),
            holds,
            trees,
        }
    }

    /// whether `child`, left unpaired, gets a region that differs.
    fn flags(&self, child: &Child, options: &NormalizeOptions) -> bool {
        let line = child.lines.start;
        match self.cuts.get(&line) {
            Some(&block) => {
                let path = &self.view.lines[line].path;
                let cut = Cut {
                    doc: self.doc,
                    block,
                    path,
                    trees: &self.trees,
                };
                cut_fingerprint(cut, cut, options).is_some()
            }
            None => self.holds.contains(&line),
        }
    }

    /// the children among `lines` of a block whose header has path length `depth`.
    fn children(
        &mut self,
        lines: Range<usize>,
        depth: usize,
        options: &NormalizeOptions,
    ) -> Vec<Child> {
        let view = self.view;
        let mut children = Vec::new();
        let mut start = lines.start;
        while start < lines.end {
            let component = view.lines[start].path.0.get(depth);
            let end = (start + 1..lines.end)
                .find(|&line| view.lines[line].path.0.get(depth) != component)
                .unwrap_or(lines.end);
            let visible = self.visible[start];
            let deep = match node_at(self.doc, &view.lines[start].path.0) {
                Some(node) if self.holds.contains(&start) => {
                    tree_fingerprints(self.doc, node, options, &mut self.trees);
                    self.trees.get(&node).copied().unwrap_or(visible)
                }
                _ => visible,
            };
            let block = end > start + 1;
            children.push(Child {
                lines: start..end,
                deep,
                visible,
                block: (block || self.cuts.contains_key(&start), block),
            });
            start = end;
        }
        children
    }
}

/// pairs `lefts` with `rights` as the engine pairs siblings, on the lines below the cut too if `deep`.
fn pair(
    (left, lefts): (&Side<'_>, &[Child]),
    (right, rights): (&Side<'_>, &[Child]),
    (policy, in_order): (OrderPolicy, bool),
    deep: bool,
) -> Vec<Option<usize>> {
    let hashes = |child: &Child| match deep {
        true => (child.deep, child.block.0),
        false => (child.visible, child.block.1),
    };
    if in_order {
        let keys = |side: &Side<'_>, children: &[Child]| {
            children
                .iter()
                .map(|child| {
                    let header = &side.view.lines[child.lines.start];
                    (header.content_key, hashes(child).0.1)
                })
                .collect::<Vec<_>>()
        };
        return pair_in_order(&keys(left, lefts), &keys(right, rights));
    }
    let keys = |side: &Side<'_>, children: &[Child]| {
        children
            .iter()
            .map(|child| {
                let ((tree, _), block) = hashes(child);
                let header = &side.view.lines[child.lines.start];
                block.then(|| (sibling_identity(header, policy), tree))
            })
            .collect::<Vec<_>>()
    };
    pair_same_contents_first(&keys(left, lefts), &keys(right, rights))
}

/// pairs each side's truncated blocks; a pair is reported once, on the left.
///
/// every level pairs from the roots down as the engine pairs siblings, cut lines included.
pub(crate) fn truncated_regions<'a>(
    a: &'a Document,
    a_view: &'a ComparisonView,
    a_truncations: &[Truncation],
    b: &'a Document,
    b_view: &'a ComparisonView,
    b_truncations: &[Truncation],
    options: &NormalizeOptions,
) -> Vec<TruncatedRegion<'a>> {
    if a_truncations.is_empty() && b_truncations.is_empty() {
        return Vec::new();
    }
    let mut left = Side::new(a, a_view, a_truncations, options);
    let mut right = Side::new(b, b_view, b_truncations, options);
    let mut partners = HashMap::new();
    let mut levels = vec![(0..a_view.lines.len(), 0..b_view.lines.len(), 0, false)];
    while let Some((left_lines, right_lines, depth, mispaired)) = levels.pop() {
        let policy = match depth {
            0 => options.policy_for_path(&Path(Vec::new())),
            _ => options.policy_for_path(&a_view.lines[left_lines.start - 1].path),
        };
        let in_order =
            policy == OrderPolicy::Ordered || (depth == 0 && policy == OrderPolicy::KeyedStable);
        let lefts = left.children(left_lines, depth, options);
        let rights = right.children(right_lines, depth, options);
        let sides = ((&left, &lefts[..]), (&right, &rights[..]));
        let visible = pair(sides.0, sides.1, (policy, in_order), false);
        let deep = pair(sides.0, sides.1, (policy, in_order), true);
        let paired = |partners: &[Option<usize>]| partners.iter().flatten().copied().collect();
        let (right_deep, right_visible): (HashSet<_>, HashSet<_>) =
            (paired(&deep), paired(&visible));
        let dropped = lefts
            .iter()
            .zip(&deep)
            .zip(&visible)
            .any(|((child, deep), visible)| {
                deep.is_none() && visible.is_some() && !left.flags(child, options)
            })
            || rights.iter().enumerate().any(|(index, child)| {
                right_visible.contains(&index)
                    && !right_deep.contains(&index)
                    && !right.flags(child, options)
            });
        let mispaired = mispaired || dropped;
        for ((child, partner), visible) in lefts.iter().zip(deep).zip(visible) {
            let Some(partner) = partner else {
                continue;
            };
            let other = &rights[partner];
            let mispaired =
                mispaired || (visible != Some(partner) && child.visible.0 != other.visible.0);
            let (line, other_line) = (child.lines.start, other.lines.start);
            if left.cuts.contains_key(&line) {
                partners.insert(line, (other_line, mispaired));
            } else if left.holds.contains(&line) || right.holds.contains(&other_line) {
                levels.push((
                    line + 1..child.lines.end,
                    other_line + 1..other.lines.end,
                    depth + 1,
                    mispaired,
                ));
            }
        }
    }

    let mut regions = Vec::new();
    let mut paired = HashSet::new();
    for truncation in a_truncations {
        let cut = Cut {
            doc: a,
            block: truncation.block,
            path: &a_view.lines[truncation.line].path,
            trees: &left.trees,
        };
        let Some(fingerprint) = cut_fingerprint(cut, cut, options) else {
            continue;
        };
        let partner = partners.get(&truncation.line);
        let other = partner.and_then(|&(line, _)| {
            let &block = right.cuts.get(&line)?;
            paired.insert(line);
            let other = Cut {
                doc: b,
                block,
                path: &b_view.lines[line].path,
                trees: &right.trees,
            };
            cut_fingerprint(other, cut, options)
        });
        regions.push(TruncatedRegion {
            side: "left",
            header: &a_view.lines[truncation.line],
            differs: other != Some(fingerprint) || partner.is_some_and(|&(_, mispaired)| mispaired),
        });
    }
    for truncation in b_truncations {
        let cut = Cut {
            doc: b,
            block: truncation.block,
            path: &b_view.lines[truncation.line].path,
            trees: &right.trees,
        };
        if !paired.contains(&truncation.line) && cut_fingerprint(cut, cut, options).is_some() {
            regions.push(TruncatedRegion {
                side: "right",
                header: &b_view.lines[truncation.line],
                differs: true,
            });
        }
    }
    regions
}

pub(crate) fn collect_findings(
    a_doc: &Document,
    b_doc: &Document,
    a_view: &ComparisonView,
    b_view: &ComparisonView,
    ctx: &DiffContext,
    fallback_contexts: &[Path],
    truncated: &[TruncatedRegion<'_>],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    collect_parse_findings(
        a_doc,
        a_view,
        "left",
        &ctx.a_span_line_first_index,
        &mut findings,
    );
    collect_parse_findings(
        b_doc,
        b_view,
        "right",
        &ctx.b_span_line_first_index,
        &mut findings,
    );
    collect_unknown_block_findings(a_doc, "left", &mut findings);
    collect_unknown_block_findings(b_doc, "right", &mut findings);
    collect_ambiguity_findings(a_view, ctx, &mut findings);
    collect_extracted_key_ambiguity_findings(a_view, ctx, &mut findings);
    collect_fallback_alignment_findings(fallback_contexts, &mut findings);
    collect_truncation_findings(truncated, &mut findings);
    findings.sort_by(|a, b| {
        let ap = a.path.as_ref().map(|p| p.0.as_slice()).unwrap_or(&[]);
        let bp = b.path.as_ref().map(|p| p.0.as_slice()).unwrap_or(&[]);
        (a.message.as_str(), ap).cmp(&(b.message.as_str(), bp))
    });
    findings
}

fn push_ambiguity_finding(
    anchor: Option<&crate::model::ComparisonLine>,
    message: String,
    out: &mut Vec<Finding>,
) {
    out.push(Finding {
        code: finding_code::AMBIGUOUS_KEY_MATCH.to_string(),
        level: FindingLevel::Warning,
        message,
        path: anchor.map(|line| line.path.clone()),
        span: anchor.map(|line| line.span.clone()),
    });
}

fn collect_extracted_key_ambiguity_findings(
    a_view: &ComparisonView,
    ctx: &DiffContext,
    out: &mut Vec<Finding>,
) {
    let mut entries: Vec<_> = ctx.ambiguous_extracted_keys.iter().collect();
    entries.sort_by_key(|&(k, _)| k);
    for (key, &(left_count, right_count)) in entries {
        let anchor = ctx
            .extracted_key_first_index
            .get(key)
            .and_then(|&idx| a_view.lines.get(idx));
        push_ambiguity_finding(
            anchor,
            format!(
                "ambiguous extracted key `{}` appears {}x on left and {}x on right",
                key, left_count, right_count
            ),
            out,
        );
    }
}

fn collect_parse_findings(
    doc: &Document,
    view: &ComparisonView,
    side: &str,
    span_line_first_index: &HashMap<usize, usize>,
    out: &mut Vec<Finding>,
) {
    for pf in &doc.metadata.parse_findings {
        let matched_path = span_line_first_index
            .get(&pf.span.line)
            .and_then(|&idx| view.lines.get(idx))
            .map(|line| line.path.clone());
        out.push(Finding {
            code: finding_code::UNKNOWN_UNPARSED_CONSTRUCT.to_string(),
            level: FindingLevel::Warning,
            message: format!("{side} parse uncertainty [{}]: {}", pf.code, pf.message),
            path: matched_path,
            span: Some(pf.span.clone()),
        });
    }
}

fn collect_unknown_block_findings(doc: &Document, side: &str, out: &mut Vec<Finding>) {
    let mut path = Vec::new();
    let mut open = HashSet::new();
    let mut stack = vec![(None, doc.roots.iter().enumerate())];
    while let Some((_, siblings)) = stack.last_mut() {
        let Some((idx, &node_id)) = siblings.next() else {
            if let Some((Some(block), _)) = stack.pop() {
                open.remove(&block);
            }
            path.pop();
            continue;
        };
        path.push(idx);
        let Some(Node::Block(block)) = doc.node(node_id).filter(|_| !open.contains(&node_id))
        else {
            path.pop();
            continue;
        };
        open.insert(node_id);
        if block.kind_label.as_deref() == Some("unknown") {
            out.push(Finding {
                code: finding_code::UNKNOWN_UNPARSED_CONSTRUCT.to_string(),
                level: FindingLevel::Warning,
                message: format!("{side} document has an unknown block"),
                path: Some(Path(path.clone())),
                span: Some(block.header.span.clone()),
            });
        }
        stack.push((Some(node_id), block.children.iter().enumerate()));
    }
}

fn collect_ambiguity_findings(a_view: &ComparisonView, ctx: &DiffContext, out: &mut Vec<Finding>) {
    let mut entries: Vec<_> = ctx.ambiguous_content_keys.iter().collect();
    entries.sort_unstable_by_key(|&(&k, _)| k);
    for (&key, &(left_count, right_count)) in entries {
        let anchor = ctx
            .content_key_first_index
            .get(&key)
            .and_then(|&idx| a_view.lines.get(idx));
        push_ambiguity_finding(
            anchor,
            format!(
                "ambiguous content key {} appears {}x on left and {}x on right",
                crate::util::key_label(Some(key)),
                left_count,
                right_count
            ),
            out,
        );
    }
}

fn collect_fallback_alignment_findings(contexts: &[Path], out: &mut Vec<Finding>) {
    for context in contexts {
        out.push(Finding {
            code: finding_code::DIFF_UNRELIABLE_REGION.to_string(),
            level: FindingLevel::Warning,
            message: "diff used fallback segment alignment for this context".to_string(),
            path: Some(context.clone()),
            span: None,
        });
    }
}

fn collect_truncation_findings(truncated: &[TruncatedRegion<'_>], out: &mut Vec<Finding>) {
    for region in truncated {
        out.push(Finding {
            code: finding_code::DIFF_UNRELIABLE_REGION.to_string(),
            level: FindingLevel::Warning,
            message: format!(
                "{} document nests deeper than {} levels under `{}`; those lines were compared as a whole and {}",
                region.side,
                MAX_NESTING_DEPTH - 1,
                region.header.original.trim(),
                if region.differs { "differ" } else { "match" }
            ),
            path: Some(region.header.path.clone()),
            span: Some(region.header.span.clone()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::{BlockNode, LineNode, ParseFinding, Span, TriviaKind};

    fn make_line(content_key: u64, key_hint: Option<&str>) -> ComparisonLine {
        ComparisonLine {
            content_key,
            occurrence_key: content_key,
            key_hint: key_hint.map(|s| s.to_string()),
            normalized: format!("line-{content_key}"),
            original: format!("line-{content_key}"),
            path: Path(vec![0]),
            span: Span {
                line: 1,
                start_byte: 0,
                end_byte: 1,
            },
            trivia: TriviaKind::Content,
        }
    }

    fn line_at(content_key: u64, span_line: usize, path: Vec<usize>) -> ComparisonLine {
        ComparisonLine {
            content_key,
            occurrence_key: content_key,
            key_hint: None,
            normalized: format!("line-{content_key}"),
            original: format!("line-{content_key}"),
            path: Path(path),
            span: Span {
                line: span_line,
                start_byte: 0,
                end_byte: 1,
            },
            trivia: TriviaKind::Content,
        }
    }

    fn make_view(lines: Vec<ComparisonLine>) -> ComparisonView {
        ComparisonView { lines }
    }

    #[test]
    fn from_views_detects_content_key_ambiguity() {
        let a = make_view(vec![
            make_line(1, None),
            make_line(1, None),
            make_line(2, None),
        ]);
        let b = make_view(vec![make_line(1, None), make_line(1, None)]);

        let ctx = DiffContext::from_views(&a, &b);

        assert_eq!(ctx.ambiguous_content_keys.len(), 1);
        assert_eq!(ctx.ambiguous_content_keys[&1], (2, 2));
    }

    #[test]
    fn from_views_ignores_single_side_duplicates() {
        let a = make_view(vec![make_line(1, None), make_line(1, None)]);
        let b = make_view(vec![make_line(1, None)]);

        let ctx = DiffContext::from_views(&a, &b);

        assert!(ctx.ambiguous_content_keys.is_empty());
    }

    #[test]
    fn from_views_detects_extracted_key_ambiguity() {
        let a = make_view(vec![
            make_line(10, Some("interface:Eth1")),
            make_line(20, Some("interface:Eth1")),
        ]);
        let b = make_view(vec![
            make_line(30, Some("interface:Eth1")),
            make_line(40, Some("interface:Eth1")),
        ]);

        let ctx = DiffContext::from_views(&a, &b);

        assert_eq!(ctx.ambiguous_extracted_keys.len(), 1);
        assert_eq!(ctx.ambiguous_extracted_keys["interface:Eth1"], (2, 2));
    }

    #[test]
    fn from_views_no_extracted_ambiguity_when_unique() {
        let a = make_view(vec![make_line(10, Some("interface:Eth1"))]);
        let b = make_view(vec![make_line(30, Some("interface:Eth1"))]);

        let ctx = DiffContext::from_views(&a, &b);

        assert!(ctx.ambiguous_extracted_keys.is_empty());
    }

    #[test]
    fn fallback_alignment_findings_emitted_per_context() {
        let contexts = vec![Path(vec![0]), Path(vec![1, 2])];
        let mut out = Vec::new();

        collect_fallback_alignment_findings(&contexts, &mut out);

        assert_eq!(out.len(), 2);
        assert!(
            out.iter()
                .all(|f| f.code == finding_code::DIFF_UNRELIABLE_REGION)
        );
        assert_eq!(out[0].path.as_ref().unwrap().0, vec![0]);
        assert_eq!(out[1].path.as_ref().unwrap().0, vec![1, 2]);
    }

    #[test]
    fn ambiguity_findings_include_counts() {
        let a = make_view(vec![make_line(1, None), make_line(1, None)]);
        let b = make_view(vec![
            make_line(1, None),
            make_line(1, None),
            make_line(1, None),
        ]);
        let ctx = DiffContext::from_views(&a, &b);
        let mut out = Vec::new();

        collect_ambiguity_findings(&a, &ctx, &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, finding_code::AMBIGUOUS_KEY_MATCH);
        assert!(out[0].message.contains("2x on left"));
        assert!(out[0].message.contains("3x on right"));
        assert!(out[0].path.is_some());
    }

    #[test]
    fn extracted_key_ambiguity_findings_include_key_name() {
        let a = make_view(vec![
            make_line(10, Some("bgp:65000")),
            make_line(20, Some("bgp:65000")),
        ]);
        let b = make_view(vec![
            make_line(30, Some("bgp:65000")),
            make_line(40, Some("bgp:65000")),
        ]);
        let ctx = DiffContext::from_views(&a, &b);
        let mut out = Vec::new();

        collect_extracted_key_ambiguity_findings(&a, &ctx, &mut out);

        assert_eq!(out.len(), 1);
        assert!(out[0].message.contains("bgp:65000"));
        assert!(out[0].message.contains("ambiguous extracted key"));
    }

    #[test]
    fn parse_findings_propagated_with_side_label() {
        let mut doc = Document::default();
        doc.metadata.parse_findings.push(ParseFinding {
            code: "orphan-indentation".to_string(),
            message: "indented content".to_string(),
            span: Span {
                line: 1,
                start_byte: 0,
                end_byte: 10,
            },
        });
        let v = make_view(vec![]);
        let ctx = DiffContext::from_views(&v, &make_view(vec![]));
        let mut out = Vec::new();

        collect_parse_findings(&doc, &v, "left", &ctx.a_span_line_first_index, &mut out);

        assert_eq!(out.len(), 1);
        assert!(out[0].message.starts_with("left"));
        assert!(out[0].message.contains("orphan-indentation"));
    }

    #[test]
    fn parse_findings_anchor_to_first_view_line_on_span() {
        let mut doc = Document::default();
        for line in [2usize, 2, 3] {
            doc.metadata.parse_findings.push(ParseFinding {
                code: "orphan-indentation".to_string(),
                message: "indented content".to_string(),
                span: Span {
                    line,
                    start_byte: 0,
                    end_byte: 1,
                },
            });
        }
        let view = make_view(vec![
            line_at(1, 2, vec![0]),
            line_at(2, 2, vec![1]),
            line_at(3, 3, vec![2]),
        ]);
        let ctx = DiffContext::from_views(&view, &make_view(vec![]));
        let mut out = Vec::new();

        collect_parse_findings(&doc, &view, "left", &ctx.a_span_line_first_index, &mut out);

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].path.as_ref().unwrap().0, vec![0]);
        assert_eq!(out[1].path.as_ref().unwrap().0, vec![0]);
        assert_eq!(out[2].path.as_ref().unwrap().0, vec![2]);
    }

    #[test]
    fn unknown_block_findings_emitted() {
        let mut doc = Document::default();
        let child = doc.insert_node(Node::Line(LineNode {
            raw: "  child".to_string(),
            line_ending: "\n".to_string(),
            span: Span {
                line: 2,
                start_byte: 10,
                end_byte: 17,
            },
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        }));
        doc.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "header".to_string(),
                line_ending: "\n".to_string(),
                span: Span {
                    line: 1,
                    start_byte: 0,
                    end_byte: 6,
                },
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            },
            children: vec![child],
            footer: None,
            kind_label: Some("unknown".to_string()),
        }));
        let mut out = Vec::new();

        collect_unknown_block_findings(&doc, "right", &mut out);

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].code, finding_code::UNKNOWN_UNPARSED_CONSTRUCT);
        assert_eq!(out[0].path, Some(Path(vec![0])));
        assert_eq!(
            out[0].span,
            Some(Span {
                line: 1,
                start_byte: 0,
                end_byte: 6,
            })
        );
        assert!(out[0].message.contains("right"));
        assert!(out[0].message.contains("unknown block"));
    }

    #[test]
    fn unknown_block_findings_reach_any_depth() {
        let line = |raw: &str| LineNode {
            raw: raw.to_string(),
            line_ending: "\n".to_string(),
            span: Span {
                line: 1,
                start_byte: 0,
                end_byte: 0,
            },
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        };
        let block = |children, kind_label: Option<&str>| {
            Node::Block(BlockNode {
                header: line("block"),
                children,
                footer: None,
                kind_label: kind_label.map(str::to_string),
            })
        };
        let mut doc = Document::default();
        doc.insert_root(Node::Line(line("before")));
        let mut chain = doc.insert_node(Node::Line(line("leaf")));
        chain = doc.insert_node(block(vec![chain], Some("unknown")));
        for _ in 1..100_000 {
            chain = doc.insert_node(block(vec![chain], None));
        }
        doc.roots.push(chain);
        let leaf = doc.insert_node(Node::Line(line("leaf")));
        doc.insert_root(block(vec![leaf], Some("unknown")));
        let mut out = Vec::new();

        collect_unknown_block_findings(&doc, "left", &mut out);

        let mut deep = vec![0; 100_000];
        deep[0] = 1;
        let paths = out.into_iter().map(|f| f.path).collect::<Vec<_>>();
        assert_eq!(paths, vec![Some(Path(deep)), Some(Path(vec![2]))]);
    }

    #[test]
    fn collect_findings_sorts_by_message_then_path() {
        let a_doc = Document::default();
        let b_doc = Document::default();
        let a_view = make_view(vec![]);
        let b_view = make_view(vec![]);
        let ctx = DiffContext::from_views(&a_view, &b_view);
        let contexts = vec![Path(vec![1]), Path(vec![0])];

        let findings = collect_findings(&a_doc, &b_doc, &a_view, &b_view, &ctx, &contexts, &[]);

        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].path.as_ref().unwrap().0, vec![0]);
        assert_eq!(findings[1].path.as_ref().unwrap().0, vec![1]);
    }
}
