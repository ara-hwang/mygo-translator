//! Shared chrome: the top bar with tabs, the status bar, and small form helpers.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::{Tab as UiTab, TabBar};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement, Context, Div, FontWeight, Hsla, Window};

use hymt_core::engine::EngineState;

use crate::root::{Root, Tab};
use crate::widgets;

impl gpui_kit::Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_top_bar(cx))
            .child(self.render_body(cx))
            .child(self.render_status_bar(cx))
    }
}

impl Root {
    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = match self.tab {
            Tab::Translate => 0,
            Tab::Models => 1,
            Tab::Settings => 2,
        };

        h_flex()
            .items_center()
            .gap_4()
            .px_4()
            .py_2()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().title_bar_border)
            .bg(cx.theme().title_bar)
            .child(
                v_flex()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child("HyMT"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Tencent Hy-MT2 · 오프라인 번역"),
                    ),
            )
            .child(
                TabBar::new("tabs")
                    .segmented()
                    .selected_index(selected)
                    .children([
                        UiTab::new().label("번역"),
                        UiTab::new().label("모델"),
                        UiTab::new().label("설정"),
                    ])
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        this.tab = match *index {
                            0 => Tab::Translate,
                            1 => Tab::Models,
                            _ => Tab::Settings,
                        };
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(widgets::status_dot(
                self.engine_color(cx),
                self.engine_summary(),
                cx,
            ))
            .child(self.engine_button(cx))
    }

    fn engine_color(&self, cx: &Context<Self>) -> Hsla {
        match &self.engine_state {
            EngineState::Ready => cx.theme().success,
            EngineState::Starting => cx.theme().warning,
            EngineState::Stopped => cx.theme().muted_foreground,
            EngineState::Failed(_) => cx.theme().danger,
        }
    }

    fn engine_button(&self, cx: &mut Context<Self>) -> Button {
        match &self.engine_state {
            EngineState::Starting => Button::new("engine").disabled(true).label("불러오는 중…"),
            EngineState::Ready => Button::new("engine")
                .secondary()
                .label("엔진 중지")
                .on_click(cx.listener(|this, _, _, cx| this.stop_engine(cx))),
            _ => Button::new("engine")
                .secondary()
                .label("엔진 시작")
                .on_click(cx.listener(|this, _, window, cx| this.start_engine(window, cx))),
        }
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> Div {
        let content: AnyElement = match self.tab {
            Tab::Translate => self.render_translate(cx).into_any_element(),
            Tab::Models => self.render_models(cx).into_any_element(),
            Tab::Settings => self.render_settings(cx).into_any_element(),
        };
        div().flex().flex_col().flex_1().min_h_0().child(content)
    }

    fn render_status_bar(&self, cx: &Context<Self>) -> StatusBar {
        StatusBar::new()
            .left(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        self.current_model_name()
                            .unwrap_or_else(|| "모델 없음".to_string()),
                    ),
            )
            .left(
                div()
                    .w(px(1.0))
                    .h(px(12.0))
                    .bg(cx.theme().status_bar_border),
            )
            .left(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.stats_line()),
            )
            .right(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "ctx {} · 스레드 {} · GPU {} · 포트 {}",
                        self.settings.server.ctx_size,
                        self.settings.server.threads,
                        self.settings.server.gpu_layers,
                        self.settings.server.port
                    )),
            )
    }
}
