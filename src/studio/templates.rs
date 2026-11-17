//! Writing templates. A template is a Markdown file with a small front matter:
//! constraints (length, time, register, sections), a rubric, tips, and a
//! skeleton whose `// ` lines are guidance comments (shown in the editor, but
//! ignored by analysis and export). Built-in templates use the same format, so
//! "Customize" just writes one to the templates folder for the user to edit.

use std::path::PathBuf;

use super::analysis::Report;
use super::tone::Register;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub register: Register,
    pub min_words: Option<usize>,
    pub max_words: Option<usize>,
    pub minutes: Option<u32>,
    pub min_paragraphs: Option<usize>,
    pub min_headings: Option<usize>,
    /// Headings the document must contain (matched case-insensitively).
    pub sections: Vec<String>,
    pub grade_band: Option<(f32, f32)>,
    /// The task or question to answer.
    pub prompt: String,
    /// "Criterion — what a strong answer does".
    pub rubric: Vec<String>,
    pub tips: Vec<String>,
    /// Each entry "label: a|b|c" — at least one alternative must appear.
    pub expect: Vec<String>,
    /// Rule ids that do not apply to this kind of writing.
    pub disable: Vec<String>,
    pub skeleton: String,
    pub builtin: bool,
}

fn range(v: &str) -> (Option<f32>, Option<f32>) {
    let (a, b) = v.split_once('-').unwrap_or((v, v));
    (a.trim().parse().ok(), b.trim().parse().ok())
}

pub fn parse(id: &str, src: &str) -> Template {
    let mut t = Template { id: id.to_string(), name: id.to_string(), ..Default::default() };
    let body = match src.strip_prefix("---\n").and_then(|r| r.split_once("\n---\n")) {
        Some((front, body)) => {
            for line in front.lines() {
                let Some((k, v)) = line.split_once(':') else { continue };
                let v = v.trim();
                match k.trim() {
                    "name" => t.name = v.into(),
                    "category" => t.category = v.into(),
                    "description" => t.description = v.into(),
                    "register" => t.register = Register::parse(v).unwrap_or_default(),
                    "words" => {
                        let (a, b) = range(v);
                        t.min_words = a.map(|x| x as usize);
                        t.max_words = if v.trim_end().ends_with('-') { None } else { b.map(|x| x as usize) };
                    }
                    "minutes" => t.minutes = v.parse().ok(),
                    "paragraphs" => t.min_paragraphs = v.parse().ok(),
                    "headings" => t.min_headings = v.parse().ok(),
                    "section" => t.sections.push(v.into()),
                    "grade" => {
                        if let (Some(a), Some(b)) = range(v) {
                            t.grade_band = Some((a, b));
                        }
                    }
                    "prompt" => t.prompt = v.into(),
                    "rubric" => t.rubric.push(v.into()),
                    "tip" => t.tips.push(v.into()),
                    "expect" => t.expect.push(v.into()),
                    "disable" => t.disable.extend(v.split([',', ' ']).filter(|s| !s.is_empty()).map(str::to_string)),
                    _ => {}
                }
            }
            body
        }
        None => src,
    };
    t.skeleton = body.trim_start_matches('\n').to_string();
    t
}

/// Back to the file format (used by "Customize").
pub fn serialize(t: &Template) -> String {
    let mut s = String::from("---\n");
    let mut kv = |k: &str, v: &str| {
        if !v.is_empty() {
            s.push_str(&format!("{k}: {v}\n"));
        }
    };
    kv("name", &t.name);
    kv("category", &t.category);
    kv("description", &t.description);
    kv("register", t.register.label());
    match (t.min_words, t.max_words) {
        (Some(a), Some(b)) => kv("words", &format!("{a}-{b}")),
        (Some(a), None) => kv("words", &format!("{a}-")),
        (None, Some(b)) => kv("words", &format!("0-{b}")),
        _ => {}
    }
    if let Some(m) = t.minutes {
        kv("minutes", &m.to_string());
    }
    if let Some(p) = t.min_paragraphs {
        kv("paragraphs", &p.to_string());
    }
    if let Some(h) = t.min_headings {
        kv("headings", &h.to_string());
    }
    for x in &t.sections {
        kv("section", x);
    }
    if let Some((a, b)) = t.grade_band {
        kv("grade", &format!("{a}-{b}"));
    }
    kv("prompt", &t.prompt);
    for x in &t.rubric {
        kv("rubric", x);
    }
    for x in &t.tips {
        kv("tip", x);
    }
    for x in &t.expect {
        kv("expect", x);
    }
    if !t.disable.is_empty() {
        kv("disable", &t.disable.join(", "));
    }
    s.push_str("---\n\n");
    s.push_str(&t.skeleton);
    s
}

const BUILTIN: &[(&str, &str)] = &[
    ("ielts-task-2", r#"---
name: IELTS Writing Task 2
category: IELTS
description: Opinion / discussion essay, Academic and General Training. At least 250 words in 40 minutes.
register: Academic
words: 250-
minutes: 40
paragraphs: 4
grade: 9-14
prompt: Some people believe that university education should be free for everyone, while others think students should pay for it. Discuss both views and give your own opinion.
rubric: Task Response — answers every part of the question, states a clear position, and develops ideas with relevant examples.
rubric: Coherence and Cohesion — logical paragraphs with one central idea each; varied, accurate linking words and referencing.
rubric: Lexical Resource — wide, precise vocabulary with natural collocations; few spelling or word-form errors.
rubric: Grammatical Range and Accuracy — a mix of simple and complex sentences; most sentences error-free.
tip: Spend 5 minutes planning: decide your position and one main idea per body paragraph.
tip: Paraphrase the question in the introduction — do not copy it.
tip: Each body paragraph: topic sentence → explanation → example → link back to the question.
tip: Use a range of linking words (however, moreover, as a result, for instance) but do not overuse them.
expect: Clear position: i believe|in my opinion|i agree|i disagree|this essay will argue|i would argue|my view
expect: Conclusion: in conclusion|to conclude|to sum up|in summary
---

// Introduction: paraphrase the question, then state your position in one sentence.


// Body paragraph 1: the first view (or your first reason). Topic sentence, explanation, example.


// Body paragraph 2: the second view (or your second reason). Topic sentence, explanation, example.


// Conclusion: summarise both views and restate your opinion. No new ideas.

"#),
    ("ielts-task-1", r#"---
name: IELTS Writing Task 1 (Academic)
category: IELTS
description: Describe a chart, graph, table, map or process. At least 150 words in 20 minutes.
register: Academic
words: 150-
minutes: 20
paragraphs: 3
grade: 9-14
prompt: The chart shows the percentage of households with internet access in three countries between 2000 and 2020. Summarise the information by selecting and reporting the main features, and make comparisons where relevant.
rubric: Task Achievement — a clear overview of the main trends; key features selected and supported with accurate figures; no opinion.
rubric: Coherence and Cohesion — information logically grouped; accurate linking and referencing.
rubric: Lexical Resource — precise vocabulary for trends (rise, plummet, fluctuate, peak) and comparisons.
rubric: Grammatical Range and Accuracy — varied structures (comparatives, passives, relative clauses) used accurately.
tip: The overview (main trends, no numbers) is the most important sentence. Many writers forget it.
tip: Group data — do not describe every number.
tip: Do not give opinions or reasons; only report what the visual shows.
expect: Overview: overall|in general|it is clear that|it is evident that|in summary
---

// Introduction: paraphrase what the visual shows (what, where, when).


// Overview: the two or three most important trends or features, without numbers.


// Details 1: describe and compare the first group of data with figures.


// Details 2: describe and compare the second group of data with figures.

"#),
    ("argumentative-essay", r#"---
name: Argumentative Essay
category: Essay
description: Classic five-paragraph argument: thesis, three supporting points, counter-argument, conclusion.
register: Formal
words: 500-1000
paragraphs: 5
grade: 10-14
prompt: Should cities ban private cars from their centres?
rubric: Thesis — a specific, arguable claim in the introduction.
rubric: Evidence — every claim supported by facts, examples, data or expert sources.
rubric: Reasoning — the link between evidence and claim is explained, not assumed.
rubric: Counter-argument — the strongest opposing view is stated fairly and answered.
rubric: Organisation — one idea per paragraph, clear transitions, a conclusion that adds perspective.
tip: Write your thesis first. If you cannot state it in one sentence, keep thinking.
tip: Steelman the other side — answer its best version, not a weak one.
tip: Replace 'some people say' with a named source.
expect: Counter-argument: however|opponents|critics|although|on the other hand|some argue|it could be argued
expect: Conclusion: in conclusion|ultimately|therefore|to conclude
---

# [Title]

// Introduction: hook, context, and your thesis (one arguable sentence).


// Point 1: topic sentence, evidence, explanation of how the evidence supports the thesis.


// Point 2: topic sentence, evidence, explanation.


// Counter-argument: the strongest opposing view, then your rebuttal.


// Conclusion: restate the thesis in new words and say why it matters.

"#),
    ("blog-post", r#"---
name: Blog Post
category: Blog
description: A readable article for the web: a strong hook, scannable headings, short paragraphs, a clear takeaway.
register: Conversational
words: 600-1500
headings: 2
grade: 6-10
rubric: Hook — the first two sentences make the reader want to continue.
rubric: Scannability — descriptive subheadings, short paragraphs, lists where useful.
rubric: Voice — sounds like a person talking to one reader; concrete examples.
rubric: Takeaway — the reader knows what to do or think next.
tip: Lead with the problem or a surprising fact, not with background.
tip: Keep paragraphs to 1–4 sentences.
tip: Subheadings should tell the story on their own.
disable: tone.conjunction-start, tone.second-person, structure.fragment
---

# [Headline that promises a benefit]

// Hook: a question, a surprising fact, or a short story. Two or three sentences.


## [First point]

// One idea per section. Use an example from your own experience.


## [Second point]


## [Takeaway]

// What should the reader do or remember? End with a call to action.

"#),
    ("technical-design", r#"---
name: Technical Design Document
category: Technical
description: Propose a technical change: problem, proposal, alternatives, risks. Clear, precise and skimmable.
register: Formal
words: 400-
headings: 4
grade: 10-15
section: Summary
section: Problem
section: Proposal
section: Alternatives
section: Risks
rubric: Problem — stated with evidence (metrics, incidents, user reports) before any solution.
rubric: Proposal — concrete enough that another engineer could implement it.
rubric: Alternatives — at least two options with honest trade-offs.
rubric: Precision — defined terms, numbers with units, no vague words (fast, soon, easy).
tip: Write the Summary last, in three sentences: problem, proposal, impact.
tip: Prefer active voice: 'The service retries the request' not 'The request is retried'.
tip: Replace adjectives with numbers: 'fast' → 'p99 under 200 ms'.
disable: structure.fragment
---

# [Feature or change name]

## Summary

// Three sentences: the problem, the proposed solution, the expected impact.

## Problem

// What is wrong today, for whom, and how do we know? Include numbers.

## Proposal

// How it works. Components, data flow, interfaces.

## Alternatives

// Other options you considered and why you rejected them.

## Risks

// What could go wrong, and how you will detect and mitigate it.
"#),
    ("how-to-guide", r#"---
name: How-to Guide
category: Technical
description: Step-by-step instructions that get the reader to a working result.
register: Neutral
words: 250-
headings: 3
grade: 7-11
section: Prerequisites
section: Steps
section: Troubleshooting
rubric: Goal — the first paragraph says what the reader will achieve.
rubric: Steps — numbered, one action each, in imperative mood, with expected results.
rubric: Completeness — prerequisites listed; common errors explained.
tip: Start each step with a verb: 'Open', 'Run', 'Click'.
tip: After important steps, tell the reader what they should see.
disable: tone.second-person, structure.fragment
---

# How to [achieve the goal]

// One sentence: what the reader will have at the end.

## Prerequisites

-

## Steps

1.

## Troubleshooting

// The two or three errors readers hit most, with fixes.
"#),
    ("short-story", r#"---
name: Short Story
category: Creative
description: A complete short story with a character who wants something, an obstacle, and a change.
register: Neutral
words: 800-3000
grade: 5-10
prompt: Write a story that begins with a letter arriving twenty years late.
rubric: Character — a protagonist with a clear desire and a distinct voice.
rubric: Conflict — an obstacle that raises the stakes and forces a choice.
rubric: Scene — concrete sensory details; show, don't tell.
rubric: Dialogue — reveals character and moves the plot; natural rhythm.
rubric: Ending — something has changed; the ending feels earned.
tip: Start as late as possible — in the middle of something happening.
tip: Replace emotion words (sad, angry) with actions and details that show them.
tip: Vary sentence length for rhythm: short sentences for tension, long ones for reflection.
disable: structure.fragment, tone.conjunction-start, clarity.passive, structure.repetitive-start
---

# [Title]

// Opening: drop the reader into a moment. Who wants what?


// Rising action: the obstacle. What does the character try, and how does it fail?


// Climax: the choice.


// Ending: what has changed?

"#),
];

pub fn builtins() -> Vec<Template> {
    BUILTIN
        .iter()
        .map(|(id, src)| Template { builtin: true, ..parse(id, src) })
        .collect()
}

pub fn dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("quill").join("templates"))
}

/// Built-ins, overridden or extended by the user's `*.md` files.
pub fn load_all() -> Vec<Template> {
    let mut all = builtins();
    if let Some(d) = dir()
        && let Ok(rd) = std::fs::read_dir(d)
    {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
        files.sort();
        for p in files {
            let Ok(src) = std::fs::read_to_string(&p) else { continue };
            let id = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let t = parse(&id, &src);
            match all.iter_mut().find(|x| x.id == id) {
                Some(x) => *x = t,
                None => all.push(t),
            }
        }
    }
    all
}

/// Writes a template to the user's folder (to customize it) and returns the path.
pub fn customize(t: &Template) -> std::io::Result<PathBuf> {
    let d = dir().ok_or_else(|| std::io::Error::other("no config directory"))?;
    std::fs::create_dir_all(&d)?;
    let p = d.join(format!("{}.md", t.id));
    if !p.exists() {
        std::fs::write(&p, serialize(t))?;
    }
    Ok(p)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Requirement {
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

fn headings(text: &str) -> Vec<String> {
    text.lines().filter(|l| l.starts_with('#')).map(|l| l.trim_start_matches('#').trim().to_lowercase()).collect()
}

/// How the document meets the template's requirements.
pub fn requirements(t: &Template, text: &str, r: &Report) -> Vec<Requirement> {
    let mut out = Vec::new();
    let words = r.words;
    if t.min_words.is_some() || t.max_words.is_some() {
        let ok = t.min_words.is_none_or(|m| words >= m) && t.max_words.is_none_or(|m| words <= m);
        let target = match (t.min_words, t.max_words) {
            (Some(a), Some(b)) => format!("{a}–{b}"),
            (Some(a), None) => format!("at least {a}"),
            (None, Some(b)) => format!("at most {b}"),
            _ => String::new(),
        };
        out.push(Requirement { label: "Length".into(), ok, detail: format!("{words} words (target {target})") });
    }
    if let Some(p) = t.min_paragraphs {
        // Paragraphs of prose (not headings).
        let n = r.structure.prose_paragraphs;
        out.push(Requirement { label: "Paragraphs".into(), ok: n >= p, detail: format!("{n} of at least {p}") });
    }
    let hs = headings(text);
    if let Some(h) = t.min_headings {
        let n = hs.len();
        out.push(Requirement { label: "Headings".into(), ok: n >= h, detail: format!("{n} of at least {h}") });
    }
    for s in &t.sections {
        let ok = hs.iter().any(|h| h.contains(&s.to_lowercase()));
        out.push(Requirement { label: format!("Section “{s}”"), ok, detail: if ok { "present".into() } else { "missing heading".into() } });
    }
    let lower = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    for e in &t.expect {
        let (label, alts) = e.split_once(':').unwrap_or(("Expected", e));
        let alts: Vec<&str> = alts.split('|').map(str::trim).filter(|a| !a.is_empty()).collect();
        let found = alts.iter().find(|a| lower.contains(*a));
        out.push(Requirement {
            label: label.trim().to_string(),
            ok: found.is_some(),
            detail: match found {
                Some(f) => format!("found “{f}”"),
                None => format!("try: {}", alts.iter().take(3).copied().collect::<Vec<_>>().join(", ")),
            },
        });
    }
    if words >= 40 {
        let want = t.register;
        let got = r.tone.register;
        let ok = want.is_formal() == got.is_formal() || (want == Register::Neutral && got != Register::Academic);
        out.push(Requirement { label: "Register".into(), ok, detail: format!("{} (target {})", got.label(), want.label()) });
        if let Some((a, b)) = t.grade_band {
            let g = r.readability.grade();
            out.push(Requirement { label: "Reading level".into(), ok: g >= a - 0.5 && g <= b + 0.5, detail: format!("grade {g:.1} (target {a}–{b})") });
        }
    }
    if t.category == "IELTS" && words >= 80 {
        let n = r.vocab.linker_functions;
        out.push(Requirement { label: "Cohesive devices".into(), ok: n >= 3, detail: format!("{n} kinds of linking words (aim for 3+)") });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::analysis::{Options, analyze};

    #[test]
    fn builtins_parse() {
        let all = builtins();
        assert_eq!(all.len(), 7);
        let t2 = all.iter().find(|t| t.id == "ielts-task-2").unwrap();
        assert_eq!(t2.min_words, Some(250));
        assert_eq!(t2.max_words, None);
        assert_eq!(t2.minutes, Some(40));
        assert_eq!(t2.register, Register::Academic);
        assert_eq!(t2.rubric.len(), 4);
        assert!(t2.skeleton.starts_with("// Introduction"));
        let blog = all.iter().find(|t| t.id == "blog-post").unwrap();
        assert_eq!(blog.disable.len(), 3);
        let td = all.iter().find(|t| t.id == "technical-design").unwrap();
        assert_eq!(td.sections.len(), 5);
        for cat in ["IELTS", "Essay", "Blog", "Technical", "Creative"] {
            assert!(all.iter().any(|t| t.category == cat), "{cat}");
        }
    }

    #[test]
    fn round_trip() {
        for t in builtins() {
            let back = parse(&t.id, &serialize(&t));
            assert_eq!(Template { builtin: true, ..back }, t, "{}", t.id);
        }
    }

    #[test]
    fn requirement_checks() {
        let t = builtins().into_iter().find(|t| t.id == "technical-design").unwrap();
        let text = "# X\n\n## Summary\n\nShort.\n\n## Problem\n\nIt is slow.\n";
        let r = analyze(text, &Options::default());
        let req = requirements(&t, text, &r);
        let missing: Vec<_> = req.iter().filter(|q| !q.ok).map(|q| q.label.as_str()).collect();
        assert!(missing.contains(&"Section “Proposal”") && missing.contains(&"Length"));
        assert!(!missing.contains(&"Section “Summary”"));

        let t = builtins().into_iter().find(|t| t.id == "ielts-task-2").unwrap();
        let text = format!("{}\nIn my opinion this is right.\n\nIn conclusion, yes.", t.skeleton);
        let r = analyze(&text, &Options::default());
        let req = requirements(&t, &text, &r);
        assert!(req.iter().find(|q| q.label == "Clear position").unwrap().ok);
        assert!(req.iter().find(|q| q.label == "Conclusion").unwrap().ok);
        // Guidance comments do not count as writing.
        assert!(r.words < 15);
    }
}
