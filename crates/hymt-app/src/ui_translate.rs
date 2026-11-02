//! The translation workspace: language pair, source editor, streaming output and actions.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::combobox::Combobox;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{h_flex, v_flex, Disableable, Selectable};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, relative, rems, Context, Div, KeyDownEvent, Window};
use hymt_core::settings::{text_line_height, zoom_text_size, TextZoom, TEXT_SIZE_DEFAULT};

use crate::root::{Root, STYLE_PRESETS};
use crate::widgets;

impl Root {
    pub fn render_translate(&mut self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_3()
            .capture_key_down(cx.listener(Self::on_text_zoom_key))
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
                    .tooltip("언어 교체")
                    .accessibility_label("언어 교체")
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
            .child(self.text_size_controls(cx))
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
                    .ghost()
                    .compact()
                    .icon(IconName::ClipboardPaste)
                    .tooltip("붙여넣기")
                    .accessibility_label("붙여넣기")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.paste_into_source(window, cx);
                    })),
            )
            .child(
                Button::new("clear")
                    .ghost()
                    .compact()
                    .icon(IconName::Eraser)
                    .tooltip("지우기")
                    .accessibility_label("지우기")
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
                div().flex_1().min_h_0().min_w_0().child(
                    Textarea::new(&self.source)
                        .h(relative(1.0))
                        .bordered(false)
                        .text_size(px(f32::from(self.settings.text_size)))
                        .line_height(px(text_line_height(self.settings.text_size))),
                ),
            )
    }

    fn render_output_panel(&self, cx: &Context<Self>) -> Div {
        let actions = h_flex()
            .items_center()
            .gap_1()
            .child(
                Button::new("to-input")
                    .ghost()
                    .compact()
                    .icon(IconName::ArrowLeft)
                    .tooltip("입력으로")
                    .accessibility_label("입력으로")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.output_to_input(window, cx);
                    })),
            )
            .child(
                Button::new("copy")
                    .ghost()
                    .compact()
                    .icon(IconName::Copy)
                    .tooltip("복사")
                    .accessibility_label("복사")
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
                        .readonly(true)
                        .text_size(px(f32::from(self.settings.text_size)))
                        .line_height(px(text_line_height(self.settings.text_size))),
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

    /// `A- 14 A+` controls shared by both editors; clicking the size resets it.
    fn text_size_controls(&self, cx: &Context<Self>) -> Div {
        let size = self.settings.text_size;
        let button = |id: &'static str, zoom: TextZoom, label: String, tip: &'static str| {
            Button::new(id)
                .ghost()
                .compact()
                .label(label)
                .tooltip(tip)
                .accessibility_label(tip)
                .on_click(cx.listener(move |this, _, _, cx| this.zoom_text(zoom, cx)))
        };
        h_flex()
            .items_center()
            .gap_0p5()
            .child(button(
                "text-size-dec",
                TextZoom::Out,
                "A-".into(),
                "글자 작게  Ctrl+-",
            ))
            .child(
                button(
                    "text-size-reset",
                    TextZoom::Reset,
                    size.to_string(),
                    "글자 크기 초기화  Ctrl+0",
                )
                .selected(size != TEXT_SIZE_DEFAULT),
            )
            .child(button(
                "text-size-inc",
                TextZoom::In,
                "A+".into(),
                "글자 크게  Ctrl+=",
            ))
    }

    fn zoom_text(&mut self, zoom: TextZoom, cx: &mut Context<Self>) {
        let next = zoom_text_size(self.settings.text_size, zoom);
        if next != self.settings.text_size {
            self.settings.text_size = next;
            let _ = self.save_settings();
            cx.notify();
        }
    }

    /// Ctrl+= / Ctrl+- / Ctrl+0 change the shared editor text size.
    fn on_text_zoom_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = &keystroke.modifiers;
        if !m.control || m.alt || m.platform {
            return;
        }
        let zoom = match keystroke.key.as_str() {
            "=" | "+" => TextZoom::In,
            "-" => TextZoom::Out,
            "0" => TextZoom::Reset,
            _ => return,
        };
        self.zoom_text(zoom, cx);
        cx.stop_propagation();
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
