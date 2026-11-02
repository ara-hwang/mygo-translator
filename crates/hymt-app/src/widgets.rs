//! Small styled building blocks shared by all views.

use gpui::{div, prelude::*, rgb, Div, SharedString};

use crate::theme;

/// Filled accent button. Attach a click handler at the call site.
pub fn primary(label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(gpui::px(34.0))
        .px_4()
        .rounded_md()
        .bg(rgb(theme::ACCENT))
        .text_color(gpui::white())
        .text_sm()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|style| style.bg(gpui::rgb(0x7aa2ff)))
        .child(label.into())
}

/// Outlined button for secondary actions.
pub fn secondary(label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(gpui::px(34.0))
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL_ALT))
        .text_color(rgb(theme::TEXT))
        .text_sm()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
        .child(label.into())
}

/// A button that looks unavailable.
pub fn disabled(label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(gpui::px(34.0))
        .px_4()
        .rounded_md()
        .bg(rgb(theme::PANEL_ALT))
        .text_color(rgb(theme::DISABLED))
        .text_sm()
        .child(label.into())
}

/// Compact pill, used for language and option selection.
pub fn chip(label: impl Into<SharedString>, active: bool) -> Div {
    let (background, border, color) = if active {
        (theme::ACCENT_SOFT, theme::ACCENT, theme::TEXT)
    } else {
        (theme::PANEL_ALT, theme::BORDER, theme::MUTED)
    };
    div()
        .flex()
        .items_center()
        .justify_center()
        .h(gpui::px(30.0))
        .px_3()
        .rounded_full()
        .border_1()
        .border_color(rgb(border))
        .bg(rgb(background))
        .text_color(rgb(color))
        .text_sm()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(theme::PANEL_HOVER)))
        .child(label.into())
}

/// A raised panel with the standard border and radius.
pub fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL))
}

/// Small uppercase-ish section label.
pub fn section_label(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(theme::MUTED))
        .child(text.into())
}

/// Secondary explanatory text.
pub fn muted(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .text_color(rgb(theme::MUTED))
        .child(text.into())
}

/// A colored status dot with a label.
pub fn status_dot(color: u32, label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(div().size(gpui::px(8.0)).rounded_full().bg(rgb(color)))
        .child(
            div()
                .text_xs()
                .text_color(rgb(theme::MUTED))
                .child(label.into()),
        )
}

/// Lays out a label and a value on one line, value on the right.
pub fn setting_row(label: impl Into<SharedString>, value: impl gpui::IntoElement) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap_4()
        .py_1()
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme::TEXT))
                .child(label.into()),
        )
        .child(value)
}
