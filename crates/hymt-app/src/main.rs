//! HyMT — an offline translator for Windows 11.
//!
//! The UI is GPUI Kit (`gpui-kit` = GPUI + gpui-base + gpui-component); the model runs in a
//! `llama-server` child process and is reached over loopback HTTP. Nothing leaves the machine.

mod root;
mod smooth_scroll;
mod ui_common;
mod ui_models;
mod ui_settings;
mod ui_translate;
mod widgets;

use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{
    px, size, App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowOptions,
};
use std::io::Write;

/// Panics inside the GPUI message loop never reach the console in a packaged or
/// background run: gpui turns them into a bare `abort()` in debug builds. Record every
/// panic, with a backtrace, in the app log so a crash can always be diagnosed.
fn install_panic_logger() {
    let log_path = hymt_core::paths::AppPaths::detect().app_log();
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(file, "[panic] {info}");
            let _ = writeln!(file, "{}", std::backtrace::Backtrace::force_capture());
        }
        default_hook(info);
    }));
}

fn main() {
    install_panic_logger();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);

            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1180.0), px(780.0)), cx);
            let opened = gpui_kit::open_window(
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
                cx,
                |window, cx| {
                    let root = cx.new(|cx| root::Root::new(window.window_handle(), window, cx));
                    // Start typing immediately.
                    let source = root.read(cx).source.clone();
                    source.update(cx, |state, cx| state.focus(window, cx));
                    root
                },
            );

            match opened {
                Ok(_) => cx.activate(true),
                Err(err) => eprintln!("창을 열 수 없습니다: {err}"),
            }
        });
}
