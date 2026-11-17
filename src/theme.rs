//! Colors. Editor values are sampled from iA's own screenshots on ia.net
//! (bg #F7F7F7 / #1A1A1A, focus-dimmed #C2C1BE, caret #18C4F5, Markdown marks in
//! the text color); chrome values come from the app's asset catalogs
//! (#8E8E93, folder #00C7FF); preview values from its stylesheet (#fcfcfc).

use gpui::{Global, Hsla, Rgba, rgb, rgba};

use crate::syntax::Pos;

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: Hsla,
    pub text: Hsla,
    /// Text inside the focused sentence/paragraph.
    pub focus_text: Hsla,
    pub preview_bg: Hsla,
    pub markup: Hsla,
    pub dimmed: Hsla,
    pub caret: Hsla,
    pub selection: Hsla,
    pub highlight: Hsla,
    pub highlight_text: Hsla,
    pub highlight_underline: Hsla,
    pub find_match: Hsla,
    pub code_bg: Hsla,
    pub check: Hsla,
    pub misspelled: Hsla,
    /// Writing Studio: grammar/spelling/punctuation mistakes and style advice.
    pub studio_mistake: Hsla,
    pub studio_style: Hsla,
    pub ai_text: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    pub separator: Hsla,
    pub sidebar_bg: Hsla,
    // Window chrome, sampled from the original's Library/Organizer screenshot.
    pub organizer_bg: Hsla,
    pub organizer_text: Hsla,
    pub section_header: Hsla,
    pub list_bg: Hsla,
    pub list_title: Hsla,
    pub list_meta: Hsla,
    pub list_separator: Hsla,
    pub sortbar_bg: Hsla,
    pub toolbar_bg: Hsla,
    pub toolbar_border: Hsla,
    pub toolbar_title: Hsla,
    pub toolbar_icon: Hsla,
    pub divider: Hsla,
    pub doc_icon: Hsla,
    pub selection_bar: Hsla,
    /// macOS folder blue, as the original's Organizer uses system folder icons.
    pub system_folder: Hsla,
    pub row_selected: Hsla,
    pub row_selected_text: Hsla,
    pub hover: Hsla,
    pub popover_bg: Hsla,
    pub popover_border: Hsla,
    pub accent: Hsla,
    pub folder: Hsla,
    pub adjective: Hsla,
    pub noun: Hsla,
    pub adverb: Hsla,
    pub verb: Hsla,
    pub conjunction: Hsla,
}

impl Global for Theme {}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn ca(hex: u32) -> Hsla {
    let r: Rgba = rgba(hex);
    r.into()
}

/// Text highlight styles of the original (Kit's IATextHighlight* tables, decoded
/// from OKLCH): (text, background, underline) as RGBA for light and dark.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum HighlightStyle {
    #[default]
    Yellow,
    Orange,
    Pink,
    Purple,
    Blue,
    Green,
}

impl HighlightStyle {
    pub const ALL: [HighlightStyle; 6] = [
        HighlightStyle::Yellow,
        HighlightStyle::Orange,
        HighlightStyle::Pink,
        HighlightStyle::Purple,
        HighlightStyle::Blue,
        HighlightStyle::Green,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HighlightStyle::Yellow => "Yellow",
            HighlightStyle::Orange => "Orange",
            HighlightStyle::Pink => "Pink",
            HighlightStyle::Purple => "Purple",
            HighlightStyle::Blue => "Blue",
            HighlightStyle::Green => "Green",
        }
    }

    /// (text, background, underline), each 0xRRGGBBAA.
    pub fn colors(self, dark: bool) -> (u32, u32, u32) {
        use HighlightStyle::*;
        match (self, dark) {
            (Yellow, false) => (0x1a1a1aff, 0xea8c004a, 0xea8c006e),
            (Yellow, true) => (0xffd265ff, 0xffd2652b, 0xffd265bf),
            (Orange, false) => (0x1a1a1aff, 0xda5b0e40, 0xe4500080),
            (Orange, true) => (0xfa9567ff, 0xff7a3333, 0xffa378bf),
            (Pink, false) => (0x1a1a1aff, 0xff008c26, 0xa72e5f4f),
            (Pink, true) => (0xe798b0ff, 0xe65b8f3b, 0xff9ec2bf),
            (Purple, false) => (0x1a1a1aff, 0x9c3cff33, 0x714ea880),
            (Purple, true) => (0xd0aaffff, 0xaf76ff3b, 0xc0a1ffc2),
            (Blue, false) => (0x1a1a1aff, 0x00c3ff33, 0x1b88aa80),
            (Blue, true) => (0x8ad7eaff, 0x7fb3e42e, 0x8ad7eabf),
            (Green, false) => (0x1a1a1aff, 0xa2dc6e59, 0x6f9c4880),
            (Green, true) => (0xabd886ff, 0xa4db7529, 0xb0d692bf),
        }
    }
}

impl Theme {
    pub fn with_highlight(mut self, style: HighlightStyle) -> Self {
        let (t, b, u) = style.colors(self.dark);
        self.highlight_text = ca(t);
        self.highlight = ca(b);
        self.highlight_underline = ca(u);
        self
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            bg: c(0xf7f7f7),
            text: c(0x141414),
            focus_text: c(0x141414),
            preview_bg: c(0xfcfcfc),
            markup: c(0x2e2e2e),
            dimmed: c(0xc2c1be),
            caret: c(0x18c4f5),
            selection: ca(0x18c4f540),
            highlight: ca(0xea8c004a),
            highlight_text: c(0x1a1a1a),
            highlight_underline: ca(0xea8c006e),
            find_match: ca(0xffcc0066),
            code_bg: ca(0x2424240e),
            check: c(0xe8a03a),
            misspelled: c(0xff3b30),
            studio_mistake: c(0xf06a2a),
            studio_style: c(0x3b8cf0),
            ai_text: c(0x9a9aa0),
            secondary: c(0x8e8e93),
            tertiary: c(0xc0c0c4),
            separator: c(0xe5e5e5),
            sidebar_bg: c(0xefefef),
            organizer_bg: c(0xe9e9e9),
            organizer_text: c(0x262626),
            section_header: c(0xa9a9a9),
            list_bg: c(0xfcfcfc),
            list_title: c(0x1a1a1a),
            list_meta: c(0x999999),
            list_separator: c(0xefefef),
            sortbar_bg: c(0xfafafa),
            toolbar_bg: c(0xfbfbfb),
            toolbar_border: c(0xe3e3e3),
            toolbar_title: c(0x616161),
            toolbar_icon: c(0x747474),
            divider: c(0xd9d9d9),
            doc_icon: c(0xc4c4c4),
            selection_bar: c(0x00c1ff),
            system_folder: c(0x4fa8ec),
            row_selected: c(0x0a7aff),
            row_selected_text: c(0xffffff),
            hover: ca(0x0000000d),
            popover_bg: c(0xffffff),
            popover_border: ca(0x00000026),
            accent: c(0x0088ff),
            folder: c(0x00c7ff),
            adjective: c(0xb4782e),
            noun: c(0xd0504f),
            adverb: c(0xa45bc4),
            verb: c(0x2f8ad9),
            conjunction: c(0x3b9a57),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: c(0x1a1a1a),
            text: c(0xc5c5c5),
            focus_text: c(0xf2f2f2),
            preview_bg: c(0x101010),
            markup: c(0xb0b0b0),
            dimmed: c(0x5b5b5b),
            caret: c(0x18c4f5),
            selection: ca(0x18c4f540),
            highlight: ca(0xffd2652b),
            highlight_text: c(0xffd265),
            highlight_underline: ca(0xffd265bf),
            find_match: ca(0xffcc0055),
            code_bg: ca(0xf5f5f50f),
            check: c(0xd9922e),
            misspelled: c(0xff453a),
            studio_mistake: c(0xff8a4c),
            studio_style: c(0x5aa2ff),
            ai_text: c(0x6e6e74),
            secondary: c(0x8e8e93),
            tertiary: c(0x55555a),
            separator: c(0x2a2a2a),
            sidebar_bg: c(0x222222),
            organizer_bg: c(0x262626),
            organizer_text: c(0xd6d6d6),
            section_header: c(0x6e6e6e),
            list_bg: c(0x1e1e1e),
            list_title: c(0xdedede),
            list_meta: c(0x7a7a7a),
            list_separator: c(0x2a2a2a),
            sortbar_bg: c(0x202020),
            toolbar_bg: c(0x1d1d1d),
            toolbar_border: c(0x303030),
            toolbar_title: c(0xa8a8a8),
            toolbar_icon: c(0x8c8c8c),
            divider: c(0x000000),
            doc_icon: c(0x555555),
            selection_bar: c(0x00c1ff),
            system_folder: c(0x4fa8ec),
            row_selected: c(0x0a84ff),
            row_selected_text: c(0xffffff),
            hover: ca(0xffffff12),
            popover_bg: c(0x262626),
            popover_border: ca(0xffffff24),
            accent: c(0x0a84ff),
            folder: c(0x00c7ff),
            adjective: c(0xd5a05c),
            noun: c(0xec7a72),
            adverb: c(0xc489e0),
            verb: c(0x5eaaf0),
            conjunction: c(0x62bf7d),
        }
    }

    pub fn pos(&self, p: Pos) -> Hsla {
        match p {
            Pos::Adjective => self.adjective,
            Pos::Noun => self.noun,
            Pos::Adverb => self.adverb,
            Pos::Verb => self.verb,
            Pos::Conjunction => self.conjunction,
        }
    }
}
