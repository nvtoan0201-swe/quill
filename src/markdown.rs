//! Editor-side Markdown styling: every byte of the source gets a set of flags.
//! The source text is never hidden; markup characters are only dimmed.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub type Flags = u16;
pub const MARKUP: Flags = 1 << 0;
pub const STRONG: Flags = 1 << 1;
pub const EMPH: Flags = 1 << 2;
pub const CODE: Flags = 1 << 3;
pub const HEADING: Flags = 1 << 4;
pub const LINK: Flags = 1 << 5;
pub const URL: Flags = 1 << 6;
pub const QUOTE: Flags = 1 << 7;
pub const STRIKE: Flags = 1 << 8;
pub const HIGHLIGHT: Flags = 1 << 9;
pub const CODEBLOCK: Flags = 1 << 10;
pub const HASHTAG: Flags = 1 << 11;
pub const COMMENT: Flags = 1 << 12;
pub const TASK_DONE: Flags = 1 << 13;

pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
}

pub fn style(text: &str) -> Vec<Flags> {
    let mut f = vec![0 as Flags; text.len()];
    let b = text.as_bytes();
    let set = |r: Range<usize>, flag: Flags, f: &mut Vec<Flags>| {
        for x in &mut f[r.start.min(b.len())..r.end.min(b.len())] {
            *x |= flag;
        }
    };

    let mut task_item: Option<Range<usize>> = None;
    for (event, r) in Parser::new_ext(text, options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                set(r.clone(), HEADING, &mut f);
                let hashes = count(b, r.start, b'#');
                if hashes > 0 {
                    let end = skip_spaces(b, r.start + hashes);
                    set(r.start..end, MARKUP, &mut f);
                } else {
                    // Setext: underline row is markup.
                    if let Some(nl) = text[r.clone()].trim_end().rfind('\n') {
                        set(r.start + nl..r.end, MARKUP, &mut f);
                    }
                }
            }
            Event::Start(Tag::Emphasis) => {
                set(r.clone(), EMPH, &mut f);
                set(r.start..r.start + 1, MARKUP, &mut f);
                set(r.end - 1..r.end, MARKUP, &mut f);
            }
            Event::Start(Tag::Strong) => {
                set(r.clone(), STRONG, &mut f);
                set(r.start..r.start + 2, MARKUP, &mut f);
                set(r.end - 2..r.end, MARKUP, &mut f);
            }
            Event::Start(Tag::Strikethrough) => {
                set(r.clone(), STRIKE, &mut f);
                let n = count(b, r.start, b'~');
                set(r.start..r.start + n, MARKUP, &mut f);
                set(r.end - n..r.end, MARKUP, &mut f);
            }
            Event::Code(_) => {
                set(r.clone(), CODE, &mut f);
                let n = count(b, r.start, b'`');
                set(r.start..r.start + n, MARKUP, &mut f);
                set(r.end.saturating_sub(n)..r.end, MARKUP, &mut f);
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                set(r.clone(), CODE, &mut f);
                let n = count(b, r.start, b'$');
                set(r.start..r.start + n, MARKUP, &mut f);
                set(r.end.saturating_sub(n)..r.end, MARKUP, &mut f);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                set(r.clone(), CODEBLOCK, &mut f);
                if let CodeBlockKind::Fenced(_) = kind {
                    let first = line_end(b, r.start);
                    set(r.start..first, MARKUP, &mut f);
                    let body = text[r.clone()].trim_end_matches('\n');
                    if let Some(nl) = body.rfind('\n') {
                        let last = r.start + nl + 1;
                        let close = &text[last..r.start + body.len()];
                        if close.trim_start().starts_with("```") || close.trim_start().starts_with("~~~") {
                            set(last..r.start + body.len(), MARKUP, &mut f);
                        }
                    }
                }
            }
            Event::Start(Tag::Link { .. }) | Event::Start(Tag::Image { .. }) => {
                set(r.clone(), LINK, &mut f);
                let src = &text[r.clone()];
                if src.starts_with('<') {
                    set(r.clone(), URL, &mut f);
                } else if let Some(i) = src.rfind("](").or_else(|| src.rfind("][")) {
                    let open = if src.starts_with('!') { 2 } else { 1 };
                    set(r.start..r.start + open, MARKUP, &mut f);
                    set(r.start + i..r.end, MARKUP | URL, &mut f);
                }
            }
            Event::FootnoteReference(_) => set(r, LINK | MARKUP, &mut f),
            Event::Start(Tag::FootnoteDefinition(_)) => {
                if let Some(i) = text[r.clone()].find("]:") {
                    set(r.start..r.start + i + 2, MARKUP, &mut f);
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                set(r.clone(), QUOTE, &mut f);
                let mut i = r.start;
                while i < r.end {
                    let s = skip_spaces(b, i);
                    if b.get(s) == Some(&b'>') {
                        set(i..skip_spaces(b, s + 1).min(r.end), MARKUP, &mut f);
                    }
                    i = line_end(b, i) + 1;
                }
            }
            Event::Start(Tag::Item) => {
                let s = skip_spaces(b, r.start);
                let mut e = s;
                while e < b.len() && (b[e].is_ascii_digit()) {
                    e += 1;
                }
                if e < b.len() && matches!(b[e], b'-' | b'*' | b'+' | b'.' | b')') {
                    e += 1;
                }
                set(r.start..skip_spaces(b, e).min(r.end), MARKUP, &mut f);
                task_item = Some(r);
            }
            Event::TaskListMarker(done) => {
                set(r.clone(), MARKUP, &mut f);
                if done && let Some(item) = task_item.take() {
                    set(r.end..line_end(b, item.start), TASK_DONE, &mut f);
                }
            }
            Event::Rule => set(r, MARKUP, &mut f),
            Event::Html(_) | Event::InlineHtml(_) => set(r, MARKUP, &mut f),
            Event::Start(Tag::TableHead) => {
                set(r.clone(), STRONG, &mut f);
                // The delimiter row follows the head row.
                let row_end = line_end(b, r.start);
                let delim_end = line_end(b, (row_end + 1).min(b.len()));
                set(row_end..delim_end, MARKUP, &mut f);
            }
            _ => {}
        }
    }

    // Pipes in table rows, and iA-style `//` comment lines.
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("// ") || t.trim_end() == "//" {
            set(start..start + line.trim_end_matches('\n').len(), COMMENT | MARKUP, &mut f);
        } else if f.get(start).is_none_or(|x| x & CODEBLOCK == 0) {
            scan_inline(text, start..start + line.len(), &mut f);
        }
        if t.starts_with('|') {
            for (i, c) in line.char_indices() {
                if c == '|' && f[start + i] & CODE == 0 {
                    f[start + i] |= MARKUP;
                }
            }
        }
        start += line.len();
    }
    f
}

/// `==highlight==`, `#hashtag`, `[[wikilink]]` on one source line, outside code.
fn scan_inline(text: &str, r: Range<usize>, f: &mut [Flags]) {
    let s = &text[r.clone()];
    let mut set = |a: usize, z: usize, flag: Flags| {
        if f[r.start + a..r.start + z].iter().any(|x| x & CODE != 0) {
            return;
        }
        for x in &mut f[r.start + a..r.start + z] {
            *x |= flag;
        }
    };
    let mut from = 0;
    while let Some(i) = s[from..].find("==") {
        let a = from + i;
        let Some(j) = s[a + 2..].find("==") else { break };
        let z = a + 2 + j + 2;
        if j > 0 {
            set(a, z, HIGHLIGHT);
            set(a, a + 2, MARKUP);
            set(z - 2, z, MARKUP);
        }
        from = z;
    }
    from = 0;
    while let Some(i) = s[from..].find("[[") {
        let a = from + i;
        let Some(j) = s[a..].find("]]") else { break };
        let z = a + j + 2;
        set(a, z, LINK);
        set(a, a + 2, MARKUP);
        set(z - 2, z, MARKUP);
        from = z;
    }
    let bytes = s.as_bytes();
    for (i, c) in s.char_indices() {
        let starts_tag = c == '#'
            && (i == 0 || bytes[i - 1].is_ascii_whitespace())
            && s[i + 1..].chars().next().is_some_and(|n| n.is_alphanumeric());
        if starts_tag {
            let len = s[i + 1..]
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '-' || ch == '/'))
                .unwrap_or(s.len() - i - 1);
            set(i, i + 1 + len, HASHTAG);
        }
    }
}

fn count(b: &[u8], from: usize, c: u8) -> usize {
    b[from..].iter().take_while(|&&x| x == c).count()
}

fn skip_spaces(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    i
}

fn line_end(b: &[u8], from: usize) -> usize {
    b[from..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(b.len(), |i| from + i)
}

/// Bytes of heading markup (`## `) at the start of a line; these hang in the left margin.
pub fn hanging_prefix(flags: &[Flags], line: Range<usize>) -> usize {
    flags[line.clone()]
        .iter()
        .take_while(|&&x| x & HEADING != 0 && x & MARKUP != 0)
        .count()
}

/// Title for the library and window: first heading, else first non-empty line.
pub fn title(text: &str) -> String {
    let first = text
        .lines()
        .map(str::trim)
        // `// ` comment lines (e.g. template guidance) are not the title.
        .find(|l| !l.is_empty() && !l.starts_with("// ") && *l != "//")
        .unwrap_or("");
    first.trim_start_matches('#').trim().to_string()
}

/// Plain-text excerpt (markup stripped) for the library list. Like the
/// original it keeps headings, and shows content blocks (`/file.md`) without the slash.
pub fn excerpt(text: &str, max_chars: usize) -> String {
    let cleaned: String = text
        .lines()
        .map(|l| {
            let t = l.trim();
            // A content block is a line that is just "/path.ext".
            let is_block = t.starts_with('/') && !t.starts_with("//") && t.contains('.');
            if !is_block {
                return l.to_string();
            }
            let rest = &t[1..];
            // Keep "1. Name.txt" literal instead of turning it into a list.
            let digits = rest.chars().take_while(char::is_ascii_digit).count();
            if digits > 0 && rest[digits..].starts_with('.') {
                format!("{}\\{}", &rest[..digits], &rest[digits..])
            } else {
                rest.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = String::new();
    for ev in Parser::new_ext(&cleaned, options()) {
        match ev {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::SoftBreak
            | Event::HardBreak
            | Event::End(TagEnd::Paragraph | TagEnd::TableCell | TagEnd::Item | TagEnd::CodeBlock | TagEnd::Heading(_)) => out.push(' '),
            _ => {}
        }
        if out.len() > max_chars * 4 {
            break;
        }
    }
    let s = out.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(max_chars).collect()
}

/// Shorten in the middle like macOS lists: "3. A Caucus…Long Tale.txt".
pub fn truncate_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max || max < 5 {
        return s.to_string();
    }
    let keep = max - 3;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let chars: Vec<char> = s.chars().collect();
    format!("{}...{}", chars[..head].iter().collect::<String>(), chars[n - tail..].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags_of(text: &str, needle: &str) -> Flags {
        let f = style(text);
        let i = text.find(needle).unwrap();
        f[i]
    }

    #[test]
    fn heading_marker_is_markup_and_hangs() {
        let t = "## Title\nbody";
        let f = style(t);
        assert_eq!(f[0] & (HEADING | MARKUP), HEADING | MARKUP);
        assert_eq!(f[3] & MARKUP, 0);
        assert_ne!(f[3] & HEADING, 0);
        assert_eq!(hanging_prefix(&f, 0..8), 3);
        assert_eq!(hanging_prefix(&f, 9..13), 0);
    }

    #[test]
    fn emphasis_strong_code() {
        let t = "a *em* **st** `co` ~~x~~";
        assert_eq!(flags_of(t, "*em"), EMPH | MARKUP);
        assert_eq!(flags_of(t, "em*"), EMPH);
        assert_eq!(flags_of(t, "st*"), STRONG);
        assert_eq!(flags_of(t, "co"), CODE);
        assert_eq!(flags_of(t, "`co") & MARKUP, MARKUP);
        assert_ne!(flags_of(t, "x~") & STRIKE, 0);
    }

    #[test]
    fn link_text_plain_url_dimmed() {
        let t = "see [iA](https://ia.net) now";
        assert_eq!(flags_of(t, "iA]"), LINK);
        assert_ne!(flags_of(t, "https") & (URL | MARKUP), 0);
        assert_eq!(flags_of(t, "[iA") & MARKUP, MARKUP);
    }

    #[test]
    fn lists_quotes_tasks_highlight_tags() {
        let t = "- [x] done\n\n> quote\n\n==hi== #tag [[Page]]";
        let f = style(t);
        assert_ne!(f[0] & MARKUP, 0);
        assert_ne!(f[t.find("done").unwrap()] & TASK_DONE, 0);
        assert_ne!(f[t.find('>').unwrap()] & MARKUP, 0);
        assert_ne!(f[t.find("quote").unwrap()] & QUOTE, 0);
        assert_eq!(f[t.find("hi").unwrap()], HIGHLIGHT);
        assert_ne!(f[t.find("tag").unwrap()] & HASHTAG, 0);
        assert_eq!(f[t.find("Page").unwrap()], LINK);
    }

    #[test]
    fn fenced_code_and_comment() {
        let t = "```rust\nlet x;\n```\n// note";
        let f = style(t);
        assert_ne!(f[0] & MARKUP, 0);
        assert_eq!(f[t.find("let").unwrap()] & MARKUP, 0);
        assert_ne!(f[t.find("let").unwrap()] & CODEBLOCK, 0);
        assert_ne!(f[t.find("note").unwrap()] & COMMENT, 0);
    }

    #[test]
    fn title_and_excerpt() {
        let t = "# My *Doc*\n\nFirst **para** here.\n\nSecond.";
        assert_eq!(title(t), "My *Doc*");
        assert_eq!(excerpt(t, 100), "My Doc First para here. Second.");
        assert_eq!(excerpt(t, 6), "My Doc");
        assert_eq!(excerpt("# T\n## Sub\nBody", 50), "T Sub Body");
        assert_eq!(excerpt("# A\n\n/1. Part.txt\n", 50), "A 1. Part.txt");
        assert_eq!(truncate_middle("3. A Caucus-Race and a Long Tale.txt", 26), "3. A Caucus-...ng Tale.txt");
        assert_eq!(truncate_middle("short.md", 26), "short.md");
        assert_eq!(excerpt("# T\n| a | b |\n|---|---|\n| 1 | 2 |", 50), "T a b 1 2");
    }
}
