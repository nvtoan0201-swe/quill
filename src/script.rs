//! Scripted input for end-to-end checks without a desktop automation tool.
//! QUILL_SCRIPT="type:Hello;;key:ctrl-d;;click:300,200;;drag:300,200,500,260;;wait:500"
//! Mouse coordinates are logical window pixels. Events go through GPUI's normal
//! dispatch (hit-testing included), exactly like platform input.
//! `shot:/path.png` asks the harness to capture: it prints a marker and waits.

use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PlatformInput, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase,
    Window, point, px,
};

fn send(window: &mut Window, cx: &mut App, e: PlatformInput) {
    let _ = window.dispatch_event(e, cx);
}

fn pt(arg: &str) -> Vec<Pixels> {
    arg.split(',').map(|v| px(v.trim().parse::<f32>().unwrap_or(0.))).collect()
}

fn moved(window: &mut Window, cx: &mut App, p: Point<Pixels>, held: Option<MouseButton>) {
    let e = MouseMoveEvent { position: p, pressed_button: held, modifiers: Modifiers::default() };
    send(window, cx, PlatformInput::MouseMove(e));
}

fn press(window: &mut Window, cx: &mut App, p: Point<Pixels>, button: MouseButton, count: usize) {
    let m = Modifiers::default();
    let down = MouseDownEvent { button, position: p, modifiers: m, click_count: count, first_mouse: false };
    send(window, cx, PlatformInput::MouseDown(down));
}

fn release(window: &mut Window, cx: &mut App, p: Point<Pixels>, button: MouseButton, count: usize) {
    let up = MouseUpEvent { button, position: p, modifiers: Modifiers::default(), click_count: count };
    send(window, cx, PlatformInput::MouseUp(up));
}

pub fn run(window: AnyWindowHandle, script: String, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(600)).await;
        for step in script.split(";;") {
            let (cmd, arg) = step.split_once(':').unwrap_or((step, ""));
            let arg = arg.to_string();
            if cmd == "wait" {
                let ms = arg.parse().unwrap_or(300);
                cx.background_executor().timer(Duration::from_millis(ms)).await;
                continue;
            }
            let _ = window.update(cx, |_, window, cx| match cmd {
                "type" => {
                    for c in arg.chars() {
                        let key = match c {
                            ' ' => "space".to_string(),
                            '\n' => "enter".to_string(),
                            c => c.to_lowercase().to_string(),
                        };
                        let ks = Keystroke { modifiers: Modifiers::default(), key, key_char: Some(c.to_string()) };
                        window.dispatch_keystroke(ks, cx);
                    }
                }
                "key" => {
                    if let Ok(ks) = Keystroke::parse(&arg) {
                        window.dispatch_keystroke(ks, cx);
                    }
                }
                "move" => {
                    let v = pt(&arg);
                    moved(window, cx, point(v[0], v[1]), None);
                }
                "click" | "dclick" | "tclick" | "rclick" => {
                    let v = pt(&arg);
                    let p = point(v[0], v[1]);
                    let button = if cmd == "rclick" { MouseButton::Right } else { MouseButton::Left };
                    let n = match cmd { "dclick" => 2, "tclick" => 3, _ => 1 };
                    moved(window, cx, p, None);
                    for count in 1..=n {
                        press(window, cx, p, button, count);
                        release(window, cx, p, button, count);
                    }
                }
                "drag" => {
                    let v = pt(&arg);
                    let (a, b) = (point(v[0], v[1]), point(v[2], v[3]));
                    moved(window, cx, a, None);
                    press(window, cx, a, MouseButton::Left, 1);
                    for i in 1..=10 {
                        let t = i as f32 / 10.;
                        moved(window, cx, point(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t), Some(MouseButton::Left));
                    }
                    release(window, cx, b, MouseButton::Left, 1);
                }
                "scroll" => {
                    let v = pt(&arg);
                    let e = ScrollWheelEvent {
                        position: point(v[0], v[1]),
                        delta: ScrollDelta::Pixels(point(px(0.), v[2])),
                        modifiers: Modifiers::default(),
                        touch_phase: TouchPhase::Moved,
                    };
                    send(window, cx, PlatformInput::ScrollWheel(e));
                }
                "shot" => {
                    // The harness watches stdout for this marker and captures the window.
                    println!("QUILL_SHOT {arg}");
                }
                "action" => match cx.build_action(&arg, None) {
                    Ok(a) => window.dispatch_action(a, cx),
                    Err(e) => eprintln!("QUILL_SCRIPT: {e}"),
                },
                _ => eprintln!("QUILL_SCRIPT: unknown command {cmd}"),
            });
            cx.background_executor().timer(Duration::from_millis(120)).await;
        }
        println!("QUILL_DONE");
    })
    .detach();
}
