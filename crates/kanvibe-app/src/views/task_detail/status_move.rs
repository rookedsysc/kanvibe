//! 상태 이동(TS `statusMove` = `useTrackedAction(updateTaskStatus)`). 상태 버튼·Done 버튼·커맨드 팔레트가 같은 요청 하나를
//! 나눠 써서, 응답 전에는 다른 상태로 겹쳐 옮기지 않는다. 실패하면 지금 상태를 유지하고 사유를 보인다.

use std::sync::Arc;

use gpui_kit::{App, Context, PromptButton, PromptLevel, Window};
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_services::kanban::KanbanService;

use super::TaskDetailView;
use crate::services::{AppServices, ServiceResult};
use crate::views::settings::service_call::{call_service, failure};

/// 데이터 디렉터리 없이 뜬 실행에서 보드 서비스가 필요한 요청을 거절하는 사유
pub const KANBAN_SERVICE_UNAVAILABLE: &str =
    "데이터 디렉터리 없이 실행되어 보드 서비스를 쓸 수 없습니다.";

/// 런타임의 보드 서비스. 데이터 디렉터리 없이 뜬 실행이면 `None`이다
pub fn kanban_service(cx: &App) -> Option<Arc<KanbanService>> {
    cx.global::<AppServices>()
        .runtime()
        .map(|runtime| Arc::clone(runtime.kanban()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusMoveState {
    pub pending: Option<TaskStatus>,
    pub failure: Option<StatusMoveFailure>,
}

/// 실패한 이동. 다시 시도는 같은 상태로 한 번 더 옮긴다(TS `useTrackedAction.retry`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMoveFailure {
    pub status: TaskStatus,
    pub reason: String,
}

impl TaskDetailView {
    /// 커맨드 팔레트가 옮길 작업과 그 지금 상태(TS `registerActiveTaskContext`). 작업을 읽은 뒤에만 있다
    pub fn palette_move_target(&self) -> Option<(String, TaskStatus)> {
        let snapshot = self.snapshot()?;
        Some((snapshot.task.id.clone(), snapshot.task.status))
    }

    /// 팔레트의 열 이동(TS `movePaletteTaskToStatus` → `statusMove.run`). Done 보존 안내는 묻지 않는다
    pub fn move_status_from_palette(&mut self, status: TaskStatus, cx: &mut Context<Self>) {
        self.move_status(status, cx);
    }

    /// Done 이동은 보존 안내를 먼저 확인받는다(`DoneConfirmDialog`)
    pub(super) fn request_status_move(
        &mut self,
        status: TaskStatus,
        needs_confirmation: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !needs_confirmation {
            self.move_status(status, cx);
            return;
        }
        let common = kanvibe_domain::i18n::Translator::new(self.screen.route.locale, "common");
        let answer = window.prompt(
            PromptLevel::Info,
            &common.translate("doneAlert.title"),
            Some(&common.translate("doneAlert.message")),
            &[
                PromptButton::ok(common.translate("doneAlert.confirm")),
                PromptButton::cancel(common.translate("doneAlert.cancel")),
            ],
            cx,
        );
        self._status_confirm = Some(cx.spawn(async move |this, cx| {
            if answer.await.ok() == Some(0) {
                let _ = this.update(cx, |view, cx| view.move_status(status, cx));
            }
        }));
    }

    /// 응답 전에는 다른 이동을 받지 않는다. QA 모드에서는 TS IPC 이름(`kanban.updateTaskStatus`)으로 고장 주입을 거친다
    pub(super) fn move_status(&mut self, status: TaskStatus, cx: &mut Context<Self>) {
        if self.status_move.pending.is_some() {
            return;
        }
        self.status_move = StatusMoveState {
            pending: Some(status),
            failure: None,
        };
        let service = kanban_service(cx);
        let task_id = self.task_id.clone();
        let arguments = serde_json::json!([task_id, status.as_str()]).to_string();
        let update = call_service(
            cx,
            "kanban.updateTaskStatus",
            arguments,
            move || -> ServiceResult<Option<KanbanTask>> {
                let service = service.ok_or_else(|| failure(KANBAN_SERVICE_UNAVAILABLE))?;
                service
                    .update_task_status(&task_id, status)
                    .map_err(failure)
            },
        );
        self._status_move = Some(cx.spawn(async move |this, cx| {
            let result = update.await;
            let _ = this.update(cx, |view, cx| view.finish_status_move(status, result, cx));
        }));
        cx.notify();
    }

    /// 실패하면 화면의 상태를 그대로 두고 사유를 남긴다
    fn finish_status_move(
        &mut self,
        status: TaskStatus,
        result: ServiceResult<Option<KanbanTask>>,
        cx: &mut Context<Self>,
    ) {
        self.status_move.pending = None;
        match result {
            Ok(Some(updated)) => {
                if let super::DetailLoad::Loaded(snapshot) = &mut self.load {
                    snapshot.task = updated;
                }
            }
            Ok(None) => {}
            Err(reason) => {
                self.status_move.failure = Some(StatusMoveFailure {
                    status,
                    reason: reason.display_message(),
                })
            }
        }
        cx.notify();
    }

    /// 실패 안내의 다시 시도. Done 보존 안내는 이미 확인받았으므로 다시 묻지 않는다
    pub(super) fn retry_status_move(&mut self, cx: &mut Context<Self>) {
        if let Some(failed) = self.status_move.failure.clone() {
            self.move_status(failed.status, cx);
        }
    }
}
