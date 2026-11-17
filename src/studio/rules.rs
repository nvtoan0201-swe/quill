//! Rule-based checks for grammar, spelling, punctuation, clarity and sentence
//! structure. Each rule has a short lesson (`RULES`) that the Studio shows
//! next to the finding, so the writer learns the rule instead of just accepting
//! a fix. The rules aim for precision over recall: a missed mistake costs less
//! than teaching a learner something wrong.

use std::collections::HashSet;
use std::ops::Range;

use super::text::Prose;
use super::{Category, Issue, Severity};
use crate::syntax::{self, VerbForm};

pub struct RuleInfo {
    pub id: &'static str,
    pub category: Category,
    pub title: &'static str,
    /// The rule, explained for a learner.
    pub lesson: &'static str,
    pub wrong: &'static str,
    pub right: &'static str,
}

macro_rules! rule {
    ($id:literal, $cat:ident, $title:literal, $lesson:literal, $wrong:literal, $right:literal) => {
        RuleInfo { id: $id, category: Category::$cat, title: $title, lesson: $lesson, wrong: $wrong, right: $right }
    };
}

pub static RULES: &[RuleInfo] = &[
    rule!("spelling.common", Spelling, "Commonly misspelled word",
        "Some words are misspelled so often that the wrong spelling looks right. Learn the correct form by its tricky part: 'i before e except after c' (receive), double letters (accommodate, occurred), and silent letters (definitely).",
        "I recieved a seperate message.", "I received a separate message."),
    rule!("spelling.dictionary", Spelling, "Unknown word",
        "This word is not in the English dictionary. It may be a typo, or a name or technical term — if so, ignore it.",
        "The resluts were clear.", "The results were clear."),
    rule!("grammar.article", Grammar, "a / an",
        "Use 'an' before a vowel SOUND and 'a' before a consonant SOUND. It is the sound that matters, not the letter: an hour (silent h), a university (sounds like 'you'), an MBA ('em').",
        "She is a honest person with an unique idea.", "She is an honest person with a unique idea."),
    rule!("grammar.repeated-word", Grammar, "Repeated word",
        "The same word appears twice in a row. This usually happens when editing a sentence.",
        "We went to the the park.", "We went to the park."),
    rule!("grammar.agreement", Grammar, "Subject–verb agreement",
        "A verb must agree with its subject. Singular third person (he, she, it, a person) takes -s in the present: he works, she has, it does. I/you/we/they take the base form: they work, we have. With 'was/were': I/he/she/it was, you/we/they were.",
        "She have two brothers and they was happy.", "She has two brothers and they were happy."),
    rule!("grammar.verb-form", Grammar, "Verb form after an auxiliary",
        "After a modal (can, will, should, must…) or do/does/did, use the base verb: no -s, no -ed, no 'to'. The auxiliary already carries the tense and person.",
        "He didn't went home. She can speaks French. You must to go.", "He didn't go home. She can speak French. You must go."),
    rule!("grammar.confused", Grammar, "Commonly confused words",
        "Some words sound alike but mean different things: its (belonging to it) / it's (it is); your (belonging to you) / you're (you are); their (belonging to them) / there (place) / they're (they are); than (comparison) / then (time); could have, never 'could of'.",
        "Its clear that your right, more then before.", "It's clear that you're right, more than before."),
    rule!("grammar.uncountable", Grammar, "Uncountable noun",
        "Uncountable nouns (information, advice, research, equipment, furniture, knowledge, evidence, feedback, homework, luggage, software) have no plural and do not take 'a/an'. Use 'some', 'a piece of', or 'much'.",
        "She gave me an advice and many informations.", "She gave me some advice and a lot of information."),
    rule!("grammar.plural", Grammar, "Singular noun where a plural is needed",
        "After numbers above one and words like many, several, few, various, both, these, those and 'one of the', a countable noun must be plural.",
        "There are many student in one of the best school.", "There are many students in one of the best schools."),
    rule!("grammar.quantifier", Grammar, "much / many, less / fewer",
        "Use 'many' and 'fewer' with countable plurals (many people, fewer cars); use 'much' and 'less' with uncountable nouns (much time, less money).",
        "There were much problems and less people.", "There were many problems and fewer people."),
    rule!("grammar.capitalization", Grammar, "Capitalization",
        "Capitalize the first word of a sentence, the pronoun 'I', and proper nouns such as days, months, languages and nationalities (Monday, July, English, Vietnamese).",
        "i study english on monday. my teacher is kind.", "I study English on Monday. My teacher is kind."),
    rule!("punct.spacing", Punctuation, "Spacing around punctuation",
        "In English, commas, periods, colons, semicolons, question and exclamation marks attach to the word before them, with one space after.",
        "Hello , world.It is me !", "Hello, world. It is me!"),
    rule!("punct.repeated", Punctuation, "Repeated punctuation",
        "Use one punctuation mark. Doubled marks (!!, ??, ,,) look informal, and two dots are not an ellipsis (use three: …).",
        "Really?? That is great!!", "Really? That is great!"),
    rule!("punct.missing-end", Punctuation, "Missing end punctuation",
        "A sentence ends with a period, question mark or exclamation mark.",
        "The results were surprising", "The results were surprising."),
    rule!("structure.comma-splice", Structure, "Comma splice",
        "Two complete sentences joined only by a comma form a comma splice. Join them with a period, a semicolon, or a comma plus a conjunction (and, but, so). With 'however' or 'therefore', use a semicolon before and a comma after.",
        "It was raining, we stayed inside. The plan failed, however we learned a lot.", "It was raining, so we stayed inside. The plan failed; however, we learned a lot."),
    rule!("structure.fragment", Structure, "Sentence fragment",
        "A clause starting with because, although, which, unless or whereas cannot stand alone; it needs a main clause. Attach it to the sentence before or after.",
        "We stayed inside. Because it was raining.", "We stayed inside because it was raining."),
    rule!("structure.run-on", Structure, "Run-on sentence",
        "A long chain of clauses joined by 'and' is hard to follow. Split it, or use subordination (because, while, which) to show how the ideas relate.",
        "We went to the shop and we bought food and then we went home and we cooked and ate.", "After buying food at the shop, we went home to cook."),
    rule!("structure.long-sentence", Structure, "Very long sentence",
        "Sentences over about 30 words make the reader hold too much in mind. Readers understand best when most sentences have 15–20 words, with some variety. Split at a natural break or remove extra words.",
        "(a 45-word sentence)", "(two sentences of 20 words)"),
    rule!("structure.repetitive-start", Structure, "Repetitive sentence openings",
        "Several sentences in a row start with the same word. Vary openings with a time phrase, a subordinate clause, or a different subject to keep the rhythm alive.",
        "I woke up. I ate breakfast. I went to work.", "I woke up early. After breakfast, I went to work."),
    rule!("structure.paragraph", Structure, "Very long paragraph",
        "Paragraphs over about 150–180 words often contain more than one idea. Give each paragraph one main idea with a clear topic sentence.",
        "(one 250-word paragraph)", "(two focused paragraphs)"),
    rule!("clarity.passive", Clarity, "Passive voice",
        "In the passive voice the subject receives the action (The report was written by Anna). Active voice (Anna wrote the report) is usually shorter and clearer. Passive is fine when the actor is unknown or unimportant, as in scientific methods.",
        "The decision was made by the committee.", "The committee made the decision."),
    rule!("clarity.wordy", Clarity, "Wordy phrase",
        "Long phrases often have a one-word equivalent. Concise writing respects the reader's time.",
        "Due to the fact that it rained, in order to stay dry…", "Because it rained, to stay dry…"),
    rule!("clarity.nominalization", Clarity, "Hidden verb (nominalization)",
        "A 'hidden verb' turns an action into a noun with a weak verb: make a decision, give an explanation. Use the verb itself: decide, explain. Sentences become shorter and more direct.",
        "We made a decision to conduct an analysis of the data.", "We decided to analyse the data."),
    rule!("clarity.redundant", Clarity, "Redundancy",
        "Some phrases say the same thing twice (end result, past history, revert back). Keep the word that carries the meaning.",
        "The end result was a free gift.", "The result was a gift."),
    rule!("clarity.filler", Clarity, "Filler word",
        "Words like basically, actually, really and literally rarely add meaning. Remove them, or replace the weak word they prop up with a stronger one.",
        "It was basically really important.", "It was essential."),
    rule!("clarity.unsupported", Clarity, "Unsupported claim",
        "Phrases like 'some people say' or 'studies show' hide who said it. Name the source or give evidence; vague attributions weaken an argument.",
        "Studies show that coffee is healthy.", "A 2019 Harvard study of 10,000 adults found that…"),
    rule!("clarity.hedge", Clarity, "Stacked hedges",
        "One hedge (may, perhaps) signals appropriate caution. Two in a row (might possibly) make the writer sound unsure. Keep one.",
        "This might possibly help.", "This might help."),
    rule!("vocab.intensifier", Vocabulary, "Weak intensifier",
        "'Very' + adjective is often weaker than one precise adjective: very big → huge, very tired → exhausted. Strong single words make writing vivid and score higher for lexical resource.",
        "It was a very big and very important day.", "It was a momentous day."),
    rule!("vocab.vague", Vocabulary, "Vague word",
        "Words like thing, stuff, nice, good and a lot say little. Name the specific thing or quality.",
        "There are a lot of good things about it.", "It has three clear advantages: …"),
    rule!("vocab.overused", Vocabulary, "Overused word",
        "Repeating the same content word close together makes writing monotonous. Use a synonym, a pronoun, or restructure the sentence.",
        "The problem is important. This problem shows the problem of…", "The problem is important. It shows the difficulty of…"),
    rule!("tone.contraction", Tone, "Contraction in formal writing",
        "Formal and academic writing (IELTS, essays, reports) avoids contractions: write 'do not', 'it is', 'cannot'.",
        "It's clear that we can't wait.", "It is clear that we cannot wait."),
    rule!("tone.informal", Tone, "Informal word",
        "Some words are fine in conversation but too casual for formal writing: gonna, kids, stuff, a lot of, pretty good, awesome. Choose a neutral or academic equivalent.",
        "Kids get a lot of stuff from TV.", "Children acquire considerable knowledge from television."),
    rule!("tone.conjunction-start", Tone, "Sentence starts with a conjunction",
        "Starting a sentence with And, But or So is common in blogs and fiction but often discouraged in formal essays. Use However, Moreover or Therefore, or join the sentences.",
        "But this is not true.", "However, this is not true."),
    rule!("tone.second-person", Tone, "Addressing the reader as 'you'",
        "Academic essays usually avoid 'you' for general statements. Use 'people', 'one', or a specific subject.",
        "If you study hard, you will succeed.", "Students who study hard tend to succeed."),
    rule!("tone.exclamation", Tone, "Exclamation mark in formal writing",
        "Formal writing makes its point with words, not exclamation marks.",
        "This is a huge problem!", "This is a serious problem."),
    rule!("tone.cliche", Tone, "Cliché",
        "Clichés are phrases used so often they have lost their force (at the end of the day, think outside the box). Say what you mean in your own words.",
        "At the end of the day, it's a game changer.", "Ultimately, it changes how we work."),
];

pub fn info(id: &str) -> &'static RuleInfo {
    RULES.iter().find(|r| r.id == id).unwrap_or(&RULES[0])
}

// ---- word lists ----------------------------------------------------------------

const COMMON_MISSPELLINGS: &[(&str, &str)] = &[
    ("teh", "the"), ("recieve", "receive"), ("recieved", "received"), ("beleive", "believe"), ("belive", "believe"),
    ("acheive", "achieve"), ("achive", "achieve"), ("adress", "address"), ("accomodate", "accommodate"),
    ("accomodation", "accommodation"), ("occured", "occurred"), ("occuring", "occurring"), ("occurence", "occurrence"),
    ("seperate", "separate"), ("seperately", "separately"), ("definately", "definitely"), ("definitly", "definitely"),
    ("goverment", "government"), ("enviroment", "environment"), ("enviromental", "environmental"), ("untill", "until"),
    ("wich", "which"), ("becuase", "because"), ("beacuse", "because"), ("becasue", "because"), ("thier", "their"),
    ("freind", "friend"), ("freinds", "friends"), ("wierd", "weird"), ("tommorow", "tomorrow"), ("tomorow", "tomorrow"),
    ("begining", "beginning"), ("buisness", "business"), ("calender", "calendar"), ("collegue", "colleague"),
    ("comming", "coming"), ("commited", "committed"), ("concious", "conscious"), ("existance", "existence"),
    ("familar", "familiar"), ("finaly", "finally"), ("foriegn", "foreign"), ("fourty", "forty"), ("gaurd", "guard"),
    ("happend", "happened"), ("harrass", "harass"), ("immediatly", "immediately"), ("independant", "independent"),
    ("knowlege", "knowledge"), ("libary", "library"), ("lisence", "license"), ("maintainance", "maintenance"),
    ("neccessary", "necessary"), ("necesary", "necessary"), ("noticable", "noticeable"), ("ocassion", "occasion"),
    ("occassion", "occasion"), ("persue", "pursue"), ("posession", "possession"), ("prefered", "preferred"),
    ("priviledge", "privilege"), ("probaly", "probably"), ("publically", "publicly"), ("realy", "really"),
    ("recomend", "recommend"), ("refered", "referred"), ("relevent", "relevant"), ("religous", "religious"),
    ("remeber", "remember"), ("resistence", "resistance"), ("responsability", "responsibility"), ("rythm", "rhythm"),
    ("succesful", "successful"), ("successfull", "successful"), ("sucess", "success"), ("suprise", "surprise"),
    ("truely", "truly"), ("unfortunatly", "unfortunately"), ("usefull", "useful"), ("wether", "whether"),
    ("writting", "writing"), ("writen", "written"), ("arguement", "argument"), ("embarass", "embarrass"),
    ("millenium", "millennium"), ("mispell", "misspell"), ("occurrance", "occurrence"), ("paralel", "parallel"),
    ("percieve", "perceive"), ("pronounciation", "pronunciation"), ("questionaire", "questionnaire"),
    ("reccomend", "recommend"), ("strenght", "strength"), ("lenght", "length"), ("threshhold", "threshold"),
    ("tounge", "tongue"), ("vaccum", "vacuum"), ("visable", "visible"), ("whereever", "wherever"),
    ("alot", "a lot"), ("infomation", "information"), ("developement", "development"),
    ("advertisment", "advertisement"), ("benifit", "benefit"), ("benifits", "benefits"), ("diffrent", "different"),
    ("dissapear", "disappear"), ("dissapoint", "disappoint"), ("excercise", "exercise"), ("experiance", "experience"),
    ("grammer", "grammar"), ("hieght", "height"), ("intresting", "interesting"), ("languege", "language"),
    ("oppurtunity", "opportunity"), ("oppertunity", "opportunity"), ("particulary", "particularly"),
    ("peices", "pieces"), ("populer", "popular"), ("posible", "possible"), ("prefer", "prefer"), ("studing", "studying"),
    ("techonology", "technology"), ("tecnology", "technology"), ("transfered", "transferred"), ("wich", "which"),
    ("youre", "you're"), ("dont", "don't"), ("doesnt", "doesn't"), ("didnt", "didn't"), ("cant", "can't"),
    ("wont", "won't"), ("isnt", "isn't"), ("im", "I'm"), ("ive", "I've"), ("thats", "that's"), ("irregardless", "regardless"),
];

/// Words that start with a vowel letter but a consonant sound, and the reverse.
fn vowel_sound(word: &str, original: &str) -> Option<bool> {
    let w = word;
    let first = w.chars().next()?;
    if !first.is_alphabetic() {
        return None;
    }
    // Acronyms are read letter by letter: an MBA, a UFO, an SUV.
    if original.len() >= 2 && original.len() <= 5 && original.chars().all(|c| c.is_ascii_uppercase()) {
        return Some(matches!(first, 'a' | 'e' | 'f' | 'h' | 'i' | 'l' | 'm' | 'n' | 'o' | 'r' | 's' | 'x'));
    }
    const CONSONANT_SOUND: &[&str] = &["uni", "use", "usu", "uti", "ute", "ura", "uro", "eu", "ewe", "one", "once", "ubiq", "uk"];
    const VOWEL_SOUND: &[&str] = &["hour", "honest", "honor", "honour", "heir", "herb"];
    if CONSONANT_SOUND.iter().any(|p| w.starts_with(p)) && !w.starts_with("unin") && !w.starts_with("unim") {
        return Some(false);
    }
    if VOWEL_SOUND.iter().any(|p| w.starts_with(p)) {
        return Some(true);
    }
    Some(matches!(first, 'a' | 'e' | 'i' | 'o' | 'u'))
}

fn third_person(v: &str) -> String {
    match v {
        "have" => "has".into(),
        "do" => "does".into(),
        "go" => "goes".into(),
        _ if v.ends_with('s') || v.ends_with("sh") || v.ends_with("ch") || v.ends_with('x') || v.ends_with('z') || v.ends_with('o') => format!("{v}es"),
        _ if v.ends_with('y') && !v.ends_with("ay") && !v.ends_with("ey") && !v.ends_with("oy") && !v.ends_with("uy") => format!("{}ies", &v[..v.len() - 1]),
        _ => format!("{v}s"),
    }
}

pub const IRREGULAR_BASE: &[(&str, &str)] = &[
    ("went", "go"), ("gone", "go"), ("came", "come"), ("ate", "eat"), ("eaten", "eat"), ("saw", "see"), ("seen", "see"),
    ("took", "take"), ("taken", "take"), ("gave", "give"), ("given", "give"), ("made", "make"), ("did", "do"), ("done", "do"),
    ("had", "have"), ("got", "get"), ("knew", "know"), ("known", "know"), ("thought", "think"), ("told", "tell"), ("said", "say"),
    ("wrote", "write"), ("written", "write"), ("bought", "buy"), ("brought", "bring"), ("found", "find"), ("left", "leave"),
    ("felt", "feel"), ("kept", "keep"), ("began", "begin"), ("ran", "run"), ("sat", "sit"), ("stood", "stand"), ("spoke", "speak"),
    ("met", "meet"), ("paid", "pay"), ("chose", "choose"), ("drove", "drive"), ("broke", "break"), ("fell", "fall"), ("flew", "fly"),
    ("grew", "grow"), ("won", "win"), ("lost", "lose"), ("sent", "send"), ("spent", "spend"), ("taught", "teach"), ("caught", "catch"),
    ("built", "build"), ("held", "hold"), ("heard", "hear"), ("understood", "understand"), ("became", "become"), ("forgot", "forget"),
    ("drank", "drink"), ("sang", "sing"), ("slept", "sleep"), ("swam", "swim"), ("threw", "throw"), ("wore", "wear"), ("woke", "wake"),
    ("led", "lead"), ("meant", "mean"), ("stole", "steal"), ("hid", "hide"), ("rode", "ride"), ("shook", "shake"), ("fought", "fight"),
];

fn base_of(w: &str) -> Option<(VerbForm, String)> {
    if let Some((_, b)) = IRREGULAR_BASE.iter().find(|(p, _)| *p == w) {
        return Some((VerbForm::Past, b.to_string()));
    }
    syntax::verb_form(w)
}

const COUNTABLE: &[&str] = &[
    "student", "book", "country", "year", "day", "problem", "reason", "idea", "example", "friend", "city", "company",
    "job", "car", "house", "school", "teacher", "word", "question", "place", "factor", "benefit", "advantage",
    "disadvantage", "solution", "method", "member", "group", "family", "hour", "minute", "month", "week", "step",
    "type", "kind", "product", "service", "user", "worker", "employee", "citizen", "activity", "opportunity",
    "skill", "subject", "language", "tourist", "visitor", "customer", "resident", "parent", "player", "game", "team",
    "result", "test", "rule", "law", "university", "museum", "building", "child", "man", "woman", "person", "animal",
    "tree", "computer", "phone", "website", "app", "device", "road", "change", "challenge", "difference", "aspect",
    "decade", "century", "area", "region", "organization", "industry", "business", "consumer", "source", "option",
    "feature", "issue", "expert", "scientist", "researcher", "policy", "program", "project", "task", "goal", "plan",
    "brother", "sister", "mother", "father", "son", "daughter", "boss", "dog", "cat", "wife", "husband", "neighbour",
    "effect", "side", "way", "point", "view", "car", "event", "situation", "experience", "job", "film", "movie",
];

fn plural(noun: &str) -> String {
    match noun {
        "child" => "children".into(),
        "man" => "men".into(),
        "woman" => "women".into(),
        "person" => "people".into(),
        "business" => "businesses".into(),
        _ if noun.ends_with('y') && !noun.ends_with("ay") && !noun.ends_with("ey") && !noun.ends_with("oy") => format!("{}ies", &noun[..noun.len() - 1]),
        _ if noun.ends_with('s') || noun.ends_with("sh") || noun.ends_with("ch") || noun.ends_with('x') => format!("{noun}es"),
        _ => format!("{noun}s"),
    }
}

const UNCOUNTABLE_PLURALS: &[(&str, &str)] = &[
    ("informations", "information"), ("advices", "advice"), ("furnitures", "furniture"), ("equipments", "equipment"),
    ("knowledges", "knowledge"), ("evidences", "evidence"), ("feedbacks", "feedback"), ("homeworks", "homework"),
    ("luggages", "luggage"), ("baggages", "baggage"), ("softwares", "software"), ("stuffs", "stuff"),
    ("sceneries", "scenery"), ("traffics", "traffic"), ("vocabularies", "vocabulary"), ("pollutions", "pollution"),
];
const UNCOUNTABLE: &[&str] = &[
    "information", "advice", "furniture", "equipment", "knowledge", "evidence", "feedback", "homework", "luggage",
    "research", "software", "traffic", "pollution", "progress", "news", "scenery", "money", "music", "weather",
];

const PASSIVE_IRREGULAR: &[&str] = &[
    "known", "seen", "given", "taken", "made", "done", "written", "built", "found", "held", "told", "said", "shown",
    "sold", "bought", "paid", "kept", "sent", "spent", "thought", "brought", "taught", "caught", "born", "chosen",
    "driven", "eaten", "forgotten", "hidden", "broken", "spoken", "stolen", "worn", "torn", "begun", "put", "set",
    "led", "met", "understood", "fed", "heard", "meant", "drawn", "grown", "thrown", "beaten", "bitten", "frozen",
];
/// -ed words that usually act as adjectives after "be".
const ED_ADJECTIVES: &[&str] = &[
    "interested", "tired", "excited", "bored", "married", "worried", "concerned", "pleased", "surprised", "satisfied",
    "scared", "supposed", "used", "confused", "embarrassed", "amazed", "annoyed", "disappointed", "frightened",
    "relaxed", "shocked", "talented", "advanced", "experienced", "qualified", "complicated", "detailed", "limited",
    "crowded", "balanced", "dedicated", "determined", "prepared", "motivated", "sophisticated", "naked", "wicked",
    "ashamed", "devoted", "exhausted", "delighted", "stressed", "united", "related", "red", "bed", "need", "seed", "speed",
];

/// (phrase, replacement, rule)
const PHRASES: &[(&str, &str, &str)] = &[
    ("due to the fact that", "because", "clarity.wordy"),
    ("owing to the fact that", "because", "clarity.wordy"),
    ("in light of the fact that", "because", "clarity.wordy"),
    ("despite the fact that", "although", "clarity.wordy"),
    ("in spite of the fact that", "although", "clarity.wordy"),
    ("in order to", "to", "clarity.wordy"),
    ("at this point in time", "now", "clarity.wordy"),
    ("at the present time", "now", "clarity.wordy"),
    ("in the event that", "if", "clarity.wordy"),
    ("a large number of", "many", "clarity.wordy"),
    ("a great deal of", "much", "clarity.wordy"),
    ("the majority of", "most", "clarity.wordy"),
    ("has the ability to", "can", "clarity.wordy"),
    ("have the ability to", "can", "clarity.wordy"),
    ("is able to", "can", "clarity.wordy"),
    ("are able to", "can", "clarity.wordy"),
    ("with regard to", "about", "clarity.wordy"),
    ("with respect to", "about", "clarity.wordy"),
    ("in regard to", "about", "clarity.wordy"),
    ("for the purpose of", "to", "clarity.wordy"),
    ("it is important to note that", "", "clarity.wordy"),
    ("it should be noted that", "", "clarity.wordy"),
    ("prior to", "before", "clarity.wordy"),
    ("subsequent to", "after", "clarity.wordy"),
    ("in close proximity to", "near", "clarity.wordy"),
    ("on a daily basis", "daily", "clarity.wordy"),
    ("in the near future", "soon", "clarity.wordy"),
    ("until such time as", "until", "clarity.wordy"),
    ("in the process of", "", "clarity.wordy"),
    ("the reason why is that", "because", "clarity.wordy"),
    ("there is no doubt that", "undoubtedly", "clarity.wordy"),
    ("is indicative of", "indicates", "clarity.wordy"),
    ("in today's society", "today", "clarity.wordy"),
    ("in today's world", "today", "clarity.wordy"),
    ("make a decision", "decide", "clarity.nominalization"),
    ("made a decision", "decided", "clarity.nominalization"),
    ("make an assumption", "assume", "clarity.nominalization"),
    ("make a recommendation", "recommend", "clarity.nominalization"),
    ("make an improvement", "improve", "clarity.nominalization"),
    ("make improvements", "improve", "clarity.nominalization"),
    ("give an explanation of", "explain", "clarity.nominalization"),
    ("give an explanation", "explain", "clarity.nominalization"),
    ("give a description of", "describe", "clarity.nominalization"),
    ("have a discussion about", "discuss", "clarity.nominalization"),
    ("conduct an analysis of", "analyse", "clarity.nominalization"),
    ("perform an analysis of", "analyse", "clarity.nominalization"),
    ("carry out an analysis of", "analyse", "clarity.nominalization"),
    ("conduct an investigation into", "investigate", "clarity.nominalization"),
    ("carry out an investigation into", "investigate", "clarity.nominalization"),
    ("provide assistance to", "help", "clarity.nominalization"),
    ("take into consideration", "consider", "clarity.nominalization"),
    ("give consideration to", "consider", "clarity.nominalization"),
    ("come to the conclusion that", "conclude that", "clarity.nominalization"),
    ("reach a conclusion", "conclude", "clarity.nominalization"),
    ("make a contribution to", "contribute to", "clarity.nominalization"),
    ("make a suggestion", "suggest", "clarity.nominalization"),
    ("make a comparison between", "compare", "clarity.nominalization"),
    ("make an attempt to", "try to", "clarity.nominalization"),
    ("have an effect on", "affect", "clarity.nominalization"),
    ("has an effect on", "affects", "clarity.nominalization"),
    ("have an impact on", "affect", "clarity.nominalization"),
    ("has an impact on", "affects", "clarity.nominalization"),
    ("make a choice", "choose", "clarity.nominalization"),
    ("achieve a reduction in", "reduce", "clarity.nominalization"),
    ("added bonus", "bonus", "clarity.redundant"),
    ("past history", "history", "clarity.redundant"),
    ("free gift", "gift", "clarity.redundant"),
    ("end result", "result", "clarity.redundant"),
    ("final outcome", "outcome", "clarity.redundant"),
    ("close proximity", "proximity", "clarity.redundant"),
    ("completely finished", "finished", "clarity.redundant"),
    ("each and every", "each", "clarity.redundant"),
    ("true fact", "fact", "clarity.redundant"),
    ("unexpected surprise", "surprise", "clarity.redundant"),
    ("basic fundamentals", "fundamentals", "clarity.redundant"),
    ("future plans", "plans", "clarity.redundant"),
    ("join together", "join", "clarity.redundant"),
    ("revert back", "revert", "clarity.redundant"),
    ("return back", "return", "clarity.redundant"),
    ("repeat again", "repeat", "clarity.redundant"),
    ("combine together", "combine", "clarity.redundant"),
    ("absolutely essential", "essential", "clarity.redundant"),
    ("new innovation", "innovation", "clarity.redundant"),
    ("general consensus", "consensus", "clarity.redundant"),
    ("past experience", "experience", "clarity.redundant"),
    ("collaborate together", "collaborate", "clarity.redundant"),
    ("first began", "began", "clarity.redundant"),
    ("still remains", "remains", "clarity.redundant"),
    ("the reason why", "the reason", "clarity.redundant"),
    ("exact same", "same", "clarity.redundant"),
    ("brief summary", "summary", "clarity.redundant"),
    ("mutual cooperation", "cooperation", "clarity.redundant"),
    ("advance planning", "planning", "clarity.redundant"),
    ("some people say", "", "clarity.unsupported"),
    ("some people think", "", "clarity.unsupported"),
    ("some people believe", "", "clarity.unsupported"),
    ("many people believe", "", "clarity.unsupported"),
    ("many people think", "", "clarity.unsupported"),
    ("it is said that", "", "clarity.unsupported"),
    ("it is believed that", "", "clarity.unsupported"),
    ("experts say", "", "clarity.unsupported"),
    ("studies show", "", "clarity.unsupported"),
    ("research shows", "", "clarity.unsupported"),
    ("everyone knows", "", "clarity.unsupported"),
    ("it is widely known", "", "clarity.unsupported"),
    ("might possibly", "might", "clarity.hedge"),
    ("may possibly", "may", "clarity.hedge"),
    ("could possibly", "could", "clarity.hedge"),
    ("could perhaps", "could", "clarity.hedge"),
    ("perhaps maybe", "perhaps", "clarity.hedge"),
    ("i think maybe", "perhaps", "clarity.hedge"),
    ("for all intensive purposes", "for all intents and purposes", "grammar.confused"),
    ("between you and i", "between you and me", "grammar.confused"),
];

const FILLERS: &[&str] = &["basically", "actually", "literally", "totally", "honestly", "essentially", "kind of", "sort of", "pretty much"];

// ---- engine --------------------------------------------------------------------

pub struct Ctx<'a> {
    pub src: &'a str,
    pub p: &'a Prose,
    pub out: Vec<Issue>,
}

impl Ctx<'_> {
    fn push(&mut self, rule: &'static str, range: Range<usize>, severity: Severity, message: impl Into<String>, suggestions: Vec<String>) {
        if self.out.iter().any(|i| i.range == range && i.rule == rule) {
            return;
        }
        self.out.push(Issue { rule, range, message: message.into(), suggestions, severity });
    }

    fn w(&self, i: usize) -> &str {
        self.p.word(i)
    }

    /// Original spelling of word `i`.
    fn orig(&self, i: usize) -> &str {
        &self.src[self.p.words[i].range.clone()]
    }

    fn span(&self, a: usize, b: usize) -> Range<usize> {
        self.p.words[a].range.start..self.p.words[b].range.end
    }

    /// Words `a` and `b` (a < b) are separated by whitespace only.
    fn adjacent(&self, a: usize, b: usize) -> bool {
        let gap = &self.p.text[self.p.words[a].range.end..self.p.words[b].range.start];
        !gap.is_empty() && gap.chars().all(|c| c == ' ' || c == '\t') && !self.src[self.p.words[a].range.end..self.p.words[b].range.start].contains('\n')
    }

    /// Next word index if it directly follows `i` in the same sentence.
    fn next(&self, i: usize) -> Option<usize> {
        (i + 1 < self.p.words.len() && self.adjacent(i, i + 1)).then_some(i + 1)
    }

    fn prev(&self, i: usize) -> Option<usize> {
        (i > 0 && self.adjacent(i - 1, i)).then(|| i - 1)
    }
}

fn keep_case(original: &str, replacement: &str) -> String {
    let mut chars = original.chars();
    match chars.next() {
        Some(c) if c.is_uppercase() && original.len() > 1 && original.chars().skip(1).all(|c| c.is_uppercase()) => replacement.to_uppercase(),
        Some(c) if c.is_uppercase() => {
            let mut r = replacement.chars();
            r.next().map(|f| f.to_uppercase().collect::<String>() + r.as_str()).unwrap_or_default()
        }
        _ => replacement.to_string(),
    }
}

pub fn check(src: &str, p: &Prose, misspelled: Option<&HashSet<String>>) -> Vec<Issue> {
    let mut cx = Ctx { src, p, out: Vec::new() };
    spelling(&mut cx, misspelled);
    articles(&mut cx);
    repeated(&mut cx);
    agreement(&mut cx);
    verb_forms(&mut cx);
    confused(&mut cx);
    nouns(&mut cx);
    capitalization(&mut cx);
    punctuation(&mut cx);
    phrases(&mut cx);
    passive(&mut cx);
    structure(&mut cx);
    cx.out
}

fn spelling(cx: &mut Ctx, misspelled: Option<&HashSet<String>>) {
    for i in 0..cx.p.words.len() {
        let w = cx.w(i).to_string();
        // Also plurals of listed words: "goverments" → "governments".
        let hit = COMMON_MISSPELLINGS.iter().find(|(bad, good)| *bad == w && bad != good).map(|(_, g)| g.to_string()).or_else(|| {
            let stem = w.strip_suffix('s')?;
            COMMON_MISSPELLINGS.iter().find(|(bad, good)| *bad == stem && bad != good && !good.contains(['\'', ' '])).map(|(_, g)| format!("{g}s"))
        });
        if let Some(right) = hit {
            let right = right.as_str();
            let fix = keep_case(cx.orig(i), right);
            let msg = format!("“{}” is a common misspelling of “{}”.", cx.orig(i), right);
            cx.push("spelling.common", cx.p.words[i].range.clone(), Severity::Error, msg, vec![fix]);
        } else if let Some(bad) = misspelled {
            let o = cx.orig(i);
            if bad.contains(o) {
                let msg = format!("“{o}” is not in the dictionary.");
                cx.push("spelling.dictionary", cx.p.words[i].range.clone(), Severity::Error, msg, vec![]);
            }
        }
    }
}

fn articles(cx: &mut Ctx) {
    for i in 0..cx.p.words.len() {
        let art = cx.w(i);
        if art != "a" && art != "an" {
            continue;
        }
        let Some(j) = cx.next(i) else { continue };
        let orig_next = cx.orig(j);
        // "a" as a letter ("point a", "a)") or a variable.
        if orig_next.len() == 1 {
            continue;
        }
        let Some(vowel) = vowel_sound(cx.w(j), orig_next) else { continue };
        let wrong = (art == "a" && vowel) || (art == "an" && !vowel);
        if wrong {
            let right = if art == "a" { "an" } else { "a" };
            let fix = keep_case(cx.orig(i), right);
            let sound = if vowel { "a vowel sound" } else { "a consonant sound" };
            let msg = format!("Use “{right}” before “{orig_next}”, which starts with {sound}.");
            cx.push("grammar.article", cx.p.words[i].range.clone(), Severity::Error, msg, vec![fix]);
        }
    }
}

fn repeated(cx: &mut Ctx) {
    for i in 1..cx.p.words.len() {
        if cx.w(i) == cx.w(i - 1) && cx.adjacent(i - 1, i) && !matches!(cx.w(i), "had" | "that" | "is" | "do" | "very" | "bye" | "ha" | "no") {
            let msg = format!("“{}” is repeated.", cx.orig(i));
            let keep = cx.orig(i - 1).to_string();
            cx.push("grammar.repeated-word", cx.span(i - 1, i), Severity::Error, msg, vec![keep]);
        }
    }
}

fn agreement(cx: &mut Ctx) {
    // (subjects, wrong verb, right verb)
    const SINGULAR: &[&str] = &["he", "she", "it", "everyone", "everybody", "someone", "somebody", "nobody", "each"];
    // Uncountable subjects take a singular verb: "technology has", "education helps".
    const MASS: &[&str] = &["technology", "education", "society", "information", "pollution", "research", "money", "music", "knowledge", "traffic", "unemployment", "tourism", "globalization", "globalisation"];
    const MASS_VERBS: &[&str] = &["help", "make", "give", "allow", "cause", "play", "become", "bring", "change", "affect", "provide", "need", "seem", "mean", "take", "create", "lead", "improve", "reduce", "increase", "benefit", "depend", "require", "seem", "contain", "cost", "work", "happen", "look", "sound", "matter", "remain"];
    const PLURAL: &[&str] = &["you", "we", "they"];
    for i in 0..cx.p.words.len() {
        let s = cx.w(i).to_string();
        let Some(j) = cx.next(i) else { continue };
        let v = cx.w(j).to_string();
        let prev = cx.prev(i).map(|k| cx.w(k).to_string()).unwrap_or_default();
        // Questions and subjunctives invert or keep the base form: "did he go", "if it were".
        let inverted = matches!(prev.as_str(), "did" | "does" | "do" | "will" | "would" | "can" | "could" | "should" | "may" | "might" | "must" | "let" | "make" | "help" | "if" | "wish" | "though" | "and" | "or" | "that" | "than" | "as" | "to" | "of" | "for" | "with" | "is" | "was" | "has" | "had" | "have" | "are" | "were");
        if inverted && !matches!(prev.as_str(), "and" | "or" | "that" | "than" | "though") {
            continue;
        }
        let mass = (MASS.contains(&s.as_str()) && !matches!(prev.as_str(), "of" | "in" | "about" | "on" | "for" | "with")) || s == "it";
        if mass && MASS_VERBS.contains(&v.as_str()) {
            let f = third_person(&v);
            let msg = if s == "it" { format!("With “{}”, the present tense verb takes -s: “{f}”.", cx.orig(i)) } else { format!("“{}” is singular (uncountable), so the verb takes -s: “{f}”.", cx.orig(i)) };
            cx.push("grammar.agreement", cx.p.words[j].range.clone(), Severity::Error, msg, vec![f]);
            continue;
        }
        let fix: Option<&str> = if SINGULAR.contains(&s.as_str()) || mass {
            match v.as_str() {
                "have" => Some("has"),
                "do" if !inverted => Some("does"),
                "don't" => Some("doesn't"),
                "are" => Some("is"),
                "were" if !matches!(prev.as_str(), "if" | "wish" | "as") => Some("was"),
                "weren't" => Some("wasn't"),
                "haven't" => Some("hasn't"),
                _ => None,
            }
        } else if s == "i" {
            match v.as_str() {
                "has" => Some("have"),
                "is" | "are" => Some("am"),
                "does" => Some("do"),
                "doesn't" => Some("don't"),
                "were" if !matches!(prev.as_str(), "if" | "wish" | "as") => Some("was"),
                _ => None,
            }
        } else if PLURAL.contains(&s.as_str()) || s == "people" || s == "children" {
            match v.as_str() {
                "has" => Some("have"),
                "is" => Some("are"),
                "was" => Some("were"),
                "does" => Some("do"),
                "doesn't" => Some("don't"),
                "hasn't" => Some("haven't"),
                "wasn't" => Some("weren't"),
                _ => None,
            }
        } else {
            None
        };
        if let Some(f) = fix {
            let msg = format!("“{}” does not agree with “{}”: use “{}”.", cx.orig(j), cx.orig(i), f);
            let fixed = keep_case(cx.orig(j), f);
            cx.push("grammar.agreement", cx.p.words[j].range.clone(), Severity::Error, msg, vec![fixed]);
            continue;
        }
        // "he go" → "he goes": only he/she, whose next word is unambiguous.
        if (s == "he" || s == "she") && !inverted {
            if let Some((VerbForm::Base, base)) = syntax::verb_form(&v)
                && base != "be"
                && !matches!(v.as_str(), "can" | "will" | "must" | "may" | "should" | "would" | "could" | "might" | "shall" | "need" | "dare" | "used")
            {
                let f = third_person(&base);
                let msg = format!("With “{}”, the present tense verb takes -s: “{}”.", cx.orig(i), f);
                cx.push("grammar.agreement", cx.p.words[j].range.clone(), Severity::Error, msg, vec![f]);
            }
        }
        // "my brother have" → "has": determiner + singular countable noun + plural verb.
        if matches!(s.as_str(), "my" | "his" | "her" | "our" | "their" | "your" | "the" | "this" | "that" | "a" | "an" | "every" | "each")
            && COUNTABLE.contains(&v.as_str())
            && let Some(k) = cx.next(j)
        {
            let verb = cx.w(k).to_string();
            let f = match verb.as_str() {
                "have" => Some("has"),
                "are" => Some("is"),
                "were" => Some("was"),
                "don't" => Some("doesn't"),
                "haven't" => Some("hasn't"),
                "aren't" => Some("isn't"),
                _ => None,
            };
            if let Some(f) = f {
                let msg = format!("“{}” is singular, so use “{f}”.", cx.orig(j));
                let fixed = keep_case(cx.orig(k), f);
                cx.push("grammar.agreement", cx.p.words[k].range.clone(), Severity::Error, msg, vec![fixed]);
            }
        }
        // "friends who lives" → "live": a plural antecedent takes a plural verb.
        if matches!(v.as_str(), "who" | "that")
            && (s == "people" || s == "children" || COUNTABLE.iter().any(|c| plural(c) == s))
            && let Some(k) = cx.next(j)
            && let Some((VerbForm::Third, base)) = syntax::verb_form(cx.w(k))
            && !matches!(cx.w(k), "is" | "has" | "was" | "does")
        {
            let msg = format!("“{}” refers to “{}” (plural), so use “{base}”.", cx.orig(j), cx.orig(i));
            cx.push("grammar.agreement", cx.p.words[k].range.clone(), Severity::Error, msg, vec![base]);
        }
        // "there is many/two/several …"
        if s == "there" && (v == "is" || v == "was") {
            if let Some(k) = cx.next(j) {
                let q = cx.w(k);
                if matches!(q, "many" | "several" | "two" | "three" | "four" | "five" | "few" | "various" | "numerous" | "lots") {
                    let f = if v == "is" { "are" } else { "were" };
                    let msg = format!("“{}” is followed by a plural, so use “there {f}”.", cx.orig(k));
                    let fixed = keep_case(cx.orig(j), f);
                    cx.push("grammar.agreement", cx.p.words[j].range.clone(), Severity::Error, msg, vec![fixed]);
                }
            }
        }
    }
}

fn verb_forms(cx: &mut Ctx) {
    const MODALS: &[&str] = &["can", "could", "will", "would", "shall", "should", "may", "might", "must", "cannot", "can't", "won't", "wouldn't", "shouldn't", "couldn't", "mustn't"];
    const DO: &[&str] = &["do", "does", "did", "don't", "doesn't", "didn't"];
    for i in 0..cx.p.words.len() {
        let a = cx.w(i).to_string();
        let modal = MODALS.contains(&a.as_str());
        let aux_do = DO.contains(&a.as_str());
        if !modal && !aux_do {
            continue;
        }
        let Some(mut j) = cx.next(i) else { continue };
        // Skip "not" and a subject in questions: "did you went", "can she speaks".
        if cx.w(j) == "not" || ((aux_do || modal) && matches!(cx.w(j), "you" | "he" | "she" | "it" | "we" | "they" | "i")) {
            let Some(k) = cx.next(j) else { continue };
            j = k;
        }
        let v = cx.w(j).to_string();
        if modal && v == "to" && a != "ought" {
            let msg = format!("After “{}”, use the base verb without “to”.", cx.orig(i));
            let next = cx.next(j).map(|k| cx.orig(k).to_string());
            if let Some(n) = next {
                let r = cx.p.words[j].range.start..cx.p.words[j + 1].range.end;
                cx.push("grammar.verb-form", r, Severity::Error, msg, vec![n]);
            }
            continue;
        }
        let Some((form, base)) = base_of(&v) else { continue };
        if matches!(base.as_str(), "be" | "have") && modal {
            continue;
        }
        // "do" as a main verb: "did homework" is fine, "did went" is not.
        let wrong = match form {
            VerbForm::Third => !matches!(v.as_str(), "is" | "has" | "was" | "news" | "series" | "means" | "lens" | "process" | "access" | "address" | "success" | "business"),
            VerbForm::Past => {
                // Participles after "do" are wrong; but "had", "done" may start "have done" chains after modals.
                !(modal && matches!(v.as_str(), "had" | "been"))
            }
            _ => false,
        };
        if wrong && base != v {
            let msg = format!("After “{}”, use the base form “{}”, not “{}”.", cx.orig(i), base, cx.orig(j));
            cx.push("grammar.verb-form", cx.p.words[j].range.clone(), Severity::Error, msg, vec![base]);
        }
    }
    // "to went", "to ate": infinitive with a past form.
    for i in 0..cx.p.words.len() {
        if cx.w(i) != "to" {
            continue;
        }
        let Some(j) = cx.next(i) else { continue };
        if let Some((_, base)) = IRREGULAR_BASE.iter().find(|(p, _)| *p == cx.w(j) && !matches!(*p, "left" | "found" | "set" | "led" | "met" | "paid" | "had" | "made" | "done" | "known" | "given" | "taken" | "seen" | "lost" | "spent" | "sent" | "kept" | "built" | "held" | "taught" | "bought" | "caught" | "thought" | "told" | "said" | "felt" | "heard" | "understood" | "written" | "eaten" | "gone" | "won" | "meant")) {
            let msg = format!("After “to”, use the base verb “{base}”.");
            cx.push("grammar.verb-form", cx.p.words[j].range.clone(), Severity::Error, msg, vec![base.to_string()]);
        }
    }
}

fn confused(cx: &mut Ctx) {
    let n = cx.p.words.len();
    for i in 0..n {
        let w = cx.w(i).to_string();
        let next = cx.next(i).map(|j| cx.w(j).to_string());
        let prev = cx.prev(i).map(|j| cx.w(j).to_string());
        let nx = next.as_deref().unwrap_or("");
        let pv = prev.as_deref().unwrap_or("");
        let fix = |cx: &mut Ctx, right: &str, why: &str, sev: Severity| {
            let msg = format!("“{}” → “{}”: {why}", cx.orig(i), right);
            let fixed = keep_case(cx.orig(i), right);
            cx.push("grammar.confused", cx.p.words[i].range.clone(), sev, msg, vec![fixed]);
        };
        match w.as_str() {
            "its" if matches!(nx, "a" | "an" | "the" | "not" | "been" | "going" | "very" | "so" | "too" | "really" | "important" | "clear" | "obvious" | "possible" | "necessary" | "likely" | "true" | "time" | "hard" | "easy" | "difficult") => {
                fix(cx, "it's", "here you mean “it is”. “Its” shows possession (its colour).", Severity::Error)
            }
            "it's" if nx == "own" => fix(cx, "its", "possession is written without an apostrophe.", Severity::Error),
            "your" if matches!(nx, "welcome" | "going" | "right" | "wrong" | "not" | "being" | "a" | "an" | "the" | "very" | "so" | "too" | "doing" | "making") => {
                fix(cx, "you're", "here you mean “you are”. “Your” shows possession (your book).", Severity::Error)
            }
            "you're" if nx == "own" => fix(cx, "your", "possession is “your”.", Severity::Error),
            "their" if matches!(nx, "is" | "are" | "was" | "were" | "has" | "seems" | "seem") => {
                fix(cx, "there", "“there is/are” introduces something; “their” shows possession.", Severity::Error)
            }
            "there" if matches!(nx, "own" | "parents" | "children" | "friends" | "family" | "families" | "homes" | "lives") => {
                fix(cx, "their", "possession is “their” (their family).", Severity::Error)
            }
            "then" if matches!(pv, "more" | "less" | "better" | "worse" | "rather" | "other" | "greater" | "fewer" | "bigger" | "smaller" | "higher" | "lower" | "larger" | "faster" | "easier" | "harder" | "longer" | "older" | "younger") || is_comparative(pv) => {
                fix(cx, "than", "comparisons use “than”; “then” is about time.", Severity::Error)
            }
            "to" if matches!(nx, "much" | "many") && !matches!(pv, "up" | "due" | "next" | "close" | "similar" | "compared" | "equal" | "according") => {
                fix(cx, "too", "“too” means “more than enough”; “to” is a preposition or part of a verb.", Severity::Error)
            }
            "then" if (2..=4).any(|k| i >= k && cx.p.words[i - k].range.end <= cx.p.words[i].range.start && {
                let w = cx.w(i - k);
                matches!(w, "more" | "less" | "fewer" | "rather") || is_comparative(w)
            }) && !matches!(pv, "and" | "since" | "just") && cx.src[cx.p.words[i - 1].range.end..cx.p.words[i].range.start].trim().is_empty() => {
                fix(cx, "than", "comparisons use “than”; “then” is about time.", Severity::Warning)
            }
            "than" if matches!(pv, "and" | "since" | "until" | "back" | "just") => fix(cx, "then", "“then” is about time; “than” compares.", Severity::Warning),
            "of" if matches!(pv, "could" | "would" | "should" | "must" | "might") => {
                let r = cx.p.words[i].range.clone();
                let msg = format!("“{pv} of” → “{pv} have”: it sounds like “of” in speech, but the verb is “have”.");
                cx.push("grammar.confused", r, Severity::Error, msg, vec!["have".into()]);
            }
            "affect" if matches!(pv, "an" | "the" | "positive" | "negative" | "significant" | "big") && pv != "" => {
                fix(cx, "effect", "the noun is “effect”; “affect” is usually the verb.", Severity::Warning)
            }
            "loose" if matches!(pv, "to" | "will" | "would" | "might" | "may" | "could" | "can" | "not") => {
                fix(cx, "lose", "“lose” means fail to keep; “loose” means not tight.", Severity::Error)
            }
            "suppose" if nx == "to" && matches!(pv, "is" | "are" | "was" | "were" | "am" | "be" | "not") => {
                fix(cx, "supposed", "the expression is “be supposed to”.", Severity::Error)
            }
            "use" if nx == "to" && matches!(pv, "i" | "we" | "they" | "he" | "she" | "you" | "be" | "get" | "got" | "am" | "is" | "are" | "was" | "were") => {
                fix(cx, "used", "past habits use “used to” (I used to swim); “be/get used to” means accustomed.", Severity::Error)
            }
            "everyday" if next.is_none() || matches!(nx, "and" | "but" | "for" | "in" | "at") => {
                fix(cx, "every day", "“everyday” is an adjective (everyday life); as a time phrase write “every day”.", Severity::Warning)
            }
            _ => {}
        }
    }
}

/// "cheaper", "happier", "larger": an adjective + -er.
fn is_comparative(w: &str) -> bool {
    let stems = [w.strip_suffix("ier").map(|s| format!("{s}y")), w.strip_suffix("er").map(str::to_string), w.strip_suffix('r').map(str::to_string)];
    const MORE: &[&str] = &["cheap", "tall", "smart", "nice", "safe", "wise", "kind", "busy", "healthy", "wealthy", "lucky", "heavy", "dear", "fine", "pure", "rare", "brave", "fresh", "clever", "quiet", "sweet", "tough", "wealthy", "friendly", "noisy", "tidy", "rough", "loud", "smooth"];
    stems.into_iter().flatten().any(|s| s.len() >= 3 && (syntax::is_adjective(&s) || MORE.contains(&s.as_str())))
}

fn nouns(cx: &mut Ctx) {
    const PLURAL_TRIGGERS: &[&str] = &["many", "several", "few", "various", "numerous", "both", "these", "those", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "hundreds", "thousands", "millions", "multiple"];
    for i in 0..cx.p.words.len() {
        let w = cx.w(i).to_string();
        if let Some((_, right)) = UNCOUNTABLE_PLURALS.iter().find(|(bad, _)| *bad == w) {
            let msg = format!("“{right}” is uncountable and has no plural.");
            let fixed = keep_case(cx.orig(i), right);
            cx.push("grammar.uncountable", cx.p.words[i].range.clone(), Severity::Error, msg, vec![fixed]);
            continue;
        }
        // "a feedback", "a useful feedback"
        let after_adj = |cx: &Ctx, j: usize| {
            let a = cx.w(j);
            let adj = syntax::is_adjective(a) || ["ful", "ive", "ous", "al", "ant", "ent", "ble", "ic"].iter().any(|s| a.len() > 4 && a.ends_with(s));
            if adj { cx.next(j).filter(|&k| UNCOUNTABLE.contains(&cx.w(k))) } else { None }
        };
        if (w == "a" || w == "an")
            && let Some(j) = cx.next(i).and_then(|j| if UNCOUNTABLE.contains(&cx.w(j)) { Some(j) } else { after_adj(cx, j) })
            && !(cx.w(j) == "research" && cx.next(j).is_some_and(|k| matches!(cx.w(k), "project" | "paper" | "study" | "team" | "question" | "method")))
        {
            let first = i + 1;
            let noun = cx.orig(j).to_string();
            let phrase = cx.src[cx.span(first, j)].to_string();
            let msg = format!("“{noun}” is uncountable: drop “{}” or say “a piece of {phrase}”.", cx.orig(i));
            cx.push("grammar.uncountable", cx.span(i, j), Severity::Error, msg, vec![phrase.clone(), format!("a piece of {phrase}")]);
            continue;
        }
        if w == "much"
            && let Some(j) = cx.next(i)
            && (COUNTABLE.iter().any(|c| plural(c) == cx.w(j)) || cx.w(j) == "people")
        {
            let msg = format!("“{}” is countable, so use “many”.", cx.orig(j));
            let fixed = keep_case(cx.orig(i), "many");
            cx.push("grammar.quantifier", cx.p.words[i].range.clone(), Severity::Error, msg, vec![fixed]);
        }
        if w == "less"
            && let Some(j) = cx.next(i)
            && (COUNTABLE.iter().any(|c| plural(c) == cx.w(j)) || cx.w(j) == "people")
        {
            let msg = format!("“{}” is countable, so use “fewer”.", cx.orig(j));
            let fixed = keep_case(cx.orig(i), "fewer");
            cx.push("grammar.quantifier", cx.p.words[i].range.clone(), Severity::Warning, msg, vec![fixed]);
        }
        // many student → many students; one of the best school → schools
        let mut target = None;
        if PLURAL_TRIGGERS.contains(&w.as_str()) {
            target = cx.next(i);
        } else if w == "of"
            && cx.prev(i).is_some_and(|k| cx.w(k) == "one")
            && let Some(j) = cx.next(i)
            && matches!(cx.w(j), "the" | "my" | "his" | "her" | "our" | "their" | "your" | "its")
        {
            target = cx.next(j);
        }
        let Some(mut t) = target else { continue };
        // Allow one adjective in between: "many young student".
        if !COUNTABLE.contains(&cx.w(t)) {
            let adj = syntax::is_adjective(cx.w(t)) || matches!(cx.w(t), "most" | "best" | "biggest" | "largest" | "main" | "greatest" | "worst" | "top" | "major" | "different" | "other" | "foreign" | "international" | "local");
            match cx.next(t) {
                Some(k) if adj => t = k,
                _ => continue,
            }
        }
        let noun = cx.w(t).to_string();
        if COUNTABLE.contains(&noun.as_str())
            // "these kind of" etc. are separate idioms; "both" + "the" handled by adjacency.
            && !cx.next(t).is_some_and(|k| cx.w(k) == "of" && matches!(noun.as_str(), "kind" | "type"))
        {
            let pl = plural(&noun);
            let msg = format!("After “{}”, use the plural “{}”.", cx.orig(i), pl);
            let fixed = keep_case(cx.orig(t), &pl);
            cx.push("grammar.plural", cx.p.words[t].range.clone(), Severity::Error, msg, vec![fixed]);
        }
    }
}

fn capitalization(cx: &mut Ctx) {
    const PROPER: &[&str] = &[
        "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday", "january", "february", "april",
        "june", "july", "august", "september", "october", "november", "december", "english", "vietnamese", "french",
        "chinese", "japanese", "korean", "german", "spanish", "american", "british", "european", "asian", "african",
        "australian", "canadian", "italian", "russian", "thai", "indian", "christmas", "vietnam", "england", "america",
    ];
    for i in 0..cx.p.words.len() {
        let o = cx.orig(i).to_string();
        let w = cx.w(i);
        if matches!(w, "i" | "i'm" | "i've" | "i'll" | "i'd") && o.starts_with('i') {
            let fixed = format!("I{}", &o[1..]);
            cx.push("grammar.capitalization", cx.p.words[i].range.clone(), Severity::Error, "The pronoun “I” is always capitalized.", vec![fixed]);
        } else if PROPER.contains(&w) && o.chars().next().is_some_and(char::is_lowercase) {
            let fixed = keep_case("X", &o);
            let msg = format!("“{fixed}” is a proper noun and starts with a capital letter.");
            cx.push("grammar.capitalization", cx.p.words[i].range.clone(), Severity::Error, msg, vec![fixed]);
        }
    }
    for s in cx.p.body() {
        let Some(first) = cx.p.words.get(s.words.start) else { continue };
        if s.words.is_empty() || first.range.start != s.range.start {
            continue;
        }
        let o = &cx.src[first.range.clone()];
        // Only after a real sentence end, not after a line break inside a list.
        let before = cx.src[..s.range.start].trim_end_matches([' ', '\t']);
        let after_end = before.ends_with(['.', '!', '?']) && !before.ends_with("...") && !before.ends_with("e.g.") && !before.ends_with("i.e.");
        let para_start = cx.p.paragraphs.get(s.paragraph).is_some_and(|p| {
            cx.src[p.start..s.range.start].trim_start_matches(['#', '-', '*', '>', ' ', '\t']).trim().is_empty()
        });
        if (after_end || para_start)
            && o.chars().next().is_some_and(char::is_lowercase)
            && !o.chars().skip(1).any(char::is_uppercase)
            && !matches!(cx.p.word(s.words.start), "i" | "e.g" | "i.e" | "etc")
        {
            let fixed = keep_case("X", o);
            let range = first.range.clone();
            cx.push("grammar.capitalization", range, Severity::Error, "A sentence starts with a capital letter.", vec![fixed]);
        }
    }
}

fn punctuation(cx: &mut Ctx) {
    let t = cx.p.text.as_bytes();
    let n = t.len();
    for i in 1..n {
        let c = t[i];
        // Space before punctuation: "word ,"
        if matches!(c, b',' | b'.' | b';' | b':' | b'!' | b'?') && t[i - 1] == b' ' && i >= 2 && t[i - 2].is_ascii_alphanumeric() {
            // Not an ellipsis or a leading decimal (" .5").
            if c == b'.' && t.get(i + 1).is_some_and(|d| *d == b'.' || d.is_ascii_digit()) {
                continue;
            }
            let p = (c as char).to_string();
            cx.push("punct.spacing", i - 1..i + 1, Severity::Error, format!("No space before “{p}”."), vec![p.clone()]);
        }
        // Missing space after: "word,word" or "end.Next"
        if matches!(c, b',' | b';') && i + 1 < n && t[i - 1].is_ascii_alphabetic() && t[i + 1].is_ascii_alphabetic() {
            let p = (c as char).to_string();
            cx.push("punct.spacing", i..i + 1, Severity::Error, format!("Add a space after “{p}”."), vec![format!("{p} ")]);
        }
        if matches!(c, b'.' | b'!' | b'?') && i + 1 < n && i >= 2 && t[i - 1].is_ascii_lowercase() && t[i - 2].is_ascii_lowercase() && t[i + 1].is_ascii_uppercase() {
            let p = (c as char).to_string();
            cx.push("punct.spacing", i..i + 1, Severity::Error, format!("Add a space after “{p}”."), vec![format!("{p} ")]);
        }
        // Repeated marks
        if matches!(c, b'!' | b'?' | b',') && t[i - 1] == c && (i < 2 || t[i - 2] != c) {
            let mut e = i + 1;
            while e < n && t[e] == c {
                e += 1;
            }
            let p = (c as char).to_string();
            cx.push("punct.repeated", i - 1..e, Severity::Warning, format!("Use a single “{p}”."), vec![p]);
        }
        if c == b'.' && t[i - 1] == b'.' && (i < 2 || t[i - 2] != b'.') && t.get(i + 1) != Some(&b'.') && i >= 2 && t[i - 2].is_ascii_alphabetic() {
            cx.push("punct.repeated", i - 1..i + 1, Severity::Warning, "Two dots: use one period, or three for an ellipsis.", vec![".".into(), "…".into()]);
        }
    }
    // Missing end punctuation on prose paragraphs.
    for (pi, para) in cx.p.paragraphs.iter().enumerate() {
        let raw = cx.src[para.clone()].trim_start();
        if raw.starts_with('#') || raw.starts_with('|') || raw.starts_with("- ") || raw.starts_with("* ") || raw.starts_with('>') || raw.starts_with("```") || raw.starts_with('!') {
            continue;
        }
        let words: Vec<usize> = cx.p.sentences.iter().filter(|s| s.paragraph == pi && !s.heading).flat_map(|s| s.words.clone()).collect();
        if words.len() < 6 {
            continue;
        }
        let last_char = cx.p.text[para.clone()].trim_end().chars().last();
        let src_last = cx.src[para.clone()].trim_end().chars().last();
        if let (Some(c), Some(sc)) = (last_char, src_last)
            && c.is_alphanumeric()
            && sc.is_alphanumeric()
        {
            let w = *words.last().unwrap();
            let r = cx.p.words[w].range.clone();
            let fixed = format!("{}.", cx.orig(w));
            cx.push("punct.missing-end", r, Severity::Warning, "This sentence has no end punctuation.", vec![fixed]);
        }
    }
}

/// Multi-word phrase matches as (first word, last word) index pairs.
pub fn find_phrase(p: &Prose, phrase: &str) -> Vec<(usize, usize)> {
    let parts: Vec<String> = phrase.split_whitespace().map(|s| s.replace('’', "'")).collect();
    let mut out = Vec::new();
    if parts.is_empty() || p.words.len() < parts.len() {
        return out;
    }
    'outer: for i in 0..=p.words.len() - parts.len() {
        for (k, part) in parts.iter().enumerate() {
            if p.words[i + k].lower != *part {
                continue 'outer;
            }
            if k > 0 {
                let gap = &p.text[p.words[i + k - 1].range.end..p.words[i + k].range.start];
                if !gap.chars().all(|c| c == ' ' || c == '-') {
                    continue 'outer;
                }
            }
        }
        out.push((i, i + parts.len() - 1));
    }
    out
}

fn phrases(cx: &mut Ctx) {
    for (phrase, repl, rule) in PHRASES {
        for (a, b) in find_phrase(cx.p, phrase) {
            let orig = cx.src[cx.span(a, b)].to_string();
            let (sev, msg) = match *rule {
                "clarity.wordy" if repl.is_empty() => (Severity::Hint, format!("“{orig}” adds words without meaning — cut it.")),
                "clarity.wordy" => (Severity::Warning, format!("“{orig}” can be just “{repl}”.")),
                "clarity.nominalization" => (Severity::Hint, format!("“{orig}” hides the verb: try “{repl}”.")),
                "clarity.redundant" => (Severity::Hint, format!("“{orig}” repeats itself: “{repl}” is enough.")),
                "clarity.unsupported" => (Severity::Hint, format!("“{orig}” — who exactly? Name the source or give evidence.")),
                "clarity.hedge" => (Severity::Hint, format!("“{orig}” stacks two hedges: keep one.")),
                _ => (Severity::Error, format!("“{orig}” → “{repl}”.")),
            };
            let suggestions = if *rule == "clarity.unsupported" { vec![] } else { vec![keep_case(&orig, repl)] };
            cx.push(rule, cx.span(a, b), sev, msg, suggestions);
        }
    }
    for f in FILLERS {
        for (a, b) in find_phrase(cx.p, f) {
            let orig = cx.src[cx.span(a, b)].to_string();
            cx.push("clarity.filler", cx.span(a, b), Severity::Hint, format!("“{orig}” rarely adds meaning."), vec![String::new()]);
        }
    }
    for c in crate::style_check::patterns(crate::style_check::Check::Cliche) {
        for (a, b) in find_phrase(cx.p, c) {
            let orig = cx.src[cx.span(a, b)].to_string();
            cx.push("tone.cliche", cx.span(a, b), Severity::Hint, format!("“{orig}” is a cliché — say it in your own words."), vec![]);
        }
    }
}

fn passive(cx: &mut Ctx) {
    const BE: &[&str] = &["am", "is", "are", "was", "were", "be", "been", "being", "isn't", "aren't", "wasn't", "weren't"];
    for s in cx.p.body() {
        let ws = s.words.clone();
        for i in ws.clone() {
            if !BE.contains(&cx.w(i)) {
                continue;
            }
            let Some(mut j) = cx.next(i) else { continue };
            if cx.w(j).ends_with("ly") || cx.w(j) == "not" {
                match cx.next(j) {
                    Some(k) => j = k,
                    None => continue,
                }
            }
            let v = cx.w(j);
            let participle = (v.ends_with("ed") && v.len() > 4 && !ED_ADJECTIVES.contains(&v)) || PASSIVE_IRREGULAR.contains(&v);
            if !participle {
                continue;
            }
            let by = (j + 1..ws.end.min(j + 8)).find(|&k| cx.w(k) == "by");
            let msg = match by {
                Some(_) => "Passive voice: the doer comes after “by”. Try making it the subject.".to_string(),
                None => "Passive voice: who does the action? Consider naming them as the subject.".to_string(),
            };
            cx.push("clarity.passive", cx.span(i, j), Severity::Hint, msg, vec![]);
        }
    }
}

fn structure(cx: &mut Ctx) {
    const SUBORD: &[&str] = &["although", "because", "if", "when", "while", "since", "as", "after", "before", "though", "unless", "whereas", "which", "who", "that", "where", "whether"];
    const SUBJ: &[&str] = &["i", "you", "he", "she", "it", "we", "they", "this", "there"];
    const VERB: &[&str] = &["is", "are", "was", "were", "am", "has", "have", "had", "will", "can", "would", "should", "could", "must", "seems", "makes", "means", "did"];
    let sentences: Vec<_> = cx.p.body().cloned().collect();
    for s in &sentences {
        let n = s.words.len();
        let first = s.words.start;
        if n > 30 {
            let msg = format!("This sentence has {n} words. Aim for 15–25; split it at a natural break.");
            let r = s.range.clone();
            cx.push("structure.long-sentence", r, if n > 40 { Severity::Warning } else { Severity::Hint }, msg, vec![]);
        }
        let ands = s.words.clone().filter(|&k| cx.w(k) == "and").count();
        if ands >= 3 && n >= 20 {
            let r = s.range.clone();
            cx.push("structure.run-on", r, Severity::Warning, format!("{ands} clauses chained with “and”. Split or subordinate some of them."), vec![]);
        }
        // Fragment: subordinate clause standing alone.
        if n > 0 && n <= 15 && matches!(cx.w(first), "because" | "although" | "though" | "whereas" | "which" | "unless") && !cx.src[s.range.clone()].contains(',') {
            let r = s.range.clone();
            let msg = format!("This starts with “{}” but has no main clause. Join it to the neighbouring sentence.", cx.orig(first));
            cx.push("structure.fragment", r, Severity::Warning, msg, vec![]);
        }
        // Comma splice: ", it is", ", we were" with no subordinator before the comma.
        let text = &cx.p.text;
        for k in s.words.clone().skip(3) {
            let gap = &text[cx.p.words[k - 1].range.end..cx.p.words[k].range.start];
            if gap.trim() != "," || k + 1 >= s.words.end {
                continue;
            }
            let before_has_sub = (first..k).any(|m| SUBORD.contains(&cx.w(m)));
            let w = cx.w(k);
            if w == "however" || w == "therefore" {
                let after = &text[cx.p.words[k].range.end..cx.p.words[k + 1].range.start];
                if !before_has_sub {
                    // Replace ", however" (and its comma, if any) with "; however,".
                    let end = if after.trim() == "," { cx.p.words[k].range.end + after.find(',').unwrap() + 1 } else { cx.p.words[k].range.end };
                    let r = cx.p.words[k - 1].range.end..end;
                    let fixed = format!("; {},", cx.orig(k));
                    let msg = format!("“{}” joining two sentences needs a semicolon before it (and a comma after).", cx.orig(k));
                    cx.push("structure.comma-splice", r, Severity::Warning, msg, vec![fixed]);
                }
                continue;
            }
            // Subject, optional adverb, finite verb: ", we just need", ", it was".
            let mut v = k + 1;
            if matches!(cx.w(v), "just" | "also" | "often" | "still" | "always" | "never" | "usually" | "really" | "only" | "then" | "now") && v + 1 < s.words.end {
                v += 1;
            }
            let personal = matches!(w, "i" | "you" | "he" | "she" | "we" | "they");
            let finite = VERB.contains(&cx.w(v))
                || matches!(base_of(cx.w(v)), Some((VerbForm::Past | VerbForm::Third, _)))
                || (personal && matches!(base_of(cx.w(v)), Some((VerbForm::Base, _))));
            const LINKING: &[&str] = &["however", "therefore", "moreover", "consequently", "thus", "nevertheless", "furthermore", "also", "instead", "meanwhile", "otherwise", "indeed", "then", "finally", "first", "firstly", "secondly", "yes", "no", "well", "now", "today", "yesterday", "recently", "unfortunately", "fortunately"];
            let clause_start = cx.src[s.range.start..cx.p.words[k].range.start].rfind([';', ':']).is_some();
            if SUBJ.contains(&w) && finite && !before_has_sub && !clause_start && !LINKING.contains(&cx.w(k - 1)) && k - first > 2 && !matches!(cx.w(k - 1), "and" | "but" | "so" | "or" | "yet") {
                let r = cx.p.words[k - 1].range.end..cx.p.words[k].range.start;
                cx.push(
                    "structure.comma-splice",
                    r,
                    Severity::Warning,
                    "Two complete sentences are joined by just a comma.",
                    vec![". ".into(), "; ".into(), ", and ".into()],
                );
            }
        }
    }
    // Same opening word three times in a row.
    for win in sentences.windows(3) {
        let first: Vec<&str> = win.iter().filter_map(|s| cx.p.words.get(s.words.start).map(|w| w.lower.as_str())).collect();
        if first.len() == 3 && first[0] == first[1] && first[1] == first[2] && win.iter().all(|s| !s.words.is_empty()) {
            let w = win[2].words.start;
            let msg = format!("Three sentences in a row start with “{}”.", cx.orig(w));
            cx.push("structure.repetitive-start", cx.p.words[w].range.clone(), Severity::Hint, msg, vec![]);
        }
    }
    for (pi, para) in cx.p.paragraphs.iter().enumerate() {
        let words: usize = cx.p.sentences.iter().filter(|s| s.paragraph == pi && !s.heading).map(|s| s.words.len()).sum();
        if words > 180 {
            let first_sentence = cx.p.sentences.iter().find(|s| s.paragraph == pi).map(|s| s.range.clone()).unwrap_or(para.clone());
            cx.push("structure.paragraph", first_sentence, Severity::Hint, format!("This paragraph has {words} words. Does it hold more than one idea?"), vec![]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::text::prose;

    fn run(t: &str) -> Vec<(&'static str, String, Vec<String>)> {
        let p = prose(t);
        let mut v: Vec<_> = check(t, &p, None).into_iter().map(|i| (i.rule, t[i.range].to_string(), i.suggestions)).collect();
        v.sort_by_key(|x| x.1.clone());
        v
    }

    fn rules(t: &str) -> Vec<&'static str> {
        let mut r: Vec<_> = run(t).into_iter().map(|x| x.0).collect();
        r.sort();
        r
    }

    #[test]
    fn every_rule_has_a_lesson() {
        let mut seen = HashSet::new();
        for r in RULES {
            assert!(seen.insert(r.id), "duplicate {}", r.id);
            assert!(!r.lesson.is_empty());
        }
        for (_, _, rule) in PHRASES {
            assert_eq!(info(rule).id, *rule);
        }
    }

    #[test]
    fn articles() {
        let r = run("She is a honest person with an unique idea and an apple, a university and an MBA. A hour passed.");
        let found: Vec<_> = r.iter().filter(|x| x.0 == "grammar.article").map(|x| (x.1.as_str(), x.2[0].as_str())).collect();
        assert_eq!(found, [("A", "An"), ("a", "an"), ("an", "a")]);
    }

    #[test]
    fn agreement_and_verbs() {
        let r = rules("She have two brothers. They was happy. He go to school. I is late. Did he go home?");
        assert_eq!(r.iter().filter(|x| **x == "grammar.agreement").count(), 4);
        let r = run("He didn't went home. She can speaks French. You must to go. I want to went.");
        let fixes: Vec<_> = r.iter().filter(|x| x.0 == "grammar.verb-form").map(|x| x.2[0].clone()).collect();
        assert_eq!(fixes.len(), 4, "{r:?}");
        assert!(fixes.contains(&"go".to_string()) && fixes.contains(&"speak".to_string()));
        // Correct sentences stay quiet.
        assert!(rules("He goes to school. They were happy. She can speak French. You must go. Did she have time?").is_empty());
    }

    #[test]
    fn confused_words() {
        let r = run("Its a good day and your welcome. It is bigger then mine. I could of known. Their is a cat.");
        let fixes: Vec<_> = r.iter().filter(|x| x.0 == "grammar.confused").map(|x| x.2[0].clone()).collect();
        assert_eq!(fixes.len(), 5, "{r:?}");
        assert!(rules("It's a good day and you're welcome. Its colour is red. There is a cat.").is_empty());
        let r = run("They spend to much time. It has more advantages then disadvantages. I went home and then slept.");
        let fixes: Vec<_> = r.iter().filter(|x| x.0 == "grammar.confused").map(|x| x.2[0].clone()).collect();
        assert_eq!(fixes, ["than", "too"]);
    }

    #[test]
    fn nouns_and_quantifiers() {
        let r = rules("She gave me an advice and some informations. There are many student here. It is one of the best school. There were much problems.");
        assert_eq!(r, ["grammar.plural", "grammar.plural", "grammar.quantifier", "grammar.uncountable", "grammar.uncountable"]);
    }

    #[test]
    fn capitalization_and_punctuation() {
        let r = rules("i study english on monday. my teacher is kind , really!! Ok.Next we go");
        assert!(r.iter().filter(|x| **x == "grammar.capitalization").count() >= 3, "{r:?}");
        assert!(r.contains(&"punct.spacing") && r.contains(&"punct.repeated"));
        assert!(r.contains(&"punct.missing-end") || r.len() > 5);
    }

    #[test]
    fn clarity_rules() {
        let r = rules("Due to the fact that we made a decision, the report was written by Anna. The end result was clear.");
        assert!(r.contains(&"clarity.wordy") && r.contains(&"clarity.nominalization") && r.contains(&"clarity.passive") && r.contains(&"clarity.redundant"), "{r:?}");
        // Adjectives after "be" are not passive.
        assert!(!rules("I was tired and interested.").contains(&"clarity.passive"));
    }

    #[test]
    fn structure_rules() {
        let r = rules("It was raining, we stayed inside. We stayed home. Because it was cold.");
        assert!(r.contains(&"structure.comma-splice") && r.contains(&"structure.fragment"), "{r:?}");
        let r = rules("I woke up. I ate. I left.");
        assert_eq!(r, ["structure.repetitive-start"]);
        assert!(!rules("Although it was raining, we stayed inside.").contains(&"structure.comma-splice"));
    }

    #[test]
    fn spelling_and_repeats() {
        assert_eq!(run("Two goverments agreed.")[0].2, ["governments"]);
        assert_eq!(run("I think it benefit everyone.")[0].2, ["benefits"]);
        assert!(rules("Let it work. It works.").is_empty());
        let r = run("I recieved the the message definately.");
        let mut kinds: Vec<_> = r.iter().map(|x| x.0).collect();
        kinds.sort();
        assert_eq!(kinds, ["grammar.repeated-word", "spelling.common", "spelling.common"]);
        assert!(r.iter().any(|x| x.2 == ["received"]));
    }
}
