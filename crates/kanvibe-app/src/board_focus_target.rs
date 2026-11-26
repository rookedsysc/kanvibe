//! 보드로 돌아왔을 때 포커스를 줄 작업 카드(TS `boardFocusTarget.ts`).
//! 작업 상세가 열리면 그 작업 id를 적어 두고, 같은 창에서 보드가 처음 그려질 때 꺼내 그 카드에 포커스를 준다.
//! TS는 창마다 따로인 `sessionStorage`에 두었으므로 창 id별로 담는다.

use std::collections::HashMap;

use gpui_kit::{App, Global, WindowId};

#[derive(Default)]
struct BoardFocusTargets(HashMap<WindowId, String>);

impl Global for BoardFocusTargets {}

/// 이 창의 보드 복귀 포커스 대상으로 작업을 적는다. 빈 id는 적지 않는다
pub fn remember_board_focus_task(window_id: WindowId, task_id: &str, cx: &mut App) {
    if task_id.trim().is_empty() {
        return;
    }
    cx.default_global::<BoardFocusTargets>()
        .0
        .insert(window_id, task_id.to_string());
}

/// 적어 둔 대상을 꺼내고 지운다
pub fn consume_board_focus_task(window_id: WindowId, cx: &mut App) -> Option<String> {
    cx.default_global::<BoardFocusTargets>()
        .0
        .remove(&window_id)
}
