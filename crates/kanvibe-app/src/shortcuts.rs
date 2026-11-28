//! 단축키 디스패치(계획 AD-10). 명령 정의와 판정은 `kanvibe_domain::shortcuts`가 갖고, 여기서는 GPUI 키 입력을
//! 그 판정에 넣어 GPUI `Action`으로 바꾼다.
//!
//! TS 앱은 Electron `before-input-event`가 터미널보다 먼저 일부 명령을 가로챘다. GPUI에서는 키 바인딩·포커스된 터미널보다
//! 먼저 도는 `App::intercept_keystrokes`가 그 자리다. 그래서 dock·탭·사용량 단축키는 터미널 키 컨텍스트에
//! 같은 조합의 바인딩이 있어도 앞선다. 사용자가 바꾼 조합은 AppSettings `shortcut_bindings`에서 읽는다.

use std::collections::HashSet;

use gpui_kit::{
    Action, App, Global, KeyContext, Keystroke, KeystrokeEvent, Subscription, Window, WindowId,
    actions,
};
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_domain::shortcuts::bindings::{
    ShortcutBindings, ShortcutCommandId, resolve_terminal_tab_command,
};
use kanvibe_domain::shortcuts::key_event::{
    ModifierState, ShortcutKeyEvent, is_blocked_shortcut_event,
};
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;

use crate::services::AppServices;

actions!(
    kanvibe,
    [
        /// 작업 상세 AI 사용량 패널 열기·닫기
        ToggleTaskDetailUsage,
        /// 작업 빠른 검색
        OpenTaskSearch,
        /// 알림 센터 열기·닫기
        ToggleNotificationCenter,
        /// 보드 프로젝트 필터 열기
        OpenProjectFilter,
        /// 보드 데스크톱 필터 열기
        OpenDesktopFilter,
        /// 커맨드 팔레트
        OpenCommandPalette,
        /// 보드 찾기 바
        OpenBoardPageFind,
        /// 작업 생성 모달
        OpenCreateTask,
        /// 지금 경로로 새 창 열기
        OpenNewWindow,
        /// 뒤로
        NavigateBack,
        /// 앞으로
        NavigateForward,
        /// 현재 pane 터미널·채팅 전환
        ToggleTerminalChat,
    ]
);

/// 작업 상세 dock n번 항목 열기(`Mod+{n}`)
#[derive(Debug, Clone, PartialEq, gpui_kit::Action)]
#[action(namespace = kanvibe, no_json)]
pub struct OpenTaskDetailDockItem {
    pub position: u8,
}

/// 터미널보다 먼저 가로챈 탭·창 명령
#[derive(Debug, Clone, PartialEq, gpui_kit::Action)]
#[action(namespace = kanvibe, no_json)]
pub struct RunTerminalTabCommand {
    pub command: TerminalTabShortcutCommand,
}

/// 터미널 뷰가 거는 키 컨텍스트. 앱 단축키 가로채기가 이 컨텍스트보다 먼저 돈다
pub const TERMINAL_KEY_CONTEXT: &str = "Terminal";
/// 작업 상세 화면이 거는 키 컨텍스트. TS가 URL로 하던 "작업 상세인가" 판정을 포커스 경로의 컨텍스트로 한다
pub const TASK_DETAIL_KEY_CONTEXT: &str = "TaskDetail";
/// diff 화면 파일 편집기가 거는 키 컨텍스트
pub const DIFF_FILE_EDITOR_KEY_CONTEXT: &str = "DiffFileEditor";
/// diff 화면 찾기 상자가 거는 키 컨텍스트
pub const FIND_WIDGET_KEY_CONTEXT: &str = "FindWidget";
/// 보드 한 줄 입력란이 거는 키 컨텍스트
pub const BOARD_TEXT_FIELD_KEY_CONTEXT: &str = "BoardTextField";
/// gpui-component `InputState`가 거는 키 컨텍스트
const COMPONENT_INPUT_KEY_CONTEXT: &str = "Input";
/// 글을 입력받는 요소의 키 컨텍스트. TS `shouldIgnoreGlobalShortcut`이 거르던 input·textarea(xterm 입력 textarea 포함)에 해당한다
const TEXT_ENTRY_KEY_CONTEXTS: [&str; 5] = [
    COMPONENT_INPUT_KEY_CONTEXT,
    BOARD_TEXT_FIELD_KEY_CONTEXT,
    DIFF_FILE_EDITOR_KEY_CONTEXT,
    FIND_WIDGET_KEY_CONTEXT,
    TERMINAL_KEY_CONTEXT,
];

/// 앱 전역 단축키 상태(AppSettings가 원천). 모든 창이 이 한 벌을 읽으므로 재배정은 열린 창 전부에 함께 적용된다
pub struct ShortcutState {
    pub bindings: ShortcutBindings,
    pub platform: ShortcutPlatform,
    /// 단축키 설정 화면이 조합을 녹화하는 중인 창. 그 창의 입력은 명령으로 해석하지 않는다(TS는 webContents id 집합)
    pub capturing_windows: HashSet<WindowId>,
}

impl Global for ShortcutState {}

/// 단축키 설정 화면이 녹화를 시작할 때 부른다
pub fn begin_shortcut_capture(window_id: WindowId, cx: &mut App) {
    if cx.has_global::<ShortcutState>() {
        cx.global_mut::<ShortcutState>()
            .capturing_windows
            .insert(window_id);
    }
}

/// 녹화를 끝내거나, 녹화하던 창이 닫히거나 다른 경로로 바뀔 때 부른다.
/// 표시가 남으면 그 창은 앱 단축키 가로채기를 통째로 잃는다
pub fn end_shortcut_capture(window_id: WindowId, cx: &mut App) {
    if cx.has_global::<ShortcutState>() {
        cx.global_mut::<ShortcutState>()
            .capturing_windows
            .remove(&window_id);
    }
}

/// 단축키 설정을 저장한 뒤 부른다. AppSettings `shortcut_bindings`를 다시 읽어 전역 상태를 바꾸고 열린 창을 모두 다시 그린다
pub fn reload_shortcut_bindings(cx: &mut App) {
    let bindings = cx.global::<AppServices>().shortcut_bindings();
    if cx.has_global::<ShortcutState>() {
        cx.global_mut::<ShortcutState>().bindings = bindings;
    }
    cx.refresh_windows();
}

/// 가로챈 입력을 어떻게 다룰지. TS `before-input-event` 분기를 그대로 옮겼다
#[derive(Debug)]
pub enum ShortcutRouting {
    /// 터미널·키 바인딩보다 먼저 이 동작을 실행하고 키를 삼킨다
    Intercept(Box<dyn Action>),
    /// 포커스 경로가 이 동작을 받을 때만 실행한다(렌더러 keydown 처리에 해당)
    WhenHandled(Box<dyn Action>),
    /// 앱이 관여하지 않는다
    PassThrough,
}

impl PartialEq for ShortcutRouting {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Intercept(left), Self::Intercept(right))
            | (Self::WhenHandled(left), Self::WhenHandled(right)) => {
                left.partial_eq(right.as_ref())
            }
            (Self::PassThrough, Self::PassThrough) => true,
            _ => false,
        }
    }
}

/// GPUI 키 입력을 도메인 판정이 읽는 브라우저 `KeyboardEvent.key` 모양으로 바꾼다
pub fn shortcut_event_from_keystroke(keystroke: &Keystroke) -> ShortcutKeyEvent {
    // AppKit은 한글 두벌식 W를 ㅈ/ㅉ으로 보낼 수 있다. GPUI가 ㅉ에 Shift를 남기지 않아도 복원한다.
    let has_shifted_korean_w = keystroke.modifiers.platform && keystroke.key == "ㅉ";
    let key = match keystroke.key.as_str() {
        "ㅈ" | "ㅉ" if keystroke.modifiers.platform => "w",
        "escape" => "Escape",
        "enter" => "Enter",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "space" => " ",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        other => other,
    };
    ShortcutKeyEvent {
        key: key.to_string(),
        modifiers: ModifierState {
            has_meta: keystroke.modifiers.platform,
            has_ctrl: keystroke.modifiers.control,
            has_alt: keystroke.modifiers.alt,
            has_shift: keystroke.modifiers.shift || has_shifted_korean_w,
        },
    }
}

/// 명령과 화면으로 처리 방식을 정한다
pub fn route_shortcut_command(
    command: ShortcutCommandId,
    is_task_detail_route: bool,
) -> ShortcutRouting {
    use ShortcutCommandId as Id;
    let always: Option<Box<dyn Action>> = match command {
        Id::BoardNotification => Some(Box::new(ToggleNotificationCenter)),
        Id::CreateTask => Some(Box::new(OpenCreateTask)),
        Id::CommandPalette => Some(Box::new(OpenCommandPalette)),
        Id::NewWindow => Some(Box::new(OpenNewWindow)),
        _ => None,
    };
    if let Some(action) = always {
        return ShortcutRouting::Intercept(action);
    }
    if let Some(action) = task_detail_only_action(command).filter(|_| is_task_detail_route) {
        return ShortcutRouting::Intercept(action);
    }
    if let Some(tab_command) = resolve_terminal_tab_command(Some(command), is_task_detail_route) {
        return ShortcutRouting::Intercept(Box::new(RunTerminalTabCommand {
            command: tab_command,
        }));
    }
    renderer_action(command)
        .map(ShortcutRouting::WhenHandled)
        .unwrap_or(ShortcutRouting::PassThrough)
}

/// 작업 상세 밖에서 글 입력 요소에 포커스가 있으면 뒤로/앞으로 단축키를 그 요소에 넘긴다(TS `BoardCommandProvider`).
/// 작업 상세는 TS 캡처 단계 처리처럼 입력 중에도 이동한다
fn should_leave_history_shortcut_to_text_entry(
    command: ShortcutCommandId,
    is_task_detail_route: bool,
    context_stack: &[KeyContext],
) -> bool {
    let is_history_command = matches!(
        command,
        ShortcutCommandId::PageBack | ShortcutCommandId::PageForward
    );
    let is_text_entry_focused = context_stack.iter().any(|context| {
        TEXT_ENTRY_KEY_CONTEXTS
            .iter()
            .any(|text_entry_context| context.contains(text_entry_context))
    });
    is_history_command && !is_task_detail_route && is_text_entry_focused
}

/// 작업 상세에서만 터미널보다 앞서는 명령. 다른 화면에서 숫자 키를 삼키지 않게 화면을 먼저 본다
fn task_detail_only_action(command: ShortcutCommandId) -> Option<Box<dyn Action>> {
    if let Some(position) = command.task_detail_dock_index() {
        return Some(Box::new(OpenTaskDetailDockItem { position }));
    }
    match command {
        ShortcutCommandId::TerminalToggleChat => Some(Box::new(ToggleTerminalChat)),
        ShortcutCommandId::TaskDetailUsage => Some(Box::new(ToggleTaskDetailUsage)),
        _ => None,
    }
}

/// TS 렌더러 keydown 리스너가 처리하던 명령
fn renderer_action(command: ShortcutCommandId) -> Option<Box<dyn Action>> {
    use ShortcutCommandId as Id;
    let action: Box<dyn Action> = match command {
        Id::TaskSearch => Box::new(OpenTaskSearch),
        Id::BoardProjectFilter => Box::new(OpenProjectFilter),
        Id::BoardDesktopFilter => Box::new(OpenDesktopFilter),
        Id::BoardPageFind => Box::new(OpenBoardPageFind),
        Id::PageBack => Box::new(NavigateBack),
        Id::PageForward => Box::new(NavigateForward),
        _ => return None,
    };
    Some(action)
}

/// 앱 전역 키 가로채기를 건다. 반환값을 앱 수명 동안 붙잡아 두어야 한다
pub fn install_shortcut_dispatch(cx: &mut App) -> Subscription {
    cx.intercept_keystrokes(handle_intercepted_keystroke)
}

fn handle_intercepted_keystroke(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    let Some(state) = cx.try_global::<ShortcutState>() else {
        return;
    };
    let key_event = shortcut_event_from_keystroke(&event.keystroke);
    if is_blocked_shortcut_event(&key_event, state.platform) {
        cx.stop_propagation();
        return;
    }
    if state
        .capturing_windows
        .contains(&window.window_handle().window_id())
    {
        return;
    }
    let Some(command) = state
        .bindings
        .find_command_for_event(&key_event, state.platform)
    else {
        return;
    };
    let is_task_detail_route = event
        .context_stack
        .iter()
        .any(|context| context.contains(TASK_DETAIL_KEY_CONTEXT));
    if should_leave_history_shortcut_to_text_entry(
        command,
        is_task_detail_route,
        &event.context_stack,
    ) {
        return;
    }
    match route_shortcut_command(command, is_task_detail_route) {
        ShortcutRouting::Intercept(action) => {
            window.dispatch_action(action, cx);
            cx.stop_propagation();
        }
        ShortcutRouting::WhenHandled(action) if window.is_action_available(action.as_ref(), cx) => {
            window.dispatch_action(action, cx);
            cx.stop_propagation();
        }
        ShortcutRouting::WhenHandled(_) | ShortcutRouting::PassThrough => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keystroke(text: &'static str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    #[test]
    fn gpui_keystrokes_become_browser_style_key_events() {
        let event = shortcut_event_from_keystroke(&keystroke("ctrl-shift-["));
        assert_eq!(event.key, "[");
        assert!(event.modifiers.has_ctrl && event.modifiers.has_shift && !event.modifiers.has_meta);
        assert_eq!(
            shortcut_event_from_keystroke(&keystroke("escape")).key,
            "Escape"
        );
    }

    #[test]
    fn the_native_korean_w_mapping_preserves_non_command_input() {
        let shifted = shortcut_event_from_keystroke(&keystroke("cmd-ㅉ"));
        assert_eq!(shifted.key, "w");
        assert!(shifted.modifiers.has_shift);

        let unshifted = shortcut_event_from_keystroke(&keystroke("cmd-ㅈ"));
        assert_eq!(unshifted.key, "w");
        assert!(!unshifted.modifiers.has_shift);

        let typed = shortcut_event_from_keystroke(&keystroke("ㅉ"));
        assert_eq!(typed.key, "ㅉ");
        assert!(!typed.modifiers.has_shift);
    }

    fn focus_path(contexts: &[&str]) -> Vec<KeyContext> {
        contexts
            .iter()
            .map(|context| KeyContext::parse(context).unwrap())
            .collect()
    }

    #[test]
    fn history_shortcuts_stay_with_the_diff_editor_outside_task_detail() {
        let diff_editor_focus = focus_path(&["KanvibeWindow", DIFF_FILE_EDITOR_KEY_CONTEXT]);

        for command in [ShortcutCommandId::PageBack, ShortcutCommandId::PageForward] {
            assert!(should_leave_history_shortcut_to_text_entry(
                command,
                false,
                &diff_editor_focus
            ));
        }
    }

    #[test]
    fn history_shortcuts_stay_with_text_inputs_outside_task_detail() {
        for text_entry_context in [
            COMPONENT_INPUT_KEY_CONTEXT,
            BOARD_TEXT_FIELD_KEY_CONTEXT,
            FIND_WIDGET_KEY_CONTEXT,
            TERMINAL_KEY_CONTEXT,
        ] {
            let focus = focus_path(&["KanvibeWindow", text_entry_context]);

            assert!(should_leave_history_shortcut_to_text_entry(
                ShortcutCommandId::PageBack,
                false,
                &focus
            ));
        }
    }

    #[test]
    fn history_shortcuts_navigate_when_no_text_entry_is_focused() {
        let board_focus = focus_path(&["KanvibeWindow", "BoardRoute"]);

        assert!(!should_leave_history_shortcut_to_text_entry(
            ShortcutCommandId::PageBack,
            false,
            &board_focus
        ));
    }

    #[test]
    fn task_detail_history_shortcuts_navigate_even_from_text_entry() {
        let task_detail_terminal_focus = focus_path(&[
            "KanvibeWindow",
            TASK_DETAIL_KEY_CONTEXT,
            TERMINAL_KEY_CONTEXT,
        ]);

        assert!(!should_leave_history_shortcut_to_text_entry(
            ShortcutCommandId::PageBack,
            true,
            &task_detail_terminal_focus
        ));
    }

    #[test]
    fn other_commands_ignore_text_entry_focus_here() {
        let input_focus = focus_path(&["KanvibeWindow", COMPONENT_INPUT_KEY_CONTEXT]);

        assert!(!should_leave_history_shortcut_to_text_entry(
            ShortcutCommandId::CommandPalette,
            false,
            &input_focus
        ));
    }

    #[test]
    fn dock_numbers_are_intercepted_only_on_task_detail() {
        let dock = ShortcutCommandId::TaskDetailDock3;
        assert_eq!(
            route_shortcut_command(dock, true),
            ShortcutRouting::Intercept(Box::new(OpenTaskDetailDockItem { position: 3 }))
        );
        assert_eq!(
            route_shortcut_command(dock, false),
            ShortcutRouting::PassThrough
        );
    }

    #[test]
    fn tab_commands_outside_task_detail_only_close_the_window() {
        assert_eq!(
            route_shortcut_command(ShortcutCommandId::TerminalTabClose, false),
            ShortcutRouting::Intercept(Box::new(RunTerminalTabCommand {
                command: TerminalTabShortcutCommand::CloseWindow
            }))
        );
        assert_eq!(
            route_shortcut_command(ShortcutCommandId::TerminalTabNew, false),
            ShortcutRouting::PassThrough
        );
        assert_eq!(
            route_shortcut_command(ShortcutCommandId::TerminalTabClose, true),
            ShortcutRouting::Intercept(Box::new(RunTerminalTabCommand {
                command: TerminalTabShortcutCommand::CloseTab
            }))
        );
    }

    #[test]
    fn notification_create_palette_and_new_window_are_always_intercepted() {
        for command in [
            ShortcutCommandId::BoardNotification,
            ShortcutCommandId::CreateTask,
            ShortcutCommandId::CommandPalette,
            ShortcutCommandId::NewWindow,
        ] {
            assert!(matches!(
                route_shortcut_command(command, false),
                ShortcutRouting::Intercept(_)
            ));
        }
        assert_eq!(
            route_shortcut_command(ShortcutCommandId::PageBack, false),
            ShortcutRouting::WhenHandled(Box::new(NavigateBack))
        );
    }
}
