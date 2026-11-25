//! `NotFoundRoute`: 404 배지, 제목·설명, 뒤로가기 버튼을 화면 가운데 둔다.

use gpui_kit::{
    App, ClickEvent, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
    Styled, Window, div, px,
};
use kanvibe_domain::i18n::{Locale, Translator};

use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::TailwindText;

/// 번역하지 않는 배지 글자(TS도 하드코딩)
const NOT_FOUND_BADGE: &str = "404";
/// `max-w-md`
const CONTENT_MAX_WIDTH: f32 = 448.0;

pub fn render_not_found(
    theme: &ThemeTokens,
    locale: Locale,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let common = Translator::new(locale, "common");
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.background.page)
        .px(spacing(4.0))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(spacing(4.0))
                .max_w(px(CONTENT_MAX_WIDTH))
                .text_center()
                .child(render_badge(theme))
                .child(render_heading(theme, &common))
                .child(render_back_button(
                    theme,
                    common.translate("goBack"),
                    on_back,
                )),
        )
}

/// `rounded-full border bg-bg-surface px-3 py-1 text-xs font-medium text-text-muted`
fn render_badge(theme: &ThemeTokens) -> impl IntoElement {
    div()
        .rounded(radius::FULL)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .px(spacing(3.0))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .css_weight(500)
        .text_color(theme.text.muted)
        .child(NOT_FOUND_BADGE)
}

fn render_heading(theme: &ThemeTokens, common: &Translator) -> impl IntoElement {
    div()
        .child(
            div()
                .text_scale(text_scale::XL)
                .css_weight(600)
                .text_color(theme.text.primary)
                .child(common.translate("notFoundTitle")),
        )
        .child(
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.secondary)
                .child(common.translate("notFoundDescription")),
        )
}

/// `rounded-md border bg-bg-surface px-4 py-2 text-sm text-text-secondary`, 호버하면 brand 테두리와 primary 글자
fn render_back_button(
    theme: &ThemeTokens,
    label: String,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover_border = theme.brand.primary;
    let hover_text = theme.text.primary;
    div()
        .id("not-found-back")
        .relative()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .px(spacing(4.0))
        .py(spacing(2.0))
        .text_scale(text_scale::SM)
        .text_color(theme.text.secondary)
        .hover(move |style| style.border_color(hover_border).text_color(hover_text))
        .on_click(on_back)
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }))
}
