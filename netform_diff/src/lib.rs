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

mod engine;
mod findings;
mod flatten;
mod model;
mod normalize;
mod plan;
mod report;
mod util;

pub use flatten::build_comparison_view;
pub use model::{
    ComparisonLine, ComparisonView, Diff, DiffLine, DiffStats, Edit, EditAnchor, Finding,
    FindingLevel, KeyKind, NormalizationStep, NormalizeOptions, OrderPolicy, OrderPolicyConfig,
    OrderPolicyOverride, Plan, PlanAction, PlanFinding, PlanLineEdit, PlanLineEditKind,
    derive_content_key, derive_occurrence_key,
};
pub use plan::build_plan;
pub use report::format_markdown_report;

use netform_ir::Document;

/// Compute a deterministic diff between two parsed documents.
pub fn diff_documents(a: &Document, b: &Document, options: NormalizeOptions) -> Diff {
    let a_view = build_comparison_view(a, &options);
    let b_view = build_comparison_view(b, &options);
    let ctx = findings::DiffContext::from_views(&a_view, &b_view);
    let computation = engine::diff_views(&a_view, &b_view, &options);
    let stats = engine::build_stats(&computation.edits);
    let findings =
        findings::collect_findings(a, b, &a_view, &b_view, &ctx, &computation.fallback_contexts);
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

#[cfg(test)]
mod tests;
