//! terminal 세션에 붙을 탭이 없을 때의 터미널 자리(TS `TerminalLoader`): 첫 목록을 기다리는 중, 목록을 읽지 못함, 탭이 하나도 없음.

use gpui_kit::{
    AnyElement, App, ClickEvent, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use super::{TabCommand, TerminalTabsController};
use crate::components::{TailwindText, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::task_detail::load_states::render_inline_load_error;

/// 실패면 사유와 다시 시도, 첫 목록 전이면 읽는 중 안내, 그 밖에는 새 탭 버튼
pub fn render_terminal_loader(
    controller: &Entity<TerminalTabsController>,
    theme: &ThemeTokens,
    cx: &App,
) -> AnyElement {
    let tabs = controller.read(cx);
    let frame = div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(spacing(6.0));
    if let Some(error) = &tabs.state.error {
        let message = tabs
            .common
            .translate_with("loadFailed", &[("reason", error)]);
        let retry_target = controller.downgrade();
        return frame
            .child(
                render_inline_load_error(theme, message, tabs.common.translate("retry"))
                    .w_full()
                    .max_w(px(448.0))
                    .on_click_retry(move |_: &ClickEvent, _, cx| {
                        let _ = retry_target.update(cx, |tabs, cx| tabs.refresh(cx));
                    }),
            )
            .into_any_element();
    }
    if !tabs.state.is_list_loaded {
        let loading = tabs.common.translate("loading");
        return frame
            .child(
                div()
                    .relative()
                    .text_scale(text_scale::XS)
                    .text_color(theme.terminal.text)
                    .opacity(0.7)
                    .child(loading.clone())
                    .child(qa_target_layer(QaTargetDescription {
                        role: "status",
                        name: loading.clone().into(),
                        text: loading.into(),
                    })),
            )
            .into_any_element();
    }
    let label = tabs.translator.translate("newTerminalTab");
    frame
        .child(new_tab_button(controller, theme, label))
        .into_any_element()
}

/// 탭이 하나도 없을 때의 `rounded-md border border-white/10 px-3 py-1.5 text-xs` 새 탭 버튼
fn new_tab_button(
    controller: &Entity<TerminalTabsController>,
    theme: &ThemeTokens,
    label: String,
) -> impl IntoElement {
    let create_target = controller.downgrade();
    div()
        .id("terminal-loader-new-tab")
        .relative()
        .rounded(radius::MD)
        .border_1()
        .border_color(fade(gpui_kit::rgb(0xffffff), 0.1))
        .px(spacing(3.0))
        .py(spacing(1.5))
        .text_scale(text_scale::XS)
        .text_color(theme.terminal.text)
        .cursor_pointer()
        .child(label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.clone().into(),
            text: label.into(),
        }))
        .on_click(move |_, _, cx| {
            let _ = create_target.update(cx, |tabs, cx| tabs.run_command(TabCommand::Create, cx));
        })
}
