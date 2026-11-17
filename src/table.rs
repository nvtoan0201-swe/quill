//! Format › Edit Table: operations on the Markdown table under the caret.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::format::Change;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Reformat,
    RowAbove,
    RowBelow,
    ColumnBefore,
    ColumnAfter,
    DeleteRow,
    DeleteColumn,
    Align(Align),
    SortAscending,
    SortDescending,
    MoveRowUp,
    MoveRowDown,
    MoveColumnLeft,
    MoveColumnRight,
    Transpose,
    Merge,
    Unmerge,
}

struct Table {
    range: Range<usize>,
    head: Vec<String>,
    align: Vec<Align>,
    body: Vec<Vec<String>>,
    /// Caret position: row (0 = head, 1.. = body) and column.
    row: usize,
    col: usize,
}

/// A cell covered by the colspan of the one before it (MultiMarkdown `||`).
const MERGED: &str = "\u{1}";

fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|')
        .enumerate()
        .map(|(i, c)| if c.is_empty() && i > 0 { MERGED.to_string() } else { c.trim().to_string() })
        .collect()
}

fn is_delimiter(line: &str) -> bool {
    let c = cells(line);
    !c.is_empty() && c.iter().all(|x| !x.is_empty() && x.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn parse(text: &str, caret: usize) -> Option<Table> {
    let mut lines: Vec<Range<usize>> = Vec::new();
    let mut start = 0;
    for l in text.split_inclusive('\n') {
        lines.push(start..start + l.trim_end_matches('\n').len());
        start += l.len();
    }
    let cur = lines.iter().position(|r| caret <= r.end)?;
    let is_row = |i: usize| text[lines[i].clone()].trim_start().starts_with('|');
    if !is_row(cur) {
        return None;
    }
    let mut a = cur;
    while a > 0 && is_row(a - 1) {
        a -= 1;
    }
    let mut z = cur;
    while z + 1 < lines.len() && is_row(z + 1) {
        z += 1;
    }
    if z - a < 1 || !is_delimiter(&text[lines[a + 1].clone()]) {
        return None;
    }
    let head = cells(&text[lines[a].clone()]);
    let align = cells(&text[lines[a + 1].clone()])
        .iter()
        .map(|d| match (d.starts_with(':'), d.ends_with(':')) {
            (true, true) => Align::Center,
            (true, false) => Align::Left,
            (false, true) => Align::Right,
            _ => Align::None,
        })
        .collect();
    let body = (a + 2..=z).map(|i| cells(&text[lines[i].clone()])).collect();
    let row = match cur - a {
        0 | 1 => 0,
        n => n - 1,
    };
    let line = &text[lines[cur].start..caret];
    let col = line.matches('|').count().saturating_sub(usize::from(line.trim_start().starts_with('|')));
    let mut t = Table { range: lines[a].start..lines[z].end, head, align, body, row, col };
    t.normalize();
    t.col = t.col.min(t.width() - 1);
    Some(t)
}

impl Table {
    fn width(&self) -> usize {
        self.head.len().max(1)
    }

    fn normalize(&mut self) {
        let w = self.body.iter().map(Vec::len).chain([self.head.len(), self.align.len()]).max().unwrap_or(1).max(1);
        self.head.resize(w, String::new());
        self.align.resize(w, Align::None);
        for r in &mut self.body {
            r.resize(w, String::new());
        }
    }

    fn render(&self) -> (String, usize) {
        let w = self.width();
        let width_of = |s: &str| if s == MERGED { 0 } else { s.graphemes(true).count() };
        let mut widths = vec![3usize; w];
        for r in std::iter::once(&self.head).chain(&self.body) {
            for (i, c) in r.iter().enumerate() {
                widths[i] = widths[i].max(width_of(c));
            }
        }
        let pad = |s: &str, i: usize| {
            let n = widths[i].saturating_sub(width_of(s));
            match self.align[i] {
                Align::Right => format!("{}{s}", " ".repeat(n)),
                Align::Center => format!("{}{s}{}", " ".repeat(n / 2), " ".repeat(n - n / 2)),
                _ => format!("{s}{}", " ".repeat(n)),
            }
        };
        let row = |r: &Vec<String>| {
            let mut out = String::from("|");
            for (i, c) in r.iter().enumerate() {
                if c == MERGED {
                    out.push('|');
                } else {
                    out.push_str(&format!(" {} |", pad(c, i)));
                }
            }
            out
        };
        let delim = format!(
            "| {} |",
            (0..w)
                .map(|i| {
                    let d = "-".repeat(widths[i]);
                    match self.align[i] {
                        Align::Left => format!(":{}", &d[1..]),
                        Align::Right => format!("{}:", &d[1..]),
                        Align::Center => format!(":{}:", &d[2..]),
                        Align::None => d,
                    }
                })
                .collect::<Vec<_>>()
                .join(" | ")
        );
        let mut lines = vec![row(&self.head), delim];
        lines.extend(self.body.iter().map(row));
        // Caret goes to the start of the current cell's text.
        let line_ix = if self.row == 0 { 0 } else { self.row + 1 };
        let before: usize = lines[..line_ix].iter().map(|l| l.len() + 1).sum();
        let col = self.col.min(w - 1);
        let in_line = 2 + (0..col).map(|i| pad("", i).len() + 3).sum::<usize>();
        let caret = before + in_line;
        (lines.join("\n"), caret)
    }
}

pub fn edit(text: &str, caret: usize, op: Op) -> Option<Change> {
    let mut t = parse(text, caret)?;
    let w = t.width();
    let body_ix = t.row.checked_sub(1);
    match op {
        Op::Reformat => {}
        Op::RowAbove => {
            let at = body_ix.unwrap_or(0);
            t.body.insert(at, vec![String::new(); w]);
            t.row = at + 1;
        }
        Op::RowBelow => {
            let at = body_ix.map_or(0, |b| b + 1);
            t.body.insert(at, vec![String::new(); w]);
            t.row = at + 1;
        }
        Op::ColumnBefore | Op::ColumnAfter => {
            let at = if op == Op::ColumnBefore { t.col } else { t.col + 1 };
            t.head.insert(at, String::new());
            t.align.insert(at, Align::None);
            for r in &mut t.body {
                r.insert(at, String::new());
            }
            t.col = at;
        }
        Op::DeleteRow => {
            if let Some(b) = body_ix {
                t.body.remove(b);
                t.row = t.row.min(t.body.len());
            }
        }
        Op::DeleteColumn => {
            if w > 1 {
                t.head.remove(t.col);
                t.align.remove(t.col);
                for r in &mut t.body {
                    r.remove(t.col);
                }
                t.col = t.col.min(w - 2);
            }
        }
        Op::Align(a) => t.align[t.col] = a,
        Op::SortAscending | Op::SortDescending => {
            let c = t.col;
            let key = |r: &Vec<String>| {
                let s = r[c].to_lowercase();
                (s.parse::<f64>().ok(), s)
            };
            t.body.sort_by(|a, b| {
                let (ka, kb) = (key(a), key(b));
                match (ka.0, kb.0) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                    _ => ka.1.cmp(&kb.1),
                }
            });
            if op == Op::SortDescending {
                t.body.reverse();
            }
        }
        Op::MoveRowUp => {
            if let Some(b) = body_ix.filter(|&b| b > 0) {
                t.body.swap(b, b - 1);
                t.row -= 1;
            }
        }
        Op::MoveRowDown => {
            if let Some(b) = body_ix.filter(|&b| b + 1 < t.body.len()) {
                t.body.swap(b, b + 1);
                t.row += 1;
            }
        }
        Op::MoveColumnLeft | Op::MoveColumnRight => {
            let other = if op == Op::MoveColumnLeft { t.col.checked_sub(1) } else { (t.col + 1 < w).then_some(t.col + 1) };
            if let Some(o) = other {
                t.head.swap(t.col, o);
                t.align.swap(t.col, o);
                for r in &mut t.body {
                    r.swap(t.col, o);
                }
                t.col = o;
            }
        }
        Op::Merge | Op::Unmerge => {
            let row = if t.row == 0 { &mut t.head } else { &mut t.body[t.row - 1] };
            let c = t.col;
            if op == Op::Merge {
                // Absorb the next visible cell into this one.
                if let Some(next) = (c + 1..w).find(|&i| row[i] != MERGED) {
                    let moved = std::mem::replace(&mut row[next], MERGED.to_string());
                    if !moved.is_empty() {
                        let sep = if row[c].is_empty() { "" } else { " " };
                        row[c] = format!("{}{sep}{moved}", row[c]);
                    }
                }
            } else {
                for cell in row[c + 1..].iter_mut().take_while(|x| x.as_str() == MERGED) {
                    cell.clear();
                }
            }
        }
        Op::Transpose => {
            let rows: Vec<Vec<String>> = std::iter::once(t.head.clone()).chain(t.body.clone()).collect();
            let cols = rows.len();
            let new: Vec<Vec<String>> = (0..w).map(|c| rows.iter().map(|r| r[c].clone()).collect()).collect();
            t.head = new[0].clone();
            t.body = new[1..].to_vec();
            t.align = vec![Align::None; cols];
            (t.row, t.col) = (t.col, t.row);
        }
    }
    t.normalize();
    let (rendered, caret_rel) = t.render();
    let c = t.range.start + caret_rel;
    Some(Change { range: t.range.clone(), text: rendered, select: c..c })
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "x\n| a | b |\n|---|--:|\n| 2 | y |\n| 1 | z |\nafter";

    fn apply(t: &str, c: &Change) -> String {
        format!("{}{}{}", &t[..c.range.start], c.text, &t[c.range.end..])
    }

    #[test]
    fn reformat_keeps_alignment_and_surroundings() {
        let at = T.find("| 2").unwrap() + 2;
        let c = edit(T, at, Op::Reformat).unwrap();
        assert_eq!(apply(T, &c), "x\n| a   |   b |\n| --- | --: |\n| 2   |   y |\n| 1   |   z |\nafter");
        assert_eq!(&apply(T, &c)[c.select.start..c.select.start + 1], "2");
        assert!(edit("no table", 1, Op::Reformat).is_none());
    }

    #[test]
    fn rows_columns_sort_transpose() {
        let at = T.find("| 2").unwrap() + 2;
        let rows = |s: &str| s.lines().filter(|l| l.starts_with('|')).count();
        assert_eq!(rows(&apply(T, &edit(T, at, Op::RowBelow).unwrap())), 5);
        assert_eq!(rows(&apply(T, &edit(T, at, Op::DeleteRow).unwrap())), 3);
        let wide = apply(T, &edit(T, at, Op::ColumnAfter).unwrap());
        assert!(wide.contains("| a   |     |   b |"));
        let sorted = apply(T, &edit(T, at, Op::SortAscending).unwrap());
        assert!(sorted.find("| 1").unwrap() < sorted.find("| 2").unwrap());
        let moved = apply(T, &edit(T, at, Op::MoveRowDown).unwrap());
        assert!(moved.find("| 1").unwrap() < moved.find("| 2").unwrap());
        let tr = apply(T, &edit(T, at, Op::Transpose).unwrap());
        assert!(tr.contains("| a   | 2   | 1   |"), "{tr}");
        let merged = apply(T, &edit(T, at, Op::Merge).unwrap());
        assert!(merged.contains("| 2 y ||"), "{merged}");
        let at2 = merged.find("| 2 y").unwrap() + 2;
        assert!(edit(&merged, at2, Op::Reformat).unwrap().text.contains("| 2 y ||"));
        let unmerged = apply(&merged, &edit(&merged, at2, Op::Unmerge).unwrap());
        assert!(unmerged.contains("| 2 y |     |"), "{unmerged}");
        let centered = apply(T, &edit(T, at, Op::Align(Align::Center)).unwrap());
        assert!(centered.contains("| :-: |"));
    }
}
