//! Turning a Markdown document into prose the analyzers can read.
//! Offsets always stay byte offsets into the original text, so every finding
//! can be underlined in the editor.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::markdown;

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub range: Range<usize>,
    /// Lowercase form, curly apostrophes folded to `'`.
    pub lower: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sentence {
    pub range: Range<usize>,
    /// Indices into `Prose::words`.
    pub words: Range<usize>,
    pub paragraph: usize,
    /// Headings, list items without a verb-ish ending etc. are not judged as sentences.
    pub heading: bool,
}

pub struct Prose {
    /// Same length as the source; code, URLs, markup and comments replaced by spaces.
    pub text: String,
    pub words: Vec<Word>,
    pub sentences: Vec<Sentence>,
    pub paragraphs: Vec<Range<usize>>,
}

impl Prose {
    pub fn word(&self, i: usize) -> &str {
        &self.words[i].lower
    }

    pub fn sentence_words(&self, s: &Sentence) -> &[Word] {
        &self.words[s.words.clone()]
    }

    /// Sentences that are real prose (not headings).
    pub fn body(&self) -> impl Iterator<Item = &Sentence> {
        self.sentences.iter().filter(|s| !s.heading)
    }

    pub fn body_words(&self) -> usize {
        self.body().map(|s| s.words.len()).sum()
    }
}

/// Byte mask of the text that is prose (not code, URLs, Markdown marks or comments).
fn mask(text: &str) -> String {
    let flags = markdown::style(text);
    let quiet = markdown::CODE | markdown::CODEBLOCK | markdown::URL | markdown::MARKUP | markdown::COMMENT;
    let mut out = String::with_capacity(text.len());
    for (i, ch) in text.char_indices() {
        let f = flags.get(i).copied().unwrap_or(0);
        if f & quiet != 0 && ch != '\n' {
            // Keep byte length identical.
            for _ in 0..ch.len_utf8() {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_heading_line(src: &str, line: Range<usize>) -> bool {
    let l = src[line].trim_start();
    l.starts_with('#') || l.starts_with("---") || l.starts_with("===") || l.starts_with('|')
}

pub fn prose(src: &str) -> Prose {
    let text = mask(src);
    let mut words = Vec::new();
    for (i, w) in text.unicode_word_indices() {
        if !w.chars().any(char::is_alphabetic) {
            continue;
        }
        let lower = w.to_lowercase().replace('’', "'");
        words.push(Word { range: i..i + w.len(), lower });
    }
    // Paragraphs: runs of non-blank lines. Each list item or heading is its own paragraph.
    let mut paragraphs: Vec<Range<usize>> = Vec::new();
    let mut lines = Vec::new();
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let end = at + line.trim_end_matches(['\n', '\r']).len();
        lines.push(at..end);
        at += line.len();
    }
    let mut cur: Option<Range<usize>> = None;
    for l in &lines {
        let raw = src[l.clone()].trim_start();
        let blank = text[l.clone()].trim().is_empty();
        let starts_block = raw.starts_with('#')
            || raw.starts_with("- ")
            || raw.starts_with("* ")
            || raw.starts_with("> ")
            || raw.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if blank {
            if let Some(c) = cur.take() {
                paragraphs.push(c);
            }
            continue;
        }
        match &mut cur {
            Some(c) if !starts_block && !is_heading_line(src, c.clone()) => c.end = l.end,
            _ => {
                if let Some(c) = cur.take() {
                    paragraphs.push(c);
                }
                cur = Some(l.clone());
            }
        }
    }
    if let Some(c) = cur {
        paragraphs.push(c);
    }

    let mut sentences = Vec::new();
    for (pi, p) in paragraphs.iter().enumerate() {
        let heading = is_heading_line(src, p.clone());
        let ptext = &text[p.clone()];
        for (off, s) in split_sentences(ptext) {
            let trimmed_start = s.len() - s.trim_start().len();
            let start = p.start + off + trimmed_start;
            let end = p.start + off + s.trim_end().len();
            if start >= end || !text[start..end].chars().any(char::is_alphabetic) {
                continue;
            }
            let w0 = words.partition_point(|w| w.range.start < start);
            let w1 = words.partition_point(|w| w.range.start < end);
            sentences.push(Sentence { range: start..end, words: w0..w1, paragraph: pi, heading });
        }
    }
    Prose { text, words, sentences, paragraphs }
}

/// Sentence split that does not break after common abbreviations ("e.g.", "Dr.").
fn split_sentences(p: &str) -> Vec<(usize, &str)> {
    const ABBREV: &[&str] = &["e.g.", "i.e.", "etc.", "vs.", "mr.", "mrs.", "ms.", "dr.", "prof.", "st.", "fig.", "no.", "approx.", "u.s."];
    let mut out: Vec<(usize, &str)> = Vec::new();
    let mut pending: Option<usize> = None;
    for (i, s) in p.split_sentence_bound_indices() {
        let start = pending.take().unwrap_or(i);
        let end = i + s.len();
        let last = s.trim_end().rsplit(char::is_whitespace).next().unwrap_or("").to_lowercase();
        if ABBREV.contains(&last.as_str()) && end < p.len() {
            pending = Some(start);
            continue;
        }
        out.push((start, &p[start..end]));
    }
    if let Some(start) = pending {
        out.push((start, &p[start..]));
    }
    out
}

/// English syllable estimate (vowel groups with the usual silent-e corrections).
pub fn syllables(word: &str) -> usize {
    let w: String = word.to_lowercase().chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if w.is_empty() {
        return 0;
    }
    if w.len() <= 3 {
        return 1;
    }
    let b = w.as_bytes();
    let vowel = |c: u8| matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y');
    let mut count = 0;
    let mut prev = false;
    for &c in b {
        let v = vowel(c);
        if v && !prev {
            count += 1;
        }
        prev = v;
    }
    // Silent final e ("make"), but not "-le" after a consonant ("table").
    if w.ends_with('e') && !w.ends_with("le") && !w.ends_with("ee") && count > 1 {
        count -= 1;
    }
    // "-ed" is usually not its own syllable unless after t/d ("wanted").
    if w.ends_with("ed") && !w.ends_with("ted") && !w.ends_with("ded") && count > 1 {
        count -= 1;
    }
    // "-es" after most consonants is silent ("makes"), not after s/x/z/ch/sh.
    if w.ends_with("es") && !(w.ends_with("ses") || w.ends_with("xes") || w.ends_with("zes") || w.ends_with("ches") || w.ends_with("shes")) && count > 1 {
        count -= 1;
    }
    // Vowel pairs that are two syllables ("ratio", "media"), except the
    // "-tion", "-sion", "-cial", "-gion" endings where they merge.
    for pair in ["ia", "io", "eo", "ua"] {
        count += w
            .match_indices(pair)
            .filter(|(i, _)| {
                let merged = *i > 0
                    && matches!(b[i - 1], b't' | b's' | b'c' | b'x' | b'g')
                    && matches!(b.get(i + 2), Some(b'n' | b'l' | b'u'));
                !merged
            })
            .count();
    }
    count.max(1)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_markup_and_code() {
        let p = prose("# Title\n\nSee `code here` and **bold** text. Visit https://x.y now.");
        assert!(p.words.iter().all(|w| w.lower != "code"));
        assert!(p.words.iter().any(|w| w.lower == "bold"));
        assert_eq!(p.text.len(), "# Title\n\nSee `code here` and **bold** text. Visit https://x.y now.".len());
        assert!(p.sentences[0].heading);
        assert_eq!(p.body().count(), 2);
    }

    #[test]
    fn sentences_keep_abbreviations() {
        let p = prose("Use tools, e.g. a hammer. Then stop.");
        assert_eq!(p.sentences.len(), 2);
        assert_eq!(p.sentence_words(&p.sentences[0]).len(), 5);
    }

    #[test]
    fn paragraphs_and_lists() {
        let p = prose("One line\ncontinues here.\n\n- item one\n- item two\n\nLast.");
        assert_eq!(p.paragraphs.len(), 4);
    }

    #[test]
    fn syllable_counts() {
        for (w, n) in [("cat", 1), ("table", 2), ("make", 1), ("wanted", 2), ("jumped", 1), ("beautiful", 3), ("readability", 5), ("ratio", 3), ("the", 1), ("nation", 2), ("social", 2), ("media", 3)] {
            assert_eq!(syllables(w), n, "{w}");
        }
    }
}
