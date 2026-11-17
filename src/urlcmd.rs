//! URL commands (`quill://command?…`), mirroring the original's documented
//! `ia-writer://` scheme: open, new, quick-search, read, write, version,
//! with optional x-callback-url return values.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

#[derive(Debug, PartialEq)]
pub struct UrlCommand {
    pub name: String,
    pub params: HashMap<String, String>,
    pub callback: bool,
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn parse(url: &str) -> Option<UrlCommand> {
    let rest = url.strip_prefix("quill://")?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (callback, name) = match path.strip_prefix("x-callback-url/") {
        Some(n) => (true, n),
        None => (false, path),
    };
    let params = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (percent_decode(k), percent_decode(v))
        })
        .collect();
    Some(UrlCommand { name: name.trim_end_matches('/').to_string(), params, callback })
}

/// "/File.md" is relative to the library root; absolute paths outside it are refused.
pub fn library_path(root: &Path, p: &str) -> Result<PathBuf> {
    let rel = p.trim_start_matches('/');
    let path = root.join(rel);
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        bail!("path must stay inside the library");
    }
    Ok(path)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteMode {
    Create,
    Replace,
    Add,
    Patch,
}

impl WriteMode {
    pub fn parse(s: Option<&str>) -> Result<Self> {
        Ok(match s.unwrap_or("create") {
            "create" => WriteMode::Create,
            "replace" => WriteMode::Replace,
            "add" | "append" => WriteMode::Add,
            "patch" => WriteMode::Patch,
            other => bail!("unknown mode {other}"),
        })
    }
}

/// New contents for `mode: add`.
pub fn add_text(existing: &str, text: &str, at_beginning: bool, padding: &str) -> String {
    let sep = match padding {
        "sentence" => " ",
        "paragraph" => "\n\n",
        _ => "\n",
    };
    if existing.trim().is_empty() {
        return text.to_string();
    }
    if at_beginning {
        format!("{text}{sep}{}", existing.trim_start())
    } else {
        format!("{}{sep}{text}", existing.trim_end())
    }
}

/// Build the x-success URL with return parameters appended.
pub fn callback_url(base: &str, params: &[(&str, &str)]) -> String {
    let q = params.iter().map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v))).collect::<Vec<_>>().join("&");
    if q.is_empty() {
        base.to_string()
    } else if base.contains('?') {
        format!("{base}&{q}")
    } else {
        format!("{base}?{q}")
    }
}

pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_and_params() {
        let c = parse("quill://x-callback-url/write?path=%2FInbox%2FNote.md&text=Hello%20world&mode=add&x-success=app%3A%2F%2Fok").unwrap();
        assert_eq!(c.name, "write");
        assert!(c.callback);
        assert_eq!(c.params["path"], "/Inbox/Note.md");
        assert_eq!(c.params["text"], "Hello world");
        assert_eq!(c.params["x-success"], "app://ok");
        let c = parse("quill://quick-search?query=a+b").unwrap();
        assert_eq!((c.name.as_str(), c.params["query"].as_str(), c.callback), ("quick-search", "a b", false));
        assert!(parse("https://x").is_none());
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }

    #[test]
    fn paths_and_modes() {
        let root = Path::new("/lib");
        assert_eq!(library_path(root, "/a/b.md").unwrap(), Path::new("/lib/a/b.md"));
        assert!(library_path(root, "/../etc/passwd").is_err());
        assert_eq!(WriteMode::parse(Some("patch")).unwrap(), WriteMode::Patch);
        assert!(WriteMode::parse(Some("nope")).is_err());
        assert_eq!(add_text("One.", "Two.", false, "sentence"), "One. Two.");
        assert_eq!(add_text("Body\n", "Top", true, "paragraph"), "Top\n\nBody\n");
        assert_eq!(add_text("", "x", false, "line"), "x");
        assert_eq!(callback_url("app://ok", &[("path", "/a b.md")]), "app://ok?path=%2Fa%20b.md");
        assert_eq!(new_token().len(), 32);
    }
}
