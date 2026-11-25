//! Done 이동 안내(TS `DoneConfirmDialog.tsx`). "다시 묻지 않기"를 고르면 AppSettings `done_alert_dismissed`에 남긴다.
//! Esc와 취소는 이동을 버리고, 이동은 고른 카드를 Done으로 옮긴다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::i18n::Translator;

use super::BoardView;
use super::header::{qa_button_layer, qa_text_layer};
use super::icons::{BoardIcon, board_icon_sized};
use crate::components::TailwindText;
use crate::components::dialog::{
    DialogWidth, dialog_description, dialog_panel, dialog_title, modal_layer,
};
use crate::theme::{ThemeMode, ThemeTokens, radius, spacing, text_scale};

impl BoardView {
    pub(super) fn render_done_confirm(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.pending_done_move.as_ref()?;
        let labels = Translator::new(self.locale(), "common.doneAlert");
        if !self.dialog_focus.is_focused(window) {
            self.dialog_focus.focus(window, cx);
        }
        let is_checked = self.is_done_dont_ask_checked;
        let checkbox = div()
            .size(spacing(4.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.0))
            .border_1()
            .when_checked(is_checked, theme);
        let dont_ask = div()
            .id("done-dont-ask")
            .relative()
            .mb(spacing(5.0))
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.is_done_dont_ask_checked = !this.is_done_dont_ask_checked;
                cx.notify();
            }))
            .child(checkbox)
            .child(
                div()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(labels.translate("dontAskAgain")),
            )
            .child(qa_text_layer("checkbox", &labels.translate("dontAskAgain")));
        let panel = dialog_panel(theme, DialogWidth::Small)
            .id("done-confirm")
            .track_focus(&self.dialog_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.cancel_done_move(cx);
                }
            }))
            .child(
                dialog_title(theme, labels.translate("title"))
                    .relative()
                    .child(qa_text_layer("heading", &labels.translate("title"))),
            )
            .child(dialog_description(theme, labels.translate("message")))
            .child(dont_ask)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(spacing(2.0))
                    .child(self.done_dialog_button(
                        theme,
                        "done-cancel",
                        labels.translate("cancel"),
                        false,
                        cx,
                    ))
                    .child(self.done_dialog_button(
                        theme,
                        "done-confirm-move",
                        labels.translate("confirm"),
                        true,
                        cx,
                    )),
            );
        let viewport = self.overlay_viewport(window);
        Some(modal_layer(theme, viewport, panel).into_any_element())
    }

    fn done_dialog_button(
        &self,
        theme: &ThemeTokens,
        id: &'static str,
        label: String,
        is_primary: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let button = div()
            .id(id)
            .relative()
            .px(spacing(4.0))
            .py(px(6.0))
            .rounded(radius::MD)
            .text_scale(text_scale::SM)
            .cursor_pointer()
            .child(label.clone())
            .child(qa_button_layer(&label, &label));
        if is_primary {
            let hover = theme.brand.hover;
            button
                .bg(theme.brand.primary)
                .text_color(theme.text.inverse)
                .hover(move |style| style.bg(hover))
                .on_click(cx.listener(|this, _, _, cx| this.confirm_done_move(cx)))
        } else {
            let hover_border = theme.brand.primary;
            button
                .bg(theme.background.page)
                .border_1()
                .border_color(theme.border.default)
                .text_color(theme.text.secondary)
                .hover(move |style| style.border_color(hover_border))
                .on_click(cx.listener(|this, _, _, cx| this.cancel_done_move(cx)))
        }
    }
}

/// Chromium 기본 체크박스(`<input type="checkbox">`). 앱 토큰이 아니라 브라우저 색이다
trait CheckboxState: ParentElement + Styled + Sized {
    fn when_checked(self, is_checked: bool, theme: &ThemeTokens) -> Self {
        let (border, fill) = match theme.mode {
            ThemeMode::Dark => (gpui_kit::rgb(0x858585), gpui_kit::rgb(0x3b3b3b)),
            ThemeMode::Light => (gpui_kit::rgb(0x767676), gpui_kit::rgb(0xffffff)),
        };
        if is_checked {
            self.border_color(theme.brand.primary)
                .bg(theme.brand.primary)
                .child(board_icon_sized(
                    BoardIcon::Check,
                    theme.text.inverse,
                    10.0,
                    8.0,
                ))
        } else {
            self.border_color(border).bg(fill)
        }
    }
}

impl CheckboxState for gpui_kit::Div {}
