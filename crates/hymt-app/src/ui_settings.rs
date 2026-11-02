//! Settings: sampling, prompt construction, glossary and engine launch options.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex, Selectable};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, relative, AnyElement, Context, Div, SharedString, Window};

use hymt_core::{glossary, settings::Settings};

use crate::root::{Root, PROMPT_LANGUAGES, STYLE_PRESETS};
use crate::widgets;

impl Root {
    pub fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let prompt_radios: Vec<Radio> = PROMPT_LANGUAGES
            .iter()
            .enumerate()
            .map(|(index, (label, _))| {
                Radio::new(SharedString::from(format!(
                    "prompt-language-{index}"
                )))
                .label(*label)
                .checked(self.settings.prompt.language == PROMPT_LANGUAGES[index].1)
            })
            .collect();

        let style_buttons: Vec<AnyElement> = STYLE_PRESETS
            .iter()
            .map(|(label, value)| {
                Button::new(SharedString::from(format!("style-{label}")))
                    .text()
                    .label(*label)
                    .selected(self.settings.prompt.style == *value)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.prompt.style = value.to_string();
                        this.auto_save(cx);
                    }))
                    .into_any_element()
            })
            .collect();

        let (pairs, warnings) = glossary::parse_report(&self.settings.prompt.glossary);
        let glossary_status = if pairs.is_empty() {
            "아직 용어가 없습니다.".to_string()
        } else {
            format!("용어 {}개", pairs.len())
        };
        let warning_lines: Vec<AnyElement> = warnings
            .into_iter()
            .map(|warning| widgets::muted(warning, cx).into_any_element())
            .collect();

        let selected_prompt_language = PROMPT_LANGUAGES
            .iter()
            .position(|(_, language)| *language == self.settings.prompt.language)
            .unwrap_or(0);

        v_flex()
            .flex_1()
            .min_h_0()
            .id("settings-scroll")
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .child(widgets::section_label("생성 파라미터", cx))
            .child(
                widgets::card(cx)
                    .child(widgets::panel_header(
                        "Hy-MT2 권장값에서 시작합니다",
                        Some(
                            Button::new("reset-settings")
                                .secondary()
                                .label("기본값")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.on_reset_settings(window, cx);
                                }))
                                .into_any_element(),
                        ),
                        cx,
                    ))
                    .child(
                        v_flex()
                            .gap_1()
                            .p_3()
                            .child(widgets::setting_row(
                                "temperature",
                                stepper("temperature", format!("{:.2}", self.settings.generation.temperature), cx),
                            ))
                            .child(widgets::setting_row(
                                "top_p",
                                stepper("top-p", format!("{:.2}", self.settings.generation.top_p), cx),
                            ))
                            .child(widgets::setting_row(
                                "top_k",
                                stepper("top-k", format!("{}", self.settings.generation.top_k), cx),
                            ))
                            .child(widgets::setting_row(
                                "repetition_penalty",
                                stepper(
                                    "repeat-penalty",
                                    format!("{:.2}", self.settings.generation.repeat_penalty),
                                    cx,
                                ),
                            ))
                            .child(widgets::setting_row(
                                "max_tokens",
                                stepper("max-tokens", format!("{}", self.settings.generation.max_tokens), cx),
                            )),
                    ),
            )
            .child(widgets::section_label("프롬프트", cx))
            .child(
                widgets::card(cx)
                    .child(widgets::panel_header("번역 지시문", None, cx))
                    .child(
                        v_flex()
                            .gap_3()
                            .p_3()
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(widgets::muted("지시문 언어", cx))
                                    .child(
                                        RadioGroup::new("prompt-language")
                                            .children(prompt_radios)
                                            .selected_index(Some(selected_prompt_language))
                                            .on_change(cx.listener(|this, index: &usize, _, cx| {
                                                if let Some((_, language)) = PROMPT_LANGUAGES.get(*index) {
                                                    this.settings.prompt.language = *language;
                                                    this.auto_save(cx);
                                                }
                                            })),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(widgets::muted(
                                        "스타일 (용어집보다 우선순위가 낮습니다)",
                                        cx,
                                    ))
                                    .child(
                                        h_flex().gap_2().flex_wrap().children(style_buttons),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_6()
                                    .child(
                                        Switch::new("preserve-delimiters")
                                            .label("구분자 보존")
                                            .checked(self.settings.prompt.preserve_delimiters)
                                            .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                                this.settings.prompt.preserve_delimiters = *checked;
                                                this.auto_save(cx);
                                            })),
                                    )
                                    .child(
                                        Switch::new("use-glossary")
                                            .label("용어집 적용")
                                            .checked(self.settings.prompt.use_glossary)
                                            .on_change(cx.listener(|this, checked: &bool, _, cx| {
                                                this.settings.prompt.use_glossary = *checked;
                                                this.auto_save(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(widgets::section_label("용어집", cx))
            .child(
                widgets::card(cx)
                    .child(widgets::panel_header(
                        format!("용어집 · {glossary_status}"),
                        None,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .gap_2()
                            .p_3()
                            .child(widgets::muted(
                                "한 줄에 `원문 => 번역` 하나씩 입력합니다. 탭 구분도 됩니다.",
                                cx,
                            ))
                            .child(
                                div()
                                    .h(px(170.0))
                                    .child(Textarea::new(&self.glossary_editor).h(relative(1.0))),
                            )
                            .children(warning_lines),
                    ),
            )
            .child(widgets::section_label("엔진", cx))
            .child(
                widgets::card(cx)
                    .child(widgets::panel_header("llama.cpp 실행 옵션", None, cx))
                    .child(
                        v_flex()
                            .gap_1()
                            .p_3()
                            .child(widgets::setting_row(
                                "모델",
                                widgets::muted(
                                    self.current_model_name()
                                        .unwrap_or_else(|| "선택 없음".to_string()),
                                    cx,
                                ),
                            ))
                            .child(widgets::setting_row(
                                "context",
                                stepper("ctx", format!("{}", self.settings.server.ctx_size), cx),
                            ))
                            .child(widgets::setting_row(
                                "threads",
                                stepper("threads", format!("{}", self.settings.server.threads), cx),
                            ))
                            .child(widgets::setting_row(
                                "GPU layers (-1 = 자동)",
                                stepper(
                                    "gpu-layers",
                                    format!("{}", self.settings.server.gpu_layers),
                                    cx,
                                ),
                            ))
                            .child(widgets::setting_row(
                                "port",
                                widgets::muted(format!("{}", self.settings.server.port), cx),
                            ))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .mt_2()
                                    .child(
                                        Button::new("save-settings")
                                            .secondary()
                                            .label("설정 저장")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.save_settings_with_notice(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("save-restart")
                                            .primary()
                                            .label("저장 후 엔진 재시작")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.on_restart_engine(window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(widgets::section_label("다운로드", cx))
            .child(
                widgets::card(cx)
                    .child(widgets::panel_header("모델 다운로드 서버", None, cx))
                    .child(
                        v_flex()
                            .gap_2()
                            .p_3()
                            .child(widgets::muted(
                                "네트워크 환경에 따라 Hugging Face 또는 미러를 선택하세요.",
                                cx,
                            ))
                            .child(
                                h_flex().gap_2().children([
                                    self.base_url_button("Hugging Face", "https://huggingface.co", cx),
                                    self.base_url_button("hf-mirror.com", "https://hf-mirror.com", cx),
                                ]),
                            )
                            .child(widgets::muted(self.settings.download_base.clone(), cx)),
                    ),
            )
    }

    fn base_url_button(
        &self,
        label: &'static str,
        url: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        let active = self.settings.download_base == url;
        Button::new(SharedString::from(format!("base-url-{label}")))
            .text()
            .label(label)
            .selected(active)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings.download_base = url.to_string();
                this.auto_save(cx);
            }))
    }

    fn auto_save(&mut self, cx: &mut Context<Self>) {
        self.settings = self.settings.clone().sanitized();
        let _ = self.save_settings();
        cx.notify();
    }

    /// Applies one stepper step to the field named by `id`; ranges come from
    /// [`Settings::sanitized`], which [`Self::auto_save`] runs afterwards.
    fn adjust_setting(&mut self, id: &'static str, direction: f64, cx: &mut Context<Self>) {
        match id {
            "temperature" => self.settings.generation.temperature += direction * 0.05,
            "top-p" => self.settings.generation.top_p += direction * 0.05,
            "top-k" => self.settings.generation.top_k += direction as i32,
            "repeat-penalty" => self.settings.generation.repeat_penalty += direction * 0.01,
            "max-tokens" => {
                self.settings.generation.max_tokens = (self.settings.generation.max_tokens as i64
                    + direction as i64 * 128)
                    .clamp(16, 8192) as u32;
            }
            "ctx" => {
                self.settings.server.ctx_size =
                    (self.settings.server.ctx_size as i64 + direction as i64 * 1024)
                        .clamp(512, 32_768) as u32;
            }
            "threads" => {
                self.settings.server.threads =
                    (self.settings.server.threads as i64 + direction as i64).clamp(1, 64) as u32;
            }
            "gpu-layers" => {
                self.settings.server.gpu_layers =
                    (self.settings.server.gpu_layers + direction as i32).clamp(-1, 256);
            }
            _ => {}
        }
        self.auto_save(cx);
    }

    fn on_reset_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.settings.server.model_path.clone();
        let base = self.settings.download_base.clone();
        self.settings = Settings::default();
        self.settings.server.model_path = model;
        self.settings.download_base = base;
        self.glossary_editor
            .update(cx, |state, cx| state.clean(window, cx));
        self.settings.prompt.glossary.clear();
        let _ = self.save_settings();
        self.notify("설정을 기본값으로 되돌렸습니다.", false, window, cx);
        cx.notify();
    }

    fn on_restart_engine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.prompt.glossary = self.glossary_editor.read(cx).value().to_string();
        self.settings = self.settings.clone().sanitized();
        let _ = self.save_settings();
        self.restart_engine(window, cx);
    }
}

/// A `- value +` control bound to a settings field, wired to [`Root::adjust_setting`].
fn stepper(id: &'static str, value: String, cx: &mut Context<Root>) -> Div {
    let dec = cx.listener(move |this, _, _, cx| this.adjust_setting(id, -1.0, cx));
    let inc = cx.listener(move |this, _, _, cx| this.adjust_setting(id, 1.0, cx));
    widgets::stepper(id, value, dec, inc)
}

