//! The writing surface: a custom GPUI element that shapes the whole document,
//! hangs heading markers in the left margin, and paints selection and caret.

use std::ops::Range;
use std::path::PathBuf;
use std::time::Instant;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    FontStyle, FontWeight, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    ScrollWheelEvent, SharedString, StrikethroughStyle, Style, TextAlign, TextRun,
    UTF16Selection, UnderlineStyle, Window, WrappedLine, div, fill, font, point, prelude::*, px,
    relative, size,
};

use crate::actions::*;
use crate::authors::{Author, Authors};
use crate::buffer::{Buffer, EditKind};
use crate::focus::{self, FocusMode};
use crate::format::{self, Case, Change, Prefix};
use crate::markdown::{self, Flags};
use crate::style_check::{self, Check};
use crate::syntax::{self, Pos};
use crate::table;
use crate::theme::Theme;

#[derive(Clone, PartialEq, Debug)]
pub struct EditorConfig {
    pub font_family: SharedString,
    pub font_size: f32,
    pub line_height: f32,
    pub focus: FocusMode,
    pub typewriter: bool,
    pub syntax: Vec<Pos>,
    pub checks: Vec<Check>,
    pub custom_patterns: Vec<String>,
    pub show_authors: bool,
    pub smart_quotes: bool,
    pub smart_dashes: bool,
    pub auto_pair: bool,
    pub spell: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            font_family: crate::fonts::DUO.into(),
            font_size: 18.,
            line_height: 32.,
            focus: FocusMode::Off,
            typewriter: false,
            syntax: Vec::new(),
            checks: Vec::new(),
            custom_patterns: Vec::new(),
            show_authors: false,
            smart_quotes: false,
            smart_dashes: true,
            auto_pair: true,
            spell: false,
        }
    }
}

pub enum EditorEvent {
    Edited,
    ContextMenu(Point<Pixels>),
}

#[derive(Default)]
pub struct FindState {
    pub query: String,
    pub matches: Vec<Range<usize>>,
    pub current: Option<usize>,
}

struct Analysis {
    version: u64,
    key: String,
    flags: Vec<Flags>,
    /// 0 = none, else Pos index + 1.
    pos: Vec<u8>,
    check: Vec<bool>,
    spell: Vec<bool>,
    /// Writing Studio marks: 0 none, 1 mistake, 2 style advice.
    studio: Vec<u8>,
}

struct LineInfo {
    range: Range<usize>,
    top: Pixels,
    height: Pixels,
    hang: Pixels,
    rows: Vec<usize>,
    shaped: WrappedLine,
}

struct Layout {
    bounds: Bounds<Pixels>,
    text_x: Pixels,
    top_pad: Pixels,
    line_height: Pixels,
    lines: Vec<LineInfo>,
    content_height: Pixels,
}

#[derive(Clone, Copy, PartialEq)]
enum DragUnit {
    Char,
    Word,
    Line,
}

pub struct Editor {
    pub buffer: Buffer,
    pub path: Option<PathBuf>,
    pub authors: Authors,
    pub config: EditorConfig,
    pub find: Option<FindState>,
    pub last_input: Instant,
    /// Misspelled words from the last background check.
    pub misspelled: std::sync::Arc<std::collections::HashSet<String>>,
    misspelled_gen: u64,
    /// Writing Studio findings as (range, is_mistake), valid for `studio_version`.
    studio_marks: Vec<(Range<usize>, bool)>,
    studio_version: u64,
    studio_gen: u64,
    focus_handle: FocusHandle,
    scroll_y: Pixels,
    autoscroll: bool,
    goal_x: Option<Pixels>,
    layout: Option<Layout>,
    analysis: Option<Analysis>,
    runs_cache: Option<(String, Vec<TextRun>)>,
    drag: Option<(DragUnit, Range<usize>)>,
    /// IME: text the composition replaced, restored before the final commit.
    composition_base: Option<(usize, String)>,
}

impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Editor {
    pub fn new(text: &str, path: Option<PathBuf>, config: EditorConfig, cx: &mut Context<Self>) -> Self {
        let authors = path
            .as_deref()
            .map(|p| Authors::load(p, text.len()))
            .unwrap_or_default();
        Self {
            buffer: Buffer::new(text),
            path,
            authors,
            config,
            find: None,
            last_input: Instant::now() - std::time::Duration::from_secs(60),
            misspelled: Default::default(),
            misspelled_gen: 0,
            studio_marks: Vec::new(),
            studio_version: u64::MAX,
            studio_gen: 0,
            focus_handle: cx.focus_handle(),
            scroll_y: px(0.),
            autoscroll: true,
            goal_x: None,
            layout: None,
            analysis: None,
            runs_cache: None,
            drag: None,
            composition_base: None,
        }
    }

    pub fn set_config(&mut self, config: EditorConfig, cx: &mut Context<Self>) {
        if self.config != config {
            self.config = config;
            self.autoscroll = true;
            cx.notify();
        }
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => crate::library::file_stem(p),
            None => {
                let t = markdown::title(self.buffer.text());
                if t.is_empty() { "Untitled".into() } else { t.chars().take(60).collect() }
            }
        }
    }

    // ---- editing core -------------------------------------------------------

    fn edit(&mut self, range: Range<usize>, text: &str, kind: EditKind, author: Author, cx: &mut Context<Self>) {
        self.authors.edit(range.clone(), text.len(), author);
        let current = self.studio_version == self.buffer.version();
        self.buffer.replace(range.clone(), text, kind);
        self.shift_studio(current, range, text.len());
        self.after_edit(cx);
    }

    /// Keep Studio underlines in place while typing; the next analysis replaces them.
    fn shift_studio(&mut self, current: bool, range: Range<usize>, new_len: usize) {
        if !current {
            return;
        }
        let delta = new_len as isize - range.len() as isize;
        self.studio_marks.retain_mut(|(r, _)| {
            if r.end <= range.start {
                true
            } else if r.start >= range.end {
                *r = (r.start as isize + delta) as usize..(r.end as isize + delta) as usize;
                true
            } else {
                false
            }
        });
        self.studio_version = self.buffer.version();
        self.studio_gen += 1;
    }

    pub fn set_studio_marks(&mut self, marks: Vec<(Range<usize>, bool)>, version: u64, cx: &mut Context<Self>) {
        if version != self.buffer.version() {
            return;
        }
        if self.studio_marks != marks || self.studio_version != version {
            self.studio_marks = marks;
            self.studio_version = version;
            self.studio_gen += 1;
            cx.notify();
        }
    }

    pub fn select_range(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let n = self.buffer.len();
        self.select(range.start.min(n), range.end.min(n), cx);
    }

    pub fn selected_text(&self) -> String {
        let r = self.buffer.selection();
        self.buffer.text()[r].to_string()
    }

    fn apply(&mut self, c: Change, cx: &mut Context<Self>) {
        self.authors.edit(c.range.clone(), c.text.len(), Author::Human);
        let current = self.studio_version == self.buffer.version();
        let (range, len) = (c.range.clone(), c.text.len());
        self.buffer.replace(c.range, &c.text, EditKind::Other);
        self.shift_studio(current, range, len);
        self.buffer.set_selection(c.select.start, c.select.end);
        self.after_edit(cx);
    }

    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        self.goal_x = None;
        self.last_input = Instant::now();
        if self.find.is_some() {
            self.refresh_find();
        }
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    pub fn insert_text(&mut self, text: &str, author: Author, cx: &mut Context<Self>) {
        let sel = self.buffer.selection();
        let kind = if text.chars().count() == 1 { EditKind::Typing } else { EditKind::Other };
        self.edit(sel, text, kind, author, cx);
    }


    fn select(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        self.buffer.set_selection(anchor, head);
        self.autoscroll = true;
        cx.notify();
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.select(offset, offset, cx);
    }

    fn extend_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let anchor = self.buffer.anchor();
        self.select(anchor, offset, cx);
    }



    // ---- analysis -----------------------------------------------------------

    fn analysis_key(&self) -> String {
        format!(
            "{:?}|{:?}|{:?}|{}|{}|{}|{}",
            self.config.syntax, self.config.checks, self.config.custom_patterns, self.config.spell, self.misspelled_gen, self.studio_gen,
            self.studio_version == self.buffer.version()
        )
    }

    pub fn set_misspelled(&mut self, words: std::collections::HashSet<String>, cx: &mut Context<Self>) {
        if *self.misspelled != words {
            self.misspelled = std::sync::Arc::new(words);
            self.misspelled_gen += 1;
            cx.notify();
        }
    }

    fn ensure_analysis(&mut self) {
        let key = self.analysis_key();
        let version = self.buffer.version();
        if self.analysis.as_ref().is_some_and(|a| a.version == version && a.key == key) {
            return;
        }
        let text = self.buffer.text();
        let flags = markdown::style(text);
        let mut pos = vec![0u8; text.len()];
        if !self.config.syntax.is_empty() {
            for (r, p) in syntax::tag(text) {
                if self.config.syntax.contains(&p) && flags[r.start] & (markdown::CODE | markdown::CODEBLOCK | markdown::URL) == 0 {
                    let idx = Pos::ALL.iter().position(|x| *x == p).unwrap() as u8 + 1;
                    pos[r].fill(idx);
                }
            }
        }
        let mut check = vec![false; text.len()];
        for (r, _) in style_check::check(text, &self.config.checks, &self.config.custom_patterns) {
            check[r].fill(true);
        }
        let mut spell = vec![false; text.len()];
        if self.config.spell && !self.misspelled.is_empty() {
            use unicode_segmentation::UnicodeSegmentation;
            let quiet = markdown::CODE | markdown::CODEBLOCK | markdown::URL | markdown::MARKUP | markdown::COMMENT;
            for (i, w) in text.unicode_word_indices() {
                if self.misspelled.contains(w) && flags[i] & quiet == 0 {
                    spell[i..i + w.len()].fill(true);
                }
            }
        }
        let mut studio = vec![0u8; text.len()];
        if self.studio_version == version {
            for (r, mistake) in &self.studio_marks {
                if r.end <= text.len() {
                    studio[r.clone()].fill(if *mistake { 1 } else { 2 });
                }
            }
        }
        self.analysis = Some(Analysis { version, key, flags, pos, check, spell, studio });
    }

    /// Window position just below the caret (for popups such as completions).
    pub fn caret_anchor(&self) -> Option<Point<Pixels>> {
        let layout = self.layout.as_ref()?;
        let head = self.buffer.head();
        let (li, _) = locate(layout, head)?;
        let (x, y) = caret_point(layout, head)?;
        let origin = self.line_origin(&layout.lines[li]);
        Some(point(origin.x + x, layout.bounds.top() + layout.top_pad + y - self.scroll_y + layout.line_height))
    }

    pub fn replace_range(&mut self, range: Range<usize>, text: &str, cx: &mut Context<Self>) {
        self.edit(range, text, EditKind::Other, Author::Human, cx);
    }

    pub fn focus_range(&self) -> Option<Range<usize>> {
        focus::focus_range(self.buffer.text(), self.buffer.head(), self.config.focus)
    }

    fn build_runs(&mut self, theme: &Theme) -> Vec<TextRun> {
        self.ensure_analysis();
        let focus = self.focus_range();
        let key = format!(
            "{}|{}|{:?}|{}|{}|{}|{:?}",
            self.buffer.version(),
            self.analysis_key(),
            focus,
            theme.dark,
            self.config.font_family,
            self.config.show_authors,
            self.authors.ai
        );
        if let Some((k, runs)) = &self.runs_cache
            && *k == key
        {
            return runs.clone();
        }
        let a = self.analysis.as_ref().unwrap();
        let text = self.buffer.text();
        let n = text.len();
        let family = self.config.font_family.clone();
        let mut ai = vec![false; n];
        if self.config.show_authors {
            for r in &self.authors.ai {
                ai[r.start.min(n)..r.end.min(n)].fill(true);
            }
        }
        let style_key = |i: usize| -> (Flags, u8, u8, bool, bool) {
            let dim = focus.as_ref().is_some_and(|f| !f.contains(&i) && !(f.is_empty() && f.start == i));
            let underline = if a.spell[i] {
                2
            } else if a.check[i] {
                1
            } else if a.studio[i] > 0 {
                2 + a.studio[i]
            } else {
                0
            };
            (a.flags[i], a.pos[i], underline, dim, ai[i])
        };
        let make = |len: usize, k: (Flags, u8, u8, bool, bool)| -> TextRun {
            let (f, p, underline, dim, is_ai) = k;
            let mut ft = font(family.clone());
            if f & (markdown::STRONG | markdown::HEADING) != 0 {
                ft.weight = FontWeight::BOLD;
            }
            if f & markdown::EMPH != 0 {
                ft.style = FontStyle::Italic;
            }
            let quiet = f & (markdown::MARKUP | markdown::URL | markdown::COMMENT) != 0;
            let in_focus = focus.is_some() && !dim;
            let color = if dim {
                theme.dimmed
            } else if f & (markdown::URL | markdown::COMMENT) != 0 || f & markdown::TASK_DONE != 0 {
                theme.secondary
            } else if quiet {
                theme.markup
            } else if is_ai {
                theme.ai_text
            } else if f & markdown::HIGHLIGHT != 0 {
                theme.highlight_text
            } else if p > 0 {
                theme.pos(Pos::ALL[p as usize - 1])
            } else if in_focus {
                theme.focus_text
            } else {
                theme.text
            };
            let strike = f & (markdown::STRIKE | markdown::TASK_DONE) != 0 && !quiet;
            TextRun {
                len,
                font: ft,
                color,
                background_color: (f & markdown::HIGHLIGHT != 0 && !quiet && !dim).then_some(theme.highlight),
                underline: match (underline, dim) {
                    (1, false) => Some(UnderlineStyle { thickness: px(2.), color: Some(theme.check), wavy: false }),
                    (2, false) => Some(UnderlineStyle { thickness: px(1.5), color: Some(theme.misspelled), wavy: true }),
                    (3, false) => Some(UnderlineStyle { thickness: px(1.5), color: Some(theme.studio_mistake), wavy: true }),
                    (4, false) => Some(UnderlineStyle { thickness: px(1.), color: Some(theme.studio_style), wavy: false }),
                    // mark.css: a 0.175 em bar under highlighted text.
                    _ if f & markdown::HIGHLIGHT != 0 && !quiet && !dim => Some(UnderlineStyle {
                        thickness: px(self.config.font_size * 0.175),
                        color: Some(theme.highlight_underline),
                        wavy: false,
                    }),
                    _ => None,
                },
                strikethrough: strike.then_some(StrikethroughStyle {
                    thickness: px(1.5),
                    color: Some(color),
                }),
            }
        };
        let mut runs = Vec::new();
        let mut start = 0;
        let bytes = text.as_bytes();
        while start < n {
            let k = style_key(start);
            let mut end = start + 1;
            while end < n && (bytes[end] & 0xC0 == 0x80 || style_key(end) == k) {
                end += 1;
            }
            runs.push(make(end - start, k));
            start = end;
        }
        if runs.is_empty() {
            runs.push(make(0, (0, 0, 0, false, false)));
        }
        self.runs_cache = Some((key, runs.clone()));
        runs
    }

    // ---- layout -------------------------------------------------------------

    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let theme = *cx.global::<Theme>();
        let runs = self.build_runs(&theme);
        let font_size = px(self.config.font_size);
        let lh = px(self.config.line_height);

        // Measure an average character to size the column like iA (~64 chars).
        let probe_run = TextRun { len: 10, ..runs[0].clone() };
        let probe = window
            .text_system()
            .shape_line("nnnnnnnnnn".into(), font_size, &[TextRun { background_color: None, underline: None, strikethrough: None, ..probe_run }], None);
        let ch = probe.width / 10.;
        // TypographyKit MarginCharacterCounts: ≤512w 3, ≤768w 4, wider 6, wide/full 8.
        let w = bounds.size.width;
        let margin = ch * if w <= px(512.) { 3. } else if w <= px(768.) { 4. } else if w <= px(1280.) { 6. } else { 8. };
        let wrap_width = (ch * 64.).min(bounds.size.width - margin * 2.).max(ch * 10.);
        let text_x = bounds.left() + (bounds.size.width - wrap_width) / 2.;

        let text: SharedString = self.buffer.text().to_string().into();
        let shaped = window
            .text_system()
            .shape_text(text, font_size, &runs, Some(wrap_width), None)
            .unwrap_or_default();

        let a = self.analysis.as_ref().unwrap();
        let mut lines = Vec::with_capacity(shaped.len());
        let mut top = px(0.);
        let mut start = 0;
        for line in shaped {
            let len = line.len();
            let range = start..start + len;
            let hang_bytes = if len > 0 { markdown::hanging_prefix(&a.flags, range.clone()) } else { 0 };
            let hang = if hang_bytes > 0 { line.unwrapped_layout.x_for_index(hang_bytes) } else { px(0.) };
            let mut rows = vec![0];
            for b in line.wrap_boundaries() {
                let ix = line.unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix].index;
                // A trailing space can produce an empty last row; keep the caret on the text row.
                if ix < len {
                    rows.push(ix);
                }
            }
            let height = lh * rows.len() as f32;
            lines.push(LineInfo { range, top, height, hang, rows, shaped: line });
            top += height;
            start += len + 1;
        }

        let top_pad = if self.config.typewriter {
            (bounds.size.height - lh) / 2.
        } else {
            // About two lines below the toolbar, as in the original.
            lh * 2.
        };
        let bottom_pad = bounds.size.height / 2.;
        let layout = Layout { bounds, text_x, top_pad, line_height: lh, lines, content_height: top };

        // Scrolling: typewriter keeps the caret line centred; otherwise keep it visible.
        if let Some((_, y)) = caret_point(&layout, self.buffer.head()) {
            let caret_top = top_pad + y;
            if self.config.typewriter {
                self.scroll_y = caret_top - (bounds.size.height - lh) / 2.;
            } else if self.autoscroll {
                let margin = lh * 2.;
                if caret_top - self.scroll_y < margin {
                    self.scroll_y = caret_top - margin;
                } else if caret_top + lh - self.scroll_y > bounds.size.height - margin {
                    self.scroll_y = caret_top + lh - bounds.size.height + margin;
                }
            }
        }
        self.autoscroll = false;
        let max = (top_pad + layout.content_height + bottom_pad - bounds.size.height).max(px(0.));
        self.scroll_y = self.scroll_y.clamp(px(0.), max);
        self.layout = Some(layout);
    }

    fn line_origin(&self, l: &LineInfo) -> Point<Pixels> {
        let layout = self.layout.as_ref().unwrap();
        point(layout.text_x - l.hang, layout.bounds.top() + layout.top_pad + l.top - self.scroll_y)
    }

    /// Byte offset under a window position.
    fn offset_for_point(&self, p: Point<Pixels>) -> usize {
        let Some(layout) = &self.layout else { return 0 };
        let y = p.y - layout.bounds.top() - layout.top_pad + self.scroll_y;
        if layout.lines.is_empty() || y < px(0.) {
            return 0;
        }
        let ix = layout
            .lines
            .partition_point(|l| l.top + l.height <= y)
            .min(layout.lines.len() - 1);
        let l = &layout.lines[ix];
        let row = (((y - l.top) / layout.line_height).floor() as usize).min(l.rows.len() - 1);
        let x = p.x - (layout.text_x - l.hang);
        l.range.start + index_in_row(l, row, x, self.buffer.text())
    }

    fn vertical(&mut self, rows: isize, extend: bool, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else { return };
        let head = self.buffer.head();
        let Some((li, row)) = locate(layout, head) else { return };
        let l = &layout.lines[li];
        let x = self.goal_x.unwrap_or_else(|| x_in_row(l, row, head - l.range.start));
        // Walk `rows` visual rows up or down across line boundaries.
        let (mut li, mut row) = (li, row as isize);
        let mut left = rows;
        while left != 0 {
            if left > 0 {
                if row + 1 < layout.lines[li].rows.len() as isize {
                    row += 1;
                } else if li + 1 < layout.lines.len() {
                    li += 1;
                    row = 0;
                } else {
                    let end = self.buffer.len();
                    return self.finish_vertical(end, x, extend, cx);
                }
                left -= 1;
            } else {
                if row > 0 {
                    row -= 1;
                } else if li > 0 {
                    li -= 1;
                    row = layout.lines[li].rows.len() as isize - 1;
                } else {
                    return self.finish_vertical(0, x, extend, cx);
                }
                left += 1;
            }
        }
        let l = &layout.lines[li];
        let target = l.range.start + index_in_row(l, row as usize, x, self.buffer.text());
        self.finish_vertical(target, x, extend, cx);
    }

    fn finish_vertical(&mut self, target: usize, x: Pixels, extend: bool, cx: &mut Context<Self>) {
        if extend { self.extend_to(target, cx) } else { self.move_to(target, cx) }
        self.goal_x = Some(x);
    }

    fn visual_line_bounds(&self, offset: usize) -> Range<usize> {
        let Some(layout) = &self.layout else { return self.buffer.line_range(offset) };
        let Some((li, row)) = locate(layout, offset) else { return self.buffer.line_range(offset) };
        let l = &layout.lines[li];
        let start = l.range.start + l.rows[row];
        let end = match l.rows.get(row + 1) {
            Some(&next) => l.range.start + prev_boundary(&self.buffer.text()[l.range.clone()], next),
            None => l.range.end,
        };
        start..end
    }

    fn page_rows(&self) -> isize {
        self.layout
            .as_ref()
            .map_or(20, |l| ((l.bounds.size.height / l.line_height) as isize - 2).max(1))
    }

    // ---- find ---------------------------------------------------------------

    pub fn set_find_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let find = self.find.get_or_insert_with(FindState::default);
        find.query = query.to_string();
        self.refresh_find();
        let head = self.buffer.selection().start;
        if let Some(f) = &mut self.find {
            f.current = f.matches.iter().position(|m| m.start >= head).or((!f.matches.is_empty()).then_some(0));
            if let Some(i) = f.current {
                let r = f.matches[i].clone();
                self.buffer.set_selection(r.start, r.end);
                self.autoscroll = true;
            }
        }
        cx.notify();
    }

    fn refresh_find(&mut self) {
        let Some(f) = &mut self.find else { return };
        f.matches = if f.query.is_empty() {
            Vec::new()
        } else {
            style_check::find_substring(self.buffer.text(), &f.query)
        };
        if f.current.is_some_and(|c| c >= f.matches.len()) {
            f.current = None;
        }
    }

    pub fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(f) = &mut self.find else { return };
        if f.matches.is_empty() {
            return;
        }
        let sel = self.buffer.selection();
        let n = f.matches.len();
        let next = if forward {
            f.matches.iter().position(|m| m.start >= sel.end).unwrap_or(0)
        } else {
            f.matches.iter().rposition(|m| m.end <= sel.start).unwrap_or(n - 1)
        };
        f.current = Some(next);
        let r = f.matches[next].clone();
        self.select(r.start, r.end, cx);
    }

    pub fn replace_current(&mut self, with: &str, cx: &mut Context<Self>) {
        let Some(f) = &self.find else { return };
        let sel = self.buffer.selection();
        if f.matches.contains(&sel) {
            self.edit(sel, with, EditKind::Other, Author::Human, cx);
        }
        self.find_step(true, cx);
    }

    pub fn replace_all(&mut self, with: &str, cx: &mut Context<Self>) -> usize {
        let Some(f) = &self.find else { return 0 };
        let matches = f.matches.clone();
        if matches.is_empty() {
            return 0;
        }
        let first = matches[0].start;
        let last = matches.last().unwrap().end;
        let mut out = String::new();
        let mut at = first;
        for m in &matches {
            out.push_str(&self.buffer.text()[at..m.start]);
            out.push_str(with);
            at = m.end;
        }
        self.edit(first..last, &out, EditKind::Other, Author::Human, cx);
        matches.len()
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        cx.notify();
    }

    // ---- utf16 for the IME ---------------------------------------------------

    fn to_utf16(&self, offset: usize) -> usize {
        self.buffer.text()[..offset.min(self.buffer.len())].encode_utf16().count()
    }

    fn from_utf16(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        for (i, c) in self.buffer.text().char_indices() {
            if utf16 >= offset {
                return i;
            }
            utf16 += c.len_utf16();
        }
        self.buffer.len()
    }

    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.from_utf16(r.start)..self.from_utf16(r.end)
    }

    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.to_utf16(r.start)..self.to_utf16(r.end)
    }

    // ---- mouse --------------------------------------------------------------

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let at = self.offset_for_point(ev.position);
        if ev.modifiers.shift {
            self.extend_to(at, cx);
            self.drag = Some((DragUnit::Char, self.buffer.anchor()..self.buffer.anchor()));
            return;
        }
        let (unit, r) = match ev.click_count {
            1 => (DragUnit::Char, at..at),
            2 => (DragUnit::Word, self.buffer.word_at(at)),
            _ => {
                let r = self.buffer.line_range(at);
                (DragUnit::Line, r.start..(r.end + 1).min(self.buffer.len()))
            }
        };
        self.select(r.start, r.end, cx);
        self.drag = Some((unit, r));
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some((unit, origin)) = self.drag.clone() else { return };
        if ev.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let at = self.offset_for_point(ev.position);
        let r = match unit {
            DragUnit::Char => at..at,
            DragUnit::Word => self.buffer.word_at(at),
            DragUnit::Line => self.buffer.line_range(at),
        };
        if r.start < origin.start {
            self.select(origin.end, r.start, cx);
        } else {
            self.select(origin.start, r.end.max(origin.end), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag = None;
    }

    fn on_right_click(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let at = self.offset_for_point(ev.position);
        if !self.buffer.selection().contains(&at) {
            self.move_to(at, cx);
        }
        cx.emit(EditorEvent::ContextMenu(ev.position));
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let d = ev.delta.pixel_delta(px(self.config.line_height));
        self.scroll_y -= d.y;
        if self.config.typewriter {
            // In typewriter mode scrolling moves the caret line instead.
            let rows = (-d.y / px(self.config.line_height)).round() as isize;
            if rows != 0 {
                self.vertical(rows, false, cx);
            }
        }
        cx.notify();
    }

    // ---- actions: movement --------------------------------------------------

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let s = self.buffer.selection();
        let to = if s.is_empty() { self.buffer.prev_grapheme(s.start) } else { s.start };
        self.move_to(to, cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let s = self.buffer.selection();
        let to = if s.is_empty() { self.buffer.next_grapheme(s.end) } else { s.end };
        self.move_to(to, cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, false, cx);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, false, cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.prev_grapheme(self.buffer.head());
        self.extend_to(to, cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_grapheme(self.buffer.head());
        self.extend_to(to, cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(-1, true, cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.vertical(1, true, cx);
    }
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.prev_word(self.buffer.head());
        self.move_to(to, cx);
    }
    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_word(self.buffer.head());
        self.move_to(to, cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.prev_word(self.buffer.head());
        self.extend_to(to, cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.buffer.next_word(self.buffer.head());
        self.extend_to(to, cx);
    }
    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.visual_line_bounds(self.buffer.head()).start;
        self.move_to(to, cx);
    }
    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.visual_line_bounds(self.buffer.head()).end;
        self.move_to(to, cx);
    }
    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.visual_line_bounds(self.buffer.head()).start;
        self.extend_to(to, cx);
    }
    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let to = self.visual_line_bounds(self.buffer.head()).end;
        self.extend_to(to, cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.buffer.len();
        self.move_to(n, cx);
    }
    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.extend_to(0, cx);
    }
    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.buffer.len();
        self.extend_to(n, cx);
    }
    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.page_rows();
        self.scroll_y -= px(self.config.line_height) * rows as f32;
        self.vertical(-rows, false, cx);
    }
    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.page_rows();
        self.scroll_y += px(self.config.line_height) * rows as f32;
        self.vertical(rows, false, cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.buffer.len();
        self.select(0, n, cx);
    }

    // ---- actions: editing ---------------------------------------------------

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.buffer.selection();
        if s.is_empty() {
            if self.config.auto_pair
                && let Some(r) = format::delete_pair(self.buffer.text(), s.start)
            {
                s = r;
            } else {
                s = self.buffer.prev_grapheme(s.start)..s.start;
            }
        }
        self.edit(s, "", EditKind::Delete, Author::Human, cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.buffer.selection();
        if s.is_empty() {
            s = s.start..self.buffer.next_grapheme(s.start);
        }
        self.edit(s, "", EditKind::Delete, Author::Human, cx);
    }
    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.buffer.selection();
        if s.is_empty() {
            s = self.buffer.prev_word(s.start)..s.start;
        }
        self.edit(s, "", EditKind::Other, Author::Human, cx);
    }
    fn delete_word_right(&mut self, _: &DeleteWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.buffer.selection();
        if s.is_empty() {
            s = s.start..self.buffer.next_word(s.start);
        }
        self.edit(s, "", EditKind::Other, Author::Human, cx);
    }
    fn delete_to_line_start(&mut self, _: &DeleteToLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let h = self.buffer.head();
        let s = self.buffer.line_start(h);
        self.edit(s..h, "", EditKind::Other, Author::Human, cx);
    }
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        let c = format::newline(self.buffer.text(), self.buffer.selection());
        self.apply(c, cx);
    }
    fn soft_newline(&mut self, _: &SoftNewline, _: &mut Window, cx: &mut Context<Self>) {
        self.insert_text("\n", Author::Human, cx);
    }
    fn tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.buffer.text();
        let s = self.buffer.selection();
        let line = &t[self.buffer.line_range(s.start)];
        if !s.is_empty() || format::marker_len(line) > 0 && !line.trim_start().starts_with('#') {
            let c = format::indent(t, s.clone());
            // Keep the caret where it was, shifted by the inserted tab.
            let caret = s.end + 1;
            self.apply(c, cx);
            if s.is_empty() {
                self.move_to(caret, cx);
            }
        } else {
            self.insert_text("\t", Author::Human, cx);
        }
    }
    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        let c = format::outdent(self.buffer.text(), self.buffer.selection());
        self.apply(c, cx);
    }
    fn move_line_up(&mut self, _: &MoveLineUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = format::move_lines(self.buffer.text(), self.buffer.selection(), true) {
            self.apply(c, cx);
        }
    }
    fn move_line_down(&mut self, _: &MoveLineDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(c) = format::move_lines(self.buffer.text(), self.buffer.selection(), false) {
            self.apply(c, cx);
        }
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.undo() {
            self.after_edit(cx);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.redo() {
            self.after_edit(cx);
        }
    }

    // ---- clipboard ----------------------------------------------------------

    fn copy_range(&self) -> Range<usize> {
        let s = self.buffer.selection();
        if s.is_empty() {
            // Like most editors, copying with no selection copies the line.
            let r = self.buffer.line_range(s.start);
            r.start..(r.end + 1).min(self.buffer.len())
        } else {
            s
        }
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.copy_range();
        cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.text()[r].to_string()));
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.copy_range();
        cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.text()[r.clone()].to_string()));
        self.edit(r, "", EditKind::Other, Author::Human, cx);
    }
    fn copy_formatted(&mut self, _: &CopyFormatted, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.copy_range();
        let text = crate::export::plain_text(&self.buffer.text()[r]);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
    fn copy_html(&mut self, _: &CopyHtml, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.copy_range();
        let html = crate::export::body_html(&self.buffer.text()[r]);
        cx.write_to_clipboard(ClipboardItem::new_string(html));
    }
    fn copy_markdown(&mut self, _: &CopyMarkdown, _: &mut Window, cx: &mut Context<Self>) {
        let r = self.copy_range();
        cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.text()[r].to_string()));
    }
    fn paste_with(&mut self, author: Author, plain: bool, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) else { return };
        let text = text.replace("\r\n", "\n");
        let text = if plain { crate::export::plain_text(&text).trim_end().to_string() } else { text };
        self.insert_text(&text, author, cx);
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_with(Author::Human, false, cx);
    }
    fn paste_plain(&mut self, _: &PastePlain, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_with(Author::Human, true, cx);
    }
    fn paste_as_ai(&mut self, _: &PasteAsAi, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_with(Author::Ai, false, cx);
    }
    fn paste_as_human(&mut self, _: &PasteAsHuman, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_with(Author::Human, false, cx);
    }
    fn paste_edits(&mut self, author: Author, cx: &mut Context<Self>) {
        let Some(new) = cx.read_from_clipboard().and_then(|i| i.text()) else { return };
        self.apply_external(&new.replace("\r\n", "\n"), author, cx);
    }

    /// Replace the document with `new`, applying only the differences (one undo step).
    pub fn apply_external(&mut self, new: &str, author: Author, cx: &mut Context<Self>) {
        let new = new.to_string();
        let hunks = crate::diff::diff(self.buffer.text(), &new);
        for h in hunks.iter().rev() {
            self.authors.edit(h.old.clone(), h.new.len(), author);
        }
        let edits: Vec<(Range<usize>, String)> = hunks.into_iter().map(|h| (h.old, h.new)).collect();
        self.buffer.replace_many(&edits);
        self.after_edit(cx);
    }
    fn paste_edits_ai(&mut self, _: &PasteEditsFromAi, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_edits(Author::Ai, cx);
    }
    fn paste_edits_human(&mut self, _: &PasteEditsFromHuman, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_edits(Author::Human, cx);
    }
    fn mark_as_ai(&mut self, _: &MarkAsAi, _: &mut Window, cx: &mut Context<Self>) {
        self.authors.mark(self.buffer.selection(), Author::Ai);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }
    fn mark_as_human(&mut self, _: &MarkAsHuman, _: &mut Window, cx: &mut Context<Self>) {
        self.authors.mark(self.buffer.selection(), Author::Human);
        cx.emit(EditorEvent::Edited);
        cx.notify();
    }

    // ---- actions: format ----------------------------------------------------

    fn wrap(&mut self, open: &str, close: &str, cx: &mut Context<Self>) {
        let c = format::toggle_wrap(self.buffer.text(), self.buffer.selection(), open, close);
        self.apply(c, cx);
    }
    fn heading(&mut self, level: usize, cx: &mut Context<Self>) {
        let c = format::set_heading(self.buffer.text(), self.buffer.selection(), level);
        self.apply(c, cx);
    }
    fn prefix(&mut self, p: Prefix, cx: &mut Context<Self>) {
        let c = format::toggle_prefix(self.buffer.text(), self.buffer.selection(), p);
        self.apply(c, cx);
    }
    fn block(&mut self, text: &str, caret: usize, cx: &mut Context<Self>) {
        let c = format::insert_block(self.buffer.text(), self.buffer.selection(), text, caret);
        self.apply(c, cx);
    }
    fn case(&mut self, case: Case, cx: &mut Context<Self>) {
        let c = format::transform(self.buffer.text(), self.buffer.selection(), case);
        self.apply(c, cx);
    }
    fn table(&mut self, op: table::Op, cx: &mut Context<Self>) {
        if let Some(c) = table::edit(self.buffer.text(), self.buffer.head(), op) {
            self.apply(c, cx);
        }
    }

    fn alert(&mut self, kind: &str, cx: &mut Context<Self>) {
        let text = format!("> [!{kind}]\n> ");
        let n = text.len();
        self.block(&text, n, cx);
    }

    fn register_format_actions<E: InteractiveElement>(&self, d: E, cx: &mut Context<Self>) -> E {
        d.on_action(cx.listener(|e, _: &Bold, _, cx| e.wrap("**", "**", cx)))
            .on_action(cx.listener(|e, _: &Italic, _, cx| e.wrap("*", "*", cx)))
            .on_action(cx.listener(|e, _: &Strikethrough, _, cx| e.wrap("~~", "~~", cx)))
            .on_action(cx.listener(|e, _: &Highlight, _, cx| e.wrap("==", "==", cx)))
            .on_action(cx.listener(|e, _: &Code, _, cx| e.wrap("`", "`", cx)))
            .on_action(cx.listener(|e, _: &Math, _, cx| e.wrap("$", "$", cx)))
            .on_action(cx.listener(|e, _: &CodeBlock, _, cx| {
                let c = format::wrap_lines(e.buffer.text(), e.buffer.selection(), "```", "```");
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &DisplayMath, _, cx| {
                let c = format::wrap_lines(e.buffer.text(), e.buffer.selection(), "$$", "$$");
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &Heading1, _, cx| e.heading(1, cx)))
            .on_action(cx.listener(|e, _: &Heading2, _, cx| e.heading(2, cx)))
            .on_action(cx.listener(|e, _: &Heading3, _, cx| e.heading(3, cx)))
            .on_action(cx.listener(|e, _: &Heading4, _, cx| e.heading(4, cx)))
            .on_action(cx.listener(|e, _: &Heading5, _, cx| e.heading(5, cx)))
            .on_action(cx.listener(|e, _: &Heading6, _, cx| e.heading(6, cx)))
            .on_action(cx.listener(|e, _: &Body, _, cx| e.heading(0, cx)))
            .on_action(cx.listener(|e, _: &List, _, cx| e.prefix(Prefix::List, cx)))
            .on_action(cx.listener(|e, _: &TaskList, _, cx| e.prefix(Prefix::Task, cx)))
            .on_action(cx.listener(|e, _: &OrderedList, _, cx| e.prefix(Prefix::Ordered, cx)))
            .on_action(cx.listener(|e, _: &OrderedTaskList, _, cx| e.prefix(Prefix::OrderedTask, cx)))
            .on_action(cx.listener(|e, _: &Blockquote, _, cx| e.prefix(Prefix::Quote, cx)))
            .on_action(cx.listener(|e, _: &Comment, _, cx| e.prefix(Prefix::Comment, cx)))
            .on_action(cx.listener(|e, _: &MarkTaskCompleted, _, cx| {
                let c = format::toggle_task_done(e.buffer.text(), e.buffer.selection());
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &AlertNote, _, cx| e.alert("NOTE", cx)))
            .on_action(cx.listener(|e, _: &AlertTip, _, cx| e.alert("TIP", cx)))
            .on_action(cx.listener(|e, _: &AlertImportant, _, cx| e.alert("IMPORTANT", cx)))
            .on_action(cx.listener(|e, _: &AlertWarning, _, cx| e.alert("WARNING", cx)))
            .on_action(cx.listener(|e, _: &AlertCaution, _, cx| e.alert("CAUTION", cx)))
            .on_action(cx.listener(|e, _: &AddLink, _, cx| {
                let c = format::link(e.buffer.text(), e.buffer.selection(), false);
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &AddImageLink, _, cx| {
                let c = format::link(e.buffer.text(), e.buffer.selection(), true);
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &AddWikilink, _, cx| {
                let c = format::wikilink(e.buffer.text(), e.buffer.selection());
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &AddFootnote, _, cx| {
                let c = format::footnote(e.buffer.text(), e.buffer.selection());
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &AddHorizontalRule, _, cx| e.block("---\n", 4, cx)))
            .on_action(cx.listener(|e, _: &AddPageBreak, _, cx| e.block("+++\n", 4, cx)))
            .on_action(cx.listener(|e, _: &AddTableOfContents, _, cx| e.block("{{TOC}}\n", 8, cx)))
            .on_action(cx.listener(|e, _: &AddContentBlock, _, cx| e.block("/", 1, cx)))
            .on_action(cx.listener(|e, _: &AddTable, _, cx| {
                let (t, caret) = format::table_template();
                e.block(t, caret, cx)
            }))
            .on_action(cx.listener(|e, _: &AddHashtag, _, cx| e.insert_text("#", Author::Human, cx)))
            .on_action(cx.listener(|e, _: &AddDate, _, cx| {
                let d = crate::library::today_iso();
                e.insert_text(&d, Author::Human, cx)
            }))
            .on_action(cx.listener(|e, _: &ClearStyles, _, cx| {
                let c = format::clear_styles(e.buffer.text(), e.buffer.selection());
                e.apply(c, cx)
            }))
            .on_action(cx.listener(|e, _: &TableSortAscending, _, cx| e.table(table::Op::SortAscending, cx)))
            .on_action(cx.listener(|e, _: &TableSortDescending, _, cx| e.table(table::Op::SortDescending, cx)))
            .on_action(cx.listener(|e, _: &TableRowAbove, _, cx| e.table(table::Op::RowAbove, cx)))
            .on_action(cx.listener(|e, _: &TableRowBelow, _, cx| e.table(table::Op::RowBelow, cx)))
            .on_action(cx.listener(|e, _: &TableColumnBefore, _, cx| e.table(table::Op::ColumnBefore, cx)))
            .on_action(cx.listener(|e, _: &TableColumnAfter, _, cx| e.table(table::Op::ColumnAfter, cx)))
            .on_action(cx.listener(|e, _: &TableAlignLeft, _, cx| e.table(table::Op::Align(table::Align::Left), cx)))
            .on_action(cx.listener(|e, _: &TableAlignCenter, _, cx| e.table(table::Op::Align(table::Align::Center), cx)))
            .on_action(cx.listener(|e, _: &TableAlignRight, _, cx| e.table(table::Op::Align(table::Align::Right), cx)))
            .on_action(cx.listener(|e, _: &TableDeleteColumn, _, cx| e.table(table::Op::DeleteColumn, cx)))
            .on_action(cx.listener(|e, _: &TableDeleteRow, _, cx| e.table(table::Op::DeleteRow, cx)))
            .on_action(cx.listener(|e, _: &TableMoveColumnLeft, _, cx| e.table(table::Op::MoveColumnLeft, cx)))
            .on_action(cx.listener(|e, _: &TableMoveColumnRight, _, cx| e.table(table::Op::MoveColumnRight, cx)))
            .on_action(cx.listener(|e, _: &TableMoveRowUp, _, cx| e.table(table::Op::MoveRowUp, cx)))
            .on_action(cx.listener(|e, _: &TableMoveRowDown, _, cx| e.table(table::Op::MoveRowDown, cx)))
            .on_action(cx.listener(|e, _: &TableReformat, _, cx| e.table(table::Op::Reformat, cx)))
            .on_action(cx.listener(|e, _: &TableTranspose, _, cx| e.table(table::Op::Transpose, cx)))
            .on_action(cx.listener(|e, _: &TableMerge, _, cx| e.table(table::Op::Merge, cx)))
            .on_action(cx.listener(|e, _: &TableUnmerge, _, cx| e.table(table::Op::Unmerge, cx)))
            .on_action(cx.listener(|e, _: &UpperCase, _, cx| e.case(Case::Upper, cx)))
            .on_action(cx.listener(|e, _: &LowerCase, _, cx| e.case(Case::Lower, cx)))
            .on_action(cx.listener(|e, _: &Capitalize, _, cx| e.case(Case::Capitalize, cx)))
            .on_action(cx.listener(|e, _: &TitleCase, _, cx| e.case(Case::Title, cx)))
    }
}

// ---- layout helpers (free functions so they can borrow Layout alone) ------------

fn locate(layout: &Layout, offset: usize) -> Option<(usize, usize)> {
    let li = layout.lines.partition_point(|l| l.range.end < offset);
    let l = layout.lines.get(li)?;
    let rel = offset.saturating_sub(l.range.start);
    let row = l.rows.partition_point(|&s| s <= rel).saturating_sub(1);
    Some((li, row))
}

fn x_in_row(l: &LineInfo, row: usize, rel: usize) -> Pixels {
    let u = &l.shaped.unwrapped_layout;
    u.x_for_index(rel) - u.x_for_index(l.rows[row])
}

fn prev_boundary(s: &str, mut i: usize) -> usize {
    if i == 0 {
        return 0;
    }
    i -= 1;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Index (relative to the line) closest to `x` within a visual row.
fn index_in_row(l: &LineInfo, row: usize, x: Pixels, text: &str) -> usize {
    let u = &l.shaped.unwrapped_layout;
    let start = l.rows[row];
    let end = l.rows.get(row + 1).copied().unwrap_or(l.range.len());
    let idx = u.closest_index_for_x(x + u.x_for_index(start)).clamp(start, end);
    if idx == end && row + 1 < l.rows.len() {
        prev_boundary(&text[l.range.clone()], end)
    } else {
        idx
    }
}

/// Caret position (x relative to the line's text origin, y relative to content top).
fn caret_point(layout: &Layout, offset: usize) -> Option<(Pixels, Pixels)> {
    let (li, row) = locate(layout, offset)?;
    let l = &layout.lines[li];
    Some((x_in_row(l, row, offset - l.range.start), l.top + layout.line_height * row as f32))
}

// ---- input handler --------------------------------------------------------------

impl EntityInputHandler for Editor {
    fn text_for_range(&mut self, range: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let r = self.range_from_utf16(&range);
        actual.replace(self.range_to_utf16(&r));
        Some(self.buffer.text()[r].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.buffer.selection()),
            reversed: self.buffer.is_reversed(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer.marked.as_ref().map(|r| self.range_to_utf16(r))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.buffer.marked = None;
        self.composition_base = None;
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        let mut target = range
            .map(|r| self.range_from_utf16(&r))
            .or(self.buffer.marked.clone())
            .unwrap_or(self.buffer.selection());
        if let (Some(marked), Some((at, original))) = (self.buffer.marked.take(), self.composition_base.take()) {
            // Restore what the composition replaced, then commit as one undoable edit.
            self.buffer.replace_untracked(marked.clone(), &original);
            target = at..at + original.len();
        }
        let sel = self.buffer.selection();
        if target == sel
            && let Some(c) = format::smart_input(
                self.buffer.text(),
                sel,
                text,
                self.config.smart_quotes,
                self.config.smart_dashes,
                self.config.auto_pair,
            )
        {
            self.authors.edit(c.range.clone(), c.text.len(), Author::Human);
            let kind = if c.text.chars().count() == 1 { EditKind::Typing } else { EditKind::Other };
            self.buffer.replace(c.range, &c.text, kind);
            self.buffer.set_selection(c.select.start, c.select.end);
            self.after_edit(cx);
            return;
        }
        let kind = if text.chars().count() == 1 { EditKind::Typing } else { EditKind::Other };
        self.edit(target, text, kind, Author::Human, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, selected: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        let target = range
            .map(|r| self.range_from_utf16(&r))
            .or(self.buffer.marked.clone())
            .unwrap_or(self.buffer.selection());
        if self.composition_base.is_none() {
            self.composition_base = Some((target.start, self.buffer.text()[target.clone()].to_string()));
        }
        self.buffer.replace_untracked(target.clone(), text);
        self.buffer.marked = (!text.is_empty()).then(|| target.start..target.start + text.len());
        if let Some(sel) = selected {
            // `selected` is relative to the marked text, in UTF-16.
            let marked = &text;
            let to8 = |u: usize| {
                let mut n = 0;
                for (i, c) in marked.char_indices() {
                    if n >= u {
                        return i;
                    }
                    n += c.len_utf16();
                }
                marked.len()
            };
            self.buffer.set_selection(target.start + to8(sel.start), target.start + to8(sel.end));
        }
        self.autoscroll = true;
        cx.notify();
    }

    fn bounds_for_range(&mut self, range: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let r = self.range_from_utf16(&range);
        let layout = self.layout.as_ref()?;
        let (li, _) = locate(layout, r.start)?;
        let (x, y) = caret_point(layout, r.start)?;
        let origin = self.line_origin(&layout.lines[li]);
        let top = layout.bounds.top() + layout.top_pad + y - self.scroll_y;
        Some(Bounds::new(point(origin.x + x, top), size(px(2.), layout.line_height)))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let o = self.offset_for_point(p);
        Some(self.to_utf16(o))
    }
}

// ---- element --------------------------------------------------------------------

struct EditorElement {
    editor: Entity<Editor>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |e, cx| e.prepare(bounds, window, cx));
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        let theme = *cx.global::<Theme>();
        let focus_handle = self.editor.read(cx).focus_handle.clone();
        window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.editor.clone()), cx);
        let focused = focus_handle.is_focused(window);
        let (layout, scroll_y, sel, head, marked, find_matches, font_size) = self.editor.update(cx, |e, _| {
            (
                e.layout.take(),
                e.scroll_y,
                e.buffer.selection(),
                e.buffer.head(),
                e.buffer.marked.clone(),
                e.find.as_ref().map(|f| f.matches.clone()).unwrap_or_default(),
                px(e.config.font_size),
            )
        });
        let Some(layout) = layout else { return };
        let lh = layout.line_height;
        let line_origin = |l: &LineInfo| {
            point(layout.text_x - l.hang, layout.bounds.top() + layout.top_pad + l.top - scroll_y)
        };

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let first = layout.lines.partition_point(|l| l.top + l.height + layout.top_pad < scroll_y);
            for l in &layout.lines[first..] {
                let origin = line_origin(l);
                if origin.y > bounds.bottom() {
                    break;
                }
                let paint_range = |r: &Range<usize>, color: Hsla, window: &mut Window| {
                    if r.end < l.range.start || r.start > l.range.end {
                        return;
                    }
                    let a = r.start.max(l.range.start);
                    let b = r.end.min(l.range.end);
                    let ra = l.rows.partition_point(|&s| s <= a - l.range.start).saturating_sub(1);
                    let rb = l.rows.partition_point(|&s| s <= b - l.range.start).saturating_sub(1);
                    for row in ra..=rb {
                        let row_end_rel = l.rows.get(row + 1).copied().unwrap_or(l.range.len());
                        let x0 = if row == ra { x_in_row(l, row, a - l.range.start) } else { px(0.) };
                        let mut x1 = if row == rb { x_in_row(l, row, b - l.range.start) } else { x_in_row(l, row, row_end_rel) };
                        if row == rb && r.end > l.range.end {
                            x1 += font_size * 0.4; // selected newline
                        }
                        if x1 > x0 {
                            let y = origin.y + lh * row as f32;
                            window.paint_quad(fill(Bounds::new(point(origin.x + x0, y), size(x1 - x0, lh)), color));
                        }
                    }
                };
                for m in &find_matches {
                    if m.end >= l.range.start && m.start <= l.range.end {
                        paint_range(m, theme.find_match, window);
                    }
                }
                if !sel.is_empty() {
                    paint_range(&sel, theme.selection, window);
                }
                let _ = l.shaped.paint_background(origin, lh, TextAlign::Left, None, window, cx);
                let _ = l.shaped.paint(origin, lh, TextAlign::Left, None, window, cx);
                if let Some(m) = &marked
                    && m.start >= l.range.start
                    && m.end <= l.range.end
                {
                    let (li, _) = locate(&layout, m.start).unwrap_or((0, 0));
                    if std::ptr::eq(&layout.lines[li], l) {
                        let x0 = x_in_row(l, 0, m.start - l.range.start);
                        let x1 = x_in_row(l, 0, m.end - l.range.start);
                        let row = l.rows.partition_point(|&s| s <= m.start - l.range.start).saturating_sub(1);
                        let y = origin.y + lh * (row as f32 + 1.) - px(4.);
                        window.paint_quad(fill(Bounds::new(point(origin.x + x0, y), size(x1 - x0, px(1.5))), theme.text));
                    }
                }
            }

            if focused && sel.is_empty()
                && let Some((x, y)) = caret_point(&layout, head)
                && let Some((li, _)) = locate(&layout, head)
            {
                let origin = line_origin(&layout.lines[li]);
                // Measured on iA's screenshots: ~3 pt wide, nearly the full line height.
                let caret_h = lh - px(2.);
                let top = origin.y + y - layout.lines[li].top + px(1.);
                window.paint_quad(
                    fill(Bounds::new(point(origin.x + x - px(1.5), top), size(px(3.), caret_h)), theme.caret)
                        .corner_radii(px(1.5)),
                );
            }
        });
        self.editor.update(cx, |e, _| e.layout = Some(layout));
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let d = div()
            .id("editor")
            .key_context("Editor")
            .track_focus(&self.focus_handle)
            .size_full()
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::soft_newline))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::move_line_up))
            .on_action(cx.listener(Self::move_line_down))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy_formatted))
            .on_action(cx.listener(Self::copy_html))
            .on_action(cx.listener(Self::copy_markdown))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::paste_plain))
            .on_action(cx.listener(Self::paste_as_ai))
            .on_action(cx.listener(Self::paste_as_human))
            .on_action(cx.listener(Self::mark_as_ai))
            .on_action(cx.listener(Self::paste_edits_ai))
            .on_action(cx.listener(Self::paste_edits_human))
            .on_action(cx.listener(Self::mark_as_human))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_click))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll));
        self.register_format_actions(d, cx)
            .child(EditorElement { editor: cx.entity() })
    }
}
