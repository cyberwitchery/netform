use std::collections::{HashMap, HashSet};

use netform_ir::{Document, Node, NodeId, Path};

use crate::engine::{pair_same_contents_first, sibling_identity, tree_fingerprint};
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

/// a truncated block, and its parent.
struct Sibling<'a> {
    cut: Cut<'a>,
    header: &'a ComparisonLine,
    line: usize,
    /// the index of the parent's header in the view, and the occurrence key of its parent's.
    parent: (usize, u64),
    /// how many earlier children of the parent share the header's content key.
    rank: u64,
}

fn siblings<'a>(
    doc: &'a Document,
    view: &'a ComparisonView,
    truncations: &[Truncation],
    trees: &'a HashMap<NodeId, u64>,
) -> Vec<Sibling<'a>> {
    let mut grandparent = 0;
    let mut parent = (0, 0);
    let mut ranks = HashMap::new();
    let mut scanned = 0;
    truncations
        .iter()
        .map(|truncation| {
            let header = &view.lines[truncation.line];
            let depth = header.path.0.len();
            for (index, line) in view.lines[..=truncation.line]
                .iter()
                .enumerate()
                .skip(scanned)
            {
                match line.path.0.len() + 2 {
                    len if len == depth => grandparent = line.occurrence_key,
                    len if len == depth + 1 => {
                        parent = (index, grandparent);
                        ranks.clear();
                    }
                    len if len == depth + 2 => {
                        *ranks.entry(line.content_key).or_insert(0u64) += 1;
                    }
                    _ => {}
                }
            }
            scanned = truncation.line + 1;
            Sibling {
                cut: Cut {
                    doc,
                    block: truncation.block,
                    path: &header.path,
                    trees,
                },
                header,
                line: truncation.line,
                parent,
                rank: ranks[&header.content_key] - 1,
            }
        })
        .collect()
}

fn node_at(doc: &Document, path: &[usize]) -> Option<NodeId> {
    let (&first, rest) = path.split_first()?;
    rest.iter()
        .try_fold(*doc.roots.get(first)?, |id, &index| match doc.node(id)? {
            Node::Block(block) => block.children.get(index).copied(),
            Node::Line(_) => None,
        })
}

/// the `tree_fingerprints` of each block at the depth of a truncated block's parent.
fn cut_trees(
    doc: &Document,
    view: &ComparisonView,
    options: &NormalizeOptions,
) -> HashMap<NodeId, u64> {
    let mut trees = HashMap::new();
    for line in &view.lines {
        if line.path.0.len() + 2 == MAX_NESTING_DEPTH
            && let Some(node) = node_at(doc, &line.path.0)
        {
            tree_fingerprints(doc, node, options, &mut trees);
        }
    }
    trees
}

/// a block at the depth of a truncated block's parent.
struct Parent {
    line: usize,
    grandparent: u64,
    /// its `tree_fingerprint` with and without the lines below the cut.
    trees: (u64, u64),
}

fn parents(doc: &Document, view: &ComparisonView, trees: &HashMap<NodeId, u64>) -> Vec<Parent> {
    let depth = MAX_NESTING_DEPTH - 2;
    let mut parents = Vec::new();
    let mut grandparent = 0;
    for (line, header) in view.lines.iter().enumerate() {
        match header.path.0.len() {
            len if len + 1 == depth => grandparent = header.occurrence_key,
            len if len == depth => {
                let end = view.lines[line + 1..]
                    .iter()
                    .position(|next| next.path.0.len() <= depth)
                    .map_or(view.lines.len(), |offset| line + 1 + offset);
                let deep = node_at(doc, &header.path.0).and_then(|node| trees.get(&node));
                if let (true, Some(&deep)) = (end > line + 1, deep) {
                    let visible = tree_fingerprint(&view.lines[line..end]);
                    parents.push(Parent {
                        line,
                        grandparent,
                        trees: (deep, visible),
                    });
                }
            }
            _ => {}
        }
    }
    parents
}

/// pairs `lefts` with `rights`, each `(identity, rank, tree_fingerprint)`, as the engine would.
fn pair_siblings(
    policy: OrderPolicy,
    lefts: &[(u64, u64, u64)],
    rights: &[(u64, u64, u64)],
) -> Vec<Option<usize>> {
    if policy == OrderPolicy::Ordered {
        let by_rank = rights
            .iter()
            .enumerate()
            .map(|(index, &(identity, rank, _))| ((identity, rank), index))
            .collect::<HashMap<_, _>>();
        return lefts
            .iter()
            .map(|&(identity, rank, _)| by_rank.get(&(identity, rank)).copied())
            .collect();
    }
    let keys = |siblings: &[(u64, u64, u64)]| {
        siblings
            .iter()
            .map(|&(identity, _, tree)| Some((identity, tree)))
            .collect::<Vec<_>>()
    };
    pair_same_contents_first(&keys(lefts), &keys(rights))
}

/// pairs each side's truncated blocks; a pair is reported once, on the left.
///
/// parents pair under the policy of the left grandparent's path, blocks under that of the left
/// parent's, as the engine pairs siblings: by rank if `ordered`, else same `tree_fingerprint` first.
pub(crate) fn truncated_regions<'a>(
    a: &'a Document,
    a_view: &'a ComparisonView,
    a_truncations: &[Truncation],
    b: &'a Document,
    b_view: &'a ComparisonView,
    b_truncations: &[Truncation],
    options: &NormalizeOptions,
) -> Vec<TruncatedRegion<'a>> {
    let a_trees = cut_trees(a, a_view, options);
    let b_trees = cut_trees(b, b_view, options);
    let a_parents = parents(a, a_view, &a_trees);
    let b_parents = parents(b, b_view, &b_trees);
    let parent_partners = pair_parents((a_view, &a_parents), (b_view, &b_parents), options, |p| {
        p.trees.0
    });
    let visible_partners = pair_parents((a_view, &a_parents), (b_view, &b_parents), options, |p| {
        p.trees.1
    });
    let b_visible = b_parents
        .iter()
        .map(|parent| (parent.line, parent.trees.1))
        .collect::<HashMap<_, _>>();
    let mispaired = a_parents
        .iter()
        .filter(|parent| {
            let partner = parent_partners.get(&parent.line);
            partner != visible_partners.get(&parent.line)
                && partner.is_none_or(|right| b_visible.get(right) != Some(&parent.trees.1))
        })
        .map(|parent| parent.grandparent)
        .collect::<HashSet<_>>();
    let left = siblings(a, a_view, a_truncations, &a_trees)
        .into_iter()
        .filter_map(|left| Some((cut_fingerprint(left.cut, left.cut, options)?, left)))
        .collect::<Vec<_>>();
    let right = siblings(b, b_view, b_truncations, &b_trees);

    let right_parents = parent_partners
        .iter()
        .map(|(&left, &right)| (right, left))
        .collect::<HashMap<_, _>>();

    let mut policies = HashMap::new();
    let mut buckets = HashMap::new();
    for (index, (_, sibling)) in left.iter().enumerate() {
        let parent = sibling.parent.0;
        let policy = *policies
            .entry(parent)
            .or_insert_with(|| options.policy_for_path(&Path(a_view.lines[parent].path.0.clone())));
        let identity = sibling_identity(sibling.header, policy);
        buckets
            .entry((parent, identity))
            .or_insert_with(|| (policy, Vec::new(), Vec::new()))
            .1
            .push(index);
    }
    for (index, sibling) in right.iter().enumerate() {
        let Some(&parent) = right_parents.get(&sibling.parent.0) else {
            continue;
        };
        if let Some(&policy) = policies.get(&parent) {
            let identity = sibling_identity(sibling.header, policy);
            if let Some(bucket) = buckets.get_mut(&(parent, identity)) {
                bucket.2.push(index);
            }
        }
    }

    let mut partners = vec![None; left.len()];
    let mut paired = vec![false; right.len()];
    let key = |sibling: &Sibling<'_>, policy| {
        (
            sibling_identity(sibling.header, policy),
            sibling.rank,
            sibling
                .cut
                .trees
                .get(&sibling.cut.block)
                .copied()
                .unwrap_or_default(),
        )
    };
    for (policy, lefts, rights) in buckets.values() {
        let pairs = pair_siblings(
            *policy,
            &lefts
                .iter()
                .map(|&index| key(&left[index].1, *policy))
                .collect::<Vec<_>>(),
            &rights
                .iter()
                .map(|&index| key(&right[index], *policy))
                .collect::<Vec<_>>(),
        );
        for (&index, partner) in lefts.iter().zip(pairs) {
            if let Some(partner) = partner {
                partners[index] = Some(rights[partner]);
                paired[rights[partner]] = true;
            }
        }
    }

    let mut regions = Vec::new();
    for ((fingerprint, sibling), partner) in left.iter().zip(&partners) {
        let right =
            partner.and_then(|partner| cut_fingerprint(right[partner].cut, sibling.cut, options));
        regions.push(TruncatedRegion {
            side: "left",
            header: &a_view.lines[sibling.line],
            differs: right != Some(*fingerprint) || mispaired.contains(&sibling.parent.1),
        });
    }
    for (sibling, paired) in right.iter().zip(paired) {
        if !paired && cut_fingerprint(sibling.cut, sibling.cut, options).is_some() {
            regions.push(TruncatedRegion {
                side: "right",
                header: &b_view.lines[sibling.line],
                differs: true,
            });
        }
    }
    regions
}

/// the right parent each left parent is paired with on `tree`, by view index.
fn pair_parents(
    (a_view, lefts): (&ComparisonView, &[Parent]),
    (b_view, rights): (&ComparisonView, &[Parent]),
    options: &NormalizeOptions,
    tree: impl Fn(&Parent) -> u64,
) -> HashMap<usize, usize> {
    let mut policies = HashMap::new();
    let mut groups = HashMap::new();
    for parent in lefts {
        let (line, grandparent) = (parent.line, parent.grandparent);
        let header = &a_view.lines[line];
        let policy = *policies.entry(grandparent).or_insert_with(|| {
            let path = &header.path.0;
            options.policy_for_path(&Path(path[..path.len() - 1].to_vec()))
        });
        groups
            .entry((grandparent, sibling_identity(header, policy)))
            .or_insert_with(|| (policy, Vec::new(), Vec::new()))
            .1
            .push((line, header.occurrence_key, tree(parent)));
    }
    for parent in rights {
        let (line, grandparent) = (parent.line, parent.grandparent);
        let header = &b_view.lines[line];
        if let Some(&policy) = policies.get(&grandparent)
            && let Some(group) = groups.get_mut(&(grandparent, sibling_identity(header, policy)))
        {
            group.2.push((line, header.occurrence_key, tree(parent)));
        }
    }

    let mut partners = HashMap::new();
    for (policy, lefts, rights) in groups.into_values() {
        let key = |&(_, occurrence, tree): &(usize, u64, u64)| (0, occurrence, tree);
        let pairs = pair_siblings(
            policy,
            &lefts.iter().map(key).collect::<Vec<_>>(),
            &rights.iter().map(key).collect::<Vec<_>>(),
        );
        for (&(left, _, _), partner) in lefts.iter().zip(pairs) {
            if let Some(partner) = partner {
                partners.insert(left, rights[partner].0);
            }
        }
    }
    partners
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
