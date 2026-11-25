//! pane 오른쪽 위 `터미널/채팅 전환` 버튼(TS `TerminalPaneChatLayer`의 `pane-chat-toggle`:
//! `absolute right-2 top-1 h-9 rounded-md bg-bg-surface px-2 text-xs`). 그 버튼이 속한 pane 영역 안에 놓인다.

use gpui_kit::{
    AnyElement, App, ClickEvent, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use super::super::icons::{DetailIcon, detail_icon};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// `label`은 보이는 글, `pane_id`는 어느 pane의 버튼인지 알리는 이름 뒷부분이다
pub fn render_pane_chat_toggle(
    label: String,
    pane_id: &str,
    theme: &ThemeTokens,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id("pane-chat-toggle")
        .absolute()
        .right(spacing(2.0))
        .top(spacing(1.0))
        .h(spacing(9.0))
        .flex()
        .items_center()
        .gap(spacing(1.0))
        .rounded(radius::MD)
        .bg(theme.background.surface)
        .px(spacing(2.0))
        .text_size(text_scale::XS.font_size)
        .line_height(text_scale::XS.line_height)
        .text_color(theme.text.primary)
        .cursor_pointer()
        .occlude()
        .hover(|style| style.bg(theme.background.page))
        .child(detail_icon(
            DetailIcon::Chatting,
            theme.text.primary,
            px(16.0),
        ))
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: format!("{label} · {pane_id}").into(),
            text: label.into(),
        }))
        .on_click(on_click)
        .into_any_element()
}
