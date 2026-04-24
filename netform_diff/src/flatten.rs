use std::collections::HashMap;

use netform_ir::{Document, Node, NodeId, Path, TriviaKind};

use crate::model::{
    ComparisonLine, ComparisonView, KeyKind, NormalizeOptions, derive_content_key,
    derive_occurrence_key,
};
use crate::normalize::normalize_for_compare;

#[derive(Debug, Default)]
struct KeyAllocator {
    counters: HashMap<(u64, KeyKind, u64), u64>,
}

impl KeyAllocator {
    fn next_keys(
        &mut self,
        parent_signature: u64,
        kind: KeyKind,
        trivia: TriviaKind,
        normalized_for_key: &str,
    ) -> (u64, u64) {
        let content_key = derive_content_key(parent_signature, kind, trivia, normalized_for_key);

        let bucket = (parent_signature, kind, content_key);
        let ordinal = self.counters.entry(bucket).or_insert(0);
        *ordinal += 1;

        let occurrence_key = derive_occurrence_key(content_key, *ordinal);

        (content_key, occurrence_key)
    }
}

/// Build a flattened comparison view from a parsed document.
pub fn build_comparison_view(doc: &Document, options: &NormalizeOptions) -> ComparisonView {
    let mut out = Vec::new();
    let mut keys = KeyAllocator::default();
    let mut path = Vec::new();

    for (idx, root) in doc.roots.iter().copied().enumerate() {
        path.clear();
        path.push(idx);
        flatten_node(doc, root, 0, &mut path, &mut out, &mut keys, options);
    }

    ComparisonView { lines: out }
}

fn flatten_node(
    doc: &Document,
    node_id: NodeId,
    parent_signature: u64,
    path: &mut Vec<usize>,
    out: &mut Vec<ComparisonLine>,
    keys: &mut KeyAllocator,
    options: &NormalizeOptions,
) {
    let Some(node) = doc.node(node_id) else {
        return;
    };

    match node {
        Node::Line(line) => {
            if let Some(normalized) = normalize_for_compare(&line.raw, line.trivia, options) {
                let key_material = key_material_for_line(
                    KeyKind::Line,
                    line.trivia,
                    line.key_hint.as_deref(),
                    normalized.as_str(),
                );
                let (content_key, occurrence_key) = keys.next_keys(
                    parent_signature,
                    KeyKind::Line,
                    line.trivia,
                    key_material.for_hash.as_str(),
                );

                out.push(ComparisonLine {
                    content_key,
                    occurrence_key,
                    key_hint: key_material.hint,
                    normalized,
                    original: line.raw.clone(),
                    path: Path(path.clone()),
                    span: line.span.clone(),
                    trivia: line.trivia,
                });
            }
        }
        Node::Block(block) => {
            if let Some(normalized) =
                normalize_for_compare(&block.header.raw, block.header.trivia, options)
            {
                let key_material = key_material_for_line(
                    KeyKind::BlockHeader,
                    block.header.trivia,
                    block.header.key_hint.as_deref(),
                    normalized.as_str(),
                );
                let (header_content_key, header_occurrence_key) = keys.next_keys(
                    parent_signature,
                    KeyKind::BlockHeader,
                    block.header.trivia,
                    key_material.for_hash.as_str(),
                );

                out.push(ComparisonLine {
                    content_key: header_content_key,
                    occurrence_key: header_occurrence_key,
                    key_hint: key_material.hint,
                    normalized,
                    original: block.header.raw.clone(),
                    path: Path(path.clone()),
                    span: block.header.span.clone(),
                    trivia: block.header.trivia,
                });

                for (child_idx, child_id) in block.children.iter().copied().enumerate() {
                    path.push(child_idx);
                    flatten_node(doc, child_id, header_content_key, path, out, keys, options);
                    path.pop();
                }

                if let Some(footer) = &block.footer {
                    path.push(block.children.len());

                    if let Some(footer_normalized) =
                        normalize_for_compare(&footer.raw, footer.trivia, options)
                    {
                        let key_material = key_material_for_line(
                            KeyKind::BlockFooter,
                            footer.trivia,
                            footer.key_hint.as_deref(),
                            footer_normalized.as_str(),
                        );
                        let (footer_content_key, footer_occurrence_key) = keys.next_keys(
                            header_content_key,
                            KeyKind::BlockFooter,
                            footer.trivia,
                            key_material.for_hash.as_str(),
                        );

                        out.push(ComparisonLine {
                            content_key: footer_content_key,
                            occurrence_key: footer_occurrence_key,
                            key_hint: key_material.hint,
                            normalized: footer_normalized,
                            original: footer.raw.clone(),
                            path: Path(path.clone()),
                            span: footer.span.clone(),
                            trivia: footer.trivia,
                        });
                    }

                    path.pop();
                }
            }
        }
    }
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
) -> KeyMaterial {
    if trivia == TriviaKind::Content
        && let Some(hint) = key_hint
    {
        match kind {
            KeyKind::BlockHeader => {
                // Keep a stable and explicit namespace prefix for extracted keys.
                let for_hash = format!("stanza:{hint}");
                return KeyMaterial {
                    for_hash,
                    hint: Some(hint.to_string()),
                };
            }
            KeyKind::Line => {
                // Leaf-line hints (e.g. FortiOS `set:<field>`) stabilise content
                // keys across value changes.  We intentionally do NOT expose the
                // hint on ComparisonLine — leaf hints repeat across many sibling
                // blocks and would flood extracted-key ambiguity findings.
                let for_hash = format!("subkey:{hint}");
                return KeyMaterial {
                    for_hash,
                    hint: None,
                };
            }
            KeyKind::BlockFooter => {}
        }
    }

    KeyMaterial {
        for_hash: normalized.to_string(),
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
    fn multiple_roots_have_sequential_paths() {
        let doc = parse_generic("line a\nline b\nline c\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 3);
        assert_eq!(view.lines[0].path, Path(vec![0]));
        assert_eq!(view.lines[1].path, Path(vec![1]));
        assert_eq!(view.lines[2].path, Path(vec![2]));
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
    fn nested_blocks_flatten_recursively() {
        let doc = parse_generic("a\n  b\n    c\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 3);
        assert_eq!(view.lines[0].path, Path(vec![0]));
        assert_eq!(view.lines[1].path, Path(vec![0, 0]));
        assert_eq!(view.lines[2].path, Path(vec![0, 0, 0]));
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
        let doc = parse_generic("permit any\npermit any\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 2);
        assert_eq!(view.lines[0].content_key, view.lines[1].content_key);
        assert_ne!(view.lines[0].occurrence_key, view.lines[1].occurrence_key);
    }

    #[test]
    fn different_lines_have_different_content_keys() {
        let doc = parse_generic("line a\nline b\n");
        let view = build_comparison_view(&doc, &default_opts());

        assert_eq!(view.lines.len(), 2);
        assert_ne!(view.lines[0].content_key, view.lines[1].content_key);
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
        // Two lines with the same key hint but different text should share a
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

        // The set lines should share a content key despite different text.
        assert_eq!(view_a.lines[1].content_key, view_b.lines[1].content_key);

        // The hint should NOT be exposed on the ComparisonLine (avoids
        // extracted-key ambiguity noise).
        assert_eq!(view_a.lines[1].key_hint, None);
    }

    #[test]
    fn leaf_line_without_hint_keys_on_normalized_text() {
        // Lines without key hints should still use full normalized text,
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
}
