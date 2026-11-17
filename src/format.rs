//! Markdown editing commands as pure functions: (text, selection) → one replacement.

use std::ops::Range;

#[derive(Debug, PartialEq)]
pub struct Change {
    /// Range of the original text to replace.
    pub range: Range<usize>,
    pub text: String,
    /// Selection afterwards, in new-text coordinates.
    pub select: Range<usize>,
}

fn line_start(t: &str, i: usize) -> usize {
    t[..i].rfind('\n').map_or(0, |p| p + 1)
}

fn line_end(t: &str, i: usize) -> usize {
    t[i..].find('\n').map_or(t.len(), |p| i + p)
}

/// Whole lines covered by the selection.
fn lines_of(t: &str, sel: &Range<usize>) -> Range<usize> {
    let end = if sel.end > sel.start && t[..sel.end].ends_with('\n') {
        sel.end - 1
    } else {
        sel.end
    };
    line_start(t, sel.start)..line_end(t, end.max(sel.start))
}

fn word_around(t: &str, i: usize) -> Range<usize> {
    let is_w = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let s = t[..i]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_w(*c))
        .last()
        .map_or(i, |(p, _)| p);
    let e = t[i..]
        .char_indices()
        .take_while(|(_, c)| is_w(*c))
        .last()
        .map_or(i, |(p, c)| i + p + c.len_utf8());
    s..e
}

/// Bold/italic/code/strike/highlight: wrap the selection (or word), or unwrap if already wrapped.
pub fn toggle_wrap(t: &str, sel: Range<usize>, open: &str, close: &str) -> Change {
    let sel = if sel.is_empty() { word_around(t, sel.start) } else { sel };
    let before = &t[..sel.start];
    let after = &t[sel.end..];
    if before.ends_with(open) && after.starts_with(close) {
        let a = sel.start - open.len();
        return Change {
            range: a..sel.end + close.len(),
            text: t[sel.clone()].to_string(),
            select: a..a + sel.len(),
        };
    }
    let inner = &t[sel.clone()];
    if inner.len() >= open.len() + close.len() && inner.starts_with(open) && inner.ends_with(close) {
        let text = inner[open.len()..inner.len() - close.len()].to_string();
        let n = text.len();
        return Change { range: sel.clone(), text, select: sel.start..sel.start + n };
    }
    let text = format!("{open}{inner}{close}");
    let s = sel.start + open.len();
    Change { range: sel.clone(), text, select: s..s + inner.len() }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Prefix {
    List,
    Task,
    Ordered,
    OrderedTask,
    Quote,
    Comment,
}

/// Leading block markers of a line: indentation + list/task/quote/heading/comment marker.
pub fn marker_len(line: &str) -> usize {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    let mut n = 0;
    if let Some(r) = rest.strip_prefix("// ") {
        return indent + rest.len() - r.len();
    }
    if rest.starts_with('#') {
        let h = rest.chars().take_while(|&c| c == '#').count();
        if rest[h..].starts_with(' ') {
            return indent + h + 1;
        }
    }
    if let Some(r) = rest.strip_prefix("> ") {
        n = rest.len() - r.len();
    } else if let Some(r) = rest.strip_prefix(['-', '*', '+']).and_then(|r| r.strip_prefix(' ')) {
        n = rest.len() - r.len();
    } else {
        let d = rest.chars().take_while(char::is_ascii_digit).count();
        if d > 0 && (rest[d..].starts_with(". ") || rest[d..].starts_with(") ")) {
            n = d + 2;
        }
    }
    if n > 0 {
        let r = &rest[n..];
        if r.starts_with("[ ] ") || r.starts_with("[x] ") || r.starts_with("[X] ") {
            n += 4;
        }
    }
    indent + n
}

fn edit_lines(t: &str, sel: &Range<usize>, mut f: impl FnMut(usize, &str) -> String) -> Change {
    let lines = lines_of(t, sel);
    let src = &t[lines.clone()];
    let out: Vec<String> = src.split('\n').enumerate().map(|(i, l)| f(i, l)).collect();
    let text = out.join("\n");
    let n = text.len();
    let select = if sel.is_empty() {
        // Keep the caret at the end of the (single) edited line.
        lines.start + n..lines.start + n
    } else {
        lines.start..lines.start + n
    };
    Change { range: lines, text, select }
}

pub fn set_heading(t: &str, sel: Range<usize>, level: usize) -> Change {
    edit_lines(t, &sel, |_, l| {
        let body = l.trim_start_matches('#').trim_start();
        let current = l.chars().take_while(|&c| c == '#').count();
        if level == 0 || current == level {
            body.to_string()
        } else {
            format!("{} {}", "#".repeat(level), body)
        }
    })
}

pub fn toggle_prefix(t: &str, sel: Range<usize>, kind: Prefix) -> Change {
    let lines = lines_of(t, &sel);
    let all_have = t[lines.clone()].split('\n').filter(|l| !l.trim().is_empty()).all(|l| {
        let m = &l[..marker_len(l)];
        let m = m.trim_start();
        match kind {
            Prefix::List => (m.starts_with("- ") || m.starts_with("* ")) && !m.contains('['),
            Prefix::Task => m.contains("[ ]") || m.contains("[x]"),
            Prefix::Ordered => m.starts_with(|c: char| c.is_ascii_digit()) && !m.contains('['),
            Prefix::OrderedTask => m.starts_with(|c: char| c.is_ascii_digit()) && m.contains('['),
            Prefix::Quote => m.starts_with('>'),
            Prefix::Comment => m.starts_with("//"),
        }
    });
    let mut n = 0;
    edit_lines(t, &sel, |_, l| {
        if l.trim().is_empty() && !sel.is_empty() {
            return l.to_string();
        }
        let indent_len = l.len() - l.trim_start_matches([' ', '\t']).len();
        let indent = &l[..indent_len];
        let body = &l[marker_len(l)..];
        if all_have {
            return format!("{indent}{body}");
        }
        n += 1;
        let marker = match kind {
            Prefix::List => "- ".to_string(),
            Prefix::Task => "- [ ] ".to_string(),
            Prefix::Ordered => format!("{n}. "),
            Prefix::OrderedTask => format!("{n}. [ ] "),
            Prefix::Quote => "> ".to_string(),
            Prefix::Comment => "// ".to_string(),
        };
        format!("{indent}{marker}{body}")
    })
}

pub fn toggle_task_done(t: &str, sel: Range<usize>) -> Change {
    edit_lines(t, &sel, |_, l| {
        if let Some(i) = l.find("[ ] ") {
            format!("{}[x] {}", &l[..i], &l[i + 4..])
        } else if let Some(i) = l.find("[x] ").or_else(|| l.find("[X] ")) {
            format!("{}[ ] {}", &l[..i], &l[i + 4..])
        } else {
            l.to_string()
        }
    })
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.") || s.starts_with("mailto:")
}

pub fn link(t: &str, sel: Range<usize>, image: bool) -> Change {
    let inner = &t[sel.clone()];
    let bang = if image { "!" } else { "" };
    if looks_like_url(inner) {
        let text = format!("{bang}[]({inner})");
        let c = sel.start + bang.len() + 1;
        Change { range: sel, text, select: c..c }
    } else {
        let text = format!("{bang}[{inner}]()");
        let c = sel.start + text.len() - 1;
        Change { range: sel, text, select: c..c }
    }
}

pub fn wikilink(t: &str, sel: Range<usize>) -> Change {
    let inner = &t[sel.clone()];
    let text = format!("[[{inner}]]");
    let c = sel.start + 2 + inner.len();
    Change { range: sel, text, select: c..c }
}

pub fn footnote(t: &str, sel: Range<usize>) -> Change {
    let mut n = 1;
    while t.contains(&format!("[^{n}]")) {
        n += 1;
    }
    let reference = format!("[^{n}]");
    let mut text = format!("{reference}{}", &t[sel.end..]);
    let tail = if t.ends_with('\n') { "\n" } else { "\n\n" };
    text.push_str(&format!("{tail}{reference}: "));
    let c = sel.end + text.len();
    Change { range: sel.end..t.len(), text, select: c..c }
}

/// Insert a block on its own line(s), e.g. "---" or a table.
pub fn insert_block(t: &str, sel: Range<usize>, block: &str, caret_in_block: usize) -> Change {
    let ls = line_start(t, sel.start);
    let le = line_end(t, sel.end);
    let current = &t[ls..le];
    let (range, prefix) = if current.trim().is_empty() {
        (ls..le, "")
    } else {
        (le..le, "\n")
    };
    let text = format!("{prefix}{block}");
    let c = range.start + prefix.len() + caret_in_block;
    Change { range, text, select: c..c }
}

pub fn wrap_lines(t: &str, sel: Range<usize>, open: &str, close: &str) -> Change {
    let lines = lines_of(t, &sel);
    let inner = &t[lines.clone()];
    let text = format!("{open}\n{inner}\n{close}");
    let c = lines.start + open.len() + 1 + inner.len();
    Change { range: lines, text, select: c..c }
}

/// Enter: continue lists/quotes; an empty item ends the list.
pub fn newline(t: &str, sel: Range<usize>) -> Change {
    let ls = line_start(t, sel.start);
    let line = &t[ls..line_end(t, sel.start)];
    let m = marker_len(line);
    let marker = &line[..m];
    let is_heading = marker.trim_start().starts_with('#');
    if m == 0 || is_heading || sel.start < ls + m {
        return Change { range: sel.clone(), text: "\n".into(), select: sel.start + 1..sel.start + 1 };
    }
    if line[m..].trim().is_empty() {
        // Empty item: remove its marker instead of adding another.
        return Change { range: ls..ls + line.len(), text: String::new(), select: ls..ls };
    }
    let indent_len = marker.len() - marker.trim_start().len();
    let trimmed = marker.trim_start();
    let mut next = trimmed.to_string();
    let d = trimmed.chars().take_while(char::is_ascii_digit).count();
    if d > 0 {
        let n: u64 = trimmed[..d].parse().unwrap_or(0);
        next = format!("{}{}", n + 1, &trimmed[d..]);
    }
    next = next.replace("[x] ", "[ ] ").replace("[X] ", "[ ] ");
    let text = format!("\n{}{}", &marker[..indent_len], next);
    let c = sel.start + text.len();
    Change { range: sel, text, select: c..c }
}

pub fn indent(t: &str, sel: Range<usize>) -> Change {
    edit_lines(t, &sel, |_, l| format!("\t{l}"))
}

pub fn outdent(t: &str, sel: Range<usize>) -> Change {
    edit_lines(t, &sel, |_, l| {
        if let Some(r) = l.strip_prefix('\t') {
            r.to_string()
        } else {
            let n = l.len() - l.trim_start_matches(' ').len();
            l[n.min(4)..].to_string()
        }
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Case {
    Upper,
    Lower,
    Capitalize,
    Title,
}

const SMALL_WORDS: &[&str] = &[
    "a", "an", "and", "as", "at", "but", "by", "for", "in", "nor", "of", "on", "or", "the", "to", "up", "via", "vs",
];

pub fn transform(t: &str, sel: Range<usize>, case: Case) -> Change {
    let sel = if sel.is_empty() { word_around(t, sel.start) } else { sel };
    let s = &t[sel.clone()];
    let cap = |w: &str| {
        let mut c = w.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase())
            .unwrap_or_default()
    };
    let text = match case {
        Case::Upper => s.to_uppercase(),
        Case::Lower => s.to_lowercase(),
        Case::Capitalize | Case::Title => {
            let mut out = String::new();
            let mut first = true;
            for piece in s.split_inclusive(char::is_whitespace) {
                let word = piece.trim_end();
                let ws = &piece[word.len()..];
                let lower = word.to_lowercase();
                let keep_small = case == Case::Title && !first && SMALL_WORDS.contains(&lower.as_str());
                out.push_str(&if keep_small { lower } else { cap(word) });
                out.push_str(ws);
                if !word.is_empty() {
                    first = false;
                }
            }
            out
        }
    };
    let n = text.len();
    Change { range: sel.clone(), text, select: sel.start..sel.start + n }
}

/// Swap the selected lines with the line above/below.
pub fn move_lines(t: &str, sel: Range<usize>, up: bool) -> Option<Change> {
    let lines = lines_of(t, &sel);
    if up {
        if lines.start == 0 {
            return None;
        }
        let prev = line_start(t, lines.start - 1)..lines.start - 1;
        let text = format!("{}\n{}", &t[lines.clone()], &t[prev.clone()]);
        let shift = prev.len() + 1;
        Some(Change {
            range: prev.start..lines.end,
            text,
            select: sel.start - shift..sel.end - shift,
        })
    } else {
        if lines.end >= t.len() {
            return None;
        }
        let next = lines.end + 1..line_end(t, lines.end + 1);
        let text = format!("{}\n{}", &t[next.clone()], &t[lines.clone()]);
        let shift = next.len() + 1;
        Some(Change {
            range: lines.start..next.end,
            text,
            select: sel.start + shift..sel.end + shift,
        })
    }
}

/// Remove inline and block Markdown markers.
pub fn clear_styles(t: &str, sel: Range<usize>) -> Change {
    let sel = if sel.is_empty() { lines_of(t, &sel) } else { sel };
    let lines: Vec<String> = t[sel.clone()]
        .split('\n')
        .map(|l| {
            let body = &l[marker_len(l)..];
            let indent = &l[..l.len() - l.trim_start().len()];
            let mut s = format!("{indent}{body}");
            for m in ["**", "__", "~~", "==", "`", "*", "_"] {
                s = s.replace(m, "");
            }
            s
        })
        .collect();
    let text = lines.join("\n");
    let n = text.len();
    Change { range: sel.clone(), text, select: sel.start..sel.start + n }
}

pub const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];

/// Typing-time substitutions: wrap selection with markers, auto-pair, smart quotes and dashes.
pub fn smart_input(t: &str, sel: Range<usize>, typed: &str, quotes: bool, dashes: bool, pair: bool) -> Option<Change> {
    let mut chars = typed.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let prev = t[..sel.start].chars().next_back();
    let next = t[sel.end..].chars().next();
    if !sel.is_empty() && matches!(c, '*' | '_' | '`' | '~' | '=' | '(' | '[' | '"') {
        let close = PAIRS.iter().find(|p| p.0 == c).map_or(c, |p| p.1);
        let inner = &t[sel.clone()];
        let text = format!("{c}{inner}{close}");
        let s = sel.start + c.len_utf8();
        return Some(Change { range: sel.clone(), text, select: s..s + inner.len() });
    }
    if quotes && (c == '"' || c == '\'') {
        let opening = prev.is_none_or(|p| p.is_whitespace() || "([{-—".contains(p));
        let q = match (c, opening) {
            ('"', true) => '“',
            ('"', false) => '”',
            ('\'', true) => '‘',
            _ => '’',
        };
        let s = sel.start + q.len_utf8();
        return Some(Change { range: sel, text: q.to_string(), select: s..s });
    }
    if dashes && c == ' ' && t[..sel.start].ends_with(" --") {
        let a = sel.start - 2;
        return Some(Change { range: a..sel.end, text: "— ".into(), select: a + 4..a + 4 });
    }
    if pair {
        if let Some(&(open, close)) = PAIRS.iter().find(|p| p.0 == c) {
            let at_word_end = next.is_none_or(|n| n.is_whitespace() || ")]}.,;:".contains(n));
            let after_word = prev.is_some_and(char::is_alphanumeric);
            if at_word_end && !(open == '"' && after_word) {
                let s = sel.start + 1;
                return Some(Change { range: sel, text: format!("{open}{close}"), select: s..s });
            }
        }
        // Typing a closer that is already there just steps over it.
        if PAIRS.iter().any(|p| p.1 == c) && next == Some(c) && sel.is_empty() {
            let s = sel.start + 1;
            return Some(Change { range: sel.start..sel.start + 1, text: c.to_string(), select: s..s });
        }
    }
    None
}

/// Backspace between an empty pair "(|)" removes both.
pub fn delete_pair(t: &str, cursor: usize) -> Option<Range<usize>> {
    let prev = t[..cursor].chars().next_back()?;
    let next = t[cursor..].chars().next()?;
    PAIRS
        .iter()
        .any(|&(o, c)| o == prev && c == next)
        .then(|| cursor - 1..cursor + 1)
}

pub fn table_template() -> (&'static str, usize) {
    ("| Column | Column |\n| --- | --- |\n| Cell | Cell |\n", 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(t: &str, c: &Change) -> String {
        format!("{}{}{}", &t[..c.range.start], c.text, &t[c.range.end..])
    }

    #[test]
    fn wrap_and_unwrap() {
        let t = "make this bold";
        let c = toggle_wrap(t, 5..9, "**", "**");
        let t2 = apply(t, &c);
        assert_eq!(t2, "make **this** bold");
        assert_eq!(&t2[c.select.clone()], "this");
        let c2 = toggle_wrap(&t2, c.select.clone(), "**", "**");
        assert_eq!(apply(&t2, &c2), t);
        // Empty selection wraps the word under the caret.
        assert_eq!(apply(t, &toggle_wrap(t, 6..6, "*", "*")), "make *this* bold");
    }

    #[test]
    fn headings_and_prefixes() {
        let t = "Title\nbody";
        assert_eq!(apply(t, &set_heading(t, 2..2, 2)), "## Title\nbody");
        let t2 = "## Title";
        assert_eq!(apply(t2, &set_heading(t2, 0..0, 2)), "Title");
        assert_eq!(apply(t2, &set_heading(t2, 0..0, 1)), "# Title");

        let t = "a\nb";
        let c = toggle_prefix(t, 0..3, Prefix::Ordered);
        let t2 = apply(t, &c);
        assert_eq!(t2, "1. a\n2. b");
        assert_eq!(apply(&t2, &toggle_prefix(&t2, 0..t2.len(), Prefix::Ordered)), "a\nb");
        assert_eq!(apply(&t2, &toggle_prefix(&t2, 0..t2.len(), Prefix::List)), "- a\n- b");
        assert_eq!(apply("x", &toggle_prefix("x", 0..0, Prefix::Task)), "- [ ] x");
        assert_eq!(apply("- [ ] x", &toggle_task_done("- [ ] x", 0..0)), "- [x] x");
    }

    #[test]
    fn list_continuation() {
        let t = "- one";
        let c = newline(t, 5..5);
        assert_eq!(apply(t, &c), "- one\n- ");
        let t = "1. [x] one";
        assert_eq!(apply(t, &newline(t, 10..10)), "1. [x] one\n2. [ ] ");
        let t = "- one\n- ";
        assert_eq!(apply(t, &newline(t, 8..8)), "- one\n");
        let t = "> quote";
        assert_eq!(apply(t, &newline(t, 7..7)), "> quote\n> ");
        let t = "# Head";
        assert_eq!(apply(t, &newline(t, 6..6)), "# Head\n");
    }

    #[test]
    fn links_footnotes_blocks() {
        let t = "see docs";
        let c = link(t, 4..8, false);
        assert_eq!(apply(t, &c), "see [docs]()");
        assert_eq!(c.select, 11..11);
        let t = "https://ia.net";
        assert_eq!(apply(t, &link(t, 0..t.len(), false)), "[](https://ia.net)");
        let t = "Text.";
        let c = footnote(t, 4..4);
        assert_eq!(apply(t, &c), "Text[^1].\n\n[^1]: ");
        let t = "para";
        assert_eq!(apply(t, &insert_block(t, 2..2, "---\n", 4)), "para\n---\n");
    }

    #[test]
    fn transforms_and_moves() {
        let t = "the lord of the rings";
        assert_eq!(apply(t, &transform(t, 0..t.len(), Case::Title)), "The Lord of the Rings");
        assert_eq!(apply(t, &transform(t, 0..t.len(), Case::Capitalize)), "The Lord Of The Rings");
        assert_eq!(apply(t, &transform(t, 0..3, Case::Upper)), "THE lord of the rings");
        let t = "a\nb\nc";
        let c = move_lines(t, 2..2, true).unwrap();
        assert_eq!(apply(t, &c), "b\na\nc");
        assert_eq!(c.select, 0..0);
        let c = move_lines(t, 2..2, false).unwrap();
        assert_eq!(apply(t, &c), "a\nc\nb");
        assert_eq!(c.select, 4..4);
        assert!(move_lines(t, 0..0, true).is_none());
    }

    #[test]
    fn smart_typing() {
        assert_eq!(apply("say ", &smart_input("say ", 4..4, "\"", true, false, false).unwrap()), "say “");
        assert_eq!(apply("it", &smart_input("it", 2..2, "'", true, false, false).unwrap()), "it’");
        let c = smart_input("f", 1..1, "(", false, false, true).unwrap();
        assert_eq!((apply("f", &c), c.select), ("f()".into(), 2..2));
        let c = smart_input("ab", 0..2, "*", false, false, false).unwrap();
        assert_eq!(apply("ab", &c), "*ab*");
        let t = "a --";
        assert_eq!(apply(t, &smart_input(t, 4..4, " ", false, true, false).unwrap()), "a — ");
        assert_eq!(delete_pair("()", 1), Some(0..2));
        assert_eq!(apply("x**y**", &clear_styles("x**y**", 0..6)), "xy");
    }
}

/// Word completion: words in `text` starting with the word before `cursor`,
/// most frequent first. Returns (prefix range, candidates).
pub fn completions(text: &str, cursor: usize, limit: usize) -> (std::ops::Range<usize>, Vec<String>) {
    let start = text[..cursor]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '-')
        .last()
        .map_or(cursor, |(i, _)| i);
    let prefix = &text[start..cursor];
    if prefix.chars().count() < 2 {
        return (start..cursor, Vec::new());
    }
    let lower = prefix.to_lowercase();
    let mut counts: std::collections::HashMap<&str, usize> = Default::default();
    for w in unicode_segmentation::UnicodeSegmentation::unicode_words(text) {
        if w.len() > prefix.len() && w.to_lowercase().starts_with(&lower) {
            *counts.entry(w).or_default() += 1;
        }
    }
    let mut words: Vec<(&str, usize)> = counts.into_iter().collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    (start..cursor, words.into_iter().take(limit).map(|(w, _)| w.to_string()).collect())
}

#[cfg(test)]
mod completion_tests {
    use super::completions;

    #[test]
    fn completes_from_document_words() {
        let t = "writing wrote writing written wr";
        let (r, c) = completions(t, t.len(), 5);
        assert_eq!(&t[r], "wr");
        assert_eq!(c, ["writing", "written", "wrote"]);
        assert!(completions("a b", 3, 5).1.is_empty());
    }
}
