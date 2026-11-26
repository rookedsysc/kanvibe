//! 모바일에서 태스크를 만들고, 세션 타입을 바꾸고, 지우고, 상태를 옮기는 경로.
//! 모바일이 보낸 문자열이 데스크탑 열거형에 없으면 서버 오류가 아니라 잘못된 요청이라 `MobileTaskInputError`로 올린다.
//! 모르는 값을 조용히 버리면 기기에는 바뀐 것으로 보이고 데스크탑은 그대로 남는다.

use std::sync::Arc;

use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority, TaskStatus};
use kanvibe_process::git_ref::is_safe_git_ref;
use kanvibe_services::board_notifier::{BoardEvent, BoardEventSubscription};
use kanvibe_services::kanban::{CreateTaskInput, TaskDeletionOptions};
use serde::Serialize;
use serde_json::Value;

use super::MobileBridge;
use crate::app_settings::open_app_settings;
use crate::error::{MobileError, MobileResult};

/// 모바일 생성 화면이 보내는 태스크 초안. 데스크탑 입력에 있는 `sshHost`는 화면에 없으므로 받지 않는다.
/// 열어 두면 화면에 없는 경로로 임의의 호스트에 세션을 여는 요청을 만들 수 있다
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MobileTaskDraft {
    pub project_id: String,
    pub branch_name: String,
    pub base_branch: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    /// 기기가 보낸 값 그대로. 없으면 PC 기본값을 쓴다
    pub session_type: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileTaskDeletion {
    /// 꺼져 있어 삭제를 전파받지 못한 데스크탑 수
    pub failed_peer_count: usize,
}

/// 비어 있으면 고르지 않은 것이고, 모르는 값이면 잘못된 요청이다(TS `readEnumValue`)
fn read_enum_value<T>(
    parse: impl Fn(&str) -> Option<T>,
    submitted: Option<&str>,
    field_label: &str,
) -> MobileResult<Option<T>> {
    let Some(submitted) = submitted.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    parse(submitted)
        .map(Some)
        .ok_or_else(|| MobileError::task_input(format!("{field_label} 값이 올바르지 않습니다")))
}

fn read_git_ref(submitted: &str, field_label: &str) -> MobileResult<String> {
    if !is_safe_git_ref(submitted) {
        return Err(MobileError::task_input(format!(
            "{field_label}에 쓸 수 없는 문자가 있습니다"
        )));
    }
    Ok(submitted.to_string())
}

fn invalid_session_type() -> MobileError {
    MobileError::task_input("세션 타입 값이 올바르지 않습니다")
}

impl MobileBridge {
    fn read_draft_session_type(&self, submitted: Option<&Value>) -> MobileResult<SessionType> {
        let Some(submitted) = submitted else {
            return Ok(open_app_settings(&self.database).get_default_session_type()?);
        };
        // 문자열이 아닌 값은 비었든 아니든 열거형에서 찾을 수 없어 같은 문구로 거절된다
        let text = submitted.as_str().ok_or_else(invalid_session_type)?;
        read_enum_value(SessionType::parse, Some(text), "세션 타입")?
            .ok_or_else(invalid_session_type)
    }

    /// 제목을 따로 받지 않고 브랜치 이름을 그대로 쓰는 것은 데스크탑 생성 화면과 같다
    pub fn create_mobile_task(&self, draft: &MobileTaskDraft) -> MobileResult<KanbanTask> {
        let session_type = self.read_draft_session_type(draft.session_type.as_ref())?;
        let branch_name = read_git_ref(&draft.branch_name, "브랜치 이름")?;
        let base_branch = match draft.base_branch.as_deref().filter(|base| !base.is_empty()) {
            Some(base_branch) => Some(read_git_ref(base_branch, "베이스 브랜치")?),
            None => None,
        };
        let priority = read_enum_value(TaskPriority::parse, draft.priority.as_deref(), "우선순위")?;
        let input = CreateTaskInput {
            title: draft.branch_name.clone(),
            branch_name: Some(branch_name),
            project_id: Some(draft.project_id.clone()),
            base_branch,
            description: draft.description.clone(),
            priority,
            session_type: Some(session_type),
            ssh_host: None,
        };
        let created_task = self.tasks.create_task(&input)?;
        self.register_mobile_task_on_peer_clients(&created_task)?;
        Ok(created_task)
    }

    /// worktree는 연결된 이 데스크탑만 만들고, 같은 프로젝트를 다루는 다른 데스크탑에는 task 등록만 전파한다.
    /// 이미 만든 task를 되돌리지 않도록 전파 실패는 기기에 오류로 돌려주지 않고 로그로만 남긴다
    fn register_mobile_task_on_peer_clients(&self, created_task: &KanbanTask) -> MobileResult<()> {
        let (Some(project_id), Some(branch_name), Some(_)) = (
            created_task.project_id.as_deref(),
            created_task.branch_name.as_deref(),
            created_task.worktree_path.as_deref(),
        ) else {
            return Ok(());
        };
        let Some(project) = self.projects.get_project_by_id(project_id)? else {
            return Ok(());
        };
        let peer_results = self.peers.propagate_task_registration_to_peers(
            &project.repo_path,
            branch_name,
            project.ssh_host.as_deref(),
        );
        let failed_peers: Vec<_> = peer_results
            .iter()
            .filter(|result| !result.is_success)
            .collect();
        if !failed_peers.is_empty() {
            eprintln!(
                "[mobile] 다른 데스크탑에 task를 등록하지 못했습니다 {{ taskId: {:?}, failedPeers: {failed_peers:?} }}",
                created_task.id
            );
        }
        Ok(())
    }

    /// 기기에서 세션 타입을 바꾼다. 새 세션 도구가 없으면 기기에서는 설치할 수 없으니 데스크탑에서 설치하라고 거절한다
    pub fn change_mobile_task_session_type(
        &self,
        task_id: &str,
        submitted_session_type: &str,
        should_close_existing_session: bool,
    ) -> MobileResult<Option<KanbanTask>> {
        let session_type = read_enum_value(
            SessionType::parse,
            Some(submitted_session_type),
            "세션 타입",
        )?
        .ok_or_else(invalid_session_type)?;
        let Some(task) = self.tasks.get_task_by_id(task_id)? else {
            return Ok(None);
        };
        let dependency = self
            .session_dependencies
            .get_session_dependency_status(session_type, task.task.ssh_host.as_deref())?;
        if let (false, Some(tool_name)) = (dependency.is_available, dependency.tool_name) {
            return Err(MobileError::SessionDependencyMissing(format!(
                "{tool_name}이(가) 설치되어 있지 않습니다. 데스크탑에서 설치한 뒤 다시 시도해 주세요."
            )));
        }
        let changed = self.tasks.change_task_session_type(
            task_id,
            session_type,
            should_close_existing_session,
        )?;
        changed
            .map(Some)
            .ok_or_else(|| MobileError::Internal(format!("세션 타입 변경 실패: {task_id}")))
    }

    /// 기기가 세션을 닫을지 묻기 전에 기존 세션이 켜져 있는지 확인한다
    pub fn is_mobile_task_session_alive(&self, task_id: &str) -> MobileResult<bool> {
        Ok(self.tasks.is_task_session_alive(task_id)?)
    }

    /// 태스크 세션이 바뀌면 알린다. 이전 세션을 비추던 기기 스트림을 끊어 새 세션으로 다시 붙게 하는 데 쓴다
    pub fn watch_task_session_change(
        &self,
        task_id: &str,
        on_changed: Arc<dyn Fn() + Send + Sync>,
    ) -> BoardEventSubscription {
        let watched_task_id = task_id.to_string();
        self.board.subscribe(move |event| {
            if let BoardEvent::TaskSessionChanged(payload) = event
                && payload.task_id == watched_task_id
            {
                on_changed();
            }
        })
    }

    /// 연결된 이 데스크탑이 지우고 같은 task를 가진 다른 데스크탑으로 전파한다. 강제가 아니면 커밋하지 않은 변경에서 멈춘다.
    /// 없는 태스크면 `None`
    pub fn delete_mobile_task(
        &self,
        task_id: &str,
        is_forced: bool,
    ) -> MobileResult<Option<MobileTaskDeletion>> {
        let options = TaskDeletionOptions {
            should_reject_dirty_worktree: !is_forced,
            should_propagate_to_peers: true,
        };
        let deletion = self.tasks.delete_task_across_targets(task_id, &options)?;
        if !deletion.is_deleted {
            return Ok(None);
        }
        let failed_peer_count = deletion
            .peer_results
            .iter()
            .filter(|result| !result.is_success)
            .count();
        Ok(Some(MobileTaskDeletion { failed_peer_count }))
    }

    /// 파일·브랜치·세션은 그대로 두고 이 데스크탑의 등록만 지운다
    pub fn remove_mobile_task_registration(&self, task_id: &str) -> MobileResult<bool> {
        Ok(self.tasks.remove_task_registration(task_id)?)
    }

    /// 없는 태스크면 `None`
    pub fn update_mobile_task_status(
        &self,
        task_id: &str,
        submitted_status: &str,
    ) -> MobileResult<Option<KanbanTask>> {
        let status = read_enum_value(TaskStatus::parse, Some(submitted_status), "상태")?
            .ok_or_else(|| MobileError::task_input("상태 값이 올바르지 않습니다"))?;
        Ok(self.tasks.update_task_status(task_id, status)?)
    }

    /// 태스크 생성 화면의 베이스 브랜치 칸을 채운다
    pub fn get_mobile_project_branches(&self, project_id: &str) -> MobileResult<Vec<String>> {
        Ok(self.projects.get_project_branches(project_id)?)
    }
}
