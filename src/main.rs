mod actions;
mod assets;
mod authors;
mod buffer;
mod diff;
mod editor;
mod export;
mod focus;
mod fonts;
mod format;
mod help;
mod ipc;
mod library;
mod markdown;
mod menus;
mod preview;
mod publish;
mod script;
mod settings;
mod spell;
mod stats;
mod studio;
mod style_check;
mod syntax;
mod table;
mod text_field;
mod theme;
mod urlcmd;
mod workspace;

use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowDecorations,
    WindowOptions, point, px, size,
};

use crate::settings::Settings;
use crate::workspace::Workspace;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().is_some_and(|a| a == "--analyze") {
        std::process::exit(studio::cli::analyze(&raw[1..]));
    }
    let args = ipc::normalize(raw);
    // Hand over to a running instance when there is one.
    if ipc::forward(&args) {
        return;
    }
    let inbox = ipc::listen();
    Application::new().with_assets(assets::Assets).run(move |cx: &mut App| {
        fonts::register(cx).expect("embedded fonts should load");
        cx.set_global(theme::Theme::light());
        actions::bind(cx);
        menus::build_shortcuts(cx);

        let s = Settings::load();
        let bounds = Bounds::centered(None, size(px(s.window_width), px(s.window_height)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Quill".into()),
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(20.), px(20.))),
                    }),
                    window_decorations: Some(WindowDecorations::Client),
                    // Lets the rounded corners and the shadow show the desktop behind.
                    window_background: WindowBackgroundAppearance::Transparent,
                    window_min_size: Some(size(px(480.), px(320.))),
                    app_id: Some("quill".into()),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Workspace::new(args, inbox, window, cx)),
            )
            .expect("failed to open window");
        window
            .update(cx, |_, window, cx| {
                window.on_window_should_close(cx, |window, cx| {
                    window.dispatch_action(Box::new(actions::Quit), cx);
                    false
                });
                cx.activate(true);
            })
            .ok();
        if let Ok(script) = std::env::var("QUILL_SCRIPT") {
            script::run(window.into(), script, cx);
        }
    });
}
