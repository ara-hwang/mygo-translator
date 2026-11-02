//! HyMT — an offline translator for Windows 11.
//!
//! The UI is GPUI Kit (`gpui-kit` = GPUI + gpui-base + gpui-component); the model runs in a
//! `llama-server` child process and is reached over loopback HTTP. Nothing leaves the machine.

// Release builds can run at Windows sign-in, so they must not open a console window. Debug
// builds keep the console for `cargo run` diagnostics.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod root;
mod smooth_scroll;
mod tray;
mod ui_common;
mod ui_models;
mod ui_settings;
mod ui_translate;
mod widgets;
mod window_ops;

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

/// Records a startup failure in the app log. Release builds are GUI-subsystem binaries, so
/// stderr is not visible when one is launched from the Run key.
fn log_startup_error(message: &str) {
    let paths = hymt_core::paths::AppPaths::detect();
    if paths.ensure().is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.app_log())
    {
        let _ = writeln!(file, "[startup] {message}");
    }
}

fn main() {
    install_panic_logger();

    // A launch from the Run key passes `--autostart`; hide the window then when the tray
    // option is enabled, so sign-in starts quietly in the notification area.
    let paths = hymt_core::paths::AppPaths::detect();
    let (settings, _) = hymt_core::settings::Settings::load_or_default(&paths.settings_file());
    let autostart_launch = std::env::args_os()
        .skip(1)
        .any(|arg| arg == std::ffi::OsStr::new("--autostart"));
    let start_hidden = autostart_launch && settings.desktop.keep_in_tray;

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx: &mut App| {
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
                    focus: !start_hidden,
                    titlebar: Some(TitlebarOptions {
                        title: Some("HyMT — 오프라인 번역".into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    let root = cx.new(|cx| {
                        root::Root::new(window.window_handle(), window, start_hidden, cx)
                    });
                    // Start typing immediately.
                    let source = root.read(cx).source.clone();
                    source.update(cx, |state, cx| state.focus(window, cx));
                    root
                },
            );

            match opened {
                Ok(_) => {
                    if !start_hidden {
                        cx.activate(true);
                    }
                }
                Err(err) => {
                    let message = format!("창을 열 수 없습니다: {err}");
                    eprintln!("{message}");
                    log_startup_error(&message);
                }
            }
        });
}
