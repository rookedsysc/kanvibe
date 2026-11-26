//! 보드의 Done 이동 안내. 공용 안내(`components::done_confirm`)에 보드의 확인·취소를 잇는다.

use gpui_kit::{AnyElement, Context, Window};

use super::BoardView;
use crate::components::done_confirm::{DoneConfirmDialog, render_done_confirm};
use crate::theme::ThemeTokens;

impl BoardView {
    pub(super) fn render_done_confirm(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        self.pending_done_move.as_ref()?;
        let dialog: DoneConfirmDialog<Self> = DoneConfirmDialog {
            locale: self.locale(),
            is_dont_ask_checked: self.is_done_dont_ask_checked,
            viewport: self.overlay_viewport(window),
            on_toggle_dont_ask: |board, cx| {
                board.is_done_dont_ask_checked = !board.is_done_dont_ask_checked;
                cx.notify();
            },
            on_confirm: |board, _, cx| board.confirm_done_move(cx),
            on_cancel: |board, _, cx| board.cancel_done_move(cx),
        };
        let focus_handle = self.dialog_focus.clone();
        Some(render_done_confirm(
            dialog,
            theme,
            &focus_handle,
            window,
            cx,
        ))
    }
}
