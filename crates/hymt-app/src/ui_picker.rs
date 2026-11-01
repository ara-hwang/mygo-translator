//! A searchable language picker that replaces the workspace until a language is chosen.

use gpui::{div, prelude::*, px, rgb, Context, Div, MouseButton, MouseUpEvent, Window};

use hymt_core::lang::Language;

use crate::{
    root::{LangSlot, Root},
    theme, widgets,
};

impl Root {
    pub fn render_picker(&mut self, cx: &mut Context<Self>) -> Div {
        let slot = self.picker.unwrap_or(LangSlot::Source);
        let title = match slot {
            LangSlot::Source => "원문 언어",
            LangSlot::Target => "번역 언어",
        };
        let languages = self.filtered_languages(cx);
        let current = match slot {
            LangSlot::Source => self.settings.source_lang.clone(),
            LangSlot::Target => self.settings.target_lang.clone(),
        };

        let mut rows = Vec::new();
        for language in languages {
            rows.push(self.language_row(slot, language, &current, cx));
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .p_4()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(title),
                            )
                            .child(widgets::muted(
                                "Hy-MT2가 지원하는 38개 언어 중에서 선택하세요.",
                            )),
                    )
                    .child(
                        widgets::secondary("닫기  Esc")
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_close_picker)),
                    ),
            )
            .child(
                div()
                    .h(px(46.0))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(theme::BORDER))
                    .bg(rgb(theme::PANEL_ALT))
                    .child(self.picker_filter.clone()),
            )
            .child(
                widgets::card()
                    .flex_1()
                    .min_h_0()
                    .id("language-list")
                    .overflow_y_scroll()
                    .children(rows),
            )
    }

    fn language_row(
        &self,
        slot: LangSlot,
        language: &'static Language,
        current: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let selected = language.code == current;
        let code = language.code;
        let subtitle = format!("{} · {}", language.code, language.name_en);

        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .h(px(44.0))
            .px_3()
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .cursor_pointer()
            .when(selected, |row| row.bg(rgb(theme::ACCENT_SOFT)))
            .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |root, _: &MouseUpEvent, window, cx| {
                    root.set_language(slot, code, cx);
                    root.close_picker(cx);
                    root.source
                        .update(cx, |editor, cx| editor.focus(window, cx));
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme::TEXT))
                            .child(language.name_ko),
                    )
                    .child(widgets::muted(subtitle)),
            )
            .child(if selected {
                div()
                    .text_sm()
                    .text_color(rgb(theme::ACCENT))
                    .child("✓ 선택됨")
                    .into_any_element()
            } else {
                div().into_any_element()
            })
    }

    fn on_close_picker(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.close_picker(cx);
    }
}
