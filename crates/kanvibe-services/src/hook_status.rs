//! 에이전트 hook이 보낸 상태 변경(`POST /api/hooks/status`)을 task에 반영한다(TS `hookService.ts`).
//! 상태 hook도 task CLI와 같은 task 토큰으로 인증한다. 토큰은 그 worktree의 targets.json을 읽을 수 있어야 얻으므로,
//! 소켓이나 평문 포트에 닿기만 한 프로세스가 아무 task의 상태를 바꾸지 못한다.

use std::collections::HashSet;
use std::sync::Arc;

use kanvibe_ai::host::SessionHost;
use kanvibe_ai::pane_chat::session_bindings::{PaneChatBindingContext, PaneChatSessionBindings};
use kanvibe_ai::types::AiSessionReaderContext;
use kanvibe_db::Database;
use kanvibe_db::repositories::projects::find_project;
use kanvibe_db::repositories::tasks::{find_task, update_task};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_process::task_cli::token::TaskCliTokenService;
use kanvibe_process::task_state_service::TaskStateService;
use serde::Serialize;
use serde_json::Value;

use crate::board_notifier::{
    BoardNotifier, HookStatusTargetMissingPayload, HookStatusTargetMissingReason,
    TaskStatusChangedPayload,
};
use kanvibe_process::request_scope::{is_task_reachable_from_tunnel, parse_bearer_token};

const HOOK_AUTHENTICATION_FAILURE: &str =
    "hook 인증에 실패했습니다. KanVibe에서 이 task에 접속하면 hooks가 다시 설치됩니다.";

/// hook이 보낸 본문(`taskId`, `status`, 선택 `agentSession`)
#[derive(Debug, Clone, PartialEq)]
pub struct HookStatusInput {
    pub task_id: String,
    pub status: String,
    pub agent_session: Option<Value>,
}

/// 성공 응답의 `data`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatusUpdated {
    pub id: String,
    pub status: TaskStatus,
    pub branch_name: Option<String>,
    pub project_name: String,
}

/// 거절 응답. `status_code`는 HTTP 상태 코드다
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookStatusRejection {
    #[serde(rename = "error")]
    pub message: String,
    #[serde(rename = "status")]
    pub status_code: u16,
}

impl HookStatusRejection {
    fn new(message: impl Into<String>, status_code: u16) -> Self {
        Self {
            message: message.into(),
            status_code,
        }
    }
}

/// hook이 보낸 에이전트 대화 정보를 pane chat에 묶을 task 범위
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatBindingTarget {
    pub task_id: String,
    pub worktree_path: Option<String>,
    pub repo_path: Option<String>,
    pub ssh_host: Option<String>,
}

/// TS `recordPaneChatSessionBinding`. 실패해도 상태 변경은 계속되어야 하므로 결과를 돌려주지 않는다
pub trait PaneChatSessionBindingRecorder: Send + Sync {
    fn record_pane_chat_session_binding(
        &self,
        target: &PaneChatBindingTarget,
        agent_session: &Value,
    );
}

/// kanvibe-ai 바인딩 표에 로컬 홈 기준 호스트 문맥을 붙여 넘긴다
pub struct AiPaneChatSessionBindingRecorder {
    pub bindings: Arc<PaneChatSessionBindings>,
    pub local_host: SessionHost,
}

impl PaneChatSessionBindingRecorder for AiPaneChatSessionBindingRecorder {
    fn record_pane_chat_session_binding(
        &self,
        target: &PaneChatBindingTarget,
        agent_session: &Value,
    ) {
        let host = self.local_host.with_ssh_host(target.ssh_host.as_deref());
        let mut reader = AiSessionReaderContext::new(host, target.worktree_path.as_deref());
        reader.repo_path = target.repo_path.clone();
        let context = PaneChatBindingContext {
            task_id: target.task_id.clone(),
            reader,
        };
        self.bindings
            .record_pane_chat_session_binding(&context, agent_session);
    }
}

/// task 상태를 저장소의 `.kanvibe/status.json`에 남긴다(TS `persistTaskStateForTask`). 실패는 로그만 남긴다
pub trait TaskStatePersistence: Send + Sync {
    fn persist_task_state_for_task(&self, task: &KanbanTask);
}

impl TaskStatePersistence for TaskStateService<'_> {
    fn persist_task_state_for_task(&self, task: &KanbanTask) {
        TaskStateService::persist_task_state_for_task(self, task);
    }
}

pub struct HookStatusService {
    database: Arc<Database>,
    board: Arc<BoardNotifier>,
    task_state: Arc<dyn TaskStatePersistence>,
    pane_chat_bindings: Arc<dyn PaneChatSessionBindingRecorder>,
}

/// JS 참 판정. `null`, `false`, `0`, 빈 문자열은 없는 값으로 본다
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn non_empty(value: Option<&String>) -> Option<&String> {
    value.filter(|text| !text.is_empty())
}

fn task_not_found(task_id: &str) -> HookStatusRejection {
    HookStatusRejection::new(format!("작업을 찾을 수 없습니다: {task_id}"), 404)
}

impl HookStatusService {
    pub fn new(
        database: Arc<Database>,
        board: Arc<BoardNotifier>,
        task_state: Arc<dyn TaskStatePersistence>,
        pane_chat_bindings: Arc<dyn PaneChatSessionBindingRecorder>,
    ) -> Self {
        Self {
            database,
            board,
            task_state,
            pane_chat_bindings,
        }
    }

    /// `authorization_header`가 없거나 다른 task의 토큰이면 task를 조회하지 않고 401로 거절한다.
    /// `source_ssh_hosts`는 요청이 들어온 터널이 맡은 호스트 별칭들이며, 터널로 온 요청은 그 호스트의 task만 바꿀 수 있다
    pub fn update_hook_task_status(
        &self,
        input: &HookStatusInput,
        authorization_header: Option<&str>,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<HookStatusUpdated, HookStatusRejection> {
        if input.task_id.is_empty() || input.status.is_empty() {
            return Err(HookStatusRejection::new(
                "taskId, status는 필수입니다.",
                400,
            ));
        }
        if !self.is_authorized_hook_request(&input.task_id, authorization_header) {
            return Err(HookStatusRejection::new(HOOK_AUTHENTICATION_FAILURE, 401));
        }
        let status = TaskStatus::parse(&input.status.to_lowercase()).ok_or_else(|| {
            HookStatusRejection::new(format!("유효하지 않은 상태입니다: {}", input.status), 400)
        })?;
        let (task, project) = self.find_task_with_project(&input.task_id)?;
        let Some(mut task) = task else {
            self.broadcast_target_missing(&input.task_id, status);
            return Err(task_not_found(&input.task_id));
        };
        if !is_task_reachable_from_tunnel(&task, project.as_ref(), source_ssh_hosts) {
            return Err(task_not_found(&input.task_id));
        }
        self.record_agent_session(input, &task, project.as_ref(), source_ssh_hosts);
        // 같은 상태를 다시 알리는 hook은 DB에 쓰지 않는다. 쓰면 수정 시각이 올라 보드 정렬 순서가 바뀐다
        if task.status != status {
            task.status = status;
            update_task(&self.database.connection(), &task)
                .map_err(|error| HookStatusRejection::new(error.to_string(), 500))?;
        }
        self.task_state.persist_task_state_for_task(&task);
        Ok(self.announce_status_change(&task, project.as_ref()))
    }

    fn is_authorized_hook_request(
        &self,
        task_id: &str,
        authorization_header: Option<&str>,
    ) -> bool {
        let Some(token) = parse_bearer_token(authorization_header) else {
            return false;
        };
        TaskCliTokenService::new(&self.database)
            .is_valid_task_cli_token(task_id, token)
            .unwrap_or(false)
    }

    fn find_task_with_project(
        &self,
        task_id: &str,
    ) -> Result<(Option<KanbanTask>, Option<Project>), HookStatusRejection> {
        let connection = self.database.connection();
        let database_failure =
            |error: kanvibe_db::DatabaseError| HookStatusRejection::new(error.to_string(), 500);
        let task = find_task(&connection, task_id).map_err(database_failure)?;
        let project = match task.as_ref().and_then(|task| task.project_id.as_deref()) {
            Some(project_id) => find_project(&connection, project_id).map_err(database_failure)?,
            None => None,
        };
        Ok((task, project))
    }

    fn broadcast_target_missing(&self, task_id: &str, status: TaskStatus) {
        self.board
            .broadcast_hook_status_target_missing(HookStatusTargetMissingPayload {
                task_id: task_id.to_string(),
                requested_status: status.as_str().to_string(),
                reason: HookStatusTargetMissingReason::TaskNotFound,
            });
    }

    /// 로컬 task이거나 그 호스트의 터널로 들어온 요청일 때만 대화 정보를 묶는다
    fn record_agent_session(
        &self,
        input: &HookStatusInput,
        task: &KanbanTask,
        project: Option<&Project>,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) {
        let Some(agent_session) = input
            .agent_session
            .as_ref()
            .filter(|value| is_truthy(value))
        else {
            return;
        };
        let ssh_host = non_empty(task.ssh_host.as_ref())
            .or_else(|| non_empty(project.and_then(|project| project.ssh_host.as_ref())))
            .cloned();
        let is_from_task_host = match &ssh_host {
            None => true,
            Some(host) => source_ssh_hosts.is_some_and(|hosts| hosts.contains(host)),
        };
        if !is_from_task_host {
            return;
        }
        let repo_path = project.map(|project| project.repo_path.clone());
        let target = PaneChatBindingTarget {
            task_id: task.id.clone(),
            worktree_path: non_empty(task.worktree_path.as_ref())
                .cloned()
                .or_else(|| repo_path.clone().filter(|path| !path.is_empty())),
            repo_path,
            ssh_host,
        };
        self.pane_chat_bindings
            .record_pane_chat_session_binding(&target, agent_session);
    }

    fn announce_status_change(
        &self,
        task: &KanbanTask,
        project: Option<&Project>,
    ) -> HookStatusUpdated {
        let project_name = non_empty(project.map(|project| &project.name))
            .or_else(|| non_empty(task.project_id.as_ref()))
            .cloned()
            .unwrap_or_else(|| "Unknown project".to_string());
        self.board.broadcast_board_update();
        self.board
            .broadcast_task_status_changed(TaskStatusChangedPayload {
                project_name: project_name.clone(),
                branch_name: task.branch_name.clone().unwrap_or_default(),
                task_title: task.title.clone(),
                description: task.description.clone(),
                new_status: task.status.as_str().to_string(),
                task_id: task.id.clone(),
            });
        HookStatusUpdated {
            id: task.id.clone(),
            status: task.status,
            branch_name: task.branch_name.clone(),
            project_name,
        }
    }
}

#[cfg(test)]
mod tests;
