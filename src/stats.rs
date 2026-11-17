//! Document statistics shown in the status bar.

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum StatKind {
    Characters,
    CharactersWithoutSpaces,
    #[default]
    Words,
    Sentences,
    ReadingTime,
    SpeakingTime,
    Tasks,
}

impl StatKind {
    pub const ALL: [StatKind; 7] = [
        StatKind::Characters,
        StatKind::CharactersWithoutSpaces,
        StatKind::Words,
        StatKind::Sentences,
        StatKind::ReadingTime,
        StatKind::SpeakingTime,
        StatKind::Tasks,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StatKind::Characters => "Characters",
            StatKind::CharactersWithoutSpaces => "Characters Without Spaces",
            StatKind::Words => "Words",
            StatKind::Sentences => "Sentences",
            StatKind::ReadingTime => "Reading Time",
            StatKind::SpeakingTime => "Speaking Time",
            StatKind::Tasks => "Tasks",
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Stats {
    pub characters: usize,
    pub characters_no_spaces: usize,
    pub words: usize,
    pub sentences: usize,
    pub tasks_done: usize,
    pub tasks_total: usize,
}

const READ_WPM: usize = 230;
const SPEAK_WPM: usize = 140;

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF)
}

pub fn compute(text: &str) -> Stats {
    let mut s = Stats::default();
    for g in text.graphemes(true) {
        if g == "\n" || g == "\r\n" {
            continue;
        }
        s.characters += 1;
        if !g.chars().all(char::is_whitespace) {
            s.characters_no_spaces += 1;
        }
    }
    for w in text.unicode_words() {
        // CJK has no spaces; count each ideograph as a word.
        let cjk = w.chars().filter(|&c| is_cjk(c)).count();
        s.words += if cjk > 0 { cjk } else { 1 };
    }
    s.sentences = text
        .unicode_sentences()
        .filter(|x| x.chars().any(char::is_alphanumeric))
        .count();
    for line in text.lines() {
        let t = line.trim_start();
        let t = t
            .strip_prefix("- ")
            .or_else(|| t.strip_prefix("* "))
            .or_else(|| t.split_once(". ").map(|(_, r)| r))
            .unwrap_or("");
        if t.starts_with("[ ]") {
            s.tasks_total += 1;
        } else if t.starts_with("[x]") || t.starts_with("[X]") {
            s.tasks_total += 1;
            s.tasks_done += 1;
        }
    }
    s
}

pub fn group(n: usize) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn minutes(words: usize, wpm: usize) -> String {
    let secs = words * 60 / wpm;
    if secs < 60 {
        format!("{secs} sec")
    } else {
        format!("{} min", (secs + 30) / 60)
    }
}

impl Stats {
    pub fn format(&self, kind: StatKind) -> String {
        let plural = |n: usize, one: &str, many: &str| {
            format!("{} {}", group(n), if n == 1 { one } else { many })
        };
        match kind {
            StatKind::Characters => plural(self.characters, "Character", "Characters"),
            StatKind::CharactersWithoutSpaces => {
                plural(self.characters_no_spaces, "Character", "Characters")
            }
            StatKind::Words => plural(self.words, "Word", "Words"),
            StatKind::Sentences => plural(self.sentences, "Sentence", "Sentences"),
            StatKind::ReadingTime => format!("{} Reading", minutes(self.words, READ_WPM)),
            StatKind::SpeakingTime => format!("{} Speaking", minutes(self.words, SPEAK_WPM)),
            StatKind::Tasks => format!("{} of {} Tasks", self.tasks_done, self.tasks_total),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts() {
        let s = compute("Hello world. Xin chào!\n- [x] a\n- [ ] b");
        assert_eq!(s.words, 7);
        // Each list item is its own sentence.
        assert_eq!(s.sentences, 4);
        assert_eq!(s.tasks_done, 1);
        assert_eq!(s.tasks_total, 2);
        assert_eq!(compute("a b").characters, 3);
        assert_eq!(compute("a b").characters_no_spaces, 2);
    }

    #[test]
    fn cjk_words() {
        assert_eq!(compute("日本語").words, 3);
    }

    #[test]
    fn formatting() {
        let s = Stats {
            words: 1234,
            ..Default::default()
        };
        assert_eq!(s.format(StatKind::Words), "1,234 Words");
        assert_eq!(s.format(StatKind::ReadingTime), "5 min Reading");
        assert_eq!(group(1_000_000), "1,000,000");
        assert_eq!(Stats { words: 1, ..Default::default() }.format(StatKind::Words), "1 Word");
    }
}
