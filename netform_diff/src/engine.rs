use std::collections::HashMap;

use crate::model::{
    ComparisonLine, ComparisonView, DiffLine, DiffStats, Edit, EditAnchor, NormalizeOptions,
    OrderPolicy,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
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
    let mut a_iter = a_segments.into_iter();
    let mut b_iter = b_segments.into_iter();
    let mut pending_deleted_segments: Vec<Segment> = Vec::new();
    let mut pending_inserted_segments: Vec<Segment> = Vec::new();

    let mut flush_segment_fallback =
        |edits: &mut Vec<Edit>, deleted: &mut Vec<Segment>, inserted: &mut Vec<Segment>| {
            if deleted.is_empty() && inserted.is_empty() {
                return;
            }

            let deleted_lines = deleted
                .drain(..)
                .flat_map(|segment| segment.lines)
                .collect::<Vec<_>>();
            let inserted_lines = inserted
                .drain(..)
                .flat_map(|segment| segment.lines)
                .collect::<Vec<_>>();

            let mut fallback = line_diff(
                &deleted_lines,
                &inserted_lines,
                options.policy_for_path(
                    &deleted_lines
                        .first()
                        .map(|line| line.path.clone())
                        .or_else(|| inserted_lines.first().map(|line| line.path.clone()))
                        .unwrap_or(netform_ir::Path(Vec::new())),
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

                let left = a_iter.next().unwrap();
                let right = b_iter.next().unwrap();
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
            }
            Op::Delete => {
                pending_deleted_segments.push(a_iter.next().unwrap());
            }
            Op::Insert => {
                pending_inserted_segments.push(b_iter.next().unwrap());
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

        left.sort_by_key(|line| (line.occurrence_key, line.path.0.clone()));
        right.sort_by_key(|line| (line.occurrence_key, line.path.0.clone()));

        let common = left.len().min(right.len());

        let mut bucket_deletes = Vec::new();
        let mut bucket_inserts = Vec::new();

        // Paired lines share a content key but may differ in text (e.g.
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

fn compute_ops(a: &[u64], b: &[u64]) -> Vec<Op> {
    if a.is_empty() {
        return vec![Op::Insert; b.len()];
    }
    if b.is_empty() {
        return vec![Op::Delete; a.len()];
    }

    let n = a.len() as isize;
    let m = b.len() as isize;
    let max = (a.len() + b.len()) as isize;
    let offset = max + 1;
    let v_len = (2 * max + 3) as usize;

    // Myers SES trace over diagonals. This avoids the quadratic LCS matrix and
    // remains deterministic for a fixed input/order.
    let mut v = vec![0isize; v_len];
    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=max {
        let mut current = v.clone();
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
            current[idx] = x;

            if x >= n && y >= m {
                trace.push(current);
                return backtrack_ops(a, b, &trace, offset);
            }
            k += 2;
        }
        trace.push(current.clone());
        v = current;
    }

    unreachable!("Myers SES must converge within n+m steps")
}

fn backtrack_ops(a: &[u64], b: &[u64], trace: &[Vec<isize>], offset: isize) -> Vec<Op> {
    let mut x = a.len() as isize;
    let mut y = b.len() as isize;
    let mut rev_ops = Vec::new();

    for d in (1..trace.len()).rev() {
        let d = d as isize;
        let k = x - y;
        let prev = &trace[(d - 1) as usize];
        let idx = (k + offset) as usize;
        let go_down = k == -d || (k != d && prev[idx - 1] < prev[idx + 1]);
        let prev_k = if go_down { k + 1 } else { k - 1 };
        let prev_x = prev[(prev_k + offset) as usize];
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

    fn view(lines: Vec<ComparisonLine>) -> ComparisonView {
        ComparisonView { lines }
    }

    fn default_options() -> NormalizeOptions {
        NormalizeOptions::default()
    }

    // ── compute_ops ──

    #[test]
    fn compute_ops_identical_sequences() {
        let ops = compute_ops(&[1, 2, 3], &[1, 2, 3]);
        assert_eq!(ops, vec![Op::Equal, Op::Equal, Op::Equal]);
    }

    #[test]
    fn compute_ops_empty_left_yields_all_inserts() {
        let ops = compute_ops(&[], &[1, 2]);
        assert_eq!(ops, vec![Op::Insert, Op::Insert]);
    }

    #[test]
    fn compute_ops_empty_right_yields_all_deletes() {
        let ops = compute_ops(&[1, 2], &[]);
        assert_eq!(ops, vec![Op::Delete, Op::Delete]);
    }

    #[test]
    fn compute_ops_both_empty() {
        let ops = compute_ops(&[], &[]);
        assert!(ops.is_empty());
    }

    #[test]
    fn compute_ops_single_insertion() {
        let ops = compute_ops(&[1, 3], &[1, 2, 3]);
        let inserts = ops.iter().filter(|o| **o == Op::Insert).count();
        let equals = ops.iter().filter(|o| **o == Op::Equal).count();
        assert_eq!(inserts, 1);
        assert_eq!(equals, 2);
    }

    #[test]
    fn compute_ops_single_deletion() {
        let ops = compute_ops(&[1, 2, 3], &[1, 3]);
        let deletes = ops.iter().filter(|o| **o == Op::Delete).count();
        let equals = ops.iter().filter(|o| **o == Op::Equal).count();
        assert_eq!(deletes, 1);
        assert_eq!(equals, 2);
    }

    #[test]
    fn compute_ops_complete_replacement() {
        let ops = compute_ops(&[1, 2], &[3, 4]);
        let deletes = ops.iter().filter(|o| **o == Op::Delete).count();
        let inserts = ops.iter().filter(|o| **o == Op::Insert).count();
        assert_eq!(deletes, 2);
        assert_eq!(inserts, 2);
    }

    #[test]
    fn compute_ops_preserves_length_invariant() {
        let a = [10, 20, 30, 40];
        let b = [10, 25, 30, 50];
        let ops = compute_ops(&a, &b);
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

    // ── build_stats ──

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

    // ── build_segments ──

    #[test]
    fn build_segments_empty_view() {
        let v = view(vec![]);
        let segs = build_segments(&v);
        assert!(segs.is_empty());
    }

    #[test]
    fn build_segments_single_line() {
        let v = view(vec![cline("hostname router1", 100, vec![0])]);
        let segs = build_segments(&v);
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
        let segs = build_segments(&v);
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
        let segs = build_segments(&v);
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
        let segs = build_segments(&v);
        assert_eq!(segs.len(), 2);
        assert!(!segs[0].is_block);
        assert!(!segs[1].is_block);
    }

    // ── diff_views ──

    #[test]
    fn diff_views_identical_flat_lines() {
        let a = view(vec![
            cline("hostname router1", 100, vec![0]),
            cline("ip route 0.0.0.0/0 10.0.0.1", 200, vec![1]),
        ]);
        let b = a.clone();
        let result = diff_views(&a, &b, &default_options());
        assert!(result.edits.is_empty());
    }

    #[test]
    fn diff_views_detects_flat_insertion() {
        let a = view(vec![cline("hostname router1", 100, vec![0])]);
        let b = view(vec![
            cline("hostname router1", 100, vec![0]),
            cline("ip route default", 200, vec![1]),
        ]);
        let result = diff_views(&a, &b, &default_options());
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
        let result = diff_views(&a, &b, &default_options());
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
        let result = diff_views(&a, &b, &default_options());
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
        let result = diff_views(&a, &b, &default_options());
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
    fn diff_views_segment_replacement_uses_fallback() {
        let a = view(vec![
            cline("interface Eth1", 100, vec![0]),
            cline("  description a", 101, vec![0, 0]),
        ]);
        let b = view(vec![
            cline("router bgp 65000", 200, vec![0]),
            cline("  neighbor 10.0.0.1", 201, vec![0, 0]),
        ]);
        let result = diff_views(&a, &b, &default_options());
        assert!(!result.edits.is_empty());
        assert!(!result.fallback_contexts.is_empty());
    }

    // ── line_diff (ordered) ──

    #[test]
    fn line_diff_ordered_no_changes() {
        let lines = vec![
            cline("  description uplink", 10, vec![0, 0]),
            cline("  mtu 9000", 20, vec![0, 1]),
        ];
        let edits = line_diff(&lines, &lines, OrderPolicy::Ordered);
        assert!(edits.is_empty());
    }

    #[test]
    fn line_diff_ordered_detects_replacement() {
        let a = vec![cline("  description old", 10, vec![0, 0])];
        let b = vec![cline("  description new", 20, vec![0, 0])];
        let edits = line_diff(&a, &b, OrderPolicy::Ordered);
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
        let edits = line_diff(&a, &b, OrderPolicy::Ordered);
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
        let edits = line_diff(&a, &b, OrderPolicy::Ordered);
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
        let edits = line_diff(&a, &b, OrderPolicy::Ordered);
        assert!(!edits.is_empty());
    }

    // ── line_diff (unordered) ──

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
        let edits = line_diff(&a, &b, OrderPolicy::Unordered);
        assert!(edits.is_empty());
    }

    #[test]
    fn line_diff_unordered_detects_extra_line() {
        let a = vec![cline("  mtu 9000", 10, vec![0, 0])];
        let b = vec![
            cline("  mtu 9000", 10, vec![0, 0]),
            cline("  no shutdown", 20, vec![0, 1]),
        ];
        let edits = line_diff(&a, &b, OrderPolicy::Unordered);
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
        let edits = line_diff(&a, &b, OrderPolicy::Unordered);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Delete { .. }));
    }

    // ── line_diff (keyed_stable) ──

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
        let edits = line_diff(&a, &b, OrderPolicy::KeyedStable);
        assert!(edits.is_empty());
    }

    #[test]
    fn line_diff_keyed_stable_detects_content_change() {
        let a = vec![cline("  description old", 10, vec![0, 0])];
        let b = vec![cline("  description new", 20, vec![0, 0])];
        let edits = line_diff(&a, &b, OrderPolicy::KeyedStable);
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
        let edits = line_diff(&a, &b, OrderPolicy::KeyedStable);
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

    // ── finalize_chunked_edits ──

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

    // ── to_diff_line / to_anchor ──

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
