//! Native preview: Markdown → blocks → GPUI elements, styled like a reading template.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    IntoElement, ParentElement, Render, ScrollHandle, SharedString, StrikethroughStyle, Styled,
    StyledText, UnderlineStyle, Window, div, img, prelude::*, px, relative,
};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::markdown;
use crate::settings::Template;
use crate::theme::Theme;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub mark: bool,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Para(Vec<Span>),
    List { start: Option<u64>, items: Vec<Item> },
    Quote(Vec<Block>),
    Alert(String, Vec<Block>),
    Code { lang: String, text: String },
    Math(String),
    Rule,
    PageBreak,
    Table { align: Vec<Alignment>, head: Vec<Vec<Span>>, rows: Vec<Vec<Vec<Span>>> },
    Image { src: String, alt: String },
    Footnote(String, Vec<Block>),
    Toc,
}

#[derive(Default, Clone, Copy)]
struct Inline {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

/// One open container while walking the event stream.
enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List(Option<u64>, Vec<Item>),
    Item(Option<bool>, Vec<Block>),
    Footnote(String, Vec<Block>),
}

fn push_block(stack: &mut [Frame], b: Block) {
    match stack.last_mut().unwrap() {
        Frame::Root(v) | Frame::Quote(v) | Frame::Footnote(_, v) => v.push(b),
        Frame::Item(_, v) => v.push(b),
        Frame::List(..) => {}
    }
}

/// `==mark==` inside a text span.
fn split_marks(text: &str, base: &Span) -> Vec<Span> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(a) = rest.find("==") {
        let Some(z) = rest[a + 2..].find("==").filter(|&z| z > 0) else { break };
        if a > 0 {
            out.push(Span { text: rest[..a].to_string(), ..base.clone() });
        }
        out.push(Span { text: rest[a + 2..a + 2 + z].to_string(), mark: true, ..base.clone() });
        rest = &rest[a + 2 + z + 2..];
    }
    if !rest.is_empty() {
        out.push(Span { text: rest.to_string(), ..base.clone() });
    }
    out
}

/// Remove the first `n` bytes of text (plus following whitespace) from a span list.
fn drop_prefix(spans: &mut Vec<Span>, mut n: usize) {
    while n > 0 && !spans.is_empty() {
        let len = spans[0].text.len();
        if len <= n {
            n -= len;
            spans.remove(0);
        } else {
            spans[0].text.drain(..n);
            n = 0;
        }
    }
    while let Some(first) = spans.first_mut() {
        let t = first.text.trim_start().to_string();
        if t.is_empty() {
            spans.remove(0);
        } else {
            first.text = t;
            break;
        }
    }
}

/// Marker for a cell covered by the colspan of the cell before it.
pub const MERGED: &str = "\u{1}";

pub fn parse(md: &str) -> Vec<Block> {
    let md = crate::export::strip_comments(md);
    let mut stack = vec![Frame::Root(Vec::new())];
    let mut spans: Vec<Span> = Vec::new();
    let mut inline = Inline::default();
    let mut link: Option<String> = None;
    let mut heading: Option<u8> = None;
    let mut code: Option<(String, String)> = None;
    let mut table: Option<(Vec<Alignment>, Vec<Vec<Span>>, Vec<Vec<Vec<Span>>>, Vec<Vec<Span>>, bool)> = None;
    let mut image: Option<(String, String)> = None;

    let span = |text: &str, inline: Inline, link: &Option<String>| Span {
        text: text.to_string(),
        bold: inline.bold,
        italic: inline.italic,
        code: inline.code,
        strike: inline.strike,
        mark: false,
        link: link.clone(),
    };

    for (ev, range) in Parser::new_ext(&md, markdown::options()).into_offset_iter() {
        if let Some((_, alt)) = &mut image {
            match ev {
                Event::Text(t) | Event::Code(t) => alt.push_str(&t),
                Event::End(TagEnd::Image) => {
                    let (src, alt) = image.take().unwrap();
                    push_block(&mut stack, Block::Image { src, alt });
                }
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                })
            }
            Event::End(TagEnd::Heading(_)) => {
                let s = std::mem::take(&mut spans);
                push_block(&mut stack, Block::Heading(heading.take().unwrap_or(1), s));
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                let s = std::mem::take(&mut spans);
                let plain: String = s.iter().map(|x| x.text.as_str()).collect();
                let t = plain.trim();
                if t == "{{TOC}}" {
                    push_block(&mut stack, Block::Toc);
                } else if t == "+++" {
                    push_block(&mut stack, Block::PageBreak);
                } else if !s.is_empty() {
                    push_block(&mut stack, Block::Para(s));
                }
            }
            Event::Start(Tag::BlockQuote(_)) => stack.push(Frame::Quote(Vec::new())),
            Event::End(TagEnd::BlockQuote(_)) => {
                if let Some(Frame::Quote(mut v)) = stack.pop() {
                    // GitHub-style alerts: first paragraph is "[!NOTE]".
                    let alert = match v.first() {
                        Some(Block::Para(sp)) => {
                            let plain: String = sp.iter().map(|x| x.text.as_str()).collect();
                            plain
                                .strip_prefix("[!")
                                .and_then(|r| r.find(']').map(|i| (r[..i].to_string(), i + 3)))
                        }
                        _ => None,
                    };
                    if let Some((kind, prefix_len)) = alert {
                        if let Some(Block::Para(sp)) = v.first_mut() {
                            drop_prefix(sp, prefix_len);
                            if sp.is_empty() {
                                v.remove(0);
                            }
                        }
                        push_block(&mut stack, Block::Alert(kind, v));
                    } else {
                        push_block(&mut stack, Block::Quote(v));
                    }
                }
            }
            Event::Start(Tag::List(start)) => {
                if let Some(Frame::Item(_, v)) = stack.last_mut()
                    && !spans.is_empty()
                {
                    v.push(Block::Para(std::mem::take(&mut spans)));
                }
                stack.push(Frame::List(start, Vec::new()))
            }
            Event::End(TagEnd::List(_)) => {
                if let Some(Frame::List(start, items)) = stack.pop() {
                    push_block(&mut stack, Block::List { start, items });
                }
            }
            Event::Start(Tag::Item) => stack.push(Frame::Item(None, Vec::new())),
            Event::End(TagEnd::Item) => {
                if let Some(Frame::Item(task, mut blocks)) = stack.pop() {
                    // Tight list items have no paragraph events; flush stray spans.
                    if !spans.is_empty() {
                        blocks.push(Block::Para(std::mem::take(&mut spans)));
                    }
                    if let Some(Frame::List(_, items)) = stack.last_mut() {
                        items.push(Item { task, blocks });
                    }
                }
            }
            Event::TaskListMarker(done) => {
                if let Some(Frame::Item(task, ..)) = stack.last_mut() {
                    *task = Some(done);
                }
            }
            Event::Start(Tag::FootnoteDefinition(label)) => stack.push(Frame::Footnote(label.to_string(), Vec::new())),
            Event::End(TagEnd::FootnoteDefinition) => {
                if let Some(Frame::Footnote(label, v)) = stack.pop() {
                    push_block(&mut stack, Block::Footnote(label, v));
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((lang, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, text)) = code.take() {
                    push_block(&mut stack, Block::Code { lang, text: text.trim_end_matches('\n').to_string() });
                }
            }
            Event::Start(Tag::Table(align)) => table = Some((align, Vec::new(), Vec::new(), Vec::new(), false)),
            Event::Start(Tag::TableHead) => {
                if let Some(t) = &mut table {
                    t.4 = true;
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(t) = &mut table {
                    t.1 = std::mem::take(&mut t.3);
                    t.4 = false;
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = &mut table {
                    let row = std::mem::take(&mut t.3);
                    t.2.push(row);
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(t) = &mut table {
                    // MultiMarkdown `||`: an empty source range continues the previous cell.
                    let cell = std::mem::take(&mut spans);
                    t.3.push(if range.is_empty() { vec![Span { text: MERGED.into(), ..Default::default() }] } else { cell });
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some((align, head, rows, _, _)) = table.take() {
                    push_block(&mut stack, Block::Table { align, head, rows });
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => image = Some((dest_url.to_string(), String::new())),
            Event::Start(Tag::Strong) => inline.bold = true,
            Event::End(TagEnd::Strong) => inline.bold = false,
            Event::Start(Tag::Emphasis) => inline.italic = true,
            Event::End(TagEnd::Emphasis) => inline.italic = false,
            Event::Start(Tag::Strikethrough) => inline.strike = true,
            Event::End(TagEnd::Strikethrough) => inline.strike = false,
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => link = None,
            Event::Text(t) => {
                if let Some((_, text)) = &mut code {
                    text.push_str(&t);
                } else {
                    let base = span("", inline, &link);
                    spans.extend(split_marks(&t, &base));
                }
            }
            Event::Code(t) => spans.push(span(&t, Inline { code: true, ..inline }, &link)),
            Event::InlineMath(t) => spans.push(span(&t, Inline { code: true, ..inline }, &link)),
            Event::DisplayMath(t) => push_block(&mut stack, Block::Math(t.to_string())),
            Event::FootnoteReference(l) => spans.push(Span { text: format!("[{l}]"), link: Some(format!("#fn-{l}")), ..Default::default() }),
            Event::SoftBreak => spans.push(span(" ", inline, &link)),
            Event::HardBreak => spans.push(span("\n", inline, &link)),
            Event::Rule => push_block(&mut stack, Block::Rule),
            _ => {}
        }
    }
    match stack.into_iter().next() {
        Some(Frame::Root(v)) => v,
        _ => Vec::new(),
    }
}

/// iA content blocks: a paragraph that is just "/file.ext" embeds that file.
pub fn resolve_content_blocks(blocks: Vec<Block>, base: Option<&Path>, depth: usize) -> Vec<Block> {
    let mut out = Vec::new();
    for b in blocks {
        let Block::Para(spans) = &b else {
            out.push(b);
            continue;
        };
        let plain: String = spans.iter().map(|s| s.text.as_str()).collect();
        let t = plain.trim();
        let embed = t.starts_with('/') && !t.contains(' ') && t.len() > 1 && t.contains('.');
        if !embed || base.is_none() {
            out.push(b);
            continue;
        }
        let path = base.unwrap().join(&t[1..]);
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => out.push(Block::Image {
                src: path.to_string_lossy().into_owned(),
                alt: String::new(),
            }),
            "md" | "markdown" | "txt" | "text" if depth < 3 => match std::fs::read_to_string(&path) {
                Ok(s) => out.extend(resolve_content_blocks(parse(&s), path.parent(), depth + 1)),
                Err(_) => out.push(b),
            },
            "csv" | "tsv" => match std::fs::read_to_string(&path) {
                Ok(s) => {
                    let sep = if ext == "tsv" { '\t' } else { ',' };
                    let mut rows: Vec<Vec<Vec<Span>>> = s
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(|l| l.split(sep).map(|c| vec![Span { text: c.trim().trim_matches('"').to_string(), ..Default::default() }]).collect())
                        .collect();
                    if rows.is_empty() {
                        out.push(b);
                    } else {
                        let head = rows.remove(0);
                        out.push(Block::Table { align: vec![Alignment::None; head.len()], head, rows });
                    }
                }
                Err(_) => out.push(b),
            },
            _ => out.push(b),
        }
    }
    out
}

pub fn headings(blocks: &[Block]) -> Vec<(u8, String)> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Heading(l, s) => Some((*l, s.iter().map(|x| x.text.as_str()).collect())),
            _ => None,
        })
        .collect()
}

// ---- view -----------------------------------------------------------------------

/// Heading style: size relative to body, margins above/below in em of the heading.
#[derive(Clone, Copy)]
struct HeadingSpec {
    scale: f32,
    top: f32,
    bottom: f32,
    rule: bool,
}

/// Measurements taken from the original templates' stylesheets.
#[derive(Clone, Copy)]
pub struct Spec {
    family: &'static str,
    size: f32,
    line_height: f32,
    measure_em: f32,
    block_gap_em: f32,
    headings: [HeadingSpec; 6],
    /// GitHub greys out h6.
    muted_h6: bool,
    /// Top margin (em of the second heading) when h1/h2 directly follows h1/h2.
    stacked_heading_em: f32,
}

pub fn spec(t: Template) -> Spec {
    let same = |top: f32, bottom: f32| HeadingSpec { scale: 1., top, bottom, rule: false };
    // Manuscript templates: headings at body size, 29/16 em around blocks.
    let manuscript = |family, line_height| Spec {
        family,
        size: 18.,
        line_height,
        measure_em: 38.,
        block_gap_em: 29. / 16.,
        headings: [
            same(29. / 16., 29. / 16.),
            same(29. / 16., 29. / 16.),
            same(29. / 16., 0.),
            same(29. / 16., 0.),
            same(29. / 16., 0.),
            same(29. / 16., 0.),
        ],
        muted_h6: false,
        stacked_heading_em: -15. / 16.,
    };
    match t {
        Template::Duo => manuscript(crate::fonts::DUO, 1.75),
        Template::Mono => manuscript(crate::fonts::MONO, 1.75),
        Template::Quattro => manuscript(crate::fonts::QUATTRO, 1.64),
        Template::Serif => Spec {
            family: crate::fonts::SERIF,
            size: 18.,
            line_height: 1.64,
            measure_em: 37.5,
            block_gap_em: 1.,
            headings: [
                HeadingSpec { scale: 23. / 19., top: 62. / 23., bottom: 62. / 23., rule: false },
                HeadingSpec { scale: 21. / 19., top: 62. / 21., bottom: 31. / 21., rule: false },
                same(31. / 19., 0.),
                same(31. / 19., 0.),
                same(31. / 19., 0.),
                same(31. / 19., 0.),
            ],
            muted_h6: false,
            stacked_heading_em: -46. / 21.,
        },
        Template::Sans => Spec {
            // The original Sans template is IBM Plex Sans.
            family: crate::fonts::UI_FALLBACK,
            size: 18.,
            line_height: 1.64,
            measure_em: 37.5,
            block_gap_em: 1.,
            headings: [
                HeadingSpec { scale: 23. / 18., top: 60. / 23., bottom: 60. / 23., rule: false },
                HeadingSpec { scale: 21. / 18., top: 60. / 21., bottom: 30. / 21., rule: false },
                same(30. / 18., 0.),
                same(30. / 18., 0.),
                same(30. / 18., 0.),
                same(30. / 18., 0.),
            ],
            muted_h6: false,
            stacked_heading_em: -40. / 21.,
        },
        Template::GitHub => Spec {
            family: crate::fonts::ui(),
            size: 16.,
            line_height: 1.5,
            measure_em: 980. / 16.,
            block_gap_em: 1.,
            headings: [
                HeadingSpec { scale: 2., top: 0.75, bottom: 0.5, rule: true },
                HeadingSpec { scale: 1.5, top: 1., bottom: 0.67, rule: true },
                HeadingSpec { scale: 1.25, top: 1.2, bottom: 0.8, rule: false },
                HeadingSpec { scale: 1., top: 1.5, bottom: 1., rule: false },
                HeadingSpec { scale: 0.875, top: 1.7, bottom: 1.15, rule: false },
                HeadingSpec { scale: 0.85, top: 1.75, bottom: 1.2, rule: false },
            ],
            muted_h6: true,
            stacked_heading_em: 0.,
        },
    }
}

pub struct Preview {
    /// Bottom-right window corner radius (the preview sits in that corner).
    pub corner: gpui::Pixels,
    blocks: Vec<Block>,
    template: Template,
    scroll: ScrollHandle,
}

impl Preview {
    pub fn new() -> Self {
        Self { corner: px(0.), blocks: Vec::new(), template: Template::default(), scroll: ScrollHandle::new() }
    }

    pub fn update(&mut self, md: &str, base: Option<PathBuf>, template: Template, cx: &mut Context<Self>) {
        self.blocks = resolve_content_blocks(parse(md), base.as_deref(), 0);
        self.template = template;
        cx.notify();
    }
}

struct Ctx<'a> {
    theme: &'a Theme,
    spec: Spec,
    toc: &'a [(u8, String)],
}

fn styled(spans: &[Span], c: &Ctx, id: impl Into<gpui::ElementId>, base_bold: bool) -> AnyElement {
    let mut text = String::new();
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    let mut links: Vec<(Range<usize>, String)> = Vec::new();
    for s in spans {
        let start = text.len();
        text.push_str(&s.text);
        let r = start..text.len();
        if r.is_empty() {
            continue;
        }
        let mut h = HighlightStyle::default();
        if s.bold || base_bold {
            h.font_weight = Some(FontWeight::BOLD);
        }
        if s.italic {
            h.font_style = Some(FontStyle::Italic);
        }
        if s.strike {
            h.strikethrough = Some(StrikethroughStyle { thickness: px(1.5), color: Some(c.theme.text) });
        }
        if s.mark {
            h.background_color = Some(c.theme.highlight);
            h.color = Some(c.theme.highlight_text);
            h.underline = Some(UnderlineStyle { thickness: px(c.spec.size * 0.175), color: Some(c.theme.highlight_underline), wavy: false });
        }
        if s.code {
            h.background_color = Some(c.theme.code_bg);
        }
        if let Some(url) = &s.link {
            h.underline = Some(UnderlineStyle { thickness: px(1.), color: Some(c.theme.secondary), wavy: false });
            links.push((r.clone(), url.clone()));
        }
        highlights.push((r, h));
    }
    let st = StyledText::new(text).with_highlights(highlights);
    if links.is_empty() {
        st.into_any_element()
    } else {
        let ranges = links.iter().map(|(r, _)| r.clone()).collect();
        let urls: Vec<String> = links.into_iter().map(|(_, u)| u).collect();
        InteractiveText::new(id, st)
            .on_click(ranges, move |ix, _, cx| {
                if urls[ix].starts_with("http") || urls[ix].starts_with("mailto:") {
                    cx.open_url(&urls[ix]);
                }
            })
            .into_any_element()
    }
}

/// CSS vertical margin collapsing: largest positive plus most negative.
pub fn collapse(a: f32, b: f32) -> f32 {
    a.max(b).max(0.) + a.min(b).min(0.)
}

/// (margin-top, margin-bottom) in px for a block, following the template rules.
fn margins(b: &Block, prev: Option<&Block>, c: &Ctx) -> (f32, f32) {
    let size = c.spec.size;
    match b {
        Block::Heading(l, _) => {
            let h = c.spec.headings[(*l as usize).clamp(1, 6) - 1];
            let fs = size * h.scale;
            let top = match prev {
                None => 0.,
                Some(Block::Heading(pl, _)) if *l <= 2 && *pl <= 2 => fs * c.spec.stacked_heading_em,
                Some(Block::Heading(pl, _)) if *pl <= 5 && *l >= 3 => 0.,
                _ => fs * h.top,
            };
            (top, fs * h.bottom)
        }
        Block::Rule => (size * 3., size * 3.),
        _ => match prev {
            None => (0., 0.),
            // `:any(h3, h4, h5) + p { margin-top: 0 }`
            Some(Block::Heading(pl, _)) if (3..=5).contains(pl) => (0., 0.),
            _ => (size * c.spec.block_gap_em, 0.),
        },
    }
}

fn render_blocks(blocks: &[Block], c: &Ctx, path: &str) -> Vec<AnyElement> {
    let mut prev_bottom = 0.;
    blocks
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let prev = i.checked_sub(1).map(|p| &blocks[p]);
            let (top, bottom) = margins(b, prev, c);
            let gap = if prev.is_none() { top } else { collapse(prev_bottom, top) };
            prev_bottom = bottom;
            let el = render_block(b, c, &format!("{path}.{i}"));
            div().mt(px(gap)).child(el).into_any_element()
        })
        .collect()
}

fn render_block(b: &Block, c: &Ctx, id: &str) -> AnyElement {
    let t = c.theme;
    let size = c.spec.size;
    let mono: SharedString = crate::fonts::MONO.into();
    match b {
        Block::Heading(level, spans) => {
            let h = c.spec.headings[(*level as usize).clamp(1, 6) - 1];
            let fs = size * h.scale;
            div()
                .text_size(px(fs))
                .line_height(relative(1.3))
                .when(h.rule, |d| d.border_b_1().border_color(t.separator).pb(px(fs * 0.3)))
                .when(*level == 6 && c.spec.muted_h6, |d| d.text_color(t.secondary))
                .child(styled(spans, c, SharedString::from(format!("h{id}")), *level != 5))
                .into_any_element()
        }
        Block::Para(spans) => styled(spans, c, SharedString::from(format!("p{id}")), false),
        Block::List { start, items } => {
            let mut list = div().flex().flex_col();
            for (n, item) in items.iter().enumerate() {
                let marker: AnyElement = match (item.task, start) {
                    (Some(done), _) => div()
                        .mt(px(size * 0.4))
                        .size(px(size * 0.7))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(t.text)
                        .when(done, |d| d.bg(t.text))
                        .into_any_element(),
                    (None, Some(s)) => div().child(format!("{}.", s + n as u64)).into_any_element(),
                    (None, None) => div().child("•").into_any_element(),
                };
                list = list.child(
                    div()
                        .flex()
                        .gap(px(size * 0.6))
                        .child(div().w(px(size * 1.2)).flex().justify_end().child(marker))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .when(item.task == Some(true), |d| d.text_color(t.secondary))
                                .children(
                                    item.blocks
                                        .iter()
                                        .enumerate()
                                        .map(|(k, b)| render_block(b, c, &format!("{id}.{n}.{k}"))),
                                ),
                        ),
                );
            }
            list.into_any_element()
        }
        Block::Quote(inner) => div()
            .pl(px(size))
            .border_l_2()
            .border_color(t.tertiary)
            .italic()
            .children(render_blocks(inner, c, id))
            .into_any_element(),
        Block::Alert(kind, inner) => div()
            .pl(px(size))
            .border_l_2()
            .border_color(t.accent)
            .child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child(capitalize(kind)))
            .children(render_blocks(inner, c, id))
            .into_any_element(),
        Block::Code { text, .. } | Block::Math(text) => div()
            .p(px(size))
            .rounded(px(4.))
            .bg(t.code_bg)
            .font_family(mono)
            .text_size(px(size * 0.85))
            .child(text.clone())
            .into_any_element(),
        Block::Rule => div().h(px(1.)).bg(t.separator).into_any_element(),
        Block::PageBreak => div()
            .my(px(size * 1.5))
            .flex()
            .items_center()
            .gap_2()
            .text_color(t.tertiary)
            .text_size(px(11.))
            .child(div().flex_1().h(px(1.)).bg(t.separator))
            .child("Page Break")
            .child(div().flex_1().h(px(1.)).bg(t.separator))
            .into_any_element(),
        Block::Table { head, rows, align } => {
            let cell = |spans: &Vec<Span>, bold: bool, k: String, i: usize| {
                let a = align.get(i).copied().unwrap_or(Alignment::None);
                div()
                    .flex_1()
                    .py(px(size * 0.3))
                    .px(px(size * 0.5))
                    .flex()
                    .when(a == Alignment::Right, |d| d.justify_end())
                    .when(a == Alignment::Center, |d| d.justify_center())
                    .child(styled(spans, c, SharedString::from(k), bold))
            };
            let is_merged = |s: &Vec<Span>| s.len() == 1 && s[0].text == MERGED;
            let row_el = |cells: &Vec<Vec<Span>>, bold: bool, r: usize| {
                let mut row = div().flex().border_b_1().border_color(t.separator);
                for (i, s) in cells.iter().enumerate() {
                    if is_merged(s) {
                        continue;
                    }
                    let span = 1 + cells[i + 1..].iter().take_while(|c| is_merged(c)).count();
                    let mut el = cell(s, bold, format!("t{id}.{r}.{i}"), i);
                    el.style().flex_grow = Some(span as f32);
                    row = row.child(el);
                }
                row
            };
            div()
                .flex()
                .flex_col()
                .child(row_el(head, true, 0))
                .children(rows.iter().enumerate().map(|(r, row)| row_el(row, false, r + 1)))
                .into_any_element()
        }
        Block::Image { src, alt } => {
            let el = if src.starts_with("http://") || src.starts_with("https://") {
                img(SharedString::from(src.clone())).max_w_full().into_any_element()
            } else {
                img(PathBuf::from(src)).max_w_full().into_any_element()
            };
            div()
                .flex()
                .flex_col()
                .child(el)
                .when(!alt.is_empty(), |d| d.child(div().text_size(px(size * 0.75)).text_color(t.secondary).child(alt.clone())))
                .into_any_element()
        }
        Block::Footnote(label, inner) => div()
            .flex()
            .gap_2()
            .text_size(px(size * 0.8))
            .text_color(t.secondary)
            .child(format!("[{label}]"))
            .child(div().flex_1().children(render_blocks(inner, c, id)))
            .into_any_element(),
        Block::Toc => div()
            .flex()
            .flex_col()
            .children(c.toc.iter().map(|(level, text)| {
                div().pl(px(size * (*level as f32 - 1.) * 1.2)).child(text.clone())
            }))
            .into_any_element(),
    }
}

fn capitalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut c = lower.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

impl Render for Preview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let toc = headings(&self.blocks);
        let c = Ctx { theme: &theme, spec: spec(self.template), toc: &toc };
        let children = render_blocks(&self.blocks, &c, "b");
        div()
            .id("preview")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .bg(theme.preview_bg)
            .rounded_br(self.corner)
            .text_color(theme.text)
            .font_family(c.spec.family)
            .text_size(px(c.spec.size))
            .line_height(relative(c.spec.line_height))
            .child(
                div().w_full().flex().justify_center().child(
                    // `html.mac body { --padding: 72px }` in the templates.
                    div()
                        .max_w(px(c.spec.size * c.spec.measure_em + 144.))
                        .w_full()
                        .px(px(72.))
                        .pt(px(72.))
                        .pb(px(240.))
                        .children(children),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn multimarkdown_colspan() {
        let b = parse("| a | b | c |\n|---|---|---|\n| x || y |\n");
        let Block::Table { rows, .. } = &b[0] else { panic!() };
        assert_eq!(text(&rows[0][1]), MERGED);
        assert_eq!(text(&rows[0][2]), "y");
    }

    #[test]
    fn css_margin_collapsing() {
        // Serif: h1 bottom 62 px, h2 directly after pulls up by 46 px → 16 px gap.
        assert_eq!(collapse(62., -46.), 16.);
        assert_eq!(collapse(31., 18.), 31.);
        assert_eq!(collapse(-5., -10.), -10.);
    }

    #[test]
    fn blocks_and_inlines() {
        let b = parse("# Title\n\nA **b** ==m== [l](http://x).\n\n---\n\n> q\n\n```rs\ncode\n```\n");
        assert!(matches!(&b[0], Block::Heading(1, s) if text(s) == "Title"));
        let Block::Para(p) = &b[1] else { panic!() };
        assert_eq!(text(p), "A b m l.");
        assert!(p.iter().any(|s| s.bold && s.text == "b"));
        assert!(p.iter().any(|s| s.mark && s.text == "m"));
        assert!(p.iter().any(|s| s.link.as_deref() == Some("http://x")));
        assert_eq!(b[2], Block::Rule);
        assert!(matches!(&b[3], Block::Quote(_)));
        assert_eq!(b[4], Block::Code { lang: "rs".into(), text: "code".into() });
    }

    #[test]
    fn lists_tasks_tables_alerts() {
        let b = parse("- [x] done\n- todo\n\n1. one\n2. two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> [!NOTE]\n> hi\n");
        let Block::List { start: None, items } = &b[0] else { panic!("{:?}", b[0]) };
        assert_eq!(items[0].task, Some(true));
        assert!(matches!(&items[1].blocks[0], Block::Para(s) if text(s) == "todo"));
        assert!(matches!(&b[1], Block::List { start: Some(1), items } if items.len() == 2));
        let Block::Table { head, rows, .. } = &b[2] else { panic!() };
        assert_eq!(text(&head[1]), "b");
        assert_eq!(text(&rows[0][0]), "1");
        assert!(matches!(&b[3], Block::Alert(k, inner) if k == "NOTE" && inner.len() == 1));
    }

    #[test]
    fn ia_extensions() {
        let b = parse("{{TOC}}\n\n# A\n\n+++\n\n// hidden\nshown");
        assert_eq!(b[0], Block::Toc);
        assert_eq!(b[2], Block::PageBreak);
        assert!(matches!(&b[3], Block::Para(s) if text(s) == "shown"));
        assert_eq!(headings(&b), vec![(1, "A".to_string())]);

        let dir = std::env::temp_dir().join(format!("quill-cb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("part.md"), "## Part").unwrap();
        std::fs::write(dir.join("data.csv"), "x,y\n1,2").unwrap();
        let b = resolve_content_blocks(parse("/part.md\n\n/data.csv\n\n/pic.png"), Some(&dir), 0);
        assert!(matches!(&b[0], Block::Heading(2, _)));
        assert!(matches!(&b[1], Block::Table { rows, .. } if rows.len() == 1));
        assert!(matches!(&b[2], Block::Image { .. }));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
