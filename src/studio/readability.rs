//! Classic readability formulas. All of them are deterministic counts; we show
//! several because each one weighs sentence length and word length differently.

use super::text::{Prose, syllables};

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Readability {
    pub words: usize,
    pub sentences: usize,
    pub syllables: usize,
    /// Words with three or more syllables.
    pub complex_words: usize,
    pub letters: usize,
    /// Flesch Reading Ease: 0–100, higher is easier.
    pub flesch: f32,
    /// Flesch–Kincaid grade level.
    pub fk_grade: f32,
    pub gunning_fog: f32,
    pub smog: f32,
    pub coleman_liau: f32,
    pub ari: f32,
    pub avg_sentence_words: f32,
    pub avg_word_syllables: f32,
}

impl Readability {
    /// Median of the grade-level formulas, which is steadier than any single one.
    pub fn grade(&self) -> f32 {
        let mut g = [self.fk_grade, self.gunning_fog, self.smog, self.coleman_liau, self.ari];
        g.sort_by(|a, b| a.total_cmp(b));
        g[2]
    }

    pub fn ease_label(&self) -> &'static str {
        match self.flesch {
            f if f >= 80. => "Very easy",
            f if f >= 70. => "Easy",
            f if f >= 60. => "Plain English",
            f if f >= 50. => "Fairly difficult",
            f if f >= 30. => "Difficult",
            _ => "Very difficult",
        }
    }

    /// Typical reader for the grade level.
    pub fn audience(&self) -> &'static str {
        match self.grade() {
            g if g < 6. => "Primary school",
            g if g < 9. => "Middle school",
            g if g < 13. => "High school",
            g if g < 16. => "University",
            _ => "Specialist / academic",
        }
    }
}

pub fn measure(p: &Prose) -> Readability {
    let mut r = Readability::default();
    for s in p.body() {
        if s.words.is_empty() {
            continue;
        }
        r.sentences += 1;
        for w in p.sentence_words(s) {
            let n = syllables(&w.lower);
            r.words += 1;
            r.syllables += n;
            if n >= 3 && !w.lower.contains('-') {
                r.complex_words += 1;
            }
            r.letters += w.lower.chars().filter(|c| c.is_alphanumeric()).count();
        }
    }
    if r.words == 0 || r.sentences == 0 {
        return r;
    }
    let w = r.words as f32;
    let s = r.sentences as f32;
    let wps = w / s;
    let spw = r.syllables as f32 / w;
    r.avg_sentence_words = wps;
    r.avg_word_syllables = spw;
    r.flesch = (206.835 - 1.015 * wps - 84.6 * spw).clamp(0., 100.);
    r.fk_grade = (0.39 * wps + 11.8 * spw - 15.59).max(0.);
    r.gunning_fog = 0.4 * (wps + 100. * r.complex_words as f32 / w);
    r.smog = 1.043 * (r.complex_words as f32 * 30. / s).sqrt() + 3.1291;
    let l = r.letters as f32 / w * 100.;
    let s100 = s / w * 100.;
    r.coleman_liau = (0.0588 * l - 0.296 * s100 - 15.8).max(0.);
    r.ari = (4.71 * r.letters as f32 / w + 0.5 * wps - 21.43).max(0.);
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::text::prose;

    #[test]
    fn simple_text_is_easy() {
        let r = measure(&prose("The cat sat on the mat. The dog ran to the park. We had fun."));
        assert_eq!(r.sentences, 3);
        assert_eq!(r.words, 15);
        assert!(r.flesch > 90., "{}", r.flesch);
        assert!(r.grade() < 3.);
    }

    #[test]
    fn academic_text_is_hard() {
        let r = measure(&prose(
            "The implementation of comprehensive environmental regulations necessitates considerable institutional coordination, \
             particularly regarding the international harmonization of administrative procedures.",
        ));
        assert!(r.flesch < 20., "{}", r.flesch);
        assert!(r.grade() > 16.);
        assert_eq!(r.audience(), "Specialist / academic");
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(measure(&prose("")).flesch, 0.);
    }
}
