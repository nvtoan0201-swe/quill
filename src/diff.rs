//! Word-level diff for Edit › Paste Edits From: apply only what changed,
//! so unchanged text keeps its author and new text can be attributed.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// One replacement in the old text: `old` range becomes `new` text.
#[derive(Debug, PartialEq)]
pub struct Hunk {
    pub old: Range<usize>,
    pub new: String,
}

fn tokens(s: &str) -> Vec<(usize, &str)> {
    s.split_word_bound_indices().collect()
}

/// Hunks turning `old` into `new`, in ascending order. Falls back to one
/// whole-text hunk when the inputs are too large for the quadratic table.
pub fn diff(old: &str, new: &str) -> Vec<Hunk> {
    let a = tokens(old);
    let b = tokens(new);
    if a.len() * b.len() > 25_000_000 {
        return if old == new { Vec::new() } else { vec![Hunk { old: 0..old.len(), new: new.to_string() }] };
    }
    // lcs[i][j] = LCS length of a[i..], b[j..]
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i].1 == b[j].1 {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }
    let old_pos = |i: usize| if i < n { a[i].0 } else { old.len() };
    let new_pos = |j: usize| if j < m { b[j].0 } else { new.len() };
    let mut hunks = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut open: Option<(usize, usize)> = None;
    while i < n || j < m {
        if i < n && j < m && a[i].1 == b[j].1 {
            if let Some((oi, oj)) = open.take() {
                hunks.push(Hunk { old: old_pos(oi)..old_pos(i), new: new[new_pos(oj)..new_pos(j)].to_string() });
            }
            i += 1;
            j += 1;
        } else {
            open.get_or_insert((i, j));
            if j < m && (i == n || lcs[at(i, j + 1)] >= lcs[at(i + 1, j)]) {
                j += 1;
            } else {
                i += 1;
            }
        }
    }
    if let Some((oi, oj)) = open {
        hunks.push(Hunk { old: old_pos(oi)..old.len(), new: new[new_pos(oj)..].to_string() });
    }
    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &str, hunks: &[Hunk]) -> String {
        let mut out = old.to_string();
        for h in hunks.iter().rev() {
            out.replace_range(h.old.clone(), &h.new);
        }
        out
    }

    #[test]
    fn minimal_word_hunks() {
        let old = "The cat sat on the mat.";
        let new = "The black cat sat on a mat.";
        let h = diff(old, new);
        assert_eq!(apply(old, &h), new);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].new, "black ");
        assert!(diff("same", "same").is_empty());
        assert_eq!(apply("", &diff("", "new text")), "new text");
    }
}
