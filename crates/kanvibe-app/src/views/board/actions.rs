//! 칸반 카드 동작: 이동(드래그·메뉴·커맨드), Done 이동 확인, 삭제, Done 더 보기, 상세 열기.
//! TS `Board.tsx`처럼 화면을 먼저 바꾸고 저장하며, 저장 중에는 같은 카드의 요청을 다시 보내지 않는다.

use gpui_kit::{Context, FocusHandle, Pixels, Point, Window};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::TaskStatus;

use super::board_model::{
    TaskDeletionState, TaskMoveState, append_done_page, build_drag_move_plan, find_task,
    remove_task, restore_moved_task, should_confirm_done_move,
};
use super::task_filters::TaskKindFilter;
use super::{BoardView, HeaderPopover, OpenContextMenu};
use crate::qa::prompts::confirm_action;
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

impl BoardView {
    pub(super) fn card_focus_handle(
        &mut self,
        task_id: &str,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        self.card_focus
            .entry(task_id.to_string())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    pub(super) fn set_task_kind_filter(&mut self, filter: TaskKindFilter, cx: &mut Context<Self>) {
        self.filters.task_kind = filter;
        self.sync_task_kind_query(filter, cx);
        cx.notify();
    }

    /// 프로젝트 필터 트리거의 칩: 앞의 두 이름과 나머지 개수(`+N` 배지)
    pub fn selected_project_chips(&self) -> (Vec<String>, usize) {
        let names = self.selected_project_names();
        let hidden_count = names
            .len()
            .saturating_sub(super::header_popovers::MAX_VISIBLE_CHIPS);
        let visible = names
            .into_iter()
            .take(super::header_popovers::MAX_VISIBLE_CHIPS)
            .collect();
        (visible, hidden_count)
    }

    pub(super) fn selected_project_names(&self) -> Vec<String> {
        let Some(board) = self.board.as_ref() else {
            return Vec::new();
        };
        self.filters
            .selected_project_ids
            .iter()
            .filter_map(|id| board.catalog.names.get(id).cloned())
            .collect()
    }

    pub(super) fn toggle_popover(
        &mut self,
        popover: HeaderPopover,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open_popover == Some(popover) {
            self.close_popover(window, cx);
        } else {
            self.open_popover = Some(popover);
            self.project_search
                .update(cx, |field, cx| field.set_text("", cx));
            self.popover_highlight = 0;
            self.is_popover_keyboard_navigated = false;
            self.popover_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_popover = None;
        self.board_focus.focus(window, cx);
        cx.notify();
    }

    /// 카드를 눌렀을 때. Shift를 누르고 있으면 새 창, 아니면 같은 작업 창을 먼저 찾고 없을 때 이 창에서 연다
    pub(super) fn activate_task_card(
        &mut self,
        task_id: &str,
        should_open_new_window: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.deletion_states.get(task_id),
            Some(TaskDeletionState::Deleting)
        ) {
            return;
        }
        let request = TaskNavigationRequest {
            origin_window: Some(window.window_handle().window_id()),
            should_open_new_window,
        };
        navigate_to_task_detail(task_id, self.locale(), request, cx);
    }

    pub(super) fn open_context_menu(
        &mut self,
        task_id: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            self.deletion_states.get(task_id),
            Some(TaskDeletionState::Deleting)
        ) {
            return;
        }
        self.context_menu = Some(OpenContextMenu {
            task_id: task_id.to_string(),
            position,
            is_status_list_open: false,
            highlighted_index: 0,
        });
        // 마우스를 누른 카드가 포커스를 가져간 뒤에 메뉴로 옮긴다
        let menu_focus = self.menu_focus.clone();
        window.defer(cx, move |window, cx| menu_focus.focus(window, cx));
        cx.notify();
    }

    /// 메뉴를 닫는다. Esc로 닫았을 때만 메뉴를 연 카드로 포커스를 돌려준다(TS는 항목 실행·바깥 클릭에서 포커스를 옮기지 않는다)
    pub(super) fn close_context_menu(
        &mut self,
        should_restore_focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.context_menu.take() else {
            return;
        };
        if should_restore_focus {
            let focus = self.card_focus_handle(&menu.task_id, cx);
            focus.focus(window, cx);
        } else {
            self.board_focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn drop_task_on_column(
        &mut self,
        task_id: &str,
        target_status: TaskStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_task_to_status(task_id, target_status, window, cx);
    }

    /// 카드를 다른 열로 옮긴다. 정리할 자원이 있는 카드를 Done으로 옮기면 먼저 확인을 받는다
    pub(crate) fn move_task_to_status(
        &mut self,
        task_id: &str,
        target_status: TaskStatus,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.board.as_ref() else {
            return;
        };
        let Some(task) = find_task(&board.tasks, task_id) else {
            return;
        };
        if task.status == target_status {
            return;
        }
        if should_confirm_done_move(task, target_status, board.is_done_alert_dismissed) {
            self.pending_done_move = Some((task_id.to_string(), target_status));
            self.is_done_dont_ask_checked = false;
            cx.notify();
            return;
        }
        self.execute_move(task_id, target_status, cx);
    }

    /// 화면을 먼저 옮기고 저장한다. 실패하면 그 카드만 원래 자리로 되돌리고 사유와 다시 옮기기를 남긴다
    pub(super) fn execute_move(
        &mut self,
        task_id: &str,
        target_status: TaskStatus,
        cx: &mut Context<Self>,
    ) {
        let (Some(board), Some(services)) = (self.board.as_mut(), self.services.clone()) else {
            return;
        };
        if self.saving_move_task_ids.contains(task_id) {
            return;
        }
        let Some(plan) = build_drag_move_plan(&board.tasks, task_id, target_status) else {
            return;
        };
        board.tasks = plan.updated_tasks.clone();
        board.done.apply_delta(plan.done_delta);
        self.saving_move_task_ids.insert(task_id.to_string());
        self.move_states
            .insert(task_id.to_string(), TaskMoveState::Saving);
        let save = services.move_task_to_column(task_id.to_string(), target_status, cx);
        let task = cx.spawn(async move |this, cx| {
            let result = save.await;
            let _ = this.update(cx, |this, cx| {
                this.saving_move_task_ids.remove(&plan.task_id);
                match result {
                    Ok(()) => {
                        this.move_states.remove(&plan.task_id);
                    }
                    Err(failure) => {
                        if let Some(board) = this.board.as_mut() {
                            restore_moved_task(&mut board.tasks, &plan);
                            board.done.apply_delta(-plan.done_delta);
                        }
                        this.move_states.insert(
                            plan.task_id.clone(),
                            TaskMoveState::Failed {
                                reason: failure.reason,
                                target_status: plan.target_status,
                            },
                        );
                    }
                }
                cx.notify();
            });
        });
        task.detach();
        cx.notify();
    }

    /// Done 이동 확인. 다시 묻지 않기를 골랐으면 설정에 남기고 이후로는 묻지 않는다
    pub(super) fn confirm_done_move(&mut self, cx: &mut Context<Self>) {
        let Some((task_id, target_status)) = self.pending_done_move.take() else {
            return;
        };
        if self.is_done_dont_ask_checked
            && let Some(services) = &self.services
            && let Err(failure) = services.dismiss_done_alert()
        {
            eprintln!("[kanvibe] failed to dismiss done alert: {}", failure.reason);
        }
        if let Some(board) = self.board.as_mut() {
            board.is_done_alert_dismissed = true;
        }
        self.execute_move(&task_id, target_status, cx);
    }

    pub(super) fn cancel_done_move(&mut self, cx: &mut Context<Self>) {
        self.pending_done_move = None;
        cx.notify();
    }

    /// 삭제를 확인받은 뒤 지운다
    pub(super) fn confirm_and_delete_task(
        &mut self,
        task_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = Translator::new(self.locale(), "task").translate("deleteConfirm");
        let answer = confirm_action(&message, None, window, cx);
        cx.spawn(async move |this, cx| {
            if !answer.await {
                return;
            }
            let _ = this.update(cx, |this, cx| this.delete_task(task_id, cx));
        })
        .detach();
    }

    /// 같은 task의 삭제는 한 번만 보낸다. 메뉴·키보드·재시도가 모두 이 함수를 지난다
    pub(super) fn delete_task(&mut self, task_id: String, cx: &mut Context<Self>) {
        let Some(services) = self.services.clone() else {
            return;
        };
        if !self.deleting_task_ids.insert(task_id.clone()) {
            return;
        }
        self.deletion_states
            .insert(task_id.clone(), TaskDeletionState::Deleting);
        let deletion = services.delete_task(task_id.clone(), cx);
        let task = cx.spawn(async move |this, cx| {
            let result = deletion.await;
            let _ = this.update(cx, |this, cx| {
                this.deleting_task_ids.remove(&task_id);
                match result {
                    Ok(is_deleted) => {
                        this.deletion_states.remove(&task_id);
                        if is_deleted {
                            this.remove_deleted_task(&task_id);
                        }
                    }
                    Err(failure) => {
                        this.deletion_states.insert(
                            task_id.clone(),
                            TaskDeletionState::Failed {
                                reason: failure.reason,
                            },
                        );
                    }
                }
                cx.notify();
            });
        });
        task.detach();
        cx.notify();
    }

    fn remove_deleted_task(&mut self, task_id: &str) {
        let Some(board) = self.board.as_mut() else {
            return;
        };
        let was_done = board.tasks.done.iter().any(|task| task.id == task_id);
        if remove_task(&mut board.tasks, task_id) && was_done {
            board.done.apply_delta(-1);
        }
    }

    /// Done 열 다음 페이지. 실패하면 사유를 남기고 다시 누르면 같은 페이지를 다시 요청한다
    pub(super) fn load_more_done(&mut self, cx: &mut Context<Self>) {
        let (Some(board), Some(services)) = (self.board.as_mut(), self.services.clone()) else {
            return;
        };
        let Some((generation, offset)) = board.done.begin() else {
            return;
        };
        let page = services.load_more_done(offset, cx);
        let task = cx.spawn(async move |this, cx| {
            let result = page.await;
            let _ = this.update(cx, |this, cx| {
                let Some(board) = this.board.as_mut() else {
                    return;
                };
                match result {
                    Ok((tasks, total)) => {
                        let fetched_count = tasks.len();
                        if board.done.finish(generation, Ok((fetched_count, total))) {
                            append_done_page(&mut board.tasks, tasks);
                        }
                    }
                    Err(failure) => {
                        board.done.finish(generation, Err(failure.reason));
                    }
                }
                cx.notify();
            });
        });
        task.detach();
        cx.notify();
    }
}
