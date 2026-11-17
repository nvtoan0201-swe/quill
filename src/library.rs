//! Library: the folder of plain-text documents shown in the sidebar.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::markdown;

pub const EXTENSIONS: [&str; 4] = ["md", "markdown", "txt", "text"];

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub title: String,
    pub excerpt: String,
    pub modified: SystemTime,
    /// Number of documents inside (folders only).
    pub count: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum SortBy {
    Name,
    #[default]
    Modified,
}

pub fn is_document(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_lowercase().as_str()))
}

fn hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}

pub fn list(dir: &Path, sort: SortBy) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in fs::read_dir(dir)? {
        let path = item?.path();
        if hidden(&path) {
            continue;
        }
        let meta = fs::metadata(&path)?;
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        if meta.is_dir() {
            let count = fs::read_dir(&path)
                .map(|rd| rd.flatten().filter(|e| is_document(&e.path())).count())
                .unwrap_or(0);
            out.push(Entry {
                title: file_stem(&path),
                excerpt: String::new(),
                path,
                is_dir: true,
                modified,
                count,
            });
        } else if is_document(&path) {
            let text = read_head(&path, 4096);
            out.push(Entry {
                title: file_stem(&path),
                excerpt: markdown::excerpt(&text, 140),
                path,
                is_dir: false,
                modified,
                count: 0,
            });
        }
    }
    out.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| match sort {
            SortBy::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            SortBy::Modified => b.modified.cmp(&a.modified),
        })
    });
    Ok(out)
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn read_head(path: &Path, max: usize) -> String {
    let bytes = fs::read(path).unwrap_or_default();
    let mut end = bytes.len().min(max);
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() {
        end -= 1;
    }
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: PathBuf,
    pub title: String,
    /// Matching line, or empty when only the file name matched.
    pub line: String,
}

/// Quick Search: file names first, then file contents. Recursive.
pub fn search(root: &Path, query: &str, limit: usize) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    let mut names = Vec::new();
    let mut contents = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if hidden(&p) {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !is_document(&p) {
                continue;
            }
            let title = file_stem(&p);
            if q.is_empty() || title.to_lowercase().contains(&q) {
                names.push(Hit { path: p, title, line: String::new() });
            } else if let Ok(text) = fs::read_to_string(&p)
                && let Some(line) = text.lines().find(|l| l.to_lowercase().contains(&q))
            {
                contents.push(Hit { path: p, title, line: line.trim().chars().take(120).collect() });
            }
        }
    }
    names.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    names.extend(contents);
    names.truncate(limit);
    names
}

fn documents(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if hidden(&p) {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if is_document(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// `#hashtags` used anywhere in the library, with document counts (Smart Folders).
pub fn tags(root: &Path) -> Vec<(String, usize)> {
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for p in documents(root) {
        let Ok(text) = fs::read_to_string(&p) else { continue };
        let flags = markdown::style(&text);
        let mut seen = std::collections::HashSet::new();
        let mut i = 0;
        while i < flags.len() {
            if flags[i] & markdown::HASHTAG != 0 {
                let start = i;
                while i < flags.len() && flags[i] & markdown::HASHTAG != 0 {
                    i += 1;
                }
                let tag = text[start + 1..i].to_lowercase();
                if seen.insert(tag.clone()) {
                    *counts.entry(tag).or_default() += 1;
                }
            } else {
                i += 1;
            }
        }
    }
    counts.into_iter().collect()
}

/// Documents containing `#tag`, as library entries.
pub fn tagged(root: &Path, tag: &str, sort: SortBy) -> Vec<Entry> {
    let needle = format!("#{}", tag.to_lowercase());
    let mut out: Vec<Entry> = documents(root)
        .into_iter()
        .filter_map(|p| {
            let text = fs::read_to_string(&p).ok()?;
            let lower = text.to_lowercase();
            let found = lower.match_indices(&needle).any(|(i, _)| {
                let after = lower[i + needle.len()..].chars().next();
                !after.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-')
            });
            found.then(|| Entry {
                title: file_stem(&p),
                excerpt: markdown::excerpt(&text, 140),
                modified: fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH),
                path: p,
                is_dir: false,
                count: 0,
            })
        })
        .collect();
    out.sort_by(|a, b| match sort {
        SortBy::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        SortBy::Modified => b.modified.cmp(&a.modified),
    });
    out
}

/// Smart Folder: documents whose name or text contains `query` (case-insensitive).
pub fn matching(root: &Path, query: &str, sort: SortBy) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<Entry> = documents(root)
        .into_iter()
        .filter_map(|p| {
            let text = fs::read_to_string(&p).ok()?;
            let hit = q.is_empty() || file_stem(&p).to_lowercase().contains(&q) || text.to_lowercase().contains(&q);
            hit.then(|| Entry {
                title: file_stem(&p),
                excerpt: markdown::excerpt(&text, 140),
                modified: fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH),
                path: p,
                is_dir: false,
                count: 0,
            })
        })
        .collect();
    out.sort_by(|a, b| match sort {
        SortBy::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        SortBy::Modified => b.modified.cmp(&a.modified),
    });
    out
}

/// Local UTC offset in seconds (from `date +%z`, cached).
pub fn utc_offset() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        let out = std::process::Command::new("date").arg("+%z").output().ok();
        let s = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        parse_offset(&s)
    })
}

fn parse_offset(s: &str) -> i64 {
    if s.len() != 5 {
        return 0;
    }
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let h: i64 = s[1..3].parse().unwrap_or(0);
    let m: i64 = s[3..5].parse().unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

/// File list date like the original: "12:28" today, otherwise "2024/02/22".
pub fn list_date(t: SystemTime, now: SystemTime, offset: i64) -> String {
    let secs = |t: SystemTime| t.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as i64 + offset;
    let (a, b) = (secs(t), secs(now));
    if a.div_euclid(86_400) == b.div_euclid(86_400) {
        let s = a.rem_euclid(86_400);
        format!("{:02}:{:02}", s / 3600, s / 60 % 60)
    } else {
        let (y, m, d) = civil_from_days(a.div_euclid(86_400));
        format!("{y:04}/{m:02}/{d:02}")
    }
}

/// A file name that does not exist yet in `dir`: "Untitled.md", "Untitled 2.md", …
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} {n}.{ext}"));
        n += 1;
    }
    p
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's algorithm.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

pub fn today_iso() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("quill-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lists_documents_and_folders() {
        let d = tmp("list");
        fs::write(d.join("b.md"), "# B title\n\nBody of b.").unwrap();
        fs::write(d.join("a.txt"), "Plain a").unwrap();
        fs::write(d.join("image.png"), "x").unwrap();
        fs::write(d.join(".hidden.md"), "x").unwrap();
        fs::create_dir(d.join("Notes")).unwrap();
        fs::write(d.join("Notes/n.md"), "n").unwrap();
        let l = list(&d, SortBy::Name).unwrap();
        let titles: Vec<&str> = l.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["Notes", "a", "b"]);
        assert_eq!(l[0].count, 1);
        assert_eq!(l[2].excerpt, "B title Body of b.");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn search_names_then_content() {
        let d = tmp("search");
        fs::write(d.join("Groceries.md"), "milk").unwrap();
        fs::create_dir(d.join("sub")).unwrap();
        fs::write(d.join("sub/Plan.md"), "buy groceries today").unwrap();
        let hits = search(&d, "grocer", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Groceries");
        assert_eq!(hits[1].line, "buy groceries today");
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tags_and_tagged_documents() {
        let d = tmp("tags");
        fs::write(d.join("a.md"), "Plan #work and #home").unwrap();
        fs::create_dir(d.join("sub")).unwrap();
        fs::write(d.join("sub/b.md"), "More #work. `#notatag`").unwrap();
        fs::write(d.join("c.md"), "#workshop only").unwrap();
        assert_eq!(tags(&d), vec![("home".into(), 1), ("work".into(), 2), ("workshop".into(), 1)]);
        let docs: Vec<String> = tagged(&d, "work", SortBy::Name).into_iter().map(|e| e.title).collect();
        assert_eq!(docs, ["a", "b"]);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn list_dates_and_smart_folders() {
        let day = Duration::from_secs(86_400);
        let now = UNIX_EPOCH + day * 20_773 + Duration::from_secs(12 * 3600 + 28 * 60);
        assert_eq!(list_date(now, now, 0), "12:28");
        assert_eq!(list_date(now, now, 7 * 3600), "19:28");
        assert_eq!(list_date(now - day * 268, now, 0), "2026/02/21");
        assert_eq!(parse_offset("+0700"), 25_200);
        assert_eq!(parse_offset("-0130"), -5_400);
        let d = tmp("smart");
        fs::write(d.join("a.md"), "about rabbits").unwrap();
        fs::write(d.join("Rabbit notes.md"), "x").unwrap();
        fs::write(d.join("c.md"), "nothing").unwrap();
        let names: Vec<String> = matching(&d, "rabbit", SortBy::Name).into_iter().map(|e| e.title).collect();
        assert_eq!(names, ["a", "Rabbit notes"]);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn unique_names_and_dates() {
        let d = tmp("unique");
        assert_eq!(unique_path(&d, "Untitled", "md"), d.join("Untitled.md"));
        fs::write(d.join("Untitled.md"), "").unwrap();
        assert_eq!(unique_path(&d, "Untitled", "md"), d.join("Untitled 2.md"));
        fs::remove_dir_all(d).unwrap();

        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_773), (2026, 11, 16));
        let day = Duration::from_secs(86_400);
        let now = UNIX_EPOCH + day * 20_773;
    }
}
