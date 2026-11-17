//! Runs every deterministic check and condenses them into scores.

use std::collections::{BTreeMap, HashSet};

use super::readability::{self, Readability};
use super::rules::{self, Ctx};
use super::text::{self, Prose};
use super::tone::{self, Register, ToneReport};
use super::vocab::{self, VocabStats};
use super::{Category, Issue, Severity};

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub register: Register,
    /// Misspellings from the system dictionary (enchant), when available.
    pub misspelled: Option<HashSet<String>>,
    /// Rule ids the writer switched off.
    pub disabled: HashSet<String>,
    /// Grade band the text should land in (min, max).
    pub grade_band: Option<(f32, f32)>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StructureStats {
    pub sentences: usize,
    pub paragraphs: usize,
    /// Paragraphs with at least one prose sentence (not headings or comments).
    pub prose_paragraphs: usize,
    pub avg_len: f32,
    pub sd_len: f32,
    pub longest: usize,
    /// Share of sentences under 10 words / over 25 words.
    pub short_share: f32,
    pub long_share: f32,
    /// Distinct first words / sentences.
    pub opener_variety: f32,
    /// Word count of every sentence, in order (for the rhythm chart).
    pub lengths: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub issues: Vec<Issue>,
    pub readability: Readability,
    pub vocab: VocabStats,
    pub tone: ToneReport,
    pub structure: StructureStats,
    /// 0–100 per category, plus "Readability".
    pub scores: BTreeMap<String, f32>,
    pub words: usize,
}

impl Report {
    pub fn overall(&self) -> f32 {
        overall_of(&self.scores)
    }

    pub fn count(&self, c: Category) -> usize {
        self.issues.iter().filter(|i| i.category() == c).count()
    }

    /// Issue counts per rule, for the learner history.
    pub fn rule_counts(&self) -> BTreeMap<String, u32> {
        let mut m = BTreeMap::new();
        for i in &self.issues {
            *m.entry(i.rule.to_string()).or_default() += 1;
        }
        m
    }
}

/// Weighted mean of the area scores: accuracy and expression matter more than
/// register or reading level.
pub fn overall_of(scores: &BTreeMap<String, f32>) -> f32 {
    let weight = |k: &str| match k {
        "Grammar" => 2.,
        "Vocabulary" | "Structure" => 1.5,
        "Tone" | "Readability" => 0.5,
        _ => 1.,
    };
    let total: f32 = scores.keys().map(|k| weight(k)).sum();
    if total == 0. {
        return 0.;
    }
    scores.iter().map(|(k, v)| weight(k) * v).sum::<f32>() / total
}

fn structure_stats(p: &Prose) -> StructureStats {
    let lengths: Vec<usize> = p.body().map(|s| s.words.len()).filter(|n| *n > 0).collect();
    let prose_paragraphs = p.body().map(|s| s.paragraph).collect::<HashSet<_>>().len();
    let mut s = StructureStats { sentences: lengths.len(), paragraphs: p.paragraphs.len(), prose_paragraphs, ..Default::default() };
    if lengths.is_empty() {
        return s;
    }
    let n = lengths.len() as f32;
    s.avg_len = lengths.iter().sum::<usize>() as f32 / n;
    s.sd_len = (lengths.iter().map(|&l| (l as f32 - s.avg_len).powi(2)).sum::<f32>() / n).sqrt();
    s.longest = *lengths.iter().max().unwrap();
    s.short_share = lengths.iter().filter(|&&l| l < 10).count() as f32 / n;
    s.long_share = lengths.iter().filter(|&&l| l > 25).count() as f32 / n;
    let openers: HashSet<&str> = p.body().filter_map(|s| p.words.get(s.words.start).filter(|_| !s.words.is_empty())).map(|w| w.lower.as_str()).collect();
    s.opener_variety = openers.len() as f32 / n;
    s.lengths = lengths;
    s
}

fn issue_score(issues: &[Issue], cats: &[Category], words: usize, k: f32) -> f32 {
    let w: f32 = issues.iter().filter(|i| cats.contains(&i.category())).map(|i| i.severity.weight()).sum();
    let per100 = w * 100. / words.max(50) as f32;
    100. * (-k * per100).exp()
}

pub fn analyze(src: &str, opts: &Options) -> Report {
    let p = text::prose(src);
    let mut cx = Ctx { src, p: &p, out: rules::check(src, &p, opts.misspelled.as_ref()) };
    vocab::check(&mut cx, opts.register.is_formal());
    tone::check(&mut cx, opts.register);
    let mut issues = cx.out;
    issues.retain(|i| !opts.disabled.contains(i.rule));
    // When two findings overlap, keep the more severe (then the earlier) one, so
    // the editor never shows stacked underlines with conflicting fixes.
    issues.sort_by(|a, b| a.range.start.cmp(&b.range.start).then(b.severity.cmp(&a.severity)));
    let mut kept: Vec<Issue> = Vec::with_capacity(issues.len());
    for i in issues {
        // Sentence-wide findings (long sentence, run-on) may overlap anything.
        let local = |x: &Issue| !x.suggestions.is_empty() && x.range.len() < 60;
        let clash = kept.iter().position(|k| k.range.start < i.range.end && i.range.start < k.range.end && local(k) && local(&i));
        match clash {
            Some(ix) if kept[ix].severity >= i.severity => {}
            Some(ix) => kept[ix] = i,
            None => kept.push(i),
        }
    }
    // A dictionary miss on a word another rule already explains adds nothing.
    let explained: Vec<std::ops::Range<usize>> = kept.iter().filter(|i| i.rule != "spelling.dictionary").map(|i| i.range.clone()).collect();
    kept.retain(|i| i.rule != "spelling.dictionary" || !explained.iter().any(|r| r.start <= i.range.start && i.range.end <= r.end));
    let issues = kept;
    let readability = readability::measure(&p);
    let vocab = vocab::stats(&p);
    let tone = tone::analyze(&p, src);
    let structure = structure_stats(&p);
    let words = p.body_words();

    let mut scores = BTreeMap::new();
    if words > 0 {
        scores.insert("Grammar".to_string(), issue_score(&issues, &[Category::Grammar], words, 0.12));
        scores.insert("Spelling".to_string(), issue_score(&issues, &[Category::Spelling], words, 0.15));
        scores.insert("Punctuation".to_string(), issue_score(&issues, &[Category::Punctuation], words, 0.15));
        let vi = issue_score(&issues, &[Category::Vocabulary], words, 0.15);
        let vocab_score = if vocab.words >= 50 {
            let richness = 100. * (0.6 * ((vocab.mtld - 30.) / 50.).clamp(0., 1.) + 0.4 * (vocab.long_word_ratio / 0.25).clamp(0., 1.));
            0.5 * vi + 0.5 * richness
        } else {
            vi
        };
        scores.insert("Vocabulary".to_string(), vocab_score);
        let si = issue_score(&issues, &[Category::Structure], words, 0.15);
        let structure_score = if structure.sentences >= 4 {
            let variety = 100. * (structure.sd_len / structure.avg_len.max(1.) / 0.5).clamp(0., 1.);
            0.65 * si + 0.35 * variety
        } else {
            si
        };
        scores.insert("Structure".to_string(), structure_score);
        scores.insert("Clarity".to_string(), issue_score(&issues, &[Category::Clarity], words, 0.1));
        let mut tone_score = issue_score(&issues, &[Category::Tone], words, 0.12);
        if opts.register.is_formal() && !tone.register.is_formal() {
            tone_score -= 15.;
        }
        scores.insert("Tone".to_string(), tone_score.max(0.));
        let (lo, hi) = opts.grade_band.unwrap_or((7., 13.));
        let g = readability.grade();
        let off = if g < lo { lo - g } else if g > hi { g - hi } else { 0. };
        scores.insert("Readability".to_string(), (100. - off * 12.).clamp(0., 100.));
    }
    Report { issues, readability, vocab, tone, structure, scores, words }
}

/// The issues worth showing first: errors before hints, then by position.
pub fn prioritized(issues: &[Issue]) -> Vec<&Issue> {
    let mut v: Vec<&Issue> = issues.iter().collect();
    v.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.range.start.cmp(&b.range.start)));
    v
}

/// The range to replace when applying `suggestion`: deleting a word also
/// drops one following space so no double space remains.
pub fn fix_range(text: &str, issue: &Issue, suggestion: &str) -> std::ops::Range<usize> {
    let mut r = issue.range.clone();
    if suggestion.is_empty() && text[r.end..].starts_with(' ') {
        r.end += 1;
    }
    r
}

pub fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Error => "Mistake",
        Severity::Warning => "Check",
        Severity::Hint => "Style",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESSAY: &str = "# My opinion\n\nNowadays, technology is very important in our life. Many people think its a good thing, but some people say it have many bad effect. In this essay i will discuss both side.\n\nFirstly, technology help us to communicate. For example, we can talk with our friends who lives far away. It is very easy, we just need a phone.";

    #[test]
    fn essay_report() {
        let r = analyze(ESSAY, &Options { register: Register::Academic, ..Default::default() });
        let rules: HashSet<&str> = r.issues.iter().map(|i| i.rule).collect();
        for want in ["grammar.confused", "grammar.agreement", "grammar.capitalization", "grammar.plural", "structure.comma-splice", "vocab.intensifier", "clarity.unsupported"] {
            assert!(rules.contains(want), "missing {want}: {rules:?}");
        }
        assert!(r.scores["Grammar"] < 60., "{:?}", r.scores);
        assert!(r.issues.iter().any(|i| i.rule == "grammar.agreement" && &ESSAY[i.range.clone()] == "help"));
        assert!(r.issues.iter().any(|i| i.rule == "grammar.agreement" && &ESSAY[i.range.clone()] == "lives"));
        assert!(r.words > 40);
        assert_eq!(r.structure.paragraphs, 3);
    }

    #[test]
    fn clean_text_scores_high() {
        let t = "The library opens at nine. Students gather in the reading room, where quiet conversation fills the space. Later, the sun warms the old wooden tables, and visitors linger over their books.";
        let r = analyze(t, &Options::default());
        assert!(r.issues.iter().all(|i| i.severity != Severity::Error), "{:?}", r.issues);
        assert!(r.scores["Grammar"] > 95.);
    }

    fn apply(text: &str, issue: &Issue, suggestion: &str) -> String {
        let r = fix_range(text, issue, suggestion);
        format!("{}{}{}", &text[..r.start], suggestion, &text[r.end..])
    }

    #[test]
    fn disabled_rules_and_apply() {
        let t = "I recieved it.";
        let r = analyze(t, &Options { disabled: ["spelling.common".to_string()].into(), ..Default::default() });
        assert!(r.issues.is_empty());
        let r = analyze(t, &Options::default());
        assert_eq!(apply(t, &r.issues[0], "received"), "I received it.");
        let t = "It was basically done.";
        let r = analyze(t, &Options::default());
        let i = r.issues.iter().find(|i| i.rule == "clarity.filler").unwrap();
        assert_eq!(apply(t, i, ""), "It was done.");
    }
}
