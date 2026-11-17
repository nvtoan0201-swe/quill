//! Spell checking through the system's Enchant (hunspell/aspell dictionaries).

use std::collections::HashSet;
use std::io::Write;
use std::process::{Command, Stdio};

use unicode_segmentation::UnicodeSegmentation;

/// Words worth checking: plain ASCII letters (accented words are skipped so
/// Vietnamese or other languages without a dictionary are not flagged).
pub fn candidates(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    text.unicode_words()
        .filter(|w| w.len() > 1 && w.chars().all(|c| c.is_ascii_alphabetic() || c == '\''))
        .filter(|w| seen.insert(w.to_string()))
        .map(str::to_string)
        .collect()
}

/// Misspelled words among `words`, or None when no checker is installed.
pub fn misspelled(words: &[String], lang: &str) -> Option<HashSet<String>> {
    if words.is_empty() {
        return Some(HashSet::new());
    }
    let mut child = Command::new("enchant-2")
        .args(["-l", "-d", lang])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .or_else(|_| {
            Command::new("aspell")
                .args(["list", "-l", lang])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        })
        .ok()?;
    let input = words.join("\n");
    child.stdin.take()?.write_all(input.as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_skip_accents_and_duplicates() {
        let c = candidates("Hello hello Xin chào thế giới don't a");
        assert_eq!(c, ["Hello", "hello", "Xin", "don't"]);
    }

    #[test]
    #[ignore = "needs enchant-2 with an en_US dictionary"]
    fn finds_typos() {
        let bad = misspelled(&["hello".into(), "wrold".into()], "en_US").unwrap();
        assert!(bad.contains("wrold") && !bad.contains("hello"));
    }
}
