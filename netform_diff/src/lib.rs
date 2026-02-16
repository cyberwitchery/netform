//! Diff engine and reporting primitives for `netform_ir::Document`.
//!
//! This crate builds comparison views from lossless IR documents, applies
//! explicit normalization and ordering policies, and emits deterministic edits.
//!
//! Primary entrypoints:
//! - [`diff_documents`]
//! - [`format_markdown_report`]
//! - [`build_plan`]
//!
//! # Example
//!
//! ```rust
//! use netform_diff::{diff_documents, NormalizeOptions};
//! use netform_ir::parse_generic;
//!
//! let left = parse_generic("hostname old\n");
//! let right = parse_generic("hostname new\n");
//! let diff = diff_documents(&left, &right, NormalizeOptions::default());
//! assert!(diff.has_changes);
//! ```

use std::collections::HashMap;

use netform_ir::{Document, Node, NodeId, Path, Span, TriviaKind};
use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_64;

/// One ordered normalization step in the comparison pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizationStep {
    IgnoreComments,
    IgnoreBlankLines,
    TrimTrailingWhitespace,
    NormalizeLeadingWhitespace,
    CollapseInternalWhitespace,
}

/// Options controlling normalization and ordering semantics for diffing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizeOptions {
    pub steps: Vec<NormalizationStep>,
    pub order_policy: OrderPolicyConfig,
}

impl NormalizeOptions {
    /// Create options with an explicit ordered normalization step list.
    pub fn new(steps: Vec<NormalizationStep>) -> Self {
        Self {
            steps,
            order_policy: OrderPolicyConfig::default(),
        }
    }

    /// Override ordering policy configuration.
    pub fn with_order_policy(mut self, order_policy: OrderPolicyConfig) -> Self {
        self.order_policy = order_policy;
        self
    }

    fn policy_for_path(&self, path: &Path) -> OrderPolicy {
        self.order_policy.policy_for_path(path)
    }
}

/// Ordering behavior used when comparing sibling lines in a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderPolicy {
    Ordered,
    Unordered,
    KeyedStable,
}

/// Path-based policy override for specific subtree contexts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderPolicyOverride {
    pub context_prefix: Vec<usize>,
    pub policy: OrderPolicy,
}

/// Ordering policy configuration with a default and longest-prefix overrides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderPolicyConfig {
    pub default: OrderPolicy,
    pub overrides: Vec<OrderPolicyOverride>,
}

impl Default for OrderPolicyConfig {
    fn default() -> Self {
        Self {
            default: OrderPolicy::Ordered,
            overrides: Vec::new(),
        }
    }
}

impl OrderPolicyConfig {
    fn policy_for_path(&self, path: &Path) -> OrderPolicy {
        let mut best: Option<(&OrderPolicyOverride, usize)> = None;
        for rule in &self.overrides {
            if path_starts_with(&path.0, &rule.context_prefix) {
                let len = rule.context_prefix.len();
                if best.is_none_or(|(_, best_len)| len > best_len) {
                    best = Some((rule, len));
                }
            }
        }
        best.map_or(self.default, |(rule, _)| rule.policy)
    }
}

/// One normalized line in the internal comparison view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonLine {
    pub content_key: u64,
    pub occurrence_key: u64,
    pub normalized: String,
    pub original: String,
    pub path: Path,
    pub span: Span,
    pub trivia: TriviaKind,
}

/// Flattened line-oriented view derived from a document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComparisonView {
    pub lines: Vec<ComparisonLine>,
}

/// Serializable line payload embedded in diff edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub content_key: u64,
    pub occurrence_key: u64,
    pub text: String,
    pub path: Path,
    pub span: Span,
}

/// Path/span anchor for edit placement and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditAnchor {
    pub path: Path,
    pub span: Span,
}

/// Edit script operation emitted by the diff engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
pub enum Edit {
    Insert {
        at_key: Option<u64>,
        left_anchor: Option<EditAnchor>,
        right_anchor: Option<EditAnchor>,
        lines: Vec<DiffLine>,
    },
    Delete {
        at_key: Option<u64>,
        left_anchor: Option<EditAnchor>,
        right_anchor: Option<EditAnchor>,
        lines: Vec<DiffLine>,
    },
    Replace {
        old_at_key: Option<u64>,
        new_at_key: Option<u64>,
        left_anchor: Option<EditAnchor>,
        right_anchor: Option<EditAnchor>,
        old_lines: Vec<DiffLine>,
        new_lines: Vec<DiffLine>,
    },
}

/// Aggregate counters for diff output.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct DiffStats {
    pub inserts: usize,
    pub deletes: usize,
    pub replaces: usize,
    pub inserted_lines: usize,
    pub deleted_lines: usize,
    pub replaced_old_lines: usize,
    pub replaced_new_lines: usize,
}

/// Warning/info emitted during parse propagation or diff uncertainty handling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: String,
    pub level: FindingLevel,
    pub message: String,
    pub path: Option<Path>,
    pub span: Option<Span>,
}

/// Severity level for a [`Finding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingLevel {
    Warning,
    Info,
}

/// Top-level diff output contract.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Diff {
    pub normalization_steps: Vec<NormalizationStep>,
    pub order_policy: OrderPolicyConfig,
    pub has_changes: bool,
    pub edits: Vec<Edit>,
    pub stats: DiffStats,
    pub findings: Vec<Finding>,
}

/// Transport-neutral action plan derived from a [`Diff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub version: String,
    pub actions: Vec<PlanAction>,
    pub findings: Vec<PlanFinding>,
}

/// Action variants emitted in a [`Plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanAction {
    ReplaceBlock {
        target_path: Path,
        target_span: Span,
        intended_lines: Vec<String>,
    },
    ApplyLineEditsUnderContext {
        context_path: Path,
        line_edits: Vec<PlanLineEdit>,
    },
}

/// One line-oriented edit in `apply_line_edits_under_context`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanLineEdit {
    pub kind: PlanLineEditKind,
    pub text: String,
}

/// Line operation kind for [`PlanLineEdit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanLineEditKind {
    Insert,
    Delete,
    Replace,
}

/// Plan-level warning (for example missing anchors).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanFinding {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Delete,
    Insert,
}

/// Key namespace discriminator used when hashing comparison identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    Line,
    BlockHeader,
    BlockFooter,
}

#[derive(Debug, Clone)]
struct Segment {
    lines: Vec<ComparisonLine>,
    segment_key: u64,
    is_block: bool,
}

#[derive(Debug, Default)]
struct DiffComputation {
    edits: Vec<Edit>,
    fallback_contexts: Vec<Path>,
}

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

/// Derive a content key from parent signature, key kind, trivia, and normalized text.
pub fn derive_content_key(
    parent_signature: u64,
    kind: KeyKind,
    trivia: TriviaKind,
    normalized_for_key: &str,
) -> u64 {
    let canonical_content = format!(
        "p={parent_signature}|k={:?}|t={}|n={}",
        kind,
        trivia_tag(trivia),
        normalized_for_key
    );
    xxh3_64(canonical_content.as_bytes())
}

/// Derive an occurrence key from content key and 1-based ordinal.
pub fn derive_occurrence_key(content_key: u64, ordinal: u64) -> u64 {
    let canonical_occurrence = format!("c={content_key}|o={ordinal}");
    xxh3_64(canonical_occurrence.as_bytes())
}

#[derive(Debug)]
struct DiffContext {
    ambiguous_content_keys: HashMap<u64, (usize, usize)>,
}

impl DiffContext {
    fn from_views(a: &ComparisonView, b: &ComparisonView) -> Self {
        let a_counts = content_counts(a);
        let b_counts = content_counts(b);

        let mut ambiguous_content_keys = HashMap::new();
        for (key, a_count) in &a_counts {
            if *a_count > 1 && let Some(b_count) = b_counts.get(key) && *b_count > 1 {
                ambiguous_content_keys.insert(*key, (*a_count, *b_count));
            }
        }

        Self {
            ambiguous_content_keys,
        }
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

/// Compute a deterministic diff between two parsed documents.
pub fn diff_documents(a: &Document, b: &Document, options: NormalizeOptions) -> Diff {
    let a_view = build_comparison_view(a, &options);
    let b_view = build_comparison_view(b, &options);
    let ctx = DiffContext::from_views(&a_view, &b_view);
    let computation = diff_views(&a_view, &b_view, &options);
    let stats = build_stats(&computation.edits);
    let findings = collect_findings(a, b, &a_view, &b_view, &ctx, &computation.fallback_contexts);
    let has_changes = !computation.edits.is_empty();

    Diff {
        normalization_steps: options.steps,
        order_policy: options.order_policy,
        has_changes,
        edits: computation.edits,
        stats,
        findings,
    }
}

/// Format a markdown-oriented human report from a diff result.
pub fn format_markdown_report(diff: &Diff, left_label: &str, right_label: &str) -> String {
    let mut out = String::new();
    out.push_str("# Config Diff Report\n\n");
    out.push_str(&format!("- Left: `{left_label}`\n"));
    out.push_str(&format!("- Right: `{right_label}`\n\n"));

    out.push_str("## Stats\n\n");
    out.push_str(&format!(
        "- Inserts: {} ({} lines)\n",
        diff.stats.inserts, diff.stats.inserted_lines
    ));
    out.push_str(&format!(
        "- Deletes: {} ({} lines)\n",
        diff.stats.deletes, diff.stats.deleted_lines
    ));
    out.push_str(&format!(
        "- Replaces: {} ({} -> {} lines)\n\n",
        diff.stats.replaces, diff.stats.replaced_old_lines, diff.stats.replaced_new_lines
    ));

    out.push_str("## Edits\n\n");
    if diff.edits.is_empty() {
        out.push_str("No changes detected.\n");
    } else {
        for (idx, edit) in diff.edits.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", idx + 1, describe_edit(edit)));
        }
    }

    if !diff.findings.is_empty() {
        out.push_str("\n## Findings\n\n");
        for finding in &diff.findings {
            out.push_str(&format!(
                "- {:?} [{}]: {}\n",
                finding.level, finding.code, finding.message
            ));
        }
    }

    out
}

/// Convert a [`Diff`] into a transport-neutral action plan.
pub fn build_plan(diff: &Diff) -> Plan {
    let mut actions = Vec::new();
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
                        let context_path = parent_path(&anchor.path);
                        actions.push(PlanAction::ApplyLineEditsUnderContext {
                            context_path,
                            line_edits: new_lines
                                .iter()
                                .map(|line| PlanLineEdit {
                                    kind: PlanLineEditKind::Replace,
                                    text: line.text.clone(),
                                })
                                .collect(),
                        });
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
                    let context_path = parent_path(&anchor.path);
                    actions.push(PlanAction::ApplyLineEditsUnderContext {
                        context_path,
                        line_edits: lines
                            .iter()
                            .map(|line| PlanLineEdit {
                                kind: PlanLineEditKind::Insert,
                                text: line.text.clone(),
                            })
                            .collect(),
                    });
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
                    let context_path = parent_path(&anchor.path);
                    actions.push(PlanAction::ApplyLineEditsUnderContext {
                        context_path,
                        line_edits: lines
                            .iter()
                            .map(|line| PlanLineEdit {
                                kind: PlanLineEditKind::Delete,
                                text: line.text.clone(),
                            })
                            .collect(),
                    });
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
                let (content_key, occurrence_key) = keys.next_keys(
                    parent_signature,
                    KeyKind::Line,
                    line.trivia,
                    normalized.as_str(),
                );

                out.push(ComparisonLine {
                    content_key,
                    occurrence_key,
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
                let (header_content_key, header_occurrence_key) = keys.next_keys(
                    parent_signature,
                    KeyKind::BlockHeader,
                    block.header.trivia,
                    normalized.as_str(),
                );

                out.push(ComparisonLine {
                    content_key: header_content_key,
                    occurrence_key: header_occurrence_key,
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
                        let (footer_content_key, footer_occurrence_key) = keys.next_keys(
                            header_content_key,
                            KeyKind::BlockFooter,
                            footer.trivia,
                            footer_normalized.as_str(),
                        );

                        out.push(ComparisonLine {
                            content_key: footer_content_key,
                            occurrence_key: footer_occurrence_key,
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

fn normalize_for_compare(
    raw: &str,
    trivia: TriviaKind,
    options: &NormalizeOptions,
) -> Option<String> {
    let mut output = raw.to_string();

    for step in &options.steps {
        match step {
            NormalizationStep::IgnoreComments => {
                if trivia == TriviaKind::Comment {
                    return None;
                }
            }
            NormalizationStep::IgnoreBlankLines => {
                if output.trim().is_empty() {
                    return None;
                }
            }
            NormalizationStep::TrimTrailingWhitespace => {
                output = output.trim_end().to_string();
            }
            NormalizationStep::NormalizeLeadingWhitespace => {
                let indent = count_indent_columns(&output);
                let body = output.trim_start_matches([' ', '\t']).to_string();
                output = format!("{}{}", " ".repeat(indent), body);
            }
            NormalizationStep::CollapseInternalWhitespace => {
                output = output.split_whitespace().collect::<Vec<_>>().join(" ");
            }
        }
    }

    Some(output)
}

fn count_indent_columns(raw: &str) -> usize {
    let mut width = 0usize;
    for ch in raw.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += 4,
            _ => break,
        }
    }
    width
}

fn trivia_tag(kind: TriviaKind) -> &'static str {
    match kind {
        TriviaKind::Blank => "blank",
        TriviaKind::Comment => "comment",
        TriviaKind::Content => "content",
        TriviaKind::Unknown => "unknown",
    }
}

fn content_counts(view: &ComparisonView) -> HashMap<u64, usize> {
    let mut counts = HashMap::new();
    for line in &view.lines {
        *counts.entry(line.content_key).or_insert(0usize) += 1;
    }
    counts
}

fn build_segments(view: &ComparisonView) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut current_root: Option<usize> = None;
    let mut current = Vec::new();

    for line in &view.lines {
        let root = line.path.0.first().copied().unwrap_or(usize::MAX);
        if current_root != Some(root) {
            if !current.is_empty() {
                segments.push(lines_to_segment(std::mem::take(&mut current)));
            }
            current_root = Some(root);
        }

        current.push(line.clone());
    }

    if !current.is_empty() {
        segments.push(lines_to_segment(current));
    }

    segments
}

fn lines_to_segment(lines: Vec<ComparisonLine>) -> Segment {
    let is_block = lines.iter().any(|line| line.path.0.len() > 1);
    let segment_key = lines.first().map(|line| line.content_key).unwrap_or(0);
    Segment {
        lines,
        segment_key,
        is_block,
    }
}

fn diff_views(
    a: &ComparisonView,
    b: &ComparisonView,
    options: &NormalizeOptions,
) -> DiffComputation {
    let a_segments = build_segments(a);
    let b_segments = build_segments(b);

    let a_keys = a_segments
        .iter()
        .map(|segment| segment.segment_key)
        .collect::<Vec<_>>();
    let b_keys = b_segments
        .iter()
        .map(|segment| segment.segment_key)
        .collect::<Vec<_>>();

    let ops = compute_ops(&a_keys, &b_keys);

    let mut edits = Vec::new();
    let mut fallback_contexts = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    let mut pending_deleted_segments: Vec<Segment> = Vec::new();
    let mut pending_inserted_segments: Vec<Segment> = Vec::new();

    let mut flush_segment_fallback =
        |edits: &mut Vec<Edit>, deleted: &mut Vec<Segment>, inserted: &mut Vec<Segment>| {
            if deleted.is_empty() && inserted.is_empty() {
                return;
            }

            let deleted_lines = deleted
                .iter()
                .flat_map(|segment| segment.lines.clone())
                .collect::<Vec<_>>();
            let inserted_lines = inserted
                .iter()
                .flat_map(|segment| segment.lines.clone())
                .collect::<Vec<_>>();
            deleted.clear();
            inserted.clear();

            let mut fallback = line_diff(
                &deleted_lines,
                &inserted_lines,
                options.policy_for_path(
                    &deleted_lines
                        .first()
                        .map(|line| line.path.clone())
                        .or_else(|| inserted_lines.first().map(|line| line.path.clone()))
                        .unwrap_or(Path(Vec::new())),
                ),
            );
            if let Some(anchor) = deleted_lines
                .first()
                .map(|line| line.path.clone())
                .or_else(|| inserted_lines.first().map(|line| line.path.clone()))
            {
                fallback_contexts.push(anchor);
            }
            edits.append(&mut fallback);
        };

    for op in ops {
        match op {
            Op::Equal => {
                flush_segment_fallback(
                    &mut edits,
                    &mut pending_deleted_segments,
                    &mut pending_inserted_segments,
                );

                let left = &a_segments[i];
                let right = &b_segments[j];
                if left.is_block && right.is_block {
                    let left_children = if left.lines.len() > 1 {
                        &left.lines[1..]
                    } else {
                        &[]
                    };
                    let right_children = if right.lines.len() > 1 {
                        &right.lines[1..]
                    } else {
                        &[]
                    };

                    let mut child_edits = line_diff(
                        left_children,
                        right_children,
                        options.policy_for_path(&left.lines[0].path),
                    );
                    edits.append(&mut child_edits);
                }

                i += 1;
                j += 1;
            }
            Op::Delete => {
                pending_deleted_segments.push(a_segments[i].clone());
                i += 1;
            }
            Op::Insert => {
                pending_inserted_segments.push(b_segments[j].clone());
                j += 1;
            }
        }
    }

    flush_segment_fallback(
        &mut edits,
        &mut pending_deleted_segments,
        &mut pending_inserted_segments,
    );

    DiffComputation {
        edits,
        fallback_contexts,
    }
}

fn line_diff(a: &[ComparisonLine], b: &[ComparisonLine], policy: OrderPolicy) -> Vec<Edit> {
    match policy {
        OrderPolicy::Ordered => line_diff_ordered(a, b),
        OrderPolicy::Unordered => line_diff_unordered(a, b),
        OrderPolicy::KeyedStable => line_diff_keyed_stable(a, b),
    }
}

fn line_diff_ordered(a: &[ComparisonLine], b: &[ComparisonLine]) -> Vec<Edit> {
    let a_tokens = a.iter().map(|line| line.content_key).collect::<Vec<_>>();
    let b_tokens = b.iter().map(|line| line.content_key).collect::<Vec<_>>();
    let ops = compute_ops(&a_tokens, &b_tokens);

    let mut edits = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    let mut pending_deletes: Vec<DiffLine> = Vec::new();
    let mut pending_inserts: Vec<DiffLine> = Vec::new();

    let flush =
        |edits: &mut Vec<Edit>, deletes: &mut Vec<DiffLine>, inserts: &mut Vec<DiffLine>| {
            if deletes.is_empty() && inserts.is_empty() {
                return;
            }

            if !deletes.is_empty() && !inserts.is_empty() {
                edits.push(Edit::Replace {
                    old_at_key: deletes.first().map(|line| line.occurrence_key),
                    new_at_key: inserts.first().map(|line| line.occurrence_key),
                    left_anchor: deletes.first().map(to_anchor),
                    right_anchor: inserts.first().map(to_anchor),
                    old_lines: std::mem::take(deletes),
                    new_lines: std::mem::take(inserts),
                });
                return;
            }

            if !deletes.is_empty() {
                edits.push(Edit::Delete {
                    at_key: deletes.first().map(|line| line.occurrence_key),
                    left_anchor: deletes.first().map(to_anchor),
                    right_anchor: None,
                    lines: std::mem::take(deletes),
                });
                return;
            }

            edits.push(Edit::Insert {
                at_key: inserts.first().map(|line| line.occurrence_key),
                left_anchor: None,
                right_anchor: inserts.first().map(to_anchor),
                lines: std::mem::take(inserts),
            });
        };

    for op in ops {
        match op {
            Op::Equal => {
                flush(&mut edits, &mut pending_deletes, &mut pending_inserts);
                i += 1;
                j += 1;
            }
            Op::Delete => {
                pending_deletes.push(to_diff_line(&a[i]));
                i += 1;
            }
            Op::Insert => {
                pending_inserts.push(to_diff_line(&b[j]));
                j += 1;
            }
        }
    }

    flush(&mut edits, &mut pending_deletes, &mut pending_inserts);
    edits
}

fn line_diff_unordered(a: &[ComparisonLine], b: &[ComparisonLine]) -> Vec<Edit> {
    line_diff_multiset(a, b, |line| xxh3_64(line.normalized.as_bytes()))
}

fn line_diff_keyed_stable(a: &[ComparisonLine], b: &[ComparisonLine]) -> Vec<Edit> {
    line_diff_multiset(a, b, |line| line.content_key)
}

fn line_diff_multiset<F>(a: &[ComparisonLine], b: &[ComparisonLine], key_fn: F) -> Vec<Edit>
where
    F: Fn(&ComparisonLine) -> u64,
{
    let mut a_buckets: HashMap<u64, Vec<&ComparisonLine>> = HashMap::new();
    let mut b_buckets: HashMap<u64, Vec<&ComparisonLine>> = HashMap::new();

    for line in a {
        a_buckets.entry(key_fn(line)).or_default().push(line);
    }
    for line in b {
        b_buckets.entry(key_fn(line)).or_default().push(line);
    }

    let mut all_keys = a_buckets.keys().copied().collect::<Vec<_>>();
    for key in b_buckets.keys().copied() {
        if !all_keys.contains(&key) {
            all_keys.push(key);
        }
    }
    all_keys.sort_unstable();

    let mut deletes = Vec::new();
    let mut inserts = Vec::new();
    for key in all_keys {
        let mut left = a_buckets.remove(&key).unwrap_or_default();
        let mut right = b_buckets.remove(&key).unwrap_or_default();

        left.sort_by_key(|line| (line.occurrence_key, line.path.0.clone()));
        right.sort_by_key(|line| (line.occurrence_key, line.path.0.clone()));

        if left.len() > right.len() {
            for line in left.into_iter().skip(right.len()) {
                deletes.push(to_diff_line(line));
            }
        } else if right.len() > left.len() {
            for line in right.into_iter().skip(left.len()) {
                inserts.push(to_diff_line(line));
            }
        }
    }

    finalize_chunked_edits(deletes, inserts)
}

fn finalize_chunked_edits(mut deletes: Vec<DiffLine>, mut inserts: Vec<DiffLine>) -> Vec<Edit> {
    if deletes.is_empty() && inserts.is_empty() {
        return Vec::new();
    }

    deletes.sort_by_key(|line| (line.content_key, line.occurrence_key, line.path.0.clone()));
    inserts.sort_by_key(|line| (line.content_key, line.occurrence_key, line.path.0.clone()));

    if !deletes.is_empty() && !inserts.is_empty() {
        return vec![Edit::Replace {
            old_at_key: deletes.first().map(|line| line.occurrence_key),
            new_at_key: inserts.first().map(|line| line.occurrence_key),
            left_anchor: deletes.first().map(to_anchor),
            right_anchor: inserts.first().map(to_anchor),
            old_lines: deletes,
            new_lines: inserts,
        }];
    }

    if !deletes.is_empty() {
        return vec![Edit::Delete {
            at_key: deletes.first().map(|line| line.occurrence_key),
            left_anchor: deletes.first().map(to_anchor),
            right_anchor: None,
            lines: deletes,
        }];
    }

    vec![Edit::Insert {
        at_key: inserts.first().map(|line| line.occurrence_key),
        left_anchor: None,
        right_anchor: inserts.first().map(to_anchor),
        lines: inserts,
    }]
}

fn to_diff_line(line: &ComparisonLine) -> DiffLine {
    DiffLine {
        content_key: line.content_key,
        occurrence_key: line.occurrence_key,
        text: line.original.clone(),
        path: line.path.clone(),
        span: line.span.clone(),
    }
}

fn to_anchor(line: &DiffLine) -> EditAnchor {
    EditAnchor {
        path: line.path.clone(),
        span: line.span.clone(),
    }
}

fn path_starts_with(path: &[usize], prefix: &[usize]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}

fn parent_path(path: &Path) -> Path {
    let mut p = path.0.clone();
    p.pop();
    Path(p)
}

fn compute_ops(a: &[u64], b: &[u64]) -> Vec<Op> {
    let n = a.len();
    let m = b.len();

    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut i = 0usize;
    let mut j = 0usize;
    let mut ops = Vec::new();

    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Equal);
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            ops.push(Op::Delete);
            i += 1;
        } else {
            ops.push(Op::Insert);
            j += 1;
        }
    }

    while i < n {
        ops.push(Op::Delete);
        i += 1;
    }
    while j < m {
        ops.push(Op::Insert);
        j += 1;
    }

    ops
}

fn build_stats(edits: &[Edit]) -> DiffStats {
    let mut stats = DiffStats::default();

    for edit in edits {
        match edit {
            Edit::Insert { lines, .. } => {
                stats.inserts += 1;
                stats.inserted_lines += lines.len();
            }
            Edit::Delete { lines, .. } => {
                stats.deletes += 1;
                stats.deleted_lines += lines.len();
            }
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                stats.replaces += 1;
                stats.replaced_old_lines += old_lines.len();
                stats.replaced_new_lines += new_lines.len();
            }
        }
    }

    stats
}

fn collect_findings(
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
    collect_fallback_alignment_findings(fallback_contexts, &mut findings);
    findings.sort_by(|a, b| {
        let ap = a.path.as_ref().map(|p| p.0.clone()).unwrap_or_default();
        let bp = b.path.as_ref().map(|p| p.0.clone()).unwrap_or_default();
        (a.message.clone(), ap).cmp(&(b.message.clone(), bp))
    });
    findings
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
                key_label(Some(key)),
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

fn describe_edit(edit: &Edit) -> String {
    match edit {
        Edit::Insert { at_key, lines, .. } => format!(
            "Insert {} line(s) at key {}",
            lines.len(),
            key_label(*at_key),
        ),
        Edit::Delete { at_key, lines, .. } => format!(
            "Delete {} line(s) at key {}",
            lines.len(),
            key_label(*at_key),
        ),
        Edit::Replace {
            old_at_key,
            new_at_key,
            old_lines,
            new_lines,
            ..
        } => format!(
            "Replace {} line(s) at key {} with {} line(s) at key {}",
            old_lines.len(),
            key_label(*old_at_key),
            new_lines.len(),
            key_label(*new_at_key),
        ),
    }
}

fn key_label(key: Option<u64>) -> String {
    match key {
        Some(v) => format!("0x{v:016x}"),
        None => "<unknown>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use netform_ir::parse_generic;

    use super::{
        Diff, DiffLine, Edit, EditAnchor, NormalizationStep, NormalizeOptions, OrderPolicy,
        OrderPolicyConfig, PlanAction, PlanLineEditKind, Span, build_plan, diff_documents,
    };

    #[test]
    fn detects_replace_edit() {
        let a = parse_generic("interface Ethernet1\n  description old\n");
        let b = parse_generic("interface Ethernet1\n  description new\n");

        let diff = diff_documents(&a, &b, NormalizeOptions::default());
        assert_eq!(diff.edits.len(), 1);
        assert!(matches!(diff.edits[0], Edit::Replace { .. }));
    }

    #[test]
    fn ignores_comments_when_configured() {
        let a = parse_generic("! generated\ninterface Ethernet1\n");
        let b = parse_generic("! changed comment\ninterface Ethernet1\n");

        let diff = diff_documents(
            &a,
            &b,
            NormalizeOptions::new(vec![NormalizationStep::IgnoreComments]),
        );

        assert!(diff.edits.is_empty());
    }

    #[test]
    fn records_applied_normalization_steps() {
        let a = parse_generic("line  a\n");
        let b = parse_generic("line a\n");
        let options = NormalizeOptions::new(vec![NormalizationStep::CollapseInternalWhitespace]);

        let diff = diff_documents(&a, &b, options);
        assert_eq!(
            diff.normalization_steps,
            vec![NormalizationStep::CollapseInternalWhitespace]
        );
    }

    #[test]
    fn block_aware_diff_only_reports_changed_children() {
        let a = parse_generic("interface Ethernet1\n  description old\n  mtu 9000\n");
        let b = parse_generic("interface Ethernet1\n  description new\n  mtu 9000\n");

        let diff = diff_documents(&a, &b, NormalizeOptions::default());
        assert_eq!(diff.edits.len(), 1);

        match &diff.edits[0] {
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                assert_eq!(old_lines.len(), 1);
                assert_eq!(new_lines.len(), 1);
                assert_eq!(old_lines[0].text, "  description old");
                assert_eq!(new_lines[0].text, "  description new");
            }
            _ => panic!("expected a replace edit"),
        }
    }

    #[test]
    fn ambiguous_duplicate_lines_create_findings() {
        let a = parse_generic("line\nline\nline\n");
        let b = parse_generic("line\nline\nline\n");

        let diff = diff_documents(&a, &b, NormalizeOptions::default());
        assert!(!diff.has_changes);
        assert!(!diff.findings.is_empty());
        assert!(
            diff.findings
                .iter()
                .any(|f| f.message.contains("ambiguous content key"))
        );
    }

    #[test]
    fn reports_has_changes_for_drift() {
        let a = parse_generic("hostname old\n");
        let b = parse_generic("hostname new\n");

        let diff = diff_documents(&a, &b, NormalizeOptions::default());
        assert!(diff.has_changes);
    }

    #[test]
    fn ordered_policy_reports_reordered_block_children_as_change() {
        let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n");
        let b = parse_generic("interface Ethernet1\n  mtu 9000\n  description uplink\n");

        let diff = diff_documents(
            &a,
            &b,
            NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
                default: OrderPolicy::Ordered,
                overrides: Vec::new(),
            }),
        );

        assert!(diff.has_changes);
    }

    #[test]
    fn unordered_policy_ignores_reordered_block_children() {
        let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n");
        let b = parse_generic("interface Ethernet1\n  mtu 9000\n  description uplink\n");

        let diff = diff_documents(
            &a,
            &b,
            NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
                default: OrderPolicy::Unordered,
                overrides: Vec::new(),
            }),
        );

        assert!(!diff.has_changes);
    }

    #[test]
    fn keyed_stable_policy_ignores_reordered_block_children() {
        let a = parse_generic("interface Ethernet1\n  description uplink\n  mtu 9000\n");
        let b = parse_generic("interface Ethernet1\n  mtu 9000\n  description uplink\n");

        let diff = diff_documents(
            &a,
            &b,
            NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
                default: OrderPolicy::KeyedStable,
                overrides: Vec::new(),
            }),
        );

        assert!(!diff.has_changes);
    }

    #[test]
    fn fallback_alignment_emits_finding() {
        let a = parse_generic("interface Ethernet1\n  description one\n");
        let b = parse_generic("router bgp 65000\n  neighbor 10.0.0.1 remote-as 65001\n");

        let diff = diff_documents(&a, &b, NormalizeOptions::default());
        assert!(
            diff.findings
                .iter()
                .any(|f| f.message.contains("fallback segment alignment"))
        );
    }

    #[test]
    fn parse_uncertainty_is_exposed_as_finding() {
        let a = parse_generic("  orphan-line\n");
        let b = parse_generic("  orphan-line\n");

        let diff = diff_documents(&a, &b, NormalizeOptions::default());
        assert!(
            diff.findings
                .iter()
                .any(|f| f.code == "unknown_unparsed_construct")
        );
    }

    #[test]
    fn build_plan_emits_missing_anchor_finding_when_anchor_is_absent() {
        let diff = Diff {
            edits: vec![Edit::Insert {
                at_key: None,
                left_anchor: None,
                right_anchor: None,
                lines: vec![DiffLine {
                    content_key: 1,
                    occurrence_key: 1,
                    text: "set system host-name edge-1".to_string(),
                    path: super::Path(vec![0]),
                    span: Span {
                        line: 1,
                        start_byte: 0,
                        end_byte: 27,
                    },
                }],
            }],
            ..Diff::default()
        };

        let plan = build_plan(&diff);
        assert!(plan.actions.is_empty());
        assert!(
            plan.findings
                .iter()
                .any(|f| f.code == "missing_anchor" && f.message.contains("insert"))
        );
    }

    #[test]
    fn build_plan_creates_insert_and_delete_line_actions_with_anchor_context() {
        let delete_anchor = EditAnchor {
            path: super::Path(vec![0, 2]),
            span: Span {
                line: 3,
                start_byte: 20,
                end_byte: 36,
            },
        };
        let insert_anchor = EditAnchor {
            path: super::Path(vec![0, 1]),
            span: Span {
                line: 2,
                start_byte: 10,
                end_byte: 28,
            },
        };

        let diff = Diff {
            edits: vec![
                Edit::Delete {
                    at_key: Some(11),
                    left_anchor: Some(delete_anchor),
                    right_anchor: None,
                    lines: vec![DiffLine {
                        content_key: 11,
                        occurrence_key: 11,
                        text: "  no shutdown".to_string(),
                        path: super::Path(vec![0, 2]),
                        span: Span {
                            line: 3,
                            start_byte: 20,
                            end_byte: 32,
                        },
                    }],
                },
                Edit::Insert {
                    at_key: Some(22),
                    left_anchor: None,
                    right_anchor: Some(insert_anchor),
                    lines: vec![DiffLine {
                        content_key: 22,
                        occurrence_key: 22,
                        text: "  shutdown".to_string(),
                        path: super::Path(vec![0, 1]),
                        span: Span {
                            line: 2,
                            start_byte: 10,
                            end_byte: 20,
                        },
                    }],
                },
            ],
            ..Diff::default()
        };

        let plan = build_plan(&diff);
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.findings.len(), 0);

        match &plan.actions[0] {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => {
                assert_eq!(context_path.0, vec![0]);
                assert_eq!(line_edits[0].kind, PlanLineEditKind::Delete);
            }
            _ => panic!("expected delete line-edit action"),
        }

        match &plan.actions[1] {
            PlanAction::ApplyLineEditsUnderContext {
                context_path,
                line_edits,
            } => {
                assert_eq!(context_path.0, vec![0]);
                assert_eq!(line_edits[0].kind, PlanLineEditKind::Insert);
            }
            _ => panic!("expected insert line-edit action"),
        }
    }
}
