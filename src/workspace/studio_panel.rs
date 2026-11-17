//! Writing Studio panel: Analyze (deterministic findings and lessons), Coach
//! (LLM tasks), Templates, and Progress (learner profile and practice).

use std::collections::{HashMap, HashSet};

use gpui::EntityId;

use super::*;
use crate::studio::analysis::{self, Options, Report};
use crate::studio::assistant::{self, Task as AiTask};
use crate::studio::exercises::{self, Exercise, Grade};
use crate::studio::history::{self, History, Snapshot};
use crate::studio::templates::{self, Template};
use crate::studio::{Category, Issue, Severity, llm, rules};

pub(super) const PANEL_W: f32 = 360.;
const MAX_LISTED: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum StudioTab {
    Analyze,
    Coach,
    Templates,
    Progress,
}

pub(super) struct StudioState {
    pub tab: StudioTab,
    report: Option<Report>,
    /// (editor, buffer version) the report was computed for.
    report_for: Option<(EntityId, u64)>,
    report_text: String,
    analyze_task: Option<Task<()>>,
    filter: Option<Category>,
    selected: Option<usize>,
    revealed: HashSet<usize>,
    pub history: History,
    templates: Vec<Template>,
    /// Templates of documents without a path yet.
    session_templates: HashMap<EntityId, String>,
    last_recorded: Option<Instant>,
    last_saved: Option<Instant>,
    // Coach
    api_key: String,
    key_field: Entity<TextField>,
    ask_field: Entity<TextField>,
    goal: usize,
    coach_title: String,
    coach_text: String,
    coach_busy: bool,
    coach_task: Option<Task<()>>,
    // Practice
    exercise: Option<Exercise>,
    queue: Vec<Exercise>,
    grade: Option<Grade>,
    answer_field: Entity<TextField>,
    /// Focus the answer field on the next render (set from background tasks).
    focus_answer: bool,
    // Timed writing (templates with a time limit)
    timer: Option<(Instant, u32)>,
    timer_task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl StudioState {
    pub fn new(cx: &mut Context<Workspace>) -> Self {
        let key_field = cx.new(|cx| TextField::new("sk-ant-…", cx));
        let ask_field = cx.new(|cx| TextField::new("Ask your coach about this text…", cx));
        let answer_field = cx.new(|cx| TextField::new("Type your corrected sentence…", cx));
        let subs = vec![
            cx.subscribe(&ask_field, |ws: &mut Workspace, f, ev: &FieldEvent, cx| {
                if let FieldEvent::Confirm = ev {
                    let q = f.read(cx).text();
                    if !q.trim().is_empty() {
                        ws.run_coach(AiTask::Ask, q, cx);
                    }
                }
            }),
            cx.subscribe(&answer_field, |ws: &mut Workspace, _, ev: &FieldEvent, cx| {
                if let FieldEvent::Confirm = ev {
                    ws.check_answer(cx);
                }
            }),
            cx.subscribe(&key_field, |ws: &mut Workspace, _, ev: &FieldEvent, cx| {
                if let FieldEvent::Confirm = ev {
                    ws.save_api_key(cx);
                }
            }),
        ];
        let api_key = llm::load_key();
        StudioState {
            tab: StudioTab::Analyze,
            report: None,
            report_for: None,
            report_text: String::new(),
            analyze_task: None,
            filter: None,
            selected: None,
            revealed: HashSet::new(),
            history: History::load(),
            templates: templates::load_all(),
            session_templates: HashMap::new(),
            last_recorded: None,
            last_saved: None,
            api_key,
            key_field,
            ask_field,
            goal: 0,
            coach_title: String::new(),
            coach_text: String::new(),
            coach_busy: false,
            coach_task: None,
            exercise: None,
            queue: Vec::new(),
            grade: None,
            answer_field,
            focus_answer: false,
            timer: None,
            timer_task: None,
            _subs: subs,
        }
    }
}

// ---- small view helpers ---------------------------------------------------------

fn button(id: impl Into<SharedString>, label: impl Into<SharedString>, t: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px(px(9.))
        .py(px(3.))
        .rounded(px(6.))
        .border_1()
        .border_color(t.separator)
        .bg(t.bg)
        .text_size(px(12.))
        .text_color(t.text)
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(t.hover))
        .child(label.into())
}

fn primary(id: impl Into<SharedString>, label: impl Into<SharedString>, t: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px(px(10.))
        .py(px(4.))
        .rounded(px(6.))
        .bg(t.accent)
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(gpui::white())
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.opacity(0.85))
        .child(label.into())
}

fn chip(id: impl Into<SharedString>, label: impl Into<SharedString>, active: bool, t: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .px(px(8.))
        .py(px(2.))
        .rounded(px(10.))
        .text_size(px(11.5))
        .cursor(CursorStyle::PointingHand)
        .when(active, |d| d.bg(t.accent).text_color(gpui::white()))
        .when(!active, |d| d.bg(t.hover).text_color(t.text).hover(|s| s.bg(t.separator)))
        .child(label.into())
}

fn heading(label: impl Into<SharedString>, t: &Theme) -> gpui::Div {
    div()
        .mt(px(14.))
        .mb(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.section_header)
        .child(label.into().to_uppercase())
}

fn note(text: impl Into<SharedString>, t: &Theme) -> gpui::Div {
    div().text_size(px(12.)).text_color(t.secondary).child(text.into())
}

fn field_box(f: Entity<TextField>, t: &Theme) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .px(px(8.))
        .py(px(4.))
        .rounded(px(6.))
        .border_1()
        .border_color(t.separator)
        .bg(t.bg)
        .text_size(px(13.))
        .text_color(t.text)
        .overflow_hidden()
        .child(f)
}

pub(super) fn score_color(v: f32, t: &Theme) -> Hsla {
    let (light, dark) = if v >= 80. {
        (0x2f9e58, 0x46c27a)
    } else if v >= 60. {
        (0xd88a1c, 0xe8a23a)
    } else {
        (0xd9483b, 0xf06a5e)
    };
    gpui::rgb(if t.dark { dark } else { light }).into()
}

fn bar(v: f32, t: &Theme) -> gpui::Div {
    div()
        .flex_1()
        .h(px(6.))
        .rounded(px(3.))
        .bg(t.hover)
        .child(div().h_full().rounded(px(3.)).w(gpui::relative((v / 100.).clamp(0.02, 1.))).bg(score_color(v, t)))
}

fn severity_color(s: Severity, t: &Theme) -> Hsla {
    match s {
        Severity::Error => t.studio_mistake,
        Severity::Warning => gpui::rgb(if t.dark { 0xe8a23a } else { 0xd88a1c }).into(),
        Severity::Hint => t.studio_style,
    }
}

/// Very small Markdown renderer for coach answers: headings, bullets, paragraphs.
fn markdown_lite(text: &str, t: &Theme) -> Vec<AnyElement> {
    let mut out = Vec::new();
    let mut in_code = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        let plain = line.replace("**", "").replace('`', "");
        if in_code {
            out.push(div().font_family(crate::fonts::MONO).text_size(px(12.)).text_color(t.text).child(plain).into_any_element());
        } else if line.trim().is_empty() {
            out.push(div().h(px(6.)).into_any_element());
        } else if let Some(h) = plain.strip_prefix("### ").or_else(|| plain.strip_prefix("## ")).or_else(|| plain.strip_prefix("# ")) {
            out.push(div().mt(px(if i == 0 { 0. } else { 6. })).text_size(px(13.5)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(h.to_string()).into_any_element());
        } else if let Some(b) = plain.trim_start().strip_prefix("- ").or_else(|| plain.trim_start().strip_prefix("* ")) {
            let indent = (line.len() - line.trim_start().len()) as f32 * 4.;
            out.push(
                div()
                    .flex()
                    .gap(px(6.))
                    .pl(px(indent))
                    .text_size(px(13.))
                    .text_color(t.text)
                    .child(div().flex_none().text_color(t.secondary).child("•"))
                    .child(div().flex_1().min_w_0().child(b.to_string()))
                    .into_any_element(),
            );
        } else {
            out.push(div().text_size(px(13.)).text_color(t.text).child(plain).into_any_element());
        }
    }
    out
}

impl Workspace {
    // ---- state ------------------------------------------------------------------

    fn doc_key(&self, cx: &App) -> String {
        let e = self.editor().read(cx);
        match &e.path {
            Some(p) => p.display().to_string(),
            None => format!("untitled:{}", e.title()),
        }
    }

    pub(super) fn current_template(&mut self, cx: &App) -> Option<Template> {
        let editor = self.editor().clone();
        let id = editor.entity_id();
        let path = editor.read(cx).path.as_ref().map(|p| p.display().to_string());
        let tid = match &path {
            Some(p) => match self.studio.history.doc_templates.get(p) {
                Some(t) => Some(t.clone()),
                None => {
                    // A new document got its path: keep its template.
                    let t = self.studio.session_templates.get(&id).cloned();
                    if let Some(t) = &t {
                        self.studio.history.doc_templates.insert(p.clone(), t.clone());
                    }
                    t
                }
            },
            None => self.studio.session_templates.get(&id).cloned(),
        }?;
        self.studio.templates.iter().find(|t| t.id == tid).cloned()
    }

    fn set_template(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let editor = self.editor().clone();
        let path = editor.read(cx).path.as_ref().map(|p| p.display().to_string());
        match (&id, &path) {
            (Some(i), Some(p)) => {
                self.studio.history.doc_templates.insert(p.clone(), i.clone());
            }
            (None, Some(p)) => {
                self.studio.history.doc_templates.remove(p);
            }
            _ => {}
        }
        match id {
            Some(i) => self.studio.session_templates.insert(editor.entity_id(), i),
            None => self.studio.session_templates.remove(&editor.entity_id()),
        };
        self.studio.history.save();
        self.schedule_studio(Duration::from_millis(10), cx);
        cx.notify();
    }

    fn studio_options(&mut self, cx: &App) -> Options {
        let template = self.current_template(cx);
        let mut disabled: HashSet<String> = self.studio.history.disabled.iter().cloned().collect();
        if let Some(t) = &template {
            disabled.extend(t.disable.iter().cloned());
        }
        let misspelled = self.settings.spell.then(|| (*self.editor().read(cx).misspelled).clone());
        Options {
            register: template.as_ref().map(|t| t.register).unwrap_or_default(),
            misspelled,
            disabled,
            grade_band: template.and_then(|t| t.grade_band),
        }
    }

    pub(super) fn studio_active(&self) -> bool {
        self.settings.studio_open || self.settings.studio_live
    }

    pub(super) fn schedule_studio(&mut self, delay: Duration, cx: &mut Context<Self>) {
        if !self.studio_active() || self.docs.is_empty() {
            return;
        }
        let editor = self.editor().clone();
        let opts = self.studio_options(cx);
        self.studio.analyze_task = Some(cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(delay).await;
            let Ok((text, version)) = cx.update(|cx| {
                let e = editor.read(cx);
                (e.text().to_string(), e.buffer.version())
            }) else {
                return;
            };
            let t2 = text.clone();
            let report = cx.background_executor().spawn(async move { analysis::analyze(&t2, &opts) }).await;
            let _ = ws.update(cx, |ws, cx| ws.studio_done(editor, version, text, report, cx));
        }));
    }

    fn studio_done(&mut self, editor: Entity<Editor>, version: u64, text: String, report: Report, cx: &mut Context<Self>) {
        let marks: Vec<(std::ops::Range<usize>, bool)> = report.issues.iter().map(|i| (i.range.clone(), i.category().is_correctness())).collect();
        editor.update(cx, |e, cx| e.set_studio_marks(marks, version, cx));
        if &editor != self.editor() {
            return;
        }
        let same_doc = self.studio.report_for.is_some_and(|(id, _)| id == editor.entity_id());
        if !same_doc {
            self.studio.selected = None;
            self.studio.revealed.clear();
        }
        // Record the session (merged per document; throttled).
        if report.words >= 30 && self.studio.last_recorded.is_none_or(|t| t.elapsed() > Duration::from_secs(20)) {
            let template = self.current_template(cx).map(|t| t.id);
            let snap = Snapshot::from_report(history::now(), &self.doc_key(cx), template.as_deref(), &report);
            if self.studio.history.record(snap) {
                self.studio.last_recorded = Some(Instant::now());
                if self.studio.last_saved.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
                    self.studio.history.save();
                    self.studio.last_saved = Some(Instant::now());
                }
            }
        }
        self.studio.report = Some(report);
        self.studio.report_for = Some((editor.entity_id(), version));
        self.studio.report_text = text;
        cx.notify();
    }

    pub(super) fn clear_studio_marks(&mut self, cx: &mut Context<Self>) {
        for d in &self.docs {
            d.editor.update(cx, |e, cx| {
                let v = e.buffer.version();
                e.set_studio_marks(Vec::new(), v, cx)
            });
        }
    }

    pub(super) fn toggle_studio(&mut self, tab: Option<StudioTab>, cx: &mut Context<Self>) {
        // A tab always opens the panel; only the plain toggle closes it.
        match tab {
            Some(t) => {
                self.studio.tab = t;
                self.settings.studio_open = true;
            }
            None => self.settings.studio_open = !self.settings.studio_open,
        }
        self.settings.save();
        if self.studio_active() {
            self.studio.templates = templates::load_all();
            self.schedule_studio(Duration::from_millis(10), cx);
        } else {
            self.clear_studio_marks(cx);
        }
        cx.notify();
    }

    pub(super) fn save_studio(&mut self) {
        self.studio.history.save();
    }

    /// The report, if it still matches the active document.
    fn fresh_issues(&self, cx: &App) -> Option<&Report> {
        let r = self.studio.report.as_ref()?;
        let (id, v) = self.studio.report_for?;
        let e = self.editor();
        (id == e.entity_id() && v == e.read(cx).buffer.version()).then_some(r)
    }

    fn select_issue(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(issue) = self.studio.report.as_ref().and_then(|r| r.issues.get(ix)).cloned() else { return };
        self.studio.selected = Some(ix);
        let editor = self.editor().clone();
        editor.update(cx, |e, cx| e.select_range(issue.range.clone(), cx));
        window.focus(&editor.focus_handle(cx));
        cx.notify();
    }

    pub(super) fn next_issue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.studio_active() {
            self.toggle_studio(Some(StudioTab::Analyze), cx);
            return;
        }
        let head = self.editor().read(cx).buffer.selection().end;
        let Some(r) = self.fresh_issues(cx) else {
            self.schedule_studio(Duration::from_millis(10), cx);
            return;
        };
        if r.issues.is_empty() {
            self.show_toast("No issues found", cx);
            return;
        }
        let ix = r.issues.iter().position(|i| i.range.start >= head && Some(i.range.end) != Some(head)).unwrap_or(0);
        let msg = r.issues[ix].message.clone();
        self.select_issue(ix, window, cx);
        self.studio.tab = StudioTab::Analyze;
        if !self.settings.studio_open {
            self.show_toast(msg, cx);
        }
    }

    fn apply_fix(&mut self, ix: usize, suggestion: String, cx: &mut Context<Self>) {
        let Some(r) = self.fresh_issues(cx) else {
            self.show_toast("The text changed — analysing again", cx);
            self.schedule_studio(Duration::from_millis(10), cx);
            return;
        };
        let Some(issue) = r.issues.get(ix).cloned() else { return };
        let range = analysis::fix_range(self.editor().read(cx).text(), &issue, &suggestion);
        self.editor().update(cx, |e, cx| e.replace_range(range.clone(), &suggestion, cx));
        // Keep the remaining findings usable until the next analysis.
        if let Some(rep) = self.studio.report.as_mut() {
            crate::studio::shift_issues(&mut rep.issues, range, suggestion.len());
        }
        let v = self.editor().read(cx).buffer.version();
        if let Some((_, ver)) = self.studio.report_for.as_mut() {
            *ver = v;
        }
        self.studio.selected = None;
        self.studio.revealed.clear();
        cx.notify();
    }

    fn ignore_rule(&mut self, rule: &str, cx: &mut Context<Self>) {
        self.studio.history.disabled.insert(rule.to_string());
        self.studio.history.save();
        self.studio.selected = None;
        self.show_toast(format!("“{}” will no longer be checked (re-enable in Progress)", rules::info(rule).title), cx);
        self.schedule_studio(Duration::from_millis(10), cx);
    }

    // ---- coach --------------------------------------------------------------------

    fn save_api_key(&mut self, cx: &mut Context<Self>) {
        let key = self.studio.key_field.read(cx).text();
        match llm::save_key(&key) {
            Ok(()) => {
                self.studio.api_key = key.trim().to_string();
                self.studio.key_field.update(cx, |f, cx| f.set_text("", cx));
                self.show_toast(if key.trim().is_empty() { "API key removed" } else { "API key saved" }, cx);
            }
            Err(e) => self.show_toast(format!("Could not save the key: {e}"), cx),
        }
        cx.notify();
    }

    fn coach_request(&mut self, task: AiTask, detail: String, cx: &mut Context<Self>) -> assistant::Request {
        let selection = self.editor().read(cx).selected_text();
        let text = self.editor().read(cx).text().to_string();
        let is_selection = selection.split_whitespace().count() >= 3 && task != AiTask::Explain;
        let findings = match self.studio.report.as_ref() {
            Some(r) => assistant::findings_summary(&self.studio.report_text, r, 15),
            None => assistant::findings_summary(&text, &analysis::analyze(&text, &Options::default()), 15),
        };
        let focus_rules = if task == AiTask::Exercises { self.studio.history.recommend(3) } else { Vec::new() };
        assistant::Request {
            passage: if is_selection { selection } else { text },
            is_selection,
            level: assistant::LEVELS.get(self.settings.studio_level).copied().unwrap_or(assistant::LEVELS[1]).to_string(),
            profile: self.studio.history.profile_summary(),
            findings,
            template: self.current_template(cx),
            detail,
            focus_rules,
        }
    }

    fn coach_config(&self) -> llm::Config {
        let key = if self.studio.api_key.is_empty() { llm::load_key() } else { self.studio.api_key.clone() };
        let model = if self.settings.studio_backend == llm::Backend::Api { self.settings.studio_model.as_str() } else { "" };
        llm::Config::with_backend(self.settings.studio_backend, &key, model)
    }

    pub(super) fn run_coach(&mut self, task: AiTask, detail: String, cx: &mut Context<Self>) {
        let cfg = self.coach_config();
        self.settings.studio_open = true;
        self.studio.tab = StudioTab::Coach;
        self.studio.coach_title = task.label().to_string();
        if !cfg.available() {
            self.studio.coach_text = match cfg.backend {
                llm::Backend::ClaudeCode => "Claude Code was not found on this computer. Install it and sign in once in a terminal (`claude`), or switch the connection below to the Anthropic API. The Analyze, Templates and Progress tabs work without it.".into(),
                llm::Backend::Api => "Add an Anthropic API key below to use the coach (or set ANTHROPIC_API_KEY). The Analyze, Templates and Progress tabs work without it.".into(),
            };
            cx.notify();
            return;
        }
        let req = self.coach_request(task, detail, cx);
        if req.passage.trim().is_empty() && !matches!(task, AiTask::Brainstorm | AiTask::Ask) {
            self.studio.coach_text = "Write something first — or use Brainstorm to get started.".into();
            cx.notify();
            return;
        }
        let (system, user, max) = assistant::build(task, &req);
        self.studio.coach_busy = true;
        self.studio.coach_text.clear();
        self.studio.coach_task = Some(cx.spawn(async move |ws, cx| {
            let result = cx.background_executor().spawn(async move { llm::complete(&cfg, &system, &user, max) }).await;
            let _ = ws.update(cx, |ws, cx| {
                ws.studio.coach_busy = false;
                match (task, result) {
                    (AiTask::Exercises, Ok(json)) => {
                        let ex = exercises::parse_generated(&json);
                        if ex.is_empty() {
                            ws.studio.coach_text = "The coach did not return usable exercises. Try again.".into();
                        } else {
                            ws.studio.coach_text = format!("{} new exercises are waiting in Progress → Practice.", ex.len());
                            ws.studio.queue = ex;
                            ws.next_exercise(cx);
                            ws.studio.focus_answer = true;
                            ws.studio.tab = StudioTab::Progress;
                        }
                    }
                    (_, Ok(text)) => ws.studio.coach_text = text,
                    (_, Err(e)) => ws.studio.coach_text = format!("⚠ {e}"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn explain_issue(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(issue) = self.studio.report.as_ref().and_then(|r| r.issues.get(ix)).cloned() else { return };
        let quote = self.studio.report_text.get(issue.range.clone()).unwrap_or("").to_string();
        let sentence = crate::studio::text::prose(&self.studio.report_text)
            .sentences
            .iter()
            .find(|s| s.range.start <= issue.range.start && issue.range.end <= s.range.end)
            .map(|s| self.studio.report_text[s.range.clone()].to_string())
            .unwrap_or_default();
        let detail = format!("[{}] “{}” in the sentence “{}” — {}", issue.info().title, quote, sentence, issue.message);
        self.run_coach(AiTask::Explain, detail, cx);
    }

    // ---- practice -----------------------------------------------------------------

    fn next_exercise(&mut self, cx: &mut Context<Self>) {
        self.studio.grade = None;
        self.studio.exercise = if self.studio.queue.is_empty() { None } else { Some(self.studio.queue.remove(0)) };
        let text = self.studio.exercise.as_ref().map(|e| e.text.clone()).unwrap_or_default();
        self.studio.answer_field.update(cx, |f, cx| f.set_text(&text, cx));
        cx.notify();
    }

    fn practice_rules(&mut self, rules: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let mut q: Vec<Exercise> = Vec::new();
        // Own mistakes first: they are the most relevant material.
        if let Some(r) = self.studio.report.as_ref() {
            for i in &r.issues {
                if rules.iter().any(|x| x == i.rule)
                    && q.iter().filter(|e| e.source == exercises::Source::YourWriting).count() < 3
                    && let Some(ex) = exercises::from_issue(&self.studio.report_text, i, &r.issues)
                {
                    q.push(ex);
                }
            }
        }
        for r in &rules {
            let mut b = exercises::bank(r);
            // Rotate by the number of past attempts so repeats come later.
            let n = self.studio.history.practice(r).0;
            if !b.is_empty() {
                let k = n % b.len();
                b.rotate_left(k);
            }
            q.extend(b.into_iter().take(3));
        }
        if q.is_empty() {
            self.show_toast("No exercises for this yet — try “AI exercises”", cx);
            return;
        }
        self.studio.queue = q;
        self.studio.tab = StudioTab::Progress;
        self.settings.studio_open = true;
        self.next_exercise(cx);
        let fh = self.studio.answer_field.focus_handle(cx);
        window.focus(&fh);
    }

    fn practice_issue(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(issue) = self.studio.report.as_ref().and_then(|r| r.issues.get(ix)).cloned() else { return };
        let mut q = Vec::new();
        let all = self.studio.report.as_ref().map(|r| r.issues.clone()).unwrap_or_default();
        q.extend(exercises::from_issue(&self.studio.report_text, &issue, &all));
        q.extend(exercises::bank(issue.rule).into_iter().take(3));
        if q.is_empty() {
            return;
        }
        self.studio.queue = q;
        self.studio.tab = StudioTab::Progress;
        self.next_exercise(cx);
        let fh = self.studio.answer_field.focus_handle(cx);
        window.focus(&fh);
    }

    pub(super) fn practice_recommended(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut rules = self.studio.history.recommend(3);
        if rules.is_empty() {
            // No history yet: practise what this document shows.
            if let Some(r) = self.studio.report.as_ref() {
                for i in analysis::prioritized(&r.issues) {
                    if exercises::has_bank(i.rule) && !rules.iter().any(|x| x == i.rule) && rules.len() < 3 {
                        rules.push(i.rule.to_string());
                    }
                }
            }
        }
        if rules.is_empty() {
            rules = vec!["grammar.article".into(), "grammar.agreement".into(), "structure.comma-splice".into()];
        }
        self.practice_rules(rules, window, cx);
    }

    fn check_answer(&mut self, cx: &mut Context<Self>) {
        let Some(ex) = self.studio.exercise.clone() else { return };
        if self.studio.grade.as_ref().is_some_and(|g| g.correct) {
            self.next_exercise(cx);
            return;
        }
        let answer = self.studio.answer_field.read(cx).text();
        let g = exercises::grade(&ex, &answer);
        // Only the first attempt counts toward the profile.
        if self.studio.grade.is_none() {
            self.studio.history.log_exercise(history::now(), &ex.rule, g.correct);
            self.studio.history.save();
        }
        self.studio.grade = Some(g);
        cx.notify();
    }

    // ---- templates ------------------------------------------------------------------

    fn new_from_template(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.studio.templates.get(ix).cloned() else { return };
        let mut body = String::new();
        if !t.prompt.is_empty() {
            body.push_str(&format!("// Task: {}\n\n", t.prompt));
        }
        body.push_str(&t.skeleton);
        self.add_doc(&body, None, window, cx);
        self.set_template(Some(t.id.clone()), cx);
        // Put the caret on the first empty line after the first guidance comment.
        let caret = body.find("\n\n\n").map(|i| i + 1).or_else(|| body.find("//").and_then(|i| body[i..].find('\n').map(|j| i + j + 1))).unwrap_or(0);
        self.editor().update(cx, |e, cx| e.select_range(caret..caret, cx));
        if let Some(m) = t.minutes {
            self.start_timer(m, cx);
        }
    }

    fn customize_template(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = self.studio.templates.get(ix).cloned() else { return };
        match templates::customize(&t) {
            Ok(p) => {
                self.open_path(&p, false, window, cx);
                self.show_toast("Edit this file to customize the template; changes apply when you reopen the Studio", cx);
            }
            Err(e) => self.show_toast(format!("Could not write the template: {e}"), cx),
        }
    }

    fn start_timer(&mut self, minutes: u32, cx: &mut Context<Self>) {
        self.studio.timer = Some((Instant::now(), minutes));
        self.studio.timer_task = Some(cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let running = ws.update(cx, |ws, cx| {
                    let Some((start, m)) = ws.studio.timer else { return false };
                    if start.elapsed() >= Duration::from_secs(m as u64 * 60) {
                        ws.studio.timer = None;
                        ws.show_toast("Time is up — finish your sentence and review", cx);
                        return false;
                    }
                    cx.notify();
                    true
                });
                if !matches!(running, Ok(true)) {
                    break;
                }
            }
        }));
        cx.notify();
    }

    pub(super) fn timer_label(&self) -> Option<String> {
        let (start, m) = self.studio.timer?;
        let left = (m as u64 * 60).saturating_sub(start.elapsed().as_secs());
        Some(format!("{}:{:02}", left / 60, left % 60))
    }

    // ---- context menu -------------------------------------------------------------

    pub(super) fn studio_context_items(&self, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let Some(r) = self.fresh_issues(cx) else { return Vec::new() };
        let head = self.editor().read(cx).buffer.head();
        let Some(ix) = r.issues.iter().position(|i| i.range.start <= head && head <= i.range.end) else { return Vec::new() };
        let issue = &r.issues[ix];
        let weak = cx.entity().downgrade();
        let mut items = vec![menus::disabled(issue.info().title)];
        for s in issue.suggestions.iter().take(4) {
            let w = weak.clone();
            let s2 = s.clone();
            let label = if s.is_empty() { "Remove it".to_string() } else { format!("Change to “{s}”") };
            items.push(call(&label, move |_, cx| {
                let _ = w.update(cx, |ws, cx| ws.apply_fix(ix, s2.clone(), cx));
            }));
        }
        let w = weak.clone();
        items.push(call("Why? Show the Lesson", move |window, cx| {
            let _ = w.update(cx, |ws, cx| {
                ws.settings.studio_open = true;
                ws.studio.tab = StudioTab::Analyze;
                ws.select_issue(ix, window, cx);
            });
        }));
        let w = weak.clone();
        items.push(call("Explain with Coach", move |_, cx| {
            let _ = w.update(cx, |ws, cx| ws.explain_issue(ix, cx));
        }));
        let w = weak.clone();
        items.push(call("Practise This", move |window, cx| {
            let _ = w.update(cx, |ws, cx| ws.practice_issue(ix, window, cx));
        }));
        let w = weak;
        let rule = issue.rule;
        items.push(call("Stop Checking This Rule", move |_, cx| {
            let _ = w.update(cx, |ws, cx| ws.ignore_rule(rule, cx));
        }));
        items.push(sep());
        items
    }

    pub(super) fn studio_menu(&self, cx: &mut Context<Self>) -> MenuItem {
        let s = &self.settings;
        let weak = cx.entity().downgrade();
        let level_items: Vec<MenuItem> = assistant::LEVELS
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let w = weak.clone();
                let mut item = call(l, move |_, cx| {
                    let _ = w.update(cx, |ws, cx| ws.update_settings(cx, |s| s.studio_level = i));
                });
                if let MenuItem::Item { checked, .. } = &mut item {
                    *checked = s.studio_level == i;
                }
                item
            })
            .collect();
        sub("Studio", vec![
            check("Show Writing Studio", ToggleStudio, s.studio_open),
            sep(),
            act("Analysis", StudioAnalyze),
            act("Coach", StudioCoach),
            act("Templates", StudioTemplates),
            act("Progress", StudioProgress),
            sep(),
            act("Next Issue", StudioNextIssue),
            act("Hints from Coach", StudioHints),
            act("Practise Weak Spots", StudioPractice),
            sep(),
            check("Underline While Typing", ToggleStudioLive, s.studio_live),
            check("Show Fixes Immediately", ToggleStudioReveal, s.studio_reveal),
            sub("My Level", level_items),
        ])
    }

    // ---- rendering ----------------------------------------------------------------

    pub(super) fn render_studio(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.global::<Theme>();
        if std::mem::take(&mut self.studio.focus_answer) && self.studio.exercise.is_some() {
            let fh = self.studio.answer_field.focus_handle(cx);
            window.focus(&fh);
        }
        let tab = self.studio.tab;
        let tabs = [(StudioTab::Analyze, "Analyze"), (StudioTab::Coach, "Coach"), (StudioTab::Templates, "Templates"), (StudioTab::Progress, "Progress")];
        let header = div()
            .flex_none()
            .h(px(TOOLBAR_H))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(4.))
            .bg(t.toolbar_bg)
            .border_b_1()
            .border_color(t.toolbar_border)
            .children(tabs.into_iter().map(|(k, label)| {
                chip(SharedString::from(format!("studio-tab-{label}")), label, tab == k, &t).on_click(cx.listener(move |ws, _, _, cx| {
                    ws.studio.tab = k;
                    if k == StudioTab::Templates {
                        ws.studio.templates = templates::load_all();
                    }
                    cx.notify();
                }))
            }))
            .child(div().flex_1())
            .when_some(self.timer_label(), |d, l| d.child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.accent).child(l)))
            .child(
                div()
                    .id("studio-close")
                    .size(px(22.))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.secondary)
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(t.hover))
                    .child("×")
                    .on_click(cx.listener(|ws, _, _, cx| ws.toggle_studio(None, cx))),
            );
        let body = match tab {
            StudioTab::Analyze => self.render_analyze(cx),
            StudioTab::Coach => self.render_coach(cx),
            StudioTab::Templates => self.render_templates(cx),
            StudioTab::Progress => self.render_progress(window, cx),
        };
        div()
            .flex_none()
            .w(px(PANEL_W))
            .h_full()
            .flex()
            .flex_col()
            .bg(t.list_bg)
            .font_family(crate::fonts::ui())
            .rounded_br(self.corner)
            .rounded_tr(self.corner)
            .child(header)
            .child(div().id("studio-body").flex_1().min_h_0().overflow_y_scroll().px(px(14.)).pb(px(24.)).child(body))
            .into_any_element()
    }

    fn render_analyze(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.global::<Theme>();
        let template = self.current_template(cx);
        let Some(r) = self.studio.report.as_ref() else {
            return div().pt(px(16.)).child(note("Analysing…  Start writing in English and the Studio will check grammar, spelling, vocabulary, structure, clarity and tone.", &t)).into_any_element();
        };
        let mut col = div().flex().flex_col();
        if r.words == 0 {
            return col.pt(px(16.)).child(note("Nothing to analyse yet. Write a few sentences — or pick a template.", &t)).into_any_element();
        }
        // Overall
        let overall = r.overall();
        col = col.child(
            div()
                .pt(px(14.))
                .flex()
                .items_end()
                .gap(px(10.))
                .child(div().text_size(px(34.)).font_weight(FontWeight::BOLD).text_color(score_color(overall, &t)).child(format!("{overall:.0}")))
                .child(
                    div()
                        .pb(px(6.))
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(12.)).text_color(t.secondary).child("Overall score"))
                        .child(div().text_size(px(12.)).text_color(t.text).child(format!(
                            "{} words · {} issues{}",
                            r.words,
                            r.issues.len(),
                            template.as_ref().map(|t| format!(" · {}", t.name)).unwrap_or_default()
                        ))),
                ),
        );
        col = col.child(heading("Scores", &t));
        for (k, v) in &r.scores {
            col = col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .py(px(2.))
                    .child(div().w(px(84.)).text_size(px(12.)).text_color(t.text).child(k.clone()))
                    .child(bar(*v, &t))
                    .child(div().w(px(26.)).text_size(px(12.)).text_color(t.secondary).child(format!("{v:.0}"))),
            );
        }
        // Readability
        let rd = &r.readability;
        col = col.child(heading("Readability", &t)).child(
            div().text_size(px(12.5)).text_color(t.text).child(format!("Grade {:.1} · {} · {}", rd.grade(), rd.ease_label(), rd.audience())),
        );
        let metric = |label: &str, v: f32| div().w(px(105.)).text_size(px(11.5)).text_color(t.secondary).child(format!("{label} {v:.1}"));
        col = col.child(
            div()
                .flex()
                .flex_wrap()
                .mt(px(4.))
                .child(metric("Flesch", rd.flesch))
                .child(metric("F–K", rd.fk_grade))
                .child(metric("Fog", rd.gunning_fog))
                .child(metric("SMOG", rd.smog))
                .child(metric("Coleman–Liau", rd.coleman_liau))
                .child(metric("ARI", rd.ari)),
        );
        // Sentence rhythm
        let st = &r.structure;
        col = col.child(heading("Sentence rhythm", &t)).child(note(
            format!("{} sentences, average {:.1} words (σ {:.1}), longest {}. {:.0}% short, {:.0}% long.", st.sentences, st.avg_len, st.sd_len, st.longest, st.short_share * 100., st.long_share * 100.),
            &t,
        ));
        let maxlen = st.lengths.iter().copied().max().unwrap_or(1).max(30) as f32;
        let shown: Vec<usize> = st.lengths.iter().rev().take(60).rev().copied().collect();
        col = col.child(div().mt(px(6.)).h(px(40.)).flex().items_end().gap(px(1.)).children(shown.into_iter().map(|l| {
            let c = if l > 30 { t.studio_mistake } else if l < 8 { t.tertiary } else { t.accent };
            div().flex_1().max_w(px(8.)).h(px(40. * l as f32 / maxlen)).rounded_t(px(1.)).bg(c)
        })));
        // Tone & vocabulary
        let tone = &r.tone;
        col = col.child(heading("Tone", &t)).child(note(
            format!(
                "{} register (formality {:.0}/100) · {} · {}{}",
                tone.register.label(),
                tone.formality,
                tone.confidence,
                tone.sentiment,
                template.as_ref().map(|t| format!(" · target {}", t.register.label())).unwrap_or_default()
            ),
            &t,
        ));
        let v = &r.vocab;
        col = col.child(heading("Vocabulary", &t)).child(note(
            format!(
                "Lexical diversity (MTLD) {:.0} · academic words {:.0}% · long words {:.0}% · {} kind{} of linking words",
                v.mtld,
                v.academic_ratio * 100.,
                v.long_word_ratio * 100.,
                v.linker_functions,
                if v.linker_functions == 1 { "" } else { "s" }
            ),
            &t,
        ));
        if !v.top_words.is_empty() {
            col = col.child(note(format!("Most repeated: {}", v.top_words.iter().take(6).map(|(w, n)| format!("{w} ×{n}")).collect::<Vec<_>>().join(", ")), &t));
        }
        // Template requirements
        if let Some(tp) = &template {
            let reqs = templates::requirements(tp, &self.studio.report_text, r);
            col = col.child(heading(format!("{} checklist", tp.name), &t));
            for q in reqs {
                col = col.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .py(px(1.))
                        .text_size(px(12.))
                        .child(div().w(px(14.)).text_color(if q.ok { score_color(100., &t) } else { score_color(0., &t) }).child(if q.ok { "✓" } else { "✗" }))
                        .child(div().text_color(t.text).child(q.label))
                        .child(div().flex_1().text_color(t.secondary).child(q.detail)),
                );
            }
        }
        // Issues
        let counts: Vec<(Category, usize)> = Category::ALL.iter().map(|c| (*c, r.count(*c))).filter(|(_, n)| *n > 0).collect();
        col = col.child(heading("Issues", &t));
        let filter = self.studio.filter;
        let mut chips = div().flex().flex_wrap().gap(px(4.)).child(chip("studio-filter-all", format!("All {}", r.issues.len()), filter.is_none(), &t).on_click(cx.listener(|ws, _, _, cx| {
            ws.studio.filter = None;
            cx.notify();
        })));
        for (c, n) in counts {
            chips = chips.child(chip(SharedString::from(format!("studio-filter-{c:?}")), format!("{} {n}", c.label()), filter == Some(c), &t).on_click(cx.listener(move |ws, _, _, cx| {
                ws.studio.filter = if ws.studio.filter == Some(c) { None } else { Some(c) };
                cx.notify();
            })));
        }
        col = col.child(chips);
        let mut order: Vec<usize> = (0..r.issues.len()).filter(|&i| filter.is_none_or(|c| r.issues[i].category() == c)).collect();
        order.sort_by(|&a, &b| r.issues[b].severity.cmp(&r.issues[a].severity).then(r.issues[a].range.start.cmp(&r.issues[b].range.start)));
        if order.is_empty() {
            col = col.child(div().mt(px(8.)).child(note("No issues here. 🎉", &t)));
        }
        let more = order.len().saturating_sub(MAX_LISTED);
        let rows: Vec<AnyElement> = order.into_iter().take(MAX_LISTED).map(|ix| self.render_issue(ix, cx)).collect();
        col = col.children(rows);
        if more > 0 {
            col = col.child(note(format!("…and {more} more. Fix the mistakes above first."), &t));
        }
        col.into_any_element()
    }

    fn render_issue(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.global::<Theme>();
        let r = self.studio.report.as_ref().unwrap();
        let issue: &Issue = &r.issues[ix];
        let info = issue.info();
        let selected = self.studio.selected == Some(ix);
        let quote: String = self.studio.report_text.get(issue.range.clone()).unwrap_or("").chars().take(48).collect();
        let mut row = div()
            .id(SharedString::from(format!("studio-issue-{ix}")))
            .mt(px(6.))
            .p(px(8.))
            .rounded(px(7.))
            .border_1()
            .border_color(if selected { t.accent } else { t.separator })
            .bg(t.bg)
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |ws, _, window, cx| {
                if ws.studio.selected == Some(ix) {
                    ws.studio.selected = None;
                    cx.notify();
                } else {
                    ws.select_issue(ix, window, cx);
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().size(px(7.)).rounded(px(4.)).bg(severity_color(issue.severity, &t)))
                    .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(info.title))
                    .child(div().flex_1())
                    .child(div().text_size(px(10.5)).text_color(t.secondary).child(analysis::severity_label(issue.severity))),
            )
            .child(div().mt(px(3.)).text_size(px(12.)).text_color(t.secondary).child(if issue.message.contains(&format!("“{quote}")) { issue.message.clone() } else { format!("“{quote}” — {}", issue.message) }));
        if selected {
            let reveal = self.settings.studio_reveal || self.studio.revealed.contains(&ix);
            let personal = self.studio.history.note(issue.rule);
            row = row
                .child(div().mt(px(8.)).text_size(px(12.5)).text_color(t.text).child(info.lesson))
                .child(
                    div()
                        .mt(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .child(div().text_color(t.studio_mistake).child(format!("✗ {}", info.wrong)))
                        .child(div().text_color(score_color(100., &t)).child(format!("✓ {}", info.right))),
                )
                .when_some(personal, |d, n| d.child(div().mt(px(6.)).p(px(6.)).rounded(px(5.)).bg(t.hover).text_size(px(12.)).text_color(t.text).child(format!("📈 {n}"))));
            let mut actions = div().mt(px(8.)).flex().flex_wrap().gap(px(5.));
            if !issue.suggestions.is_empty() {
                if reveal {
                    for (k, s) in issue.suggestions.iter().take(4).enumerate() {
                        let s2 = s.clone();
                        let label = if s.is_empty() { "Remove".to_string() } else { format!("→ {s}") };
                        actions = actions.child(primary(SharedString::from(format!("studio-fix-{ix}-{k}")), label, &t).on_click(cx.listener(move |ws, _, _, cx| ws.apply_fix(ix, s2.clone(), cx))));
                    }
                } else {
                    actions = actions.child(button(SharedString::from(format!("studio-reveal-{ix}")), "Try it yourself, then show fix", &t).on_click(cx.listener(move |ws, _, _, cx| {
                        ws.studio.revealed.insert(ix);
                        cx.notify();
                    })));
                }
            }
            actions = actions
                .child(button(SharedString::from(format!("studio-explain-{ix}")), "Explain", &t).on_click(cx.listener(move |ws, _, _, cx| ws.explain_issue(ix, cx))))
                .when(exercises::has_bank(issue.rule) || info.category.is_correctness(), |d| {
                    d.child(button(SharedString::from(format!("studio-practice-{ix}")), "Practise", &t).on_click(cx.listener(move |ws, _, window, cx| ws.practice_issue(ix, window, cx))))
                })
                .child({
                    let rule = issue.rule;
                    button(SharedString::from(format!("studio-ignore-{ix}")), "Ignore rule", &t).on_click(cx.listener(move |ws, _, _, cx| ws.ignore_rule(rule, cx)))
                });
            row = row.child(actions);
        }
        row.into_any_element()
    }

    fn render_coach(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.global::<Theme>();
        let cfg = self.coach_config();
        let selection = self.editor().read(cx).selected_text();
        let sel_words = selection.split_whitespace().count();
        let template = self.current_template(cx);
        let mut col = div().flex().flex_col();
        col = col.child(heading("Your level", &t)).child(div().flex().flex_wrap().gap(px(4.)).children(assistant::LEVELS.iter().enumerate().map(|(i, l)| {
            chip(SharedString::from(format!("studio-level-{i}")), *l, self.settings.studio_level == i, &t).on_click(cx.listener(move |ws, _, _, cx| ws.update_settings(cx, |s| s.studio_level = i)))
        })));
        col = col.child(heading("Coach", &t)).child(note(
            if sel_words >= 3 { format!("Working on your selection ({sel_words} words).") } else { "Working on the whole document. Select a passage to focus.".to_string() },
            &t,
        ));
        let task_button = |id: &str, label: &str, task: AiTask, cx: &mut Context<Self>| {
            button(SharedString::from(id.to_string()), label.to_string(), &t).on_click(cx.listener(move |ws, _, _, cx| ws.run_coach(task, String::new(), cx)))
        };
        let mut grid = div().mt(px(6.)).flex().flex_wrap().gap(px(5.));
        grid = grid
            .child(task_button("studio-ai-hints", "💡 Hints", AiTask::Hints, cx))
            .child(task_button("studio-ai-critique", "⚖ Critique argument", AiTask::Critique, cx))
            .child(task_button("studio-ai-brainstorm", "🧠 Brainstorm", AiTask::Brainstorm, cx))
            .child(task_button("studio-ai-exercises", "✍ AI exercises", AiTask::Exercises, cx));
        if template.as_ref().is_some_and(|t| !t.rubric.is_empty()) {
            grid = grid.child(task_button("studio-ai-rubric", "📋 Rubric feedback", AiTask::Rubric, cx));
        }
        col = col.child(grid);
        col = col.child(heading("Rewrite — after you try", &t)).child(div().flex().flex_wrap().gap(px(4.)).children(assistant::REWRITE_GOALS.iter().enumerate().map(|(i, g)| {
            chip(SharedString::from(format!("studio-goal-{i}")), *g, self.studio.goal == i, &t).on_click(cx.listener(move |ws, _, _, cx| {
                ws.studio.goal = i;
                cx.notify();
            }))
        })));
        col = col.child(div().mt(px(6.)).flex().gap(px(6.)).items_center().child(button("studio-ai-rewrite", "Rewrite with explanations", &t).on_click(cx.listener(|ws, _, _, cx| {
            let goal = assistant::REWRITE_GOALS[ws.studio.goal].to_string();
            ws.run_coach(AiTask::Rewrite, goal, cx)
        }))));
        col = col.child(heading("Ask", &t)).child(
            div().flex().gap(px(6.)).items_center().child(field_box(self.studio.ask_field.clone(), &t)).child(primary("studio-ask", "Ask", &t).on_click(cx.listener(|ws, _, _, cx| {
                let q = ws.studio.ask_field.read(cx).text();
                if !q.trim().is_empty() {
                    ws.run_coach(AiTask::Ask, q, cx);
                }
            }))),
        );
        // Output
        if self.studio.coach_busy || !self.studio.coach_text.is_empty() {
            let mut out = div().mt(px(14.)).p(px(10.)).rounded(px(8.)).border_1().border_color(t.separator).bg(t.bg).flex().flex_col().gap(px(2.));
            out = out.child(
                div()
                    .flex()
                    .items_center()
                    .child(div().flex_1().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(t.section_header).child(self.studio.coach_title.to_uppercase()))
                    .when(!self.studio.coach_busy && !self.studio.coach_text.is_empty(), |d| {
                        d.child(button("studio-copy", "Copy", &t).on_click(cx.listener(|ws, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(ws.studio.coach_text.clone()));
                            ws.show_toast("Copied", cx);
                        })))
                    }),
            );
            if self.studio.coach_busy {
                out = out.child(note("Thinking… (this can take up to a minute)", &t)).child(button("studio-cancel", "Cancel", &t).on_click(cx.listener(|ws, _, _, cx| {
                    ws.studio.coach_task = None;
                    ws.studio.coach_busy = false;
                    ws.studio.coach_text = "Cancelled.".into();
                    cx.notify();
                })));
            } else {
                out = out.children(markdown_lite(&self.studio.coach_text, &t));
                if self.studio.coach_title == AiTask::Rewrite.label() {
                    out = out.child(div().mt(px(6.)).child(note("Compare it with your own attempt: which changes would you have made? Apply them yourself — typing the fix is how it sticks.", &t)));
                }
            }
            col = col.child(out);
        }
        // Connection
        col = col.child(heading("Connection", &t)).child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.))
                .child(chip("studio-backend-cli", "Claude Code (Haiku · high)", cfg.backend == llm::Backend::ClaudeCode, &t).on_click(cx.listener(|ws, _, _, cx| ws.update_settings(cx, |s| s.studio_backend = llm::Backend::ClaudeCode))))
                .child(chip("studio-backend-api", "Anthropic API", cfg.backend == llm::Backend::Api, &t).on_click(cx.listener(|ws, _, _, cx| ws.update_settings(cx, |s| s.studio_backend = llm::Backend::Api)))),
        );
        let status = match (cfg.backend, cfg.available()) {
            (llm::Backend::ClaudeCode, true) => format!(
                "Using {} through your Claude Code sign-in ({}). No API key needed. Your text is sent only when you press a coach button.",
                cfg.describe(),
                cfg.cli.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
            ),
            (llm::Backend::ClaudeCode, false) => "Claude Code was not found. Install it (npm install -g @anthropic-ai/claude-code), run `claude` once to sign in, or use the API.".to_string(),
            (llm::Backend::Api, true) => format!("Connected · {}. Your text is sent to Anthropic only when you press a coach button.", cfg.describe()),
            (llm::Backend::Api, false) => "Paste an Anthropic API key (stored only on this computer) or set ANTHROPIC_API_KEY.".to_string(),
        };
        col = col.child(div().mt(px(6.)).child(note(status, &t)));
        if cfg.backend == llm::Backend::Api {
            let has_key = cfg.available();
            col = col.child(
                div()
                    .mt(px(6.))
                    .flex()
                    .gap(px(6.))
                    .items_center()
                    .child(field_box(self.studio.key_field.clone(), &t))
                    .child(button("studio-key-save", if has_key { "Replace key" } else { "Save key" }, &t).on_click(cx.listener(|ws, _, _, cx| ws.save_api_key(cx)))),
            );
        }
        col.into_any_element()
    }

    fn render_templates(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.global::<Theme>();
        let current = self.current_template(cx);
        let mut col = div().flex().flex_col();
        if let Some(tp) = &current {
            col = col.child(heading("This document", &t)).child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(tp.name.clone()));
            if !tp.prompt.is_empty() {
                col = col.child(div().mt(px(4.)).text_size(px(12.5)).italic().text_color(t.text).child(tp.prompt.clone()));
            }
            let mut row = div().mt(px(6.)).flex().flex_wrap().gap(px(5.));
            if let Some(m) = tp.minutes {
                row = row.child(if self.studio.timer.is_some() {
                    button("studio-timer-stop", "Stop timer", &t).on_click(cx.listener(|ws, _, _, cx| {
                        ws.studio.timer = None;
                        ws.studio.timer_task = None;
                        cx.notify();
                    }))
                } else {
                    button("studio-timer", format!("⏱ Start {m}-minute timer"), &t).on_click(cx.listener(move |ws, _, _, cx| ws.start_timer(m, cx)))
                });
            }
            row = row.child(button("studio-template-none", "Detach template", &t).on_click(cx.listener(|ws, _, _, cx| ws.set_template(None, cx))));
            col = col.child(row);
            if let Some(r) = self.studio.report.as_ref() {
                for q in templates::requirements(tp, &self.studio.report_text, r) {
                    col = col.child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .py(px(1.))
                            .text_size(px(12.))
                            .child(div().w(px(14.)).text_color(if q.ok { score_color(100., &t) } else { score_color(0., &t) }).child(if q.ok { "✓" } else { "✗" }))
                            .child(div().text_color(t.text).child(q.label))
                            .child(div().flex_1().text_color(t.secondary).child(q.detail)),
                    );
                }
            }
            if !tp.rubric.is_empty() {
                col = col.child(heading("Rubric", &t));
                for x in &tp.rubric {
                    col = col.child(div().py(px(1.)).text_size(px(12.)).text_color(t.text).child(format!("• {x}")));
                }
            }
            if !tp.tips.is_empty() {
                col = col.child(heading("Tips", &t));
                for x in &tp.tips {
                    col = col.child(div().py(px(1.)).text_size(px(12.)).text_color(t.secondary).child(format!("• {x}")));
                }
            }
        }
        col = col.child(heading("Templates", &t)).child(note("Every template is a Markdown file. Customize one to change its constraints, rubric, tips or skeleton.", &t));
        let mut cats: Vec<String> = Vec::new();
        for tp in &self.studio.templates {
            if !cats.contains(&tp.category) {
                cats.push(tp.category.clone());
            }
        }
        for cat in cats {
            col = col.child(div().mt(px(10.)).text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(if cat.is_empty() { "Custom".to_string() } else { cat.clone() }));
            for (ix, tp) in self.studio.templates.iter().enumerate().filter(|(_, x)| x.category == cat) {
                let mut limits = Vec::new();
                match (tp.min_words, tp.max_words) {
                    (Some(a), Some(b)) => limits.push(format!("{a}–{b} words")),
                    (Some(a), None) => limits.push(format!("{a}+ words")),
                    _ => {}
                }
                if let Some(m) = tp.minutes {
                    limits.push(format!("{m} min"));
                }
                limits.push(tp.register.label().to_string());
                let active = current.as_ref().is_some_and(|c| c.id == tp.id);
                col = col.child(
                    div()
                        .mt(px(6.))
                        .p(px(8.))
                        .rounded(px(7.))
                        .border_1()
                        .border_color(if active { t.accent } else { t.separator })
                        .bg(t.bg)
                        .child(div().flex().child(div().flex_1().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(tp.name.clone())).when(!tp.builtin, |d| d.child(note("custom", &t))))
                        .child(div().mt(px(2.)).text_size(px(12.)).text_color(t.secondary).child(tp.description.clone()))
                        .child(div().mt(px(2.)).text_size(px(11.)).text_color(t.tertiary).child(limits.join(" · ")))
                        .child(
                            div()
                                .mt(px(6.))
                                .flex()
                                .flex_wrap()
                                .gap(px(5.))
                                .child(primary(SharedString::from(format!("studio-tpl-new-{ix}")), "New document", &t).on_click(cx.listener(move |ws, _, window, cx| ws.new_from_template(ix, window, cx))))
                                .child(button(SharedString::from(format!("studio-tpl-use-{ix}")), "Use for this document", &t).on_click(cx.listener(move |ws, _, _, cx| {
                                    let id = ws.studio.templates.get(ix).map(|t| t.id.clone());
                                    ws.set_template(id, cx)
                                })))
                                .child(button(SharedString::from(format!("studio-tpl-edit-{ix}")), "Customize", &t).on_click(cx.listener(move |ws, _, window, cx| ws.customize_template(ix, window, cx)))),
                        ),
                );
            }
        }
        col = col.child(div().mt(px(10.)).child(button("studio-tpl-folder", "Open templates folder", &t).on_click(cx.listener(|_, _, _, cx| {
            if let Some(d) = templates::dir() {
                let _ = std::fs::create_dir_all(&d);
                cx.open_with_system(&d);
            }
        }))));
        col.into_any_element()
    }

    fn render_progress(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = *cx.global::<Theme>();
        let h = &self.studio.history;
        let mut col = div().flex().flex_col();
        // Practice first: it is the action this tab exists for.
        col = col.child(heading("Practice", &t));
        if let Some(ex) = self.studio.exercise.clone() {
            let src = match ex.source {
                exercises::Source::Bank => "Exercise",
                exercises::Source::YourWriting => "From your writing",
                exercises::Source::Assistant => "From your coach",
            };
            let mut card = div()
                .p(px(10.))
                .rounded(px(8.))
                .border_1()
                .border_color(t.separator)
                .bg(t.bg)
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().flex().child(div().flex_1().text_size(px(11.)).text_color(t.section_header).child(format!("{} · {}", src.to_uppercase(), rules::info(&ex.rule).title))).child(note(format!("{} left", self.studio.queue.len()), &t)))
                .child(div().text_size(px(12.5)).text_color(t.text).child(ex.instruction.clone()))
                .child(div().p(px(6.)).rounded(px(5.)).bg(t.hover).text_size(px(13.)).text_color(t.text).child(ex.text.clone()))
                .child(field_box(self.studio.answer_field.clone(), &t));
            let graded = self.studio.grade.clone();
            card = card.child(
                div()
                    .flex()
                    .gap(px(5.))
                    .child(primary("studio-check", if graded.as_ref().is_some_and(|g| g.correct) { "Next" } else { "Check" }, &t).on_click(cx.listener(|ws, _, _, cx| ws.check_answer(cx))))
                    .child(button("studio-skip", "Skip", &t).on_click(cx.listener(|ws, _, _, cx| ws.next_exercise(cx)))),
            );
            if let Some(g) = graded {
                card = card.child(div().text_size(px(12.5)).font_weight(FontWeight::SEMIBOLD).text_color(score_color(if g.correct { 100. } else { 0. }, &t)).child(g.feedback.clone()));
                if let Some(m) = g.model.filter(|_| !g.correct) {
                    card = card.child(note(format!("Model answer: {m}"), &t));
                }
                if !ex.explanation.is_empty() {
                    card = card.child(div().text_size(px(12.)).text_color(t.secondary).child(ex.explanation.clone()));
                }
            }
            col = col.child(card);
        } else {
            col = col.child(note("Practise the mistakes you make most — exercises come from your own sentences first.", &t));
        }
        col = col.child(
            div()
                .mt(px(6.))
                .flex()
                .flex_wrap()
                .gap(px(5.))
                .child(primary("studio-practice", "Practise my weak spots", &t).on_click(cx.listener(|ws, _, window, cx| ws.practice_recommended(window, cx))))
                .child(button("studio-ai-ex", "AI exercises", &t).on_click(cx.listener(|ws, _, _, cx| ws.run_coach(AiTask::Exercises, String::new(), cx)))),
        );
        // Overview
        col = col.child(heading("Overview", &t));
        if h.snapshots.is_empty() {
            col = col.child(note("Your progress appears here after you write (30+ words). Each writing session is recorded on this computer only.", &t));
            return col.into_any_element();
        }
        let stat = |n: String, label: &str| div().flex_1().flex().flex_col().child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(t.text).child(n)).child(div().text_size(px(11.)).text_color(t.secondary).child(label.to_string()));
        col = col.child(
            div()
                .flex()
                .child(stat(h.snapshots.len().to_string(), "sessions"))
                .child(stat(stats::group(h.total_words()), "words"))
                .child(stat(format!("{}d", h.streak(history::now())), "streak"))
                .child(stat(h.accuracy().map(|a| format!("{:.0}%", a * 100.)).unwrap_or("–".into()), "practice")),
        );
        let series = h.overall_series(30);
        if series.len() >= 2 {
            col = col.child(div().mt(px(8.)).text_size(px(11.)).text_color(t.secondary).child("Overall score per session")).child(div().mt(px(4.)).h(px(44.)).flex().items_end().gap(px(2.)).children(series.iter().map(|v| {
                div().flex_1().max_w(px(10.)).h(px(44. * (v / 100.).clamp(0.05, 1.))).rounded_t(px(2.)).bg(score_color(*v, &t))
            })));
        }
        let progress = h.progress();
        if !progress.is_empty() {
            col = col.child(heading("Progress (earlier → recent sessions)", &t));
            for p in progress {
                let d = p.recent - p.before;
                let arrow = if d > 2. { "▲" } else if d < -2. { "▼" } else { "■" };
                let c = if d > 2. { score_color(100., &t) } else if d < -2. { score_color(0., &t) } else { t.secondary };
                col = col.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .py(px(1.))
                        .text_size(px(12.))
                        .child(div().w(px(90.)).text_color(t.text).child(p.key.clone()))
                        .child(div().flex_1().text_color(t.secondary).child(format!("{:.0} → {:.0}", p.before, p.recent)))
                        .child(div().text_color(c).child(format!("{arrow} {d:+.0}"))),
                );
            }
        }
        let weak = h.weaknesses();
        if !weak.is_empty() {
            col = col.child(heading("Weakest areas", &t));
            for (k, v) in weak.iter().take(3) {
                col = col.child(div().flex().items_center().gap(px(8.)).py(px(2.)).child(div().w(px(84.)).text_size(px(12.)).text_color(t.text).child(k.clone())).child(bar(*v, &t)).child(div().w(px(26.)).text_size(px(12.)).text_color(t.secondary).child(format!("{v:.0}"))));
            }
        }
        let recurring = h.recurring();
        if !recurring.is_empty() {
            col = col.child(heading("Recurring mistakes", &t));
            for (k, s) in recurring.into_iter().take(8).enumerate() {
                let info = rules::info(&s.rule);
                let trend = s.trend();
                let (arrow, c) = if trend < -0.5 { ("improving", score_color(100., &t)) } else if trend > 0.5 { ("more often", score_color(0., &t)) } else { ("steady", t.secondary) };
                let (n, ok) = h.practice(&s.rule);
                let rule = s.rule.clone();
                col = col.child(
                    div()
                        .mt(px(5.))
                        .p(px(7.))
                        .rounded(px(6.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.separator)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(div().flex_1().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(info.title))
                                .child(div().text_size(px(11.)).text_color(c).child(arrow))
                                .when(exercises::has_bank(&s.rule), |d| {
                                    d.child(button(SharedString::from(format!("studio-rec-{k}")), "Practise", &t).on_click(cx.listener(move |ws, _, window, cx| ws.practice_rules(vec![rule.clone()], window, cx))))
                                }),
                        )
                        .child(note(
                            format!(
                                "{} session{} · {:.1} per 1,000 words recently (was {:.1}){}",
                                s.sessions,
                                if s.sessions == 1 { "" } else { "s" },
                                s.rate_recent,
                                s.rate_before,
                                if n > 0 { format!(" · practice {ok}/{n}") } else { String::new() }
                            ),
                            &t,
                        )),
                );
            }
        }
        let rec = h.recommend(3);
        if !rec.is_empty() {
            col = col.child(heading("Recommended next", &t)).child(note(rec.iter().map(|r| rules::info(r).title).collect::<Vec<_>>().join(" · "), &t));
        }
        if !h.disabled.is_empty() {
            col = col.child(heading("Rules you switched off", &t));
            let disabled: Vec<String> = h.disabled.iter().cloned().collect();
            col = col.child(div().flex().flex_wrap().gap(px(4.)).children(disabled.into_iter().enumerate().map(|(k, r)| {
                let label = format!("{} ↺", rules::info(&r).title);
                chip(SharedString::from(format!("studio-enable-{k}")), label, false, &t).on_click(cx.listener(move |ws, _, _, cx| {
                    ws.studio.history.disabled.remove(&r);
                    ws.studio.history.save();
                    ws.schedule_studio(Duration::from_millis(10), cx);
                    cx.notify();
                }))
            })));
        }
        col.into_any_element()
    }
}
