use std::collections::HashMap;

use crate::model::{
    ComparisonLine, ComparisonView, DiffError, DiffLine, DiffStats, Edit, EditAnchor,
    NormalizeOptions, OrderPolicy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Equal,
    Delete,
    Insert,
}

#[derive(Debug, Clone)]
struct Segment {
    lines: Vec<ComparisonLine>,
    segment_key: u64,
    is_block: bool,
}

#[derive(Debug, Default)]
pub(crate) struct DiffComputation {
    pub edits: Vec<Edit>,
    pub fallback_contexts: Vec<netform_ir::Path>,
}

pub(crate) fn diff_views(
    a: &ComparisonView,
    b: &ComparisonView,
    options: &NormalizeOptions,
) -> Result<DiffComputation, DiffError> {
    let mut edits = Vec::new();
    let mut fallback_contexts = Vec::new();
    diff_segment_level(
        &a.lines,
        &b.lines,
        0,
        options,
        &mut edits,
        &mut fallback_contexts,
    )?;
    Ok(DiffComputation {
        edits,
        fallback_contexts,
    })
}

/// structure-aware sibling matching at one nesting `depth`.
///
/// segments the lines by their path component at `depth`, aligns segments with
/// Myers, and recurses into each matched block under that block's own order
/// policy. only the top level (`depth == 0`) records fallback contexts.
fn diff_segment_level(
    a_lines: &[ComparisonLine],
    b_lines: &[ComparisonLine],
    depth: usize,
    options: &NormalizeOptions,
    edits: &mut Vec<Edit>,
    fallback_contexts: &mut Vec<netform_ir::Path>,
) -> Result<(), DiffError> {
    let a_segments = segment_at(a_lines, depth);
    let b_segments = segment_at(b_lines, depth);

    let a_keys = a_segments
        .iter()
        .map(|segment| segment.segment_key)
        .collect::<Vec<_>>();
    let b_keys = b_segments
        .iter()
        .map(|segment| segment.segment_key)
        .collect::<Vec<_>>();

    let ops = compute_ops(&a_keys, &b_keys)?;

    let a_count = a_segments.len();
    let b_count = b_segments.len();
    let record_fallbacks = depth == 0;
    let mut a_iter = a_segments.into_iter();
    let mut b_iter = b_segments.into_iter();
    let mut pending_deleted: Vec<Segment> = Vec::new();
    let mut pending_inserted: Vec<Segment> = Vec::new();

    for op in ops {
        match op {
            Op::Equal => {
                flush_replaced_segments(
                    &mut pending_deleted,
                    &mut pending_inserted,
                    options,
                    edits,
                    fallback_contexts,
                    record_fallbacks,
                )?;

                let left = a_iter.next().ok_or(DiffError::EditScriptInconsistency {
                    op: "Equal",
                    side: "left",
                    a_count,
                    b_count,
                })?;
                let right = b_iter.next().ok_or(DiffError::EditScriptInconsistency {
                    op: "Equal",
                    side: "right",
                    a_count,
                    b_count,
                })?;
                diff_matched_segment(&left, &right, options, edits, fallback_contexts)?;
            }
            Op::Delete => {
                pending_deleted.push(a_iter.next().ok_or(DiffError::EditScriptInconsistency {
                    op: "Delete",
                    side: "left",
                    a_count,
                    b_count,
                })?);
            }
            Op::Insert => {
                pending_inserted.push(b_iter.next().ok_or(DiffError::EditScriptInconsistency {
                    op: "Insert",
                    side: "right",
                    a_count,
                    b_count,
                })?);
            }
        }
    }

    flush_replaced_segments(
        &mut pending_deleted,
        &mut pending_inserted,
        options,
        edits,
        fallback_contexts,
        record_fallbacks,
    )
}

/// emits edits for two segments Myers aligned as equal.
///
/// only two matched blocks carry sub-edits: their headers are compared directly
/// and their children diffed under the policy for this block's path.
fn diff_matched_segment(
    left: &Segment,
    right: &Segment,
    options: &NormalizeOptions,
    edits: &mut Vec<Edit>,
    fallback_contexts: &mut Vec<netform_ir::Path>,
) -> Result<(), DiffError> {
    if !(left.is_block && right.is_block) {
        return Ok(());
    }

    let left_header = &left.lines[0];
    let right_header = &right.lines[0];
    // headers can share a lossy key but differ in text; surface that as a Replace.
    if left_header.normalized != right_header.normalized {
        let old_line = to_diff_line(left_header);
        let new_line = to_diff_line(right_header);
        edits.push(Edit::Replace {
            old_at_key: Some(old_line.occurrence_key),
            new_at_key: Some(new_line.occurrence_key),
            left_anchor: Some(to_anchor(&old_line)),
            right_anchor: Some(to_anchor(&new_line)),
            old_lines: vec![old_line],
            new_lines: vec![new_line],
        });
    }

    let left_children = &left.lines[1..];
    let right_children = &right.lines[1..];

    match options.policy_for_path(&left_header.path) {
        OrderPolicy::Ordered => diff_segment_level(
            left_children,
            right_children,
            left_header.path.0.len(),
            options,
            edits,
            fallback_contexts,
        )?,
        OrderPolicy::Unordered => {
            edits.append(&mut line_diff_unordered(left_children, right_children));
        }
        OrderPolicy::KeyedStable => {
            edits.append(&mut line_diff_keyed_stable(left_children, right_children));
        }
    }

    Ok(())
}

/// diffs accumulated non-matching segments as a coarse line-level fallback.
fn flush_replaced_segments(
    deleted: &mut Vec<Segment>,
    inserted: &mut Vec<Segment>,
    options: &NormalizeOptions,
    edits: &mut Vec<Edit>,
    fallback_contexts: &mut Vec<netform_ir::Path>,
    record_fallbacks: bool,
) -> Result<(), DiffError> {
    if deleted.is_empty() && inserted.is_empty() {
        return Ok(());
    }

    let deleted_lines = deleted
        .drain(..)
        .flat_map(|segment| segment.lines)
        .collect::<Vec<_>>();
    let inserted_lines = inserted
        .drain(..)
        .flat_map(|segment| segment.lines)
        .collect::<Vec<_>>();

    let first_path = deleted_lines
        .first()
        .or(inserted_lines.first())
        .map(|line| &line.path);
    let empty_path = netform_ir::Path(Vec::new());
    let mut fallback = line_diff(
        &deleted_lines,
        &inserted_lines,
        options.policy_for_path(first_path.unwrap_or(&empty_path)),
    )?;
    if record_fallbacks && let Some(path) = first_path {
        fallback_contexts.push(path.clone());
    }
    edits.append(&mut fallback);
    Ok(())
}

pub(crate) fn build_stats(edits: &[Edit]) -> DiffStats {
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

/// groups consecutive lines into segments by their path component at `depth`.
///
/// callers pass lines sharing the first `depth` components, so the component at
/// `depth` identifies each sibling: its header is at path length `depth + 1`.
fn segment_at(lines: &[ComparisonLine], depth: usize) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut current_component: Option<usize> = None;
    let mut current = Vec::new();

    for line in lines {
        let component = line.path.0.get(depth).copied().unwrap_or(usize::MAX);
        if current_component != Some(component) {
            if !current.is_empty() {
                segments.push(lines_to_segment(std::mem::take(&mut current), depth));
            }
            current_component = Some(component);
        }

        current.push(line.clone());
    }

    if !current.is_empty() {
        segments.push(lines_to_segment(current, depth));
    }

    segments
}

fn lines_to_segment(lines: Vec<ComparisonLine>, depth: usize) -> Segment {
    let is_block = lines.iter().any(|line| line.path.0.len() > depth + 1);
    let segment_key = lines.first().map(|line| line.content_key).unwrap_or(0);
    Segment {
        lines,
        segment_key,
        is_block,
    }
}

fn line_diff(
    a: &[ComparisonLine],
    b: &[ComparisonLine],
    policy: OrderPolicy,
) -> Result<Vec<Edit>, DiffError> {
    match policy {
        OrderPolicy::Ordered => line_diff_ordered(a, b),
        OrderPolicy::Unordered => Ok(line_diff_unordered(a, b)),
        OrderPolicy::KeyedStable => Ok(line_diff_keyed_stable(a, b)),
    }
}

fn line_diff_ordered(a: &[ComparisonLine], b: &[ComparisonLine]) -> Result<Vec<Edit>, DiffError> {
    let a_tokens = a.iter().map(|line| line.content_key).collect::<Vec<_>>();
    let b_tokens = b.iter().map(|line| line.content_key).collect::<Vec<_>>();
    let ops = compute_ops(&a_tokens, &b_tokens)?;

    let mut edits = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    let mut pending_deletes: Vec<DiffLine> = Vec::new();
    let mut pending_inserts: Vec<DiffLine> = Vec::new();

    // pending lines arrive in diff order and must stay that way, so this path
    // hands them to finalize_edit unsorted.
    let flush =
        |edits: &mut Vec<Edit>, deletes: &mut Vec<DiffLine>, inserts: &mut Vec<DiffLine>| {
            if let Some(edit) = finalize_edit(std::mem::take(deletes), std::mem::take(inserts)) {
                edits.push(edit);
            }
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
    Ok(edits)
}

fn line_diff_unordered(a: &[ComparisonLine], b: &[ComparisonLine]) -> Vec<Edit> {
    line_diff_multiset(a, b, |line| {
        xxhash_rust::xxh3::xxh3_64(line.normalized.as_bytes())
    })
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

    let mut all_keys: Vec<_> = a_buckets
        .keys()
        .copied()
        .chain(b_buckets.keys().copied())
        .collect();
    all_keys.sort_unstable();
    all_keys.dedup();

    let mut edits = Vec::new();
    for key in all_keys {
        let mut left = a_buckets.remove(&key).unwrap_or_default();
        let mut right = b_buckets.remove(&key).unwrap_or_default();

        left.sort_by(|a, b| {
            a.occurrence_key
                .cmp(&b.occurrence_key)
                .then_with(|| a.path.0.cmp(&b.path.0))
        });
        right.sort_by(|a, b| {
            a.occurrence_key
                .cmp(&b.occurrence_key)
                .then_with(|| a.path.0.cmp(&b.path.0))
        });

        let common = left.len().min(right.len());

        let mut bucket_deletes = Vec::new();
        let mut bucket_inserts = Vec::new();

        // paired lines share a content key but may differ in text (e.g.
        // FortiOS `set` lines matched by field name with different values).
        for idx in 0..common {
            if left[idx].normalized != right[idx].normalized {
                bucket_deletes.push(to_diff_line(left[idx]));
                bucket_inserts.push(to_diff_line(right[idx]));
            }
        }

        for line in left.into_iter().skip(common) {
            bucket_deletes.push(to_diff_line(line));
        }
        for line in right.into_iter().skip(common) {
            bucket_inserts.push(to_diff_line(line));
        }

        edits.extend(finalize_chunked_edits(bucket_deletes, bucket_inserts));
    }

    edits
}

/// Turns a chunk collected by the multiset paths into a single edit.
///
/// The multiset paths gather lines bucket by bucket, so the chunk has no
/// meaningful order until it is sorted here.
fn finalize_chunked_edits(mut deletes: Vec<DiffLine>, mut inserts: Vec<DiffLine>) -> Vec<Edit> {
    deletes.sort_by(|a, b| {
        a.content_key
            .cmp(&b.content_key)
            .then_with(|| a.occurrence_key.cmp(&b.occurrence_key))
            .then_with(|| a.path.0.cmp(&b.path.0))
    });
    inserts.sort_by(|a, b| {
        a.content_key
            .cmp(&b.content_key)
            .then_with(|| a.occurrence_key.cmp(&b.occurrence_key))
            .then_with(|| a.path.0.cmp(&b.path.0))
    });

    finalize_edit(deletes, inserts).into_iter().collect()
}

/// Builds the edit describing a chunk of deleted and inserted lines, or `None`
/// when both sides are empty.
///
/// Anchors and keys are taken from the first line of each side, so callers must
/// pass the lines in the order the edit should report them.
fn finalize_edit(deletes: Vec<DiffLine>, inserts: Vec<DiffLine>) -> Option<Edit> {
    if !deletes.is_empty() && !inserts.is_empty() {
        return Some(Edit::Replace {
            old_at_key: deletes.first().map(|line| line.occurrence_key),
            new_at_key: inserts.first().map(|line| line.occurrence_key),
            left_anchor: deletes.first().map(to_anchor),
            right_anchor: inserts.first().map(to_anchor),
            old_lines: deletes,
            new_lines: inserts,
        });
    }

    if !deletes.is_empty() {
        return Some(Edit::Delete {
            at_key: deletes.first().map(|line| line.occurrence_key),
            left_anchor: deletes.first().map(to_anchor),
            right_anchor: None,
            lines: deletes,
        });
    }

    if !inserts.is_empty() {
        return Some(Edit::Insert {
            at_key: inserts.first().map(|line| line.occurrence_key),
            left_anchor: None,
            right_anchor: inserts.first().map(to_anchor),
            lines: inserts,
        });
    }

    None
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

/// Compact snapshot of the v-vector at a single Myers edit step.
///
/// At step d, only diagonals -d, -d+2, ..., d are live (d+1 values).
/// Storing just those instead of cloning the full v-vector (length
/// 2*(a+b)+3) reduces trace memory from O(D*(a+b)) to O(D^2).
struct TraceSnapshot {
    d: isize,
    values: Vec<isize>,
}

impl TraceSnapshot {
    fn capture(d: isize, v: &[isize], offset: isize) -> Self {
        let count = (d + 1) as usize;
        let mut values = Vec::with_capacity(count);
        let mut k = -d;
        while k <= d {
            values.push(v[(k + offset) as usize]);
            k += 2;
        }
        TraceSnapshot { d, values }
    }

    fn get(&self, k: isize) -> isize {
        self.values[((k + self.d) / 2) as usize]
    }
}

pub(crate) fn compute_ops(a: &[u64], b: &[u64]) -> Result<Vec<Op>, DiffError> {
    if a.is_empty() {
        return Ok(vec![Op::Insert; b.len()]);
    }
    if b.is_empty() {
        return Ok(vec![Op::Delete; a.len()]);
    }

    let n = a.len() as isize;
    let m = b.len() as isize;
    let max = (a.len() + b.len()) as isize;
    let offset = max + 1;
    let v_len = (2 * max + 3) as usize;

    // Myers SES trace over diagonals. This avoids the quadratic LCS matrix and
    // remains deterministic for a fixed input/order.
    let mut v = vec![0isize; v_len];
    let mut trace: Vec<TraceSnapshot> = Vec::with_capacity((max + 1) as usize);

    for d in 0..=max {
        // diagonals are visited in steps of 2, so writes to v[idx] (diagonal k)
        // never collide with reads from v[idx-1] (k-1) or v[idx+1] (k+1) —
        // those diagonals have opposite parity and still hold their d-1 values.
        // this lets us mutate v in-place and snapshot the live diagonals only,
        // instead of cloning the entire v-vector into the trace.
        let mut k = -d;
        while k <= d {
            let idx = (k + offset) as usize;
            let take_down = k == -d || (k != d && v[idx - 1] < v[idx + 1]);
            let mut x = if take_down {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = x - k;

            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;

            if x >= n && y >= m {
                trace.push(TraceSnapshot::capture(d, &v, offset));
                return Ok(backtrack_ops(a, b, &trace));
            }
            k += 2;
        }
        trace.push(TraceSnapshot::capture(d, &v, offset));
    }

    Err(DiffError::SesNotConverged {
        a_len: a.len(),
        b_len: b.len(),
    })
}

fn backtrack_ops(a: &[u64], b: &[u64], trace: &[TraceSnapshot]) -> Vec<Op> {
    let mut x = a.len() as isize;
    let mut y = b.len() as isize;
    let mut rev_ops = Vec::new();

    for d in (1..trace.len()).rev() {
        let d = d as isize;
        let k = x - y;
        let prev = &trace[(d - 1) as usize];
        let go_down = k == -d || (k != d && prev.get(k - 1) < prev.get(k + 1));
        let prev_k = if go_down { k + 1 } else { k - 1 };
        let prev_x = prev.get(prev_k);
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            rev_ops.push(Op::Equal);
            x -= 1;
            y -= 1;
        }

        if x == prev_x {
            rev_ops.push(Op::Insert);
            y -= 1;
        } else {
            rev_ops.push(Op::Delete);
            x -= 1;
        }
    }

    while x > 0 && y > 0 && a[(x - 1) as usize] == b[(y - 1) as usize] {
        rev_ops.push(Op::Equal);
        x -= 1;
        y -= 1;
    }
    while x > 0 {
        rev_ops.push(Op::Delete);
        x -= 1;
    }
    while y > 0 {
        rev_ops.push(Op::Insert);
        y -= 1;
    }

    rev_ops.reverse();
    rev_ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{OrderPolicyConfig, OrderPolicyOverride};
    use netform_ir::{Path, Span, TriviaKind};

    fn span(line: usize) -> Span {
        Span {
            line,
            start_byte: 0,
            end_byte: 10,
        }
    }

    fn cline(text: &str, content_key: u64, path: Vec<usize>) -> ComparisonLine {
        let occurrence_key = crate::model::derive_occurrence_key(content_key, 1);
        ComparisonLine {
            content_key,
            occurrence_key,
            key_hint: None,
            normalized: text.to_string(),
            original: text.to_string(),
            path: Path(path.clone()),
            span: span(path.last().copied().unwrap_or(0)),
            trivia: TriviaKind::Content,
        }
    }

    fn dline(text: &str, content_key: u64, line: usize) -> DiffLine {
        DiffLine {
            content_key,
            occurrence_key: crate::model::derive_occurrence_key(content_key, 1),
            text: text.to_string(),
            path: Path(vec![line]),
            span: span(line),
        }
    }

    fn view(lines: Vec<ComparisonLine>) -> ComparisonView {
        ComparisonView { lines }
    }

    fn default_options() -> NormalizeOptions {
        NormalizeOptions::default()
    }

    fn assert_only_class_map_header_replace(edits: &[Edit]) {
        assert_eq!(
            edits.len(),
            1,
            "only the header change should surface: {edits:?}"
        );
        match &edits[0] {
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                assert_eq!(old_lines.len(), 1);
                assert_eq!(new_lines.len(), 1);
                assert_eq!(old_lines[0].text, "class-map match-any VOICE");
                assert_eq!(new_lines[0].text, "class-map match-all VOICE");
            }
            other => panic!("expected a header Replace, got {other:?}"),
        }
    }

    #[test]
    fn compute_ops_identical_sequences() {
        let ops = compute_ops(&[1, 2, 3], &[1, 2, 3]).unwrap();
        assert_eq!(ops, vec![Op::Equal, Op::Equal, Op::Equal]);
    }

    #[test]
    fn compute_ops_empty_left_yields_all_inserts() {
        let ops = compute_ops(&[], &[1, 2]).unwrap();
        assert_eq!(ops, vec![Op::Insert, Op::Insert]);
    }

    #[test]
    fn compute_ops_empty_right_yields_all_deletes() {
        let ops = compute_ops(&[1, 2], &[]).unwrap();
        assert_eq!(ops, vec![Op::Delete, Op::Delete]);
    }

    #[test]
    fn compute_ops_both_empty() {
        let ops = compute_ops(&[], &[]).unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn compute_ops_single_insertion() {
        let ops = compute_ops(&[1, 3], &[1, 2, 3]).unwrap();
        let inserts = ops.iter().filter(|o| **o == Op::Insert).count();
        let equals = ops.iter().filter(|o| **o == Op::Equal).count();
        assert_eq!(inserts, 1);
        assert_eq!(equals, 2);
    }

    #[test]
    fn compute_ops_single_deletion() {
        let ops = compute_ops(&[1, 2, 3], &[1, 3]).unwrap();
        let deletes = ops.iter().filter(|o| **o == Op::Delete).count();
        let equals = ops.iter().filter(|o| **o == Op::Equal).count();
        assert_eq!(deletes, 1);
        assert_eq!(equals, 2);
    }

    #[test]
    fn compute_ops_complete_replacement() {
        let ops = compute_ops(&[1, 2], &[3, 4]).unwrap();
        let deletes = ops.iter().filter(|o| **o == Op::Delete).count();
        let inserts = ops.iter().filter(|o| **o == Op::Insert).count();
        assert_eq!(deletes, 2);
        assert_eq!(inserts, 2);
    }

    #[test]
    fn compute_ops_preserves_length_invariant() {
        let a = [10, 20, 30, 40];
        let b = [10, 25, 30, 50];
        let ops = compute_ops(&a, &b).unwrap();
        let mut ai = 0usize;
        let mut bi = 0usize;
        for op in &ops {
            match op {
                Op::Equal => {
                    ai += 1;
                    bi += 1;
                }
                Op::Delete => ai += 1,
                Op::Insert => bi += 1,
            }
        }
        assert_eq!(ai, a.len());
        assert_eq!(bi, b.len());
    }

    #[test]
    fn build_stats_empty_edits() {
        let stats = build_stats(&[]);
        assert_eq!(stats, DiffStats::default());
    }

    #[test]
    fn build_stats_counts_inserts() {
        let edits = vec![Edit::Insert {
            at_key: Some(1),
            left_anchor: None,
            right_anchor: None,
            lines: vec![
                DiffLine {
                    content_key: 1,
                    occurrence_key: 1,
                    text: "a".into(),
                    path: Path(vec![0]),
                    span: span(0),
                },
                DiffLine {
                    content_key: 2,
                    occurrence_key: 2,
                    text: "b".into(),
                    path: Path(vec![1]),
                    span: span(1),
                },
            ],
        }];
        let stats = build_stats(&edits);
        assert_eq!(stats.inserts, 1);
        assert_eq!(stats.inserted_lines, 2);
        assert_eq!(stats.deletes, 0);
    }

    #[test]
    fn build_stats_counts_deletes() {
        let edits = vec![Edit::Delete {
            at_key: Some(1),
            left_anchor: None,
            right_anchor: None,
            lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "x".into(),
                path: Path(vec![0]),
                span: span(0),
            }],
        }];
        let stats = build_stats(&edits);
        assert_eq!(stats.deletes, 1);
        assert_eq!(stats.deleted_lines, 1);
    }

    #[test]
    fn build_stats_counts_replaces() {
        let edits = vec![Edit::Replace {
            old_at_key: Some(1),
            new_at_key: Some(2),
            left_anchor: None,
            right_anchor: None,
            old_lines: vec![DiffLine {
                content_key: 1,
                occurrence_key: 1,
                text: "old".into(),
                path: Path(vec![0]),
                span: span(0),
            }],
            new_lines: vec![
                DiffLine {
                    content_key: 2,
                    occurrence_key: 2,
                    text: "new1".into(),
                    path: Path(vec![0]),
                    span: span(0),
                },
                DiffLine {
                    content_key: 3,
                    occurrence_key: 3,
                    text: "new2".into(),
                    path: Path(vec![1]),
                    span: span(1),
                },
            ],
        }];
        let stats = build_stats(&edits);
        assert_eq!(stats.replaces, 1);
        assert_eq!(stats.replaced_old_lines, 1);
        assert_eq!(stats.replaced_new_lines, 2);
    }

    #[test]
    fn build_stats_accumulates_mixed_edits() {
        let edits = vec![
            Edit::Insert {
                at_key: Some(1),
                left_anchor: None,
                right_anchor: None,
                lines: vec![DiffLine {
                    content_key: 1,
                    occurrence_key: 1,
                    text: "i".into(),
                    path: Path(vec![0]),
                    span: span(0),
                }],
            },
            Edit::Delete {
                at_key: Some(2),
                left_anchor: None,
                right_anchor: None,
                lines: vec![DiffLine {
                    content_key: 2,
                    occurrence_key: 2,
                    text: "d".into(),
                    path: Path(vec![1]),
                    span: span(1),
                }],
            },
            Edit::Insert {
                at_key: Some(3),
                left_anchor: None,
                right_anchor: None,
                lines: vec![DiffLine {
                    content_key: 3,
                    occurrence_key: 3,
                    text: "i2".into(),
                    path: Path(vec![2]),
                    span: span(2),
                }],
            },
        ];
        let stats = build_stats(&edits);
        assert_eq!(stats.inserts, 2);
        assert_eq!(stats.deletes, 1);
        assert_eq!(stats.inserted_lines, 2);
        assert_eq!(stats.deleted_lines, 1);
    }

    #[test]
    fn build_segments_empty_view() {
        let v = view(vec![]);
        let segs = segment_at(&v.lines, 0);
        assert!(segs.is_empty());
    }

    #[test]
    fn build_segments_single_line() {
        let v = view(vec![cline("hostname router1", 100, vec![0])]);
        let segs = segment_at(&v.lines, 0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].lines.len(), 1);
        assert!(!segs[0].is_block);
    }

    #[test]
    fn build_segments_groups_same_root() {
        let v = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description foo", 101, vec![0, 0]),
            cline("  mtu 9000", 102, vec![0, 1]),
        ]);
        let segs = segment_at(&v.lines, 0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].lines.len(), 3);
        assert!(segs[0].is_block);
    }

    #[test]
    fn build_segments_splits_different_roots() {
        let v = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description foo", 101, vec![0, 0]),
            cline("interface Eth2", 200, vec![1]),
            cline("  description bar", 201, vec![1, 0]),
        ]);
        let segs = segment_at(&v.lines, 0);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].segment_key, 100);
        assert_eq!(segs[1].segment_key, 200);
    }

    #[test]
    fn build_segments_flat_lines_are_not_blocks() {
        let v = view(vec![
            cline("hostname a", 100, vec![0]),
            cline("hostname b", 200, vec![1]),
        ]);
        let segs = segment_at(&v.lines, 0);
        assert_eq!(segs.len(), 2);
        assert!(!segs[0].is_block);
        assert!(!segs[1].is_block);
    }

    #[test]
    fn diff_views_identical_flat_lines() {
        let a = view(vec![
            cline("hostname router1", 100, vec![0]),
            cline("ip route 0.0.0.0/0 10.0.0.1", 200, vec![1]),
        ]);
        let b = a.clone();
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(result.edits.is_empty());
    }

    #[test]
    fn diff_views_detects_flat_insertion() {
        let a = view(vec![cline("hostname router1", 100, vec![0])]);
        let b = view(vec![
            cline("hostname router1", 100, vec![0]),
            cline("ip route default", 200, vec![1]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(!result.edits.is_empty());
        assert!(
            result
                .edits
                .iter()
                .any(|e| matches!(e, Edit::Insert { .. }))
        );
    }

    #[test]
    fn diff_views_detects_flat_deletion() {
        let a = view(vec![
            cline("hostname router1", 100, vec![0]),
            cline("ip route default", 200, vec![1]),
        ]);
        let b = view(vec![cline("hostname router1", 100, vec![0])]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(!result.edits.is_empty());
        assert!(
            result
                .edits
                .iter()
                .any(|e| matches!(e, Edit::Delete { .. }))
        );
    }

    #[test]
    fn diff_views_both_empty() {
        let a = view(vec![]);
        let b = view(vec![]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(result.edits.is_empty());
    }

    #[test]
    fn diff_views_block_children_diffed_when_headers_match() {
        let a = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description old", 101, vec![0, 0]),
            cline("  mtu 9000", 102, vec![0, 1]),
        ]);
        let b = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description new", 201, vec![0, 0]),
            cline("  mtu 9000", 102, vec![0, 1]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(!result.edits.is_empty());
        match &result.edits[0] {
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                assert_eq!(old_lines[0].text, "  description old");
                assert_eq!(new_lines[0].text, "  description new");
            }
            _ => panic!("expected Replace"),
        }
    }

    #[test]
    fn diff_views_block_header_change_emitted_when_keys_collide() {
        // colliding headers (same key, different text): the change surfaces as
        // a Replace before the child edits.
        let a = view(vec![
            cline("class-map match-any VOICE", 100, vec![0]),
            cline("  match dscp ef", 101, vec![0, 0]),
        ]);
        let b = view(vec![
            cline("class-map match-all VOICE", 100, vec![0]),
            cline("  match dscp af31", 201, vec![0, 0]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(result.edits.len() >= 2, "expected header + child edits");

        match &result.edits[0] {
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                assert_eq!(old_lines.len(), 1);
                assert_eq!(new_lines.len(), 1);
                assert_eq!(old_lines[0].text, "class-map match-any VOICE");
                assert_eq!(new_lines[0].text, "class-map match-all VOICE");
            }
            other => panic!("expected header Replace first, got {other:?}"),
        }

        // the child change follows the header edit.
        let child_texts: Vec<&str> = result.edits[1..]
            .iter()
            .flat_map(|e| match e {
                Edit::Replace {
                    old_lines,
                    new_lines,
                    ..
                } => old_lines
                    .iter()
                    .chain(new_lines.iter())
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>(),
                Edit::Insert { lines, .. } | Edit::Delete { lines, .. } => {
                    lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>()
                }
            })
            .collect();
        assert!(child_texts.contains(&"  match dscp ef"));
        assert!(child_texts.contains(&"  match dscp af31"));
    }

    #[test]
    fn diff_views_block_header_unchanged_emits_no_header_replace() {
        // identical headers with a changed child: only the child edit, no
        // spurious header Replace.
        let a = view(vec![
            cline("class-map match-any VOICE", 100, vec![0]),
            cline("  match dscp ef", 101, vec![0, 0]),
        ]);
        let b = view(vec![
            cline("class-map match-any VOICE", 100, vec![0]),
            cline("  match dscp af31", 201, vec![0, 0]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        let header_edits = result
            .edits
            .iter()
            .filter(|e| match e {
                Edit::Replace {
                    old_lines,
                    new_lines,
                    ..
                } => old_lines
                    .iter()
                    .chain(new_lines.iter())
                    .any(|l| l.text == "class-map match-any VOICE"),
                Edit::Insert { lines, .. } | Edit::Delete { lines, .. } => {
                    lines.iter().any(|l| l.text == "class-map match-any VOICE")
                }
            })
            .count();
        assert_eq!(header_edits, 0, "unchanged header must not be reported");
    }

    #[test]
    fn colliding_block_header_change_surfaces_whether_top_level_or_nested() {
        // a top-level colliding-key header change already surfaces (#97).
        let a_top = view(vec![
            cline("class-map match-any VOICE", 200, vec![0]),
            cline("  match dscp ef", 300, vec![0, 0]),
        ]);
        let b_top = view(vec![
            cline("class-map match-all VOICE", 200, vec![0]),
            cline("  match dscp ef", 300, vec![0, 0]),
        ]);
        let top = diff_views(&a_top, &b_top, &default_options()).unwrap();
        assert_only_class_map_header_replace(&top.edits);

        // the same collision nested inside a matched block now surfaces too.
        let a_nested = view(vec![
            cline("policy-map PM", 100, vec![0]),
            cline("class-map match-any VOICE", 200, vec![0, 0]),
            cline("  match dscp ef", 300, vec![0, 0, 0]),
        ]);
        let b_nested = view(vec![
            cline("policy-map PM", 100, vec![0]),
            cline("class-map match-all VOICE", 200, vec![0, 0]),
            cline("  match dscp ef", 300, vec![0, 0, 0]),
        ]);
        let nested = diff_views(&a_nested, &b_nested, &default_options()).unwrap();
        assert_only_class_map_header_replace(&nested.edits);
    }

    #[test]
    fn deep_order_policy_override_now_honored_like_a_shallow_one() {
        // a pure reorder of two children of the block at [0].
        let shallow_a = view(vec![
            cline("block A", 100, vec![0]),
            cline("  child one", 301, vec![0, 0]),
            cline("  child two", 302, vec![0, 1]),
        ]);
        let shallow_b = view(vec![
            cline("block A", 100, vec![0]),
            cline("  child two", 302, vec![0, 0]),
            cline("  child one", 301, vec![0, 1]),
        ]);

        // the same reorder, one level deeper — under the nested block at [0, 0].
        let deep_a = view(vec![
            cline("block A", 100, vec![0]),
            cline("  block B", 200, vec![0, 0]),
            cline("    child one", 301, vec![0, 0, 0]),
            cline("    child two", 302, vec![0, 0, 1]),
        ]);
        let deep_b = view(vec![
            cline("block A", 100, vec![0]),
            cline("  block B", 200, vec![0, 0]),
            cline("    child two", 302, vec![0, 0, 0]),
            cline("    child one", 301, vec![0, 0, 1]),
        ]);

        let unordered_at = |prefix: Vec<usize>| {
            NormalizeOptions::default().with_order_policy(OrderPolicyConfig {
                default: OrderPolicy::Ordered,
                overrides: vec![OrderPolicyOverride {
                    context_prefix: prefix,
                    policy: OrderPolicy::Unordered,
                }],
            })
        };

        // both reorders are real changes under the default ordered policy.
        assert!(
            !diff_views(&shallow_a, &shallow_b, &default_options())
                .unwrap()
                .edits
                .is_empty()
        );
        assert!(
            !diff_views(&deep_a, &deep_b, &default_options())
                .unwrap()
                .edits
                .is_empty()
        );

        // a shallow [0] override already suppressed the shallow reorder.
        let shallow = diff_views(&shallow_a, &shallow_b, &unordered_at(vec![0])).unwrap();
        assert!(
            shallow.edits.is_empty(),
            "shallow override should suppress the reorder: {:?}",
            shallow.edits
        );

        // a deep [0, 0] override now suppresses the deep reorder.
        let deep = diff_views(&deep_a, &deep_b, &unordered_at(vec![0, 0])).unwrap();
        assert!(
            deep.edits.is_empty(),
            "deep override should suppress the reorder: {:?}",
            deep.edits
        );
    }

    #[test]
    fn diff_views_segment_replacement_uses_fallback() {
        let a = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description a", 101, vec![0, 0]),
        ]);
        let b = view(vec![
            cline("router bgp 65000", 200, vec![0]),
            cline("  neighbor 10.0.0.1", 201, vec![0, 0]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(!result.edits.is_empty());
        assert!(!result.fallback_contexts.is_empty());
    }

    #[test]
    fn fallback_delete_only_emits_deletes() {
        // Side A has a block; side B is empty.  All segments are deleted,
        // triggering the fallback with only deletions.
        let a = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description a", 101, vec![0, 0]),
        ]);
        let b = view(vec![]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(!result.edits.is_empty());
        assert!(
            result
                .edits
                .iter()
                .all(|e| matches!(e, Edit::Delete { .. })),
            "delete-only fallback should produce only Delete edits"
        );
        assert!(
            !result.fallback_contexts.is_empty(),
            "delete-only fallback should record a fallback context"
        );
    }

    #[test]
    fn fallback_insert_only_emits_inserts() {
        // Side A is empty; side B has a block.  All segments are inserted,
        // triggering the fallback with only insertions.
        let a = view(vec![]);
        let b = view(vec![
            cline("router bgp 65000", 200, vec![0]),
            cline("  neighbor 10.0.0.1", 201, vec![0, 0]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(!result.edits.is_empty());
        assert!(
            result
                .edits
                .iter()
                .all(|e| matches!(e, Edit::Insert { .. })),
            "insert-only fallback should produce only Insert edits"
        );
        assert!(
            !result.fallback_contexts.is_empty(),
            "insert-only fallback should record a fallback context"
        );
    }

    #[test]
    fn fallback_multiple_segments_flushed_together() {
        // Both sides have multiple unrelated segments — none of the segment
        // keys match, so all segments accumulate as pending and are flushed
        // together at the end via the fallback path.
        let a = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description a", 101, vec![0, 0]),
            cline("interface Eth2", 300, vec![1]),
            cline("  description b", 301, vec![1, 0]),
        ]);
        let b = view(vec![
            cline("router bgp 65000", 200, vec![0]),
            cline("  neighbor 10.0.0.1", 201, vec![0, 0]),
            cline("router ospf 1", 400, vec![1]),
            cline("  network 10.0.0.0/24", 401, vec![1, 0]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(!result.edits.is_empty());
        assert!(
            !result.fallback_contexts.is_empty(),
            "multi-segment replacement should use fallback"
        );
        // The fallback flattens all pending segments into lines before
        // diffing, so we should see individual line-level edits.
        let total_edit_lines: usize = result
            .edits
            .iter()
            .map(|e| match e {
                Edit::Delete { lines, .. } => lines.len(),
                Edit::Insert { lines, .. } => lines.len(),
                Edit::Replace {
                    old_lines,
                    new_lines,
                    ..
                } => old_lines.len() + new_lines.len(),
            })
            .sum();
        assert!(
            total_edit_lines > 0,
            "fallback should produce line-level edits"
        );
    }

    #[test]
    fn fallback_flushed_before_equal_segment() {
        // A has [block-X, block-Y], B has [block-Z, block-Y].
        // block-Z is unrelated to block-X (different keys) so X→Delete and
        // Z→Insert accumulate.  When block-Y matches (Equal), the pending
        // segments must be flushed via fallback before the Equal is processed.
        let a = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description a", 101, vec![0, 0]),
            cline("hostname router1", 500, vec![1]),
        ]);
        let b = view(vec![
            cline("router bgp 65000", 200, vec![0]),
            cline("  neighbor 10.0.0.1", 201, vec![0, 0]),
            cline("hostname router1", 500, vec![1]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(
            !result.fallback_contexts.is_empty(),
            "pending segments before Equal should be flushed via fallback"
        );
        // The shared segment (hostname) should not appear in edits.
        let all_edit_texts: Vec<&str> = result
            .edits
            .iter()
            .flat_map(|e| match e {
                Edit::Delete { lines, .. } => {
                    lines.iter().map(|l| l.text.as_str()).collect::<Vec<&str>>()
                }
                Edit::Insert { lines, .. } => {
                    lines.iter().map(|l| l.text.as_str()).collect::<Vec<&str>>()
                }
                Edit::Replace {
                    old_lines,
                    new_lines,
                    ..
                } => old_lines
                    .iter()
                    .chain(new_lines.iter())
                    .map(|l| l.text.as_str())
                    .collect::<Vec<&str>>(),
            })
            .collect();
        assert!(
            !all_edit_texts.contains(&"hostname router1"),
            "shared segment should not appear in edits"
        );
    }

    #[test]
    fn fallback_not_triggered_when_segments_match() {
        // When all segments match, no fallback should be invoked.
        let a = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description old", 101, vec![0, 0]),
        ]);
        let b = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description new", 201, vec![0, 0]),
        ]);
        let result = diff_views(&a, &b, &default_options()).unwrap();
        assert!(
            result.fallback_contexts.is_empty(),
            "matching segment headers should use child diffing, not fallback"
        );
    }

    #[test]
    fn line_diff_ordered_no_changes() {
        let lines = vec![
            cline("  description uplink", 10, vec![0, 0]),
            cline("  mtu 9000", 20, vec![0, 1]),
        ];
        let edits = line_diff(&lines, &lines, OrderPolicy::Ordered).unwrap();
        assert!(edits.is_empty());
    }

    #[test]
    fn line_diff_ordered_detects_replacement() {
        let a = vec![cline("  description old", 10, vec![0, 0])];
        let b = vec![cline("  description new", 20, vec![0, 0])];
        let edits = line_diff(&a, &b, OrderPolicy::Ordered).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Replace { .. }));
    }

    #[test]
    fn line_diff_ordered_insertion_at_end() {
        let a = vec![cline("  mtu 9000", 10, vec![0, 0])];
        let b = vec![
            cline("  mtu 9000", 10, vec![0, 0]),
            cline("  no shutdown", 20, vec![0, 1]),
        ];
        let edits = line_diff(&a, &b, OrderPolicy::Ordered).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Insert { .. }));
    }

    #[test]
    fn line_diff_ordered_deletion() {
        let a = vec![
            cline("  mtu 9000", 10, vec![0, 0]),
            cline("  no shutdown", 20, vec![0, 1]),
        ];
        let b = vec![cline("  mtu 9000", 10, vec![0, 0])];
        let edits = line_diff(&a, &b, OrderPolicy::Ordered).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Delete { .. }));
    }

    #[test]
    fn line_diff_ordered_reorder_is_a_change() {
        let a = vec![
            cline("  description uplink", 10, vec![0, 0]),
            cline("  mtu 9000", 20, vec![0, 1]),
        ];
        let b = vec![
            cline("  mtu 9000", 20, vec![0, 0]),
            cline("  description uplink", 10, vec![0, 1]),
        ];
        let edits = line_diff(&a, &b, OrderPolicy::Ordered).unwrap();
        assert!(!edits.is_empty());
    }

    #[test]
    fn line_diff_unordered_ignores_reorder() {
        let a = vec![
            cline("  description uplink", 10, vec![0, 0]),
            cline("  mtu 9000", 20, vec![0, 1]),
        ];
        let b = vec![
            cline("  mtu 9000", 20, vec![0, 0]),
            cline("  description uplink", 10, vec![0, 1]),
        ];
        let edits = line_diff(&a, &b, OrderPolicy::Unordered).unwrap();
        assert!(edits.is_empty());
    }

    #[test]
    fn line_diff_unordered_detects_extra_line() {
        let a = vec![cline("  mtu 9000", 10, vec![0, 0])];
        let b = vec![
            cline("  mtu 9000", 10, vec![0, 0]),
            cline("  no shutdown", 20, vec![0, 1]),
        ];
        let edits = line_diff(&a, &b, OrderPolicy::Unordered).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Insert { .. }));
    }

    #[test]
    fn line_diff_unordered_detects_removed_line() {
        let a = vec![
            cline("  mtu 9000", 10, vec![0, 0]),
            cline("  no shutdown", 20, vec![0, 1]),
        ];
        let b = vec![cline("  mtu 9000", 10, vec![0, 0])];
        let edits = line_diff(&a, &b, OrderPolicy::Unordered).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Delete { .. }));
    }

    #[test]
    fn line_diff_keyed_stable_ignores_reorder() {
        let a = vec![
            cline("  description uplink", 10, vec![0, 0]),
            cline("  mtu 9000", 20, vec![0, 1]),
        ];
        let b = vec![
            cline("  mtu 9000", 20, vec![0, 0]),
            cline("  description uplink", 10, vec![0, 1]),
        ];
        let edits = line_diff(&a, &b, OrderPolicy::KeyedStable).unwrap();
        assert!(edits.is_empty());
    }

    #[test]
    fn line_diff_keyed_stable_detects_content_change() {
        let a = vec![cline("  description old", 10, vec![0, 0])];
        let b = vec![cline("  description new", 20, vec![0, 0])];
        let edits = line_diff(&a, &b, OrderPolicy::KeyedStable).unwrap();
        assert!(!edits.is_empty());
    }

    #[test]
    fn line_diff_keyed_stable_emits_per_key_edits() {
        let a = vec![
            cline("  set allowaccess ping", 10, vec![0, 0]),
            cline("  set hostname old", 20, vec![0, 1]),
        ];
        let b = vec![
            cline("  set allowaccess https", 10, vec![0, 0]),
            cline("  set hostname new", 20, vec![0, 1]),
        ];
        let edits = line_diff(&a, &b, OrderPolicy::KeyedStable).unwrap();
        assert_eq!(edits.len(), 2);
        for edit in &edits {
            match edit {
                Edit::Replace {
                    old_lines,
                    new_lines,
                    ..
                } => {
                    assert_eq!(old_lines.len(), 1);
                    assert_eq!(new_lines.len(), 1);
                }
                _ => panic!("expected per-key Replace edits"),
            }
        }
    }

    // each test runs the same input through all three policies and asserts the
    // different outcomes side by side.  These complement the per-policy tests
    // above by showing *when* the policies diverge on identical data.

    #[test]
    fn same_content_key_different_text_diverges_across_all_three_policies() {
        // two lines whose content_key matches (simulating dialect key_hints like
        // FortiOS `set hostname`) but whose normalized text differs.
        //
        // Ordered sees matching content_keys → SES emits Equal → 0 edits.
        // Unordered hashes normalized text → distinct buckets → Delete + Insert.
        // KeyedStable buckets by content_key → paired → text differs → Replace.
        let a = vec![
            cline("  set hostname old", 10, vec![0, 0]),
            cline("  set allowaccess ping", 20, vec![0, 1]),
        ];
        let b = vec![
            cline("  set hostname new", 10, vec![0, 0]),
            cline("  set allowaccess https", 20, vec![0, 1]),
        ];

        // Ordered: content_keys [10, 20] == [10, 20] → all Equal, no edits.
        let ordered = line_diff(&a, &b, OrderPolicy::Ordered).unwrap();
        assert!(
            ordered.is_empty(),
            "Ordered matches by content_key; identical key sequences produce no edits"
        );

        // Unordered: four distinct text hashes → 2 Delete + 2 Insert.
        let unordered = line_diff(&a, &b, OrderPolicy::Unordered).unwrap();
        assert_eq!(unordered.len(), 4);
        assert_eq!(
            unordered
                .iter()
                .filter(|e| matches!(e, Edit::Delete { .. }))
                .count(),
            2
        );
        assert_eq!(
            unordered
                .iter()
                .filter(|e| matches!(e, Edit::Insert { .. }))
                .count(),
            2
        );

        // KeyedStable: two content_key buckets, each with a text mismatch → 2 Replace.
        let keyed = line_diff(&a, &b, OrderPolicy::KeyedStable).unwrap();
        assert_eq!(keyed.len(), 2);
        for edit in &keyed {
            assert!(
                matches!(edit, Edit::Replace { .. }),
                "KeyedStable pairs by content_key and detects text change as Replace"
            );
        }
    }

    #[test]
    fn reorder_plus_value_change_diverges_across_all_three_policies() {
        // lines are reordered AND one line's text changes (same content_key).
        //
        // Ordered sees key sequence [10, 20] vs [20, 10] → reorder edits.
        // Unordered ignores order, only sees the text change → Delete + Insert.
        // KeyedStable ignores order, pairs by key, detects text diff → Replace.
        let a = vec![
            cline("  set hostname old", 10, vec![0, 0]),
            cline("  set allowaccess ping", 20, vec![0, 1]),
        ];
        let b = vec![
            cline("  set allowaccess ping", 20, vec![0, 0]),
            cline("  set hostname new", 10, vec![0, 1]),
        ];

        // Ordered: reordering detected — at least one edit.
        let ordered = line_diff(&a, &b, OrderPolicy::Ordered).unwrap();
        assert!(!ordered.is_empty(), "Ordered treats reordering as a change");

        // Unordered: "set allowaccess ping" matches by text hash.
        // "set hostname old" → Delete, "set hostname new" → Insert.
        let unordered = line_diff(&a, &b, OrderPolicy::Unordered).unwrap();
        assert_eq!(unordered.len(), 2);
        assert!(
            unordered.iter().any(|e| matches!(e, Edit::Delete { .. })),
            "Unordered emits Delete for the removed text"
        );
        assert!(
            unordered.iter().any(|e| matches!(e, Edit::Insert { .. })),
            "Unordered emits Insert for the new text"
        );

        // KeyedStable: "set allowaccess ping" matches by content_key 20.
        // content_key 10 paired, text differs → single Replace.
        let keyed = line_diff(&a, &b, OrderPolicy::KeyedStable).unwrap();
        assert_eq!(keyed.len(), 1);
        assert!(
            matches!(keyed[0], Edit::Replace { .. }),
            "KeyedStable pairs the changed line by key and emits Replace"
        );
    }

    #[test]
    fn pure_reorder_only_ordered_reports_change() {
        // identical content in different order.  Ordered is the only policy
        // that treats this as a change.
        let a = vec![
            cline("  description uplink", 10, vec![0, 0]),
            cline("  mtu 9000", 20, vec![0, 1]),
            cline("  no shutdown", 30, vec![0, 2]),
        ];
        let b = vec![
            cline("  no shutdown", 30, vec![0, 0]),
            cline("  description uplink", 10, vec![0, 1]),
            cline("  mtu 9000", 20, vec![0, 2]),
        ];

        let ordered = line_diff(&a, &b, OrderPolicy::Ordered).unwrap();
        let unordered = line_diff(&a, &b, OrderPolicy::Unordered).unwrap();
        let keyed = line_diff(&a, &b, OrderPolicy::KeyedStable).unwrap();

        assert!(!ordered.is_empty(), "Ordered detects the reorder");
        assert!(unordered.is_empty(), "Unordered ignores order");
        assert!(keyed.is_empty(), "KeyedStable ignores order");
    }

    #[test]
    fn finalize_chunked_edits_both_empty() {
        let result = finalize_chunked_edits(vec![], vec![]);
        assert!(result.is_empty());
    }

    #[test]
    fn finalize_chunked_edits_only_deletes() {
        let deletes = vec![DiffLine {
            content_key: 1,
            occurrence_key: 1,
            text: "removed".into(),
            path: Path(vec![0]),
            span: span(0),
        }];
        let result = finalize_chunked_edits(deletes, vec![]);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Edit::Delete { .. }));
    }

    #[test]
    fn finalize_chunked_edits_only_inserts() {
        let inserts = vec![DiffLine {
            content_key: 1,
            occurrence_key: 1,
            text: "added".into(),
            path: Path(vec![0]),
            span: span(0),
        }];
        let result = finalize_chunked_edits(vec![], inserts);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Edit::Insert { .. }));
    }

    #[test]
    fn finalize_chunked_edits_both_present_yields_replace() {
        let deletes = vec![DiffLine {
            content_key: 1,
            occurrence_key: 1,
            text: "old".into(),
            path: Path(vec![0]),
            span: span(0),
        }];
        let inserts = vec![DiffLine {
            content_key: 2,
            occurrence_key: 2,
            text: "new".into(),
            path: Path(vec![0]),
            span: span(0),
        }];
        let result = finalize_chunked_edits(deletes, inserts);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], Edit::Replace { .. }));
    }

    #[test]
    fn finalize_edit_both_empty_is_none() {
        assert!(finalize_edit(vec![], vec![]).is_none());
    }

    #[test]
    fn finalize_edit_preserves_input_order() {
        // the ordered path passes lines already in diff order, so the shared
        // helper must report them as given rather than sorting.
        let deletes = vec![dline("second", 20, 1), dline("first", 10, 0)];
        let inserts = vec![dline("fourth", 40, 3), dline("third", 30, 2)];

        let Some(Edit::Replace {
            old_lines,
            new_lines,
            left_anchor,
            right_anchor,
            ..
        }) = finalize_edit(deletes, inserts)
        else {
            panic!("both sides non-empty yields Replace");
        };

        assert_eq!(old_lines[0].text, "second");
        assert_eq!(new_lines[0].text, "fourth");
        assert_eq!(left_anchor.unwrap().span, span(1));
        assert_eq!(right_anchor.unwrap().span, span(3));
    }

    #[test]
    fn finalize_chunked_edits_sorts_its_own_input() {
        let deletes = vec![dline("second", 20, 1), dline("first", 10, 0)];
        let result = finalize_chunked_edits(deletes, vec![]);

        let [
            Edit::Delete {
                lines, left_anchor, ..
            },
        ] = &result[..]
        else {
            panic!("deletes only yields Delete");
        };

        assert_eq!(lines[0].text, "first");
        assert_eq!(left_anchor.clone().unwrap().span, span(0));
    }

    #[test]
    fn to_diff_line_preserves_fields() {
        let cl = cline("  mtu 9000", 42, vec![0, 1]);
        let dl = to_diff_line(&cl);
        assert_eq!(dl.content_key, cl.content_key);
        assert_eq!(dl.occurrence_key, cl.occurrence_key);
        assert_eq!(dl.text, cl.original);
        assert_eq!(dl.path, cl.path);
        assert_eq!(dl.span, cl.span);
    }

    #[test]
    fn to_anchor_extracts_path_and_span() {
        let dl = DiffLine {
            content_key: 1,
            occurrence_key: 1,
            text: "test".into(),
            path: Path(vec![2, 3]),
            span: Span {
                line: 5,
                start_byte: 10,
                end_byte: 20,
            },
        };
        let anchor = to_anchor(&dl);
        assert_eq!(anchor.path, dl.path);
        assert_eq!(anchor.span, dl.span);
    }
}
