//! 단축키 정의와 판정. 모든 조합은 `Mod` 표기로 적고, `Mod`는 macOS에서 Command, 그 밖에서는 Control이다.

pub mod bindings;
pub mod bindings_cache;
pub mod key_event;
pub mod notation;
pub mod workspace_focus;

use crate::task_detail_dock::TASK_DETAIL_DOCK_ITEM_IDS;

/// 기본 단축키. 사용자가 바꿀 수 있는 명령의 기본값은 `bindings`의 명령 정의가 이 값을 가져다 쓴다.
pub mod defaults {
    pub const TASK_SEARCH: &str = "Mod+Shift+O";
    /// 다른 키 없이 눌렀다 떼면 워크스페이스 목록으로 이동한다
    pub const WORKSPACE_TASK_LIST_FOCUS: &str = "Mod+Alt";
    pub const BOARD_NOTIFICATION: &str = "Mod+Shift+I";
    pub const BOARD_PROJECT_FILTER: &str = "Mod+Shift+F";
    pub const BOARD_DESKTOP_FILTER: &str = "Mod+Shift+D";
    pub const COMMAND_PALETTE: &str = "Mod+Shift+P";
    pub const CREATE_TASK: &str = "Mod+N";
    pub const NEW_WINDOW: &str = "Mod+Shift+N";
    pub const PAGE_BACK: &str = "Mod+[";
    pub const PAGE_FORWARD: &str = "Mod+]";
    pub const BOARD_PAGE_FIND: &str = "Mod+F";
    pub const TERMINAL_TAB_NEW: &str = "Mod+T";
    pub const TERMINAL_TAB_CLOSE: &str = "Mod+W";
    pub const TERMINAL_WINDOW_CLOSE: &str = "Mod+Shift+W";
    pub const TERMINAL_TAB_PREVIOUS: &str = "Mod+Shift+[";
    pub const TERMINAL_TAB_NEXT: &str = "Mod+Shift+]";
    pub const TERMINAL_TOGGLE_CHAT: &str = "Mod+Shift+J";
    pub const TASK_DETAIL_USAGE: &str = "Mod+0";

    /// 앱이 삼키는 조합. 새로고침은 GPUI 화면에 의미가 없고 사용자 단축키로도 배정할 수 없다
    pub const BLOCKED_DESKTOP_SHORTCUTS: [&str; 1] = ["Mod+R"];
}

/// 도크에 실제로 있는 자리만큼만 번호를 준다. 비어 있는 번호를 노출하면 설정 화면이 아무것도 실행하지 않는 단축키를 보여 준다.
pub const TASK_DETAIL_DOCK_SHORTCUT_INDEXES: [u8; 6] = [1, 2, 3, 4, 5, 6];
const _: () = assert!(TASK_DETAIL_DOCK_SHORTCUT_INDEXES.len() == TASK_DETAIL_DOCK_ITEM_IDS.len());

pub const TERMINAL_TAB_SHORTCUT_INDEXES: [u8; 5] = [1, 2, 3, 4, 5];

/// dock 항목 n번을 여는 기본 단축키
pub fn task_detail_dock_shortcut(index: u8) -> String {
    format!("Mod+{index}")
}

/// 터미널 탭 n번으로 가는 기본 단축키. `Mod+숫자`는 dock이 쓰므로 Alt를 더해 비켜 간다.
/// macOS의 `Cmd+Shift+3~5`는 OS 스크린샷이 먼저 가져가 앱에 닿지 않으므로 Shift로는 비켜 갈 수 없다.
pub fn terminal_tab_shortcut(index: u8) -> String {
    format!("Mod+Alt+{index}")
}
