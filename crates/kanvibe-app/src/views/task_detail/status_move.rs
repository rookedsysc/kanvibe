//! 상태 이동(TS `statusMove` = `useTrackedAction(updateTaskStatus)`). 상태 버튼·Done 버튼·커맨드 팔레트가 같은 요청 하나를
//! 나눠 써서, 응답 전에는 다른 상태로 겹쳐 옮기지 않는다. 실패하면 지금 상태를 유지하고 사유를 보인다.

use std::sync::Arc;

use gpui_kit::{AppContext, Context, Global, PromptButton, PromptLevel, Window};
use kanvibe_domain::task::TaskStatus;
use kanvibe_services::kanban::KanbanService;

use super::TaskDetailView;

/// 앱 조립 지점이 만들 보드 서비스 자리. 아직 아무도 채우지 않으면 상태 이동은 사유와 함께 실패한다
#[derive(Default)]
pub struct SharedKanbanService {
    pub service: Option<Arc<KanbanService>>,
}

impl Global for SharedKanbanService {}

const KANBAN_SERVICE_NOT_COMPOSED: &str = "보드 서비스가 아직 앱에 조립되지 않았습니다.";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusMoveState {
    pub pending: Option<TaskStatus>,
    pub error: Option<String>,
}

impl TaskDetailView {
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

    pub(super) fn move_status(&mut self, status: TaskStatus, cx: &mut Context<Self>) {
        if self.status_move.pending.is_some() {
            return;
        }
        let service = cx
            .try_global::<SharedKanbanService>()
            .and_then(|shared| shared.service.clone());
        let Some(service) = service else {
            self.status_move.error = Some(KANBAN_SERVICE_NOT_COMPOSED.to_string());
            cx.notify();
            return;
        };
        self.status_move = StatusMoveState {
            pending: Some(status),
            error: None,
        };
        let task_id = self.task_id.clone();
        let update = cx.background_spawn(async move {
            service
                .update_task_status(&task_id, status)
                .map_err(|error| error.to_string())
        });
        self._status_move = Some(cx.spawn(async move |this, cx| {
            let result = update.await;
            let _ = this.update(cx, |view, cx| {
                view.status_move.pending = None;
                match result {
                    Ok(Some(updated)) => {
                        if let super::DetailLoad::Loaded(snapshot) = &mut view.load {
                            snapshot.task = updated;
                        }
                    }
                    Ok(None) => {}
                    Err(reason) => view.status_move.error = Some(reason),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
