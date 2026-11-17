//! Focus Mode: find the sentence or paragraph that contains the cursor.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum FocusMode {
    #[default]
    Off,
    Sentence,
    Paragraph,
}

/// Lines that start their own block even without a blank line before them.
fn starts_block(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with('#')
        || t.starts_with("- ")
        || t.starts_with("* ")
        || t.starts_with("+ ")
        || t.starts_with('>')
        || t.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

pub fn paragraph_range(text: &str, offset: usize) -> Range<usize> {
    let mut lines = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        lines.push(start..start + line.trim_end_matches('\n').len());
        start += line.len();
    }
    if text.is_empty() || text.ends_with('\n') {
        lines.push(text.len()..text.len());
    }
    let Some(cur) = lines.iter().position(|l| offset <= l.end) else {
        return offset..offset;
    };
    if lines[cur].is_empty() {
        return lines[cur].clone();
    }
    let heading = |i: usize| text[lines[i].clone()].trim_start().starts_with('#');
    let joins = |i: usize| {
        !lines[i - 1].is_empty() && !starts_block(&text[lines[i].clone()]) && !heading(i - 1)
    };
    let mut first = cur;
    while first > 0 && joins(first) {
        first -= 1;
    }
    let mut last = cur;
    while last + 1 < lines.len() && !lines[last + 1].is_empty() && joins(last + 1) {
        last += 1;
    }
    lines[first].start..lines[last].end
}

pub fn sentence_range(text: &str, offset: usize) -> Range<usize> {
    let para = paragraph_range(text, offset);
    let p = &text[para.clone()];
    let mut best = para.end..para.end;
    for (i, s) in p.split_sentence_bound_indices() {
        let r = para.start + i..para.start + i + s.trim_end().len();
        // Cursor right after the final period still belongs to that sentence.
        if offset < para.start + i + s.len() || offset <= r.end {
            best = r;
            break;
        }
    }
    best
}

pub fn focus_range(text: &str, offset: usize, mode: FocusMode) -> Option<Range<usize>> {
    match mode {
        FocusMode::Off => None,
        FocusMode::Sentence => Some(sentence_range(text, offset)),
        FocusMode::Paragraph => Some(paragraph_range(text, offset)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "# Head\nFirst one. Second one! Third?\nstill third para line.\n\nNext para.";

    #[test]
    fn paragraphs() {
        let at = |s: &str| T.find(s).unwrap();
        assert_eq!(&T[paragraph_range(T, at("Head"))], "# Head");
        assert_eq!(
            &T[paragraph_range(T, at("Second"))],
            "First one. Second one! Third?\nstill third para line."
        );
        assert_eq!(&T[paragraph_range(T, at("Next"))], "Next para.");
    }

    #[test]
    fn sentences() {
        let at = |s: &str| T.find(s).unwrap();
        assert_eq!(&T[sentence_range(T, at("Second") + 2)], "Second one!");
        assert_eq!(&T[sentence_range(T, at("one.") + 4)], "First one.");
        // Like UAX #29, a line break ends a sentence.
        assert_eq!(&T[sentence_range(T, at("Third"))], "Third?");
    }

    #[test]
    fn empty_and_end_of_text() {
        assert_eq!(sentence_range("", 0), 0..0);
        let t = "Hi.\n";
        assert_eq!(paragraph_range(t, 4), 4..4);
    }
}
