//! Model management: one-click GGUF downloads, runtime discovery and engine logs.

use std::fs;
use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, relative, rgb, Context, Div, IntoElement, MouseButton, MouseUpEvent,
    Window,
};
use hymt_core::models::{self, ModelOption};

use crate::{root::Root, theme, ui_common::panel_header, widgets};

const LLAMA_RELEASES: &str = "https://github.com/ggml-org/llama.cpp/releases";

impl Root {
    pub fn render_models(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut catalog = Vec::new();
        for option in models::CATALOG {
            catalog.push(self.catalog_row(option, cx));
        }

        let local: Vec<PathBuf> = self.models.clone();
        let mut local_rows = Vec::new();
        for path in local {
            local_rows.push(self.local_model_row(path, cx));
        }

        let log_lines: Vec<String> = self.engine_log.iter().cloned().collect();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .id("models-scroll")
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(widgets::section_label("모델 다운로드"))
            .child(widgets::muted(
                "Tencent 공식 GGUF입니다. 한 번만 내려받으면 이후에는 완전히 오프라인으로 동작합니다.",
            ))
            .child(
                widgets::card()
                    .child(panel_header("Hy-MT2 모델", None))
                    .children(catalog),
            )
            .child(widgets::section_label("엔진 런타임"))
            .child(self.runtime_card(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(widgets::section_label(format!(
                        "설치된 모델 ({})",
                        self.models.len()
                    )))
                    .child(
                        widgets::secondary("새로고침")
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_rescan_models)),
                    ),
            )
            .child(
                widgets::card()
                    .when(local_rows.is_empty(), |card| {
                        card.child(
                            div()
                                .p_3()
                                .child(widgets::muted("아직 설치된 모델이 없습니다.")),
                        )
                    })
                    .children(local_rows),
            )
            .child(widgets::section_label("서버 로그"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h(px(160.0))
                    .id("engine-log")
                    .overflow_y_scroll()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(theme::BORDER))
                    .bg(rgb(0x0b0e13))
                    .children(log_lines.into_iter().map(|line| {
                        div()
                            .text_xs()
                            .text_color(rgb(theme::MUTED))
                            .child(line)
                    })),
            )
    }

    fn catalog_row(&self, option: &'static ModelOption, cx: &mut Context<Self>) -> Div {
        let dest = self.paths.models_dir().join(option.file_name());
        let installed = dest.is_file();
        let selected = self
            .settings
            .server
            .model_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy() == option.file_name())
            .unwrap_or(false);
        let downloading =
            self.download.as_ref().map(|download| download.option.id) == Some(option.id);

        let control = if downloading {
            let fraction = self
                .download_progress()
                .map(|progress| progress.fraction())
                .unwrap_or(0.0);
            let mut label = self
                .download_progress()
                .map(|progress| progress.label())
                .unwrap_or_else(|| "내려받는 중…".to_string());
            if let Some(rate) = self.download_rate() {
                label.push_str(&format!(" · {}/s", models::human_size(rate as u64)));
            }
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(110.0))
                        .h(px(5.0))
                        .rounded_full()
                        .bg(rgb(theme::PANEL_ALT))
                        .child(
                            div()
                                .h(px(5.0))
                                .rounded_full()
                                .bg(rgb(theme::ACCENT))
                                .w(relative(fraction)),
                        ),
                )
                .child(widgets::muted(label))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme::ERROR))
                        .cursor_pointer()
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::on_cancel_download))
                        .child("취소"),
                )
                .into_any_element()
        } else if selected {
            widgets::chip("사용 중", true).into_any_element()
        } else if installed {
            let path = dest.clone();
            widgets::secondary("사용")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |root, _: &MouseUpEvent, _window, cx| {
                        root.select_model(path.clone(), cx);
                    }),
                )
                .into_any_element()
        } else {
            widgets::primary("내려받기")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |root, _: &MouseUpEvent, _window, cx| {
                        root.start_download(option, cx);
                    }),
                )
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(theme::TEXT))
                                            .child(option.file_name()),
                                    )
                                    .when(option.recommended, |row| {
                                        row.child(widgets::chip("권장", true))
                                    }),
                            )
                            .child(widgets::muted(format!(
                                "{} · {}",
                                option.family,
                                option.size_label()
                            )))
                            .child(widgets::muted(option.note)),
                    )
                    .child(control),
            )
    }

    fn local_model_row(&self, path: PathBuf, cx: &mut Context<Self>) -> Div {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let size = fs::metadata(&path)
            .map(|metadata| models::human_size(metadata.len()))
            .unwrap_or_else(|_| "크기 확인 불가".to_string());
        let selected = self
            .settings
            .server
            .model_path
            .as_ref()
            .is_some_and(|current| current == &path);

        let control = if selected {
            widgets::chip("사용 중", true).into_any_element()
        } else {
            let target = path.clone();
            widgets::secondary("사용")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |root, _: &MouseUpEvent, _window, cx| {
                        root.select_model(target.clone(), cx);
                    }),
                )
                .into_any_element()
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_sm().text_color(rgb(theme::TEXT)).child(name))
                    .child(widgets::muted(size)),
            )
            .child(control)
    }

    fn runtime_card(&self, cx: &mut Context<Self>) -> Div {
        let server = self
            .server_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "찾을 수 없음".to_string());
        let runtime_dir = self.runtime_dir().display().to_string();

        let actions = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                widgets::secondary("runtime 폴더")
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_open_runtime_folder)),
            )
            .child(
                widgets::secondary("llama.cpp 릴리스")
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_open_releases)),
            );

        widgets::card()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .child(widgets::setting_row(
                        "llama-server",
                        widgets::muted(server),
                    ))
                    .child(widgets::setting_row("런타임 폴더", widgets::muted(runtime_dir)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .mt_2()
                            .child(widgets::muted(
                                "1. llama.cpp 릴리스에서 Windows용 빌드(zip)를 내려받습니다.",
                            ))
                            .child(widgets::muted(
                                "2. 압축을 풀고 llama-server.exe와 함께 들어 있는 DLL들을 런타임 폴더에 넣습니다.",
                            ))
                            .child(widgets::muted(
                                "3. CPU만 쓰려면 기본 빌드를, NVIDIA GPU를 쓰려면 CUDA 빌드를 선택합니다.",
                            ))
                            .child(widgets::muted(
                                "4. 모델 탭에서 내려받은 뒤 엔진 시작을 누르면 준비가 끝납니다.",
                            )),
                    )
                    .child(div().mt_2().child(actions)),
            )
    }

    fn on_rescan_models(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.rescan_models(cx);
    }

    fn on_cancel_download(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.cancel_download(cx);
    }

    fn on_open_runtime_folder(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let dir = self.runtime_dir();
        let _ = fs::create_dir_all(&dir);
        self.open_folder(&dir);
        cx.notify();
    }

    fn on_open_releases(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.open_url(LLAMA_RELEASES);
        cx.notify();
    }
}
