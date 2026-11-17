//! `quill --analyze [--template ID] FILE…`: the Studio report in the terminal.

use super::analysis::{self, Options};
use super::templates;

pub fn analyze(args: &[String]) -> i32 {
    let mut template = None;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--template" {
            template = it.next().cloned();
        } else {
            files.push(a.clone());
        }
    }
    if files.is_empty() {
        eprintln!("usage: quill --analyze [--template ID] FILE…\ntemplates: {}", templates::load_all().iter().map(|t| t.id.clone()).collect::<Vec<_>>().join(", "));
        return 2;
    }
    let tpl = template.and_then(|id| templates::load_all().into_iter().find(|t| t.id == id));
    let mut status = 0;
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            eprintln!("{f}: cannot read");
            status = 1;
            continue;
        };
        let misspelled = crate::spell::misspelled(&crate::spell::candidates(&text), "en_US");
        let opts = Options {
            register: tpl.as_ref().map(|t| t.register).unwrap_or_default(),
            misspelled,
            disabled: tpl.as_ref().map(|t| t.disable.iter().cloned().collect()).unwrap_or_default(),
            grade_band: tpl.as_ref().and_then(|t| t.grade_band),
        };
        let r = analysis::analyze(&text, &opts);
        println!("{f}: {} words, overall {:.0}/100", r.words, r.overall());
        println!("  scores: {}", r.scores.iter().map(|(k, v)| format!("{k} {v:.0}")).collect::<Vec<_>>().join(", "));
        println!("  readability: grade {:.1} ({}), Flesch {:.0}; register {}; MTLD {:.0}", r.readability.grade(), r.readability.ease_label(), r.readability.flesch, r.tone.register.label(), r.vocab.mtld);
        for i in &r.issues {
            let line = text[..i.range.start].matches('\n').count() + 1;
            let sugg = if i.suggestions.is_empty() { String::new() } else { format!(" → {}", i.suggestions.iter().map(|s| format!("“{s}”")).collect::<Vec<_>>().join(" / ")) };
            println!("  {line}: [{}] “{}” {}{sugg}", i.rule, &text[i.range.clone()], i.message);
        }
        if let Some(t) = &tpl {
            for q in templates::requirements(t, &text, &r) {
                println!("  {} {}: {}", if q.ok { "✓" } else { "✗" }, q.label, q.detail);
            }
        }
    }
    status
}
