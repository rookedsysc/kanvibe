//! 창마다 보드가 마지막으로 받은 데이터(TS `BoardRoute`의 route cache, 창 `sessionStorage`).
//! 같은 창에서 보드로 돌아오면 이 데이터로 바로 그리고 뒤에서 새로 읽는다. 창이 닫히면 함께 버린다.
//! 보드는 만들어지는 순간에 아직 창 ID를 모르므로 창 루트(`AppWindow`) 엔티티 ID를 키로 쓴다.

use std::collections::HashMap;

use gpui_kit::{App, EntityId, Global};

use super::{BoardSnapshot, BoardView};

#[derive(Default)]
struct BoardSnapshotSessions(HashMap<EntityId, BoardSnapshot>);

impl Global for BoardSnapshotSessions {}

/// 창 루트가 닫힐 때 그 창의 기억을 버린다
pub fn forget_board_snapshot(app_window_id: EntityId, cx: &mut App) {
    if cx.has_global::<BoardSnapshotSessions>() {
        cx.global_mut::<BoardSnapshotSessions>()
            .0
            .remove(&app_window_id);
    }
}

impl BoardView {
    pub(super) fn remembered_board_snapshot(&self, cx: &App) -> Option<BoardSnapshot> {
        let app_window_id = self.screen.navigator.app_window_id();
        cx.try_global::<BoardSnapshotSessions>()?
            .0
            .get(&app_window_id)
            .cloned()
    }

    pub(super) fn remember_board_snapshot(&self, snapshot: &BoardSnapshot, cx: &mut App) {
        let app_window_id = self.screen.navigator.app_window_id();
        cx.default_global::<BoardSnapshotSessions>()
            .0
            .insert(app_window_id, snapshot.clone());
    }
}
