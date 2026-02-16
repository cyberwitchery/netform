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

    Vec::new()
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
