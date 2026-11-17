use std::borrow::Cow;

/// Family names as declared inside the font files (see `fc-scan`).
pub const DUO: &str = "iA Writer Duo";
pub const MONO: &str = "iA Writer Mono";
pub const QUATTRO: &str = "iA Writer Quattro";
/// Bundled UI font, used when Apple's SF Pro is not installed (SF Pro may not be
/// redistributed, so it is only picked up from the user's system).
pub const UI_FALLBACK: &str = "IBM Plex Sans";
const SYSTEM_UI: [&str; 3] = ["SF Pro Text", "SF Pro", "SF Pro Display"];

static UI_FAMILY: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();

/// UI chrome font: SF Pro when installed, otherwise IBM Plex Sans.
pub fn ui() -> &'static str {
    UI_FAMILY.get().copied().unwrap_or(UI_FALLBACK)
}

fn choose_ui(installed: &[String]) -> &'static str {
    SYSTEM_UI.into_iter().find(|f| installed.iter().any(|n| n == f)).unwrap_or(UI_FALLBACK)
}
pub const SERIF: &str = "IBM Plex Serif";

/// Fonts are embedded so the binary runs without installing anything.
/// The iA faces are static instances baked from the variable fonts by
/// `tools/make_static_fonts.py` (GPUI cannot set variation axes on Linux).
pub const EMBEDDED: [&[u8]; 22] = [
    include_bytes!("../assets/fonts/static/iAWriterDuo-Regular.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterDuo-Bold.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterDuo-Italic.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterDuo-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterMono-Regular.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterMono-Bold.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterMono-Italic.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterMono-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterQuattro-Regular.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterQuattro-Bold.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterQuattro-Italic.ttf"),
    include_bytes!("../assets/fonts/static/iAWriterQuattro-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSans-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSerif-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSerif-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSerif-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexSerif-BoldItalic.ttf"),
];

pub fn register(cx: &gpui::App) -> gpui::Result<()> {
    let fonts = EMBEDDED.iter().map(|b| Cow::Borrowed(*b)).collect();
    cx.text_system().add_fonts(fonts)?;
    let installed = cx.text_system().all_font_names();
    let _ = UI_FAMILY.set(choose_ui(&installed));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_fonts_are_truetype() {
        for bytes in EMBEDDED {
            assert_eq!(&bytes[..4], &[0x00, 0x01, 0x00, 0x00]);
        }
    }

    #[test]
    fn prefers_installed_sf_pro() {
        assert_eq!(choose_ui(&["Cantarell".into()]), UI_FALLBACK);
        assert_eq!(choose_ui(&["SF Pro".into(), "Cantarell".into()]), "SF Pro");
    }

    #[test]
    fn files_declare_the_expected_families() {
        let family = |bytes: &[u8]| {
            let face = ttf_parser::Face::parse(bytes, 0).unwrap();
            face.names()
                .into_iter()
                .find(|n| n.name_id == ttf_parser::name_id::FAMILY && n.is_unicode())
                .and_then(|n| n.to_string())
                .unwrap()
        };
        let families: Vec<String> = EMBEDDED.iter().map(|b| family(b)).collect();
        for expected in [DUO, MONO, QUATTRO, UI_FALLBACK, SERIF] {
            assert!(families.iter().any(|f| f == expected), "missing {expected}");
        }
    }
}
