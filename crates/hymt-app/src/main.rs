//! HyMT — an offline translator for Windows 11.
//!
//! The UI is GPUI; the model runs in a `llama-server` child process and is reached over
//! loopback HTTP. Nothing leaves the machine.

mod root;
mod text_area;
mod theme;
mod ui_common;
mod ui_models;
mod ui_picker;
mod ui_settings;
mod ui_translate;
mod widgets;
mod wrap;

use gpui::{px, size, App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions};

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(960.0), px(640.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("HyMT — 오프라인 번역".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                ..Default::default()
            },
            |window, cx| {
                let root = cx.new(root::Root::new);
                // Start typing immediately.
                let source = root.read(cx).source.clone();
                source.update(cx, |editor, cx| editor.focus(window, cx));
                root
            },
        );

        match opened {
            Ok(_) => cx.activate(true),
            Err(err) => eprintln!("창을 열 수 없습니다: {err}"),
        }
    });
}
