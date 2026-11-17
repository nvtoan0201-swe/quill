//! Style Check: flag fillers, clichés and redundant phrases.
//! The word lists here are Quill's own (short, English only).

use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Check {
    Filler,
    Cliche,
    Redundancy,
    Custom,
}

const FILLERS: &[&str] = &[
    "actually", "basically", "really", "very", "just", "quite", "simply", "literally", "totally",
    "pretty much", "kind of", "sort of", "in order to", "somewhat", "definitely", "honestly",
    "obviously", "essentially", "certainly", "completely", "absolutely", "extremely", "truly",
    "seriously", "virtually", "practically", "a bit", "a little bit", "at all", "for the most part",
];

const CLICHES: &[&str] = &[
    "at the end of the day", "think outside the box", "low-hanging fruit", "game changer",
    "move the needle", "in this day and age", "last but not least", "needless to say",
    "the bottom line", "at this point in time", "few and far between", "perfect storm",
    "par for the course", "to the next level", "touch base", "it goes without saying",
    "when all is said and done", "best of both worlds", "read between the lines",
    "back to square one", "only time will tell", "the elephant in the room", "a whole new world",
    "avoid it like the plague", "every cloud has a silver lining", "in the nick of time",
    "time is money", "easier said than done", "on the same page", "win-win",
];

const REDUNDANCIES: &[&str] = &[
    "added bonus", "advance planning", "past history", "free gift", "end result", "final outcome",
    "close proximity", "completely finished", "each and every", "true fact", "unexpected surprise",
    "basic fundamentals", "future plans", "join together", "revert back", "repeat again",
    "combine together", "absolutely essential", "new innovation", "general consensus", "sum total",
    "atm machine", "pin number", "past experience", "collaborate together", "merge together",
    "first began", "still remains", "reason why", "plan ahead", "postpone until later",
    "exact same", "empty space", "brief summary", "over exaggerate", "mutual cooperation",
];

pub fn patterns(check: Check) -> &'static [&'static str] {
    match check {
        Check::Filler => FILLERS,
        Check::Cliche => CLICHES,
        Check::Redundancy => REDUNDANCIES,
        Check::Custom => &[],
    }
}

/// Case-insensitive whole-word matches of `patterns` in `text`.
pub fn find<S: AsRef<str>>(text: &str, patterns: &[S]) -> Vec<Range<usize>> {
    let lower = text.to_lowercase();
    // to_lowercase can change byte lengths for some scripts; only match when it did not.
    if lower.len() != text.len() {
        return Vec::new();
    }
    let b = lower.as_bytes();
    let is_word = |i: usize| b.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c >= 0x80);
    let mut out = Vec::new();
    for p in patterns {
        let p = p.as_ref().trim().to_lowercase();
        if p.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(i) = lower[from..].find(&p) {
            let s = from + i;
            let e = s + p.len();
            if (s == 0 || !is_word(s - 1)) && !is_word(e) {
                out.push(s..e);
            }
            from = e;
        }
    }
    out.sort_by_key(|r| r.start);
    out
}

/// Case-insensitive substring matches (Find).
pub fn find_substring(text: &str, query: &str) -> Vec<Range<usize>> {
    let lower = text.to_lowercase();
    let q = query.to_lowercase();
    if q.is_empty() || lower.len() != text.len() {
        // Fallback to exact matching when lowercasing changes byte offsets.
        return text.match_indices(query).map(|(i, m)| i..i + m.len()).collect();
    }
    lower.match_indices(&q).map(|(i, m)| i..i + m.len()).collect()
}

pub fn check(text: &str, enabled: &[Check], custom: &[String]) -> Vec<(Range<usize>, Check)> {
    let mut out = Vec::new();
    for &c in enabled {
        let ranges = if c == Check::Custom {
            find(text, custom)
        } else {
            find(text, patterns(c))
        };
        out.extend(ranges.into_iter().map(|r| (r, c)));
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_words_only() {
        let t = "It was very, very justified. Just do it.";
        let found = find(t, FILLERS);
        let words: Vec<&str> = found.iter().map(|r| &t[r.clone()]).collect();
        assert_eq!(words, ["very", "very", "Just"]);
    }

    #[test]
    fn phrases_and_custom() {
        let t = "At the end of the day the end result was a free gift. Synergy!";
        let all = check(t, &[Check::Filler, Check::Cliche, Check::Redundancy, Check::Custom], &["synergy".to_string()]);
        let kinds: Vec<Check> = all.iter().map(|(_, c)| *c).collect();
        assert_eq!(
            kinds,
            [Check::Cliche, Check::Redundancy, Check::Redundancy, Check::Custom]
        );
    }
}
