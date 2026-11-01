//! The translation workspace: language pair, source editor, streaming output and actions.

use gpui::{
    div, prelude::*, px, relative, rgb, Context, Div, IntoElement, MouseButton, MouseUpEvent,
    Window,
};

use crate::{
    root::{LangSlot, Root, STYLE_PRESETS},
    theme,
    ui_common::panel_header,
    widgets,
};

impl Root {
    pub fn render_translate(&mut self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_3()
            .child(self.render_language_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(self.render_source_panel(cx))
                    .child(self.render_output_panel(cx)),
            )
            .child(self.render_action_bar(cx))
    }

    fn render_language_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(self.language_chip(LangSlot::Source, cx))
            .child(
                div()
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(theme::BORDER))
                    .bg(rgb(theme::PANEL_ALT))
                    .text_color(rgb(theme::MUTED))
                    .text_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_swap_languages))
                    .child("⇄"),
            )
            .child(self.language_chip(LangSlot::Target, cx))
            .child(div().flex_1())
            .child(self.glossary_chip(cx))
            .child(self.style_chip(cx))
    }

    fn language_chip(&self, slot: LangSlot, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .h(px(36.0))
            .w(px(200.0))
            .px_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(theme::BORDER))
            .bg(rgb(theme::PANEL_ALT))
            .text_sm()
            .text_color(rgb(theme::TEXT))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |root, _: &MouseUpEvent, window, cx| {
                    root.open_picker(slot, window, cx);
                }),
            )
            .child(self.language_label(slot))
            .child(div().text_xs().text_color(rgb(theme::MUTED)).child("▾"))
    }

    fn style_chip(&self, cx: &mut Context<Self>) -> Div {
        let current = self.settings.prompt.style.trim();
        let label = STYLE_PRESETS
            .iter()
            .find(|(_, value)| *value == current)
            .map(|(label, _)| *label)
            .unwrap_or("스타일 직접 지정");
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(36.0))
            .px_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(if current.is_empty() {
                theme::BORDER
            } else {
                theme::ACCENT
            }))
            .bg(rgb(if current.is_empty() {
                theme::PANEL_ALT
            } else {
                theme::ACCENT_SOFT
            }))
            .text_sm()
            .text_color(rgb(theme::TEXT))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_cycle_style))
            .child(format!("스타일 · {label}"))
    }

    fn glossary_chip(&self, cx: &mut Context<Self>) -> Div {
        let count = self.glossary_count();
        let active = self.settings.prompt.use_glossary && count > 0;
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(36.0))
            .px_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(if active { theme::ACCENT } else { theme::BORDER }))
            .bg(rgb(if active {
                theme::ACCENT_SOFT
            } else {
                theme::PANEL_ALT
            }))
            .text_sm()
            .text_color(rgb(if count == 0 {
                theme::MUTED
            } else {
                theme::TEXT
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_toggle_glossary))
            .child(if count == 0 {
                "용어집 없음".to_string()
            } else if active {
                format!("용어집 {count}개 사용 중")
            } else {
                format!("용어집 {count}개")
            })
    }

    fn render_source_panel(&self, cx: &mut Context<Self>) -> Div {
        let actions = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme::MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(theme::TEXT)))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_paste_click))
                    .child("붙여넣기"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme::MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(theme::TEXT)))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_clear_click))
                    .child("지우기"),
            );

        widgets::card()
            .flex_1()
            .min_h_0()
            .child(panel_header("원문", Some(actions)))
            .child(div().flex_1().min_h_0().child(self.source.clone()))
    }

    fn render_output_panel(&self, cx: &mut Context<Self>) -> Div {
        let actions = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme::MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(theme::TEXT)))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_output_to_input))
                    .child("입력으로"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme::MUTED))
                    .cursor_pointer()
                    .hover(|style| style.text_color(rgb(theme::TEXT)))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_copy_click))
                    .child("복사"),
            );

        let body = if self.output.is_empty() {
            div()
                .flex_1()
                .min_h_0()
                .p_3()
                .child(widgets::muted(if self.translating {
                    "번역을 생성하는 중…"
                } else {
                    "번역 결과가 여기에 표시됩니다."
                }))
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .id("output-scroll")
                .overflow_y_scroll()
                .child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(rgb(if self.output_is_error {
                            theme::ERROR
                        } else {
                            theme::TEXT
                        }))
                        .child(self.output.clone()),
                )
                .into_any_element()
        };

        widgets::card()
            .flex_1()
            .min_h_0()
            .child(panel_header("번역", Some(actions)))
            .child(body)
    }

    fn render_action_bar(&self, cx: &mut Context<Self>) -> Div {
        let primary = if self.translating {
            widgets::secondary("중단")
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_cancel_click))
        } else if self.engine_state.is_ready() {
            widgets::primary("번역  Ctrl+Enter")
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_translate_click))
        } else {
            widgets::disabled("모델 준비 중…")
        };

        let progress = match &self.download_progress() {
            Some(progress) => div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .w(px(120.0))
                        .h(px(4.0))
                        .rounded_full()
                        .bg(rgb(theme::PANEL_ALT))
                        .child(
                            div()
                                .h(px(4.0))
                                .rounded_full()
                                .bg(rgb(theme::ACCENT))
                                .w(relative(progress.fraction())),
                        ),
                )
                .child(widgets::muted(progress.label()))
                .into_any_element(),
            None => div().into_any_element(),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .flex_shrink_0()
            .child(progress)
            .child(div().flex_1())
            .child(widgets::muted(self.stats_line()))
            .child(primary)
    }

    fn on_translate_click(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.translate(cx);
    }

    fn on_cancel_click(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.cancel_translation(cx);
    }

    fn on_copy_click(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_output(cx);
    }

    fn on_clear_click(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.clear_all(cx);
    }

    fn on_paste_click(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_into_source(cx);
    }

    fn on_output_to_input(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.output_to_input(cx);
    }

    fn on_swap_languages(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.swap_languages(cx);
    }

    fn on_cycle_style(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let current = self.settings.prompt.style.trim().to_string();
        let index = STYLE_PRESETS
            .iter()
            .position(|(_, value)| *value == current)
            .unwrap_or(0);
        let next = (index + 1) % STYLE_PRESETS.len();
        self.settings.prompt.style = STYLE_PRESETS[next].1.to_string();
        let _ = self.save_settings();
        cx.notify();
    }

    fn on_toggle_glossary(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.glossary_count() == 0 {
            self.notice = Some(crate::root::Notice::error(
                "용어집이 비어 있습니다. 설정 탭에서 `원문 => 번역` 형식으로 추가하세요.",
            ));
            cx.notify();
            return;
        }
        self.settings.prompt.use_glossary = !self.settings.prompt.use_glossary;
        let _ = self.save_settings();
        cx.notify();
    }
}
