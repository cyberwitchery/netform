use serde::{Deserialize, Serialize};

use netform_ir::{Path, Span, TriviaKind};

/// errors that can occur during diff computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffError {
    /// the Myers shortest-edit-script algorithm did not converge within
    /// the expected number of steps.
    SesNotConverged,
    /// the edit-script operation sequence was inconsistent with the input
    /// data (e.g., referenced more elements than present in one side).
    EditScriptInconsistency(String),
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SesNotConverged => {
                write!(
                    f,
                    "Myers SES algorithm did not converge within expected steps"
                )
            }
            Self::EditScriptInconsistency(detail) => {
                write!(f, "diff engine inconsistency: {detail}")
            }
        }
    }
}

impl std::error::Error for DiffError {}

/// one ordered normalization step in the comparison pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizationStep {
    IgnoreComments,
    IgnoreBlankLines,
    TrimTrailingWhitespace,
    NormalizeLeadingWhitespace,
    CollapseInternalWhitespace,
}

/// ordering behavior used when comparing sibling lines in a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrderPolicy {
    Ordered,
    Unordered,
    KeyedStable,
}

/// path-based policy override for specific subtree contexts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderPolicyOverride {
    pub context_prefix: Vec<usize>,
    pub policy: OrderPolicy,
}

/// ordering policy configuration with a default and longest-prefix overrides.
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

/// options controlling normalization and ordering semantics for diffing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizeOptions {
    pub steps: Vec<NormalizationStep>,
    pub order_policy: OrderPolicyConfig,
}

impl NormalizeOptions {
    /// create options with an explicit ordered normalization step list.
    pub fn new(steps: Vec<NormalizationStep>) -> Self {
        Self {
            steps,
            order_policy: OrderPolicyConfig::default(),
        }
    }

    /// override ordering policy configuration.
    pub fn with_order_policy(mut self, order_policy: OrderPolicyConfig) -> Self {
        self.order_policy = order_policy;
        self
    }

    pub(crate) fn policy_for_path(&self, path: &Path) -> OrderPolicy {
        self.order_policy.policy_for_path(path)
    }
}

/// one normalized line in the internal comparison view.
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

/// flattened line-oriented view derived from a document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComparisonView {
    pub lines: Vec<ComparisonLine>,
}

/// serializable line payload embedded in diff edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub content_key: u64,
    pub occurrence_key: u64,
    pub text: String,
    pub path: Path,
    pub span: Span,
}

/// path/span anchor for edit placement and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditAnchor {
    pub path: Path,
    pub span: Span,
}

/// edit script operation emitted by the diff engine.
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

/// aggregate counters for diff output.
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

/// machine-readable diagnostic codes used in [`Finding`] and [`PlanFinding`].
pub mod finding_code {
    pub const MISSING_ANCHOR: &str = "missing_anchor";
    pub const AMBIGUOUS_KEY_MATCH: &str = "ambiguous_key_match";
    pub const UNKNOWN_UNPARSED_CONSTRUCT: &str = "unknown_unparsed_construct";
    pub const DIFF_UNRELIABLE_REGION: &str = "diff_unreliable_region";
}

/// warning/info emitted during parse propagation or diff uncertainty handling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: String,
    pub level: FindingLevel,
    pub message: String,
    pub path: Option<Path>,
    pub span: Option<Span>,
}

/// severity level for a [`Finding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingLevel {
    Warning,
    Info,
}

impl std::fmt::Display for FindingLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Warning => f.write_str("warning"),
            Self::Info => f.write_str("info"),
        }
    }
}

/// top-level diff output contract.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Diff {
    pub normalization_steps: Vec<NormalizationStep>,
    pub order_policy: OrderPolicyConfig,
    pub has_changes: bool,
    pub edits: Vec<Edit>,
    pub stats: DiffStats,
    pub findings: Vec<Finding>,
}

/// transport-neutral action plan derived from a [`Diff`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub version: String,
    pub actions: Vec<PlanAction>,
    pub findings: Vec<PlanFinding>,
}

/// action variants emitted in a [`Plan`].
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

/// one line-oriented edit in `apply_line_edits_under_context`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanLineEdit {
    pub kind: PlanLineEditKind,
    pub text: String,
}

/// line operation kind for [`PlanLineEdit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanLineEditKind {
    Insert,
    Delete,
    Replace,
}

/// plan-level warning (for example missing anchors).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanFinding {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<FindingLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

/// key namespace discriminator used when hashing comparison identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyKind {
    Line,
    BlockHeader,
    BlockFooter,
}

impl KeyKind {
    /// the `Debug` representation as a byte slice, matching what `{:?}` produces.
    fn as_bytes(self) -> &'static [u8] {
        match self {
            Self::Line => b"Line",
            Self::BlockHeader => b"BlockHeader",
            Self::BlockFooter => b"BlockFooter",
        }
    }
}

/// format a `u64` as decimal digits into a stack buffer, returning the slice.
struct U64Buf {
    buf: [u8; 20], // u64::MAX = 18446744073709551615 (20 digits)
    start: usize,
}

impl U64Buf {
    fn new(value: u64) -> Self {
        let mut buf = [0u8; 20];
        if value == 0 {
            buf[19] = b'0';
            return Self { buf, start: 19 };
        }
        let mut pos = 20usize;
        let mut v = value;
        while v > 0 {
            pos -= 1;
            buf[pos] = b'0' + (v % 10) as u8;
            v /= 10;
        }
        Self { buf, start: pos }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.buf[self.start..]
    }
}

/// derive a content key from parent signature, key kind, trivia, and normalized text.
///
/// uses incremental xxh3 hashing to avoid the intermediate `String` allocation
/// that `format!()` would require.  The byte sequence fed to the hasher is
/// identical to what the old `format!()` call produced, so hash values are
/// unchanged.
pub fn derive_content_key(
    parent_signature: u64,
    kind: KeyKind,
    trivia: TriviaKind,
    normalized_for_key: &str,
) -> u64 {
    let mut h = xxhash_rust::xxh3::Xxh3Default::new();
    h.update(b"p=");
    h.update(U64Buf::new(parent_signature).as_bytes());
    h.update(b"|k=");
    h.update(kind.as_bytes());
    h.update(b"|t=");
    h.update(crate::normalize::trivia_tag(trivia).as_bytes());
    h.update(b"|n=");
    h.update(normalized_for_key.as_bytes());
    h.digest()
}

/// derive an occurrence key from content key and 1-based ordinal.
///
/// uses incremental xxh3 hashing to avoid the intermediate `String` allocation.
pub fn derive_occurrence_key(content_key: u64, ordinal: u64) -> u64 {
    let mut h = xxhash_rust::xxh3::Xxh3Default::new();
    h.update(b"c=");
    h.update(U64Buf::new(content_key).as_bytes());
    h.update(b"|o=");
    h.update(U64Buf::new(ordinal).as_bytes());
    h.digest()
}

#[cfg(test)]
mod tests {
    use super::*;
    use netform_ir::Path;

    #[test]
    fn policy_for_path_returns_default_with_no_overrides() {
        let config = OrderPolicyConfig {
            default: OrderPolicy::Unordered,
            overrides: Vec::new(),
        };
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1, 2])),
            OrderPolicy::Unordered
        );
    }

    #[test]
    fn policy_for_path_matches_exact_prefix() {
        let config = OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: vec![OrderPolicyOverride {
                context_prefix: vec![0, 1],
                policy: OrderPolicy::Unordered,
            }],
        };
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1])),
            OrderPolicy::Unordered
        );
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1, 5])),
            OrderPolicy::Unordered
        );
    }

    #[test]
    fn policy_for_path_falls_back_when_no_prefix_matches() {
        let config = OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: vec![OrderPolicyOverride {
                context_prefix: vec![3, 4],
                policy: OrderPolicy::Unordered,
            }],
        };
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1])),
            OrderPolicy::Ordered
        );
    }

    #[test]
    fn policy_for_path_picks_longest_prefix() {
        let config = OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: vec![
                OrderPolicyOverride {
                    context_prefix: vec![0],
                    policy: OrderPolicy::Unordered,
                },
                OrderPolicyOverride {
                    context_prefix: vec![0, 1],
                    policy: OrderPolicy::KeyedStable,
                },
            ],
        };
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1, 2])),
            OrderPolicy::KeyedStable
        );
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 5])),
            OrderPolicy::Unordered
        );
    }

    #[test]
    fn policy_for_path_longest_prefix_wins_regardless_of_order() {
        let config = OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: vec![
                OrderPolicyOverride {
                    context_prefix: vec![0, 1, 2],
                    policy: OrderPolicy::KeyedStable,
                },
                OrderPolicyOverride {
                    context_prefix: vec![0],
                    policy: OrderPolicy::Unordered,
                },
                OrderPolicyOverride {
                    context_prefix: vec![0, 1],
                    policy: OrderPolicy::Ordered,
                },
            ],
        };
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1, 2])),
            OrderPolicy::KeyedStable
        );
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1])),
            OrderPolicy::Ordered
        );
        assert_eq!(
            config.policy_for_path(&Path(vec![0])),
            OrderPolicy::Unordered
        );
    }

    #[test]
    fn policy_for_path_empty_prefix_matches_everything() {
        let config = OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: vec![OrderPolicyOverride {
                context_prefix: vec![],
                policy: OrderPolicy::Unordered,
            }],
        };
        assert_eq!(
            config.policy_for_path(&Path(vec![])),
            OrderPolicy::Unordered
        );
        assert_eq!(
            config.policy_for_path(&Path(vec![0, 1])),
            OrderPolicy::Unordered
        );
    }

    #[test]
    fn policy_for_path_empty_path_only_matches_empty_prefix() {
        let config = OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: vec![
                OrderPolicyOverride {
                    context_prefix: vec![],
                    policy: OrderPolicy::Unordered,
                },
                OrderPolicyOverride {
                    context_prefix: vec![0],
                    policy: OrderPolicy::KeyedStable,
                },
            ],
        };
        assert_eq!(
            config.policy_for_path(&Path(vec![])),
            OrderPolicy::Unordered
        );
    }

    #[test]
    fn normalize_options_new_sets_steps_and_default_policy() {
        let opts = NormalizeOptions::new(vec![
            NormalizationStep::IgnoreComments,
            NormalizationStep::TrimTrailingWhitespace,
        ]);
        assert_eq!(opts.steps.len(), 2);
        assert_eq!(opts.steps[0], NormalizationStep::IgnoreComments);
        assert_eq!(opts.steps[1], NormalizationStep::TrimTrailingWhitespace);
        assert_eq!(opts.order_policy, OrderPolicyConfig::default());
    }

    #[test]
    fn normalize_options_with_order_policy_overrides_policy() {
        let policy = OrderPolicyConfig {
            default: OrderPolicy::Unordered,
            overrides: vec![OrderPolicyOverride {
                context_prefix: vec![0],
                policy: OrderPolicy::Ordered,
            }],
        };
        let opts = NormalizeOptions::new(vec![NormalizationStep::IgnoreBlankLines])
            .with_order_policy(policy.clone());
        assert_eq!(opts.order_policy, policy);
        assert_eq!(opts.steps, vec![NormalizationStep::IgnoreBlankLines]);
    }

    #[test]
    fn normalize_options_policy_for_path_delegates_to_config() {
        let opts = NormalizeOptions::new(vec![]).with_order_policy(OrderPolicyConfig {
            default: OrderPolicy::Ordered,
            overrides: vec![OrderPolicyOverride {
                context_prefix: vec![1],
                policy: OrderPolicy::KeyedStable,
            }],
        });
        assert_eq!(
            opts.policy_for_path(&Path(vec![1, 2])),
            OrderPolicy::KeyedStable
        );
        assert_eq!(opts.policy_for_path(&Path(vec![0])), OrderPolicy::Ordered);
    }

    #[test]
    fn diff_stats_default_is_all_zeros() {
        let stats = DiffStats::default();
        assert_eq!(stats.inserts, 0);
        assert_eq!(stats.deletes, 0);
        assert_eq!(stats.replaces, 0);
        assert_eq!(stats.inserted_lines, 0);
        assert_eq!(stats.deleted_lines, 0);
        assert_eq!(stats.replaced_old_lines, 0);
        assert_eq!(stats.replaced_new_lines, 0);
    }

    #[test]
    fn order_policy_config_default_is_ordered_no_overrides() {
        let config = OrderPolicyConfig::default();
        assert_eq!(config.default, OrderPolicy::Ordered);
        assert!(config.overrides.is_empty());
    }

    // --- U64Buf tests ---

    #[test]
    fn u64buf_zero() {
        assert_eq!(U64Buf::new(0).as_bytes(), b"0");
    }

    #[test]
    fn u64buf_small() {
        assert_eq!(U64Buf::new(42).as_bytes(), b"42");
    }

    #[test]
    fn u64buf_max() {
        assert_eq!(U64Buf::new(u64::MAX).as_bytes(), b"18446744073709551615");
    }

    #[test]
    fn u64buf_matches_display() {
        for &v in &[0, 1, 9, 10, 99, 100, 999, 12345, u64::MAX] {
            assert_eq!(
                std::str::from_utf8(U64Buf::new(v).as_bytes()).unwrap(),
                v.to_string(),
            );
        }
    }

    // --- Hash stability tests (incremental == one-shot) ---

    /// reference implementation using the old format!() + xxh3_64() approach.
    fn derive_content_key_reference(
        parent_signature: u64,
        kind: KeyKind,
        trivia: TriviaKind,
        normalized_for_key: &str,
    ) -> u64 {
        let canonical = format!(
            "p={parent_signature}|k={:?}|t={}|n={}",
            kind,
            crate::normalize::trivia_tag(trivia),
            normalized_for_key
        );
        xxhash_rust::xxh3::xxh3_64(canonical.as_bytes())
    }

    fn derive_occurrence_key_reference(content_key: u64, ordinal: u64) -> u64 {
        let canonical = format!("c={content_key}|o={ordinal}");
        xxhash_rust::xxh3::xxh3_64(canonical.as_bytes())
    }

    #[test]
    fn content_key_matches_reference_implementation() {
        let cases: &[(u64, KeyKind, TriviaKind, &str)] = &[
            (0, KeyKind::Line, TriviaKind::Content, "permit any"),
            (
                12345,
                KeyKind::BlockHeader,
                TriviaKind::Blank,
                "interface Ethernet0",
            ),
            (u64::MAX, KeyKind::BlockFooter, TriviaKind::Comment, "!"),
            (42, KeyKind::Line, TriviaKind::Unknown, ""),
            (
                999,
                KeyKind::Line,
                TriviaKind::Content,
                "a line with spaces and special chars: <>!@#$%",
            ),
        ];
        for &(parent, kind, trivia, text) in cases {
            assert_eq!(
                derive_content_key(parent, kind, trivia, text),
                derive_content_key_reference(parent, kind, trivia, text),
                "mismatch for parent={parent}, kind={kind:?}, trivia={trivia:?}, text={text:?}"
            );
        }
    }

    #[test]
    fn occurrence_key_matches_reference_implementation() {
        let cases: &[(u64, u64)] = &[
            (0, 1),
            (12345, 2),
            (u64::MAX, u64::MAX),
            (42, 0),
            (999_999_999, 100),
        ];
        for &(content_key, ordinal) in cases {
            assert_eq!(
                derive_occurrence_key(content_key, ordinal),
                derive_occurrence_key_reference(content_key, ordinal),
                "mismatch for content_key={content_key}, ordinal={ordinal}"
            );
        }
    }
}
