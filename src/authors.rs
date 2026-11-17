//! Authorship: which byte ranges were written by an AI (everything else is the human's).
//! Ranges follow edits; they are stored next to the app data, not inside the document.

use std::ops::Range;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Author {
    Human,
    Ai,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Authors {
    /// Sorted, non-overlapping, non-adjacent AI ranges.
    pub ai: Vec<Range<usize>>,
}

impl Authors {
    /// Text in `old` was replaced by `new_len` bytes written by `author`.
    pub fn edit(&mut self, old: Range<usize>, new_len: usize, author: Author) {
        let delta = new_len as isize - old.len() as isize;
        let mut out = Vec::with_capacity(self.ai.len() + 1);
        for r in self.ai.drain(..) {
            if r.end <= old.start {
                out.push(r);
            } else if r.start >= old.end {
                let s = (r.start as isize + delta) as usize;
                let e = (r.end as isize + delta) as usize;
                out.push(s..e);
            } else {
                // Overlap: keep the parts outside the replaced span.
                if r.start < old.start {
                    out.push(r.start..old.start);
                }
                if r.end > old.end {
                    let e = (r.end as isize + delta) as usize;
                    out.push(old.start + new_len..e);
                }
            }
        }
        if author == Author::Ai && new_len > 0 {
            out.push(old.start..old.start + new_len);
        }
        self.ai = normalize(out);
    }

    /// Re-attribute an existing span without changing text.
    pub fn mark(&mut self, range: Range<usize>, author: Author) {
        self.edit(range.clone(), range.len(), author);
    }

    #[cfg(test)]
    pub fn author_at(&self, offset: usize) -> Author {
        if self.ai.iter().any(|r| r.contains(&offset)) {
            Author::Ai
        } else {
            Author::Human
        }
    }

    /// (human, ai) byte counts for the status bar.
    pub fn shares(&self, len: usize) -> (usize, usize) {
        let ai: usize = self.ai.iter().map(|r| r.len()).sum();
        (len.saturating_sub(ai), ai)
    }

    fn sidecar(doc: &Path) -> Option<PathBuf> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        doc.hash(&mut h);
        dirs::data_dir().map(|d| d.join("quill").join("authors").join(format!("{:016x}.json", h.finish())))
    }

    pub fn load(doc: &Path, text_len: usize) -> Self {
        let mut a: Self = Self::sidecar(doc)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        a.ai.retain(|r| r.end <= text_len);
        a
    }

    pub fn save(&self, doc: &Path) {
        let Some(p) = Self::sidecar(doc) else { return };
        if self.ai.is_empty() {
            let _ = std::fs::remove_file(p);
            return;
        }
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(p, json);
        }
    }
}

fn normalize(mut v: Vec<Range<usize>>) -> Vec<Range<usize>> {
    v.retain(|r| !r.is_empty());
    v.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(v.len());
    for r in v {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_shift_split_and_merge() {
        let mut a = Authors::default();
        a.edit(5..5, 10, Author::Ai); // AI pasted 10 bytes at 5
        assert_eq!(a.ai, vec![5..15]);
        a.edit(0..0, 3, Author::Human); // typing before shifts it
        assert_eq!(a.ai, vec![8..18]);
        a.edit(10..12, 1, Author::Human); // human edits inside splits it
        assert_eq!(a.ai, vec![8..10, 11..17]);
        a.mark(10..11, Author::Ai); // marking the gap merges again
        assert_eq!(a.ai, vec![8..17]);
        assert_eq!(a.author_at(9), Author::Ai);
        assert_eq!(a.author_at(17), Author::Human);
        assert_eq!(a.shares(20), (11, 9));
        a.mark(0..20, Author::Human);
        assert!(a.ai.is_empty());
    }
}
