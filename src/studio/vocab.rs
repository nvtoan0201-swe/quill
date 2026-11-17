//! Vocabulary: lexical diversity, academic vocabulary, linking words, and
//! word-level issues (weak intensifiers, vague and overused words).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use super::rules::{Ctx, find_phrase};
use super::text::{Prose, syllables};
use super::{Issue, Severity};

pub const STOPWORDS: &str = "a an the and or but if then so of in on at by for with about to from into over under as is are was were be been being am do does did have has had having i you he she it we they me him her us them my your his its our their this that these those there here what which who whom whose when where why how not no nor all any both each few more most other some such only own same than too very can will just should would could may might must shall also up down out off again further once one two s t don't it's i'm i've you're that's isn't aren't wasn't weren't won't can't didn't doesn't let let's";

/// Coxhead's Academic Word List headwords (abridged). Matching is by prefix, so
/// "analyse" covers analysis, analytical, analysed.
const AWL: &str = "analys approach area assess assum authorit availab benefi concept consist constitu context contract creat data defin deriv distribut econom environment establish estimat eviden export factor financ formula function identif income indicat individual interpret involv issue labour legal legislat major method occur percent period polic principle proceed process requir research respond role section sector significan similar source specific structur theor vari achiev acquir administrat affect appropriat aspect assist categor chapter commission communit complex comput conclu conduct consequen construct consum credit cultur design distinct element equat evaluat feature final focus impact institut invest item journal maintain normal obtain participat perceiv positive potential previous primar purchas range region regulat relevan resid resourc restrict secur seek select site strateg survey tradition transfer alternativ circumstanc comment compensat component consent considerab constant constrain contribut coordinat core corporat correspond criteri deduc demonstrat document dominat emphas ensur exclu framework fund illustrat immigra impl initial instanc interact justif layer link locat maximi minor negat outcom partner philosoph physical proportion publish react regist rely remov scheme sequen shift specif sufficien task technical techni technolog valid volum access adequa annual apparen approximat attitud attribut civil code commit communicat concentrat confer contrast cycl debat despite dimension domestic emerg error ethnic goal grant hence hypothes implement implicat impos integrat internal investigat label mechanism obvious occup option output overall parallel paramet phase predict principal prior professional project promot regim resolv retain series statistic status stress subsequen summar undertak academ adjust alter amend aware capacit challeng clause compound conflict consult contact declin discret draft enabl energ enforc entit equivalen evolv expan expos external facilitat fundamental generat imag liberal licen logic marginal medical mental modif monitor network notion objectiv orient perspectiv precis prime psycholog pursu ratio reject revenu stabl style substitut sustain symbol target transit trend version welfar whereas abstract accura acknowledg aggregat allocat assign attach author bond brief capab cite cooperat discriminat display divers domain edit enhanc estat exceed expert explicit federal fee flexib furthermore gender ignoran incentiv inciden incorporat index inhibit initiat input instruct intellig interval lectur migrat minimum ministr motiv neutral nevertheless overseas preced presum rational recover reveal scope subsid trac transform transport underl utili adapt advocat channel chemical classic comprehensiv compris confirm contrar convert decad definit deny differentiat dispos dynamic eliminat empiric equip extract finit foundation glob grade guarante hierarch identical ideolog infer innovat insert interven isolat media mode paradigm phenomen priorit prohibit quot releas revers simulat sole somewhat submit successor surviv thesis topic transmit ultimat unique visib voluntar abandon accompan accumulat ambigu append appreciat arbitrar automat bias clarif commodit complement conform contemporar contradict crucial currenc denot detect deviat displac eventual exhibit exploit fluctuat guideline highlight implicit induc inevitab infrastructur inspect intens manipulat minimi nuclear offset practition predominan prospect radical random reinforc restor revis schedul tension terminat theme thereby uniform vehicl via virtual visual widespread accommodat analog anticipat assur attain behalf bulk ceas coheren coincid commenc compatib concurren confin controvers devic devot diminish distort duration erod ethic format inheren insight integral intermediat manual matur mediat medium militar minimal mutual norm overlap passiv portion preliminar protocol qualitativ refin relax restrain revolution rigid rout scenario spher subordinat supplement suspend temporar trigger unif violat vision adjacen albeit assembl collaps colleagu compil conceiv convinc encounter enormous forthcoming inclin integrit intrinsic invok likewise nonetheless notwithstanding ongoing panel persist pose reluctan straightforward undergo whereby";

const LINKERS: &[(&str, &str)] = &[
    // (phrase, function)
    ("however", "contrast"), ("nevertheless", "contrast"), ("nonetheless", "contrast"), ("on the other hand", "contrast"),
    ("in contrast", "contrast"), ("whereas", "contrast"), ("although", "contrast"), ("despite", "contrast"), ("yet", "contrast"),
    ("conversely", "contrast"), ("moreover", "addition"), ("furthermore", "addition"), ("in addition", "addition"),
    ("additionally", "addition"), ("besides", "addition"), ("also", "addition"), ("therefore", "result"), ("thus", "result"),
    ("consequently", "result"), ("as a result", "result"), ("hence", "result"), ("accordingly", "result"),
    ("for example", "example"), ("for instance", "example"), ("such as", "example"), ("namely", "example"),
    ("firstly", "sequence"), ("first", "sequence"), ("secondly", "sequence"), ("finally", "sequence"), ("subsequently", "sequence"),
    ("meanwhile", "sequence"), ("in conclusion", "conclusion"), ("to sum up", "conclusion"), ("overall", "conclusion"),
    ("in summary", "conclusion"), ("because", "cause"), ("since", "cause"), ("due to", "cause"), ("owing to", "cause"),
    ("in fact", "emphasis"), ("indeed", "emphasis"), ("in particular", "emphasis"), ("notably", "emphasis"),
    ("similarly", "comparison"), ("likewise", "comparison"), ("in the same way", "comparison"),
];

/// "very X" → stronger single words.
const INTENSIFIED: &[(&str, &str)] = &[
    ("good", "excellent, outstanding"), ("bad", "terrible, awful"), ("big", "huge, enormous"), ("large", "vast, enormous"),
    ("small", "tiny, minute"), ("happy", "delighted, thrilled"), ("sad", "miserable, heartbroken"), ("tired", "exhausted"),
    ("important", "crucial, essential, vital"), ("difficult", "challenging, arduous"), ("hard", "challenging, gruelling"),
    ("easy", "effortless, straightforward"), ("fast", "rapid, swift"), ("quick", "rapid, swift"), ("old", "ancient, aged"),
    ("hungry", "starving, ravenous"), ("angry", "furious, livid"), ("scared", "terrified"), ("afraid", "terrified"),
    ("beautiful", "stunning, gorgeous"), ("clean", "spotless"), ("cold", "freezing, frigid"), ("hot", "scorching, sweltering"),
    ("interesting", "fascinating, compelling"), ("smart", "brilliant"), ("clever", "brilliant, ingenious"),
    ("many", "numerous, countless"), ("quiet", "silent, hushed"), ("rich", "wealthy, affluent"), ("short", "brief"),
    ("strong", "powerful, robust"), ("ugly", "hideous"), ("wet", "soaked, drenched"), ("loud", "deafening, thunderous"),
    ("clear", "obvious, evident"), ("bright", "brilliant, dazzling"), ("dirty", "filthy"), ("funny", "hilarious"),
    ("poor", "destitute, impoverished"), ("serious", "grave, severe"), ("surprised", "astonished, astounded"),
    ("different", "distinct, dissimilar"), ("necessary", "essential, indispensable"), ("useful", "invaluable"),
    ("famous", "renowned, celebrated"), ("popular", "widespread, acclaimed"), ("expensive", "costly, exorbitant"),
    ("cheap", "inexpensive, affordable"), ("busy", "swamped, hectic"), ("careful", "meticulous, cautious"),
    ("worried", "anxious, distressed"), ("high", "soaring, elevated"), ("low", "minimal, negligible"),
];

const VAGUE: &[(&str, &str)] = &[
    ("thing", "name the specific object, idea, factor or aspect"),
    ("things", "name them: factors, items, aspects, issues…"),
    ("stuff", "say what it is: materials, belongings, information…"),
    ("nice", "pleasant, attractive, kind, enjoyable — which one?"),
    ("a lot of", "many / much / a large number of / considerable"),
    ("lots of", "many / much / numerous / plenty of"),
    ("good", "beneficial, effective, valuable, high-quality — be specific"),
    ("bad", "harmful, ineffective, poor, damaging — be specific"),
    ("big", "significant, major, substantial, large-scale"),
    ("get", "obtain, receive, become, acquire, reach — choose the precise verb"),
    ("got", "obtained, received, became, acquired"),
];

const INFORMAL: &[(&str, &str)] = &[
    ("gonna", "going to"), ("wanna", "want to"), ("gotta", "have to"), ("kinda", "rather"), ("sorta", "somewhat"),
    ("kids", "children"), ("guys", "people"), ("awesome", "impressive"), ("cool", "appealing"), ("okay", "acceptable"),
    ("ok", "acceptable"), ("tons of", "a great deal of"), ("loads of", "a great deal of"), ("pretty good", "fairly good"),
    ("really", "genuinely"), ("stuff", "material"), ("folks", "people"), ("anyways", "anyway"), ("nowadays", "today"),
    ("a bit", "slightly"), ("totally", "entirely"), ("huge", "substantial"), ("get rid of", "eliminate"),
    ("find out", "discover"), ("look into", "investigate"), ("come up with", "devise"), ("put up with", "tolerate"),
    ("cut down on", "reduce"), ("go up", "increase"), ("go down", "decrease"),
];

/// Alternatives offered for overused words.
pub const SYNONYMS: &[(&str, &str)] = &[
    ("important", "significant, crucial, essential, key"), ("show", "demonstrate, reveal, indicate, illustrate"),
    ("shows", "demonstrates, reveals, indicates"), ("think", "believe, consider, argue, maintain"),
    ("use", "employ, apply, utilise, rely on"), ("people", "individuals, citizens, the public, residents"),
    ("problem", "issue, challenge, difficulty, concern"), ("problems", "issues, challenges, difficulties"),
    ("help", "assist, support, enable, facilitate"), ("change", "shift, transformation, alteration"),
    ("increase", "rise, growth, surge, climb"), ("decrease", "decline, drop, fall, reduction"),
    ("idea", "notion, concept, proposal, view"), ("say", "state, claim, argue, note"), ("make", "create, produce, generate, cause"),
    ("many", "numerous, various, a wide range of"), ("also", "moreover, in addition, furthermore"),
    ("because", "since, as, due to, owing to"), ("but", "however, yet, nevertheless"), ("so", "therefore, thus, consequently"),
    ("very", "(choose a stronger adjective)"), ("good", "beneficial, effective, valuable"), ("bad", "harmful, detrimental, poor"),
    ("big", "major, substantial, considerable"), ("example", "instance, illustration, case"), ("way", "method, means, approach"),
    ("result", "outcome, consequence, effect"), ("effect", "impact, influence, consequence"),
    ("benefit", "advantage, gain, merit"), ("benefits", "advantages, merits, gains"), ("need", "require, demand, call for"),
];

fn set(words: &'static str) -> HashSet<&'static str> {
    words.split_whitespace().collect()
}

fn stopwords() -> &'static HashSet<&'static str> {
    static S: OnceLock<HashSet<&'static str>> = OnceLock::new();
    S.get_or_init(|| set(STOPWORDS))
}

fn awl() -> &'static Vec<&'static str> {
    static A: OnceLock<Vec<&'static str>> = OnceLock::new();
    A.get_or_init(|| AWL.split_whitespace().collect())
}

pub fn is_stopword(w: &str) -> bool {
    stopwords().contains(w)
}

pub fn is_academic(w: &str) -> bool {
    w.len() >= 4 && awl().iter().any(|a| w.starts_with(a))
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VocabStats {
    pub words: usize,
    pub unique: usize,
    pub content_words: usize,
    /// Measure of Textual Lexical Diversity; ~70+ is rich, below ~50 repetitive.
    pub mtld: f32,
    /// Share of content words from the Academic Word List.
    pub academic_ratio: f32,
    /// Share of content words with 3+ syllables.
    pub long_word_ratio: f32,
    pub avg_word_len: f32,
    /// Linking words used, and how many distinct functions they cover.
    pub linkers: Vec<(String, usize)>,
    pub linker_functions: usize,
    /// Most repeated content words (word, count).
    pub top_words: Vec<(String, usize)>,
}

fn mtld_pass<'a>(words: impl Iterator<Item = &'a str>) -> f32 {
    const T: f32 = 0.72;
    let mut factors = 0.0f32;
    let mut types: HashSet<&str> = HashSet::new();
    let mut tokens = 0usize;
    for w in words {
        tokens += 1;
        types.insert(w);
        let ttr = types.len() as f32 / tokens as f32;
        if ttr <= T {
            factors += 1.;
            types.clear();
            tokens = 0;
        }
    }
    if tokens > 0 {
        let ttr = types.len() as f32 / tokens as f32;
        factors += (1. - ttr) / (1. - T);
    }
    factors
}

/// MTLD (McCarthy & Jarvis 2010), averaged over forward and backward passes.
pub fn mtld(words: &[&str]) -> f32 {
    if words.len() < 10 {
        return 0.;
    }
    let n = words.len() as f32;
    let f = mtld_pass(words.iter().copied());
    let b = mtld_pass(words.iter().rev().copied());
    let fwd = if f > 0. { n / f } else { n };
    let bwd = if b > 0. { n / b } else { n };
    (fwd + bwd) / 2.
}

pub fn stats(p: &Prose) -> VocabStats {
    let words: Vec<&str> = p.body().flat_map(|s| p.sentence_words(s)).map(|w| w.lower.as_str()).collect();
    let mut v = VocabStats { words: words.len(), ..Default::default() };
    if words.is_empty() {
        return v;
    }
    v.unique = words.iter().collect::<HashSet<_>>().len();
    v.mtld = mtld(&words);
    v.avg_word_len = words.iter().map(|w| w.chars().count()).sum::<usize>() as f32 / words.len() as f32;
    let content: Vec<&str> = words.iter().copied().filter(|w| !is_stopword(w) && w.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-')).collect();
    v.content_words = content.len();
    if !content.is_empty() {
        v.academic_ratio = content.iter().filter(|w| is_academic(w)).count() as f32 / content.len() as f32;
        v.long_word_ratio = content.iter().filter(|w| syllables(w) >= 3).count() as f32 / content.len() as f32;
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for w in &content {
        *counts.entry(w).or_default() += 1;
    }
    let mut top: Vec<(String, usize)> = counts.into_iter().filter(|(_, c)| *c >= 3).map(|(w, c)| (w.to_string(), c)).collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(8);
    v.top_words = top;
    let mut functions = HashSet::new();
    for (l, f) in LINKERS {
        let n = find_phrase(p, l).len();
        if n > 0 {
            v.linkers.push((l.to_string(), n));
            functions.insert(*f);
        }
    }
    v.linkers.sort_by(|a, b| b.1.cmp(&a.1));
    v.linker_functions = functions.len();
    v
}

pub fn check(cx: &mut Ctx, formal: bool) {
    let p = cx.p;
    // very + adjective
    for i in 0..p.words.len().saturating_sub(1) {
        if !matches!(p.word(i), "very" | "really" | "extremely" | "so") {
            continue;
        }
        let j = i + 1;
        let gap = &p.text[p.words[i].range.end..p.words[j].range.start];
        if gap != " " {
            continue;
        }
        if let Some((adj, strong)) = INTENSIFIED.iter().find(|(a, _)| *a == p.word(j)) {
            let orig = &cx.src[p.words[i].range.start..p.words[j].range.end];
            let msg = format!("“{orig}” → one precise word: {strong}.");
            let sugg: Vec<String> = strong.split(", ").map(str::to_string).collect();
            let r = p.words[i].range.start..p.words[j].range.end;
            if p.word(i) != "so" || formal {
                cx_push(cx, "vocab.intensifier", r, Severity::Hint, msg, sugg);
            }
            let _ = adj;
        }
    }
    for (w, advice) in VAGUE {
        // "good"/"bad"/"big"/"get" are normal in casual writing; only flag in formal text.
        if !formal && matches!(*w, "good" | "bad" | "big" | "get" | "got" | "a lot of" | "lots of") {
            continue;
        }
        for (a, b) in find_phrase(p, w) {
            // "very good" is already reported as an intensifier.
            if a > 0 && p.word(a - 1) == "very" {
                continue;
            }
            let r = p.words[a].range.start..p.words[b].range.end;
            let orig = cx.src[r.clone()].to_string();
            cx_push(cx, "vocab.vague", r, Severity::Hint, format!("“{orig}” is vague: {advice}."), vec![]);
        }
    }
    if formal {
        for (w, repl) in INFORMAL {
            for (a, b) in find_phrase(p, w) {
                let r = p.words[a].range.start..p.words[b].range.end;
                let orig = cx.src[r.clone()].to_string();
                cx_push(cx, "tone.informal", r, Severity::Warning, format!("“{orig}” is informal; in formal writing try “{repl}”."), vec![repl.to_string()]);
            }
        }
    }
    // Overused: a content word used again within 30 words, the third time or more.
    let mut last: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, w) in p.words.iter().enumerate() {
        let l = w.lower.as_str();
        if is_stopword(l) || l.len() < 4 || l.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        let seen = last.entry(l).or_default();
        seen.retain(|&k| i - k <= 40);
        seen.push(i);
        if seen.len() >= 3 {
            let alt = SYNONYMS.iter().find(|(s, _)| *s == l).map(|(_, a)| *a);
            let msg = match alt {
                Some(a) => format!("“{}” appears {} times nearby. Alternatives: {a}.", cx.src[w.range.clone()].to_string(), seen.len()),
                None => format!("“{}” appears {} times nearby. Vary it or use a pronoun.", &cx.src[w.range.clone()], seen.len()),
            };
            let sugg = alt.map(|a| a.split(", ").filter(|s| !s.starts_with('(')).map(str::to_string).collect()).unwrap_or_default();
            cx_push(cx, "vocab.overused", w.range.clone(), Severity::Hint, msg, sugg);
        }
    }
}

fn cx_push(cx: &mut Ctx, rule: &'static str, range: std::ops::Range<usize>, sev: Severity, msg: String, sugg: Vec<String>) {
    if cx.out.iter().any(|i| i.range == range && i.rule == rule) {
        return;
    }
    cx.out.push(Issue { rule, range, message: msg, suggestions: sugg, severity: sev });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::text::prose;

    fn issues(t: &str, formal: bool) -> Vec<(&'static str, String)> {
        let p = prose(t);
        let mut cx = Ctx { src: t, p: &p, out: Vec::new() };
        check(&mut cx, formal);
        cx.out.into_iter().map(|i| (i.rule, t[i.range].to_string())).collect()
    }

    #[test]
    fn intensifiers_and_vague() {
        let v = issues("It was a very big day with lots of stuff.", false);
        assert!(v.contains(&("vocab.intensifier", "very big".into())));
        assert!(v.contains(&("vocab.vague", "stuff".into())));
        assert!(!v.iter().any(|x| x.1 == "lots of"));
        let v = issues("Kids get lots of stuff.", true);
        assert!(v.contains(&("tone.informal", "Kids".into())) && v.contains(&("vocab.vague", "lots of".into())));
    }

    #[test]
    fn overused() {
        let v = issues("The problem is real. This problem matters. The problem grows.", false);
        assert_eq!(v.iter().filter(|x| x.0 == "vocab.overused").count(), 1);
    }

    #[test]
    fn diversity_and_academic() {
        let rich = stats(&prose("The analysis demonstrates a significant correlation between economic policy and environmental outcomes, although further research is required to establish causation."));
        assert!(rich.academic_ratio > 0.4, "{}", rich.academic_ratio);
        let poor = stats(&prose("The dog is a dog and the dog is a good dog and a dog is a dog."));
        let varied = stats(&prose("Autumn leaves drift across quiet streets while children chase scattered kites beneath pale clouds."));
        assert!(poor.mtld < varied.mtld, "{} {}", poor.mtld, varied.mtld);
        let l = stats(&prose("However, prices rose. Moreover, wages fell. For example, rent doubled."));
        assert_eq!(l.linker_functions, 3);
    }

    #[test]
    fn awl_prefixes() {
        assert!(is_academic("analysis") && is_academic("significantly") && !is_academic("dog"));
    }
}
