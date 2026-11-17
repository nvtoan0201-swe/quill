//! The learner profile: one snapshot per writing session, exercise results,
//! and what follows from them — recurring errors, weak areas, progress, and
//! which exercises to do next.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::analysis::Report;
use super::rules;
use super::Category;

/// Edits to the same document within this window update one session.
const SESSION_SECS: u64 = 30 * 60;
const MAX_SNAPSHOTS: usize = 3000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub ts: u64,
    pub doc: String,
    pub template: Option<String>,
    pub words: usize,
    pub scores: BTreeMap<String, f32>,
    pub rules: BTreeMap<String, u32>,
    pub grade: f32,
    pub mtld: f32,
}

impl Snapshot {
    pub fn from_report(ts: u64, doc: &str, template: Option<&str>, r: &Report) -> Self {
        Snapshot {
            ts,
            doc: doc.to_string(),
            template: template.map(str::to_string),
            words: r.words,
            scores: r.scores.clone(),
            rules: r.rule_counts(),
            grade: r.readability.grade(),
            mtld: r.vocab.mtld,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExerciseLog {
    pub ts: u64,
    pub rule: String,
    pub correct: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub snapshots: Vec<Snapshot>,
    pub exercises: Vec<ExerciseLog>,
    /// Document path → template id.
    pub doc_templates: BTreeMap<String, String>,
    /// Rules the writer turned off.
    pub disabled: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RuleStat {
    pub rule: String,
    /// Sessions in which the rule fired.
    pub sessions: usize,
    pub total: u32,
    /// Occurrences per 1,000 words, older half vs. recent half of the sessions.
    pub rate_before: f32,
    pub rate_recent: f32,
}

impl RuleStat {
    /// Negative means improving.
    pub fn trend(&self) -> f32 {
        self.rate_recent - self.rate_before
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub key: String,
    pub before: f32,
    pub recent: f32,
}

pub fn path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("quill").join("studio-history.json"))
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl History {
    pub fn load() -> Self {
        path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string(self) {
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }

    /// Adds a snapshot, merging with the current session of the same document.
    /// Tiny texts are not recorded (they say nothing about the writer).
    pub fn record(&mut self, snap: Snapshot) -> bool {
        if snap.words < 30 {
            return false;
        }
        if let Some(last) = self.snapshots.iter_mut().rev().find(|s| s.doc == snap.doc)
            && snap.ts.saturating_sub(last.ts) < SESSION_SECS
        {
            *last = snap;
        } else {
            self.snapshots.push(snap);
        }
        if self.snapshots.len() > MAX_SNAPSHOTS {
            let extra = self.snapshots.len() - MAX_SNAPSHOTS;
            self.snapshots.drain(..extra);
        }
        true
    }

    pub fn log_exercise(&mut self, ts: u64, rule: &str, correct: bool) {
        self.exercises.push(ExerciseLog { ts, rule: rule.to_string(), correct });
    }

    pub fn total_words(&self) -> usize {
        self.snapshots.iter().map(|s| s.words).sum()
    }

    fn halves(&self) -> (&[Snapshot], &[Snapshot]) {
        let n = self.snapshots.len();
        let recent = (n / 2).clamp(1.min(n), 10);
        let split = n - recent;
        let before_start = split.saturating_sub(10);
        (&self.snapshots[before_start..split], &self.snapshots[split..])
    }

    /// Every rule seen, with how often and the trend.
    pub fn rule_stats(&self) -> Vec<RuleStat> {
        let (before, recent) = self.halves();
        let rate = |snaps: &[Snapshot], rule: &str| -> f32 {
            let words: usize = snaps.iter().map(|s| s.words).sum();
            if words == 0 {
                return 0.;
            }
            snaps.iter().map(|s| *s.rules.get(rule).unwrap_or(&0) as f32).sum::<f32>() * 1000. / words as f32
        };
        let mut all: BTreeSet<&str> = BTreeSet::new();
        for s in &self.snapshots {
            all.extend(s.rules.keys().map(String::as_str));
        }
        all.into_iter()
            .map(|r| RuleStat {
                rule: r.to_string(),
                sessions: self.snapshots.iter().filter(|s| s.rules.contains_key(r)).count(),
                total: self.snapshots.iter().map(|s| *s.rules.get(r).unwrap_or(&0)).sum(),
                rate_before: if before.is_empty() { rate(recent, r) } else { rate(before, r) },
                rate_recent: rate(recent, r),
            })
            .collect()
    }

    /// Mistakes that keep coming back: seen in at least two sessions (or three
    /// times in one), correctness rules first, most frequent first.
    pub fn recurring(&self) -> Vec<RuleStat> {
        let mut v: Vec<RuleStat> = self.rule_stats().into_iter().filter(|s| s.sessions >= 2 || s.total >= 3).collect();
        let weight = |s: &RuleStat| {
            let info = rules::info(&s.rule);
            let k = if info.category.is_correctness() { 2.0 } else { 1.0 };
            k * (s.sessions as f32 + s.rate_recent)
        };
        v.sort_by(|a, b| weight(b).total_cmp(&weight(a)));
        v
    }

    /// Average score per area over the recent sessions, weakest first.
    pub fn weaknesses(&self) -> Vec<(String, f32)> {
        let (_, recent) = self.halves();
        let mut sums: BTreeMap<&str, (f32, usize)> = BTreeMap::new();
        for s in recent {
            for (k, v) in &s.scores {
                let e = sums.entry(k).or_default();
                e.0 += v;
                e.1 += 1;
            }
        }
        let mut v: Vec<(String, f32)> = sums.into_iter().map(|(k, (s, n))| (k.to_string(), s / n as f32)).collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    }

    /// Score change between earlier and recent sessions (needs four or more).
    pub fn progress(&self) -> Vec<Progress> {
        if self.snapshots.len() < 4 {
            return Vec::new();
        }
        let (before, recent) = self.halves();
        let avg = |snaps: &[Snapshot], k: &str| {
            let v: Vec<f32> = snaps.iter().filter_map(|s| s.scores.get(k).copied()).collect();
            if v.is_empty() { None } else { Some(v.iter().sum::<f32>() / v.len() as f32) }
        };
        let keys: BTreeSet<&str> = self.snapshots.iter().flat_map(|s| s.scores.keys().map(String::as_str)).collect();
        keys.into_iter()
            .filter_map(|k| Some(Progress { key: k.to_string(), before: avg(before, k)?, recent: avg(recent, k)? }))
            .collect()
    }

    pub fn overall_series(&self, n: usize) -> Vec<f32> {
        let v: Vec<f32> = self
            .snapshots
            .iter()
            .filter(|s| !s.scores.is_empty())
            .map(|s| super::analysis::overall_of(&s.scores))
            .collect();
        v[v.len().saturating_sub(n)..].to_vec()
    }

    /// (attempts, correct) for one rule over its last ten exercises.
    pub fn practice(&self, rule: &str) -> (usize, usize) {
        let v: Vec<&ExerciseLog> = self.exercises.iter().filter(|e| e.rule == rule).collect();
        let last = &v[v.len().saturating_sub(10)..];
        (last.len(), last.iter().filter(|e| e.correct).count())
    }

    pub fn mastered(&self, rule: &str) -> bool {
        let (n, ok) = self.practice(rule);
        n >= 3 && ok as f32 / n as f32 >= 0.8
    }

    pub fn accuracy(&self) -> Option<f32> {
        (!self.exercises.is_empty()).then(|| self.exercises.iter().filter(|e| e.correct).count() as f32 / self.exercises.len() as f32)
    }

    /// Consecutive days, ending today or yesterday, with writing or practice.
    pub fn streak(&self, now: u64) -> usize {
        let days: BTreeSet<u64> = self.snapshots.iter().map(|s| s.ts / 86400).chain(self.exercises.iter().map(|e| e.ts / 86400)).collect();
        let today = now / 86400;
        let mut d = if days.contains(&today) { today } else if days.contains(&(today.saturating_sub(1))) { today - 1 } else { return 0 };
        let mut n = 0;
        while days.contains(&d) {
            n += 1;
            if d == 0 {
                break;
            }
            d -= 1;
        }
        n
    }

    /// A personal note for a finding, when the writer often makes this mistake.
    pub fn note(&self, rule: &str) -> Option<String> {
        let sessions = self.snapshots.iter().filter(|s| s.rules.contains_key(rule)).count();
        if sessions < 2 {
            return None;
        }
        let last10 = &self.snapshots[self.snapshots.len().saturating_sub(10)..];
        let recent = last10.iter().filter(|s| s.rules.contains_key(rule)).count();
        let mut s = format!("You made this kind of mistake in {recent} of your last {} writing sessions.", last10.len());
        if self.mastered(rule) {
            s.push_str(" Your practice shows you know the rule — slow down when proofreading.");
        } else {
            s.push_str(" A short practice set would help it stick.");
        }
        Some(s)
    }

    /// Rules to practise next: recurring mistakes not yet mastered, then rules
    /// from the weakest area.
    pub fn recommend(&self, n: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in self.recurring() {
            if out.len() >= n {
                break;
            }
            if !self.mastered(&s.rule) && super::exercises::has_bank(&s.rule) {
                out.push(s.rule);
            }
        }
        if out.len() < n {
            for (area, _) in self.weaknesses() {
                for r in rules::RULES.iter().filter(|r| r.category.label().starts_with(&area) || (area == "Structure" && r.category == Category::Structure)) {
                    if out.len() < n && !out.iter().any(|o| o == r.id) && super::exercises::has_bank(r.id) && !self.mastered(r.id) {
                        out.push(r.id.to_string());
                    }
                }
            }
        }
        out
    }

    /// A compact summary sent to the assistant so its feedback is personal.
    pub fn profile_summary(&self) -> String {
        if self.snapshots.is_empty() {
            return "No writing history yet.".into();
        }
        let mut s = format!("Sessions analysed: {} ({} words). ", self.snapshots.len(), self.total_words());
        let rec: Vec<String> = self
            .recurring()
            .iter()
            .take(5)
            .map(|r| format!("{} ({} session{}, {})", rules::info(&r.rule).title, r.sessions, if r.sessions == 1 { "" } else { "s" }, if r.trend() < -0.5 { "improving" } else if r.trend() > 0.5 { "getting worse" } else { "steady" }))
            .collect();
        if !rec.is_empty() {
            s.push_str(&format!("Recurring mistakes: {}. ", rec.join("; ")));
        }
        let weak: Vec<String> = self.weaknesses().iter().take(3).map(|(k, v)| format!("{k} {v:.0}/100")).collect();
        if !weak.is_empty() {
            s.push_str(&format!("Weakest areas: {}. ", weak.join(", ")));
        }
        if let Some(last) = self.snapshots.last() {
            s.push_str(&format!("Recent reading grade {:.1}, lexical diversity (MTLD) {:.0}.", last.grade, last.mtld));
        }
        s
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn snap(ts: u64, doc: &str, words: usize, rules: &[(&str, u32)], grammar: f32) -> Snapshot {
        Snapshot {
            ts,
            doc: doc.into(),
            words,
            rules: rules.iter().map(|(r, n)| (r.to_string(), *n)).collect(),
            scores: [("Grammar".to_string(), grammar), ("Clarity".to_string(), 80.)].into(),
            ..Default::default()
        }
    }

    #[test]
    fn sessions_merge() {
        let mut h = History::default();
        assert!(!h.record(snap(0, "a", 10, &[], 50.)));
        h.record(snap(0, "a", 100, &[], 50.));
        h.record(snap(600, "a", 120, &[], 55.));
        h.record(snap(600, "b", 120, &[], 55.));
        h.record(snap(4000, "a", 130, &[], 60.));
        assert_eq!(h.snapshots.len(), 3);
        assert_eq!(h.snapshots[0].words, 120);
    }

    #[test]
    fn recurring_progress_and_recommendations() {
        let mut h = History::default();
        let day = 86400;
        for i in 0..8u64 {
            let articles = if i < 4 { 4 } else { 1 };
            h.record(snap(i * day, &format!("d{i}"), 200, &[("grammar.article", articles), ("clarity.passive", 1)], 50. + i as f32 * 5.));
        }
        let rec = h.recurring();
        assert_eq!(rec[0].rule, "grammar.article");
        assert!(rec[0].trend() < 0.);
        let p = h.progress();
        let g = p.iter().find(|p| p.key == "Grammar").unwrap();
        assert!(g.recent > g.before);
        assert_eq!(h.weaknesses()[0].0, "Grammar");
        assert_eq!(h.recommend(1), ["grammar.article"]);
        // Mastery moves it down the list.
        for _ in 0..4 {
            h.log_exercise(8 * day, "grammar.article", true);
        }
        assert!(h.mastered("grammar.article"));
        assert!(!h.recommend(3).contains(&"grammar.article".to_string()));
        assert_eq!(h.streak(8 * day + 100), 9);
        assert!(h.note("grammar.article").unwrap().contains("8 of your last 8"));
        assert!(h.profile_summary().contains("a / an"));
    }

    #[test]
    fn serde_round_trip() {
        let mut h = History::default();
        h.record(snap(5, "x", 50, &[("grammar.article", 1)], 70.));
        h.disabled.insert("clarity.passive".into());
        let s = serde_json::to_string(&h).unwrap();
        assert_eq!(serde_json::from_str::<History>(&s).unwrap(), h);
    }
}
