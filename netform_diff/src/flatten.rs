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

    for (idx, root) in doc.roots.iter().copied().enumerate() {
        flatten_node(doc, root, 0, vec![idx], &mut out, &mut keys, options);
    }

    ComparisonView { lines: out }
}

fn flatten_node(
    doc: &Document,
    node_id: NodeId,
    parent_signature: u64,
    path: Vec<usize>,
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
                    path: Path(path),
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
                    let mut child_path = path.clone();
                    child_path.push(child_idx);
                    flatten_node(
                        doc,
                        child_id,
                        header_content_key,
                        child_path,
                        out,
                        keys,
                        options,
                    );
                }

                if let Some(footer) = &block.footer {
                    let mut footer_path = path;
                    footer_path.push(block.children.len());

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
                            path: Path(footer_path),
                            span: footer.span.clone(),
                            trivia: footer.trivia,
                        });
                    }
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
    if kind == KeyKind::BlockHeader
        && trivia == TriviaKind::Content
        && let Some(hint) = key_hint
    {
        // Keep a stable and explicit namespace prefix for extracted keys.
        let for_hash = format!("stanza:{hint}");
        return KeyMaterial {
            for_hash,
            hint: Some(hint.to_string()),
        };
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
