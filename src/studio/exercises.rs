//! Practice. Exercises come from a built-in bank, from the writer's own
//! sentences (the best material: their real mistakes), or from the assistant.
//! Grading is deterministic: an exact model answer, or — for open rewrites —
//! the analyzer must no longer find the target problem, the answer must not
//! introduce new mistakes, and it must keep the sentence's meaning words.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::analysis::{self, Options};
use super::text;
use super::vocab::is_stopword;
use super::{Issue, Severity, rules};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Bank,
    YourWriting,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Exercise {
    pub rule: String,
    pub instruction: String,
    /// The sentence to fix or rewrite.
    pub text: String,
    /// Accepted answers; empty for open rewrites.
    pub answers: Vec<String>,
    pub explanation: String,
    pub source: Source,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grade {
    pub correct: bool,
    pub feedback: String,
    pub model: Option<String>,
}

/// (rule, instruction, sentence, answers separated by " || " — empty for open rewrites)
const BANK: &[(&str, &str, &str, &str)] = &[
    ("grammar.article", "Correct the article.", "She bought a umbrella before the storm.", "She bought an umbrella before the storm."),
    ("grammar.article", "Correct the article.", "He is an university student.", "He is a university student."),
    ("grammar.article", "Correct the article.", "We waited for a hour at the station.", "We waited for an hour at the station."),
    ("grammar.article", "Correct the article.", "It was an useful lesson.", "It was a useful lesson."),
    ("grammar.article", "Correct the article.", "She wants to do a MBA next year.", "She wants to do an MBA next year."),
    ("grammar.agreement", "Make the verb agree with its subject.", "My brother have two cats.", "My brother has two cats."),
    ("grammar.agreement", "Make the verb agree with its subject.", "She don't like coffee.", "She doesn't like coffee."),
    ("grammar.agreement", "Make the verb agree with its subject.", "They was late for the meeting.", "They were late for the meeting."),
    ("grammar.agreement", "Make the verb agree with its subject.", "He go to the gym every morning.", "He goes to the gym every morning."),
    ("grammar.agreement", "Make the verb agree with its subject.", "There is many reasons for this.", "There are many reasons for this."),
    ("grammar.verb-form", "Fix the verb after the auxiliary.", "She didn't went to school yesterday.", "She didn't go to school yesterday."),
    ("grammar.verb-form", "Fix the verb after the auxiliary.", "You should to call your mother.", "You should call your mother."),
    ("grammar.verb-form", "Fix the verb after the auxiliary.", "He can speaks three languages.", "He can speak three languages."),
    ("grammar.verb-form", "Fix the verb after the auxiliary.", "Did you saw the news?", "Did you see the news?"),
    ("grammar.confused", "Choose the right word.", "Its important to check your work.", "It's important to check your work."),
    ("grammar.confused", "Choose the right word.", "This phone is cheaper then mine.", "This phone is cheaper than mine."),
    ("grammar.confused", "Choose the right word.", "I would of helped if you asked.", "I would have helped if you asked."),
    ("grammar.confused", "Choose the right word.", "Their is a problem with the printer.", "There is a problem with the printer."),
    ("grammar.confused", "Choose the right word.", "Your welcome to join us.", "You're welcome to join us."),
    ("grammar.uncountable", "Fix the uncountable noun.", "Can you give me some advices?", "Can you give me some advice?"),
    ("grammar.uncountable", "Fix the uncountable noun.", "We need more informations about the course.", "We need more information about the course."),
    ("grammar.uncountable", "Fix the uncountable noun.", "She gave me a useful feedback.", "She gave me useful feedback. || She gave me some useful feedback."),
    ("grammar.plural", "Use the correct noun form.", "There are many student in the library.", "There are many students in the library."),
    ("grammar.plural", "Use the correct noun form.", "It is one of the best university in Asia.", "It is one of the best universities in Asia."),
    ("grammar.plural", "Use the correct noun form.", "Several country have banned plastic bags.", "Several countries have banned plastic bags."),
    ("grammar.quantifier", "Choose much/many or less/fewer.", "There were much people at the concert.", "There were many people at the concert."),
    ("grammar.quantifier", "Choose much/many or less/fewer.", "We had less problems this year.", "We had fewer problems this year."),
    ("grammar.capitalization", "Fix the capitalization.", "i visited paris in july.", "I visited Paris in July."),
    ("grammar.capitalization", "Fix the capitalization.", "my sister speaks english and french.", "My sister speaks English and French."),
    ("punct.spacing", "Fix the spacing around punctuation.", "Hello , how are you ?", "Hello, how are you?"),
    ("punct.spacing", "Fix the spacing around punctuation.", "I bought apples,pears and plums.", "I bought apples, pears and plums."),
    ("structure.comma-splice", "Fix the comma splice.", "It was late, we went home.", "It was late, so we went home. || It was late; we went home. || It was late. We went home."),
    ("structure.comma-splice", "Fix the comma splice.", "The test was hard, however I passed.", "The test was hard; however, I passed. || The test was hard. However, I passed."),
    ("structure.comma-splice", "Fix the comma splice.", "She loves music, she plays the piano every day.", "She loves music, and she plays the piano every day. || She loves music; she plays the piano every day. || She loves music. She plays the piano every day."),
    ("structure.fragment", "Join the fragment to a main clause.", "We cancelled the trip. Because the weather was bad.", "We cancelled the trip because the weather was bad."),
    ("structure.fragment", "Join the fragment to a main clause.", "He passed the exam. Although he did not study.", "He passed the exam although he did not study. || Although he did not study, he passed the exam."),
    ("spelling.common", "Correct the spelling.", "I definately recieved the package.", "I definitely received the package."),
    ("spelling.common", "Correct the spelling.", "The goverment will seperate the two departments.", "The government will separate the two departments."),
    ("spelling.common", "Correct the spelling.", "It was neccessary to accomodate everyone.", "It was necessary to accommodate everyone."),
    ("clarity.passive", "Rewrite in the active voice.", "The window was broken by the children.", "The children broke the window."),
    ("clarity.passive", "Rewrite in the active voice.", "The results were analysed by the research team.", "The research team analysed the results."),
    ("clarity.passive", "Rewrite in the active voice.", "A new policy was announced by the government.", "The government announced a new policy."),
    ("clarity.wordy", "Make it concise.", "Due to the fact that it was raining, we stayed home.", "Because it was raining, we stayed home."),
    ("clarity.wordy", "Make it concise.", "In order to pass the exam, you need to practise.", "To pass the exam, you need to practise."),
    ("clarity.wordy", "Make it concise.", "The majority of students prefer online classes.", "Most students prefer online classes."),
    ("clarity.nominalization", "Use the verb instead of the noun phrase.", "The committee made a decision to close the park.", "The committee decided to close the park."),
    ("clarity.nominalization", "Use the verb instead of the noun phrase.", "We will conduct an analysis of the data.", "We will analyse the data. || We will analyze the data."),
    ("clarity.redundant", "Remove the redundancy.", "The end result was a complete surprise.", "The result was a complete surprise."),
    ("clarity.redundant", "Remove the redundancy.", "Please revert back to the old version.", "Please revert to the old version."),
    ("vocab.intensifier", "Replace 'very + adjective' with one strong word.", "After the race, she was very tired.", ""),
    ("vocab.intensifier", "Replace 'very + adjective' with one strong word.", "The new stadium is very big.", ""),
    ("vocab.intensifier", "Replace 'very + adjective' with one strong word.", "This is a very important decision.", ""),
    ("vocab.vague", "Replace the vague word with something specific.", "There are a lot of good things about living in a city.", ""),
    ("tone.contraction", "Make it formal.", "It's clear that we can't ignore the problem.", "It is clear that we cannot ignore the problem."),
    ("tone.contraction", "Make it formal.", "The results don't support the theory.", "The results do not support the theory."),
    ("tone.informal", "Make it formal.", "Kids are gonna learn a lot of stuff online.", ""),
    ("structure.run-on", "Split or restructure this run-on sentence.", "We went to the market and we bought vegetables and then we cooked dinner and we watched a film and we went to bed late.", ""),
    ("structure.long-sentence", "Split this sentence into two or three clear sentences.", "The government introduced a new policy last year which was intended to reduce traffic in the city centre by charging drivers a daily fee, but many residents complained that it unfairly affected people who had no alternative way to get to work.", ""),
];

pub fn has_bank(rule: &str) -> bool {
    BANK.iter().any(|b| b.0 == rule)
}

pub fn bank(rule: &str) -> Vec<Exercise> {
    BANK.iter()
        .filter(|b| b.0 == rule)
        .map(|(r, ins, t, a)| Exercise {
            rule: r.to_string(),
            instruction: ins.to_string(),
            text: t.to_string(),
            answers: a.split(" || ").filter(|s| !s.is_empty()).map(str::to_string).collect(),
            explanation: rules::info(r).lesson.to_string(),
            source: Source::Bank,
        })
        .collect()
}

/// A practice item made from a finding in the writer's own text. Other
/// mistakes in the same sentence are corrected first (from `all`), so the
/// exercise practises exactly one thing.
pub fn from_issue(text: &str, issue: &Issue, all: &[Issue]) -> Option<Exercise> {
    let p = text::prose(text);
    let s = p.sentences.iter().find(|s| s.range.start <= issue.range.start && issue.range.end <= s.range.end)?;
    if text[s.range.clone()].split_whitespace().count() < 3 {
        return None;
    }
    // Apply the other fixable mistakes, right to left so offsets stay valid.
    let mut others: Vec<&Issue> = all
        .iter()
        .filter(|i| i.range != issue.range && s.range.start <= i.range.start && i.range.end <= s.range.end)
        .filter(|i| i.category().is_correctness() && !i.suggestions.is_empty())
        .filter(|i| i.range.end <= issue.range.start || i.range.start >= issue.range.end)
        .collect();
    others.sort_by_key(|i| std::cmp::Reverse(i.range.start));
    let mut sentence = text[s.range.clone()].to_string();
    let mut local = issue.range.start - s.range.start..issue.range.end - s.range.start;
    for o in others {
        let r = o.range.start - s.range.start..o.range.end - s.range.start;
        let fix = &o.suggestions[0];
        sentence.replace_range(r.clone(), fix);
        if r.end <= local.start {
            let delta = fix.len() as isize - r.len() as isize;
            local = (local.start as isize + delta) as usize..(local.end as isize + delta) as usize;
        }
    }
    let info = rules::info(issue.rule);
    let answers = match issue.suggestions.first() {
        Some(fix) if info.category.is_correctness() => {
            let mut a = sentence.clone();
            a.replace_range(local, fix);
            vec![a]
        }
        _ => vec![],
    };
    Some(Exercise {
        rule: issue.rule.to_string(),
        instruction: format!("From your writing — {}: {}", info.title, issue.message),
        text: sentence,
        answers,
        explanation: info.lesson.to_string(),
        source: Source::YourWriting,
    })
}

fn normalize(s: &str, keep_case: bool) -> String {
    let s: String = s.replace(['’', '‘'], "'").replace(['“', '”'], "\"");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if keep_case { s } else { s.to_lowercase() }
}

fn content_words(s: &str) -> HashSet<String> {
    let p = text::prose(s);
    // Compare 5-letter stems so "broken"/"broke" and "children"/"child" match.
    p.words.iter().filter(|w| !is_stopword(&w.lower) && w.lower.len() > 2).map(|w| w.lower.chars().take(5).collect()).collect()
}

pub fn grade(ex: &Exercise, answer: &str) -> Grade {
    let case_matters = ex.rule == "grammar.capitalization";
    let a = normalize(answer, case_matters);
    if a.is_empty() {
        return Grade { correct: false, feedback: "Type your corrected version first.".into(), model: None };
    }
    let model = ex.answers.first().cloned();
    if ex.answers.iter().any(|m| normalize(m, case_matters) == a) {
        return Grade { correct: true, feedback: "Correct!".into(), model: None };
    }
    if normalize(&ex.text, case_matters) == a {
        return Grade { correct: false, feedback: "That is the original sentence — change it to fix the problem.".into(), model };
    }
    let opts = Options::default();
    let after = analysis::analyze(answer, &opts);
    let before = analysis::analyze(&ex.text, &opts);
    if let Some(still) = after.issues.iter().find(|i| i.rule == ex.rule) {
        return Grade { correct: false, feedback: format!("Not yet: {}", still.message), model };
    }
    let before_rules: HashSet<&str> = before.issues.iter().map(|i| i.rule).collect();
    if let Some(new) = after.issues.iter().find(|i| i.severity == Severity::Error && !before_rules.contains(i.rule)) {
        return Grade { correct: false, feedback: format!("The original problem is fixed, but there is a new one: {}", new.message), model };
    }
    // Meaning check: keep most content words (except the ones the rule targets).
    let orig = content_words(&ex.text);
    let kept = content_words(answer);
    let overlap = orig.intersection(&kept).count() as f32 / orig.len().max(1) as f32;
    if overlap < 0.4 {
        return Grade { correct: false, feedback: "This changes the meaning too much — keep the original idea.".into(), model };
    }
    // Specific checks the analyzer cannot see on its own.
    if ex.rule == "vocab.intensifier" && a.contains("very ") {
        return Grade { correct: false, feedback: "Still uses “very”. Find one stronger adjective.".into(), model };
    }
    if !ex.answers.is_empty() && !before.issues.iter().any(|i| i.rule == ex.rule) {
        // The analyzer cannot verify this rule here; only exact answers count.
        return Grade { correct: false, feedback: "Compare with the model answer.".into(), model };
    }
    Grade {
        correct: true,
        feedback: if ex.answers.is_empty() { "Good — the problem is gone and the meaning is kept.".into() } else { "Correct — your version also works.".into() },
        model,
    }
}

/// Parses exercises the assistant returned as a JSON array.
pub fn parse_generated(json: &str) -> Vec<Exercise> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        rule: String,
        #[serde(default)]
        instruction: String,
        #[serde(default)]
        sentence: String,
        #[serde(default)]
        answer: String,
        #[serde(default)]
        explanation: String,
    }
    let start = json.find('[');
    let end = json.rfind(']');
    let (Some(s), Some(e)) = (start, end) else { return Vec::new() };
    let Ok(raw) = serde_json::from_str::<Vec<Raw>>(&json[s..=e]) else { return Vec::new() };
    raw.into_iter()
        .filter(|r| !r.sentence.trim().is_empty())
        .map(|r| {
            let rule = if rules::RULES.iter().any(|x| x.id == r.rule) { r.rule } else { "grammar.confused".to_string() };
            Exercise {
                instruction: if r.instruction.is_empty() { rules::info(&rule).title.to_string() } else { r.instruction },
                rule,
                text: r.sentence,
                answers: r.answer.split(" || ").filter(|a| !a.trim().is_empty()).map(str::to_string).collect(),
                explanation: r.explanation,
                source: Source::Assistant,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::tone;

    #[test]
    fn bank_items_are_detected_and_answers_are_clean() {
        let mut bad = Vec::new();
        for (rule, _, text, answers) in BANK {
            let formal = if rule.starts_with("tone.") { tone::Register::Academic } else { tone::Register::Neutral };
            let opts = Options { register: formal, ..Default::default() };
            let r = analysis::analyze(text, &opts);
            if !r.issues.iter().any(|i| i.rule == *rule) {
                bad.push(format!("not flagged {rule}: {text} → {:?}", r.issues.iter().map(|i| i.rule).collect::<Vec<_>>()));
            }
            for a in answers.split(" || ").filter(|a| !a.is_empty()) {
                let r = analysis::analyze(a, &opts);
                if let Some(i) = r.issues.iter().find(|i| i.severity == Severity::Error || i.rule == *rule) {
                    bad.push(format!("answer flagged {}: {a}", i.rule));
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn grading() {
        let ex = &bank("grammar.article")[0];
        assert!(grade(ex, "She bought an umbrella before the storm.").correct);
        assert!(grade(ex, "  she bought an umbrella   before the storm. ").correct);
        assert!(!grade(ex, "She bought a umbrella before the storm.").correct);
        let ex = bank("clarity.passive").remove(0);
        assert!(grade(&ex, "The children broke the window.").correct);
        assert!(grade(&ex, "The kids broke the window.").correct, "alternative wording passes analyzer checks");
        assert!(!grade(&ex, "The window was smashed by the kids.").correct);
        assert!(!grade(&ex, "I like cake.").correct);
        let ex = bank("vocab.intensifier").remove(0);
        assert!(grade(&ex, "After the race, she was exhausted.").correct);
        assert!(!grade(&ex, "After the race, she was very exhausted.").correct);
    }

    #[test]
    fn from_your_writing() {
        let t = "My first day was fine. She have a dog. It is nice.";
        let r = analysis::analyze(t, &Options::default());
        let issue = r.issues.iter().find(|i| i.rule == "grammar.agreement").unwrap();
        let ex = from_issue(t, issue, &r.issues).unwrap();
        assert_eq!(ex.text, "She have a dog.");
        assert_eq!(ex.answers, ["She has a dog."]);
        assert!(grade(&ex, "She has a dog.").correct);
        // Other mistakes in the sentence are fixed so only one remains.
        let t = "Many people think its a good thing, but it have many bad effect.";
        let r = analysis::analyze(t, &Options::default());
        let issue = r.issues.iter().find(|i| i.rule == "grammar.confused").unwrap();
        let ex = from_issue(t, issue, &r.issues).unwrap();
        assert_eq!(ex.text, "Many people think its a good thing, but it has many bad effects.");
        assert_eq!(ex.answers, ["Many people think it's a good thing, but it has many bad effects."]);
    }

    #[test]
    fn generated_json() {
        let j = "Here you go:\n[{\"rule\":\"grammar.article\",\"instruction\":\"Fix it\",\"sentence\":\"I ate a apple.\",\"answer\":\"I ate an apple.\",\"explanation\":\"vowel sound\"},{\"sentence\":\"\"}]";
        let v = parse_generated(j);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].answers, ["I ate an apple."]);
        assert_eq!(v[0].source, Source::Assistant);
    }
}
