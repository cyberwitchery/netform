use std::collections::{HashMap, HashSet};

use netform_ir::{BlockNode, DialectHint, Document, LineNode, Node, NodeId, Path, TriviaKind};
use xxhash_rust::xxh3::Xxh3Default;

use crate::engine::{pair_same_contents_first, tree_hash};
use crate::model::{
    ComparisonLine, ComparisonView, KeyKind, NormalizeOptions, OrderPolicy, derive_content_key,
    derive_occurrence_key,
};
use crate::normalize::normalize_for_compare;

/// maximum nesting depth for `flatten_node` recursion.  real-world network
/// configs rarely exceed 10–15 levels; a limit of 128 is generous enough to
/// handle any legitimate document while preventing stack overflow on
/// pathologically deep (or cyclic) input.
pub(crate) const MAX_NESTING_DEPTH: usize = 128;

/// a block whose children nest too deep to flatten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Truncation {
    /// index of the block's header in the view.
    pub(crate) line: usize,
    pub(crate) block: NodeId,
}

/// a truncated block in its document, at the path of its header.
#[derive(Clone, Copy)]
pub(crate) struct Cut<'a> {
    pub(crate) doc: &'a Document,
    pub(crate) block: NodeId,
    pub(crate) path: &'a Path,
    /// the `tree_fingerprints` of its document's blocks.
    pub(crate) trees: &'a HashMap<NodeId, u64>,
}

#[derive(Debug, Default)]
struct Flattened {
    lines: Vec<ComparisonLine>,
    truncations: Vec<Truncation>,
}

#[derive(Debug, Default)]
struct KeyAllocator {
    counters: HashMap<u64, u64>,
    annotations: &'static [&'static str],
}

impl KeyAllocator {
    fn next_keys(
        &mut self,
        parent_signature: u64,
        kind: KeyKind,
        trivia: TriviaKind,
        normalized_for_key: &str,
    ) -> (u64, u64) {
        // a header keys as a line, so a statement keeps its key with or without children.
        let kind = match kind {
            KeyKind::BlockHeader => KeyKind::Line,
            kind => kind,
        };
        let content_key = derive_content_key(parent_signature, kind, trivia, normalized_for_key);

        let ordinal = self.counters.entry(content_key).or_insert(0);
        *ordinal += 1;

        let occurrence_key = derive_occurrence_key(content_key, *ordinal);

        (content_key, occurrence_key)
    }
}

/// build a flattened comparison view from a parsed document.
pub fn build_comparison_view(doc: &Document, options: &NormalizeOptions) -> ComparisonView {
    flatten_document(doc, options).0
}

/// the comparison view of `doc`, and each block whose children it leaves out.
pub(crate) fn flatten_document(
    doc: &Document,
    options: &NormalizeOptions,
) -> (ComparisonView, Vec<Truncation>) {
    let mut out = Flattened::default();
    let mut keys = KeyAllocator {
        annotations: annotations(doc),
        ..KeyAllocator::default()
    };
    let mut path = Vec::new();

    for (idx, root) in doc.roots.iter().copied().enumerate() {
        path.clear();
        path.push(idx);
        flatten_node(doc, root, 0, &mut path, &mut out, &mut keys, options);
    }

    (ComparisonView { lines: out.lines }, out.truncations)
}

/// compute keys for a line and push a [`ComparisonLine`] onto the output.
fn push_comparison_line(
    out: &mut Vec<ComparisonLine>,
    keys: &mut KeyAllocator,
    parent_signature: u64,
    kind: KeyKind,
    line: &netform_ir::LineNode,
    normalized: String,
    path: &[usize],
) -> u64 {
    let key_material = key_material_for_line(
        kind,
        line.trivia,
        line.key_hint.as_deref(),
        normalized.as_str(),
        keys.annotations,
    );
    let (content_key, occurrence_key) = keys.next_keys(
        parent_signature,
        kind,
        line.trivia,
        key_material.for_hash.as_str(),
    );

    out.push(ComparisonLine {
        content_key,
        occurrence_key,
        key_hint: key_material.hint,
        normalized,
        original: line.raw.clone(),
        path: Path(path.to_vec()),
        span: line.span.clone(),
        trivia: line.trivia,
    });

    content_key
}

fn flatten_node(
    doc: &Document,
    node_id: NodeId,
    parent_signature: u64,
    path: &mut Vec<usize>,
    out: &mut Flattened,
    keys: &mut KeyAllocator,
    options: &NormalizeOptions,
) {
    let Some(node) = doc.node(node_id) else {
        return;
    };
    let quoted_values = quotes_delimit_values(doc);

    match node {
        Node::Line(line) => {
            if let Some(normalized) =
                normalize_for_compare(&line.raw, line.trivia, quoted_values, options)
            {
                push_comparison_line(
                    &mut out.lines,
                    keys,
                    parent_signature,
                    KeyKind::Line,
                    line,
                    normalized,
                    path,
                );
            }
        }
        Node::Block(block) => {
            if let Some(normalized) = normalize_for_compare(
                &block.header.raw,
                block.header.trivia,
                quoted_values,
                options,
            ) {
                let header = out.lines.len();
                let header_content_key = push_comparison_line(
                    &mut out.lines,
                    keys,
                    parent_signature,
                    KeyKind::BlockHeader,
                    &block.header,
                    normalized,
                    path,
                );

                if path.len() + 1 < MAX_NESTING_DEPTH {
                    for (child_idx, child_id) in block.children.iter().copied().enumerate() {
                        path.push(child_idx);
                        flatten_node(doc, child_id, header_content_key, path, out, keys, options);
                        path.pop();
                    }
                } else if !block.children.is_empty() {
                    out.truncations.push(Truncation {
                        line: header,
                        block: node_id,
                    });
                }

                if let Some(footer) = &block.footer {
                    path.push(block.children.len());

                    if let Some(footer_normalized) =
                        normalize_for_compare(&footer.raw, footer.trivia, quoted_values, options)
                    {
                        push_comparison_line(
                            &mut out.lines,
                            keys,
                            header_content_key,
                            KeyKind::BlockFooter,
                            footer,
                            footer_normalized,
                            path,
                        );
                    }

                    path.pop();
                }
            }
        }
    }
}

/// the left blocks the emitted children of a block a level below a cut are paired with.
struct Guide<'g> {
    ordered: bool,
    partners: Vec<Option<(usize, &'g BlockNode)>>,
}

/// an emitted child's index, and its header text, id and node if it is a block.
type Emitted<'d> = (usize, Option<(String, NodeId, &'d BlockNode)>);

fn emitted<'d>(
    cut: &Cut<'d>,
    block: &'d BlockNode,
    options: &NormalizeOptions,
) -> Vec<Emitted<'d>> {
    let quoted_values = quotes_delimit_values(cut.doc);
    block
        .children
        .iter()
        .enumerate()
        .filter_map(|(idx, &id)| match cut.doc.node(id)? {
            Node::Line(line) => {
                normalize_for_compare(&line.raw, line.trivia, quoted_values, options)?;
                Some((idx, None))
            }
            Node::Block(child) => normalize_for_compare(
                &child.header.raw,
                child.header.trivia,
                quoted_values,
                options,
            )
            .map(|header| (idx, Some((header, id, child)))),
        })
        .collect()
}

fn contents_keys(cut: &Cut<'_>, children: &[Emitted<'_>]) -> Vec<Option<(String, u64)>> {
    children
        .iter()
        .map(|(_, child)| {
            child.as_ref().map(|(header, id, _)| {
                (
                    header.clone(),
                    cut.trees.get(id).copied().unwrap_or_default(),
                )
            })
        })
        .collect()
}

impl<'g> Guide<'g> {
    /// pairs `subject`'s emitted children with `guide`'s: in order if `path` is ordered, else as `pair_blocks` does.
    fn new(
        subject: (&Cut<'_>, &BlockNode),
        guide: (&Cut<'g>, &'g BlockNode),
        path: &Path,
        options: &NormalizeOptions,
    ) -> Self {
        let ordered = options.policy_for_path(path) == OrderPolicy::Ordered;
        let guide_children = emitted(guide.0, guide.1, options);
        let block = |partner: usize| {
            let (idx, child) = &guide_children[partner];
            child.as_ref().map(|&(_, _, node)| (*idx, node))
        };
        let partners = if ordered {
            (0..guide_children.len()).map(block).collect()
        } else {
            let subject_children = emitted(subject.0, subject.1, options);
            pair_same_contents_first(
                &contents_keys(subject.0, &subject_children),
                &contents_keys(guide.0, &guide_children),
            )
            .into_iter()
            .map(|partner| partner.and_then(block))
            .collect()
        };
        Guide { ordered, partners }
    }

    /// the block paired with the subject's emitted child at `position`.
    fn pair(&self, position: usize) -> Option<(usize, &'g BlockNode)> {
        self.partners.get(position).copied().flatten()
    }
}

/// hashes the lines `flatten_node` leaves out below `subject`, or `None` when there are none.
///
/// each level takes its order policy from the path of its paired block under `guide`, the left cut,
/// or, when it has none, of its nearest paired ancestor.
pub(crate) fn cut_fingerprint(
    subject: Cut<'_>,
    guide: Cut<'_>,
    options: &NormalizeOptions,
) -> Option<u64> {
    struct Level<'s, 'g> {
        block: NodeId,
        header: Option<String>,
        children: std::slice::Iter<'s, NodeId>,
        footer: Option<String>,
        hashes: Vec<u64>,
        guide: Option<Guide<'g>>,
    }

    let quoted_values = quotes_delimit_values(subject.doc);
    let normalize =
        |line: &LineNode| normalize_for_compare(&line.raw, line.trivia, quoted_values, options);
    let (Some(Node::Block(root)), Some(Node::Block(guide_root))) =
        (subject.doc.node(subject.block), guide.doc.node(guide.block))
    else {
        return None;
    };
    let mut guide_path = guide.path.clone();
    let mut open = HashSet::from([subject.block]);
    let mut stack = vec![Level {
        block: subject.block,
        header: None,
        children: root.children.iter(),
        footer: None,
        hashes: Vec::new(),
        guide: Some(Guide::new(
            (&subject, root),
            (&guide, guide_root),
            &guide_path,
            options,
        )),
    }];

    while let Some(level) = stack.last_mut() {
        if let Some(&child_id) = level.children.next() {
            match subject.doc.node(child_id) {
                Some(Node::Line(line)) => {
                    if let Some(text) = normalize(line) {
                        level.hashes.push(leaf_hash(LINE, &text));
                    }
                }
                Some(Node::Block(block)) => {
                    if let Some(header) = normalize(&block.header) {
                        if !open.insert(child_id) {
                            level.hashes.push(leaf_hash(CYCLE, &header));
                            continue;
                        }
                        let position = level.hashes.len();
                        let paired = level.guide.as_ref().and_then(|guide| guide.pair(position));
                        let child_guide = paired.map(|(idx, paired)| {
                            guide_path.0.push(idx);
                            Guide::new((&subject, block), (&guide, paired), &guide_path, options)
                        });
                        stack.push(Level {
                            block: child_id,
                            header: Some(header),
                            children: block.children.iter(),
                            footer: block.footer.as_ref().and_then(normalize),
                            hashes: Vec::new(),
                            guide: child_guide,
                        });
                    }
                }
                None => {}
            }
        } else if let Some(mut done) = stack.pop() {
            let ordered = done.guide.as_ref().map_or_else(
                || options.policy_for_path(&guide_path) == OrderPolicy::Ordered,
                |guide| guide.ordered,
            );
            if !ordered {
                done.hashes.sort_unstable();
            }
            open.remove(&done.block);
            let mut hasher = Xxh3Default::new();
            hasher.update(&[BLOCK]);
            update_field(&mut hasher, done.header.as_deref());
            hasher.update(&(done.hashes.len() as u64).to_le_bytes());
            if done.hashes.len() > 1 {
                hasher.update(&[u8::from(ordered)]);
            }
            for hash in &done.hashes {
                hasher.update(&hash.to_le_bytes());
            }
            update_field(&mut hasher, done.footer.as_deref());
            match stack.last_mut() {
                Some(parent) => {
                    parent.hashes.push(hasher.digest());
                    if done.guide.is_some() {
                        guide_path.0.pop();
                    }
                }
                None => return (!done.hashes.is_empty()).then(|| hasher.digest()),
            }
        }
    }
    None
}

/// records the `tree_fingerprint` of `root` and of each block below it in `trees`.
pub(crate) fn tree_fingerprints<'d>(
    doc: &'d Document,
    root: NodeId,
    options: &NormalizeOptions,
    trees: &mut HashMap<NodeId, u64>,
) {
    struct Level<'d> {
        block: NodeId,
        header: String,
        children: std::slice::Iter<'d, NodeId>,
        footer: Option<String>,
        hashes: Vec<u64>,
    }

    let quoted_values = quotes_delimit_values(doc);
    let normalize =
        |line: &LineNode| normalize_for_compare(&line.raw, line.trivia, quoted_values, options);
    let level = |block: NodeId, node: &'d BlockNode| {
        Some(Level {
            block,
            header: normalize(&node.header)?,
            children: node.children.iter(),
            footer: node.footer.as_ref().and_then(normalize),
            hashes: Vec::new(),
        })
    };
    let (false, Some(Node::Block(node))) = (trees.contains_key(&root), doc.node(root)) else {
        return;
    };
    let mut open = HashSet::from([root]);
    let mut stack = Vec::from_iter(level(root, node));

    while let Some(top) = stack.last_mut() {
        if let Some(&child) = top.children.next() {
            if let Some(&hash) = trees.get(&child) {
                top.hashes.push(hash);
                continue;
            }
            match doc.node(child) {
                Some(Node::Line(line)) => {
                    if let Some(text) = normalize(line) {
                        top.hashes.push(tree_hash(&text, Vec::new()));
                    }
                }
                Some(Node::Block(block)) => {
                    if !open.insert(child) {
                        top.hashes.push(leaf_hash(CYCLE, &block.header.raw));
                    } else if let Some(level) = level(child, block) {
                        stack.push(level);
                    }
                }
                None => {}
            }
        } else if let Some(mut done) = stack.pop() {
            if let Some(footer) = &done.footer {
                done.hashes.push(tree_hash(footer, Vec::new()));
            }
            let hash = tree_hash(&done.header, done.hashes);
            open.remove(&done.block);
            trees.insert(done.block, hash);
            if let Some(parent) = stack.last_mut() {
                parent.hashes.push(hash);
            }
        }
    }
}

const LINE: u8 = 0;
const BLOCK: u8 = 1;
const CYCLE: u8 = 2;

fn leaf_hash(tag: u8, text: &str) -> u64 {
    let mut hasher = Xxh3Default::new();
    hasher.update(&[tag]);
    hasher.update(text.as_bytes());
    hasher.digest()
}

/// feeds `text`, or its absence, to `hasher` length-prefixed.
fn update_field(hasher: &mut Xxh3Default, text: Option<&str>) {
    match text {
        Some(text) => {
            hasher.update(&[1]);
            hasher.update(&(text.len() as u64).to_le_bytes());
            hasher.update(text.as_bytes());
        }
        None => hasher.update(&[0]),
    }
}

/// FortiOS and Junos read `"…"` as a value; elsewhere a quote is plain text.
fn quotes_delimit_values(doc: &Document) -> bool {
    match &doc.metadata.dialect_hint {
        DialectHint::Named(name) => matches!(name.as_str(), "fortios" | "junos"),
        DialectHint::Generic | DialectHint::Unknown => false,
    }
}

/// leading tokens that flag a statement without changing which statement it is.
fn annotations(doc: &Document) -> &'static [&'static str] {
    match &doc.metadata.dialect_hint {
        DialectHint::Named(name) if name == "junos" => &["inactive:", "protect:"],
        DialectHint::Named(_) | DialectHint::Generic | DialectHint::Unknown => &[],
    }
}

/// `text` with its leading `annotations` removed and its indentation kept.
fn strip_annotations(text: &str, annotations: &[&str]) -> String {
    let body = text.trim_start();
    let mut rest = body;
    while let Some(after) = annotations
        .iter()
        .find_map(|annotation| rest.strip_prefix(annotation))
        .filter(|after| after.starts_with(char::is_whitespace))
    {
        rest = after.trim_start();
    }
    [&text[..text.len() - body.len()], rest].concat()
}

#[derive(Debug)]
struct KeyMaterial {
    for_hash: String,
    hint: Option<String>,
}

fn key_material_for_line(
    kind: KeyKind,
    trivia: TriviaKind,
    key_hint: Option<&str>,
    normalized: &str,
    annotations: &[&str],
) -> KeyMaterial {
    if trivia == TriviaKind::Content
        && kind != KeyKind::BlockFooter
        && let Some(hint) = key_hint
    {
        // leaf hints (e.g. FortiOS `set:<field>`) repeat across many sibling
        // blocks and would flood extracted-key ambiguity findings.
        return KeyMaterial {
            for_hash: format!("subkey:{hint}"),
            hint: (kind == KeyKind::BlockHeader).then(|| hint.to_string()),
        };
    }

    let for_hash = if trivia == TriviaKind::Content {
        strip_annotations(normalized, annotations)
    } else {
        normalized.to_string()
    };
    KeyMaterial {
        for_hash,
        hint: None,
    }
}

pub(crate) fn content_counts(view: &ComparisonView) -> HashMap<u64, usize> {
    let mut counts = HashMap::new();
    for line in &view.lines {
        *counts.entry(line.content_key).or_insert(0usize) += 1;
    }
    counts
}

pub(crate) fn extracted_key_counts(view: &ComparisonView) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in &view.lines {
        if let Some(hint) = &line.key_hint {
            *counts.entry(hint.clone()).or_insert(0usize) += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NormalizationStep;
    use netform_ir::{BlockNode, Document, LineNode, Node, Span, TriviaKind, parse_generic};

    fn default_opts() -> NormalizeOptions {
        NormalizeOptions::default()
    }

    fn dummy_span(line: usize) -> Span {
        Span {
            line,
            start_byte: 0,
            end_byte: 0,
        }
    }

    #[test]
    fn single_line_produces_one_comparison_line() {
        let doc = parse_generic("hostname edge-01\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 1);
        assert_eq!(view.lines[0].normalized, "hostname edge-01");
        assert_eq!(view.lines[0].original, "hostname edge-01");
        assert_eq!(view.lines[0].path, Path(vec![0]));
        assert_eq!(view.lines[0].trivia, TriviaKind::Content);
    }

    #[test]
    fn block_flattens_header_then_children() {
        let doc = parse_generic("interface Eth1\n  description foo\n  mtu 9000\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 3);
        assert_eq!(view.lines[0].normalized, "interface Eth1");
        assert_eq!(view.lines[0].path, Path(vec![0]));
        assert_eq!(view.lines[1].normalized, "  description foo");
        assert_eq!(view.lines[1].path, Path(vec![0, 0]));
        assert_eq!(view.lines[2].normalized, "  mtu 9000");
        assert_eq!(view.lines[2].path, Path(vec![0, 1]));
    }

    #[test]
    fn mixed_roots_and_blocks() {
        let doc = parse_generic("standalone\nparent\n  child\nanother\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 4);
        assert_eq!(view.lines[0].path, Path(vec![0]));
        assert_eq!(view.lines[1].path, Path(vec![1]));
        assert_eq!(view.lines[2].path, Path(vec![1, 0]));
        assert_eq!(view.lines[3].path, Path(vec![2]));
    }

    #[test]
    fn comment_lines_included_by_default() {
        let doc = parse_generic("! a comment\nhostname foo\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 2);
        assert_eq!(view.lines[0].trivia, TriviaKind::Comment);
        assert_eq!(view.lines[1].trivia, TriviaKind::Content);
    }

    #[test]
    fn comment_lines_dropped_with_ignore_comments() {
        let doc = parse_generic("! a comment\nhostname foo\n# another\n");
        let opts = NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]);
        let view = build_comparison_view(&doc, &opts);

        assert_eq!(view.lines.len(), 1);
        assert_eq!(view.lines[0].normalized, "hostname foo");
    }

    #[test]
    fn blank_lines_dropped_with_ignore_blank_lines() {
        let doc = parse_generic("line a\n\nline b\n");
        let opts = NormalizeOptions::new(vec![NormalizationStep::IgnoreBlankLines]);
        let view = build_comparison_view(&doc, &opts);

        assert_eq!(view.lines.len(), 2);
        assert_eq!(view.lines[0].normalized, "line a");
        assert_eq!(view.lines[1].normalized, "line b");
    }

    #[test]
    fn duplicate_lines_share_content_key_but_differ_in_occurrence_key() {
        for input in [
            "permit any\npermit any\n",
            "permit any\npermit any\n  log\n",
        ] {
            let doc = parse_generic(input);
            let view = build_comparison_view(&doc, &default_opts());

            assert_eq!(view.lines[0].content_key, view.lines[1].content_key);
            assert_ne!(view.lines[0].occurrence_key, view.lines[1].occurrence_key);
        }
    }

    #[test]
    fn children_keyed_relative_to_parent_block() {
        let doc = parse_generic("block A\n  child x\nblock B\n  child x\n");
        let view = build_comparison_view(&doc, &default_opts());

        let child_a = &view.lines[1];
        let child_b = &view.lines[3];
        assert_eq!(child_a.normalized, "  child x");
        assert_eq!(child_b.normalized, "  child x");
        assert_ne!(
            child_a.content_key, child_b.content_key,
            "same text under different parents should produce different content keys"
        );
    }

    #[test]
    fn block_with_footer_flattens_all_three_parts() {
        let mut doc = Document::default();

        let child_id = doc.insert_node(Node::Line(LineNode {
            raw: "  child line".to_string(),
            line_ending: "\n".to_string(),
            span: dummy_span(2),
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        }));

        let block_id = doc.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "begin".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            },
            children: vec![child_id],
            footer: Some(LineNode {
                raw: "end".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(3),
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            }),
            kind_label: None,
        }));

        let _ = block_id;
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 3);
        assert_eq!(view.lines[0].normalized, "begin");
        assert_eq!(view.lines[0].path, Path(vec![0]));
        assert_eq!(view.lines[1].normalized, "  child line");
        assert_eq!(view.lines[1].path, Path(vec![0, 0]));
        assert_eq!(view.lines[2].normalized, "end");
        assert_eq!(view.lines[2].path, Path(vec![0, 1]));
    }

    #[test]
    fn content_counts_aggregates_correctly() {
        let doc = parse_generic("permit any\npermit any\ndeny all\n");
        let view = build_comparison_view(&doc, &default_opts());
        let counts = content_counts(&view);

        let permit_key = view.lines[0].content_key;
        let deny_key = view.lines[2].content_key;

        assert_eq!(counts[&permit_key], 2);
        assert_eq!(counts[&deny_key], 1);
    }

    #[test]
    fn extracted_key_counts_aggregates_hints() {
        let mut doc = Document::default();

        for i in 0..3 {
            doc.insert_root(Node::Block(BlockNode {
                header: LineNode {
                    raw: format!("block {i}"),
                    line_ending: "\n".to_string(),
                    span: dummy_span(i + 1),
                    parsed: None,
                    key_hint: Some("stanza:X".to_string()),
                    trivia: TriviaKind::Content,
                },
                children: Vec::new(),
                footer: None,
                kind_label: None,
            }));
        }

        doc.insert_root(Node::Line(LineNode {
            raw: "no hint".to_string(),
            line_ending: "\n".to_string(),
            span: dummy_span(4),
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        }));

        let view = build_comparison_view(&doc, &default_opts());
        let counts = extracted_key_counts(&view);

        assert_eq!(counts.len(), 1);
        assert_eq!(counts["stanza:X"], 3);
    }

    #[test]
    fn empty_document_produces_empty_view() {
        let doc = parse_generic("");
        let view = build_comparison_view(&doc, &default_opts());
        assert!(view.lines.is_empty());
    }

    #[test]
    fn whitespace_normalization_affects_keys() {
        let doc_a = parse_generic("line  a\n");
        let doc_b = parse_generic("line  a\n");

        let plain = build_comparison_view(&doc_a, &default_opts());
        let collapsed = build_comparison_view(
            &doc_b,
            &NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]),
        );

        assert_eq!(plain.lines[0].normalized, "line  a");
        assert_eq!(collapsed.lines[0].normalized, "line a");
        assert_ne!(plain.lines[0].content_key, collapsed.lines[0].content_key);
    }

    #[test]
    fn key_hint_on_block_header_uses_stanza_prefix() {
        let mut doc = Document::default();
        doc.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "interface Ethernet1".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: Some("interface:Ethernet1".to_string()),
                trivia: TriviaKind::Content,
            },
            children: Vec::new(),
            footer: None,
            kind_label: None,
        }));

        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 1);
        assert_eq!(
            view.lines[0].key_hint.as_deref(),
            Some("interface:Ethernet1")
        );
    }

    #[test]
    fn leaf_line_hint_stabilises_content_key_across_value_changes() {
        // two lines with the same key hint but different text should share a
        // content key (under the same parent), so the diff engine treats a
        // value change as a modification rather than delete + add.
        let mut doc_a = Document::default();
        let child_a = doc_a.insert_node(Node::Line(LineNode {
            raw: "  set ip 10.0.0.1 255.255.255.0".to_string(),
            line_ending: "\n".to_string(),
            span: dummy_span(2),
            parsed: None,
            key_hint: Some("set:ip".to_string()),
            trivia: TriviaKind::Content,
        }));
        doc_a.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "edit port1".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: Some("edit:port1".to_string()),
                trivia: TriviaKind::Content,
            },
            children: vec![child_a],
            footer: None,
            kind_label: None,
        }));

        let mut doc_b = Document::default();
        let child_b = doc_b.insert_node(Node::Line(LineNode {
            raw: "  set ip 10.0.0.2 255.255.255.0".to_string(),
            line_ending: "\n".to_string(),
            span: dummy_span(2),
            parsed: None,
            key_hint: Some("set:ip".to_string()),
            trivia: TriviaKind::Content,
        }));
        doc_b.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "edit port1".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: Some("edit:port1".to_string()),
                trivia: TriviaKind::Content,
            },
            children: vec![child_b],
            footer: None,
            kind_label: None,
        }));

        let view_a = build_comparison_view(&doc_a, &default_opts());
        let view_b = build_comparison_view(&doc_b, &default_opts());

        assert_eq!(view_a.lines[1].content_key, view_b.lines[1].content_key);

        // hint not exposed on ComparisonLine (see `key_material_for_line`).
        assert_eq!(view_a.lines[1].key_hint, None);
    }

    #[test]
    fn leaf_line_without_hint_keys_on_normalized_text() {
        // lines without key hints should still use full normalized text,
        // so different text produces different content keys.
        let mut doc = Document::default();
        let child_a = doc.insert_node(Node::Line(LineNode {
            raw: "  line a".to_string(),
            line_ending: "\n".to_string(),
            span: dummy_span(2),
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        }));
        let child_b = doc.insert_node(Node::Line(LineNode {
            raw: "  line b".to_string(),
            line_ending: "\n".to_string(),
            span: dummy_span(3),
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        }));
        doc.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "parent".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            },
            children: vec![child_a, child_b],
            footer: None,
            kind_label: None,
        }));

        let view = build_comparison_view(&doc, &default_opts());

        assert_ne!(view.lines[1].content_key, view.lines[2].content_key);
    }

    #[test]
    fn block_with_no_children_and_no_footer() {
        let mut doc = Document::default();
        doc.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "empty-block".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            },
            children: Vec::new(),
            footer: None,
            kind_label: None,
        }));

        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 1);
        assert_eq!(view.lines[0].normalized, "empty-block");
        assert_eq!(view.lines[0].path, Path(vec![0]));
    }

    #[test]
    fn block_with_no_children_but_has_footer() {
        let mut doc = Document::default();
        doc.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "begin".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            },
            children: Vec::new(),
            footer: Some(LineNode {
                raw: "end".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(2),
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            }),
            kind_label: None,
        }));

        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 2);
        assert_eq!(view.lines[0].normalized, "begin");
        assert_eq!(view.lines[0].path, Path(vec![0]));
        assert_eq!(view.lines[1].normalized, "end");
        // footer path index equals children.len() (0 children → index 0).
        assert_eq!(view.lines[1].path, Path(vec![0, 0]));
    }

    #[test]
    fn block_with_only_comment_children() {
        let doc = parse_generic("parent\n  ! comment one\n  # comment two\n");
        let view = build_comparison_view(&doc, &default_opts());

        // header + two comment children.
        assert_eq!(view.lines.len(), 3);
        assert_eq!(view.lines[0].trivia, TriviaKind::Content);
        assert_eq!(view.lines[1].trivia, TriviaKind::Comment);
        assert_eq!(view.lines[2].trivia, TriviaKind::Comment);
    }

    #[test]
    fn block_with_only_comment_children_ignored_leaves_header() {
        let doc = parse_generic("parent\n  ! comment one\n  # comment two\n");
        let opts = NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]);
        let view = build_comparison_view(&doc, &opts);

        // comments are filtered out; only the header remains.
        assert_eq!(view.lines.len(), 1);
        assert_eq!(view.lines[0].normalized, "parent");
    }

    #[test]
    fn trivia_only_documents() {
        use NormalizationStep::{IgnoreBlankLines, IgnoreComments};
        use TriviaKind::{Blank, Comment};
        for (input, steps, expected) in [
            // blank lines are trivia, but included by default.
            ("\n\n\n", vec![], vec![Blank, Blank, Blank]),
            ("\n\n\n", vec![IgnoreBlankLines], vec![]),
            ("! first\n# second\n", vec![], vec![Comment, Comment]),
            ("! first\n# second\n", vec![IgnoreComments], vec![]),
        ] {
            let doc = parse_generic(input);
            let view = build_comparison_view(&doc, &NormalizeOptions::new(steps.clone()));
            let trivia: Vec<TriviaKind> = view.lines.iter().map(|l| l.trivia).collect();
            assert_eq!(trivia, expected, "{input:?} with {steps:?}");
        }
    }

    #[test]
    fn deeply_nested_blocks_flatten_with_correct_paths() {
        // build a 10-level deep nesting chain: each block has one child block,
        // with a single leaf at the bottom.
        let mut doc = Document::default();

        let leaf = doc.insert_node(Node::Line(LineNode {
            raw: "          leaf".to_string(),
            line_ending: "\n".to_string(),
            span: dummy_span(11),
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        }));

        let mut current_child = leaf;
        for depth in (1..=10).rev() {
            let indent = "  ".repeat(depth - 1);
            current_child = doc.insert_node(Node::Block(BlockNode {
                header: LineNode {
                    raw: format!("{indent}level-{depth}"),
                    line_ending: "\n".to_string(),
                    span: dummy_span(depth),
                    parsed: None,
                    key_hint: None,
                    trivia: TriviaKind::Content,
                },
                children: vec![current_child],
                footer: None,
                kind_label: None,
            }));
        }

        doc.roots.push(current_child);
        let view = build_comparison_view(&doc, &default_opts());

        // 10 block headers + 1 leaf = 11 lines.
        assert_eq!(view.lines.len(), 11);

        // each line should have a path one element longer than the previous.
        for (i, line) in view.lines.iter().enumerate() {
            assert_eq!(
                line.path.0.len(),
                i + 1,
                "line {i} should have path depth {}",
                i + 1
            );
            // all children are index 0 under their parent.
            assert!(
                line.path.0.iter().all(|&idx| idx == 0),
                "all path indices should be 0 for a single-child chain"
            );
        }

        assert_eq!(view.lines[0].normalized, "level-1");
        assert_eq!(view.lines[10].normalized, "          leaf");
    }

    #[test]
    fn single_line_document_preserves_span() {
        let mut doc = Document::default();
        doc.insert_root(Node::Line(LineNode {
            raw: "hostname edge-01".to_string(),
            line_ending: "\n".to_string(),
            span: Span {
                line: 1,
                start_byte: 0,
                end_byte: 16,
            },
            parsed: None,
            key_hint: None,
            trivia: TriviaKind::Content,
        }));

        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 1);
        assert_eq!(view.lines[0].span.line, 1);
        assert_eq!(view.lines[0].span.start_byte, 0);
        assert_eq!(view.lines[0].span.end_byte, 16);
    }

    #[test]
    fn invalid_node_id_is_silently_skipped() {
        // build a block whose child list references a node ID that doesn't
        // exist in the arena.  flatten_node should skip it gracefully.
        let mut doc = Document::default();
        let bogus_id = netform_ir::NodeId(999);
        doc.insert_root(Node::Block(BlockNode {
            header: LineNode {
                raw: "parent".to_string(),
                line_ending: "\n".to_string(),
                span: dummy_span(1),
                parsed: None,
                key_hint: None,
                trivia: TriviaKind::Content,
            },
            children: vec![bogus_id],
            footer: None,
            kind_label: None,
        }));

        let view = build_comparison_view(&doc, &default_opts());

        // only the header should appear; the invalid child is skipped.
        assert_eq!(view.lines.len(), 1);
        assert_eq!(view.lines[0].normalized, "parent");
    }
}
