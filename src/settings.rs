//! User preferences, persisted as JSON in the XDG config directory.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::focus::FocusMode;
use crate::library::SortBy;
use crate::stats::StatKind;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum FontChoice {
    Mono,
    #[default]
    Duo,
    Quattro,
}

impl FontChoice {
    pub fn family(self) -> &'static str {
        match self {
            FontChoice::Mono => crate::fonts::MONO,
            FontChoice::Duo => crate::fonts::DUO,
            FontChoice::Quattro => crate::fonts::QUATTRO,
        }
    }

}

/// Preview/export templates bundled with the original app.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Template {
    Duo,
    Mono,
    Quattro,
    /// The original's default ("Classic (Serif)").
    #[default]
    Serif,
    Sans,
    GitHub,
}

impl Template {
    pub const ALL: [Template; 6] = [
        Template::Serif,
        Template::Sans,
        Template::Duo,
        Template::Mono,
        Template::Quattro,
        Template::GitHub,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Template::Duo => "Manuscript (Duo)",
            Template::Mono => "Manuscript (Mono)",
            Template::Quattro => "Manuscript (Quattro)",
            Template::Serif => "Classic (Serif)",
            Template::Sans => "Modern (Sans)",
            Template::GitHub => "GitHub",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum PreviewLayout {
    #[default]
    Split,
    Full,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum BarMode {
    #[default]
    FadeInOut,
    AlwaysShow,
    Hide,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    pub font: FontChoice,
    /// Index into `TEXT_SIZES`.
    pub text_size: usize,
    pub focus: FocusMode,
    /// Focus mode remembers which granularity to use when toggled back on.
    pub focus_kind: FocusMode,
    pub typewriter: bool,
    pub syntax: bool,
    pub syntax_adjectives: bool,
    pub syntax_nouns: bool,
    pub syntax_adverbs: bool,
    pub syntax_verbs: bool,
    pub syntax_conjunctions: bool,
    pub style_check: bool,
    pub check_fillers: bool,
    pub check_cliches: bool,
    pub check_redundancies: bool,
    pub check_custom: bool,
    pub show_authors: bool,
    pub custom_patterns: Vec<String>,
    pub library_dir: Option<PathBuf>,
    pub show_library: bool,
    pub show_organizer: bool,
    pub favorites: Vec<PathBuf>,
    pub smart_folders: Vec<String>,
    pub show_sort_bar: bool,
    pub always_show_tabs: bool,
    pub sort: SortBy,
    pub preview: bool,
    pub preview_layout: PreviewLayout,
    pub template: Template,
    pub highlight_style: crate::theme::HighlightStyle,
    pub stat: StatKind,
    pub title_bar: BarMode,
    pub toolbar: BarMode,
    pub smart_quotes: bool,
    pub smart_dashes: bool,
    pub auto_pair: bool,
    pub spell: bool,
    pub spell_lang: String,
    /// Required by the read/write URL commands.
    pub url_token: String,
    pub recent: Vec<PathBuf>,
    pub window_width: f32,
    pub window_height: f32,
    pub sidebar_width: f32,
    /// Writing Studio panel shown at the right of the editor.
    pub studio_open: bool,
    /// Analyze and underline while typing even when the panel is closed.
    pub studio_live: bool,
    /// Show suggested fixes immediately (off: the writer tries first).
    pub studio_reveal: bool,
    /// Index into `studio::assistant::LEVELS`.
    pub studio_level: usize,
    /// Claude model for the coach over the API; empty = default.
    pub studio_model: String,
    /// How the coach reaches Claude: the local Claude Code CLI or the API.
    pub studio_backend: crate::studio::llm::Backend,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            font: FontChoice::Duo,
            text_size: DEFAULT_TEXT_SIZE,
            focus: FocusMode::Off,
            focus_kind: FocusMode::Sentence,
            typewriter: false,
            syntax: false,
            syntax_adjectives: true,
            syntax_nouns: true,
            syntax_adverbs: true,
            syntax_verbs: true,
            syntax_conjunctions: true,
            style_check: false,
            check_fillers: true,
            check_cliches: true,
            check_redundancies: true,
            check_custom: true,
            show_authors: false,
            custom_patterns: Vec::new(),
            library_dir: None,
            show_library: true,
            show_organizer: true,
            favorites: Vec::new(),
            smart_folders: Vec::new(),
            show_sort_bar: false,
            always_show_tabs: false,
            sort: SortBy::Modified,
            preview: false,
            preview_layout: PreviewLayout::Split,
            template: Template::Serif,
            highlight_style: crate::theme::HighlightStyle::Yellow,
            stat: StatKind::Words,
            title_bar: BarMode::FadeInOut,
            toolbar: BarMode::FadeInOut,
            smart_quotes: false,
            smart_dashes: true,
            auto_pair: true,
            spell: true,
            spell_lang: "en_US".into(),
            url_token: String::new(),
            recent: Vec::new(),
            window_width: 1200.,
            window_height: 800.,
            sidebar_width: 280.,
            studio_open: false,
            studio_live: false,
            studio_reveal: false,
            studio_level: 1,
            studio_model: String::new(),
            studio_backend: Default::default(),
        }
    }
}

/// (font size, line height) in px. Mirrors the metric table of iA's typography:
/// the default "L" step is 18 px with a 175 % line height.
pub const TEXT_SIZES: [(f32, f32); 9] = [
    (13., 1.79),
    (14., 1.78),
    (15., 1.77),
    (16., 1.76),
    (18., 1.75),
    (20., 1.73),
    (24., 1.69),
    (28., 1.65),
    (32., 1.61),
];
pub const DEFAULT_TEXT_SIZE: usize = 4;

impl Settings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("quill").join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(p) = Self::path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, json);
        }
    }

    pub fn library_dir(&self) -> PathBuf {
        self.library_dir.clone().unwrap_or_else(|| {
            dirs::document_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Documents"))
                .join("Quill")
        })
    }

    pub fn font_metrics(&self) -> (f32, f32) {
        let (size, lh) = TEXT_SIZES[self.text_size.min(TEXT_SIZES.len() - 1)];
        // Proportional Quattro is set 11 points tighter than Mono/Duo in iA's table.
        let lh = if self.font == FontChoice::Quattro { lh - 0.11 } else { lh };
        (size, (size * lh).round())
    }

    pub fn push_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_json() {
        let s = Settings {
            typewriter: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        // Missing fields fall back to defaults so old files keep loading.
        let old: Settings = serde_json::from_str(r#"{"typewriter":true}"#).unwrap();
        assert_eq!(old, s);
    }

    #[test]
    fn default_metrics_match_ia() {
        assert_eq!(Settings::default().font_metrics(), (18., 32.));
    }

    #[test]
    fn recent_is_deduplicated() {
        let mut s = Settings::default();
        s.push_recent("a".into());
        s.push_recent("b".into());
        s.push_recent("a".into());
        assert_eq!(s.recent, vec![PathBuf::from("a"), PathBuf::from("b")]);
    }
}
