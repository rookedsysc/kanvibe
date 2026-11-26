//! Done 이동 안내(TS `DoneConfirmDialog.tsx`). 보드 카드 이동과 작업 상세의 Done 버튼(TS `DoneStatusButton`)이 함께 띄운다.
//! "다시 묻지 않기"를 고르면 화면이 AppSettings `done_alert_dismissed`에 남긴다. Esc와 취소는 이동을 버리고, 이동은 Done으로 옮긴다.
//! 열림·체크 상태와 버튼이 할 일은 띄운 화면이 갖고, 여기서는 받은 대로 그리기만 한다.

use gpui_kit::{
    AnyElement, Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Pixels, Size, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::i18n::{Locale, Translator};

use crate::components::TailwindText;
use crate::components::dialog::{
    DialogWidth, dialog_description, dialog_panel, dialog_title, modal_layer,
};
use crate::qa::targets::{qa_button_layer, qa_text_layer};
use crate::theme::{ThemeMode, ThemeTokens, radius, spacing, text_scale};
use crate::views::board::icons::{BoardIcon, board_icon_sized};

/// 안내를 띄운 화면 `V`의 상태와 버튼이 부를 일
pub struct DoneConfirmDialog<V> {
    pub locale: Locale,
    pub is_dont_ask_checked: bool,
    /// 덮개가 가릴 영역(보드는 페이지 스크롤바 폭을 뺀다)
    pub viewport: Size<Pixels>,
    pub on_toggle_dont_ask: fn(&mut V, &mut Context<V>),
    pub on_confirm: fn(&mut V, &mut Window, &mut Context<V>),
    pub on_cancel: fn(&mut V, &mut Window, &mut Context<V>),
}

/// 안내를 그리고 포커스를 안내로 옮긴다. Esc는 취소와 같다
pub fn render_done_confirm<V: 'static>(
    dialog: DoneConfirmDialog<V>,
    theme: &ThemeTokens,
    focus_handle: &FocusHandle,
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    let labels = Translator::new(dialog.locale, "common.doneAlert");
    if !focus_handle.is_focused(window) {
        focus_handle.focus(window, cx);
    }
    let on_cancel = dialog.on_cancel;
    let panel = dialog_panel(theme, DialogWidth::Small)
        .id("done-confirm")
        .track_focus(focus_handle)
        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                on_cancel(view, window, cx);
            }
        }))
        .child(
            dialog_title(theme, labels.translate("title"))
                .relative()
                .child(qa_text_layer("heading", &labels.translate("title"))),
        )
        .child(dialog_description(theme, labels.translate("message")))
        .child(dont_ask_checkbox(&dialog, &labels, theme, cx))
        .child(
            div()
                .flex()
                .justify_end()
                .gap(spacing(2.0))
                .child(dialog_button(
                    theme,
                    "done-cancel",
                    labels.translate("cancel"),
                    false,
                    dialog.on_cancel,
                    cx,
                ))
                .child(dialog_button(
                    theme,
                    "done-confirm-move",
                    labels.translate("confirm"),
                    true,
                    dialog.on_confirm,
                    cx,
                )),
        );
    modal_layer(theme, dialog.viewport, panel).into_any_element()
}

fn dont_ask_checkbox<V: 'static>(
    dialog: &DoneConfirmDialog<V>,
    labels: &Translator,
    theme: &ThemeTokens,
    cx: &mut Context<V>,
) -> impl IntoElement + use<V> {
    let checkbox = div()
        .size(spacing(4.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.0))
        .border_1()
        .when_checked(dialog.is_dont_ask_checked, theme);
    let on_toggle = dialog.on_toggle_dont_ask;
    div()
        .id("done-dont-ask")
        .relative()
        .mb(spacing(5.0))
        .flex()
        .items_center()
        .gap(spacing(2.0))
        .cursor_pointer()
        .on_click(cx.listener(move |view, _, _, cx| on_toggle(view, cx)))
        .child(checkbox)
        .child(
            div()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(labels.translate("dontAskAgain")),
        )
        .child(qa_text_layer("checkbox", &labels.translate("dontAskAgain")))
}

fn dialog_button<V: 'static>(
    theme: &ThemeTokens,
    id: &'static str,
    label: String,
    is_primary: bool,
    on_click: fn(&mut V, &mut Window, &mut Context<V>),
    cx: &mut Context<V>,
) -> impl IntoElement + use<V> {
    let button = div()
        .id(id)
        .relative()
        .px(spacing(4.0))
        .py(px(6.0))
        .rounded(radius::MD)
        .text_scale(text_scale::SM)
        .cursor_pointer()
        .child(label.clone())
        .child(qa_button_layer(&label, &label))
        .on_click(cx.listener(move |view, _, window, cx| on_click(view, window, cx)));
    if is_primary {
        let hover = theme.brand.hover;
        button
            .bg(theme.brand.primary)
            .text_color(theme.text.inverse)
            .hover(move |style| style.bg(hover))
    } else {
        let hover_border = theme.brand.primary;
        button
            .bg(theme.background.page)
            .border_1()
            .border_color(theme.border.default)
            .text_color(theme.text.secondary)
            .hover(move |style| style.border_color(hover_border))
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
