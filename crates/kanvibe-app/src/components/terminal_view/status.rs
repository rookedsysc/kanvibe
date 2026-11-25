//! 터미널 연결 상태 겹층(TS `Terminal.tsx`의 연결 중·재연결·오류 화면)과 종료 사유 줄.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_domain::i18n::Translator;

use super::pane::TerminalPane;
use crate::components::TailwindText;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalConnection {
    Connecting,
    /// SSH 전송 쿨다운이 끝나기를 기다렸다가 한 번 다시 연다
    Reconnecting,
    Ready,
    Failed(String),
}

/// 연결 중이면 진행 문구, 실패면 사유와 다시 연결 버튼. 연결되면 아무것도 덮지 않는다
pub fn render_connection_overlay(
    connection: &TerminalConnection,
    translator: &Translator,
    cx: &mut Context<TerminalPane>,
) -> Option<AnyElement> {
    let theme = *cx.global::<ThemeTokens>();
    let overlay = div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing(3.0))
        .px(spacing(4.0))
        .bg(theme.terminal.background);
    match connection {
        TerminalConnection::Ready => None,
        TerminalConnection::Connecting | TerminalConnection::Reconnecting => {
            let key = match connection {
                TerminalConnection::Reconnecting => "terminalReconnecting",
                _ => "terminalConnecting",
            };
            Some(
                overlay
                    .child(connecting_status(&theme, translator.translate(key)))
                    .into_any_element(),
            )
        }
        TerminalConnection::Failed(message) => Some(
            overlay
                .child(failure_message(&theme, message.clone()))
                .child(retry_button(
                    &theme,
                    translator.translate("terminalRetry"),
                    cx,
                ))
                .into_any_element(),
        ),
    }
}

/// `role="alert"` 연결 실패 사유
fn failure_message(theme: &ThemeTokens, message: String) -> impl IntoElement {
    div()
        .relative()
        .text_scale(text_scale::SM)
        .text_color(theme.status.error)
        .child(message.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "alert",
            name: message.clone().into(),
            text: message.into(),
        }))
}

fn connecting_status(theme: &ThemeTokens, label: String) -> impl IntoElement {
    div()
        .relative()
        .flex()
        .items_center()
        .gap(spacing(2.0))
        .text_scale(text_scale::SM)
        .text_color(theme.text.muted)
        .child(
            div()
                .size(px(16.0))
                .rounded_full()
                .border_2()
                .border_color(theme.text.muted),
        )
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "status",
            name: label.clone().into(),
            text: label.into(),
        }))
}

fn retry_button(
    theme: &ThemeTokens,
    label: String,
    cx: &mut Context<TerminalPane>,
) -> impl IntoElement {
    div()
        .id("terminal-retry")
        .relative()
        .px(spacing(3.0))
        .py(spacing(1.5))
        .rounded(radius::BASE)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.button_neutral.base)
        .text_scale(text_scale::XS)
        .text_color(theme.text.primary)
        .cursor_pointer()
        .on_click(cx.listener(|pane, _, _, cx| pane.reconnect(cx)))
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }))
}

/// 연결이 끝났거나 원격 이미지 전송이 실패한 사유. TS는 xterm에 빨간 줄로 썼다(터미널 줄과 같은 대상으로 남긴다)
pub fn render_closed_reason(reason: String) -> AnyElement {
    div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .px(spacing(2.0))
        .py(spacing(1.0))
        .mono(400)
        .text_size(px(14.0))
        .text_color(gpui_kit::rgb(0xcc0000))
        .bg(gpui_kit::rgb(kanvibe_terminal::screen::TERMINAL_BACKGROUND))
        .child(reason.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "row",
            name: reason.clone().into(),
            text: reason.into(),
        }))
        .into_any_element()
}
