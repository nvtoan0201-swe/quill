//! Writing Studio: tools that help the writer improve, not just the text.
//!
//! - `text`, `rules`, `vocab`, `tone`, `readability`, `analysis`: deterministic,
//!   rule-based English analysis (works offline, same input → same findings).
//! - `assistant` + `llm`: semantic tasks (explain, critique, brainstorm, exercises)
//!   through the Claude API, grounded in the deterministic findings.
//! - `templates`: writing templates (IELTS, essays, blogs, technical, creative)
//!   with structure checks; users customize them as Markdown files.
//! - `history` + `exercises`: the learner profile — recurring errors,
//!   weaknesses, progress — and practice built from the writer's own sentences.

pub mod analysis;
pub mod assistant;
pub mod cli;
pub mod exercises;
pub mod history;
pub mod llm;
pub mod readability;
pub mod rules;
pub mod templates;
pub mod text;
pub mod tone;
pub mod vocab;

use std::ops::Range;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Category {
    Spelling,
    Grammar,
    Punctuation,
    Vocabulary,
    Structure,
    Clarity,
    Tone,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Grammar,
        Category::Spelling,
        Category::Punctuation,
        Category::Vocabulary,
        Category::Structure,
        Category::Clarity,
        Category::Tone,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Spelling => "Spelling",
            Category::Grammar => "Grammar",
            Category::Punctuation => "Punctuation",
            Category::Vocabulary => "Vocabulary",
            Category::Structure => "Sentence structure",
            Category::Clarity => "Clarity",
            Category::Tone => "Tone",
        }
    }

    /// Mistakes (red) versus style advice (blue) in the editor.
    pub fn is_correctness(self) -> bool {
        matches!(self, Category::Spelling | Category::Grammar | Category::Punctuation)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// Style suggestion; the sentence is not wrong.
    Hint,
    /// Probably a problem.
    Warning,
    /// Almost certainly a mistake.
    Error,
}

impl Severity {
    pub fn weight(self) -> f32 {
        match self {
            Severity::Hint => 0.25,
            Severity::Warning => 0.6,
            Severity::Error => 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// Key into `rules::RULES`.
    pub rule: &'static str,
    pub range: Range<usize>,
    /// What is wrong here, specifically.
    pub message: String,
    /// Replacements for `range`, best first. Empty when only a rewrite helps.
    pub suggestions: Vec<String>,
    pub severity: Severity,
}

impl Issue {
    pub fn info(&self) -> &'static rules::RuleInfo {
        rules::info(self.rule)
    }

    pub fn category(&self) -> Category {
        self.info().category
    }
}

/// Shifts issue ranges after an edit, dropping the ones the edit touched.
pub fn shift_issues(issues: &mut Vec<Issue>, edit: Range<usize>, new_len: usize) {
    let delta = new_len as isize - edit.len() as isize;
    issues.retain_mut(|i| {
        if i.range.end <= edit.start {
            true
        } else if i.range.start >= edit.end {
            i.range = (i.range.start as isize + delta) as usize..(i.range.end as isize + delta) as usize;
            true
        } else {
            false
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifting() {
        let mk = |r: Range<usize>| Issue { rule: "grammar.article", range: r, message: String::new(), suggestions: vec![], severity: Severity::Error };
        let mut v = vec![mk(0..2), mk(5..7), mk(10..12)];
        shift_issues(&mut v, 4..6, 0);
        assert_eq!(v.iter().map(|i| i.range.clone()).collect::<Vec<_>>(), [0..2, 8..10]);
    }
}
