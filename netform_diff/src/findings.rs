use std::collections::HashMap;

use netform_ir::{Document, Node, NodeId, Path};

use crate::flatten::{content_counts, extracted_key_counts};
use crate::model::{ComparisonView, Finding, FindingLevel, finding_code};

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
        let ap = a.path.as_ref().map(|p| p.0.as_slice()).unwrap_or(&[]);
        let bp = b.path.as_ref().map(|p| p.0.as_slice()).unwrap_or(&[]);
        (a.message.as_str(), ap).cmp(&(b.message.as_str(), bp))
    });
    findings
}

fn push_ambiguity_finding(
    a_view: &ComparisonView,
    b_view: &ComparisonView,
    anchor_predicate: impl Fn(&crate::model::ComparisonLine) -> bool,
    message: String,
    out: &mut Vec<Finding>,
) {
    let anchor = a_view
        .lines
        .iter()
        .find(|line| anchor_predicate(line))
        .or_else(|| b_view.lines.iter().find(|line| anchor_predicate(line)));

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
    b_view: &ComparisonView,
    ctx: &DiffContext,
    out: &mut Vec<Finding>,
) {
    let mut entries: Vec<_> = ctx.ambiguous_extracted_keys.iter().collect();
    entries.sort_by_key(|&(k, _)| k);
    for (key, &(left_count, right_count)) in entries {
        push_ambiguity_finding(
            a_view,
            b_view,
            |line| line.key_hint.as_deref() == Some(key.as_str()),
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
    out: &mut Vec<Finding>,
) {
    for pf in &doc.metadata.parse_findings {
        let matched_path = view
            .lines
            .iter()
            .find(|line| line.span.line == pf.span.line)
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
    for (idx, root) in doc.roots.iter().copied().enumerate() {
        path.clear();
        path.push(idx);
        walk_findings(doc, root, &mut path, side, out);
    }
}

fn walk_findings(
    doc: &Document,
    node_id: NodeId,
    path: &mut Vec<usize>,
    side: &str,
    out: &mut Vec<Finding>,
) {
    let Some(node) = doc.node(node_id) else {
        return;
    };

    if let Node::Block(block) = node {
        if block.kind_label.as_deref() == Some("unknown") {
            out.push(Finding {
                code: finding_code::UNKNOWN_UNPARSED_CONSTRUCT.to_string(),
                level: FindingLevel::Warning,
                message: format!("{side} document has an unknown block"),
                path: Some(Path(path.clone())),
                span: Some(block.header.span.clone()),
            });
        }

        for (child_idx, child_id) in block.children.iter().copied().enumerate() {
            path.push(child_idx);
            walk_findings(doc, child_id, path, side, out);
            path.pop();
        }
    }
}

fn collect_ambiguity_findings(
    a_view: &ComparisonView,
    b_view: &ComparisonView,
    ctx: &DiffContext,
    out: &mut Vec<Finding>,
) {
    let mut entries: Vec<_> = ctx.ambiguous_content_keys.iter().collect();
    entries.sort_unstable_by_key(|&(&k, _)| k);
    for (&key, &(left_count, right_count)) in entries {
        push_ambiguity_finding(
            a_view,
            b_view,
            |line| line.content_key == key,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ComparisonLine;
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
    fn collect_findings_empty_inputs() {
        let a_doc = Document::default();
        let b_doc = Document::default();
        let a_view = make_view(vec![]);
        let b_view = make_view(vec![]);
        let ctx = DiffContext::from_views(&a_view, &b_view);

        let findings = collect_findings(&a_doc, &b_doc, &a_view, &b_view, &ctx, &[]);

        assert!(findings.is_empty());
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

        collect_ambiguity_findings(&a, &b, &ctx, &mut out);

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

        collect_extracted_key_ambiguity_findings(&a, &b, &ctx, &mut out);

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
        let mut out = Vec::new();

        collect_parse_findings(&doc, &v, "left", &mut out);

        assert_eq!(out.len(), 1);
        assert!(out[0].message.starts_with("left"));
        assert!(out[0].message.contains("orphan-indentation"));
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
        assert!(out[0].message.contains("right"));
        assert!(out[0].message.contains("unknown block"));
    }

    #[test]
    fn collect_findings_sorts_by_message_then_path() {
        let a_doc = Document::default();
        let b_doc = Document::default();
        let a_view = make_view(vec![]);
        let b_view = make_view(vec![]);
        let ctx = DiffContext::from_views(&a_view, &b_view);
        let contexts = vec![Path(vec![1]), Path(vec![0])];

        let findings = collect_findings(&a_doc, &b_doc, &a_view, &b_view, &ctx, &contexts);

        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].path.as_ref().unwrap().0, vec![0]);
        assert_eq!(findings[1].path.as_ref().unwrap().0, vec![1]);
    }
}
