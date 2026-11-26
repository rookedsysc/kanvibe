//! 창 활성 상태에 묶인 일(TS `useIsWindowActive`를 쓰던 두 훅).
//!
//! 알림 읽음: 창이 활성인 동안 작업이 보이면 그 작업의 알림을 읽음으로 바꾼다. 뒤에 가려진 상세 창은 열려 있다는
//! 이유만으로 읽음 처리하지 않고, 다시 앞으로 올 때 처리한다(`useMarkTaskNotificationsReadWhenFocused`).
//! 탭 목록 조회: 터미널이 보이고 창이 활성일 때만 주기적으로 읽고, 창이 다시 앞으로 오면 한 번 따라잡는다(`useTerminalTabs`).

use gpui_kit::{AppContext, Context, Window};

use super::TaskDetailView;
use super::layout_state::MainView;
use crate::board_focus_target::remember_board_focus_task;
use crate::notifications::reload_notification_center;
use crate::services::{AppServices, apply_fault};

/// TS preload `markTaskNotificationsRead`가 부르던 IPC 채널(QA 고장 주입 대상)
const MARK_TASK_NOTIFICATIONS_READ_CHANNEL: &str = "kanvibe:notifications-mark-task-read";

impl TaskDetailView {
    /// 처음 그릴 때 한 번 창 활성 변화를 구독한다
    pub(super) fn observe_window_activation(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self._window_activation.is_some() {
            return;
        }
        self.is_window_active = window.is_window_active();
        self.window_handle = Some(window.window_handle());
        // 이 창이 보드로 돌아가면 이 작업 카드에 포커스를 준다(TS `rememberBoardFocusTask`)
        remember_board_focus_task(window.window_handle().window_id(), &self.task_id, cx);
        self._window_activation = Some(cx.observe_window_activation(window, |view, window, cx| {
            view.is_window_active = window.is_window_active();
            view.mark_task_notifications_read(cx);
            view.sync_tab_polling(cx);
            cx.notify();
        }));
    }

    /// 창이 활성이고 작업을 읽었을 때 그 작업의 알림을 읽음으로 바꾼다. 같은 활성 구간에서는 한 번만 한다
    pub(super) fn mark_task_notifications_read(&mut self, cx: &mut Context<Self>) {
        if !self.is_window_active {
            self.has_marked_notifications_read = false;
            return;
        }
        if self.has_marked_notifications_read || self.snapshot().is_none() {
            return;
        }
        let Some(runtime) = cx.global::<AppServices>().runtime().cloned() else {
            return;
        };
        self.has_marked_notifications_read = true;
        let task_id = self.task_id.clone();
        let fault = cx.global::<AppServices>().check_fault(
            MARK_TASK_NOTIFICATIONS_READ_CHANNEL,
            &serde_json::json!([task_id]).to_string(),
        );
        let executor = cx.background_executor().clone();
        let mark = cx.background_spawn(async move {
            apply_fault(fault, &executor)
                .await
                .map_err(|failure| failure.reason)?;
            runtime
                .notification_store()
                .mark_task_notifications_read(&task_id)
                .map_err(|error| error.to_string())
        });
        cx.spawn(async move |_, cx| match mark.await {
            Ok(_) => {
                cx.update(reload_notification_center);
            }
            Err(error) => eprintln!("알림 읽음 처리 실패: {error}"),
        })
        .detach();
    }

    /// 터미널이 보이고 창이 활성이면 주기 조회를 켠다. 꺼져 있다가 켜지는 순간에는 밀린 변화를 한 번 읽는다
    pub(super) fn sync_tab_polling(&mut self, cx: &mut Context<Self>) {
        let has_terminal = self
            .snapshot()
            .is_some_and(|snapshot| snapshot.has_terminal());
        let should_poll =
            has_terminal && self.is_window_active && self.layout.main_view == MainView::Terminal;
        self.terminal_tabs
            .update(cx, |tabs, cx| tabs.set_polling_active(should_poll, cx));
    }
}
