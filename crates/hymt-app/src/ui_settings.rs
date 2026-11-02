//! Settings: sampling, prompt construction, glossary and engine launch options.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::theme::Theme;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Selectable};
use gpui_kit::prelude::*;
use gpui_kit::{
    div, px, relative, rems, AnyElement, Context, DispatchPhase, Div, KeyDownEvent, ScrollEvent,
    SharedString, Window,
};

use hymt_core::{
    glossary,
    settings::{AcquisitionMode, Appearance, Settings},
};

use crate::hotkey;
use crate::root::{Root, ScrollSpot, PROMPT_LANGUAGES, STYLE_PRESETS};
use crate::widgets;

impl Root {
    pub fn render_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let prompt_radios: Vec<Radio> = PROMPT_LANGUAGES
            .iter()
            .enumerate()
            .map(|(index, (label, _))| {
                Radio::new(SharedString::from(format!("prompt-language-{index}")))
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
            .track_scroll(self.settings_scroll.handle())
            .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                this.smooth_scroll(ScrollSpot::Settings, event, window, cx);
            }))
            .p_4()
            .gap_3()
            .child(widgets::section_label("시작 및 트레이", cx))
            .child(
                widgets::card(cx)
                    .child(widgets::panel_header("백그라운드 동작", None, cx))
                    .child(
                        v_flex()
                            .gap_3()
                            .p_3()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        Switch::new("keep-in-tray")
                                            .label("트레이 아이콘으로 유지")
                                            .checked(self.settings.desktop.keep_in_tray)
                                            .on_change(cx.listener(
                                                |this, checked: &bool, window, cx| {
                                                    this.set_tray_enabled(*checked, window, cx);
                                                },
                                            )),
                                    )
                                    .child(widgets::muted(
                                        "창을 닫아도 종료하지 않고 알림 영역에서 계속 실행합니다. 아이콘을 왼쪽 클릭하면 창이 다시 열립니다.",
                                        cx,
                                    )),
                            )
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        Switch::new("start-with-windows")
                                            .label("Windows 시작 시 자동 실행")
                                            .checked(self.settings.desktop.start_with_windows)
                                            .on_change(cx.listener(
                                                |this, checked: &bool, window, cx| {
                                                    this.set_start_with_windows(*checked, window, cx);
                                                },
                                            )),
                                    )
                                    .child(widgets::muted(
                                        "현재 실행 파일을 이 사용자의 시작 프로그램에 등록합니다. 트레이 유지가 켜져 있으면 창 없이 트레이로 시작합니다.",
                                        cx,
                                    )),
                            ),
                    ),
            )
            .child(widgets::section_label("단축키", cx))
            .child(self.render_hotkey_section(window, cx))
            .child(widgets::section_label("화면", cx))
            .child(
                widgets::card(cx)
                    .child(widgets::panel_header("테마", None, cx))
                    .child(
                        v_flex()
                            .gap_2()
                            .p_3()
                            .child(widgets::muted(
                                "시스템은 Windows 설정의 모드를 따릅니다.",
                                cx,
                            ))
                            .child(Select::new(&self.theme_select).menu_max_h(rems(12.0))),
                    ),
            )
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
                                stepper(
                                    "temperature",
                                    format!("{:.2}", self.settings.generation.temperature),
                                    cx,
                                ),
                            ))
                            .child(widgets::setting_row(
                                "top_p",
                                stepper(
                                    "top-p",
                                    format!("{:.2}", self.settings.generation.top_p),
                                    cx,
                                ),
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
                                stepper(
                                    "max-tokens",
                                    format!("{}", self.settings.generation.max_tokens),
                                    cx,
                                ),
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
                                            .on_change(cx.listener(
                                                |this, index: &usize, _, cx| {
                                                    if let Some((_, language)) =
                                                        PROMPT_LANGUAGES.get(*index)
                                                    {
                                                        this.settings.prompt.language = *language;
                                                        this.auto_save(cx);
                                                    }
                                                },
                                            )),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(widgets::muted(
                                        "스타일 (용어집보다 우선순위가 낮습니다)",
                                        cx,
                                    ))
                                    .child(h_flex().gap_2().flex_wrap().children(style_buttons)),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_6()
                                    .child(
                                        Switch::new("preserve-delimiters")
                                            .label("구분자 보존")
                                            .checked(self.settings.prompt.preserve_delimiters)
                                            .on_change(cx.listener(
                                                |this, checked: &bool, _, cx| {
                                                    this.settings.prompt.preserve_delimiters =
                                                        *checked;
                                                    this.auto_save(cx);
                                                },
                                            )),
                                    )
                                    .child(
                                        Switch::new("use-glossary")
                                            .label("용어집 적용")
                                            .checked(self.settings.prompt.use_glossary)
                                            .on_change(cx.listener(
                                                |this, checked: &bool, _, cx| {
                                                    this.settings.prompt.use_glossary = *checked;
                                                    this.auto_save(cx);
                                                },
                                            )),
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
                            .child(h_flex().gap_2().children([
                                self.base_url_button("Hugging Face", "https://huggingface.co", cx),
                                self.base_url_button("hf-mirror.com", "https://hf-mirror.com", cx),
                            ]))
                            .child(widgets::muted(self.settings.download_base.clone(), cx)),
                    ),
            )
    }

    /// Global-hotkey section: master switch, both trigger bindings and the selected-text
    /// acquisition mode. While a binding is being recorded, every key press — captured on
    /// the window during paint — feeds the recorder.
    fn render_hotkey_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let recording = self.hotkey_recorder.as_ref().map(|recorder| recorder.slot);
        if recording.is_some() {
            let entity = cx.entity();
            window.on_key_event::<KeyDownEvent>(move |event, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    entity.update(cx, |root, cx| root.record_hotkey_key(event, window, cx));
                }
            });
        }

        let chord_value = self
            .settings
            .hotkey
            .combo
            .as_ref()
            .map(hotkey::format_combo)
            .unwrap_or_else(|| "등록 안 됨".to_string());
        let double_value = self
            .settings
            .hotkey
            .double_tap
            .as_ref()
            .map(hotkey::format_double_tap)
            .unwrap_or_else(|| "등록 안 됨".to_string());
        let awaiting_second = matches!(&self.hotkey_recorder, Some(recorder) if recorder.slot == hotkey::RecorderSlot::DoubleTap && recorder.first.is_some());

        let chord_button = if recording == Some(hotkey::RecorderSlot::Chord) {
            Button::new("hotkey-chord")
                .text()
                .label("기록 중… (Esc로 취소)")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_hotkey_recorder(None, window, cx);
                }))
        } else {
            Button::new("hotkey-chord")
                .text()
                .label(chord_value)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_hotkey_recorder(Some(hotkey::RecorderSlot::Chord), window, cx);
                }))
        };
        let double_button = if awaiting_second {
            Button::new("hotkey-double")
                .text()
                .label("한 번 더 눌러 주세요… (Esc로 취소)")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_hotkey_recorder(None, window, cx);
                }))
        } else if recording == Some(hotkey::RecorderSlot::DoubleTap) {
            Button::new("hotkey-double")
                .text()
                .label("두 번 눌러 주세요…")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_hotkey_recorder(None, window, cx);
                }))
        } else {
            Button::new("hotkey-double")
                .text()
                .label(double_value)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_hotkey_recorder(Some(hotkey::RecorderSlot::DoubleTap), window, cx);
                }))
        };

        let acquisition = self.settings.hotkey.acquisition;
        let acquisition_radios = vec![
            Radio::new("hotkey-acq-auto")
                .label("UI Automation 우선 (권장)")
                .checked(acquisition == AcquisitionMode::Auto),
            Radio::new("hotkey-acq-clipboard")
                .label("Ctrl+C 클립보드만")
                .checked(acquisition == AcquisitionMode::ClipboardOnly),
        ];
        let acquisition_index = match acquisition {
            AcquisitionMode::Auto => 0,
            AcquisitionMode::ClipboardOnly => 1,
        };
        let acquisition_description = match acquisition {
            AcquisitionMode::Auto => {
                "크롬·워드·터미널 등 대부분의 앱에서 선택 텍스트를 직접 읽고, 지원하지 않는 앱만 Ctrl+C로 잠시 복사합니다."
            }
            AcquisitionMode::ClipboardOnly => {
                "항상 Ctrl+C를 대신 눌러 클립보드에서 읽습니다. 이전 클립보드 텍스트는 다시 채워 넣습니다."
            }
        };

        let (status, status_error) = self.hotkey_status_text();
        let status_line = if status_error {
            div().text_xs().text_color(cx.theme().danger).child(status)
        } else {
            widgets::muted(status, cx)
        };

        widgets::card(cx)
            .child(widgets::panel_header("이 창을 열지 않고 즉시 번역", None, cx))
            .child(
                v_flex()
                    .gap_3()
                    .p_3()
                    .child(status_line)
                    .child(
                        Switch::new("hotkey-enabled")
                            .label("전역 단축키 사용")
                            .checked(self.settings.hotkey.enabled)
                            .on_change(cx.listener(|this, checked: &bool, window, cx| {
                                this.set_hotkey_enabled(*checked, window, cx);
                            })),
                    )
                    .child(widgets::muted(
                        "다른 앱에서 텍스트를 선택한 상태로 단축키를 누르면 HyMT 창이 열리면서 번역이 시작됩니다. 언어 쌍은 현재 선택된 값을 그대로 사용합니다.",
                        cx,
                    ))
                    .child(widgets::setting_row("조합 단축키", chord_button))
                    .child(widgets::muted(
                        "한 번의 조합 입력으로 즉시 실행됩니다. 등록한 조합은 다른 앱에서 원래 하던 동작 대신 HyMT를 띄웁니다.",
                        cx,
                    ))
                    .child(widgets::setting_row("두 번 누르기", double_button))
                    .child(widgets::muted(
                        "DeepL처럼 같은 키를 빠르게 두 번 누릅니다. 키 입력은 흡수하지 않으므로 일반 Ctrl+C 복사에는 영향이 없습니다.",
                        cx,
                    ))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(widgets::muted("선택 텍스트 획득 방식", cx))
                            .child(
                                RadioGroup::new("hotkey-acquisition")
                                    .children(acquisition_radios)
                                    .selected_index(Some(acquisition_index))
                                    .on_change(cx.listener(
                                        |this, index: &usize, window, cx| {
                                            let mode = if *index == 0 {
                                                AcquisitionMode::Auto
                                            } else {
                                                AcquisitionMode::ClipboardOnly
                                            };
                                            this.set_hotkey_acquisition(mode, window, cx);
                                        },
                                    )),
                            )
                            .child(widgets::muted(acquisition_description, cx)),
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

    pub fn set_appearance(&mut self, appearance: Appearance, cx: &mut Context<Self>) {
        self.settings.appearance = appearance;
        self.apply_appearance_setting(cx);
    }

    /// Applies the persisted appearance choice to every window and persists it.
    fn apply_appearance_setting(&mut self, cx: &mut Context<Self>) {
        Theme::change(crate::theme_mode(self.settings.appearance, cx), None, cx);
        self.auto_save(cx);
    }

    /// Re-applies the theme when Windows switches appearance while on `System`.
    pub fn on_system_appearance_changed(&mut self, cx: &mut Context<Self>) {
        if self.settings.appearance != Appearance::System {
            return;
        }
        // The observer also fires once when the window first opens; skip the
        // redundant work when the current mode already matches.
        let mode = crate::theme_mode(self.settings.appearance, cx);
        if cx.theme().mode != mode {
            self.apply_appearance_setting(cx);
        }
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
                self.settings.server.ctx_size = (self.settings.server.ctx_size as i64
                    + direction as i64 * 1024)
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
        let had_autostart = self.settings.desktop.start_with_windows;
        self.settings = Settings::default();
        self.settings.server.model_path = model;
        self.settings.download_base = base;
        let appearance = self.settings.appearance;
        self.theme_select.update(cx, |state, cx| {
            state.set_selected_value(&appearance, window, cx);
        });
        Theme::change(crate::theme_mode(self.settings.appearance, cx), None, cx);
        self.tray = None;
        if had_autostart {
            if let Err(err) = crate::autostart::set_enabled(false) {
                self.settings.desktop.start_with_windows = true;
                self.notify(
                    format!("시작 프로그램을 해제할 수 없습니다: {err}"),
                    true,
                    window,
                    cx,
                );
            }
        }
        self.glossary_editor
            .update(cx, |state, cx| state.clean(window, cx));
        self.settings.prompt.glossary.clear();
        let _ = self.save_settings();
        self.sync_hotkey();
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
