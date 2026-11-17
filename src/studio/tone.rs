//! Tone: formality, confidence and sentiment from counted markers. When a
//! template asks for a formal register, departures from it become issues.

use serde::{Deserialize, Serialize};

use super::rules::Ctx;
use super::text::Prose;
use super::{Issue, Severity};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Register {
    Academic,
    Formal,
    #[default]
    Neutral,
    Conversational,
}

impl Register {
    pub const ALL: [Register; 4] = [Register::Academic, Register::Formal, Register::Neutral, Register::Conversational];

    pub fn label(self) -> &'static str {
        match self {
            Register::Academic => "Academic",
            Register::Formal => "Formal",
            Register::Neutral => "Neutral",
            Register::Conversational => "Conversational",
        }
    }

    pub fn parse(s: &str) -> Option<Register> {
        Register::ALL.into_iter().find(|r| r.label().eq_ignore_ascii_case(s.trim()))
    }

    pub fn is_formal(self) -> bool {
        matches!(self, Register::Academic | Register::Formal)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToneReport {
    /// 0 (very casual) – 100 (very formal).
    pub formality: f32,
    pub register: Register,
    /// "Tentative", "Balanced" or "Assertive".
    pub confidence: String,
    /// "Positive", "Neutral" or "Negative".
    pub sentiment: String,
    pub contractions: usize,
    pub first_person: usize,
    pub second_person: usize,
    pub exclamations: usize,
    pub questions: usize,
    pub hedges: usize,
    pub boosters: usize,
}

const HEDGES: &[&str] = &["may", "might", "could", "perhaps", "possibly", "probably", "likely", "seems", "seem", "appears", "suggests", "somewhat", "arguably", "generally", "tend", "tends", "maybe"];
const BOOSTERS: &[&str] = &["clearly", "definitely", "certainly", "undoubtedly", "obviously", "always", "never", "must", "prove", "proves", "absolutely", "surely", "indeed"];
const POSITIVE: &[&str] = &["good", "great", "excellent", "benefit", "benefits", "beneficial", "improve", "improves", "improved", "success", "successful", "happy", "positive", "effective", "valuable", "love", "enjoy", "advantage", "advantages", "opportunity", "hope", "best", "better", "wonderful", "amazing", "helpful", "progress", "growth", "safe"];
const NEGATIVE: &[&str] = &["bad", "poor", "problem", "problems", "harm", "harmful", "risk", "risks", "fail", "failure", "negative", "worse", "worst", "difficult", "danger", "dangerous", "crisis", "threat", "loss", "decline", "damage", "sad", "angry", "fear", "hate", "pollution", "disadvantage", "disadvantages", "unfortunately", "crime"];

pub fn analyze(p: &Prose, src: &str) -> ToneReport {
    let mut t = ToneReport::default();
    let mut words = 0usize;
    let mut long = 0usize;
    let (mut pos, mut neg) = (0usize, 0usize);
    for s in p.body() {
        for w in p.sentence_words(s) {
            let l = w.lower.as_str();
            words += 1;
            if l.chars().count() >= 7 {
                long += 1;
            }
            if l.contains('\'') && !l.ends_with("'s") || l.ends_with("n't") || l == "it's" || l == "that's" {
                t.contractions += 1;
            }
            match l {
                "i" | "me" | "my" | "mine" | "we" | "us" | "our" => t.first_person += 1,
                "you" | "your" | "yours" | "you're" => t.second_person += 1,
                _ => {}
            }
            if HEDGES.contains(&l) {
                t.hedges += 1;
            }
            if BOOSTERS.contains(&l) {
                t.boosters += 1;
            }
            if POSITIVE.contains(&l) {
                pos += 1;
            }
            if NEGATIVE.contains(&l) {
                neg += 1;
            }
        }
        let end = src[s.range.clone()].trim_end();
        if end.ends_with('!') {
            t.exclamations += 1;
        }
        if end.ends_with('?') {
            t.questions += 1;
        }
    }
    if words == 0 {
        t.confidence = "Balanced".into();
        t.sentiment = "Neutral".into();
        return t;
    }
    let per100 = |n: usize| n as f32 * 100. / words as f32;
    // Start neutral and move with each marker; weights from counting typical
    // academic vs. blog texts (academic: long words, no contractions, no "you").
    let mut f = 50.;
    f += (long as f32 / words as f32 - 0.2) * 150.;
    f -= per100(t.contractions) * 6.;
    f -= per100(t.second_person) * 3.;
    f -= per100(t.first_person) * 1.5;
    f -= per100(t.exclamations) * 8.;
    f -= per100(t.questions) * 3.;
    t.formality = f.clamp(0., 100.);
    t.register = match t.formality {
        x if x >= 72. => Register::Academic,
        x if x >= 55. => Register::Formal,
        x if x >= 35. => Register::Neutral,
        _ => Register::Conversational,
    };
    t.confidence = match (per100(t.hedges), per100(t.boosters)) {
        (h, b) if h > 3. && h > 2. * b => "Tentative",
        (h, b) if b > 2.5 && b > 2. * h => "Assertive",
        _ => "Balanced",
    }
    .into();
    t.sentiment = match (pos as f32 - neg as f32) / words as f32 * 100. {
        x if x > 2. => "Positive",
        x if x < -2. => "Negative",
        _ => "Neutral",
    }
    .into();
    t
}

/// Register issues when the target is formal.
pub fn check(cx: &mut Ctx, target: Register) {
    if !target.is_formal() {
        return;
    }
    let p = cx.p;
    let push = |cx: &mut Ctx, rule: &'static str, r: std::ops::Range<usize>, sev: Severity, msg: String, sugg: Vec<String>| {
        if !cx.out.iter().any(|i| i.range == r && i.rule == rule) {
            cx.out.push(Issue { rule, range: r, message: msg, suggestions: sugg, severity: sev });
        }
    };
    const EXPAND: &[(&str, &str)] = &[
        ("don't", "do not"), ("doesn't", "does not"), ("didn't", "did not"), ("can't", "cannot"), ("won't", "will not"),
        ("isn't", "is not"), ("aren't", "are not"), ("wasn't", "was not"), ("weren't", "were not"), ("it's", "it is"),
        ("that's", "that is"), ("there's", "there is"), ("i'm", "I am"), ("i've", "I have"), ("we're", "we are"),
        ("they're", "they are"), ("you're", "you are"), ("we've", "we have"), ("they've", "they have"), ("shouldn't", "should not"),
        ("wouldn't", "would not"), ("couldn't", "could not"), ("haven't", "have not"), ("hasn't", "has not"), ("let's", "let us"),
        ("what's", "what is"), ("he's", "he is"), ("she's", "she is"), ("i'll", "I will"), ("we'll", "we will"), ("it'll", "it will"),
    ];
    for s in p.body() {
        let ws = s.words.clone();
        for i in ws.clone() {
            let l = p.word(i);
            let o = &cx.src[p.words[i].range.clone()];
            if let Some((_, full)) = EXPAND.iter().find(|(c, _)| *c == l) {
                let fixed = if o.starts_with(|c: char| c.is_uppercase()) {
                    let mut c = full.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                } else {
                    full.to_string()
                };
                let msg = format!("Write “{fixed}” in full in formal writing.");
                push(cx, "tone.contraction", p.words[i].range.clone(), Severity::Warning, msg, vec![fixed]);
            }
            if target == Register::Academic && matches!(l, "you" | "your") {
                push(cx, "tone.second-person", p.words[i].range.clone(), Severity::Hint, "Avoid addressing the reader in academic writing.".into(), vec![]);
            }
        }
        if let Some(first) = p.words.get(ws.start)
            && !ws.is_empty()
            && matches!(first.lower.as_str(), "and" | "but" | "so" | "or")
            && first.range.start == s.range.start
        {
            let alt = match first.lower.as_str() {
                "and" => "Moreover,",
                "but" => "However,",
                "so" => "Therefore,",
                _ => "Alternatively,",
            };
            let msg = format!("Formal writing rarely starts a sentence with “{}”.", &cx.src[first.range.clone()]);
            push(cx, "tone.conjunction-start", first.range.clone(), Severity::Hint, msg, vec![alt.into()]);
        }
        let end = cx.src[s.range.clone()].trim_end();
        if end.ends_with('!') {
            let e = s.range.start + end.len();
            push(cx, "tone.exclamation", e - 1..e, Severity::Hint, "Exclamation marks are rare in formal writing.".into(), vec![".".into()]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::text::prose;

    #[test]
    fn academic_vs_casual() {
        let a = "The proliferation of digital technologies has fundamentally transformed educational institutions, necessitating comprehensive pedagogical adaptation.";
        let c = "You won't believe it! I'm so happy, we're gonna have fun. Can't wait!";
        let ta = analyze(&prose(a), a);
        let tc = analyze(&prose(c), c);
        assert!(ta.formality > 70., "{}", ta.formality);
        assert!(tc.formality < 30., "{}", tc.formality);
        assert_eq!(tc.register, Register::Conversational);
        assert_eq!(ta.register, Register::Academic);
        assert!(tc.contractions >= 3 && tc.exclamations == 2);
    }

    #[test]
    fn hedges_and_sentiment() {
        let t = "This may possibly suggest that the policy might perhaps help. It seems likely.";
        assert_eq!(analyze(&prose(t), t).confidence, "Tentative");
        let t = "Pollution is a serious problem and a threat that causes damage and harm.";
        assert_eq!(analyze(&prose(t), t).sentiment, "Negative");
    }

    #[test]
    fn formal_issues() {
        let t = "But it's clear you can't win! And so on.";
        let p = prose(t);
        let mut cx = Ctx { src: t, p: &p, out: Vec::new() };
        check(&mut cx, Register::Academic);
        let mut rules: Vec<_> = cx.out.iter().map(|i| i.rule).collect();
        rules.sort();
        rules.dedup();
        assert_eq!(rules, ["tone.conjunction-start", "tone.contraction", "tone.exclamation", "tone.second-person"]);
        let fix = cx.out.iter().find(|i| i.rule == "tone.contraction").unwrap();
        assert_eq!(fix.suggestions, ["it is"]);
    }
}
