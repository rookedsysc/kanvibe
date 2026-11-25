//! 글자 우클릭 메뉴(TS `requestTextContextMenu`와 Electron main `kanvibe:text-context-menu` 메뉴).
//!
//! 우클릭한 자리가 글자 입력란이면 잘라내기·복사·붙여넣기를, 아니면 창에서 고른 글자의 복사만 보인다. 복사와 잘라내기는
//! 고른 글자가 있을 때만 쓸 수 있다. 입력란 명령은 그 입력란에 GPUI 편집 동작(`Cut`·`Copy`·`Paste`·`SelectAll`)으로 보내고,
//! 입력란 밖 복사는 창 글자 선택(`TextSelection`)을 클립보드에 쓴다. 메뉴를 어디서 열고 닫을지는 띄운 화면이 정한다.
//!
//! 입력란 밖 "모두 선택"은 두지 않는다. gpui-base의 창 글자 선택은 터치로 시작한 선택에만 모두 선택을 제공해
//! 마우스로 연 메뉴에서 창 글자 전체를 고를 방법이 없다.

use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, Point, StatefulInteractiveElement, Styled, Window,
    anchored, deferred, div, px,
};

use crate::components::context_menu::context_menu_item;
use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::components::{DISABLED_OPACITY, box_shadows};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing};

/// `min-w-[220px]`(앱 우클릭 메뉴와 같은 폭)
const MENU_MIN_WIDTH: f32 = 220.0;

/// TS `showTextContextMenu`에 보내던 요청
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextContextMenuRequest {
    pub is_editable: bool,
    pub has_selection: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEditCommand {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

impl TextEditCommand {
    /// Electron 기본 역할 이름(`role: "cut"` 등)
    pub fn label(self) -> &'static str {
        match self {
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::SelectAll => "Select All",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextMenuEntry {
    pub command: TextEditCommand,
    pub is_enabled: bool,
}

/// TS 메뉴 틀: 복사는 늘, 잘라내기·붙여넣기·모두 선택은 입력란에서만 보인다. 복사·잘라내기는 고른 글자가 있어야 쓴다
pub fn text_menu_entries(request: TextContextMenuRequest) -> Vec<TextMenuEntry> {
    let entry = |command, is_enabled| TextMenuEntry {
        command,
        is_enabled,
    };
    let copy = entry(TextEditCommand::Copy, request.has_selection);
    if !request.is_editable {
        return vec![copy];
    }
    vec![
        entry(TextEditCommand::Cut, request.has_selection),
        copy,
        entry(TextEditCommand::Paste, true),
        entry(TextEditCommand::SelectAll, true),
    ]
}

/// 열린 메뉴. `field`는 우클릭한 입력란의 포커스(입력란 밖이면 `None`)
#[derive(Debug, Clone)]
pub struct OpenTextMenu {
    pub position: Point<Pixels>,
    pub request: TextContextMenuRequest,
    pub field: Option<FocusHandle>,
    /// 입력란 밖에서 열 때 고른 글자. 메뉴 항목을 누르는 순간 창 글자 선택이 풀리므로 열 때 잡아 둔다
    window_selection: String,
}

impl OpenTextMenu {
    /// 입력란 밖 우클릭. 창에서 고른 글자가 있는지 본다
    pub fn outside_fields(position: Point<Pixels>, window: &mut Window, cx: &mut App) -> Self {
        let window_selection = gpui_kit::base::TextSelection::selected_text(window, cx);
        Self {
            position,
            request: TextContextMenuRequest {
                is_editable: false,
                has_selection: !window_selection.trim().is_empty(),
            },
            field: None,
            window_selection,
        }
    }

    /// 입력란 우클릭. 커서만 있으면 고른 글자가 없다
    pub fn in_field(
        position: Point<Pixels>,
        field: FocusHandle,
        selected_range: std::ops::Range<usize>,
    ) -> Self {
        Self {
            position,
            request: TextContextMenuRequest {
                is_editable: true,
                has_selection: !selected_range.is_empty(),
            },
            field: Some(field),
            window_selection: String::new(),
        }
    }
}

/// 고른 명령을 실행한다. 입력란 명령은 입력란에 포커스를 돌려준 뒤 보내고, 입력란 밖 복사는 열 때 고른 글자를 쓴다
pub fn run_text_edit_command(
    command: TextEditCommand,
    menu: &OpenTextMenu,
    window: &mut Window,
    cx: &mut App,
) {
    use gpui_kit::base::input::{Copy, Cut, Paste, SelectAll};
    let Some(field) = menu.field.as_ref() else {
        if command == TextEditCommand::Copy && menu.request.has_selection {
            cx.write_to_clipboard(ClipboardItem::new_string(menu.window_selection.clone()));
        }
        return;
    };
    field.focus(window, cx);
    // 메뉴를 띄운 화면이 갱신되는 도중이다. 입력란 동작이 그 화면(붙여넣기 가로채기 등)을 다시 부르도록 다음 차례에 보낸다
    let field = field.clone();
    window.defer(cx, move |window, cx| match command {
        TextEditCommand::Cut => field.dispatch_action(&Cut, window, cx),
        TextEditCommand::Copy => field.dispatch_action(&Copy, window, cx),
        TextEditCommand::Paste => field.dispatch_action(&Paste, window, cx),
        TextEditCommand::SelectAll => field.dispatch_action(&SelectAll, window, cx),
    });
}

/// 우클릭한 자리에 메뉴를 그린다. 메뉴 밖을 누르거나 Esc를 누르면 `on_dismiss`, 항목을 고르면 `on_command`를 부른다
pub fn render_text_menu<V: 'static>(
    menu: &OpenTextMenu,
    theme: &ThemeTokens,
    focus_handle: &FocusHandle,
    on_command: fn(&mut V, TextEditCommand, &mut Window, &mut Context<V>),
    on_dismiss: fn(&mut V, &mut Window, &mut Context<V>),
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    if !focus_handle.is_focused(window) {
        focus_handle.focus(window, cx);
    }
    let items: Vec<AnyElement> = text_menu_entries(menu.request)
        .into_iter()
        .map(|entry| menu_entry(entry, theme, on_command, cx))
        .collect();
    let panel = div()
        .id("text-context-menu")
        .track_focus(focus_handle)
        .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                cx.stop_propagation();
                on_dismiss(view, window, cx);
            }
        }))
        .on_mouse_down_out(cx.listener(move |view, _, window, cx| on_dismiss(view, window, cx)))
        .occlude()
        .relative()
        .min_w(px(MENU_MIN_WIDTH))
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .py(spacing(1.0))
        .shadow(box_shadows(&theme.shadow.lg))
        .children(items)
        .child(qa_target_layer(QaTargetDescription {
            role: "menu",
            name: "".into(),
            text: "".into(),
        }));
    deferred(
        anchored()
            .position(menu.position)
            .snap_to_window()
            .child(panel),
    )
    .with_priority(POPOVER_LAYER_PRIORITY)
    .into_any_element()
}

fn menu_entry<V: 'static>(
    entry: TextMenuEntry,
    theme: &ThemeTokens,
    on_command: fn(&mut V, TextEditCommand, &mut Window, &mut Context<V>),
    cx: &mut Context<V>,
) -> AnyElement {
    let command = entry.command;
    let item = context_menu_item(command.label(), theme, command.label(), false);
    if !entry.is_enabled {
        return item.opacity(DISABLED_OPACITY).into_any_element();
    }
    item.on_click(cx.listener(move |view, _, window, cx| on_command(view, command, window, cx)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands(request: TextContextMenuRequest) -> Vec<(TextEditCommand, bool)> {
        text_menu_entries(request)
            .into_iter()
            .map(|entry| (entry.command, entry.is_enabled))
            .collect()
    }

    // parity: TS-main-textContextMenuBridge-8d6a48
    #[test]
    fn copy_is_always_offered_and_cut_and_paste_only_inside_a_field() {
        let outside = TextContextMenuRequest {
            is_editable: false,
            has_selection: true,
        };
        let field_with_caret = TextContextMenuRequest {
            is_editable: true,
            has_selection: false,
        };

        assert_eq!(commands(outside), [(TextEditCommand::Copy, true)]);
        assert_eq!(
            commands(field_with_caret),
            [
                (TextEditCommand::Cut, false),
                (TextEditCommand::Copy, false),
                (TextEditCommand::Paste, true),
                (TextEditCommand::SelectAll, true),
            ]
        );
    }
}
