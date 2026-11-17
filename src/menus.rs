//! In-window menus (GPUI has no global menu bar on Linux): a model plus a
//! macOS-style popup renderer with cascading submenus.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    Action, AnyElement, App, Global, Hsla, IntoElement, MouseButton, ParentElement, Pixels,
    Point, SharedString, Styled, Window, div, point, prelude::*, px, svg,
};

use crate::theme::Theme;

pub type Callback = Rc<dyn Fn(&mut Window, &mut App)>;

pub enum MenuItem {
    Item {
        label: SharedString,
        action: Option<Box<dyn Action>>,
        callback: Option<Callback>,
        checked: bool,
        enabled: bool,
        /// Label color (the original colors the Syntax items).
        color: Option<Hsla>,
        /// "–": mode chosen but feature off, as in the original's Focus menu.
        dash: bool,
    },
    Sub {
        label: SharedString,
        items: Vec<MenuItem>,
    },
    Sep,
}

pub fn act(label: &str, action: impl Action) -> MenuItem {
    MenuItem::Item {
        label: SharedString::from(label.to_string()),
        action: Some(Box::new(action)),
        callback: None,
        checked: false,
        enabled: true,
        color: None,
        dash: false,
    }
}

pub fn check(label: &str, action: impl Action, checked: bool) -> MenuItem {
    MenuItem::Item {
        label: SharedString::from(label.to_string()),
        action: Some(Box::new(action)),
        callback: None,
        checked,
        enabled: true,
        color: None,
        dash: false,
    }
}

pub fn call(label: &str, f: impl Fn(&mut Window, &mut App) + 'static) -> MenuItem {
    MenuItem::Item {
        label: SharedString::from(label.to_string()),
        action: None,
        callback: Some(Rc::new(f)),
        checked: false,
        enabled: true,
        color: None,
        dash: false,
    }
}

pub fn disabled(label: &str) -> MenuItem {
    MenuItem::Item {
        label: SharedString::from(label.to_string()),
        action: None,
        callback: None,
        checked: false,
        enabled: false,
        color: None,
        dash: false,
    }
}

impl MenuItem {
    pub fn colored(mut self, c: Hsla) -> Self {
        if let MenuItem::Item { color, .. } = &mut self {
            *color = Some(c);
        }
        self
    }

    pub fn dashed(mut self, on: bool) -> Self {
        if let MenuItem::Item { dash, .. } = &mut self {
            *dash = on;
        }
        self
    }
}

pub fn sub(label: &str, items: Vec<MenuItem>) -> MenuItem {
    MenuItem::Sub { label: SharedString::from(label.to_string()), items }
}

pub fn sep() -> MenuItem {
    MenuItem::Sep
}

/// Action name → display shortcut, built once from the keymap.
#[derive(Default)]
pub struct Shortcuts(pub HashMap<&'static str, String>);

impl Global for Shortcuts {}

pub fn format_keystroke(s: &str) -> String {
    // The key itself may be "-", so split it off before splitting modifiers.
    let (mods, key) = match s.strip_suffix("--") {
        Some(m) => (m, "-"),
        None => s.rsplit_once('-').unwrap_or(("", s)),
    };
    let key = match key {
        "backspace" => "⌫".to_string(),
        "delete" => "Del".to_string(),
        "escape" => "Esc".to_string(),
        "enter" => "↩".to_string(),
        "tab" => "Tab".to_string(),
        "space" => "Space".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "pageup" => "PgUp".to_string(),
        "pagedown" => "PgDn".to_string(),
        k => k.to_uppercase(),
    };
    mods.split('-')
        .filter(|m| !m.is_empty())
        .map(|m| match m {
            "ctrl" => "Ctrl",
            "alt" => "Alt",
            "shift" => "Shift",
            "super" => "Super",
            other => other,
        })
        .map(str::to_string)
        .chain([key])
        .collect::<Vec<_>>()
        .join("+")
}

pub fn build_shortcuts(cx: &mut App) {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let mut map = HashMap::new();
    for b in keymap.bindings() {
        let name = b.action().name();
        if map.contains_key(name) {
            continue;
        }
        let text = b.keystrokes().iter().map(|k| format_keystroke(&k.unparse())).collect::<Vec<_>>().join(" ");
        map.insert(name, text);
    }
    drop(keymap);
    cx.set_global(Shortcuts(map));
}

pub const ITEM_H: f32 = 24.;
pub const SEP_H: f32 = 11.;
pub const PAD: f32 = 5.;
pub const WIDTH: f32 = 270.;

fn height_of(items: &[MenuItem]) -> f32 {
    items.iter().map(|i| if matches!(i, MenuItem::Sep) { SEP_H } else { ITEM_H }).sum::<f32>() + PAD * 2.
}

/// Render a menu and its open submenus as absolutely-positioned panels.
/// `open` is the path of the highlighted item; every submenu on that path is open.
pub fn render(
    items: &[MenuItem],
    origin: Point<Pixels>,
    open: &[usize],
    window_size: gpui::Size<Pixels>,
    on_hover: Rc<dyn Fn(Vec<usize>, &mut Window, &mut App)>,
    on_done: Rc<dyn Fn(&mut Window, &mut App)>,
    cx: &App,
) -> Vec<AnyElement> {
    let mut panels = Vec::new();
    let mut level_items = items;
    let mut level_origin = origin;
    let mut path: Vec<usize> = Vec::new();
    loop {
        let h = height_of(level_items);
        // Keep panels on screen.
        let mut o = level_origin;
        if o.y + px(h) > window_size.height {
            o.y = (window_size.height - px(h) - px(4.)).max(px(4.));
        }
        if o.x + px(WIDTH) > window_size.width {
            o.x = (o.x - px(WIDTH * 2.)).max(px(4.));
        }
        panels.push(panel(level_items, o, &path, open.get(path.len()).copied(), on_hover.clone(), on_done.clone(), cx));
        let Some(&ix) = open.get(path.len()) else { break };
        let Some(MenuItem::Sub { items, .. }) = level_items.get(ix) else { break };
        let y: f32 = level_items[..ix]
            .iter()
            .map(|i| if matches!(i, MenuItem::Sep) { SEP_H } else { ITEM_H })
            .sum();
        level_origin = point(o.x + px(WIDTH - 4.), o.y + px(y));
        path.push(ix);
        level_items = items;
    }
    panels
}

fn panel(
    items: &[MenuItem],
    origin: Point<Pixels>,
    path: &[usize],
    open_ix: Option<usize>,
    on_hover: Rc<dyn Fn(Vec<usize>, &mut Window, &mut App)>,
    on_done: Rc<dyn Fn(&mut Window, &mut App)>,
    cx: &App,
) -> AnyElement {
    let t = *cx.global::<Theme>();
    let shortcuts = cx.try_global::<Shortcuts>();
    let mut col = div()
        .absolute()
        .left(origin.x)
        .top(origin.y)
        .w(px(WIDTH))
        .p(px(PAD))
        .bg(t.popover_bg)
        .border_1()
        .border_color(t.popover_border)
        .rounded(px(8.))
        .shadow_lg()
        .font_family(crate::fonts::ui())
        .text_size(px(13.))
        .text_color(t.text)
        .flex()
        .flex_col()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    for (i, item) in items.iter().enumerate() {
        let mut here = path.to_vec();
        here.push(i);
        let row_base = || {
            div()
                .id(SharedString::from(format!("menu-{here:?}")))
                .h(px(ITEM_H))
                .px(px(6.))
                .rounded(px(4.))
                .flex()
                .items_center()
        };
        match item {
            MenuItem::Sep => {
                col = col.child(div().h(px(SEP_H)).flex().items_center().px(px(6.)).child(div().w_full().h(px(1.)).bg(t.separator)));
            }
            MenuItem::Item { label, action, callback, checked, enabled, color, dash } => {
                let shortcut = action
                    .as_ref()
                    .and_then(|a| shortcuts.and_then(|s| s.0.get(a.name()).cloned()))
                    .unwrap_or_default();
                let hover = on_hover.clone();
                let path_for_hover = path.to_vec();
                let done = on_done.clone();
                let action = action.as_ref().map(|a| a.boxed_clone());
                let callback = callback.clone();
                let enabled = *enabled;
                let fg: Hsla = if enabled { t.text } else { t.tertiary };
                let hl = open_ix == Some(i) && enabled;
                col = col.child(
                    row_base()
                        .text_color(if hl { t.row_selected_text } else { fg })
                        .when(hl, |d| d.bg(t.row_selected))
                        .when(enabled, |d| {
                            d.on_mouse_move(move |_, window, cx| hover(path_for_hover.clone(), window, cx))
                                .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                                    done(window, cx);
                                    if let Some(a) = &action {
                                        window.dispatch_action(a.boxed_clone(), cx);
                                    }
                                    if let Some(f) = &callback {
                                        f(window, cx);
                                    }
                                })
                        })
                        .child(
                            div()
                                .w(px(18.))
                                .when(*checked, |d| {
                                    d.child(svg().path("icons/check.svg").size(px(12.)).text_color(if hl { t.row_selected_text } else { fg }))
                                })
                                .when(*dash && !*checked, |d| d.child("–")),
                        )
                        .child(div().flex_1().when_some(color.filter(|_| !hl), |d, c| d.text_color(c)).child(label.clone()))
                        .child(div().text_color(if hl { t.row_selected_text } else { t.secondary }).text_size(px(12.)).child(shortcut)),
                );
            }
            MenuItem::Sub { label, .. } => {
                let hover = on_hover.clone();
                let open_path = here.clone();
                let is_open = open_ix == Some(i);
                col = col.child(
                    row_base()
                        .when(is_open, |d| d.bg(t.row_selected).text_color(t.row_selected_text))
                        .on_mouse_move(move |_, window, cx| hover(open_path.clone(), window, cx))
                        .child(div().w(px(18.)))
                        .child(div().flex_1().child(label.clone()))
                        .child(svg().path("icons/chevron-right.svg").size(px(11.)).text_color(if is_open { t.row_selected_text } else { t.secondary })),
                );
            }
        }
    }
    col.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_navigation() {
        let items = vec![
            disabled("x"),
            sep(),
            sub("S", vec![sep(), call("a", |_, _| {}), call("b", |_, _| {})]),
            call("c", |_, _| {}),
        ];
        assert_eq!(navigate(&items, &[], "down"), vec![2]);
        assert_eq!(navigate(&items, &[2], "down"), vec![3]);
        assert_eq!(navigate(&items, &[3], "down"), vec![2]);
        assert_eq!(navigate(&items, &[2], "up"), vec![3]);
        assert_eq!(navigate(&items, &[2], "right"), vec![2, 1]);
        assert_eq!(navigate(&items, &[2, 1], "up"), vec![2, 2]);
        assert_eq!(navigate(&items, &[2, 2], "left"), vec![2]);
    }

    #[test]
    fn keystroke_display() {
        assert_eq!(format_keystroke("ctrl-shift-s"), "Ctrl+Shift+S");
        assert_eq!(format_keystroke("ctrl--"), "Ctrl+-");
        assert_eq!(format_keystroke("ctrl-alt-backspace"), "Ctrl+Alt+⌫");
    }
}

/// Item at a path, if any.
pub fn item_at<'a>(items: &'a [MenuItem], path: &[usize]) -> Option<&'a MenuItem> {
    let (&last, parents) = path.split_last()?;
    let mut level = items;
    for &i in parents {
        match level.get(i)? {
            MenuItem::Sub { items, .. } => level = items,
            _ => return None,
        }
    }
    level.get(last)
}

/// Siblings of the item at `path`.
pub fn level_at<'a>(items: &'a [MenuItem], path: &[usize]) -> &'a [MenuItem] {
    let mut level = items;
    for &i in &path[..path.len().saturating_sub(1)] {
        match level.get(i) {
            Some(MenuItem::Sub { items, .. }) => level = items,
            _ => break,
        }
    }
    level
}

pub fn selectable(item: &MenuItem) -> bool {
    match item {
        MenuItem::Sep => false,
        MenuItem::Item { enabled, .. } => *enabled,
        MenuItem::Sub { .. } => true,
    }
}

/// Keyboard navigation: returns the new highlight path.
pub fn navigate(items: &[MenuItem], path: &[usize], key: &str) -> Vec<usize> {
    let mut p = path.to_vec();
    let first = |level: &[MenuItem]| level.iter().position(selectable);
    match key {
        "down" | "up" => {
            if p.is_empty() {
                return first(items).map(|i| vec![i]).unwrap_or_default();
            }
            let level = level_at(items, &p);
            let n = level.len();
            let mut i = *p.last().unwrap();
            for _ in 0..n {
                i = if key == "down" { (i + 1) % n } else { (i + n - 1) % n };
                if selectable(&level[i]) {
                    break;
                }
            }
            *p.last_mut().unwrap() = i;
        }
        "right" | "enter" => {
            if let Some(MenuItem::Sub { items: sub, .. }) = item_at(items, &p)
                && let Some(i) = first(sub)
            {
                p.push(i);
            }
        }
        "left" => {
            if p.len() > 1 {
                p.pop();
            }
        }
        _ => {}
    }
    p
}
