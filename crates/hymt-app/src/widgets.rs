//! Small building blocks shared by all views, styled with the GPUI Kit theme tokens.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, IconName};
use gpui_kit::prelude::*;
use gpui_kit::{div, px, AnyElement, App, ClickEvent, Div, FontWeight, Hsla, SharedString, Window};

/// A raised panel with the standard border and radius.
pub fn card(cx: &App) -> Div {
    v_flex()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().secondary)
}

/// A card header with a title on the left and arbitrary controls on the right.
pub fn panel_header(title: impl Into<SharedString>, right: Option<AnyElement>, cx: &App) -> Div {
    let mut header = h_flex()
        .items_center()
        .justify_between()
        .px_3()
        .h(px(42.0))
        .flex_shrink_0()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().muted_foreground)
                .child(title.into()),
        );
    if let Some(right) = right {
        header = header.child(right);
    }
    header
}

/// Small section label.
pub fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// Secondary explanatory text.
pub fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// Lays out a label and a value on one line, value on the right.
pub fn setting_row(label: impl Into<SharedString>, value: impl IntoElement) -> Div {
    h_flex()
        .items_center()
        .justify_between()
        .gap_4()
        .py_1()
        .child(div().text_sm().child(label.into()))
        .child(value)
}

/// A colored status dot with a label.
pub fn status_dot(color: Hsla, label: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .child(div().size(px(8.0)).rounded_full().bg(color))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
}

/// A `- value +` control used for numeric settings.
pub fn stepper(
    id: &'static str,
    value: impl Into<SharedString>,
    minus: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    plus: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    h_flex()
        .items_center()
        .gap_1()
        .child(
            Button::new(SharedString::from(format!("{id}-dec")))
                .text()
                .compact()
                .icon(IconName::Minus)
                .on_click(minus),
        )
        .child(
            div()
                .w(px(72.0))
                .flex()
                .justify_center()
                .text_sm()
                .child(value.into()),
        )
        .child(
            Button::new(SharedString::from(format!("{id}-inc")))
                .text()
                .compact()
                .icon(IconName::Plus)
                .on_click(plus),
        )
}
