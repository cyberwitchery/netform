//! where each right-side line goes in the left-hand document.

use std::collections::{HashMap, HashSet};

use netform_ir::Path;

use crate::model::{ComparisonView, DiffLine, Edit};
use crate::util::{parent_path, path_starts_with};

/// sets `after_path` on every line an edit adds directly under a block the left-hand
/// document also has.
///
/// a sibling that no edit touches is kept on both sides, and kept siblings pair up in
/// order, so each added line goes after the left-side counterpart of the nearest earlier
/// sibling that is kept or rewritten in place.
pub(crate) fn place_new_lines(edits: &mut [Edit], a: &ComparisonView, b: &ComparisonView) {
    let mut left_changed = HashSet::new();
    let mut right_changed = HashSet::new();
    let mut in_place = HashMap::new();
    for edit in edits.iter() {
        match edit {
            Edit::Insert { lines, .. } => right_changed.extend(paths(lines)),
            Edit::Delete { lines, .. } => left_changed.extend(paths(lines)),
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                left_changed.extend(paths(old_lines));
                right_changed.extend(paths(new_lines));
                in_place.extend(rewritten_in_place(old_lines, new_lines));
            }
        }
    }

    let placement = Placement {
        left: children(a),
        right: children(b),
        left_changed,
        right_changed,
        in_place,
    };
    let mut levels: HashMap<(Path, Path), HashMap<Path, Path>> = HashMap::new();
    for edit in edits.iter_mut() {
        let (context, lines) = match edit {
            Edit::Insert {
                left_anchor, lines, ..
            } => {
                let Some(first) = lines.first() else {
                    continue;
                };
                let context = if first.path.0.len() == 1 {
                    Path(Vec::new())
                } else if let Some(anchor) = left_anchor {
                    anchor.path.clone()
                } else {
                    continue;
                };
                (context, lines)
            }
            Edit::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                let Some(old) = old_lines.first() else {
                    continue;
                };
                (parent_path(&old.path), new_lines)
            }
            Edit::Delete { .. } => continue,
        };
        let Some(first) = lines.first() else {
            continue;
        };
        let level = parent_path(&first.path);
        let after = levels
            .entry((context.clone(), level.clone()))
            .or_insert_with(|| placement.after(&context, &level));
        for line in lines.iter_mut() {
            line.after_path = after.get(&line.path).cloned();
        }
    }
}

fn paths(lines: &[DiffLine]) -> impl Iterator<Item = Path> + '_ {
    lines.iter().map(|line| line.path.clone())
}

/// the right-side lines of a replace that take the place of a left-side line, by that line.
///
/// a one-line replace rewrites its line, and a replace within one left-side subtree
/// puts its top lines where that subtree was.
fn rewritten_in_place(old_lines: &[DiffLine], new_lines: &[DiffLine]) -> Vec<(Path, Path)> {
    let (Some(old), Some(new)) = (old_lines.first(), new_lines.first()) else {
        return Vec::new();
    };
    if old_lines.len() == 1 && new_lines.len() == 1 {
        return vec![(new.path.clone(), old.path.clone())];
    }
    if !old_lines
        .iter()
        .all(|line| path_starts_with(&line.path.0, &old.path.0))
    {
        return Vec::new();
    }
    let level = parent_path(&new.path);
    new_lines
        .iter()
        .filter(|line| parent_path(&line.path) == level)
        .map(|line| (line.path.clone(), old.path.clone()))
        .collect()
}

/// each line's children in document order, by the line's path.
fn children(view: &ComparisonView) -> HashMap<Path, Vec<&Path>> {
    let mut children: HashMap<Path, Vec<&Path>> = HashMap::new();
    for line in &view.lines {
        children
            .entry(parent_path(&line.path))
            .or_default()
            .push(&line.path);
    }
    children
}

struct Placement<'a> {
    left: HashMap<Path, Vec<&'a Path>>,
    right: HashMap<Path, Vec<&'a Path>>,
    left_changed: HashSet<Path>,
    right_changed: HashSet<Path>,
    in_place: HashMap<Path, Path>,
}

impl Placement<'_> {
    /// the left-side line each child of the right-side `level` comes after, for the
    /// left-side block `context` that pairs with it.
    fn after(&self, context: &Path, level: &Path) -> HashMap<Path, Path> {
        let none = Vec::new();
        let right = self.right.get(level).unwrap_or(&none);
        let mut kept_left = self
            .left
            .get(context)
            .unwrap_or(&none)
            .iter()
            .filter(|path| !self.left_changed.contains(**path));

        let mut after = HashMap::with_capacity(right.len());
        let mut previous = context;
        for &path in right {
            after.insert(path.clone(), previous.clone());
            let counterpart = if self.right_changed.contains(path) {
                self.in_place.get(path)
            } else {
                kept_left.next().copied()
            };
            if let Some(counterpart) = counterpart {
                previous = counterpart;
            }
        }
        after
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_documents;
    use crate::model::NormalizeOptions;
    use netform_ir::parse_generic;

    fn new_lines(left: &str, right: &str) -> Vec<(String, Option<Vec<usize>>)> {
        let diff = diff_documents(
            &parse_generic(left),
            &parse_generic(right),
            NormalizeOptions::default(),
        )
        .unwrap();
        diff.edits
            .iter()
            .flat_map(|edit| match edit {
                Edit::Insert { lines, .. } => lines.as_slice(),
                Edit::Replace { new_lines, .. } => new_lines.as_slice(),
                Edit::Delete { .. } => &[],
            })
            .map(|line| {
                (
                    line.text.trim().to_string(),
                    line.after_path.as_ref().map(|path| path.0.clone()),
                )
            })
            .collect()
    }

    #[test]
    fn an_added_line_goes_after_the_kept_line_before_it() {
        let left = "acl X\n a\n b\n";
        let cases = [
            ("acl X\n n\n a\n b\n", vec![0]),
            ("acl X\n a\n n\n b\n", vec![0, 0]),
            ("acl X\n a\n b\n n\n", vec![0, 1]),
        ];
        for (right, after) in cases {
            assert_eq!(
                new_lines(left, right),
                [("n".to_string(), Some(after))],
                "{right}"
            );
        }
    }

    #[test]
    fn a_rewritten_line_is_placed_after_its_predecessor() {
        assert_eq!(
            new_lines("acl X\n a\n b\n", "acl X\n a\n c\n"),
            [("c".to_string(), Some(vec![0, 0]))]
        );
    }

    #[test]
    fn deleted_siblings_are_skipped() {
        assert_eq!(
            new_lines("a\nb\nc\n", "a\nn\nc\n"),
            [("n".to_string(), Some(vec![0]))]
        );
        assert_eq!(
            new_lines("a\nb\nc\n", "c\nn\n"),
            [("n".to_string(), Some(vec![2]))]
        );
    }

    #[test]
    fn a_line_after_a_rewritten_one_follows_it() {
        assert_eq!(
            new_lines(
                "interface Gi1\n description a\n mtu 1500\n",
                "interface Gi1\n description b\n shutdown\n mtu 1500\n",
            ),
            [
                ("description b".to_string(), Some(vec![0])),
                ("shutdown".to_string(), Some(vec![0, 0])),
            ]
        );
    }
}
