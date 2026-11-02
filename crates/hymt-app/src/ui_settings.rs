//! Settings: sampling, prompt construction, glossary and engine launch options.

use gpui::{div, prelude::*, px, rgb, Context, Div, MouseButton, MouseUpEvent, Window};
use hymt_core::{glossary, prompt::PromptLanguage, settings::Settings};

use crate::{
    root::{Root, PROMPT_LANGUAGES, STYLE_PRESETS},
    theme,
    ui_common::{panel_header, stepper},
    widgets,
};

impl Root {
    pub fn render_settings(&mut self, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let mut style_chips = Vec::new();
        for (label, value) in STYLE_PRESETS.iter().copied() {
            style_chips.push(self.style_option(label, value, cx));
        }

        let mut prompt_chips = Vec::new();
        for (label, language) in PROMPT_LANGUAGES.iter().copied() {
            prompt_chips.push(self.prompt_option(label, language, cx));
        }

        let (pairs, warnings) = glossary::parse_report(&self.settings.prompt.glossary);
        let glossary_status = if pairs.is_empty() {
            "아직 용어가 없습니다.".to_string()
        } else {
            format!("용어 {}개", pairs.len())
        };
        let mut warning_lines = Vec::new();
        for warning in warnings {
            warning_lines.push(widgets::muted(warning));
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .id("settings-scroll")
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(widgets::section_label("생성 파라미터"))
            .child(
                widgets::card()
                    .child(panel_header(
                        "Hy-MT2 권장값에서 시작합니다",
                        Some(
                            widgets::secondary("기본값").on_mouse_up(
                                MouseButton::Left,
                                cx.listener(Self::on_reset_settings),
                            ),
                        ),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_3()
                            .child(widgets::setting_row(
                                "temperature",
                                stepper(
                                    format!("{:.2}", self.settings.generation.temperature),
                                    cx.listener(Self::on_dec_temperature),
                                    cx.listener(Self::on_inc_temperature),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "top_p",
                                stepper(
                                    format!("{:.2}", self.settings.generation.top_p),
                                    cx.listener(Self::on_dec_top_p),
                                    cx.listener(Self::on_inc_top_p),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "top_k",
                                stepper(
                                    format!("{}", self.settings.generation.top_k),
                                    cx.listener(Self::on_dec_top_k),
                                    cx.listener(Self::on_inc_top_k),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "repetition_penalty",
                                stepper(
                                    format!("{:.2}", self.settings.generation.repeat_penalty),
                                    cx.listener(Self::on_dec_repeat_penalty),
                                    cx.listener(Self::on_inc_repeat_penalty),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "max_tokens",
                                stepper(
                                    format!("{}", self.settings.generation.max_tokens),
                                    cx.listener(Self::on_dec_max_tokens),
                                    cx.listener(Self::on_inc_max_tokens),
                                ),
                            )),
                    ),
            )
            .child(widgets::section_label("프롬프트"))
            .child(
                widgets::card()
                    .child(panel_header("번역 지시문", None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p_3()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(widgets::muted("지시문 언어"))
                                    .child(div().flex().flex_row().gap_2().children(prompt_chips)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(widgets::muted(
                                        "스타일 (용어집보다 우선순위가 낮습니다)",
                                    ))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap_2()
                                            .flex_wrap()
                                            .children(style_chips),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(self.toggle_chip(
                                        "구분자 보존",
                                        self.settings.prompt.preserve_delimiters,
                                        cx,
                                        Self::on_toggle_delimiters,
                                    ))
                                    .child(self.toggle_chip(
                                        "용어집 적용",
                                        self.settings.prompt.use_glossary,
                                        cx,
                                        Self::on_toggle_glossary_switch,
                                    )),
                            ),
                    ),
            )
            .child(widgets::section_label("용어집"))
            .child(
                widgets::card()
                    .child(panel_header(format!("용어집 · {glossary_status}"), None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .child(widgets::muted(
                                "한 줄에 `원문 => 번역` 하나씩 입력합니다. 탭 구분도 됩니다.",
                            ))
                            .child(
                                div()
                                    .h(px(170.0))
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(theme::BORDER))
                                    .bg(rgb(theme::PANEL_ALT))
                                    .child(self.glossary_editor.clone()),
                            )
                            .children(warning_lines),
                    ),
            )
            .child(widgets::section_label("엔진"))
            .child(
                widgets::card()
                    .child(panel_header("llama.cpp 실행 옵션", None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p_3()
                            .child(widgets::setting_row(
                                "모델",
                                widgets::muted(
                                    self.current_model_name()
                                        .unwrap_or_else(|| "선택 없음".to_string()),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "context",
                                stepper(
                                    format!("{}", self.settings.server.ctx_size),
                                    cx.listener(Self::on_dec_ctx),
                                    cx.listener(Self::on_inc_ctx),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "threads",
                                stepper(
                                    format!("{}", self.settings.server.threads),
                                    cx.listener(Self::on_dec_threads),
                                    cx.listener(Self::on_inc_threads),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "GPU layers (-1 = 자동)",
                                stepper(
                                    format!("{}", self.settings.server.gpu_layers),
                                    cx.listener(Self::on_dec_gpu_layers),
                                    cx.listener(Self::on_inc_gpu_layers),
                                ),
                            ))
                            .child(widgets::setting_row(
                                "port",
                                widgets::muted(format!("{}", self.settings.server.port)),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .mt_2()
                                    .child(widgets::secondary("설정 저장").on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(Self::on_save_settings),
                                    ))
                                    .child(widgets::primary("저장 후 엔진 재시작").on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(Self::on_restart_engine),
                                    )),
                            ),
                    ),
            )
            .child(widgets::section_label("다운로드"))
            .child(
                widgets::card()
                    .child(panel_header("모델 다운로드 서버", None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_3()
                            .child(widgets::muted(
                                "네트워크 환경에 따라 Hugging Face 또는 미러를 선택하세요.",
                            ))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(self.base_url_option(
                                        "Hugging Face",
                                        "https://huggingface.co",
                                        cx,
                                    ))
                                    .child(self.base_url_option(
                                        "hf-mirror.com",
                                        "https://hf-mirror.com",
                                        cx,
                                    )),
                            )
                            .child(widgets::muted(self.settings.download_base.clone())),
                    ),
            )
    }

    fn toggle_chip(
        &self,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        handler: fn(&mut Self, &MouseUpEvent, &mut Window, &mut Context<Self>),
    ) -> Div {
        widgets::chip(
            if active {
                format!("{label} · 켜짐")
            } else {
                label.to_string()
            },
            active,
        )
        .on_mouse_up(MouseButton::Left, cx.listener(handler))
    }

    fn style_option(
        &self,
        label: &'static str,
        value: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let active = self.settings.prompt.style == value;
        widgets::chip(label, active).on_mouse_up(
            MouseButton::Left,
            cx.listener(move |root, _: &MouseUpEvent, _window, cx| {
                root.settings.prompt.style = value.to_string();
                root.auto_save(cx);
            }),
        )
    }

    fn prompt_option(
        &self,
        label: &'static str,
        language: PromptLanguage,
        cx: &mut Context<Self>,
    ) -> Div {
        let active = self.settings.prompt.language == language;
        widgets::chip(label, active).on_mouse_up(
            MouseButton::Left,
            cx.listener(move |root, _: &MouseUpEvent, _window, cx| {
                root.settings.prompt.language = language;
                root.auto_save(cx);
            }),
        )
    }

    fn base_url_option(
        &self,
        label: &'static str,
        url: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let active = self.settings.download_base == url;
        widgets::chip(label, active).on_mouse_up(
            MouseButton::Left,
            cx.listener(move |root, _: &MouseUpEvent, _window, cx| {
                root.settings.download_base = url.to_string();
                root.auto_save(cx);
            }),
        )
    }

    fn auto_save(&mut self, cx: &mut Context<Self>) {
        self.settings = self.settings.clone().sanitized();
        let _ = self.save_settings();
        cx.notify();
    }

    fn on_dec_temperature(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.temperature -= 0.05;
        self.auto_save(cx);
    }

    fn on_inc_temperature(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.temperature += 0.05;
        self.auto_save(cx);
    }

    fn on_dec_top_p(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.top_p -= 0.05;
        self.auto_save(cx);
    }

    fn on_inc_top_p(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.top_p += 0.05;
        self.auto_save(cx);
    }

    fn on_dec_top_k(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.top_k = (self.settings.generation.top_k - 1).max(-1);
        self.auto_save(cx);
    }

    fn on_inc_top_k(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.top_k = (self.settings.generation.top_k + 1).min(200);
        self.auto_save(cx);
    }

    fn on_dec_repeat_penalty(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.repeat_penalty -= 0.01;
        self.auto_save(cx);
    }

    fn on_inc_repeat_penalty(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.repeat_penalty += 0.01;
        self.auto_save(cx);
    }

    fn on_dec_max_tokens(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.max_tokens =
            self.settings.generation.max_tokens.saturating_sub(128);
        self.auto_save(cx);
    }

    fn on_inc_max_tokens(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.generation.max_tokens =
            self.settings.generation.max_tokens.saturating_add(128);
        self.auto_save(cx);
    }

    fn on_dec_ctx(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.server.ctx_size = self.settings.server.ctx_size.saturating_sub(1024);
        self.auto_save(cx);
    }

    fn on_inc_ctx(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.server.ctx_size = self.settings.server.ctx_size.saturating_add(1024);
        self.auto_save(cx);
    }

    fn on_dec_threads(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.server.threads = self.settings.server.threads.saturating_sub(1);
        self.auto_save(cx);
    }

    fn on_inc_threads(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.server.threads = self.settings.server.threads.saturating_add(1);
        self.auto_save(cx);
    }

    fn on_dec_gpu_layers(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.server.gpu_layers = if self.settings.server.gpu_layers > 0 {
            self.settings.server.gpu_layers - 1
        } else {
            -1
        };
        self.auto_save(cx);
    }

    fn on_inc_gpu_layers(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.server.gpu_layers = if self.settings.server.gpu_layers < 0 {
            0
        } else {
            self.settings.server.gpu_layers + 1
        };
        self.auto_save(cx);
    }

    fn on_toggle_delimiters(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.prompt.preserve_delimiters = !self.settings.prompt.preserve_delimiters;
        self.auto_save(cx);
    }

    fn on_toggle_glossary_switch(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.prompt.use_glossary = !self.settings.prompt.use_glossary;
        self.auto_save(cx);
    }

    fn on_reset_settings(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let model = self.settings.server.model_path.clone();
        let base = self.settings.download_base.clone();
        self.settings = Settings::default();
        self.settings.server.model_path = model;
        self.settings.download_base = base;
        self.glossary_editor
            .update(cx, |editor, cx| editor.clear(cx));
        self.settings.prompt.glossary.clear();
        let _ = self.save_settings();
        self.notice = Some(crate::root::Notice::info("설정을 기본값으로 되돌렸습니다."));
        cx.notify();
    }

    fn on_save_settings(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.save_settings_with_notice(cx);
    }

    fn on_restart_engine(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.settings.prompt.glossary = self.glossary_editor.read(cx).content().to_string();
        self.settings = self.settings.clone().sanitized();
        let _ = self.save_settings();
        self.restart_engine(cx);
    }
}
