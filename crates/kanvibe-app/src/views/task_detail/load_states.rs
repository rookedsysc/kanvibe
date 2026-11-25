//! 데이터가 없을 때의 화면: 로딩(+지연 안내), 조회 실패(사유 + 다시 시도), 작업 없음, 터미널 없음.

use gpui_kit::{
    App, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::i18n::Translator;

use super::TaskDetailView;
use crate::components::{TailwindText, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// `min-h-screen flex flex-col items-center justify-center gap-3 bg-bg-page px-4 text-text-muted`
fn centered_page(theme: &ThemeTokens) -> gpui_kit::Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing(3.0))
        .bg(theme.background.page)
        .px(spacing(4.0))
        .text_color(theme.text.muted)
}

pub fn render_loading(
    theme: &ThemeTokens,
    common: &Translator,
    is_delayed: bool,
) -> impl IntoElement {
    centered_page(theme)
        .child(
            div()
                .relative()
                .child("Loading...")
                .child(status_target("Loading...".to_string())),
        )
        .children(is_delayed.then(|| {
            let notice = common.translate("loadDelayed");
            div()
                .relative()
                .text_scale(text_scale::XS)
                .child(notice.clone())
                .child(status_target(notice))
        }))
}

pub fn render_load_failure(
    theme: &ThemeTokens,
    common: &Translator,
    reason: &str,
    cx: &mut Context<TaskDetailView>,
) -> impl IntoElement {
    let message = common.translate_with("loadFailed", &[("reason", reason)]);
    centered_page(theme).child(
        render_inline_load_error(theme, message, common.translate("retry"))
            .w_full()
            .max_w(px(448.0))
            .on_click_retry(cx.listener(|view, _: &ClickEvent, _, cx| view.retry_load(cx))),
    )
}

/// TS `InlineLoadError`: `flex items-start justify-between gap-3 rounded-md border border-status-error/20 bg-bg-page px-3 py-2 text-xs text-status-error`
pub struct InlineLoadError {
    container: gpui_kit::Div,
    retry: gpui_kit::Stateful<gpui_kit::Div>,
}

pub fn render_inline_load_error(
    theme: &ThemeTokens,
    message: String,
    retry_label: String,
) -> InlineLoadError {
    let container = div()
        .flex()
        .items_start()
        .justify_between()
        .gap(spacing(3.0))
        .rounded(radius::MD)
        .border_1()
        .border_color(fade(theme.status.error, 0.2))
        .bg(theme.background.page)
        .px(spacing(3.0))
        .py(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(theme.status.error)
        .child(
            div()
                .relative()
                .min_w_0()
                .child(message.clone())
                .child(qa_target_layer(QaTargetDescription {
                    role: "alert",
                    name: message.clone().into(),
                    text: message.into(),
                })),
        );
    let retry = div()
        .id("inline-load-error-retry")
        .relative()
        .flex_none()
        .rounded(radius::BASE)
        .px(spacing(1.5))
        .py(spacing(0.5))
        .css_weight(500)
        .cursor_pointer()
        .child(retry_label.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: retry_label.clone().into(),
            text: retry_label.into(),
        }));
    InlineLoadError { container, retry }
}

impl InlineLoadError {
    pub fn w_full(mut self) -> Self {
        self.container = self.container.w_full();
        self
    }

    pub fn max_w(mut self, width: gpui_kit::Pixels) -> Self {
        self.container = self.container.max_w(width);
        self
    }

    pub fn w(mut self, width: gpui_kit::Pixels) -> Self {
        self.container = self.container.w(width);
        self
    }

    pub fn shadow(mut self, shadows: Vec<gpui_kit::BoxShadow>) -> Self {
        self.container = self.container.shadow(shadows);
        self
    }

    pub fn on_click_retry(
        self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> gpui_kit::Div {
        self.container.child(self.retry.on_click(handler))
    }
}

/// `taskNotFound` + 뒤로 버튼(`rounded-md border bg-bg-surface px-4 py-2 text-sm text-text-secondary`)
pub fn render_not_found(
    theme: &ThemeTokens,
    translator: &Translator,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = translator.translate("goBack");
    let hover_border = theme.brand.primary;
    let hover_text = theme.text.primary;
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
                .child(not_found_message(
                    theme,
                    translator.translate("taskNotFound"),
                ))
                .child(
                    div()
                        .id("task-not-found-back")
                        .relative()
                        .rounded(radius::MD)
                        .border_1()
                        .border_color(theme.border.default)
                        .bg(theme.background.surface)
                        .px(spacing(4.0))
                        .py(spacing(2.0))
                        .text_scale(text_scale::SM)
                        .text_color(theme.text.secondary)
                        .cursor_pointer()
                        .hover(move |style| style.border_color(hover_border).text_color(hover_text))
                        .on_click(on_back)
                        .child(label.clone())
                        .child(qa_target_layer(QaTargetDescription {
                            role: "button",
                            name: label.clone().into(),
                            text: label.into(),
                        })),
                ),
        )
}

/// 프로젝트 없는 작업(`noTerminal`). 세션 없는 프로젝트 작업은 자동 연결 화면이 대신한다
pub fn render_no_terminal(theme: &ThemeTokens, translator: &Translator) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::LG)
        .border_1()
        .border_dashed()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .child(
            div()
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(translator.translate("noTerminal")),
        )
}

/// terminal 세션의 탭 목록 대기·실패·빈 목록(TS `TerminalLoader`)
pub fn render_terminal_loader(
    theme: &ThemeTokens,
    translator: &Translator,
    common: &Translator,
    tabs: &super::tabs::TerminalTabsState,
    cx: &mut Context<TaskDetailView>,
) -> gpui_kit::AnyElement {
    let frame = div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p(spacing(6.0));
    if let Some(error) = &tabs.error {
        let message = common.translate_with("loadFailed", &[("reason", error)]);
        return frame
            .child(
                render_inline_load_error(theme, message, common.translate("retry"))
                    .w_full()
                    .max_w(px(448.0))
                    .on_click_retry(
                        cx.listener(|view, _: &ClickEvent, _, cx| view.refresh_tabs(cx)),
                    ),
            )
            .into_any_element();
    }
    if !tabs.is_list_loaded {
        let loading = common.translate("loading");
        return frame
            .child(
                div()
                    .relative()
                    .text_scale(text_scale::XS)
                    .text_color(theme.terminal.text)
                    .opacity(0.7)
                    .child(loading.clone())
                    .child(status_target(loading)),
            )
            .into_any_element();
    }
    frame
        .child(new_tab_button(
            theme,
            translator.translate("newTerminalTab"),
            cx,
        ))
        .into_any_element()
}

/// 탭이 하나도 없을 때의 `rounded-md border border-white/10 px-3 py-1.5 text-xs` 새 탭 버튼
fn new_tab_button(
    theme: &ThemeTokens,
    label: String,
    cx: &mut Context<TaskDetailView>,
) -> impl IntoElement {
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
        .on_click(
            cx.listener(|view, _, _, cx| view.run_tab_command(super::tabs::TabCommand::Create, cx)),
        )
}

fn not_found_message(theme: &ThemeTokens, message: String) -> impl IntoElement {
    div()
        .relative()
        .text_color(theme.text.muted)
        .child(message.clone())
        .child(status_target(message))
}

/// TS `role="status"` 문구를 재생 드라이버와 시험이 찾을 수 있게 남긴다
fn status_target(text: String) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role: "status",
        name: text.clone().into(),
        text: text.into(),
    })
}
