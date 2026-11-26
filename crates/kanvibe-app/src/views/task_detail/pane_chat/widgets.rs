//! pane 채팅 화면이 함께 쓰는 작은 조각. TS 각 파일의 `controlClass` 버튼과 선택지 번호 배지다.

use gpui_kit::{
    AnyElement, App, ClickEvent, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 누를 수 없으면 누르기를 받지 않는 버튼. 흐리게 그리는 것은 만든 쪽이 정한다
pub struct PaneChatButton {
    element: Stateful<Div>,
    is_disabled: bool,
}

impl PaneChatButton {
    pub fn new(element: Stateful<Div>, is_disabled: bool) -> Self {
        Self {
            element,
            is_disabled,
        }
    }

    pub fn on_click(
        self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        match self.is_disabled {
            true => self.element,
            false => self.element.cursor_pointer().on_click(listener),
        }
    }
}

impl IntoElement for PaneChatButton {
    type Element = <Stateful<Div> as IntoElement>::Element;

    fn into_element(self) -> Self::Element {
        self.element.into_element()
    }
}

/// 테두리 버튼(`min-h-9 rounded-md border border-border-default bg-bg-surface px-3 text-xs`). 높이는 화면마다 다르다
pub fn bordered_button(
    theme: &ThemeTokens,
    id: impl Into<SharedString>,
    label: String,
    min_height: f32,
    is_disabled: bool,
) -> PaneChatButton {
    let element = div()
        .id(id.into())
        .relative()
        .flex()
        .flex_none()
        .min_h(px(min_height))
        .items_center()
        .justify_center()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .px(spacing(3.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.primary)
        .child(aligned_text(
            label.clone(),
            text_scale::XS,
            TextFamily::Sans,
        ))
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }));
    let element = match is_disabled {
        true => element.opacity(0.4),
        false => element.hover(|style| style.bg(theme.background.page)),
    };
    PaneChatButton::new(element, is_disabled)
}

/// 선택 카드의 버튼(`min-h-9`)
pub fn control_button(
    theme: &ThemeTokens,
    id: String,
    label: String,
    is_disabled: bool,
) -> PaneChatButton {
    bordered_button(theme, id, label, 36.0, is_disabled)
}

/// 대화 화면의 버튼(`min-h-10`)
pub fn view_button(
    theme: &ThemeTokens,
    id: &'static str,
    label: String,
    is_disabled: bool,
) -> PaneChatButton {
    bordered_button(theme, id, label, 40.0, is_disabled)
}

/// 선택지 번호(`h-6 w-6 rounded bg-bg-page text-xs tabular-nums text-text-secondary`)
pub fn number_badge(theme: &ThemeTokens, number: u64) -> AnyElement {
    div()
        .flex()
        .size(px(24.0))
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(radius::BASE)
        .bg(theme.background.page)
        .text_scale(text_scale::XS)
        .text_color(theme.text.secondary)
        .child(number.to_string())
        .into_any_element()
}
