use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

/// Text plus selection plus undo history. All offsets are UTF-8 byte offsets.
#[derive(Default)]
pub struct Buffer {
    text: String,
    anchor: usize,
    head: usize,
    /// IME composition range ("marked text").
    pub marked: Option<Range<usize>>,
    undo: Vec<Group>,
    redo: Vec<Group>,
    last_kind: Option<EditKind>,
    version: u64,
    saved_version: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    Typing,
    Delete,
    /// Never merged with neighbours (paste, formatting commands, …).
    Other,
}

#[derive(Clone, Debug)]
struct Edit {
    at: usize,
    deleted: String,
    inserted: String,
}

#[derive(Clone, Debug)]
struct Group {
    edits: Vec<Edit>,
    sel_before: (usize, usize),
    sel_after: (usize, usize),
}

impl Buffer {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }

    /// Replace the whole document (open/revert/reload). Clears history; the
    /// result counts as saved. The version keeps increasing so caches keyed on
    /// it are invalidated.
    pub fn reset(&mut self, text: impl Into<String>) {
        let version = self.version + 1;
        *self = Self::new(text);
        self.version = version;
        self.saved_version = version;
    }

    pub fn head(&self) -> usize {
        self.head
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_reversed(&self) -> bool {
        self.head < self.anchor
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = self.clamp(anchor);
        self.head = self.clamp(head);
        self.last_kind = None;
    }

    pub fn move_to(&mut self, offset: usize) {
        self.set_selection(offset, offset);
    }

    pub fn extend_to(&mut self, offset: usize) {
        self.set_selection(self.anchor, offset);
    }

    fn clamp(&self, offset: usize) -> usize {
        let mut o = offset.min(self.text.len());
        while !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    /// Core mutation. Puts the cursor after the inserted text.
    pub fn replace(&mut self, range: Range<usize>, new_text: &str, kind: EditKind) {
        let range = self.clamp(range.start)..self.clamp(range.end);
        let sel_before = (self.anchor, self.head);
        let edit = Edit {
            at: range.start,
            deleted: self.text[range.clone()].to_string(),
            inserted: new_text.to_string(),
        };
        self.text.replace_range(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.anchor = cursor;
        self.head = cursor;
        self.version += 1;
        self.redo.clear();

        let merge = kind != EditKind::Other
            && self.last_kind == Some(kind)
            && self.undo.last().is_some_and(|g| {
                let last = g.edits.last().unwrap();
                match kind {
                    // Contiguous typing, broken at word boundaries like most editors.
                    EditKind::Typing => {
                        last.at + last.inserted.len() == edit.at
                            && !(new_text.starts_with(char::is_whitespace)
                                && !last.inserted.ends_with(char::is_whitespace))
                    }
                    EditKind::Delete => {
                        edit.inserted.is_empty()
                            && (edit.at + edit.deleted.len() == last.at || edit.at == last.at)
                    }
                    EditKind::Other => false,
                }
            });
        if merge {
            let g = self.undo.last_mut().unwrap();
            g.edits.push(edit);
            g.sel_after = (cursor, cursor);
        } else {
            self.undo.push(Group {
                edits: vec![edit],
                sel_before,
                sel_after: (cursor, cursor),
            });
        }
        self.last_kind = Some(kind);
    }

    /// Several non-overlapping replacements (ascending) as one undo step.
    pub fn replace_many(&mut self, edits: &[(Range<usize>, String)]) {
        if edits.is_empty() {
            return;
        }
        let sel_before = (self.anchor, self.head);
        let mut group = Vec::new();
        for (r, t) in edits.iter().rev() {
            group.push(Edit { at: r.start, deleted: self.text[r.clone()].to_string(), inserted: t.clone() });
            self.text.replace_range(r.clone(), t);
        }
        // Caret after the last replacement, shifted by the edits before it.
        let (last_range, last_text) = edits.last().unwrap();
        let before: isize = edits[..edits.len() - 1].iter().map(|(r, t)| t.len() as isize - r.len() as isize).sum();
        let cursor = self.clamp((last_range.start as isize + before) as usize + last_text.len());
        self.anchor = cursor;
        self.head = cursor;
        self.version += 1;
        self.redo.clear();
        self.undo.push(Group { edits: group, sel_before, sel_after: (cursor, cursor) });
        self.last_kind = None;
    }

    /// Replace without touching history (IME composition updates).
    pub fn replace_untracked(&mut self, range: Range<usize>, new_text: &str) {
        let range = self.clamp(range.start)..self.clamp(range.end);
        self.text.replace_range(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.anchor = cursor;
        self.head = cursor;
        self.version += 1;
    }

    pub fn insert(&mut self, text: &str) {
        let kind = if text.chars().count() == 1 {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.replace(self.selection(), text, kind);
    }

    pub fn undo(&mut self) -> bool {
        let Some(group) = self.undo.pop() else {
            return false;
        };
        for e in group.edits.iter().rev() {
            self.text
                .replace_range(e.at..e.at + e.inserted.len(), &e.deleted);
        }
        (self.anchor, self.head) = group.sel_before;
        self.redo.push(group);
        self.version += 1;
        self.last_kind = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(group) = self.redo.pop() else {
            return false;
        };
        for e in &group.edits {
            self.text
                .replace_range(e.at..e.at + e.deleted.len(), &e.inserted);
        }
        (self.anchor, self.head) = group.sel_after;
        self.undo.push(group);
        self.version += 1;
        self.last_kind = None;
        true
    }

    // ---- navigation helpers -------------------------------------------------

    pub fn prev_grapheme(&self, offset: usize) -> usize {
        self.text[..offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    pub fn next_grapheme(&self, offset: usize) -> usize {
        self.text[offset..]
            .graphemes(true)
            .next()
            .map_or(self.text.len(), |g| offset + g.len())
    }

    pub fn prev_word(&self, offset: usize) -> usize {
        self.text[..offset]
            .split_word_bound_indices()
            .rev()
            .find(|(_, w)| w.chars().any(char::is_alphanumeric))
            .map_or(0, |(i, _)| i)
    }

    pub fn next_word(&self, offset: usize) -> usize {
        self.text[offset..]
            .split_word_bound_indices()
            .find(|(_, w)| w.chars().any(char::is_alphanumeric))
            .map_or(self.text.len(), |(i, w)| offset + i + w.len())
    }

    /// The word around `offset` (for double click).
    pub fn word_at(&self, offset: usize) -> Range<usize> {
        let line = self.line_range(offset);
        let base = line.start;
        for (i, w) in self.text[line].split_word_bound_indices() {
            let r = base + i..base + i + w.len();
            if r.contains(&offset) || r.end == offset && w.chars().any(char::is_alphanumeric) {
                return r;
            }
        }
        offset..offset
    }

    pub fn line_start(&self, offset: usize) -> usize {
        self.text[..offset].rfind('\n').map_or(0, |i| i + 1)
    }

    pub fn line_end(&self, offset: usize) -> usize {
        self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |i| offset + i)
    }

    pub fn line_range(&self, offset: usize) -> Range<usize> {
        self.line_start(offset)..self.line_end(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_is_grouped_by_word() {
        let mut b = Buffer::new("");
        for c in "hello world".chars() {
            b.insert(&c.to_string());
        }
        assert_eq!(b.text(), "hello world");
        b.undo();
        assert_eq!(b.text(), "hello");
        b.undo();
        assert_eq!(b.text(), "");
        b.redo();
        b.redo();
        assert_eq!(b.text(), "hello world");
        assert_eq!(b.head(), 11);
    }

    #[test]
    fn replace_selection_and_undo_restores_selection() {
        let mut b = Buffer::new("one two three");
        b.set_selection(4, 7);
        b.insert("2");
        assert_eq!(b.text(), "one 2 three");
        b.undo();
        assert_eq!(b.text(), "one two three");
        assert_eq!(b.selection(), 4..7);
    }

    #[test]
    fn backspaces_merge() {
        let mut b = Buffer::new("abcd");
        b.move_to(4);
        for _ in 0..3 {
            let h = b.head();
            let p = b.prev_grapheme(h);
            b.replace(p..h, "", EditKind::Delete);
        }
        assert_eq!(b.text(), "a");
        b.undo();
        assert_eq!(b.text(), "abcd");
    }

    #[test]
    fn graphemes_keep_vietnamese_combining_marks() {
        // "ệ" written as e + U+0323 + U+0302
        let b = Buffer::new("Vie\u{323}\u{302}t");
        let after_e = b.next_grapheme(2);
        assert_eq!(&b.text()[2..after_e], "e\u{323}\u{302}");
        assert_eq!(b.prev_grapheme(after_e), 2);
    }

    #[test]
    fn word_and_line_navigation() {
        let b = Buffer::new("foo bar\nbaz qux");
        assert_eq!(b.next_word(0), 3);
        assert_eq!(b.next_word(3), 7);
        assert_eq!(b.prev_word(7), 4);
        assert_eq!(b.line_range(9), 8..15);
        assert_eq!(b.word_at(9), 8..11);
        assert_eq!(b.word_at(3), 0..3);
    }

    #[test]
    fn replace_many_is_one_undo_step() {
        let mut b = Buffer::new("a b c");
        b.replace_many(&[(0..1, "A".into()), (4..5, "CC".into())]);
        assert_eq!(b.text(), "A b CC");
        b.undo();
        assert_eq!(b.text(), "a b c");
        b.redo();
        assert_eq!(b.text(), "A b CC");
    }

    #[test]
    fn reset_bumps_version_and_is_clean() {
        let mut b = Buffer::new("a");
        let v = b.version();
        b.reset("longer text");
        assert!(b.version() > v);
        assert!(!b.is_dirty());
        assert!(!b.undo());
    }

    #[test]
    fn dirty_tracking() {
        let mut b = Buffer::new("x");
        assert!(!b.is_dirty());
        b.insert("y");
        assert!(b.is_dirty());
        b.mark_saved();
        assert!(!b.is_dirty());
    }
}
