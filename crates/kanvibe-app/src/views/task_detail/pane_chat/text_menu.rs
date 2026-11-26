//! pane 채팅 안의 글자 우클릭 메뉴(TS `TerminalPaneChatLayer`가 채팅 칸에 건 `requestTextContextMenu`).
//! 입력창과 스킬 검색란은 그 입력란의 편집 메뉴를, 나머지 자리는 창에서 고른 글자의 복사 메뉴를 연다.
//! 메뉴는 채팅 칸 안의 우클릭에만 열리고, 터미널 칸의 우클릭은 터미널 뷰가 맡는다.

use gpui_kit::base::input::{InputBaseState, InputModeKind};
use gpui_kit::{AnyElement, Context, Entity, Focusable, MouseDownEvent, Pixels, Point, Window};

use super::view::PaneChatView;
use crate::components::text_context_menu::{
    OpenTextMenu, TextEditCommand, render_text_menu, run_text_edit_command,
};
use crate::theme::ThemeTokens;

/// 편집 메뉴를 여는 채팅 칸의 입력란
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChatTextField {
    Composer,
    SkillSearch,
}

impl PaneChatView {
    /// 입력란 밖 우클릭
    pub(super) fn open_text_menu_outside_fields(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.text_menu = Some(OpenTextMenu::outside_fields(event.position, window, cx));
        cx.notify();
    }

    /// 입력란 우클릭. 바깥 채팅 칸이 다시 메뉴를 열지 않게 전파를 멈춘다
    pub(super) fn open_text_menu_in_field(
        &mut self,
        field: ChatTextField,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let menu = match field {
            ChatTextField::Composer => field_menu(&self.composer.textarea, position, cx),
            ChatTextField::SkillSearch => field_menu(&self.composer.skill_search, position, cx),
        };
        self.text_menu = Some(menu);
        cx.notify();
    }

    pub(super) fn render_text_menu(
        &self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.text_menu.as_ref()?;
        Some(render_text_menu(
            menu,
            theme,
            &self.text_menu_focus,
            Self::run_text_menu_command,
            Self::close_text_menu,
            window,
            cx,
        ))
    }

    fn run_text_menu_command(
        &mut self,
        command: TextEditCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.text_menu.take() else {
            return;
        };
        run_text_edit_command(command, &menu, window, cx);
        cx.notify();
    }

    fn close_text_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = self.text_menu.take().and_then(|menu| menu.field) {
            field.focus(window, cx);
        }
        cx.notify();
    }
}

fn field_menu<M: InputModeKind + 'static>(
    field: &Entity<InputBaseState<M>>,
    position: Point<Pixels>,
    cx: &mut Context<PaneChatView>,
) -> OpenTextMenu
where
    InputBaseState<M>: Focusable,
{
    let state = field.read(cx);
    OpenTextMenu::in_field(position, state.focus_handle(cx), state.selected_range())
}
