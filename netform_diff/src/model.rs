use serde::{Deserialize, Serialize};

use netform_ir::{Path, Span, TriviaKind};

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
    pub(crate) fn policy_for_path(&self, path: &Path) -> OrderPolicy {
        let mut best: Option<(&OrderPolicyOverride, usize)> = None;
        for rule in &self.overrides {
            if crate::util::path_starts_with(&path.0, &rule.context_prefix) {
                let len = rule.context_prefix.len();
                if best.is_none_or(|(_, best_len)| len > best_len) {
                    best = Some((rule, len));
                }
            }
        }
        best.map_or(self.default, |(rule, _)| rule.policy)
    }
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

    pub(crate) fn policy_for_path(&self, path: &Path) -> OrderPolicy {
        self.order_policy.policy_for_path(path)
    }
}

/// One normalized line in the internal comparison view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonLine {
    pub content_key: u64,
    pub occurrence_key: u64,
    pub key_hint: Option<String>,
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

/// Key namespace discriminator used when hashing comparison identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    Line,
    BlockHeader,
    BlockFooter,
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
        crate::normalize::trivia_tag(trivia),
        normalized_for_key
    );
    xxhash_rust::xxh3::xxh3_64(canonical_content.as_bytes())
}

/// Derive an occurrence key from content key and 1-based ordinal.
pub fn derive_occurrence_key(content_key: u64, ordinal: u64) -> u64 {
    let canonical_occurrence = format!("c={content_key}|o={ordinal}");
    xxhash_rust::xxh3::xxh3_64(canonical_occurrence.as_bytes())
}
