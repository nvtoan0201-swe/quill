//! Single-line text input used by the find bar, Quick Search and dialogs.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, ShapedLine, SharedString, Style, TextRun,
    UTF16Selection, Window, div, fill, point, prelude::*, px, relative, size,
};

use crate::actions::*;
use crate::buffer::{Buffer, EditKind};
use crate::theme::Theme;

pub enum FieldEvent {
    Changed(String),
    Confirm,
    Up,
    Down,
}

pub struct TextField {
    focus_handle: FocusHandle,
    pub buffer: Buffer,
    placeholder: SharedString,
    layout: Option<(ShapedLine, Bounds<Pixels>)>,
    selecting: bool,
}

impl EventEmitter<FieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextField {
    pub fn new(placeholder: &str, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            buffer: Buffer::new(""),
            placeholder: SharedString::from(placeholder.to_string()),
            layout: None,
            selecting: false,
        }
    }

    pub fn text(&self) -> String {
        self.buffer.text().to_string()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buffer.reset(text);
        let n = self.buffer.len();
        self.buffer.set_selection(0, n);
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(FieldEvent::Changed(self.text()));
        cx.notify();
    }

    fn replace(&mut self, r: Range<usize>, t: &str, cx: &mut Context<Self>) {
        let t = t.replace('\n', " ");
        let kind = if t.chars().count() == 1 { EditKind::Typing } else { EditKind::Other };
        self.buffer.replace(r, &t, kind);
        self.buffer.marked = None;
        self.changed(cx);
    }

    fn move_to(&mut self, o: usize, cx: &mut Context<Self>) {
        self.buffer.move_to(o);
        cx.notify();
    }

    fn extend_to(&mut self, o: usize, cx: &mut Context<Self>) {
        self.buffer.extend_to(o);
        cx.notify();
    }

    fn index_for(&self, p: Point<Pixels>) -> usize {
        let Some((line, bounds)) = &self.layout else { return 0 };
        if p.x <= bounds.left() {
            return 0;
        }
        line.closest_index_for_x(p.x - bounds.left()).min(self.buffer.len())
    }

    fn utf16(&self, o: usize) -> usize {
        self.buffer.text()[..o.min(self.buffer.len())].encode_utf16().count()
    }

    fn utf8(&self, u: usize) -> usize {
        let mut n = 0;
        for (i, c) in self.buffer.text().char_indices() {
            if n >= u {
                return i;
            }
            n += c.len_utf16();
        }
        self.buffer.len()
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(&mut self, r: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let r8 = self.utf8(r.start)..self.utf8(r.end);
        actual.replace(self.utf16(r8.start)..self.utf16(r8.end));
        Some(self.buffer.text()[r8].to_string())
    }
    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let s = self.buffer.selection();
        Some(UTF16Selection { range: self.utf16(s.start)..self.utf16(s.end), reversed: self.buffer.is_reversed() })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer.marked.as_ref().map(|r| self.utf16(r.start)..self.utf16(r.end))
    }
    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.buffer.marked = None;
    }
    fn replace_text_in_range(&mut self, r: Option<Range<usize>>, t: &str, _: &mut Window, cx: &mut Context<Self>) {
        let r = r
            .map(|r| self.utf8(r.start)..self.utf8(r.end))
            .or(self.buffer.marked.clone())
            .unwrap_or(self.buffer.selection());
        self.replace(r, t, cx);
    }
    fn replace_and_mark_text_in_range(&mut self, r: Option<Range<usize>>, t: &str, _: Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) {
        let r = r
            .map(|r| self.utf8(r.start)..self.utf8(r.end))
            .or(self.buffer.marked.clone())
            .unwrap_or(self.buffer.selection());
        self.buffer.replace_untracked(r.clone(), t);
        self.buffer.marked = (!t.is_empty()).then(|| r.start..r.start + t.len());
        cx.notify();
    }
    fn bounds_for_range(&mut self, r: Range<usize>, _: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let (line, b) = self.layout.as_ref()?;
        let x = line.x_for_index(self.utf8(r.start));
        Some(Bounds::new(point(b.left() + x, b.top()), size(px(2.), b.size.height)))
    }
    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let i = self.index_for(p);
        Some(self.utf16(i))
    }
}

struct FieldElement {
    field: Entity<TextField>,
}

impl IntoElement for FieldElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for FieldElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<ShapedLine>;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Option<ShapedLine> {
        let theme = *cx.global::<Theme>();
        let f = self.field.read(cx);
        let style = window.text_style();
        let (text, color) = if f.buffer.text().is_empty() {
            (f.placeholder.clone(), theme.tertiary)
        } else {
            (SharedString::from(f.buffer.text().to_string()), style.color)
        };
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let size = style.font_size.to_pixels(window.rem_size());
        Some(window.text_system().shape_line(text, size, &[run], None))
    }
    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), line: &mut Option<ShapedLine>, window: &mut Window, cx: &mut App) {
        let theme = *cx.global::<Theme>();
        let focus = self.field.read(cx).focus_handle.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.field.clone()), cx);
        let line = line.take().unwrap();
        let f = self.field.read(cx);
        let sel = f.buffer.selection();
        let empty = f.buffer.text().is_empty();
        if !sel.is_empty() && !empty {
            let x0 = line.x_for_index(sel.start);
            let x1 = line.x_for_index(sel.end);
            window.paint_quad(fill(Bounds::new(point(bounds.left() + x0, bounds.top()), size(x1 - x0, bounds.size.height)), theme.selection));
        }
        let _ = line.paint(bounds.origin, window.line_height(), window, cx);
        if focus.is_focused(window) && sel.is_empty() {
            let x = if empty { px(0.) } else { line.x_for_index(sel.start) };
            window.paint_quad(fill(Bounds::new(point(bounds.left() + x, bounds.top() + px(2.)), size(px(2.), bounds.size.height - px(4.))), theme.caret));
        }
        let stored = if empty { None } else { Some((line, bounds)) };
        self.field.update(cx, |f, _| f.layout = stored);
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("TextField")
            .track_focus(&self.focus_handle)
            .w_full()
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(|f, _: &Left, _, cx| {
                let s = f.buffer.selection();
                let to = if s.is_empty() { f.buffer.prev_grapheme(s.start) } else { s.start };
                f.move_to(to, cx)
            }))
            .on_action(cx.listener(|f, _: &Right, _, cx| {
                let s = f.buffer.selection();
                let to = if s.is_empty() { f.buffer.next_grapheme(s.end) } else { s.end };
                f.move_to(to, cx)
            }))
            .on_action(cx.listener(|f, _: &SelectLeft, _, cx| {
                let to = f.buffer.prev_grapheme(f.buffer.head());
                f.extend_to(to, cx)
            }))
            .on_action(cx.listener(|f, _: &SelectRight, _, cx| {
                let to = f.buffer.next_grapheme(f.buffer.head());
                f.extend_to(to, cx)
            }))
            .on_action(cx.listener(|f, _: &WordLeft, _, cx| {
                let to = f.buffer.prev_word(f.buffer.head());
                f.move_to(to, cx)
            }))
            .on_action(cx.listener(|f, _: &WordRight, _, cx| {
                let to = f.buffer.next_word(f.buffer.head());
                f.move_to(to, cx)
            }))
            .on_action(cx.listener(|f, _: &LineStart, _, cx| f.move_to(0, cx)))
            .on_action(cx.listener(|f, _: &LineEnd, _, cx| {
                let n = f.buffer.len();
                f.move_to(n, cx)
            }))
            .on_action(cx.listener(|f, _: &SelectAll, _, cx| {
                let n = f.buffer.len();
                f.buffer.set_selection(0, n);
                cx.notify()
            }))
            .on_action(cx.listener(|f, _: &Backspace, _, cx| {
                let mut s = f.buffer.selection();
                if s.is_empty() {
                    s = f.buffer.prev_grapheme(s.start)..s.start;
                }
                f.replace(s, "", cx)
            }))
            .on_action(cx.listener(|f, _: &Delete, _, cx| {
                let mut s = f.buffer.selection();
                if s.is_empty() {
                    s = s.start..f.buffer.next_grapheme(s.start);
                }
                f.replace(s, "", cx)
            }))
            .on_action(cx.listener(|f, _: &DeleteWordLeft, _, cx| {
                let h = f.buffer.head();
                let p = f.buffer.prev_word(h);
                f.replace(p..h, "", cx)
            }))
            .on_action(cx.listener(|f, _: &Copy, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(f.buffer.selected_text().to_string()))
            }))
            .on_action(cx.listener(|f, _: &Cut, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(f.buffer.selected_text().to_string()));
                let s = f.buffer.selection();
                f.replace(s, "", cx)
            }))
            .on_action(cx.listener(|f, _: &Paste, _, cx| {
                if let Some(t) = cx.read_from_clipboard().and_then(|i| i.text()) {
                    let s = f.buffer.selection();
                    f.replace(s, &t, cx)
                }
            }))
            .on_action(cx.listener(|f, _: &Undo, _, cx| {
                if f.buffer.undo() {
                    f.changed(cx)
                }
            }))
            .on_action(cx.listener(|_, _: &Confirm, _, cx| cx.emit(FieldEvent::Confirm)))
            .on_action(cx.listener(|_, _: &FieldUp, _, cx| cx.emit(FieldEvent::Up)))
            .on_action(cx.listener(|_, _: &FieldDown, _, cx| cx.emit(FieldEvent::Down)))
            .on_mouse_down(MouseButton::Left, cx.listener(|f, e: &MouseDownEvent, window, cx| {
                window.focus(&f.focus_handle);
                f.selecting = true;
                let i = f.index_for(e.position);
                if e.modifiers.shift { f.extend_to(i, cx) } else { f.move_to(i, cx) }
            }))
            .on_mouse_move(cx.listener(|f, e: &MouseMoveEvent, _, cx| {
                if f.selecting {
                    let i = f.index_for(e.position);
                    f.extend_to(i, cx)
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|f, _: &MouseUpEvent, _, _| f.selecting = false))
            .child(FieldElement { field: cx.entity() })
    }
}
