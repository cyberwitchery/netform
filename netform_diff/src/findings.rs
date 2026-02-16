use std::collections::HashMap;

use netform_ir::{Document, Node, NodeId, Path};

use crate::flatten::{content_counts, extracted_key_counts};
use crate::model::{ComparisonView, Finding, FindingLevel};

#[derive(Debug)]
pub(crate) struct DiffContext {
    ambiguous_content_keys: HashMap<u64, (usize, usize)>,
    ambiguous_extracted_keys: HashMap<String, (usize, usize)>,
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

        Self {
            ambiguous_content_keys,
            ambiguous_extracted_keys,
        }
    }
}

pub(crate) fn collect_findings(
    a_doc: &Document,
    b_doc: &Document,
    a_view: &ComparisonView,
    b_view: &ComparisonView,
    ctx: &DiffContext,
    fallback_contexts: &[Path],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    collect_parse_findings(a_doc, a_view, "left", &mut findings);
    collect_parse_findings(b_doc, b_view, "right", &mut findings);
    collect_unknown_block_findings(a_doc, "left", &mut findings);
    collect_unknown_block_findings(b_doc, "right", &mut findings);
    collect_ambiguity_findings(a_view, b_view, ctx, &mut findings);
    collect_extracted_key_ambiguity_findings(a_view, b_view, ctx, &mut findings);
    collect_fallback_alignment_findings(fallback_contexts, &mut findings);
    findings.sort_by(|a, b| {
        let ap = a.path.as_ref().map(|p| p.0.clone()).unwrap_or_default();
        let bp = b.path.as_ref().map(|p| p.0.clone()).unwrap_or_default();
        (a.message.clone(), ap).cmp(&(b.message.clone(), bp))
    });
    findings
}

fn collect_extracted_key_ambiguity_findings(
    a_view: &ComparisonView,
    b_view: &ComparisonView,
    ctx: &DiffContext,
    out: &mut Vec<Finding>,
) {
    let mut keys = ctx
        .ambiguous_extracted_keys
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    for key in keys {
        let (left_count, right_count) = ctx
            .ambiguous_extracted_keys
            .get(&key)
            .expect("key from map iteration");
        let anchor = a_view
            .lines
            .iter()
            .find(|line| line.key_hint.as_deref() == Some(key.as_str()))
            .or_else(|| {
                b_view
                    .lines
                    .iter()
                    .find(|line| line.key_hint.as_deref() == Some(key.as_str()))
            });

        out.push(Finding {
            code: "ambiguous_key_match".to_string(),
            level: FindingLevel::Warning,
            message: format!(
                "ambiguous extracted key `{}` appears {}x on left and {}x on right",
                key, left_count, right_count
            ),
            path: anchor.map(|line| line.path.clone()),
            span: anchor.map(|line| line.span.clone()),
        });
    }
}

fn collect_parse_findings(
    doc: &Document,
    view: &ComparisonView,
    side: &str,
    out: &mut Vec<Finding>,
) {
    for pf in &doc.metadata.parse_findings {
        let matched_path = view
            .lines
            .iter()
            .find(|line| line.span.line == pf.span.line)
            .map(|line| line.path.clone());
        out.push(Finding {
            code: "unknown_unparsed_construct".to_string(),
            level: FindingLevel::Warning,
            message: format!("{side} parse uncertainty [{}]: {}", pf.code, pf.message),
            path: matched_path,
            span: Some(pf.span.clone()),
        });
    }
}

fn collect_unknown_block_findings(doc: &Document, side: &str, out: &mut Vec<Finding>) {
    for (idx, root) in doc.roots.iter().copied().enumerate() {
        walk_findings(doc, root, vec![idx], side, out);
    }
}

fn walk_findings(
    doc: &Document,
    node_id: NodeId,
    path: Vec<usize>,
    side: &str,
    out: &mut Vec<Finding>,
) {
    let Some(node) = doc.node(node_id) else {
        return;
    };

    if let Node::Block(block) = node {
        if block.kind_label.as_deref() == Some("unknown") {
            out.push(Finding {
                code: "unknown_unparsed_construct".to_string(),
                level: FindingLevel::Warning,
                message: format!("{side} document has an unknown block"),
                path: Some(Path(path.clone())),
                span: Some(block.header.span.clone()),
            });
        }

        for (child_idx, child_id) in block.children.iter().copied().enumerate() {
            let mut child_path = path.clone();
            child_path.push(child_idx);
            walk_findings(doc, child_id, child_path, side, out);
        }
    }
}

fn collect_ambiguity_findings(
    a_view: &ComparisonView,
    b_view: &ComparisonView,
    ctx: &DiffContext,
    out: &mut Vec<Finding>,
) {
    let mut keys = ctx
        .ambiguous_content_keys
        .keys()
        .copied()
        .collect::<Vec<_>>();
    keys.sort_unstable();
    for key in keys {
        let (left_count, right_count) = ctx
            .ambiguous_content_keys
            .get(&key)
            .expect("key from map iteration");
        let anchor = a_view
            .lines
            .iter()
            .find(|line| line.content_key == key)
            .or_else(|| b_view.lines.iter().find(|line| line.content_key == key));

        out.push(Finding {
            code: "ambiguous_key_match".to_string(),
            level: FindingLevel::Warning,
            message: format!(
                "ambiguous content key {} appears {}x on left and {}x on right",
                crate::util::key_label(Some(key)),
                left_count,
                right_count
            ),
            path: anchor.map(|line| line.path.clone()),
            span: anchor.map(|line| line.span.clone()),
        });
    }
}

fn collect_fallback_alignment_findings(contexts: &[Path], out: &mut Vec<Finding>) {
    for context in contexts {
        out.push(Finding {
            code: "diff_unreliable_region".to_string(),
            level: FindingLevel::Warning,
            message: "diff used fallback segment alignment for this context".to_string(),
            path: Some(context.clone()),
            span: None,
        });
    }
}
