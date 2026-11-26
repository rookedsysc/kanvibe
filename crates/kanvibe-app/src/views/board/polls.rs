//! 보드가 주기적으로 다시 읽는 값(TS `useBoardTaskDiffStats`·`useRunningAgentPanes`). 둘 다 공용 `PolledValue`로 돌아
//! 창이 가려지면 멈추고 다시 앞으로 오면 한 번 따라잡는다. 첫 조회가 끝나 보드가 그려지는 첫 렌더에서 건다.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, Window};
use kanvibe_ai::types::RunningAgentPane;
use kanvibe_domain::task_diff_stats::TaskDiffStats;

use super::BoardView;
use super::board_model::task_ids_to_refresh_diff_stats;
use super::services::BoardServices;
use crate::polled_value::{PollingSubscription, start_polling};

/// 보드 카드 변경 집계를 다시 읽는 주기(TS `BOARD_DIFF_STATS_POLL_INTERVAL_MS`)
const DIFF_STATS_POLL_INTERVAL: Duration = Duration::from_millis(5_000);
/// 보드 배지는 provider가 붙었는지만 보여 주므로 느리게 돈다(TS `RUNNING_PANE_POLL_INTERVAL_MS`)
const RUNNING_PANE_POLL_INTERVAL: Duration = Duration::from_millis(5_000);

type DiffStatsByTaskId = BTreeMap<String, TaskDiffStats>;

/// 보드가 띄운 주기 조회. 값이 오면 보드 상태에 옮겨 담는다
#[derive(Default)]
pub(super) struct BoardPolls {
    diff_stats: Option<PollingSubscription<DiffStatsByTaskId>>,
    running_panes: Option<PollingSubscription<Vec<RunningAgentPane>>>,
}

impl BoardView {
    /// 보드를 받은 뒤 처음 그릴 때 두 조회를 한 번 건다. 보드가 사라지면 함께 멈춘다
    pub(super) fn ensure_board_polls(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.board.is_none() || self.polls.diff_stats.is_some() {
            return;
        }
        let Some(services) = self.services.clone() else {
            return;
        };
        self.polls.diff_stats = Some(self.poll_diff_stats(services.clone(), window, cx));
        self.polls.running_panes = Some(start_polling(
            RUNNING_PANE_POLL_INTERVAL,
            move |cx| services.load_running_agent_panes(cx),
            |this: &mut Self, panes, cx| {
                if this.live_sessions.running_panes != panes {
                    this.live_sessions.running_panes = panes;
                    cx.notify();
                }
            },
            window,
            cx,
        ));
    }

    /// 진행 중 카드의 변경 집계. 조회할 때마다 지금 보이는 카드에서 다시 돌릴 작업을 고른다
    fn poll_diff_stats(
        &mut self,
        services: Arc<BoardServices>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PollingSubscription<DiffStatsByTaskId> {
        let board = cx.entity().downgrade();
        let read = move |cx: &mut gpui_kit::App| {
            let board = board.clone();
            let services = services.clone();
            cx.spawn(async move |cx| {
                let task_ids = board
                    .read_with(cx, |board, _| {
                        task_ids_to_refresh_diff_stats(&board.displayed_tasks())
                    })
                    .map_err(|_| ())?;
                let load = cx.update(|cx| services.load_diff_stats(task_ids, cx));
                load.await.map_err(|_| ())
            })
        };
        start_polling(
            DIFF_STATS_POLL_INTERVAL,
            read,
            |this: &mut Self, stats, cx| {
                if let Some(board) = this.board.get_mut().as_mut()
                    && board.diff_stats != stats
                {
                    board.diff_stats = stats;
                    this.sync_diff_counters(cx);
                    cx.notify();
                }
            },
            window,
            cx,
        )
    }
}
