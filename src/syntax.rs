//! Syntax Highlight: a small rule-based English part-of-speech tagger.
//! It is a heuristic, not a full tagger: closed word classes come from lists,
//! open classes from suffixes and the previous word.

use std::collections::HashSet;
use std::ops::Range;
use std::sync::OnceLock;

use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Pos {
    Adjective,
    Noun,
    Adverb,
    Verb,
    Conjunction,
}

impl Pos {
    pub const ALL: [Pos; 5] = [Pos::Adjective, Pos::Noun, Pos::Adverb, Pos::Verb, Pos::Conjunction];

}

const CONJUNCTIONS: &str = "and but or nor yet so for although though because since unless while whereas if when whenever where wherever after before once until till than whether as lest";
const DETERMINERS: &str = "a an the this that these those my your his her its our their some any no every each either neither another such what which whose all both half several many much few little more most other";
const PRONOUNS: &str = "i you he she it we they me him us them myself yourself himself herself itself ourselves themselves who whom mine yours hers ours theirs one someone anyone everyone nobody nothing something anything everything there";
const SUBJECTS: &str = "i you he she it we they who that which";
const PREPOSITIONS: &str = "of in on at by with from to into onto about above across against along among around behind below beneath beside between beyond during except inside near off out outside over past through throughout toward towards under underneath unlike upon via within without per";
const AUXILIARIES: &str = "is are was were be been being am have has had having do does did will would shall should can could may might must ought 's 're 've 'd 'll";
const ADVERBS: &str = "very really quite too also always never often sometimes usually still just now then here soon already almost again ever even perhaps rather maybe today tomorrow yesterday tonight away back down up forward together instead indeed however therefore thus otherwise anyway meanwhile not n't seldom rarely yet else once twice somewhat anymore everywhere somewhere nowhere elsewhere later";
const NOT_ADVERBS_LY: &str = "only family reply apply supply fly july italy ally belly bully lily jelly holy ugly silly lovely friendly likely early daily weekly monthly yearly costly lonely lively elderly curly hilly oily rely comply multiply imply";
const ADJECTIVES: &str = "good bad new old great big small large little long short high low young early late important different same able best better worse worst right wrong hard easy strong weak happy sad free full empty sure true false real whole clear open close dark light simple quick slow fast red blue green black white yellow brown gray grey hot cold warm cool rich poor beautiful ugly nice fine clean dirty deep wide narrow thick thin heavy soft loud quiet bright calm safe dangerous public private human local social political economic natural common certain main major minor final general special recent modern ready entire likely lovely friendly lonely silly holy";
const VERBS: &str = "be have do say go get make know think take see come want look use find give tell work call try ask need feel become leave put mean keep let begin seem help talk turn start show hear play run move like live believe hold bring happen write provide sit stand lose pay meet include continue set learn change lead understand watch follow stop create speak read allow add spend grow open walk win offer remember love consider appear buy wait serve die send expect build stay fall cut reach kill remain suggest raise pass sell require report decide pull break explain hope develop carry drive return agree accept describe eat sleep cry laugh choose fly jump sing dance teach draw drink throw catch fight forget forgive hide ride shake shine shoot sink steal strike swim tear wake wear wonder wish fix edit type save delete paste copy print share publish focus check search sort mark";
const IRREGULAR: &str = "said went gone got gotten made knew known thought took taken saw seen came gave given told felt became left meant kept began begun seemed brought wrote written sat stood lost paid met led understood spoke spoken read grew grown won bought built fell fallen cut broke broken drove driven ate eaten slept chose chosen flew flown sang sung drank drunk threw thrown caught fought forgot forgotten hid hidden rode ridden shook shaken shone shot sank stole struck swam tore torn woke woken wore worn ran held heard";

fn set(words: &'static str) -> HashSet<&'static str> {
    words.split_whitespace().collect()
}

struct Lexicon {
    conj: HashSet<&'static str>,
    det: HashSet<&'static str>,
    pron: HashSet<&'static str>,
    subj: HashSet<&'static str>,
    prep: HashSet<&'static str>,
    aux: HashSet<&'static str>,
    adv: HashSet<&'static str>,
    not_ly: HashSet<&'static str>,
    adj: HashSet<&'static str>,
    verb: HashSet<&'static str>,
    irregular: HashSet<&'static str>,
}

fn lex() -> &'static Lexicon {
    static L: OnceLock<Lexicon> = OnceLock::new();
    L.get_or_init(|| Lexicon {
        conj: set(CONJUNCTIONS),
        det: set(DETERMINERS),
        pron: set(PRONOUNS),
        subj: set(SUBJECTS),
        prep: set(PREPOSITIONS),
        aux: set(AUXILIARIES),
        adv: set(ADVERBS),
        not_ly: set(NOT_ADVERBS_LY),
        adj: set(ADJECTIVES),
        verb: set(VERBS),
        irregular: set(IRREGULAR),
    })
}

fn verb_base(w: &str) -> Option<&str> {
    let l = lex();
    if l.verb.contains(w) || l.irregular.contains(w) {
        return Some(w);
    }
    for (suffix, add_e) in [("ing", true), ("ed", true), ("es", false), ("s", false)] {
        if let Some(stem) = w.strip_suffix(suffix) {
            if l.verb.contains(stem) {
                return Some(stem);
            }
            if add_e && l.verb.contains(format!("{stem}e").as_str()) {
                return Some(stem);
            }
            // doubled consonant: stopped, running
            let b = stem.as_bytes();
            if b.len() > 2 && b[b.len() - 1] == b[b.len() - 2] && l.verb.contains(&stem[..stem.len() - 1]) {
                return Some(stem);
            }
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerbForm {
    Base,
    /// -s / -es
    Third,
    /// -ed or an irregular past form
    Past,
    Ing,
}

/// The form and base of a known verb ("goes" → (Third, "go")), used by the
/// Writing Studio grammar rules. Irregular past forms return their own spelling
/// as the base; callers map them with their own table.
pub fn verb_form(w: &str) -> Option<(VerbForm, String)> {
    let l = lex();
    if l.verb.contains(w) {
        return Some((VerbForm::Base, w.to_string()));
    }
    if l.irregular.contains(w) {
        return Some((VerbForm::Past, w.to_string()));
    }
    for (suffix, form) in [("ing", VerbForm::Ing), ("ied", VerbForm::Past), ("ies", VerbForm::Third), ("ed", VerbForm::Past), ("es", VerbForm::Third), ("s", VerbForm::Third)] {
        let Some(stem) = w.strip_suffix(suffix) else { continue };
        if stem.len() < 2 {
            continue;
        }
        let mut candidates = vec![stem.to_string(), format!("{stem}e")];
        if suffix.starts_with('i') {
            candidates.insert(0, format!("{stem}y"));
        }
        let b = stem.as_bytes();
        if b.len() > 2 && b[b.len() - 1] == b[b.len() - 2] {
            candidates.push(stem[..stem.len() - 1].to_string());
        }
        if let Some(base) = candidates.into_iter().find(|c| l.verb.contains(c.as_str())) {
            return Some((form, base));
        }
    }
    None
}

pub fn is_adjective(w: &str) -> bool {
    lex().adj.contains(w)
}

fn has_suffix(w: &str, list: &[&str]) -> bool {
    list.iter().any(|s| w.len() > s.len() + 2 && w.ends_with(s))
}

const NOUN_SUFFIXES: &[&str] = &["tion", "sion", "ment", "ness", "ity", "ism", "ist", "ship", "hood", "ance", "ence", "dom", "age", "ery", "logy"];
const ADJ_SUFFIXES: &[&str] = &["ous", "ful", "ive", "able", "ible", "less", "ish", "ical", "ic", "al", "ant", "ent", "ary", "est"];

#[derive(Clone, Copy, PartialEq)]
enum Prev {
    Start,
    Det,
    Subject,
    Aux,
    Prep,
    To,
    Adj,
    Noun,
    Verb,
    Other,
}

fn is_function_word(w: &str) -> bool {
    let l = lex();
    l.conj.contains(w)
        || l.det.contains(w)
        || l.pron.contains(w)
        || l.prep.contains(w)
        || l.aux.contains(w)
        || w == "to"
}

/// Tag every word of `text`; returns (byte range, pos) for colored words only.
pub fn tag(text: &str) -> Vec<(Range<usize>, Pos)> {
    let l = lex();
    // (range, lowercase word, capitalized, starts sentence)
    let mut words: Vec<(Range<usize>, String, bool, bool)> = Vec::new();
    let mut sentence_start = true;
    for (i, tok) in text.split_word_bound_indices() {
        if !tok.chars().next().is_some_and(char::is_alphabetic) {
            if tok.contains(['.', '!', '?', '\n', ':', ';']) {
                sentence_start = true;
            }
            continue;
        }
        let lower = tok.to_lowercase();
        let w = lower.trim_end_matches("'s").trim_end_matches("’s").to_string();
        let cap = tok.chars().next().is_some_and(char::is_uppercase);
        words.push((i..i + tok.len(), w, cap, sentence_start));
        sentence_start = false;
    }

    let mut out = Vec::new();
    let mut prev = Prev::Start;
    for (k, (r, w, capitalized, starts)) in words.iter().enumerate() {
        let w = w.as_str();
        if *starts {
            prev = Prev::Start;
        }
        // A content word followed by another content word, after a determiner: adjective.
        let next_is_content = words
            .get(k + 1)
            .is_some_and(|n| {
                !n.3 && !is_function_word(&n.1)
                    && !l.adv.contains(n.1.as_str())
                    && verb_base(&n.1).is_none()
            });

        let (pos, next) = if l.conj.contains(w) && !(w == "for" || w == "as") {
            (Some(Pos::Conjunction), Prev::Other)
        } else if l.det.contains(w) {
            (None, Prev::Det)
        } else if l.subj.contains(w) && prev != Prev::Det {
            (None, Prev::Subject)
        } else if l.pron.contains(w) {
            (None, Prev::Noun)
        } else if w == "to" {
            (None, Prev::To)
        } else if l.prep.contains(w) {
            (None, Prev::Prep)
        } else if l.aux.contains(w) {
            (Some(Pos::Verb), Prev::Aux)
        } else if l.adv.contains(w) || (w.ends_with("ly") && w.len() > 4 && !l.not_ly.contains(w)) {
            (Some(Pos::Adverb), prev)
        } else if *capitalized && !starts {
            (Some(Pos::Noun), Prev::Noun)
        } else if l.adj.contains(w) && !matches!(prev, Prev::Subject | Prev::To) {
            (Some(Pos::Adjective), Prev::Adj)
        } else if prev == Prev::Aux && verb_base(w).is_none() && !w.ends_with("ed") && !w.ends_with("ing") && !next_is_content {
            // Predicate adjective: "the dog is (very) hungry".
            (Some(Pos::Adjective), Prev::Adj)
        } else if matches!(prev, Prev::Subject | Prev::Aux | Prev::To) && verb_base(w).is_some() {
            (Some(Pos::Verb), Prev::Verb)
        } else if matches!(prev, Prev::Det | Prev::Adj | Prev::Prep) {
            if (has_suffix(w, ADJ_SUFFIXES) && !has_suffix(w, NOUN_SUFFIXES)) || next_is_content {
                (Some(Pos::Adjective), Prev::Adj)
            } else {
                (Some(Pos::Noun), Prev::Noun)
            }
        } else if has_suffix(w, NOUN_SUFFIXES) {
            (Some(Pos::Noun), Prev::Noun)
        } else if verb_base(w).is_some() && matches!(prev, Prev::Noun | Prev::Start) {
            (Some(Pos::Verb), Prev::Verb)
        } else if has_suffix(w, ADJ_SUFFIXES) {
            (Some(Pos::Adjective), Prev::Adj)
        } else if prev == Prev::Noun && (w.ends_with('s') || w.ends_with("ed")) {
            (Some(Pos::Verb), Prev::Verb)
        } else if prev == Prev::Verb && verb_base(w).is_none() {
            (Some(Pos::Noun), Prev::Noun)
        } else if verb_base(w).is_some() {
            (Some(Pos::Verb), Prev::Verb)
        } else {
            (Some(Pos::Noun), Prev::Noun)
        };
        if let Some(p) = pos {
            out.push((r.clone(), p));
        }
        prev = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos_of(text: &str, word: &str) -> Option<Pos> {
        let i = text.find(word).unwrap();
        tag(text).into_iter().find(|(r, _)| r.start == i).map(|(_, p)| p)
    }

    #[test]
    fn simple_sentence() {
        let t = "The quick brown fox jumps over the lazy dog and runs away.";
        assert_eq!(pos_of(t, "quick"), Some(Pos::Adjective));
        assert_eq!(pos_of(t, "fox"), Some(Pos::Noun));
        assert_eq!(pos_of(t, "jumps"), Some(Pos::Verb));
        assert_eq!(pos_of(t, "lazy"), Some(Pos::Adjective));
        assert_eq!(pos_of(t, "dog"), Some(Pos::Noun));
        assert_eq!(pos_of(t, "and"), Some(Pos::Conjunction));
        assert_eq!(pos_of(t, "runs"), Some(Pos::Verb));
        assert_eq!(pos_of(t, "away"), Some(Pos::Adverb));
        assert_eq!(pos_of(t, "The"), None);
        assert_eq!(pos_of(t, "over"), None);
    }

    #[test]
    fn subject_verb_and_ly_adverbs() {
        let t = "She quickly wrote a beautiful letter because we asked.";
        assert_eq!(pos_of(t, "quickly"), Some(Pos::Adverb));
        assert_eq!(pos_of(t, "wrote"), Some(Pos::Verb));
        assert_eq!(pos_of(t, "beautiful"), Some(Pos::Adjective));
        assert_eq!(pos_of(t, "letter"), Some(Pos::Noun));
        assert_eq!(pos_of(t, "because"), Some(Pos::Conjunction));
        assert_eq!(pos_of(t, "asked"), Some(Pos::Verb));
    }

    #[test]
    fn suffixes() {
        let t = "It is basically very hungry.";
        assert_eq!(pos_of(t, "hungry"), Some(Pos::Adjective));
        let t = "Their organization is enormous.";
        assert_eq!(pos_of(t, "organization"), Some(Pos::Noun));
        assert_eq!(pos_of(t, "is"), Some(Pos::Verb));
        assert_eq!(pos_of(t, "enormous"), Some(Pos::Adjective));
    }
}
