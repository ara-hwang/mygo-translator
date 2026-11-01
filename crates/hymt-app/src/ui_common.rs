//! Shared chrome: the top bar, notices, the status bar and small form controls.

use gpui::{
    div, prelude::*, px, rgb, App, Context, Div, IntoElement, MouseButton, MouseUpEvent,
    SharedString, Window,
};

use hymt_core::engine::EngineState;

use crate::{
    root::{Root, Tab},
    theme, widgets,
};

impl gpui::Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme::BG))
            .text_color(rgb(theme::TEXT))
            .key_context("HyMT")
            .on_key_down(cx.listener(Self::on_root_key))
            .child(self.render_top_bar(cx))
            .child(self.render_body(cx))
            .child(self.render_notice())
            .child(self.render_status_bar())
    }
}

impl Root {
    fn render_top_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .h(px(58.0))
            .px_4()
            .flex_shrink_0()
            .bg(rgb(theme::PANEL))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("HyMT"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(theme::MUTED))
                                    .child("Tencent Hy-MT2 · 오프라인 번역"),
                            ),
                    )
                    .child(div().w(px(1.0)).h(px(26.0)).bg(rgb(theme::BORDER)))
                    .child(self.tab_button("번역", Tab::Translate, cx))
                    .child(self.tab_button("모델", Tab::Models, cx))
                    .child(self.tab_button("설정", Tab::Settings, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(widgets::status_dot(
                        self.engine_color(),
                        self.engine_summary(),
                    ))
                    .child(self.engine_button(cx)),
            )
    }

    fn tab_button(&self, label: &'static str, tab: Tab, cx: &mut Context<Self>) -> Div {
        let active = self.tab == tab && self.picker.is_none();
        div()
            .flex()
            .items_center()
            .justify_center()
            .h(px(32.0))
            .px_3()
            .rounded_md()
            .text_sm()
            .text_color(rgb(if active { theme::TEXT } else { theme::MUTED }))
            .bg(rgb(if active {
                theme::PANEL_ALT
            } else {
                theme::PANEL
            }))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |root, _: &MouseUpEvent, _window, cx| {
                    root.tab = tab;
                    root.picker = None;
                    cx.notify();
                }),
            )
            .child(label)
    }

    fn engine_button(&self, cx: &mut Context<Self>) -> Div {
        match &self.engine_state {
            EngineState::Starting => widgets::disabled("불러오는 중…"),
            EngineState::Ready => widgets::secondary("엔진 중지")
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_stop_engine)),
            _ => widgets::secondary("엔진 시작")
                .on_mouse_up(MouseButton::Left, cx.listener(Self::on_start_engine)),
        }
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> Div {
        let content: gpui::AnyElement = if self.picker.is_some() {
            self.render_picker(cx).into_any_element()
        } else {
            match self.tab {
                Tab::Translate => self.render_translate(cx).into_any_element(),
                Tab::Models => self.render_models(cx).into_any_element(),
                Tab::Settings => self.render_settings(cx).into_any_element(),
            }
        };
        div().flex().flex_col().flex_1().min_h_0().child(content)
    }

    fn render_notice(&self) -> Div {
        let body = match &self.notice {
            Some(notice) => div()
                .mx_4()
                .mb_2()
                .px_3()
                .py_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(if notice.is_error {
                    theme::ERROR
                } else {
                    theme::OK
                }))
                .bg(rgb(if notice.is_error { 0x2a1a1e } else { 0x15241f }))
                .text_xs()
                .text_color(rgb(if notice.is_error { 0xffc2c2 } else { 0xa9eccd }))
                .child(notice.text.clone())
                .into_any_element(),
            None => div().h(px(0.0)).into_any_element(),
        };
        div().flex_shrink_0().child(body)
    }

    fn render_status_bar(&self) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .h(px(34.0))
            .px_4()
            .flex_shrink_0()
            .bg(rgb(theme::PANEL))
            .border_t_1()
            .border_color(rgb(theme::BORDER))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(widgets::muted(
                        self.current_model_name()
                            .unwrap_or_else(|| "모델 없음".to_string()),
                    ))
                    .child(div().w(px(1.0)).h(px(12.0)).bg(rgb(theme::BORDER)))
                    .child(widgets::muted(self.stats_line())),
            )
            .child(widgets::muted(format!(
                "ctx {} · 스레드 {} · GPU {} · 포트 {}",
                self.settings.server.ctx_size,
                self.settings.server.threads,
                self.settings.server.gpu_layers,
                self.settings.server.port
            )))
    }

    /// Keyboard shortcuts that belong to the window rather than an editor.
    fn on_root_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.to_ascii_lowercase();
        if key == "escape" && self.picker.is_some() {
            self.close_picker(cx);
            return;
        }
        let command = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        if command && key == "enter" {
            self.translate(cx);
            return;
        }
        let _ = window;
    }

    fn on_start_engine(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.start_engine(cx);
    }

    fn on_stop_engine(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_engine(cx);
    }
}

/// A `- value +` control used for numeric settings.
pub fn stepper(
    value: impl Into<SharedString>,
    minus: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    plus: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(step_button("−", minus))
        .child(
            div()
                .w(px(72.0))
                .flex()
                .justify_center()
                .text_sm()
                .text_color(rgb(theme::TEXT))
                .child(value.into()),
        )
        .child(step_button("+", plus))
}

fn step_button(
    label: &'static str,
    handler: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .size(px(26.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL_ALT))
        .text_color(rgb(theme::TEXT))
        .text_sm()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
        .on_mouse_up(MouseButton::Left, handler)
        .child(label)
}

/// A card header with a title on the left and arbitrary controls on the right.
pub fn panel_header(title: impl Into<SharedString>, right: Option<Div>) -> Div {
    let mut header = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px_3()
        .h(px(42.0))
        .flex_shrink_0()
        .border_b_1()
        .border_color(rgb(theme::BORDER))
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(theme::MUTED))
                .child(title.into()),
        );
    if let Some(right) = right {
        header = header.child(right);
    }
    header
}
