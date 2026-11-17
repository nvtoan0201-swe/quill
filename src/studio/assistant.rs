//! The writing coach: semantic tasks for the LLM. Every prompt carries the
//! deterministic findings (so the model explains real, located problems
//! instead of guessing) and the learner profile (so feedback is personal).
//! The coach teaches: it explains, hints and asks before it rewrites.

use super::analysis::Report;
use super::rules;
use super::templates::Template;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    /// Explain one finding in depth.
    Explain,
    /// Hints and guiding questions for the passage, without rewriting it.
    Hints,
    /// Rewrite toward a goal, listing every change and why.
    Rewrite,
    /// Critique the argument: thesis, evidence, reasoning, counter-arguments.
    Critique,
    /// Ideas, angles, vocabulary and outlines for the topic.
    Brainstorm,
    /// Targeted exercises as JSON.
    Exercises,
    /// Score against the template rubric.
    Rubric,
    /// A free question.
    Ask,
}

impl Task {
    pub fn label(self) -> &'static str {
        match self {
            Task::Explain => "Explain",
            Task::Hints => "Hints",
            Task::Rewrite => "Rewrite",
            Task::Critique => "Critique argument",
            Task::Brainstorm => "Brainstorm",
            Task::Exercises => "Exercises",
            Task::Rubric => "Rubric feedback",
            Task::Ask => "Ask",
        }
    }

    pub fn max_tokens(self) -> u32 {
        match self {
            Task::Explain | Task::Hints => 4000,
            Task::Exercises => 6000,
            _ => 12000,
        }
    }
}

pub const REWRITE_GOALS: &[&str] = &["clearer", "more concise", "more formal", "more vivid", "more varied in sentence structure", "more persuasive"];

pub const LEVELS: &[&str] = &["Beginner (A2–B1)", "Intermediate (B2)", "Advanced (C1–C2)"];

#[derive(Clone, Debug, Default)]
pub struct Request {
    /// The passage to work on (selection or whole document).
    pub passage: String,
    /// Whether `passage` is a selection rather than the full text.
    pub is_selection: bool,
    pub level: String,
    pub profile: String,
    pub findings: String,
    pub template: Option<Template>,
    /// Explain: the finding to explain. Ask: the question. Rewrite: the goal.
    pub detail: String,
    /// Rules to target for exercises.
    pub focus_rules: Vec<String>,
}

pub const SYSTEM: &str = "You are the writing coach inside Quill, a writing app for people who want to become better writers in English, many of them learners of English as a foreign language. \
Your job is to make the writer better, not to write for them. \
Teach the underlying principle in plain words, always quote the writer's own words when you point at something, and keep explanations short and concrete. \
Prefer hints and guiding questions that let the writer fix the problem themselves; when you do give corrected text, explain each change so the writer can apply the lesson next time. \
Match your language to the writer's level. Be encouraging but honest: name real strengths, then the few changes that matter most. \
Use simple Markdown: short headings, bullet lists, **bold** for key terms. Never invent facts about the text or claim errors that are not there. \
A deterministic analyzer has already checked grammar, spelling, punctuation and style; its findings are given to you. Treat them as reliable evidence, add what only a careful human reader would notice (meaning, logic, coherence, tone, word choice), and do not repeat every finding back.";

/// Compact text form of the deterministic report for the prompt.
pub fn findings_summary(text: &str, r: &Report, limit: usize) -> String {
    let mut s = format!(
        "Words: {}. Sentences: {} (average {:.1} words, longest {}). Reading grade {:.1} ({}). Lexical diversity MTLD {:.0}. Academic vocabulary {:.0}%. Register: {} (formality {:.0}/100). Confidence: {}.\nScores: {}.\n",
        r.words,
        r.structure.sentences,
        r.structure.avg_len,
        r.structure.longest,
        r.readability.grade(),
        r.readability.ease_label(),
        r.vocab.mtld,
        r.vocab.academic_ratio * 100.,
        r.tone.register.label(),
        r.tone.formality,
        r.tone.confidence,
        r.scores.iter().map(|(k, v)| format!("{k} {v:.0}")).collect::<Vec<_>>().join(", ")
    );
    let issues = super::analysis::prioritized(&r.issues);
    if !issues.is_empty() {
        s.push_str("Findings (most important first):\n");
        for i in issues.into_iter().take(limit) {
            let quote: String = text.get(i.range.clone()).unwrap_or("").chars().take(80).collect();
            s.push_str(&format!("- [{}] “{}” — {}\n", rules::info(i.rule).title, quote, i.message));
        }
    }
    s
}

fn template_block(t: &Template) -> String {
    let mut s = format!("Template: {} ({}). {}\n", t.name, t.category, t.description);
    if !t.prompt.is_empty() {
        s.push_str(&format!("Task/question: {}\n", t.prompt));
    }
    if !t.rubric.is_empty() {
        s.push_str("Rubric:\n");
        for r in &t.rubric {
            s.push_str(&format!("- {r}\n"));
        }
    }
    s
}

/// (system prompt, user message, max tokens)
pub fn build(task: Task, req: &Request) -> (String, String, u32) {
    let mut u = String::new();
    u.push_str(&format!("Writer's level: {}.\nLearner profile: {}\n\n", if req.level.is_empty() { LEVELS[1] } else { &req.level }, req.profile));
    if let Some(t) = &req.template {
        u.push_str(&template_block(t));
        u.push('\n');
    }
    if !req.findings.is_empty() {
        u.push_str("Analyzer report:\n");
        u.push_str(&req.findings);
        u.push('\n');
    }
    let what = if req.is_selection { "Selected passage" } else { "Text" };
    u.push_str(&format!("{what}:\n<text>\n{}\n</text>\n\n", req.passage.trim()));
    let instruction = match task {
        Task::Explain => format!(
            "Explain this finding to the writer: {}\n\
             1. Why it is a problem here, quoting their words.\n\
             2. The rule, in two or three sentences, with two new example pairs (wrong → right).\n\
             3. A memory tip.\n\
             4. Finish with a hint toward the fix, not the fix itself — let them try.",
            req.detail
        ),
        Task::Hints => "Give at most five hints, most important first. For each: quote the exact words, name the principle, and ask a guiding question that leads the writer to improve it. Do not rewrite sentences for them. Prioritise meaning, logic and coherence over small errors already listed by the analyzer.".to_string(),
        Task::Rewrite => format!(
            "Rewrite the passage to be {}. Keep the writer's meaning and voice; change only what serves the goal.\n\
             Format:\n## Rewrite\n(the new version)\n## What changed and why\n(one bullet per change: “before” → “after” — the principle behind it)\n## Try it yourself\n(one similar sentence from their text for them to improve on their own)",
            if req.detail.is_empty() { "clearer" } else { &req.detail }
        ),
        Task::Critique => "Critique the argument (not the grammar).\n\
             ## Thesis — is there one clear, arguable claim? Quote it or say it is missing.\n\
             ## Claims and evidence — list each main claim and judge its support (fact, example, data, authority, none).\n\
             ## Reasoning — gaps, leaps, or logical fallacies (name them: hasty generalisation, false dichotomy, slippery slope, appeal to popularity…), each with a quote.\n\
             ## Counter-arguments — the strongest objection the writer has not answered.\n\
             ## Questions to strengthen it — three questions the writer should answer in the next draft.\n\
             Do not rewrite the essay."
            .to_string(),
        Task::Brainstorm => "Help the writer think before writing (or when stuck). Use the task/question if a template gives one; otherwise the text's topic.\n\
             ## Angles — four different ways to approach the topic.\n\
             ## Arguments and examples — for and against, each with a concrete example the writer could research or draw from experience.\n\
             ## Useful vocabulary — ten topic-specific words or collocations with a short example each, suited to the writer's level.\n\
             ## Two outline options — paragraph by paragraph.\n\
             ## Questions to ask yourself — three.\n\
             Give ideas and structure, not finished paragraphs."
            .to_string(),
        Task::Exercises => format!(
            "Create 5 short practice exercises that target this writer's weaknesses{}. Base at least two on sentences similar to ones in their text (do not copy their text verbatim). Each exercise is one sentence with exactly one problem to fix.\n\
             Reply with only a JSON array, no prose, in this shape:\n\
             [{{\"rule\": \"<one of: {}>\", \"instruction\": \"<what to do>\", \"sentence\": \"<sentence with the problem>\", \"answer\": \"<corrected sentence; separate alternatives with ' || '>\", \"explanation\": \"<the rule in one or two sentences>\"}}]",
            if req.focus_rules.is_empty() { String::new() } else { format!(" (focus: {})", req.focus_rules.iter().map(|r| rules::info(r).title).collect::<Vec<_>>().join(", ")) },
            rules::RULES.iter().map(|r| r.id).collect::<Vec<_>>().join(", ")
        ),
        Task::Rubric => {
            let ielts = req.template.as_ref().is_some_and(|t| t.category == "IELTS");
            format!(
                "Assess the text against each rubric criterion{}.\n\
                 For each criterion: a heading, {}, two sentences of justification quoting the text, and the single most useful improvement.\n\
                 Then: ## Strengths (two) and ## Top three priorities for the next draft.\n\
                 Be calibrated: do not inflate scores. This is formative feedback, not an official score.",
                if ielts { " using IELTS band descriptors (0–9, half bands allowed)" } else { "" },
                if ielts { "an estimated band" } else { "a score from 1 to 5" }
            )
        }
        Task::Ask => format!("The writer asks: {}\nAnswer as their coach, grounded in the text.", req.detail),
    };
    u.push_str(&instruction);
    (SYSTEM.to_string(), u, task.max_tokens())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::analysis::{Options, analyze};
    use crate::studio::templates::builtins;

    #[test]
    fn prompts_carry_evidence_and_profile() {
        let text = "Technology are very important. Its a good thing.";
        let r = analyze(text, &Options::default());
        let req = Request {
            passage: text.into(),
            level: LEVELS[0].into(),
            profile: "Recurring mistakes: a / an (3 sessions).".into(),
            findings: findings_summary(text, &r, 10),
            template: builtins().into_iter().find(|t| t.id == "ielts-task-2"),
            ..Default::default()
        };
        let (sys, user, max) = build(Task::Rubric, &req);
        assert!(sys.contains("not to write for them"));
        assert!(user.contains("Beginner") && user.contains("a / an (3 sessions)"));
        assert!(user.contains("Commonly confused words") && user.contains("“Its”"));
        assert!(user.contains("IELTS band descriptors") && user.contains("Task Response"));
        assert_eq!(max, 12000);
        let (_, user, _) = build(Task::Exercises, &Request { focus_rules: vec!["grammar.article".into()], ..req.clone() });
        assert!(user.contains("focus: a / an") && user.contains("JSON array"));
        let (_, user, _) = build(Task::Hints, &req);
        assert!(user.contains("Do not rewrite"));
    }
}
