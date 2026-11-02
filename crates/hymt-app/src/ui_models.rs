//! Model management: one-click GGUF downloads, runtime discovery and engine logs.

use std::fs;
use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, Context, Div, FontWeight, SharedString};

use hymt_core::models::{self, ModelOption};

use crate::root::Root;
use crate::widgets;

const LLAMA_RELEASES: &str = "https://github.com/ggml-org/llama.cpp/releases";

impl Root {
    pub fn render_models(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let catalog: Vec<Div> = models::CATALOG
            .iter()
            .map(|option| self.catalog_row(option, cx))
            .collect();

        let local: Vec<PathBuf> = self.models.clone();
        let mut local_rows = Vec::new();
        for path in local {
            local_rows.push(self.local_model_row(path, cx));
        }

        let log_lines: Vec<String> = self.engine_log.iter().cloned().collect();

        v_flex()
            .flex_1()
            .min_h_0()
            .id("models-scroll")
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(widgets::section_label("모델 다운로드", cx))
            .child(widgets::muted(
                "Tencent 공식 GGUF입니다. 한 번만 내려받으면 이후에는 완전히 오프라인으로 동작합니다.",
                cx,
            ))
            .child(
                widgets::card(cx).child(widgets::panel_header("Hy-MT2 모델", None, cx)).children(catalog),
            )
            .child(widgets::section_label("엔진 런타임", cx))
            .child(self.runtime_card(cx))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(widgets::section_label(
                        format!("설치된 모델 ({})", self.models.len()),
                        cx,
                    ))
                    .child(
                        Button::new("rescan-models")
                            .secondary()
                            .label("새로고침")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.rescan_models(window, cx);
                            })),
                    ),
            )
            .child(
                widgets::card(cx)
                    .when(local_rows.is_empty(), |card| {
                        card.child(
                            div()
                                .p_3()
                                .child(widgets::muted("아직 설치된 모델이 없습니다.", cx)),
                        )
                    })
                    .children(local_rows),
            )
            .child(widgets::section_label("서버 로그", cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h(px(160.0))
                    .id("engine-log")
                    .overflow_y_scroll()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .children(log_lines.into_iter().map(|line| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
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
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    Progress::new("catalog-download")
                        .value(fraction * 100.0)
                        .w(px(110.0)),
                )
                .child(widgets::muted(label, cx))
                .child(
                    Button::new("cancel-download")
                        .text()
                        .compact()
                        .label("취소")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel_download(window, cx);
                        })),
                )
                .into_any_element()
        } else if selected {
            Tag::success().child("사용 중").into_any_element()
        } else if installed {
            // ElementIds must be unique across the whole frame: the catalog renders one
            // row per model, so the button id has to carry the model's id.
            Button::new(SharedString::from(format!("use-model-{}", option.id)))
                .secondary()
                .label("사용")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_model(dest.clone(), window, cx);
                }))
                .into_any_element()
        } else {
            Button::new(SharedString::from(format!("download-model-{}", option.id)))
                .primary()
                .label("내려받기")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_download(option, window, cx);
                }))
                .into_any_element()
        };

        v_flex()
            .gap_2()
            .px_3()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(option.file_name()),
                                    )
                                    .when(option.recommended, |row| {
                                        row.child(Tag::primary().child("권장"))
                                    }),
                            )
                            .child(widgets::muted(
                                format!("{} · {}", option.family, option.size_label()),
                                cx,
                            ))
                            .child(widgets::muted(option.note, cx)),
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
            Tag::success().child("사용 중").into_any_element()
        } else {
            // One row per installed model: the id has to stay unique across the frame.
            Button::new(SharedString::from(format!("use-local-model-{}", name)))
                .secondary()
                .label("사용")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_model(path.clone(), window, cx);
                }))
                .into_any_element()
        };

        h_flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().child(name))
                    .child(widgets::muted(size, cx)),
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

        let actions = h_flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("open-runtime")
                    .secondary()
                    .label("runtime 폴더")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let dir = this.runtime_dir();
                        let _ = fs::create_dir_all(&dir);
                        this.open_folder(&dir);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("open-releases")
                    .secondary()
                    .label("llama.cpp 릴리스")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_url(LLAMA_RELEASES);
                        cx.notify();
                    })),
            );

        widgets::card(cx)
            .child(widgets::panel_header("엔진 런타임", None, cx))
            .child(
                v_flex()
                    .gap_2()
                    .p_3()
                    .child(widgets::setting_row("llama-server", widgets::muted(server, cx)))
                    .child(widgets::setting_row(
                        "런타임 폴더",
                        widgets::muted(runtime_dir, cx),
                    ))
                    .child(
                        v_flex()
                            .gap_1()
                            .mt_2()
                            .child(widgets::muted(
                                "1. llama.cpp 릴리스에서 Windows용 빌드(zip)를 내려받습니다.",
                                cx,
                            ))
                            .child(widgets::muted(
                                "2. 압축을 풀고 llama-server.exe와 함께 들어 있는 DLL들을 런타임 폴더에 넣습니다.",
                                cx,
                            ))
                            .child(widgets::muted(
                                "3. CPU만 쓰려면 기본 빌드를, NVIDIA GPU를 쓰려면 CUDA 빌드를 선택합니다.",
                                cx,
                            ))
                            .child(widgets::muted(
                                "4. 모델 탭에서 내려받은 뒤 엔진 시작을 누르면 준비가 끝납니다.",
                                cx,
                            )),
                    )
                    .child(div().mt_2().child(actions)),
            )
    }
}
