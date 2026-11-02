//! The translation workspace: language pair, source editor, streaming output and actions.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::combobox::Combobox;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{h_flex, v_flex, Disableable, Selectable};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, relative, rems, Context, Div, Window};

use crate::root::{Root, STYLE_PRESETS};
use crate::widgets;

impl Root {
    pub fn render_translate(&mut self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_3()
            .child(self.render_language_bar(cx))
            .child(
                // h_flex centers on the cross axis by default; the panels must
                // stretch to fill the leftover height instead.
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .items_stretch()
                    .gap_3()
                    .child(self.render_source_panel(cx))
                    .child(self.render_output_panel(cx)),
            )
            .child(self.render_action_bar(cx))
    }

    fn render_language_bar(&self, cx: &mut Context<Self>) -> Div {
        h_flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .flex_wrap()
            .child(
                // The combobox fills its parent (size_full), so the width belongs here.
                div()
                    .w(px(220.0))
                    .child(Combobox::new(&self.source_combo).menu_max_h(rems(16.0))),
            )
            .child(
                Button::new("swap-langs")
                    .ghost()
                    .compact()
                    .icon(IconName::ArrowLeftRight)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.swap_languages(window, cx);
                    })),
            )
            .child(
                div()
                    .w(px(220.0))
                    .child(Combobox::new(&self.target_combo).menu_max_h(rems(16.0))),
            )
            .child(div().flex_1())
            .child(self.glossary_button(cx))
            .child(self.style_button(cx))
    }

    fn glossary_button(&self, cx: &Context<Self>) -> Button {
        let count = self.glossary_count();
        let active = self.settings.prompt.use_glossary && count > 0;
        let label = if count == 0 {
            "용어집 없음".to_string()
        } else if active {
            format!("용어집 {count}개 사용 중")
        } else {
            format!("용어집 {count}개")
        };
        Button::new("glossary-toggle")
            .label(label)
            .selected(active)
            .on_click(cx.listener(|this, _, window, cx| this.on_toggle_glossary(window, cx)))
    }

    fn style_button(&self, cx: &Context<Self>) -> Button {
        let current = self.settings.prompt.style.trim();
        let label = STYLE_PRESETS
            .iter()
            .find(|(_, value)| *value == current)
            .map(|(label, _)| *label)
            .unwrap_or("스타일 직접 지정");
        Button::new("style-cycle")
            .label(format!("스타일 · {label}"))
            .on_click(cx.listener(|this, _, _, cx| this.on_cycle_style(cx)))
    }

    fn render_source_panel(&self, cx: &Context<Self>) -> Div {
        let actions = h_flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("paste")
                    .text()
                    .compact()
                    .label("붙여넣기")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.paste_into_source(window, cx);
                    })),
            )
            .child(
                Button::new("clear")
                    .text()
                    .compact()
                    .label("지우기")
                    .on_click(cx.listener(|this, _, window, cx| this.clear_all(window, cx))),
            );

        widgets::card(cx)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(widgets::panel_header(
                "원문",
                Some(actions.into_any_element()),
                cx,
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(Textarea::new(&self.source).h(relative(1.0)).bordered(false)),
            )
    }

    fn render_output_panel(&self, cx: &Context<Self>) -> Div {
        let actions = h_flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("to-input")
                    .text()
                    .compact()
                    .label("입력으로")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.output_to_input(window, cx);
                    })),
            )
            .child(
                Button::new("copy")
                    .text()
                    .compact()
                    .label("복사")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.copy_output(window, cx);
                    })),
            );

        widgets::card(cx)
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(widgets::panel_header(
                "번역",
                Some(actions.into_any_element()),
                cx,
            ))
            .child(
                div().flex_1().min_h_0().min_w_0().child(
                    Textarea::new(&self.output_state)
                        .h(relative(1.0))
                        .readonly(true),
                ),
            )
    }

    fn render_action_bar(&self, cx: &Context<Self>) -> Div {
        let primary = if self.translating {
            Button::new("translate-action")
                .danger()
                .label("중단")
                .on_click(cx.listener(|this, _, _, cx| this.cancel_translation(cx)))
        } else if self.engine_state.is_ready() {
            Button::new("translate-action")
                .primary()
                .label("번역  Ctrl+Enter")
                .on_click(cx.listener(|this, _, window, cx| this.translate(window, cx)))
        } else {
            Button::new("translate-action")
                .primary()
                .disabled(true)
                .label("모델 준비 중…")
        };

        let progress = match self.download_progress() {
            Some(progress) => h_flex()
                .items_center()
                .gap_2()
                .child(
                    Progress::new("download-progress")
                        .value(progress.fraction() * 100.0)
                        .w(px(140.0)),
                )
                .child(widgets::muted(progress.label(), cx))
                .into_any_element(),
            None => div().into_any_element(),
        };

        h_flex()
            .items_center()
            .gap_3()
            .flex_shrink_0()
            .child(progress)
            .child(div().flex_1())
            .child(widgets::muted(self.stats_line(), cx))
            .child(primary)
    }

    fn on_cycle_style(&mut self, cx: &mut Context<Self>) {
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

    fn on_toggle_glossary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.glossary_count() == 0 {
            self.notify(
                "용어집이 비어 있습니다. 설정 탭에서 `원문 => 번역` 형식으로 추가하세요.",
                true,
                window,
                cx,
            );
            cx.notify();
            return;
        }
        self.settings.prompt.use_glossary = !self.settings.prompt.use_glossary;
        let _ = self.save_settings();
        cx.notify();
    }
}
