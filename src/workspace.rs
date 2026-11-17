//! The window: library sidebar, toolbar, tabs, editor, preview, status and overlays.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    AnyElement, App, AppContext, Bounds, Context, CursorStyle, Decorations, Entity, FocusHandle,
    Focusable, FontWeight, HitboxBehavior, Hsla, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Point, PromptLevel, Render, ResizeEdge, SharedString, Size, Styled,
    Subscription, Task, Window, WindowAppearance, canvas, div, point, prelude::*, px, svg,
    transparent_black,
};

use crate::actions::*;
use crate::editor::{Editor, EditorConfig, EditorEvent};
use crate::export::{self, Format};
use crate::format;
use crate::focus::FocusMode;
use crate::library::{self, Entry, Hit, SortBy};
use crate::menus::{self, MenuItem, act, call, check, sep, sub};
use crate::preview::Preview;
use crate::publish::{self, Account, Service};
use crate::settings::{Appearance, BarMode, FontChoice, PreviewLayout, Settings, TEXT_SIZES, Template};
use crate::stats::{self, StatKind, Stats};
use crate::style_check::Check;
use crate::syntax::Pos;
use crate::text_field::{FieldEvent, TextField};
use crate::theme::Theme;

mod chrome;
mod studio_panel;

use studio_panel::{StudioState, StudioTab};

const TOOLBAR_H: f32 = chrome::BAR_H;
const SHADOW: f32 = 10.;
/// Window corner radius, fitted on the reference screenshots (≈8–9 pt).
const ROUNDING: f32 = 9.;

struct Doc {
    editor: Entity<Editor>,
    save_task: Option<Task<()>>,
    /// Modification time when we last read or wrote the file.
    mtime: Option<SystemTime>,
    _sub: Subscription,
}

struct MenuOverlay {
    items: Vec<MenuItem>,
    origin: Point<Pixels>,
    open: Vec<usize>,
}

struct QuickSearchState {
    field: Entity<TextField>,
    results: Vec<Hit>,
    selected: usize,
    _sub: Subscription,
}

struct FindBar {
    query: Entity<TextField>,
    replace: Entity<TextField>,
    show_replace: bool,
    _subs: [Subscription; 2],
}

enum Overlay {
    None,
    Menu(MenuOverlay),
    QuickSearch(QuickSearchState),
    Preferences(Entity<TextField>, #[allow(dead_code)] Subscription),
    Accounts(Service, [Entity<TextField>; 3]),
    AllTabs,
    About,
}

pub struct Workspace {
    settings: Settings,
    docs: Vec<Doc>,
    active: usize,
    preview: Entity<Preview>,
    lib_dir: PathBuf,
    lib_root: PathBuf,
    entries: Vec<Entry>,
    tags: Vec<(String, usize)>,
    tag: Option<String>,
    smart: Option<String>,
    doc_back: Vec<PathBuf>,
    doc_fwd: Vec<PathBuf>,
    dir_back: Vec<PathBuf>,
    dir_fwd: Vec<PathBuf>,
    navigating: bool,
    /// Window corner radius this frame (0 when maximized/tiled/full screen).
    corner: Pixels,
    lib_sig: (Option<SystemTime>, Option<SystemTime>, usize),
    filter: Entity<TextField>,
    show_filter: bool,
    filter_text: String,
    overlay: Overlay,
    find: Option<FindBar>,
    toast: Option<(SharedString, Instant)>,
    last_mouse_move: Instant,
    stats: (u64, usize, Stats),
    speech: Option<std::process::Child>,
    focus_handle: FocusHandle,
    menu_focus: FocusHandle,
    _subs: Vec<Subscription>,
    preview_task: Option<Task<()>>,
    spell_task: Option<Task<()>>,
    studio: StudioState,
}

impl Workspace {
    pub fn new(args: Vec<String>, inbox: crate::ipc::Inbox, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut settings = Settings::load();
        if settings.url_token.is_empty() {
            settings.url_token = crate::urlcmd::new_token();
            settings.save();
        }
        let lib_root = settings.library_dir();
        let _ = std::fs::create_dir_all(&lib_root);
        let welcome = lib_root.join("Welcome.md");
        if std::fs::read_dir(&lib_root).map(|mut d| d.next().is_none()).unwrap_or(false) {
            let _ = std::fs::write(&welcome, crate::help::WELCOME);
        }
        let filter = cx.new(|cx| TextField::new("Filter", cx));
        let filter_sub = cx.subscribe(&filter, |ws: &mut Self, _, ev: &FieldEvent, cx| {
            if let FieldEvent::Changed(t) = ev {
                ws.filter_text = t.clone();
                cx.notify();
            }
        });
        let appearance_sub = cx.observe_window_appearance(window, |ws: &mut Self, window, cx| {
            ws.apply_theme(window, cx);
        });
        let mut ws = Self {
            settings,
            docs: Vec::new(),
            active: 0,
            preview: cx.new(|_| Preview::new()),
            lib_dir: lib_root.clone(),
            lib_root,
            entries: Vec::new(),
            tags: Vec::new(),
            tag: None,
            smart: None,
            doc_back: Vec::new(),
            doc_fwd: Vec::new(),
            dir_back: Vec::new(),
            dir_fwd: Vec::new(),
            navigating: false,
            corner: px(0.),
            lib_sig: (None, None, 0),
            filter,
            show_filter: false,
            filter_text: String::new(),
            overlay: Overlay::None,
            find: None,
            toast: None,
            last_mouse_move: Instant::now(),
            stats: (u64::MAX, usize::MAX, Stats::default()),
            speech: None,
            focus_handle: cx.focus_handle(),
            menu_focus: cx.focus_handle(),
            _subs: vec![filter_sub, appearance_sub],
            preview_task: None,
            spell_task: None,
            studio: StudioState::new(cx),
        };
        ws.apply_theme(window, cx);
        ws.refresh_library();
        let urls: Vec<String> = args.iter().filter(|a| a.starts_with("quill://")).cloned().collect();
        let mut paths: Vec<PathBuf> = args.iter().map(PathBuf::from).filter(|p| p.is_file()).collect();
        if paths.is_empty() {
            let last = ws.settings.recent.iter().find(|p| p.is_file()).cloned();
            paths.extend(last.or_else(|| welcome.is_file().then_some(welcome)));
        }
        for p in paths {
            ws.open_path(&p, true, window, cx);
        }
        if ws.docs.is_empty() {
            ws.new_doc(window, cx);
        }
        for u in urls {
            ws.handle_url(&u, window, cx);
        }
        // Background loop: forwarded arguments, external file changes, library changes.
        cx.spawn_in(window, async move |ws, cx| {
            let mut tick = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(250)).await;
                tick += 1;
                let msgs: Vec<String> = inbox.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
                let alive = ws.update_in(cx, |ws, window, cx| {
                    for m in msgs {
                        if m.is_empty() {
                            window.activate_window();
                        } else if m.starts_with("quill://") {
                            ws.handle_url(&m, window, cx);
                        } else {
                            ws.open_path(Path::new(&m), true, window, cx);
                        }
                    }
                    if tick % 4 == 0 {
                        ws.check_disk(window, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        ws
    }

    /// Pick up changes made by other programs or sync clients (Nextcloud, Dropbox, …).
    fn check_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for i in 0..self.docs.len() {
            let Some(path) = self.docs[i].editor.read(cx).path.clone() else { continue };
            let Some(now) = mtime_of(&path) else { continue };
            if self.docs[i].mtime.is_some_and(|m| m >= now) {
                continue;
            }
            self.docs[i].mtime = Some(now);
            let dirty = self.docs[i].editor.read(cx).buffer.is_dirty();
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if self.docs[i].editor.read(cx).text() == text {
                continue;
            }
            if dirty {
                self.show_toast(format!("{} changed on disk; your edits are kept", library::file_stem(&path)), cx);
                continue;
            }
            self.docs[i].editor.update(cx, |e, cx| {
                let head = e.buffer.head();
                e.buffer.reset(text);
                e.buffer.move_to(head);
                cx.notify();
            });
            if i == self.active {
                self.refresh_preview(cx);
                self.update_title(window, cx);
            }
        }
        let sig = library_signature(&self.lib_dir);
        if sig != self.lib_sig {
            self.lib_sig = sig;
            self.refresh_library();
            cx.notify();
        }
    }

    fn url_error(&mut self, cmd: &crate::urlcmd::UrlCommand, msg: String, cx: &mut Context<Self>) {
        if let Some(x) = cmd.params.get("x-error") {
            cx.open_url(&crate::urlcmd::callback_url(x, &[("errorMessage", &msg)]));
        }
        self.show_toast(msg, cx);
    }

    fn url_success(cmd: &crate::urlcmd::UrlCommand, params: &[(&str, &str)], cx: &mut Context<Self>) {
        if let Some(x) = cmd.params.get("x-success") {
            cx.open_url(&crate::urlcmd::callback_url(x, params));
        }
    }

    pub fn handle_url(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        use crate::urlcmd::{self, WriteMode};
        let Some(cmd) = urlcmd::parse(url) else { return };
        let get = |k: &str| cmd.params.get(k).cloned();
        let author = match get("author") {
            Some(a) if a.to_lowercase().contains("ai") || a.to_lowercase().contains("gpt") => crate::authors::Author::Ai,
            _ => crate::authors::Author::Human,
        };
        let needs_auth = matches!(cmd.name.as_str(), "read" | "write");
        if needs_auth && get("auth-token").as_deref() != Some(self.settings.url_token.as_str()) {
            return self.url_error(&cmd, "Missing or wrong auth-token (see Preferences)".into(), cx);
        }
        let root = self.lib_root.clone();
        let resolve = |p: &str| urlcmd::library_path(&root, p);
        match cmd.name.as_str() {
            "open" => match get("path").map(|p| resolve(&p)) {
                Some(Ok(p)) if p.is_file() => {
                    self.open_path(&p, true, window, cx);
                    Self::url_success(&cmd, &[], cx);
                }
                Some(Ok(_)) => {
                    self.new_doc(window, cx);
                    Self::url_success(&cmd, &[], cx);
                }
                Some(Err(e)) => self.url_error(&cmd, e.to_string(), cx),
                None => self.url_error(&cmd, "open needs a path".into(), cx),
            },
            "new" => {
                let text = get("text").unwrap_or_default();
                match get("path").map(|p| resolve(&p)) {
                    Some(Ok(p)) => {
                        let target = if p.is_dir() || get("path").is_some_and(|s| s.ends_with('/')) {
                            let _ = std::fs::create_dir_all(&p);
                            let name = Self::sanitize(&crate::markdown::title(&text));
                            library::unique_path(&p, &name, "md")
                        } else {
                            let stem = library::file_stem(&p);
                            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("md").to_string();
                            library::unique_path(p.parent().unwrap_or(&root), &stem, &ext)
                        };
                        if let Some(dir) = target.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        if let Err(e) = std::fs::write(&target, &text) {
                            return self.url_error(&cmd, e.to_string(), cx);
                        }
                        self.open_path(&target, false, window, cx);
                    }
                    Some(Err(e)) => return self.url_error(&cmd, e.to_string(), cx),
                    None => self.add_doc(&text, None, window, cx),
                }
                if author == crate::authors::Author::Ai && !text.is_empty() {
                    let n = self.editor().read(cx).text().len();
                    self.editor().update(cx, |e, _| e.authors.mark(0..n, author));
                }
                Self::url_success(&cmd, &[], cx);
            }
            "quick-search" => {
                self.open_quick_search(window, cx);
                if let (Some(q), Overlay::QuickSearch(qs)) = (get("query"), &mut self.overlay) {
                    qs.field.update(cx, |f, cx| f.set_text(&q, cx));
                    qs.results = library::search(&self.lib_root, &q, 12);
                }
                Self::url_success(&cmd, &[], cx);
            }
            "read" => match get("path").map(|p| resolve(&p)) {
                Some(Ok(p)) => match std::fs::read_to_string(&p) {
                    Ok(text) => Self::url_success(&cmd, &[("path", &get("path").unwrap_or_default()), ("text", &text)], cx),
                    Err(e) => self.url_error(&cmd, e.to_string(), cx),
                },
                Some(Err(e)) => self.url_error(&cmd, e.to_string(), cx),
                None => self.url_error(&cmd, "read needs a path".into(), cx),
            },
            "write" => {
                let mode = match WriteMode::parse(get("mode").as_deref()) {
                    Ok(m) => m,
                    Err(e) => return self.url_error(&cmd, e.to_string(), cx),
                };
                let Some(Ok(mut p)) = get("path").map(|p| resolve(&p)) else {
                    return self.url_error(&cmd, "write needs a valid path".into(), cx);
                };
                let text = get("text").unwrap_or_default();
                if mode == WriteMode::Create && p.exists() {
                    let stem = library::file_stem(&p);
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("md").to_string();
                    p = library::unique_path(p.parent().unwrap_or(&root), &stem, &ext);
                }
                let existing = std::fs::read_to_string(&p).unwrap_or_default();
                let new_text = match mode {
                    WriteMode::Create | WriteMode::Replace | WriteMode::Patch => text.clone(),
                    WriteMode::Add => urlcmd::add_text(
                        &existing,
                        &text,
                        get("add-location").as_deref() == Some("beginning"),
                        get("add-padding").as_deref().unwrap_or("line"),
                    ),
                };
                let open_ix = self.docs.iter().position(|d| d.editor.read(cx).path.as_deref() == Some(p.as_path()));
                if let Some(ix) = open_ix {
                    // Apply as a diff so authorship and undo work like Paste Edits.
                    let editor = self.docs[ix].editor.clone();
                    editor.update(cx, |e, cx| e.apply_external(&new_text, author, cx));
                    self.flush_save(ix, cx);
                } else {
                    if let Some(dir) = p.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if let Err(e) = std::fs::write(&p, &new_text) {
                        return self.url_error(&cmd, e.to_string(), cx);
                    }
                    if author == crate::authors::Author::Ai {
                        let mut a = crate::authors::Authors::load(&p, existing.len());
                        for h in crate::diff::diff(&existing, &new_text).iter().rev() {
                            a.edit(h.old.clone(), h.new.len(), author);
                        }
                        a.save(&p);
                    }
                }
                self.refresh_library();
                let rel = format!("/{}", p.strip_prefix(&root).unwrap_or(&p).display());
                Self::url_success(&cmd, &[("path", &rel), ("text", &new_text)], cx);
                cx.notify();
            }
            "version" => Self::url_success(&cmd, &[("version", env!("CARGO_PKG_VERSION"))], cx),
            other => self.url_error(&cmd, format!("Unknown command {other}"), cx),
        }
    }

    // ---- settings & theme ---------------------------------------------------

    fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = match self.settings.appearance {
            Appearance::Light => false,
            Appearance::Dark => true,
            Appearance::System => matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark),
        };
        let base = if dark { Theme::dark() } else { Theme::light() };
        cx.set_global(base.with_highlight(self.settings.highlight_style));
        cx.notify();
    }

    fn editor_config(&self) -> EditorConfig {
        let s = &self.settings;
        let (font_size, line_height) = s.font_metrics();
        let mut syntax = Vec::new();
        if s.syntax {
            for (on, p) in [
                (s.syntax_adjectives, Pos::Adjective),
                (s.syntax_nouns, Pos::Noun),
                (s.syntax_adverbs, Pos::Adverb),
                (s.syntax_verbs, Pos::Verb),
                (s.syntax_conjunctions, Pos::Conjunction),
            ] {
                if on {
                    syntax.push(p);
                }
            }
        }
        let mut checks = Vec::new();
        if s.style_check {
            for (on, c) in [
                (s.check_fillers, Check::Filler),
                (s.check_cliches, Check::Cliche),
                (s.check_redundancies, Check::Redundancy),
                (s.check_custom, Check::Custom),
            ] {
                if on {
                    checks.push(c);
                }
            }
        }
        EditorConfig {
            font_family: s.font.family().into(),
            font_size,
            line_height,
            focus: s.focus,
            typewriter: s.typewriter,
            syntax,
            checks,
            custom_patterns: s.custom_patterns.clone(),
            show_authors: s.show_authors,
            smart_quotes: s.smart_quotes,
            smart_dashes: s.smart_dashes,
            auto_pair: s.auto_pair,
            spell: s.spell,
        }
    }

    fn settings_changed(&mut self, cx: &mut Context<Self>) {
        self.settings.save();
        let config = self.editor_config();
        for d in &self.docs {
            d.editor.update(cx, |e, cx| e.set_config(config.clone(), cx));
        }
        self.refresh_preview(cx);
        self.schedule_spell(Duration::from_millis(50), cx);
        self.schedule_studio(Duration::from_millis(50), cx);
        cx.notify();
    }

    fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        self.settings_changed(cx);
    }

    // ---- documents ----------------------------------------------------------

    fn editor(&self) -> &Entity<Editor> {
        &self.docs[self.active].editor
    }

    fn add_doc(&mut self, text: &str, path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.editor_config();
        let editor = cx.new(|cx| Editor::new(text, path.clone(), config, cx));
        let sub = cx.subscribe_in(&editor, window, Self::on_editor_event);
        let mtime = path.as_deref().and_then(mtime_of);
        self.docs.push(Doc { editor: editor.clone(), save_task: None, mtime, _sub: sub });
        self.active = self.docs.len() - 1;
        window.focus(&editor.focus_handle(cx));
        self.after_switch(window, cx);
    }

    fn after_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.find {
            let q = f.query.read(cx).text();
            self.editor().update(cx, |e, cx| e.set_find_query(&q, cx));
        }
        self.update_title(window, cx);
        self.refresh_preview(cx);
        self.schedule_spell(Duration::from_millis(50), cx);
        self.schedule_studio(Duration::from_millis(50), cx);
        cx.notify();
    }

    pub fn new_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_doc("", None, window, cx);
    }

    fn is_blank(&self, ix: usize, cx: &App) -> bool {
        let e = self.docs[ix].editor.read(cx);
        e.path.is_none() && e.text().is_empty()
    }

    /// Open a file. `in_place` replaces the current tab when it is blank or clean.
    pub fn open_path(&mut self, path: &Path, in_place: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.navigating
            && let Some(cur) = self.docs.get(self.active).and_then(|d| d.editor.read(cx).path.clone())
            && cur != path
        {
            self.doc_back.push(cur);
            self.doc_fwd.clear();
        }
        if let Some(ix) = self.docs.iter().position(|d| d.editor.read(cx).path.as_deref() == Some(path)) {
            self.active = ix;
            window.focus(&self.docs[ix].editor.focus_handle(cx));
            self.after_switch(window, cx);
            return;
        }
        let text = match std::fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).replace("\r\n", "\n"),
            Err(e) => {
                self.show_toast(format!("Could not open {}: {e}", path.display()), cx);
                return;
            }
        };
        self.settings.push_recent(path.to_path_buf());
        self.settings.save();
        let replace = !self.docs.is_empty() && in_place && {
            let e = self.docs[self.active].editor.read(cx);
            self.is_blank(self.active, cx) || (!e.buffer.is_dirty() && e.path.is_some())
        };
        if replace {
            self.flush_save(self.active, cx);
            let config = self.editor_config();
            let editor = cx.new(|cx| Editor::new(&text, Some(path.to_path_buf()), config, cx));
            let sub = cx.subscribe_in(&editor, window, Self::on_editor_event);
            self.docs[self.active] = Doc { editor: editor.clone(), save_task: None, mtime: mtime_of(path), _sub: sub };
            window.focus(&editor.focus_handle(cx));
            self.after_switch(window, cx);
        } else {
            self.add_doc(&text, Some(path.to_path_buf()), window, cx);
        }
    }

    fn on_editor_event(&mut self, editor: &Entity<Editor>, ev: &EditorEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev {
            EditorEvent::Edited => {
                let Some(ix) = self.docs.iter().position(|d| &d.editor == editor) else { return };
                let weak = editor.downgrade();
                self.docs[ix].save_task = Some(cx.spawn(async move |ws, cx| {
                    cx.background_executor().timer(Duration::from_millis(1200)).await;
                    let _ = ws.update(cx, |ws, cx| {
                        if let Some(editor) = weak.upgrade() {
                            ws.autosave(&editor, cx);
                        }
                    });
                }));
                self.update_title(window, cx);
                self.schedule_preview(cx);
                self.schedule_spell(Duration::from_millis(700), cx);
                self.schedule_studio(Duration::from_millis(900), cx);
                cx.notify();
            }
            EditorEvent::ContextMenu(pos) => {
                let mut items = self.studio_context_items(cx);
                items.extend(self.context_menu_items());
                self.open_menu(items, *pos, window, cx);
            }
        }
    }

    fn sanitize(name: &str) -> String {
        let s: String = name
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '-' } else { c })
            .collect();
        let s = s.trim().trim_matches('.').to_string();
        if s.is_empty() { "Untitled".into() } else { s.chars().take(80).collect() }
    }

    /// iA-style autosave: documents always live on disk. New documents are created
    /// in the current library folder, named after their first line.
    fn autosave(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let (text, path, dirty) = {
            let e = editor.read(cx);
            (e.text().to_string(), e.path.clone(), e.buffer.is_dirty())
        };
        if !dirty {
            return;
        }
        let path = match path {
            Some(p) => p,
            None => {
                if text.trim().is_empty() {
                    return;
                }
                let name = Self::sanitize(&crate::markdown::title(&text));
                let p = library::unique_path(&self.lib_dir, &name, "md");
                editor.update(cx, |e, _| e.path = Some(p.clone()));
                self.settings.push_recent(p.clone());
                self.settings.save();
                p
            }
        };
        match std::fs::write(&path, &text) {
            Ok(()) => {
                if let Some(d) = self.docs.iter_mut().find(|d| &d.editor == editor) {
                    d.mtime = mtime_of(&path);
                }
                editor.update(cx, |e, cx| {
                    e.buffer.mark_saved();
                    e.authors.save(&path);
                    cx.notify();
                });
                self.refresh_library();
            }
            Err(err) => self.show_toast(format!("Could not save: {err}"), cx),
        }
        cx.notify();
    }

    fn flush_save(&mut self, ix: usize, cx: &mut Context<Self>) {
        let editor = self.docs[ix].editor.clone();
        self.docs[ix].save_task = None;
        self.autosave(&editor, cx);
    }

    fn save_all(&mut self, cx: &mut Context<Self>) {
        for i in 0..self.docs.len() {
            self.flush_save(i, cx);
        }
    }

    fn update_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let e = self.editor().read(cx);
        let title = e.title();
        let dirty = e.buffer.is_dirty();
        window.set_window_title(&format!("{title} — Quill"));
        window.set_window_edited(dirty);
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_save(ix, cx);
        self.docs.remove(ix);
        if self.docs.is_empty() {
            self.new_doc(window, cx);
            return;
        }
        self.active = self.active.min(self.docs.len() - 1);
        if ix < self.active {
            self.active -= 1;
        }
        let fh = self.editor().focus_handle(cx);
        window.focus(&fh);
        self.after_switch(window, cx);
    }

    // ---- library ------------------------------------------------------------

    fn refresh_library(&mut self) {
        self.entries = match (&self.smart, &self.tag) {
            (Some(q), _) => library::matching(&self.lib_root, q, self.settings.sort),
            (_, Some(t)) => library::tagged(&self.lib_root, t, self.settings.sort),
            _ => library::list(&self.lib_dir, self.settings.sort).unwrap_or_default(),
        };
        self.tags = if self.settings.show_library { library::tags(&self.lib_root) } else { Vec::new() };
    }

    fn enter_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let was_plain = self.tag.is_none() && self.smart.is_none();
        self.tag = None;
        self.smart = None;
        if was_plain && dir != self.lib_dir {
            self.dir_back.push(self.lib_dir.clone());
            self.dir_fwd.clear();
        }
        self.lib_dir = dir;
        self.refresh_library();
        cx.notify();
    }

    // ---- preview ------------------------------------------------------------

    fn schedule_preview(&mut self, cx: &mut Context<Self>) {
        if !self.settings.preview {
            return;
        }
        self.preview_task = Some(cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let _ = ws.update(cx, |ws, cx| ws.refresh_preview(cx));
        }));
    }

    fn schedule_spell(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if !self.settings.spell || self.docs.is_empty() {
            return;
        }
        let editor = self.editor().clone();
        let lang = self.settings.spell_lang.clone();
        self.spell_task = Some(cx.spawn(async move |_, cx| {
            cx.background_executor().timer(delay).await;
            let Ok(words) = cx.update(|cx| crate::spell::candidates(editor.read(cx).text())) else { return };
            let result = cx.background_executor().spawn(async move { crate::spell::misspelled(&words, &lang) }).await;
            if let Some(bad) = result {
                let _ = cx.update(|cx| editor.update(cx, |e, cx| e.set_misspelled(bad, cx)));
            }
        }));
    }

    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        if !self.settings.preview || self.docs.is_empty() {
            return;
        }
        let e = self.editor().read(cx);
        let text = e.text().to_string();
        let base = e.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf));
        let template = self.settings.template;
        self.preview.update(cx, |p, cx| p.update(&text, base, template, cx));
    }

    // ---- misc ---------------------------------------------------------------

    fn show_toast(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some((msg.into(), Instant::now()));
        cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(Duration::from_millis(3000)).await;
            let _ = ws.update(cx, |ws, cx| {
                if ws.toast.as_ref().is_some_and(|(_, t)| t.elapsed() >= Duration::from_millis(2900)) {
                    ws.toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn current_stats(&mut self, cx: &App) -> &Stats {
        let e = self.editor().read(cx);
        let v = e.buffer.version();
        let id = self.active;
        if self.stats.0 != v || self.stats.1 != id {
            let sel = e.buffer.selection();
            let text = if sel.is_empty() { e.text() } else { &e.text()[sel] };
            self.stats = (v, id, stats::compute(text));
        }
        &self.stats.2
    }

    fn chrome_visible(&self, mode: BarMode, cx: &App) -> bool {
        match mode {
            BarMode::AlwaysShow => true,
            BarMode::Hide => false,
            BarMode::FadeInOut => {
                let typed = self.editor().read(cx).last_input;
                !(typed > self.last_mouse_move && typed.elapsed() < Duration::from_secs(600))
                    || !matches!(self.overlay, Overlay::None)
            }
        }
    }

    fn export_to(&mut self, format: Format, window: &mut Window, cx: &mut Context<Self>) {
        let e = self.editor().read(cx);
        let text = e.text().to_string();
        let title = e.title();
        let dir = e
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.lib_dir.clone());
        let name = format!("{}.{}", Self::sanitize(&title), format.extension());
        let rx = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |ws, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let result = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { export::export(&text, &title, format, &path) }
                })
                .await;
            let _ = ws.update(cx, |ws, cx| match result {
                Ok(()) => ws.show_toast(format!("Exported {}", path.file_name().unwrap_or_default().to_string_lossy()), cx),
                Err(err) => ws.show_toast(format!("Export failed: {err}"), cx),
            });
        })
        .detach();
    }

    fn print(&mut self, plain: bool, cx: &mut Context<Self>) {
        let e = self.editor().read(cx);
        let title = e.title();
        let text = if plain {
            format!("```\n{}\n```", e.text())
        } else {
            e.text().to_string()
        };
        let out = std::env::temp_dir().join(format!("{}.pdf", Self::sanitize(&title)));
        cx.spawn(async move |ws, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let out = out.clone();
                    async move {
                        export::pdf(&text, &title, &out)?;
                        // Send to the default printer if CUPS is there, else open the PDF.
                        let lp = std::process::Command::new("lp").arg(&out).status();
                        if !lp.is_ok_and(|s| s.success()) {
                            std::process::Command::new("xdg-open").arg(&out).spawn()?;
                        }
                        anyhow::Ok(())
                    }
                })
                .await;
            let _ = ws.update(cx, |ws, cx| {
                if let Err(err) = result {
                    ws.show_toast(format!("Print failed: {err}"), cx);
                }
            });
        })
        .detach();
    }

    fn speak(&mut self, cx: &mut Context<Self>) {
        self.stop_speaking();
        let e = self.editor().read(cx);
        let sel = e.buffer.selection();
        let text = export::plain_text(if sel.is_empty() { e.text() } else { &e.text()[sel] });
        for cmd in ["spd-say", "espeak-ng", "espeak"] {
            let mut c = std::process::Command::new(cmd);
            if cmd == "spd-say" {
                c.arg("-w");
            }
            if let Ok(child) = c.arg(&text).spawn() {
                self.speech = Some(child);
                return;
            }
        }
        self.show_toast("Speech needs spd-say or espeak installed", cx);
    }

    fn stop_speaking(&mut self) {
        if let Some(mut c) = self.speech.take() {
            let _ = c.kill();
        }
        let _ = std::process::Command::new("spd-say").arg("-S").status();
    }

    // ---- find ---------------------------------------------------------------

    fn open_find(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            let query = cx.new(|cx| TextField::new("Search", cx));
            let rep = cx.new(|cx| TextField::new("Replace", cx));
            let s1 = cx.subscribe_in(&query, window, |ws: &mut Self, _, ev: &FieldEvent, window, cx| match ev {
                FieldEvent::Changed(q) => {
                    let q = q.clone();
                    ws.editor().update(cx, |e, cx| e.set_find_query(&q, cx));
                }
                FieldEvent::Confirm => {
                    let shift = window.modifiers().shift;
                    ws.editor().update(cx, |e, cx| e.find_step(!shift, cx));
                }
                _ => {}
            });
            let s2 = cx.subscribe(&rep, |ws: &mut Self, f, ev: &FieldEvent, cx| {
                if let FieldEvent::Confirm = ev {
                    let r = f.read(cx).text();
                    ws.editor().update(cx, |e, cx| e.replace_current(&r, cx));
                }
            });
            self.find = Some(FindBar { query, replace: rep, show_replace: replace, _subs: [s1, s2] });
        }
        let sel = self.editor().read(cx).buffer.selected_text().to_string();
        let f = self.find.as_mut().unwrap();
        f.show_replace |= replace;
        if !sel.is_empty() && !sel.contains('\n') {
            f.query.update(cx, |q, cx| q.set_text(&sel, cx));
        }
        let q = f.query.read(cx).text();
        let query = f.query.clone();
        self.editor().update(cx, |e, cx| e.set_find_query(&q, cx));
        window.focus(&query.focus_handle(cx));
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        self.editor().update(cx, |e, cx| e.close_find(cx));
        let fh = self.editor().focus_handle(cx);
        window.focus(&fh);
        cx.notify();
    }

    // ---- quick search -------------------------------------------------------

    fn open_quick_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| TextField::new("Search Library", cx));
        let sub = cx.subscribe_in(&field, window, |ws: &mut Self, _, ev: &FieldEvent, window, cx| {
            let Overlay::QuickSearch(qs) = &mut ws.overlay else { return };
            match ev {
                FieldEvent::Changed(q) => {
                    qs.results = library::search(&ws.lib_root, q, 12);
                    qs.selected = 0;
                }
                FieldEvent::Up => qs.selected = qs.selected.saturating_sub(1),
                FieldEvent::Down => qs.selected = (qs.selected + 1).min(qs.results.len().saturating_sub(1)),
                FieldEvent::Confirm => {
                    if let Some(hit) = qs.results.get(qs.selected).cloned() {
                        ws.overlay = Overlay::None;
                        ws.open_path(&hit.path, true, window, cx);
                    }
                }
            }
            cx.notify();
        });
        let results = library::search(&self.lib_root, "", 12);
        window.focus(&field.focus_handle(cx));
        self.overlay = Overlay::QuickSearch(QuickSearchState { field, results, selected: 0, _sub: sub });
        cx.notify();
    }

    fn open_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            let mut f = TextField::new("e.g. synergy, leverage", cx);
            f.buffer.reset(self.settings.custom_patterns.join(", "));
            f
        });
        let sub = cx.subscribe(&field, |ws: &mut Self, _, ev: &FieldEvent, cx| {
            if let FieldEvent::Changed(t) = ev {
                let list = t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                ws.update_settings(cx, |s| s.custom_patterns = list);
            }
        });
        let _ = window;
        self.overlay = Overlay::Preferences(field, sub);
        cx.notify();
    }

    fn open_accounts(&mut self, service: Service, window: &mut Window, cx: &mut Context<Self>) {
        let existing = publish::load_accounts().into_iter().find(|a| a.service == service);
        let hints = service.fields();
        let values = existing.map(|a| [a.url, a.user, a.token]).unwrap_or_default();
        let fields = [0, 1, 2].map(|i| {
            let v = values[i].clone();
            cx.new(|cx| {
                let mut f = TextField::new(hints[i], cx);
                f.buffer.reset(v);
                f
            })
        });
        window.focus(&fields[0].focus_handle(cx));
        self.overlay = Overlay::Accounts(service, fields);
        cx.notify();
    }

    fn save_account(&mut self, remove: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Accounts(service, fields) = &self.overlay else { return };
        let service = *service;
        let [url, user, token] = [0, 1, 2].map(|i| fields[i].read(cx).text().trim().to_string());
        let mut all = publish::load_accounts();
        all.retain(|a| a.service != service);
        if !remove {
            if url.is_empty() || token.is_empty() {
                self.show_toast("Site URL and token are required", cx);
                return;
            }
            all.push(Account { service, url, user, token });
        }
        match publish::save_accounts(&all) {
            Ok(()) => self.show_toast(if remove { format!("{} account removed", service.label()) } else { format!("{} account saved", service.label()) }, cx),
            Err(e) => self.show_toast(format!("Could not save account: {e}"), cx),
        }
        self.dismiss(window, cx);
    }

    fn publish_to(&mut self, service: Service, cx: &mut Context<Self>) {
        let Some(account) = publish::load_accounts().into_iter().find(|a| a.service == service) else { return };
        let post = publish::post_from_markdown(self.editor().read(cx).text());
        self.show_toast(format!("Publishing to {}…", service.label()), cx);
        cx.spawn(async move |ws, cx| {
            let result = cx.background_executor().spawn(async move { publish::publish(&account, &post) }).await;
            let _ = ws.update(cx, |ws, cx| match result {
                Ok(url) if !url.is_empty() => ws.show_toast(format!("Draft created: {url}"), cx),
                Ok(_) => ws.show_toast(format!("Draft created on {}", service.label()), cx),
                Err(e) => ws.show_toast(format!("Publishing failed: {e}"), cx),
            });
        })
        .detach();
    }

    fn publish_menu(&self, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let accounts = publish::load_accounts();
        let mut items: Vec<MenuItem> = Service::ALL
            .iter()
            .map(|&svc| {
                let weak = cx.entity().downgrade();
                if accounts.iter().any(|a| a.service == svc) {
                    call(&format!("Publish to {}", svc.label()), move |_, cx| {
                        let _ = weak.update(cx, |ws, cx| ws.publish_to(svc, cx));
                    })
                } else {
                    call(&format!("Add {} Account…", svc.label()), move |window, cx| {
                        let _ = weak.update(cx, |ws, cx| ws.open_accounts(svc, window, cx));
                    })
                }
            })
            .collect();
        items.push(sep());
        items.push(act("Manage Accounts…", ManageAccounts));
        items
    }

    fn show_completions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor().clone();
        let e = editor.read(cx);
        let (range, words) = format::completions(e.text(), e.buffer.head(), 8);
        let Some(anchor) = e.caret_anchor() else { return };
        if words.is_empty() {
            self.show_toast("No completions", cx);
            return;
        }
        let items = words
            .into_iter()
            .map(|w| {
                let (editor, range) = (editor.clone(), range.clone());
                call(&w.clone(), move |_, cx| editor.update(cx, |e, cx| e.replace_range(range.clone(), &w, cx)))
            })
            .collect();
        self.open_menu(items, anchor, window, cx);
        if let Overlay::Menu(m) = &mut self.overlay {
            m.open = vec![0];
        }
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.overlay, Overlay::None) {
            self.overlay = Overlay::None;
            let fh = self.editor().focus_handle(cx);
            window.focus(&fh);
            cx.notify();
        } else if self.find.is_some() {
            self.close_find(window, cx);
        }
    }

    fn open_menu(&mut self, items: Vec<MenuItem>, origin: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Menu(MenuOverlay { items, origin, open: Vec::new() });
        // Menus take keyboard focus so arrow keys navigate them, not the editor.
        window.focus(&self.menu_focus);
        cx.notify();
    }

    fn menu_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Overlay::Menu(m) = &mut self.overlay {
            m.open = menus::navigate(&m.items, &m.open, key);
            cx.notify();
        }
    }

    fn menu_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Overlay::Menu(m) = &mut self.overlay else { return };
        if let Some(MenuItem::Item { action, callback, enabled: true, .. }) = menus::item_at(&m.items, &m.open) {
            let action = action.as_ref().map(|a| a.boxed_clone());
            let callback = callback.clone();
            self.overlay = Overlay::None;
            let fh = self.editor().focus_handle(cx);
            window.focus(&fh);
            if let Some(a) = action {
                window.dispatch_action(a, cx);
            }
            if let Some(f) = callback {
                // Callbacks may update the workspace, which is mid-update here.
                window.defer(cx, move |window, cx| f(window, cx));
            }
        } else {
            m.open = menus::navigate(&m.items, &m.open, "enter");
        }
        cx.notify();
    }
}

// ---- menus ------------------------------------------------------------------------

impl Workspace {
    fn main_menu(&self, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let s = &self.settings;
        let weak = cx.entity().downgrade();
        let recent: Vec<MenuItem> = s
            .recent
            .iter()
            .take(10)
            .map(|p| {
                let p2 = p.clone();
                let weak = weak.clone();
                call(&library::file_stem(p), move |window, cx| {
                    let _ = weak.update(cx, |ws, cx| ws.open_path(&p2, true, window, cx));
                })
            })
            .chain([sep(), act("Clear Menu", OpenRecentClear)])
            .collect();
        let has_ai = !self.editor().read(cx).authors.ai.is_empty();
        let publish_items = self.publish_menu(cx);
        let share_items = self.share_menu(cx);
        vec![
            sub("Quill", vec![
                act("About Quill", About),
                sep(),
                act("Preferences…", Preferences),
                sep(),
                act("Quit Quill", Quit),
            ]),
            sub("File", vec![
                act("New in Library", NewDocument),
                act("New Tab", NewTab),
                sep(),
                act("Open…", Open),
                sub("Open Recent", recent),
                sep(),
                act("Close", Close),
                sep(),
                act("Save", Save),
                act("Save As…", SaveAs),
                act("Revert to Saved", Revert),
                sep(),
                act("Show in Files", ShowInFolder),
                act("Show in Library", ShowInLibrary),
                sep(),
                sub("Share", share_items),
                sub("Export", vec![
                    act("PDF…", ExportPdf),
                    act("HTML…", ExportHtml),
                    act("Microsoft Word…", ExportDocx),
                    act("Plain Text…", ExportText),
                ]),
                sub("Publish", publish_items),
                sub("Print", vec![act("Print…", Print), act("Print Plain Text…", PrintPlainText)]),
            ]),
            sub("Edit", vec![
                act("Undo", Undo),
                act("Redo", Redo),
                sep(),
                act("Cut", Cut),
                act("Copy", Copy),
                act("Copy Formatted", CopyFormatted),
                act("Copy HTML", CopyHtml),
                act("Copy Markdown", CopyMarkdown),
                act("Paste", Paste),
                sub("Paste As", vec![act("Plain Text", PastePlain), act("Human", PasteAsHuman), act("AI", PasteAsAi)]),
                sub("Paste Edits From", vec![act("Human", PasteEditsFromHuman), act("AI", PasteEditsFromAi)]),
                sub("Mark As", vec![act("Human", MarkAsHuman), act("AI", MarkAsAi)]),
                act("Select All", SelectAll),
                sep(),
                sub("Find", vec![
                    act("Find…", Find),
                    act("Find Next", FindNext),
                    act("Find Previous", FindPrevious),
                    act("Use Selection for Find", UseSelectionForFind),
                    sep(),
                    act("Find and Replace…", FindReplace),
                    act("Replace", ReplaceOne),
                    act("Replace All", ReplaceAll),
                    sep(),
                    act("Filter Library…", FilterLibrary),
                ]),
                sub("Spelling and Grammar", vec![
                    act("Check Document Now", CheckSpelling),
                    sep(),
                    check("Check Spelling While Typing", ToggleSpelling, s.spell),
                ]),
                sub("Substitutions", vec![
                    check("Smart Quotes", ToggleSmartQuotes, s.smart_quotes),
                    check("Smart Dashes", ToggleSmartDashes, s.smart_dashes),
                    check("Auto-Pair Brackets", ToggleAutoPair, s.auto_pair),
                ]),
                sub("Transformations", vec![
                    act("Make Upper Case", UpperCase),
                    act("Make Lower Case", LowerCase),
                    act("Capitalize", Capitalize),
                    act("Make Title Case", TitleCase),
                ]),
                sub("Speech", vec![act("Start Speaking", StartSpeaking), act("Stop Speaking", StopSpeaking)]),
            ]),
            sub("Format", vec![
                sub("Heading", vec![
                    act("Heading 1", Heading1),
                    act("Heading 2", Heading2),
                    act("Heading 3", Heading3),
                    act("Heading 4", Heading4),
                    act("Heading 5", Heading5),
                    act("Heading 6", Heading6),
                ]),
                sub("List", vec![
                    act("List", List),
                    act("Task List", TaskList),
                    act("Ordered List", OrderedList),
                    act("Ordered Task List", OrderedTaskList),
                    sep(),
                    act("Mark Task as Completed", MarkTaskCompleted),
                ]),
                act("Blockquote", Blockquote),
                sub("Alert", vec![
                    act("Note", AlertNote),
                    act("Tip", AlertTip),
                    act("Important", AlertImportant),
                    act("Warning", AlertWarning),
                    act("Caution", AlertCaution),
                ]),
                act("Comment", Comment),
                act("Body", Body),
                sep(),
                act("Bold", Bold),
                act("Italic", Italic),
                act("Strikethrough", Strikethrough),
                act("Highlight", Highlight),
                sep(),
                act("Code", Code),
                act("Code Block", CodeBlock),
                sep(),
                act("Math", Math),
                act("Display Math", DisplayMath),
                sep(),
                act("Add Table", AddTable),
                sub("Edit Table", vec![
                    act("Sort Column Ascending", TableSortAscending),
                    act("Sort Column Descending", TableSortDescending),
                    sep(),
                    act("Add Row Above", TableRowAbove),
                    act("Add Row Below", TableRowBelow),
                    sep(),
                    act("Add Column Before", TableColumnBefore),
                    act("Add Column After", TableColumnAfter),
                    sep(),
                    act("Align Column Left", TableAlignLeft),
                    act("Align Column Center", TableAlignCenter),
                    act("Align Column Right", TableAlignRight),
                    sep(),
                    act("Delete Column", TableDeleteColumn),
                    act("Delete Row", TableDeleteRow),
                    sep(),
                    act("Move Column Left", TableMoveColumnLeft),
                    act("Move Column Right", TableMoveColumnRight),
                    act("Move Row Up", TableMoveRowUp),
                    act("Move Row Down", TableMoveRowDown),
                    sep(),
                    act("Merge Cells", TableMerge),
                    act("Unmerge Cells", TableUnmerge),
                    sep(),
                    act("Reformat Table", TableReformat),
                    act("Transpose Rows and Columns", TableTranspose),
                ]),
                sep(),
                act("Add Link", AddLink),
                act("Add Image Link", AddImageLink),
                act("Add Wikilink", AddWikilink),
                act("Add Footnote", AddFootnote),
                act("Add Content Block", AddContentBlock),
                act("Add Hashtag", AddHashtag),
                sep(),
                act("Add Date", AddDate),
                act("Add Table of Contents", AddTableOfContents),
                sep(),
                act("Add Horizontal Rule", AddHorizontalRule),
                act("Add Page Break", AddPageBreak),
                sep(),
                act("Clear Styles", ClearStyles),
            ]),
            sub("View", vec![
                sub("Appearance", vec![
                    check("System", AppearanceSystem, s.appearance == Appearance::System),
                    check("Light", AppearanceLight, s.appearance == Appearance::Light),
                    check("Dark", AppearanceDark, s.appearance == Appearance::Dark),
                ]),
                act("Dark Appearance", ToggleDarkAppearance),
                sep(),
                check("Show Library", ToggleLibrary, s.show_library),
                check("Show Organizer", ToggleOrganizer, s.show_organizer),
                sep(),
                check("Show Sort Bar", ToggleSortBar, s.show_sort_bar),
                check("Show Filter Bar", ToggleFilterBar, self.show_filter),
                sub("Sort Files By", vec![
                    check("Name", SortByName, s.sort == SortBy::Name),
                    check("Date Modified", SortByDate, s.sort == SortBy::Modified),
                ]),
                sep(),
                sub("Font", vec![
                    check("Mono", FontMono, s.font == FontChoice::Mono),
                    check("Duo", FontDuo, s.font == FontChoice::Duo),
                    check("Quattro", FontQuattro, s.font == FontChoice::Quattro),
                ]),
                sub("Text Size", vec![
                    act("Make Text Bigger", TextBigger),
                    act("Make Text Normal Size", TextNormal),
                    act("Make Text Smaller", TextSmaller),
                ]),
                sep(),
                check("Show Preview", TogglePreview, s.preview),
                act("Reload Preview", ReloadPreview),
                sub("Preview", vec![
                    check("Full", PreviewFull, s.preview_layout == PreviewLayout::Full),
                    check("Split", PreviewSplit, s.preview_layout == PreviewLayout::Split),
                ]),
                sub("Template", {
                    let weak = cx.entity().downgrade();
                    Template::ALL
                        .iter()
                        .map(|&t| {
                            let weak = weak.clone();
                            let mut item = call(t.label(), move |_, cx| {
                                let _ = weak.update(cx, |ws, cx| ws.update_settings(cx, |s| s.template = t));
                            });
                            if let MenuItem::Item { checked, .. } = &mut item {
                                *checked = s.template == t;
                            }
                            item
                        })
                        .collect()
                }),
                sep(),
                sub("Title Bar", vec![
                    check("Fade In/Out", TitleBarFade, s.title_bar == BarMode::FadeInOut),
                    check("Always Show", TitleBarAlways, s.title_bar == BarMode::AlwaysShow),
                ]),
                sub("Toolbar", vec![
                    check("Fade In/Out", ToolbarFade, s.toolbar == BarMode::FadeInOut),
                    check("Always Show", ToolbarAlways, s.toolbar == BarMode::AlwaysShow),
                    check("Hide", ToolbarHide, s.toolbar == BarMode::Hide),
                    sep(),
                    check("Characters", StatCharacters, s.stat == StatKind::Characters),
                    check("Characters Without Spaces", StatCharactersNoSpaces, s.stat == StatKind::CharactersWithoutSpaces),
                    check("Words", StatWords, s.stat == StatKind::Words),
                    check("Sentences", StatSentences, s.stat == StatKind::Sentences),
                    check("Reading Time", StatReadingTime, s.stat == StatKind::ReadingTime),
                    check("Speaking Time", StatSpeakingTime, s.stat == StatKind::SpeakingTime),
                    check("Tasks", StatTasks, s.stat == StatKind::Tasks),
                ]),
                sep(),
                act("Show Completions", ShowCompletions),
                sep(),
                check("Show Tab Bar", ToggleTabBar, s.always_show_tabs),
                act("Show All Tabs", ShowAllTabs),
                sep(),
                act("Enter Full Screen", ToggleFullScreen),
            ]),
            sub("Focus", vec![
                check("Enable Focus Mode", ToggleFocus, s.focus != FocusMode::Off),
                check("Sentence", FocusSentence, s.focus_kind == FocusMode::Sentence),
                check("Paragraph", FocusParagraph, s.focus_kind == FocusMode::Paragraph),
                check("Typewriter", ToggleTypewriter, s.typewriter),
                sep(),
                check("Show Syntax", ToggleSyntax, s.syntax),
                check("Adjectives", SyntaxAdjectives, s.syntax_adjectives),
                check("Nouns", SyntaxNouns, s.syntax_nouns),
                check("Adverbs", SyntaxAdverbs, s.syntax_adverbs),
                check("Verbs", SyntaxVerbs, s.syntax_verbs),
                check("Conjunctions", SyntaxConjunctions, s.syntax_conjunctions),
                sep(),
                check("Enable Style Check", ToggleStyleCheck, s.style_check),
                check("Fillers", CheckFillers, s.check_fillers),
                check("Clichés", CheckCliches, s.check_cliches),
                check("Redundancies", CheckRedundancies, s.check_redundancies),
                check("Custom", CheckCustom, s.check_custom),
                sep(),
                check("Show Authors", ToggleAuthors, s.show_authors),
                if has_ai { act("Mark Selection as Human", MarkAsHuman) } else { menus::disabled("No AI text in document") },
            ]),
            self.studio_menu(cx),
            sub("Go", vec![act("Quick Search…", QuickSearch), act("Next Tab", NextTab), act("Previous Tab", PreviousTab)]),
            sub("Window", vec![act("Minimize", Minimize), act("Zoom", Zoom), act("Enter Full Screen", ToggleFullScreen)]),
            sub("Help", vec![act("Quill Help", Help), act("Markdown Guide", MarkdownGuide), act("What's New in Quill", WhatsNew)]),
        ]
    }

    fn context_menu_items(&self) -> Vec<MenuItem> {
        vec![
            act("Cut", Cut),
            act("Copy", Copy),
            act("Paste", Paste),
            sep(),
            sub("Paste As", vec![act("Plain Text", PastePlain), act("Human", PasteAsHuman), act("AI", PasteAsAi)]),
            sub("Mark As", vec![act("Human", MarkAsHuman), act("AI", MarkAsAi)]),
            sep(),
            sub("Format", vec![
                act("Bold", Bold),
                act("Italic", Italic),
                act("Strikethrough", Strikethrough),
                act("Highlight", Highlight),
                act("Code", Code),
                sep(),
                act("Add Link", AddLink),
                act("Add Footnote", AddFootnote),
                act("Clear Styles", ClearStyles),
            ]),
            sub("Transformations", vec![
                act("Make Upper Case", UpperCase),
                act("Make Lower Case", LowerCase),
                act("Capitalize", Capitalize),
                act("Make Title Case", TitleCase),
            ]),
            sep(),
            act("Start Speaking", StartSpeaking),
        ]
    }

    fn share_menu(&self, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let publish_items = self.publish_menu(cx);
        vec![
            act("Export PDF…", ExportPdf),
            act("Export HTML…", ExportHtml),
            act("Export Microsoft Word…", ExportDocx),
            act("Export Plain Text…", ExportText),
            sep(),
            sub("Publish", publish_items),
            act("Print…", Print),
            sep(),
            act("Copy HTML", CopyHtml),
            act("Copy Formatted", CopyFormatted),
            sep(),
            act("Show in Files", ShowInFolder),
        ]
    }

    /// Laid out like the original's Focus menu: dashes mark the chosen but
    /// inactive mode, syntax items are shown in their highlight colors.
    fn focus_menu(&self, cx: &App) -> Vec<MenuItem> {
        let s = &self.settings;
        let t = cx.global::<Theme>();
        let focus_on = s.focus != FocusMode::Off;
        let kind = if focus_on { s.focus } else { s.focus_kind };
        vec![
            check(if focus_on { "Disable Focus Mode" } else { "Enable Focus Mode" }, ToggleFocus, false),
            check("Sentence", FocusSentence, focus_on && kind == FocusMode::Sentence).dashed(!focus_on && kind == FocusMode::Sentence),
            check("Paragraph", FocusParagraph, focus_on && kind == FocusMode::Paragraph).dashed(!focus_on && kind == FocusMode::Paragraph),
            check("Typewriter", ToggleTypewriter, s.typewriter),
            sep(),
            check(if s.syntax { "Hide Syntax" } else { "Show Syntax" }, ToggleSyntax, false),
            check("Adjectives", SyntaxAdjectives, s.syntax && s.syntax_adjectives).dashed(!s.syntax && s.syntax_adjectives).colored(t.adjective),
            check("Nouns", SyntaxNouns, s.syntax && s.syntax_nouns).dashed(!s.syntax && s.syntax_nouns).colored(t.noun),
            check("Adverbs", SyntaxAdverbs, s.syntax && s.syntax_adverbs).dashed(!s.syntax && s.syntax_adverbs).colored(t.adverb),
            check("Verbs", SyntaxVerbs, s.syntax && s.syntax_verbs).dashed(!s.syntax && s.syntax_verbs).colored(t.verb),
            check("Conjunctions", SyntaxConjunctions, s.syntax && s.syntax_conjunctions).dashed(!s.syntax && s.syntax_conjunctions).colored(t.conjunction),
            sep(),
            check(if s.style_check { "Disable Style Check" } else { "Enable Style Check" }, ToggleStyleCheck, false),
            check("Fillers", CheckFillers, s.style_check && s.check_fillers).dashed(!s.style_check && s.check_fillers),
            check("Clichés", CheckCliches, s.style_check && s.check_cliches).dashed(!s.style_check && s.check_cliches),
            check("Redundancies", CheckRedundancies, s.style_check && s.check_redundancies).dashed(!s.style_check && s.check_redundancies),
            check("Custom", CheckCustom, s.style_check && s.check_custom).dashed(!s.style_check && s.check_custom),
            sep(),
            check(if s.show_authors { "Hide Authors" } else { "Show Authors" }, ToggleAuthors, false),
        ]
    }

    fn stat_menu(&self) -> Vec<MenuItem> {
        let s = &self.settings;
        StatKind::ALL
            .iter()
            .map(|&k| {
                let a: Box<dyn gpui::Action> = match k {
                    StatKind::Characters => Box::new(StatCharacters),
                    StatKind::CharactersWithoutSpaces => Box::new(StatCharactersNoSpaces),
                    StatKind::Words => Box::new(StatWords),
                    StatKind::Sentences => Box::new(StatSentences),
                    StatKind::ReadingTime => Box::new(StatReadingTime),
                    StatKind::SpeakingTime => Box::new(StatSpeakingTime),
                    StatKind::Tasks => Box::new(StatTasks),
                };
                MenuItem::Item { label: k.label().into(), action: Some(a), callback: None, checked: s.stat == k, enabled: true, color: None, dash: false }
            })
            .collect()
    }

    fn library_item_menu(&self, path: PathBuf, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let (p1, p2, path_fav, p3) = (path.clone(), path.clone(), path.clone(), path);
        let (w1, w3) = (cx.entity().downgrade(), cx.entity().downgrade());
        vec![
            call("Open in New Tab", move |window, cx| {
                let _ = w1.update(cx, |ws, cx| ws.open_path(&p1, false, window, cx));
            }),
            call("Show in Files", move |_, cx| cx.reveal_path(&p2)),
            {
                let (p4, w4) = (path_fav.clone(), cx.entity().downgrade());
                call(if self.settings.favorites.contains(&path_fav) { "Remove from Favorites" } else { "Add to Favorites" }, move |_, cx| {
                    let _ = w4.update(cx, |ws, cx| ws.toggle_favorite(p4.clone(), cx));
                })
            },
            sep(),
            call("Move to Trash", move |window, cx| {
                let _ = w3.update(cx, |ws, cx| ws.trash(&p3, window, cx));
            }),
        ]
    }

    fn trash(&mut self, p: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let ok = std::process::Command::new("gio").arg("trash").arg(p).status().is_ok_and(|s| s.success());
        if !ok {
            self.show_toast("Could not move to Trash", cx);
            return;
        }
        if let Some(ix) = self.docs.iter().position(|d| d.editor.read(cx).path.as_deref() == Some(p)) {
            self.docs[ix].save_task = None;
            self.docs[ix].editor.update(cx, |e, _| e.buffer.mark_saved());
            self.close_tab(ix, window, cx);
        }
        self.refresh_library();
        self.show_toast("Moved to Trash", cx);
    }
}

gpui::actions!(quill, [FocusOff, ManageAccounts, OpenMenu, MenuUp, MenuDown, MenuLeft, MenuRight, MenuConfirm]);

// ---- rendering --------------------------------------------------------------------

fn mtime_of(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Cheap fingerprint of a folder: its own mtime, the newest entry and the entry count.
fn library_signature(dir: &Path) -> (Option<SystemTime>, Option<SystemTime>, usize) {
    let entries: Vec<_> = std::fs::read_dir(dir).map(|rd| rd.flatten().collect()).unwrap_or_default();
    let newest = entries.iter().filter_map(|e| e.metadata().ok()?.modified().ok()).max();
    (mtime_of(dir), newest, entries.len())
}

fn icon_button(id: &str, icon: &str, t: &Theme, active: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .size(px(28.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(t.hover))
        .child(
            svg()
                .path(SharedString::from(format!("icons/{icon}.svg")))
                .size(px(18.))
                .text_color(if active { t.accent } else { t.secondary }),
        )
}

fn traffic_lights(window: &Window, t: &Theme) -> impl IntoElement {
    let active = window.is_window_active();
    let light = |id: &'static str, color: u32, glyph: &'static str| {
        let c: Hsla = if active { gpui::rgb(color).into() } else { t.tertiary };
        div()
            .id(id)
            .size(px(12.))
            .rounded_full()
            .bg(c)
            .border_1()
            .border_color(gpui::rgba(0x0000001a))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.))
            .line_height(px(9.))
            .font_weight(FontWeight::BOLD)
            .text_color(gpui::rgba(0x00000000))
            .group_hover("lights", |s| s.text_color(gpui::rgba(0x0000008c)))
            .child(glyph)
    };
    div()
        .group("lights")
        .flex()
        .gap(px(8.))
        .pl(px(8.))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(light("close", 0xff5f57, "×").on_click(|_, window, cx| window.dispatch_action(Box::new(Quit), cx)))
        .child(light("min", 0xfebc2e, "−").on_click(|_, window, _| window.minimize_window()))
        .child(light("zoom", 0x28c840, "+").on_click(|_, window, _| window.zoom_window()))
}

fn resize_edge(pos: Point<Pixels>, inset: Pixels, size: Size<Pixels>) -> Option<ResizeEdge> {
    let (l, r, t, b) = (pos.x < inset, pos.x > size.width - inset, pos.y < inset, pos.y > size.height - inset);
    Some(match (t, b, l, r) {
        (true, _, true, _) => ResizeEdge::TopLeft,
        (true, _, _, true) => ResizeEdge::TopRight,
        (_, true, true, _) => ResizeEdge::BottomLeft,
        (_, true, _, true) => ResizeEdge::BottomRight,
        (true, ..) => ResizeEdge::Top,
        (_, true, ..) => ResizeEdge::Bottom,
        (_, _, true, _) => ResizeEdge::Left,
        (_, _, _, true) => ResizeEdge::Right,
        _ => return None,
    })
}

impl Workspace {
    fn render_tabs(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *cx.global::<Theme>();
        let mut row = div()
            .h(px(30.))
            .flex_none()
            .flex()
            .border_b_1()
            .border_t_1()
            .border_color(t.separator)
            .bg(t.sidebar_bg)
            .font_family(crate::fonts::ui())
            .text_size(px(12.));
        for (i, d) in self.docs.iter().enumerate() {
            let e = d.editor.read(cx);
            let active = i == self.active;
            row = row.child(
                div()
                    .id(SharedString::from(format!("tab-{i}")))
                    .group(SharedString::from(format!("tab-{i}")))
                    .flex_1()
                    .min_w(px(80.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .relative()
                    .when(i > 0, |d| d.border_l_1().border_color(t.separator))
                    .when(active, |d| d.bg(t.bg))
                    .text_color(if active { t.text } else { t.secondary })
                    .cursor(CursorStyle::PointingHand)
                    .on_click(cx.listener(move |ws, _, window, cx| {
                        ws.active = i;
                        let fh = ws.editor().focus_handle(cx);
                        window.focus(&fh);
                        ws.after_switch(window, cx);
                    }))
                    .child(
                        div()
                            .id(SharedString::from(format!("tab-close-{i}")))
                            .absolute()
                            .left(px(8.))
                            .invisible()
                            .group_hover(SharedString::from(format!("tab-{i}")), |s| s.visible())
                            .child(svg().path("icons/close.svg").size(px(11.)).text_color(t.secondary))
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                cx.stop_propagation();
                                ws.close_tab(i, window, cx);
                            })),
                    )
                    .child(div().truncate().px(px(24.)).child(e.title())),
            );
        }
        row
    }

    fn render_find(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = *cx.global::<Theme>();
        let f = self.find.as_ref()?;
        let (count, current) = self
            .editor()
            .read(cx)
            .find
            .as_ref()
            .map(|s| (s.matches.len(), s.current))
            .unwrap_or((0, None));
        let label = match (count, current) {
            (0, _) => "No results".to_string(),
            (n, Some(c)) => format!("{} of {n}", c + 1),
            (n, None) => format!("{n} found"),
        };
        let field = |child: Entity<TextField>| {
            div()
                .flex_1()
                .px(px(8.))
                .py(px(3.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.separator)
                .bg(t.bg)
                .text_size(px(13.))
                .text_color(t.text)
                .child(child)
        };
        let small_button = |id: &str, label: &str| {
            div()
                .id(SharedString::from(id.to_string()))
                .px(px(10.))
                .py(px(3.))
                .rounded(px(6.))
                .border_1()
                .border_color(t.separator)
                .bg(t.bg)
                .text_size(px(12.))
                .text_color(t.text)
                .cursor(CursorStyle::PointingHand)
                .hover(|s| s.bg(t.hover))
                .child(label.to_string())
        };
        let show_replace = f.show_replace;
        Some(
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(6.))
                .px(px(14.))
                .py(px(8.))
                .bg(t.sidebar_bg)
                .border_b_1()
                .border_color(t.separator)
                .font_family(crate::fonts::ui())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(svg().path("icons/search.svg").size(px(14.)).text_color(t.secondary))
                        .child(field(f.query.clone()))
                        .child(div().w(px(80.)).text_size(px(12.)).text_color(t.secondary).child(label))
                        .child(icon_button("find-prev", "chevron-left", &t, false).on_click(cx.listener(|ws, _, _, cx| {
                            ws.editor().update(cx, |e, cx| e.find_step(false, cx))
                        })))
                        .child(icon_button("find-next", "chevron-right", &t, false).on_click(cx.listener(|ws, _, _, cx| {
                            ws.editor().update(cx, |e, cx| e.find_step(true, cx))
                        })))
                        .child(small_button("find-toggle-replace", if show_replace { "Hide Replace" } else { "Replace" }).on_click(cx.listener(
                            |ws, _, _, cx| {
                                if let Some(f) = &mut ws.find {
                                    f.show_replace = !f.show_replace;
                                }
                                cx.notify();
                            },
                        )))
                        .child(small_button("find-done", "Done").on_click(cx.listener(|ws, _, window, cx| ws.close_find(window, cx)))),
                )
                .when(show_replace, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(14.)))
                            .child(field(f.replace.clone()))
                            .child(small_button("replace-one", "Replace").on_click(cx.listener(|ws, _, window, cx| {
                                window.dispatch_action(Box::new(ReplaceOne), cx);
                                let _ = ws;
                            })))
                            .child(small_button("replace-all", "All").on_click(cx.listener(|_, _, window, cx| {
                                window.dispatch_action(Box::new(ReplaceAll), cx)
                            }))),
                    )
                })
                .into_any_element(),
        )
    }

    fn render_status(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *cx.global::<Theme>();
        let kind = self.settings.stat;
        let show_authors = self.settings.show_authors;
        let visible = self.chrome_visible(self.settings.toolbar, cx);
        let text = self.current_stats(cx).format(kind);
        let (human, ai) = {
            let e = self.editor().read(cx);
            e.authors.shares(e.text().len())
        };
        let authors = if show_authors && human + ai > 0 {
            Some(format!("{}% Human · {}% AI", human * 100 / (human + ai), ai * 100 / (human + ai)))
        } else {
            None
        };
        div()
            .absolute()
            .bottom(px(12.))
            .right(px(18.))
            .flex()
            .gap(px(14.))
            .font_family(crate::fonts::ui())
            .text_size(px(13.))
            .text_color(t.secondary)
            .opacity(if visible { 1. } else { 0. })
            .when_some(authors, |d, a| d.child(a))
            .child(
                div()
                    .id("status-stat")
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(t.hover))
                    .child(text)
                    .on_click(cx.listener(|ws, ev: &gpui::ClickEvent, window, cx| {
                        let items = ws.stat_menu();
                        let p = ev.position();
                        let h = items.len() as f32 * menus::ITEM_H + menus::PAD * 2.;
                        ws.open_menu(items, point(p.x - px(menus::WIDTH - 40.), p.y - px(h + 10.)), window, cx);
                    })),
            )
    }

    fn render_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = *cx.global::<Theme>();
        let ws = cx.entity().downgrade();
        let backdrop = |dim: bool| {
            div()
                .id("overlay")
                .absolute()
                .inset_0()
                // Block hover and clicks from reaching the window underneath.
                .occlude()
                .when(dim, |d| d.bg(gpui::rgba(0x00000026)))
                .on_mouse_down(MouseButton::Left, cx.listener(|ws, _, window, cx| ws.dismiss(window, cx)))
                .on_mouse_down(MouseButton::Right, cx.listener(|ws, _, window, cx| ws.dismiss(window, cx)))
        };
        match &self.overlay {
            Overlay::None => None,
            Overlay::Menu(m) => {
                let ws1 = ws.clone();
                let ws2 = ws.clone();
                let on_hover: Rc<dyn Fn(Vec<usize>, &mut Window, &mut App)> = Rc::new(move |path, _, cx| {
                    let _ = ws1.update(cx, |w, cx| {
                        if let Overlay::Menu(m) = &mut w.overlay
                            && m.open != path
                        {
                            m.open = path;
                            cx.notify();
                        }
                    });
                });
                let on_done: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |window, cx| {
                    let _ = ws2.update(cx, |w, cx| {
                        w.overlay = Overlay::None;
                        let fh = w.editor().focus_handle(cx);
                        window.focus(&fh);
                        cx.notify();
                    });
                });
                let size = window.viewport_size();
                let panels = menus::render(&m.items, m.origin, &m.open, size, on_hover, on_done, cx);
                Some(backdrop(false).key_context("Menu").track_focus(&self.menu_focus).children(panels).into_any_element())
            }
            Overlay::QuickSearch(qs) => {
                let results = qs.results.clone();
                let selected = qs.selected;
                let field = qs.field.clone();
                let mut list = div().flex().flex_col().max_h(px(420.)).id("qs-list").overflow_y_scroll();
                for (i, hit) in results.into_iter().enumerate() {
                    let path = hit.path.clone();
                    let rel = hit
                        .path
                        .strip_prefix(&self.lib_root)
                        .map(|p| p.parent().map(|p| p.display().to_string()).unwrap_or_default())
                        .unwrap_or_default();
                    let sel = i == selected;
                    list = list.child(
                        div()
                            .id(SharedString::from(format!("qs-{i}")))
                            .px(px(12.))
                            .py(px(7.))
                            .rounded(px(6.))
                            .when(sel, |d| d.bg(t.row_selected))
                            .when(!sel, |d| d.hover(|s| s.bg(t.hover)))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .cursor(CursorStyle::PointingHand)
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.overlay = Overlay::None;
                                ws.open_path(&path, true, window, cx);
                            }))
                            .child(svg().path("icons/doc.svg").size(px(16.)).text_color(if sel { t.row_selected_text } else { t.secondary }))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(if sel { t.row_selected_text } else { t.text }).child(hit.title))
                                    .when(!hit.line.is_empty() || !rel.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .text_size(px(12.))
                                                .truncate()
                                                .text_color(if sel { t.row_selected_text.opacity(0.8) } else { t.secondary })
                                                .child(if hit.line.is_empty() { rel } else { hit.line }),
                                        )
                                    }),
                            ),
                    );
                }
                Some(
                    backdrop(true)
                        .flex()
                        .justify_center()
                        .items_start()
                        .child(
                            div()
                                .mt(px(90.))
                                .w(px(580.))
                                .h_auto()
                                .p(px(8.))
                                .bg(t.popover_bg)
                                .rounded(px(12.))
                                .border_1()
                                .border_color(t.popover_border)
                                .shadow_lg()
                                .font_family(crate::fonts::ui())
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(10.))
                                        .px(px(10.))
                                        .py(px(8.))
                                        .text_size(px(18.))
                                        .text_color(t.text)
                                        .child(svg().path("icons/search.svg").size(px(20.)).text_color(t.secondary))
                                        .child(field),
                                )
                                .child(div().h(px(1.)).bg(t.separator))
                                .child(list),
                        )
                        .into_any_element(),
                )
            }
            Overlay::Preferences(field, _) => Some(backdrop(true).flex().justify_center().items_center().child(self.render_preferences(field.clone(), cx)).into_any_element()),
            Overlay::Accounts(service, fields) => {
                let service = *service;
                let fields = fields.clone();
                let labels = ["Site URL", "User Name", "Token"];
                let mut tabs = div().flex().p(px(2.)).rounded(px(7.)).bg(t.hover);
                for svc in Service::ALL {
                    let on = svc == service;
                    tabs = tabs.child(
                        div()
                            .id(SharedString::from(format!("svc-{}", svc.label())))
                            .px(px(12.))
                            .py(px(3.))
                            .rounded(px(5.))
                            .text_size(px(12.))
                            .when(on, |d| d.bg(t.popover_bg).shadow_sm())
                            .cursor(CursorStyle::PointingHand)
                            .child(svc.label())
                            .on_click(cx.listener(move |ws, _, window, cx| ws.open_accounts(svc, window, cx))),
                    );
                }
                let button = |id: &str, label: &str, primary: bool| {
                    div()
                        .id(SharedString::from(id.to_string()))
                        .px(px(14.))
                        .py(px(4.))
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .cursor(CursorStyle::PointingHand)
                        .when(primary, |d| d.bg(t.accent).text_color(t.row_selected_text))
                        .when(!primary, |d| d.border_1().border_color(t.separator).hover(|s| s.bg(t.hover)))
                        .child(label.to_string())
                };
                Some(
                    backdrop(true)
                        .flex()
                        .justify_center()
                        .items_center()
                        .child(
                            div()
                                .w(px(520.))
                                .p(px(24.))
                                .bg(t.popover_bg)
                                .rounded(px(12.))
                                .border_1()
                                .border_color(t.popover_border)
                                .shadow_lg()
                                .font_family(crate::fonts::ui())
                                .text_color(t.text)
                                .flex()
                                .flex_col()
                                .gap(px(14.))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).child("Publishing Accounts"))
                                .child(tabs)
                                .children(fields.iter().enumerate().map(|(i, f)| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(12.))
                                        .child(div().w(px(90.)).text_size(px(13.)).text_color(t.secondary).child(labels[i]))
                                        .child(
                                            div()
                                                .flex_1()
                                                .px(px(8.))
                                                .py(px(4.))
                                                .rounded(px(6.))
                                                .border_1()
                                                .border_color(t.separator)
                                                .text_size(px(13.))
                                                .child(f.clone()),
                                        )
                                }))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(t.secondary)
                                        .child("Posts are created as drafts. Credentials are stored in ~/.config/quill/accounts.json (readable only by you)."),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .justify_end()
                                        .gap(px(8.))
                                        .child(button("acc-remove", "Remove", false).on_click(cx.listener(|ws, _, window, cx| ws.save_account(true, window, cx))))
                                        .child(button("acc-cancel", "Cancel", false).on_click(cx.listener(|ws, _, window, cx| ws.dismiss(window, cx))))
                                        .child(button("acc-save", "Save", true).on_click(cx.listener(|ws, _, window, cx| ws.save_account(false, window, cx)))),
                                ),
                        )
                        .into_any_element(),
                )
            }
            Overlay::AllTabs => {
                let mut grid = div().flex().flex_wrap().gap(px(18.)).justify_center();
                for (i, d) in self.docs.iter().enumerate() {
                    let e = d.editor.read(cx);
                    let title = e.title();
                    let body: String = crate::markdown::excerpt(e.text(), 220);
                    let active = i == self.active;
                    grid = grid.child(
                        div()
                            .id(SharedString::from(format!("alltab-{i}")))
                            .w(px(240.))
                            .h(px(170.))
                            .p(px(14.))
                            .rounded(px(10.))
                            .bg(t.bg)
                            .border_2()
                            .border_color(if active { t.accent } else { t.popover_border })
                            .shadow_lg()
                            .overflow_hidden()
                            .cursor(CursorStyle::PointingHand)
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::BOLD).text_color(t.text).truncate().child(title))
                            .child(div().text_size(px(11.)).line_height(px(16.)).text_color(t.secondary).font_family(crate::fonts::DUO).child(body))
                            .on_click(cx.listener(move |ws, _, window, cx| {
                                ws.active = i;
                                ws.overlay = Overlay::None;
                                let fh = ws.editor().focus_handle(cx);
                                window.focus(&fh);
                                ws.after_switch(window, cx);
                            })),
                    );
                }
                Some(backdrop(true).flex().items_center().justify_center().p(px(60.)).child(grid).into_any_element())
            }
            Overlay::About => Some(
                backdrop(true)
                    .flex()
                    .justify_center()
                    .items_center()
                    .child(
                        div()
                            .w(px(380.))
                            .p(px(28.))
                            .bg(t.popover_bg)
                            .rounded(px(12.))
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(8.))
                            .font_family(crate::fonts::ui())
                            .text_color(t.text)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(div().font_family(crate::fonts::DUO).text_size(px(34.)).font_weight(FontWeight::BOLD).child("Quill"))
                            .child(div().text_size(px(12.)).text_color(t.secondary).child(format!("Version {}", env!("CARGO_PKG_VERSION"))))
                            .child(div().text_size(px(13.)).text_center().child("A focused Markdown writer for Linux, modelled on iA Writer's design. Not affiliated with Information Architects Inc."))
                            .child(div().text_size(px(11.)).text_center().text_color(t.secondary).child("iA Writer Duo/Mono/Quattro and IBM Plex Sans are licensed under the SIL Open Font License 1.1.")),
                    )
                    .into_any_element(),
            ),
        }
    }

    fn render_preferences(&mut self, field: Entity<TextField>, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let t = *cx.global::<Theme>();
        let s = self.settings.clone();
        let segmented = |id: &str, options: Vec<(&'static str, bool, Box<dyn Fn(&mut Settings)>)>, cx: &mut Context<Self>| {
            let mut row = div().flex().p(px(2.)).rounded(px(7.)).bg(t.hover);
            for (i, (label, on, f)) in options.into_iter().enumerate() {
                let f: Rc<dyn Fn(&mut Settings)> = Rc::from(f);
                row = row.child(
                    div()
                        .id(SharedString::from(format!("{id}-{i}")))
                        .px(px(12.))
                        .py(px(3.))
                        .rounded(px(5.))
                        .text_size(px(12.))
                        .when(on, |d| d.bg(t.popover_bg).shadow_sm())
                        .cursor(CursorStyle::PointingHand)
                        .child(label)
                        .on_click(cx.listener(move |ws, _, window, cx| {
                            let f = f.clone();
                            ws.update_settings(cx, |s| f(s));
                            ws.apply_theme(window, cx);
                        })),
                );
            }
            row
        };
        let toggle = |id: &str, label: &str, on: bool, f: fn(&mut Settings), cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(id.to_string()))
                .flex()
                .items_center()
                .gap(px(8.))
                .cursor(CursorStyle::PointingHand)
                .on_click(cx.listener(move |ws, _, _, cx| ws.update_settings(cx, f)))
                .child(
                    div()
                        .size(px(16.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(if on { t.accent } else { t.tertiary })
                        .when(on, |d| d.bg(t.accent))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(on, |d| d.child(svg().path("icons/check.svg").size(px(12.)).text_color(t.row_selected_text))),
                )
                .child(div().text_size(px(13.)).child(label.to_string()))
        };
        let row = |label: &str, control: AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(div().w(px(150.)).text_align(gpui::TextAlign::Right).text_size(px(13.)).text_color(t.secondary).child(label.to_string()))
                .child(control)
        };
        let lib = self.lib_root.display().to_string();
        div()
            .w(px(600.))
            .p(px(24.))
            .bg(t.popover_bg)
            .rounded(px(12.))
            .border_1()
            .border_color(t.popover_border)
            .shadow_lg()
            .font_family(crate::fonts::ui())
            .text_color(t.text)
            .flex()
            .flex_col()
            .gap(px(14.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).child("Preferences"))
            .child(row(
                "Appearance",
                segmented(
                    "appearance",
                    vec![
                        ("System", s.appearance == Appearance::System, Box::new(|s: &mut Settings| s.appearance = Appearance::System)),
                        ("Light", s.appearance == Appearance::Light, Box::new(|s: &mut Settings| s.appearance = Appearance::Light)),
                        ("Dark", s.appearance == Appearance::Dark, Box::new(|s: &mut Settings| s.appearance = Appearance::Dark)),
                    ],
                    cx,
                )
                .into_any_element(),
            ))
            .child(row(
                "Font",
                segmented(
                    "font",
                    vec![
                        ("Mono", s.font == FontChoice::Mono, Box::new(|s: &mut Settings| s.font = FontChoice::Mono)),
                        ("Duo", s.font == FontChoice::Duo, Box::new(|s: &mut Settings| s.font = FontChoice::Duo)),
                        ("Quattro", s.font == FontChoice::Quattro, Box::new(|s: &mut Settings| s.font = FontChoice::Quattro)),
                    ],
                    cx,
                )
                .into_any_element(),
            ))
            .child(row(
                "Text Size",
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(icon_button("size-down", "arrow-down", &t, false).on_click(cx.listener(|ws, _, _, cx| {
                        ws.update_settings(cx, |s| s.text_size = s.text_size.saturating_sub(1))
                    })))
                    .child(div().text_size(px(13.)).child(format!("{} px", TEXT_SIZES[s.text_size.min(TEXT_SIZES.len() - 1)].0)))
                    .child(icon_button("size-up", "arrow-up", &t, false).on_click(cx.listener(|ws, _, _, cx| {
                        ws.update_settings(cx, |s| s.text_size = (s.text_size + 1).min(TEXT_SIZES.len() - 1))
                    })))
                    .into_any_element(),
            ))
            .child(row(
                "Editor",
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(toggle("p-typewriter", "Typewriter mode", s.typewriter, |s| s.typewriter = !s.typewriter, cx))
                    .child(toggle("p-quotes", "Smart quotes", s.smart_quotes, |s| s.smart_quotes = !s.smart_quotes, cx))
                    .child(toggle("p-dashes", "Smart dashes ( -- → — )", s.smart_dashes, |s| s.smart_dashes = !s.smart_dashes, cx))
                    .child(toggle("p-pair", "Auto-pair brackets and quotes", s.auto_pair, |s| s.auto_pair = !s.auto_pair, cx))
                    .child(toggle("p-spell", "Check spelling while typing", s.spell, |s| s.spell = !s.spell, cx))
                    .into_any_element(),
            ))
            .child(row(
                "Highlight",
                {
                    let mut row = div().flex().gap(px(8.));
                    for (i, hs) in crate::theme::HighlightStyle::ALL.into_iter().enumerate() {
                        let (_, bg, ul) = hs.colors(t.dark);
                        let on = s.highlight_style == hs;
                        row = row.child(
                            div()
                                .id(SharedString::from(format!("hl-{i}")))
                                .px(px(8.))
                                .py(px(2.))
                                .rounded(px(4.))
                                .text_size(px(12.))
                                .bg(gpui::rgba(bg))
                                .border_b_2()
                                .border_color(gpui::rgba(ul))
                                .when(on, |d| d.border_1().border_color(t.accent))
                                .cursor(CursorStyle::PointingHand)
                                .child(hs.label())
                                .on_click(cx.listener(move |ws, _, window, cx| {
                                    ws.update_settings(cx, |s| s.highlight_style = hs);
                                    ws.apply_theme(window, cx);
                                })),
                        );
                    }
                    row.into_any_element()
                },
            ))
            .child(row(
                "Title Bar",
                segmented(
                    "titlebar",
                    vec![
                        ("Fade In/Out", s.title_bar == BarMode::FadeInOut, Box::new(|s: &mut Settings| s.title_bar = BarMode::FadeInOut)),
                        ("Always Show", s.title_bar == BarMode::AlwaysShow, Box::new(|s: &mut Settings| s.title_bar = BarMode::AlwaysShow)),
                    ],
                    cx,
                )
                .into_any_element(),
            ))
            .child(row(
                "Library",
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(12.)).max_w(px(260.)).truncate().child(lib))
                    .child(
                        div()
                            .id("choose-lib")
                            .px(px(10.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(t.separator)
                            .text_size(px(12.))
                            .cursor(CursorStyle::PointingHand)
                            .hover(|s| s.bg(t.hover))
                            .child("Choose…")
                            .on_click(cx.listener(|_, _, window, cx| {
                                let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
                                    files: false,
                                    directories: true,
                                    multiple: false,
                                    prompt: Some("Choose Library Folder".into()),
                                });
                                cx.spawn_in(window, async move |ws, cx| {
                                    if let Ok(Ok(Some(paths))) = rx.await
                                        && let Some(dir) = paths.into_iter().next()
                                    {
                                        let _ = ws.update(cx, |ws, cx| {
                                            ws.lib_root = dir.clone();
                                            ws.lib_dir = dir.clone();
                                            ws.update_settings(cx, |s| s.library_dir = Some(dir));
                                            ws.refresh_library();
                                        });
                                    }
                                })
                                .detach();
                            })),
                    )
                    .into_any_element(),
            ))
            .child(row(
                "URL Commands",
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(12.)).font_family(crate::fonts::MONO).child(format!("auth-token={}", s.url_token)))
                    .child(
                        div()
                            .id("copy-token")
                            .px(px(10.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(t.separator)
                            .text_size(px(12.))
                            .cursor(CursorStyle::PointingHand)
                            .hover(|s| s.bg(t.hover))
                            .child("Copy")
                            .on_click(cx.listener(|ws, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(ws.settings.url_token.clone()));
                                ws.show_toast("Auth token copied", cx);
                            })),
                    )
                    .into_any_element(),
            ))
            .child(row(
                "Custom Style Check",
                div()
                    .w(px(320.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.separator)
                    .text_size(px(13.))
                    .child(field)
                    .into_any_element(),
            ))
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *cx.global::<Theme>();
        let decorations = window.window_decorations();
        let inset = px(SHADOW);
        let tiling = match decorations {
            Decorations::Client { tiling } => Some(tiling),
            Decorations::Server => None,
        };
        let floating = tiling.is_some_and(|t| !t.is_tiled()) && !window.is_fullscreen() && !window.is_maximized();
        window.set_client_inset(if floating { inset } else { px(0.) });
        // GPUI clips children to rectangles only, so every element touching a
        // window corner rounds that corner itself.
        self.corner = if floating { px(ROUNDING) } else { px(0.) };
        let r = self.corner;
        let studio_open = self.settings.studio_open;
        let preview_corner = if studio_open { px(0.) } else { r };
        self.preview.update(cx, |p, _| p.corner = preview_corner);

        let organizer = (self.settings.show_library && self.settings.show_organizer).then(|| self.render_organizer(window, cx));
        let sidebar = self.settings.show_library.then(|| self.render_file_list(window, cx));
        let toolbar = self.render_toolbar(window, cx);
        let tabs = (self.docs.len() > 1 || self.settings.always_show_tabs).then(|| self.render_tabs(cx));
        let find = self.render_find(cx);
        let status = self.render_status(cx);
        let overlay = self.render_overlay(window, cx);
        let studio = studio_open.then(|| self.render_studio(window, cx));
        let editor = self.editor().clone();
        let preview_on = self.settings.preview;
        let full_preview = preview_on && self.settings.preview_layout == PreviewLayout::Full;
        let toast = self.toast.as_ref().map(|(m, _)| m.clone());

        let content = div()
            .flex_1()
            .min_h_0()
            .flex()
            .relative()
            .when(!full_preview, |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .relative()
                        .when(!preview_on && !studio_open, |d| d.rounded_br(r))
                        .when(!self.settings.show_library, |d| d.rounded_bl(r))
                        .child(editor)
                        .child(status),
                )
            })
            .when(preview_on, |d| {
                d.when(!full_preview, |d| d.child(div().w(px(1.)).h_full().bg(t.separator)))
                    .child(div().flex_1().min_w_0().h_full().child(self.preview.clone()))
            })
            .when_some(studio, |d, panel| d.child(div().w(px(1.)).h_full().bg(t.separator)).child(panel));

        let main = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .rounded_tr(r)
            .rounded_br(r)
            .when(!self.settings.show_library, |d| d.rounded_tl(r).rounded_bl(r))
            .child(toolbar)
            .children(tabs)
            .children(find)
            .child(content);

        let app = div()
            .size_full()
            .flex()
            .bg(t.bg)
            .overflow_hidden()
            .when(floating, |d| {
                d.rounded(px(ROUNDING)).border_1().border_color(t.popover_border).shadow(vec![gpui::BoxShadow {
                    color: Hsla { h: 0., s: 0., l: 0., a: 0.35 },
                    blur_radius: inset / 2.,
                    spread_radius: px(0.),
                    offset: point(px(0.), px(1.)),
                }])
            })
            .cursor(CursorStyle::Arrow)
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .children(organizer)
            .children(sidebar)
            .child(main);

        div()
            .id("root")
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .bg(transparent_black())
            .font_family(crate::fonts::ui())
            .when(floating, |d| {
                d.p(inset).child(
                    canvas(
                        |_, window, _| {
                            window.insert_hitbox(Bounds::new(point(px(0.), px(0.)), window.window_bounds().get_bounds().size), HitboxBehavior::Normal)
                        },
                        move |_, hitbox, window, _| {
                            let size = window.window_bounds().get_bounds().size;
                            if let Some(edge) = resize_edge(window.mouse_position(), inset, size) {
                                window.set_cursor_style(
                                    match edge {
                                        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                                        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
                                        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
                                        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
                                    },
                                    &hitbox,
                                );
                            }
                        },
                    )
                    .size_full()
                    .absolute(),
                )
                .on_mouse_down(MouseButton::Left, move |e, window, _| {
                    let size = window.window_bounds().get_bounds().size;
                    if let Some(edge) = resize_edge(e.position, inset, size) {
                        log::debug!("edge drag: start_window_resize({edge:?})");
                        window.start_window_resize(edge);
                    }
                })
            })
            .on_mouse_move(cx.listener(|ws, _, window, cx| {
                let hidden = !ws.chrome_visible(BarMode::FadeInOut, cx);
                ws.last_mouse_move = Instant::now();
                if hidden {
                    cx.notify();
                }
                window.refresh();
            }))
            .child(app)
            .children(overlay)
            .when_some(toast, |d, msg| {
                d.child(
                    div().absolute().bottom(px(48.)).left_0().right_0().flex().justify_center().child(
                        div()
                            .px(px(14.))
                            .py(px(7.))
                            .rounded(px(8.))
                            .bg(t.popover_bg)
                            .border_1()
                            .border_color(t.popover_border)
                            .shadow_lg()
                            .text_size(px(13.))
                            .text_color(t.text)
                            .child(msg),
                    ),
                )
            })
            .on_action(cx.listener(|ws, _: &MenuUp, _, cx| ws.menu_key("up", cx)))
            .on_action(cx.listener(|ws, _: &MenuDown, _, cx| ws.menu_key("down", cx)))
            .on_action(cx.listener(|ws, _: &MenuLeft, _, cx| ws.menu_key("left", cx)))
            .on_action(cx.listener(|ws, _: &MenuRight, _, cx| ws.menu_key("right", cx)))
            .on_action(cx.listener(|ws, _: &MenuConfirm, window, cx| ws.menu_confirm(window, cx)))
            .on_action(cx.listener(|ws, _: &OpenMenu, window, cx| {
                let items = ws.main_menu(cx);
                let w = window.viewport_size().width;
                ws.open_menu(items, point(w - px(menus::WIDTH + 30.), px(TOOLBAR_H + 4.)), window, cx);
                if let Overlay::Menu(m) = &mut ws.overlay {
                    m.open = vec![0];
                }
            }))
            .on_action(cx.listener(|ws, _: &ToggleOrganizer, _, cx| ws.update_settings(cx, |s| {
                s.show_organizer = !s.show_organizer;
                s.show_library = true
            })))
            .on_action(cx.listener(|ws, _: &ToggleSortBar, _, cx| ws.update_settings(cx, |s| s.show_sort_bar = !s.show_sort_bar)))
            .on_action(cx.listener(|ws, _: &ToggleFilterBar, window, cx| {
                ws.show_filter = !ws.show_filter;
                if ws.show_filter {
                    ws.settings.show_library = true;
                    let fh = ws.filter.focus_handle(cx);
                    window.focus(&fh);
                } else {
                    ws.filter.update(cx, |f, cx| f.set_text("", cx));
                    ws.filter_text.clear();
                }
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ToggleTabBar, _, cx| ws.update_settings(cx, |s| s.always_show_tabs = !s.always_show_tabs)))
            .on_action(cx.listener(|ws, _: &ShowAllTabs, _, cx| {
                ws.overlay = Overlay::AllTabs;
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &ShowCompletions, window, cx| ws.show_completions(window, cx)))
            .on_action(cx.listener(|ws, _: &ManageAccounts, window, cx| ws.open_accounts(Service::WordPress, window, cx)))
            .on_action(cx.listener(Self::handle_quit))
            .on_action(cx.listener(|ws, _: &About, _, cx| {
                ws.overlay = Overlay::About;
                cx.notify()
            }))
            .on_action(cx.listener(|ws, _: &Preferences, window, cx| ws.open_preferences(window, cx)))
            .on_action(cx.listener(|ws, _: &Help, window, cx| ws.add_doc(crate::help::HELP, None, window, cx)))
            .on_action(cx.listener(|ws, _: &MarkdownGuide, window, cx| ws.add_doc(crate::help::MARKDOWN, None, window, cx)))
            .on_action(cx.listener(|ws, _: &WhatsNew, window, cx| ws.add_doc(crate::help::WHATS_NEW, None, window, cx)))
            .on_action(cx.listener(|ws, _: &NewDocument, window, cx| ws.new_doc(window, cx)))
            .on_action(cx.listener(|ws, _: &NewTab, window, cx| ws.new_doc(window, cx)))
            .on_action(cx.listener(|ws, _: &Open, window, cx| {
                let rx = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
                cx.spawn_in(window, async move |ws, cx| {
                    if let Ok(Ok(Some(paths))) = rx.await {
                        let _ = ws.update_in(cx, |ws, window, cx| {
                            for p in paths {
                                ws.open_path(&p, true, window, cx);
                            }
                        });
                    }
                })
                .detach();
                let _ = ws;
            }))
            .on_action(cx.listener(|ws, _: &OpenRecentClear, _, cx| ws.update_settings(cx, |s| s.recent.clear())))
            .on_action(cx.listener(|ws, _: &Close, window, cx| {
                let ix = ws.active;
                ws.close_tab(ix, window, cx)
            }))
            .on_action(cx.listener(|ws, _: &Save, window, cx| {
                if ws.editor().read(cx).path.is_some() {
                    let ix = ws.active;
                    ws.flush_save(ix, cx);
                    ws.update_title(window, cx);
                } else {
                    window.dispatch_action(Box::new(SaveAs), cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &SaveAs, window, cx| {
                let e = ws.editor().read(cx);
                let name = format!("{}.md", Self::sanitize(&crate::markdown::title(e.text())));
                let dir = e.path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| ws.lib_dir.clone());
                let editor = ws.editor().clone();
                let rx = cx.prompt_for_new_path(&dir, Some(&name));
                cx.spawn_in(window, async move |ws, cx| {
                    if let Ok(Ok(Some(path))) = rx.await {
                        let _ = ws.update_in(cx, |ws, window, cx| {
                            editor.update(cx, |e, cx| {
                                e.path = Some(path.clone());
                                // Force a write even if nothing changed since the last autosave.
                                e.buffer.insert("");
                                cx.notify();
                            });
                            let text = editor.read(cx).text().to_string();
                            match std::fs::write(&path, text) {
                                Ok(()) => editor.update(cx, |e, _| e.buffer.mark_saved()),
                                Err(err) => ws.show_toast(format!("Could not save: {err}"), cx),
                            }
                            ws.settings.push_recent(path);
                            ws.settings.save();
                            ws.refresh_library();
                            ws.update_title(window, cx);
                        });
                    }
                })
                .detach();
            }))
            .on_action(cx.listener(|ws, _: &Revert, window, cx| {
                let Some(path) = ws.editor().read(cx).path.clone() else { return };
                if let Ok(text) = std::fs::read_to_string(&path) {
                    ws.editor().update(cx, |e, cx| {
                        e.buffer.reset(text);
                        cx.notify();
                    });
                    ws.update_title(window, cx);
                    ws.refresh_preview(cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &ShowInFolder, _, cx| {
                if let Some(p) = ws.editor().read(cx).path.clone() {
                    cx.reveal_path(&p);
                }
            }))
            .on_action(cx.listener(|ws, _: &ShowInLibrary, _, cx| {
                if let Some(dir) = ws.editor().read(cx).path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)) {
                    ws.settings.show_library = true;
                    ws.enter_folder(dir, cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &Export, window, cx| ws.export_to(Format::Pdf, window, cx)))
            .on_action(cx.listener(|ws, _: &ExportPdf, window, cx| ws.export_to(Format::Pdf, window, cx)))
            .on_action(cx.listener(|ws, _: &ExportHtml, window, cx| ws.export_to(Format::Html, window, cx)))
            .on_action(cx.listener(|ws, _: &ExportDocx, window, cx| ws.export_to(Format::Docx, window, cx)))
            .on_action(cx.listener(|ws, _: &ExportText, window, cx| ws.export_to(Format::Text, window, cx)))
            .on_action(cx.listener(|ws, _: &Print, _, cx| ws.print(false, cx)))
            .on_action(cx.listener(|ws, _: &PrintPlainText, _, cx| ws.print(true, cx)))
            .on_action(cx.listener(|ws, _: &Find, window, cx| ws.open_find(false, window, cx)))
            .on_action(cx.listener(|ws, _: &FindReplace, window, cx| ws.open_find(true, window, cx)))
            .on_action(cx.listener(|ws, _: &UseSelectionForFind, window, cx| ws.open_find(false, window, cx)))
            .on_action(cx.listener(|ws, _: &FindNext, _, cx| ws.editor().update(cx, |e, cx| e.find_step(true, cx))))
            .on_action(cx.listener(|ws, _: &FindPrevious, _, cx| ws.editor().update(cx, |e, cx| e.find_step(false, cx))))
            .on_action(cx.listener(|ws, _: &ReplaceOne, _, cx| {
                if let Some(f) = &ws.find {
                    let r = f.replace.read(cx).text();
                    ws.editor().update(cx, |e, cx| e.replace_current(&r, cx));
                }
            }))
            .on_action(cx.listener(|ws, _: &ReplaceAll, _, cx| {
                if let Some(f) = &ws.find {
                    let r = f.replace.read(cx).text();
                    let n = ws.editor().update(cx, |e, cx| e.replace_all(&r, cx));
                    ws.show_toast(format!("Replaced {n} occurrences"), cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &FilterLibrary, window, cx| {
                ws.settings.show_library = true;
                ws.show_filter = true;
                let fh = ws.filter.focus_handle(cx);
                window.focus(&fh);
                cx.notify();
            }))
            .on_action(cx.listener(|ws, _: &StartSpeaking, _, cx| ws.speak(cx)))
            .on_action(cx.listener(|ws, _: &StopSpeaking, _, _| ws.stop_speaking()))
            .on_action(cx.listener(|ws, _: &ToggleSmartQuotes, _, cx| ws.update_settings(cx, |s| s.smart_quotes = !s.smart_quotes)))
            .on_action(cx.listener(|ws, _: &ToggleSmartDashes, _, cx| ws.update_settings(cx, |s| s.smart_dashes = !s.smart_dashes)))
            .on_action(cx.listener(|ws, _: &ToggleAutoPair, _, cx| ws.update_settings(cx, |s| s.auto_pair = !s.auto_pair)))
            .on_action(cx.listener(|ws, _: &ToggleSpelling, _, cx| ws.update_settings(cx, |s| s.spell = !s.spell)))
            .on_action(cx.listener(|ws, _: &CheckSpelling, _, cx| {
                ws.update_settings(cx, |s| s.spell = true);
                let n = ws.editor().read(cx).misspelled.len();
                ws.show_toast(if n == 0 { "No spelling issues".to_string() } else { format!("{n} possible misspellings") }, cx);
            }))
            .on_action(cx.listener(|ws, _: &AppearanceSystem, window, cx| {
                ws.update_settings(cx, |s| s.appearance = Appearance::System);
                ws.apply_theme(window, cx)
            }))
            .on_action(cx.listener(|ws, _: &AppearanceLight, window, cx| {
                ws.update_settings(cx, |s| s.appearance = Appearance::Light);
                ws.apply_theme(window, cx)
            }))
            .on_action(cx.listener(|ws, _: &AppearanceDark, window, cx| {
                ws.update_settings(cx, |s| s.appearance = Appearance::Dark);
                ws.apply_theme(window, cx)
            }))
            .on_action(cx.listener(|ws, _: &ToggleDarkAppearance, window, cx| {
                let dark = cx.global::<Theme>().dark;
                ws.update_settings(cx, |s| s.appearance = if dark { Appearance::Light } else { Appearance::Dark });
                ws.apply_theme(window, cx)
            }))
            .on_action(cx.listener(|ws, _: &ToggleLibrary, _, cx| ws.update_settings(cx, |s| s.show_library = !s.show_library)))
            .on_action(cx.listener(|ws, _: &SortByName, _, cx| {
                ws.update_settings(cx, |s| s.sort = SortBy::Name);
                ws.refresh_library()
            }))
            .on_action(cx.listener(|ws, _: &SortByDate, _, cx| {
                ws.update_settings(cx, |s| s.sort = SortBy::Modified);
                ws.refresh_library()
            }))
            .on_action(cx.listener(|ws, _: &TextBigger, _, cx| ws.update_settings(cx, |s| s.text_size = (s.text_size + 1).min(TEXT_SIZES.len() - 1))))
            .on_action(cx.listener(|ws, _: &TextSmaller, _, cx| ws.update_settings(cx, |s| s.text_size = s.text_size.saturating_sub(1))))
            .on_action(cx.listener(|ws, _: &TextNormal, _, cx| ws.update_settings(cx, |s| s.text_size = crate::settings::DEFAULT_TEXT_SIZE)))
            .on_action(cx.listener(|ws, _: &TogglePreview, _, cx| ws.update_settings(cx, |s| s.preview = !s.preview)))
            .on_action(cx.listener(|ws, _: &ReloadPreview, _, cx| {
                ws.settings.preview = true;
                ws.settings_changed(cx)
            }))
            .on_action(cx.listener(|ws, _: &PreviewFull, _, cx| ws.update_settings(cx, |s| {
                s.preview = true;
                s.preview_layout = PreviewLayout::Full
            })))
            .on_action(cx.listener(|ws, _: &PreviewSplit, _, cx| ws.update_settings(cx, |s| {
                s.preview = true;
                s.preview_layout = PreviewLayout::Split
            })))
            .on_action(cx.listener(|ws, _: &FontMono, _, cx| ws.update_settings(cx, |s| s.font = FontChoice::Mono)))
            .on_action(cx.listener(|ws, _: &FontDuo, _, cx| ws.update_settings(cx, |s| s.font = FontChoice::Duo)))
            .on_action(cx.listener(|ws, _: &FontQuattro, _, cx| ws.update_settings(cx, |s| s.font = FontChoice::Quattro)))
            .on_action(cx.listener(|ws, _: &TitleBarFade, _, cx| ws.update_settings(cx, |s| s.title_bar = BarMode::FadeInOut)))
            .on_action(cx.listener(|ws, _: &TitleBarAlways, _, cx| ws.update_settings(cx, |s| s.title_bar = BarMode::AlwaysShow)))
            .on_action(cx.listener(|ws, _: &ToolbarFade, _, cx| ws.update_settings(cx, |s| s.toolbar = BarMode::FadeInOut)))
            .on_action(cx.listener(|ws, _: &ToolbarAlways, _, cx| ws.update_settings(cx, |s| s.toolbar = BarMode::AlwaysShow)))
            .on_action(cx.listener(|ws, _: &ToolbarHide, _, cx| ws.update_settings(cx, |s| s.toolbar = BarMode::Hide)))
            .on_action(cx.listener(|ws, _: &StatCharacters, _, cx| ws.update_settings(cx, |s| s.stat = StatKind::Characters)))
            .on_action(cx.listener(|ws, _: &StatCharactersNoSpaces, _, cx| ws.update_settings(cx, |s| s.stat = StatKind::CharactersWithoutSpaces)))
            .on_action(cx.listener(|ws, _: &StatWords, _, cx| ws.update_settings(cx, |s| s.stat = StatKind::Words)))
            .on_action(cx.listener(|ws, _: &StatSentences, _, cx| ws.update_settings(cx, |s| s.stat = StatKind::Sentences)))
            .on_action(cx.listener(|ws, _: &StatReadingTime, _, cx| ws.update_settings(cx, |s| s.stat = StatKind::ReadingTime)))
            .on_action(cx.listener(|ws, _: &StatSpeakingTime, _, cx| ws.update_settings(cx, |s| s.stat = StatKind::SpeakingTime)))
            .on_action(cx.listener(|ws, _: &StatTasks, _, cx| ws.update_settings(cx, |s| s.stat = StatKind::Tasks)))
            .on_action(cx.listener(|_, _: &ToggleFullScreen, window, _| window.toggle_fullscreen()))
            .on_action(cx.listener(|ws, _: &ToggleFocus, _, cx| ws.update_settings(cx, |s| {
                s.focus = if s.focus == FocusMode::Off { s.focus_kind } else { FocusMode::Off }
            })))
            .on_action(cx.listener(|ws, _: &FocusOff, _, cx| ws.update_settings(cx, |s| s.focus = FocusMode::Off)))
            .on_action(cx.listener(|ws, _: &FocusSentence, _, cx| ws.update_settings(cx, |s| {
                s.focus_kind = FocusMode::Sentence;
                s.focus = FocusMode::Sentence
            })))
            .on_action(cx.listener(|ws, _: &FocusParagraph, _, cx| ws.update_settings(cx, |s| {
                s.focus_kind = FocusMode::Paragraph;
                s.focus = FocusMode::Paragraph
            })))
            .on_action(cx.listener(|ws, _: &ToggleTypewriter, _, cx| ws.update_settings(cx, |s| s.typewriter = !s.typewriter)))
            .on_action(cx.listener(|ws, _: &ToggleSyntax, _, cx| ws.update_settings(cx, |s| s.syntax = !s.syntax)))
            .on_action(cx.listener(|ws, _: &SyntaxAdjectives, _, cx| ws.update_settings(cx, |s| {
                s.syntax_adjectives = !s.syntax_adjectives;
                s.syntax = true
            })))
            .on_action(cx.listener(|ws, _: &SyntaxNouns, _, cx| ws.update_settings(cx, |s| {
                s.syntax_nouns = !s.syntax_nouns;
                s.syntax = true
            })))
            .on_action(cx.listener(|ws, _: &SyntaxAdverbs, _, cx| ws.update_settings(cx, |s| {
                s.syntax_adverbs = !s.syntax_adverbs;
                s.syntax = true
            })))
            .on_action(cx.listener(|ws, _: &SyntaxVerbs, _, cx| ws.update_settings(cx, |s| {
                s.syntax_verbs = !s.syntax_verbs;
                s.syntax = true
            })))
            .on_action(cx.listener(|ws, _: &SyntaxConjunctions, _, cx| ws.update_settings(cx, |s| {
                s.syntax_conjunctions = !s.syntax_conjunctions;
                s.syntax = true
            })))
            .on_action(cx.listener(|ws, _: &ToggleStyleCheck, _, cx| ws.update_settings(cx, |s| s.style_check = !s.style_check)))
            .on_action(cx.listener(|ws, _: &CheckFillers, _, cx| ws.update_settings(cx, |s| {
                s.check_fillers = !s.check_fillers;
                s.style_check = true
            })))
            .on_action(cx.listener(|ws, _: &CheckCliches, _, cx| ws.update_settings(cx, |s| {
                s.check_cliches = !s.check_cliches;
                s.style_check = true
            })))
            .on_action(cx.listener(|ws, _: &CheckRedundancies, _, cx| ws.update_settings(cx, |s| {
                s.check_redundancies = !s.check_redundancies;
                s.style_check = true
            })))
            .on_action(cx.listener(|ws, _: &CheckCustom, _, cx| ws.update_settings(cx, |s| {
                s.check_custom = !s.check_custom;
                s.style_check = true
            })))
            .on_action(cx.listener(|ws, _: &ToggleAuthors, _, cx| ws.update_settings(cx, |s| s.show_authors = !s.show_authors)))
            .on_action(cx.listener(|ws, _: &ToggleStudio, _, cx| ws.toggle_studio(None, cx)))
            .on_action(cx.listener(|ws, _: &StudioAnalyze, _, cx| ws.toggle_studio(Some(StudioTab::Analyze), cx)))
            .on_action(cx.listener(|ws, _: &StudioCoach, _, cx| ws.toggle_studio(Some(StudioTab::Coach), cx)))
            .on_action(cx.listener(|ws, _: &StudioTemplates, _, cx| ws.toggle_studio(Some(StudioTab::Templates), cx)))
            .on_action(cx.listener(|ws, _: &StudioProgress, _, cx| ws.toggle_studio(Some(StudioTab::Progress), cx)))
            .on_action(cx.listener(|ws, _: &StudioNextIssue, window, cx| ws.next_issue(window, cx)))
            .on_action(cx.listener(|ws, _: &StudioHints, _, cx| ws.run_coach(crate::studio::assistant::Task::Hints, String::new(), cx)))
            .on_action(cx.listener(|ws, _: &StudioPractice, window, cx| ws.practice_recommended(window, cx)))
            .on_action(cx.listener(|ws, _: &ToggleStudioReveal, _, cx| ws.update_settings(cx, |s| s.studio_reveal = !s.studio_reveal)))
            .on_action(cx.listener(|ws, _: &ToggleStudioLive, _, cx| {
                ws.update_settings(cx, |s| s.studio_live = !s.studio_live);
                if !ws.studio_active() {
                    ws.clear_studio_marks(cx);
                }
            }))
            .on_action(cx.listener(|ws, _: &QuickSearch, window, cx| ws.open_quick_search(window, cx)))
            .on_action(cx.listener(|ws, _: &NextTab, window, cx| {
                ws.active = (ws.active + 1) % ws.docs.len();
                let fh = ws.editor().focus_handle(cx);
                window.focus(&fh);
                ws.after_switch(window, cx)
            }))
            .on_action(cx.listener(|ws, _: &PreviousTab, window, cx| {
                ws.active = (ws.active + ws.docs.len() - 1) % ws.docs.len();
                let fh = ws.editor().focus_handle(cx);
                window.focus(&fh);
                ws.after_switch(window, cx)
            }))
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &Zoom, window, _| window.zoom_window()))
            .on_action(cx.listener(|ws, _: &DismissOverlay, window, cx| ws.dismiss(window, cx)))
    }
}

impl Workspace {
    fn handle_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        self.save_studio();
        self.save_settings_on_exit(window);
        self.stop_speaking();
        let unsaved = self.docs.iter().any(|d| {
            let e = d.editor.read(cx);
            e.buffer.is_dirty() && !e.text().trim().is_empty()
        });
        if !unsaved {
            cx.quit();
            return;
        }
        let answer = window.prompt(PromptLevel::Warning, "Quit with unsaved changes?", Some("Some documents could not be saved."), &["Quit", "Cancel"], cx);
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                let _ = cx.update(|cx| cx.quit());
            }
        })
        .detach();
    }

    pub fn save_settings_on_exit(&mut self, window: &Window) {
        let b = window.bounds();
        // While floating, the bounds include the client-side shadow inset on both sides.
        let inset = if self.corner > px(0.) { 2. * SHADOW } else { 0. };
        self.settings.window_width = f32::from(b.size.width) - inset;
        self.settings.window_height = f32::from(b.size.height) - inset;
        self.settings.save();
    }
}
