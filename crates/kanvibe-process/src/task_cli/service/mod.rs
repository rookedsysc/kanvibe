//! 워크트리의 task CLI가 보낸 요청을 처리하는 핵심 로직(TS `taskCliService.ts`). HTTP 라우팅은 서버 crate가 맡는다.
//! 요청한 worktree의 task 토큰으로 인증하고, 그 task가 속한 프로젝트 안에서만 생성·조회·세션 준비·삭제를 허용한다.
//! 보드·세션·터널 같은 앱 서비스는 `TaskCliBackend` 하나로 받는다.

mod request;
#[cfg(test)]
mod tests;

use std::collections::HashSet;

use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, SessionType, TaskPriority};
use serde::Serialize;

pub use self::request::RequestBody;
use self::request::{
    read_git_ref_field, read_optional_string_field, read_priority_field,
    read_required_string_field, read_session_type_field,
};
use crate::request_scope::{is_task_reachable_from_tunnel, parse_bearer_token};
use crate::task_target_propagation::PeerTargetResult;

/// CLI 요청을 거절할 때 HTTP 응답 코드와 함께 돌려주는 오류(TS `TaskCliRequestError`), 또는 협력 서비스가 낸 오류
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskCliError {
    #[error("{message}")]
    Request { message: String, status_code: u16 },
    /// 협력 서비스가 돌려준 오류. 라우팅이 원문 메시지를 그대로 알린다
    #[error("{0}")]
    Failed(String),
}

impl TaskCliError {
    fn request(message: impl Into<String>, status_code: u16) -> Self {
        Self::Request {
            message: message.into(),
            status_code,
        }
    }

    /// 요청 거절이면 HTTP 응답 코드
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Request { status_code, .. } => Some(*status_code),
            Self::Failed(_) => None,
        }
    }
}

const AUTHENTICATION_FAILED_MESSAGE: &str =
    "task CLI 인증에 실패했습니다. KanVibe에서 hooks를 다시 설치하세요.";

/// 에이전트가 화면과 같은 세션에 접속하는 데 쓰는 준비 결과. 준비에 실패하면 error만 있고 task는 그대로 남는다
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliTaskSession {
    pub is_prepared: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub herdr_workspace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub herdr_panes: Option<Vec<HerdrTaskPane>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HerdrTaskPane {
    pub pane_id: String,
    pub tab_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedCliTask {
    pub id: String,
    pub project_id: Option<String>,
    pub branch_name: Option<String>,
    pub base_branch: Option<String>,
    pub worktree_path: Option<String>,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
    pub status: String,
    pub session: CliTaskSession,
    pub peers: Vec<PeerTargetResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedCliTaskSession {
    pub id: String,
    pub branch_name: Option<String>,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
    pub session: CliTaskSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedCliTask {
    pub id: String,
    pub title: String,
    pub branch_name: Option<String>,
    pub status: String,
    pub priority: Option<TaskPriority>,
    pub worktree_path: Option<String>,
    pub session_type: Option<SessionType>,
    pub session_name: Option<String>,
    pub ssh_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedCliTask {
    pub id: String,
    pub branch_name: Option<String>,
    /// 응답에서는 TS처럼 항상 `deleted: true`다
    #[serde(rename = "deleted")]
    pub is_deleted: bool,
    pub peers: Vec<PeerTargetResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredPeerTask {
    pub id: String,
    pub branch_name: String,
}

/// 보드 서비스에 넘기는 새 task 입력(TS `createTask`의 인자)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliTaskCreation {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<TaskPriority>,
    pub branch_name: String,
    pub base_branch: String,
    pub session_type: SessionType,
    pub project_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskDeletionOptions {
    pub should_reject_dirty_worktree: bool,
    pub should_propagate_to_peers: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDeletion {
    pub is_deleted: bool,
    pub peer_results: Vec<PeerTargetResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskDeletionError {
    /// 커밋하지 않은 변경이 있어 삭제를 멈췄다(TS `DirtyWorktreeError`). 메시지는 원문 그대로다
    DirtyWorktree(String),
    Failed(String),
}

/// 프로젝트 안에서 다른 task를 가리키는 방법. taskId는 client마다 다르므로 다른 task는 브랜치로 가리킨다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTaskSelector {
    Id(String),
    Branch(String),
}

/// task CLI 요청이 쓰는 앱 서비스. 각 메서드 설명에 대신하는 TS 함수를 적는다
pub trait TaskCliBackend: Sync {
    /// `taskCliTokenService.isValidTaskCliToken`
    fn is_valid_task_cli_token(&self, task_id: &str, token: &str) -> Result<bool, String>;
    /// `taskRepo.findOneBy({ id })`
    fn find_task(&self, task_id: &str) -> Result<Option<KanbanTask>, String>;
    /// `taskRepo.findOneBy({ ...where, projectId })`
    fn find_project_task(
        &self,
        project_id: &str,
        selector: &ProjectTaskSelector,
    ) -> Result<Option<KanbanTask>, String>;
    /// `projectRepo.findOneBy({ id })`
    fn find_project(&self, project_id: &str) -> Result<Option<Project>, String>;
    /// `taskRepo.find({ where: { projectId }, order: { createdAt: "ASC" } })`
    fn list_project_tasks(&self, project_id: &str) -> Result<Vec<KanbanTask>, String>;
    /// `kanbanService.getTaskIdByProjectAndBranch`
    fn task_id_by_project_and_branch(
        &self,
        project_id: &str,
        branch_name: &str,
    ) -> Result<Option<String>, String>;
    /// `kanbanService.createTask`. worktree와 task를 만든다
    fn create_task(&self, creation: &CliTaskCreation) -> Result<KanbanTask, String>;
    /// `kanbanService.deleteTaskAcrossTargets`
    fn delete_task_across_targets(
        &self,
        task_id: &str,
        options: &TaskDeletionOptions,
    ) -> Result<TaskDeletion, TaskDeletionError>;
    /// `projectService.syncRegisteredProjectWorktrees([project])`. 바뀐 것이 있으면 `true`
    fn sync_registered_project_worktrees(&self, project: &Project) -> Result<bool, String>;
    /// `boardNotifier.broadcastBoardUpdate`
    fn broadcast_board_update(&self);
    /// `terminalBridge.prepareTaskTerminal`
    fn prepare_task_terminal(&self, task: &KanbanTask) -> Result<CliTaskSession, String>;
    /// `taskTargetPropagationService.propagateTaskRegistrationToPeers`
    fn propagate_task_registration_to_peers(
        &self,
        project_repo_path: &str,
        branch_name: &str,
        ssh_host: Option<&str>,
    ) -> Vec<PeerTargetResult>;
}

/// 요청을 보낸 worktree의 task와 그 프로젝트
struct AuthorizedTaskContext {
    context_task: KanbanTask,
    project: Project,
}

struct CreationBase {
    base_branch: String,
    inherited_session_type: Option<SessionType>,
}

pub struct TaskCliService<'a> {
    backend: &'a dyn TaskCliBackend,
}

impl<'a> TaskCliService<'a> {
    pub fn new(backend: &'a dyn TaskCliBackend) -> Self {
        Self { backend }
    }

    /// 요청한 worktree의 브랜치(또는 지정한 task의 브랜치)에서 새 worktree와 task를 이 client에 만들고,
    /// 같은 프로젝트를 다루는 다른 client에는 task 등록만 전파한다.
    pub fn create_task_from_cli(
        &self,
        authorization_header: Option<&str>,
        body: &RequestBody,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<CreatedCliTask, TaskCliError> {
        let AuthorizedTaskContext {
            context_task,
            project,
        } = self.authorize_task_context(authorization_header, body, source_ssh_hosts)?;
        let creation = self.read_task_creation(body, &context_task, &project)?;
        let branch_name = creation.branch_name.clone();
        let created_task = self
            .backend
            .create_task(&creation)
            .map_err(TaskCliError::Failed)?;
        let (peers, prepared) =
            self.propagate_while_preparing(&project, &branch_name, &created_task.id);
        let (created_task_entity, session) = prepared?;

        Ok(CreatedCliTask {
            id: created_task.id,
            project_id: created_task.project_id,
            branch_name: created_task.branch_name,
            base_branch: created_task.base_branch,
            worktree_path: created_task.worktree_path,
            session_type: created_task.session_type,
            session_name: created_task_entity.session_name,
            ssh_host: created_task_entity.ssh_host,
            status: created_task.status.as_str().to_string(),
            session,
            peers,
        })
    }

    /// 요청 필드와 기준 브랜치를 확인해 보드 서비스에 넘길 생성 입력을 만든다. 같은 브랜치 task가 있으면 409다
    fn read_task_creation(
        &self,
        body: &RequestBody,
        context_task: &KanbanTask,
        project: &Project,
    ) -> Result<CliTaskCreation, TaskCliError> {
        let branch_name = read_git_ref_field(body, "branchName")?;
        let title = read_optional_string_field(body, "title")?;
        let description = read_optional_string_field(body, "description")?;
        let priority = read_priority_field(body)?;
        let requested_session_type = read_session_type_field(body)?;
        let creation_base = self.resolve_creation_base(body, context_task, project)?;

        let existing_task_id = self
            .backend
            .task_id_by_project_and_branch(&project.id, &branch_name)
            .map_err(TaskCliError::Failed)?;
        if existing_task_id.is_some() {
            return Err(TaskCliError::request(
                format!("이미 같은 브랜치의 task가 있습니다: {branch_name}"),
                409,
            ));
        }

        Ok(CliTaskCreation {
            title: title.unwrap_or_else(|| branch_name.clone()),
            description,
            priority,
            branch_name,
            base_branch: creation_base.base_branch,
            session_type: requested_session_type
                .or(creation_base.inherited_session_type)
                .unwrap_or(SessionType::Terminal),
            project_id: project.id.clone(),
        })
    }

    /// 전파와 세션 준비는 서로의 결과를 쓰지 않으므로 함께 시작해 응답 지연을 둘 중 긴 쪽으로 줄인다
    fn propagate_while_preparing(
        &self,
        project: &Project,
        branch_name: &str,
        created_task_id: &str,
    ) -> (
        Vec<PeerTargetResult>,
        Result<(KanbanTask, CliTaskSession), TaskCliError>,
    ) {
        std::thread::scope(|scope| {
            let propagation = scope.spawn(|| {
                self.backend.propagate_task_registration_to_peers(
                    &project.repo_path,
                    branch_name,
                    project.ssh_host.as_deref(),
                )
            });
            let prepared = self.prepare_created_task_session(created_task_id, project);
            let peers = propagation.join().expect("전파 스레드");
            (peers, prepared)
        })
    }

    /// 같은 프로젝트의 task를 돌려준다. 토큰 하나로 볼 수 있는 범위를 생성·삭제와 같은 프로젝트 안으로 묶는다
    pub fn list_tasks_from_cli(
        &self,
        authorization_header: Option<&str>,
        body: &RequestBody,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<Vec<ListedCliTask>, TaskCliError> {
        let AuthorizedTaskContext { project, .. } =
            self.authorize_task_context(authorization_header, body, source_ssh_hosts)?;
        let project_tasks = self
            .backend
            .list_project_tasks(&project.id)
            .map_err(TaskCliError::Failed)?;

        Ok(project_tasks
            .into_iter()
            .map(|task| ListedCliTask {
                id: task.id,
                title: task.title,
                branch_name: task.branch_name,
                status: task.status.as_str().to_string(),
                priority: task.priority,
                worktree_path: task.worktree_path,
                session_type: task.session_type,
                session_name: task.session_name,
                ssh_host: task.ssh_host,
            })
            .collect())
    }

    /// task의 세션을 준비한다. 브랜치를 주지 않으면 요청한 worktree의 task가 대상이다.
    /// 세션 생성 전에 만든 task나 준비가 실패했던 task를 에이전트가 다시 준비할 때 쓴다.
    pub fn prepare_task_session_from_cli(
        &self,
        authorization_header: Option<&str>,
        body: &RequestBody,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<PreparedCliTaskSession, TaskCliError> {
        let AuthorizedTaskContext {
            context_task,
            project,
        } = self.authorize_task_context(authorization_header, body, source_ssh_hosts)?;
        let target_task = self.resolve_branch_target(body, context_task, &project)?;
        let session = self.prepare_cli_task_session(&target_task);

        Ok(PreparedCliTaskSession {
            id: target_task.id,
            branch_name: target_task.branch_name,
            session_type: target_task.session_type,
            session_name: target_task.session_name,
            ssh_host: target_task.ssh_host,
            session,
        })
    }

    fn prepare_created_task_session(
        &self,
        created_task_id: &str,
        project: &Project,
    ) -> Result<(KanbanTask, CliTaskSession), TaskCliError> {
        let selector = ProjectTaskSelector::Id(created_task_id.to_string());
        let not_found_message = format!("만든 task를 찾을 수 없습니다: {created_task_id}");
        let created_task_entity =
            self.find_task_in_project(&selector, project, &not_found_message)?;
        let session = self.prepare_cli_task_session(&created_task_entity);
        Ok((created_task_entity, session))
    }

    /// 세션 준비 실패는 task 생성 실패가 아니므로 응답에만 남기고, 에이전트가 session 명령으로 다시 시도하게 한다
    fn prepare_cli_task_session(&self, task: &KanbanTask) -> CliTaskSession {
        let has_session_name = task
            .session_name
            .as_deref()
            .is_some_and(|name| !name.is_empty());
        if task.session_type.is_none() || !has_session_name {
            return CliTaskSession {
                is_prepared: false,
                error: Some("task에 연결된 세션이 없습니다.".into()),
                ..CliTaskSession::default()
            };
        }

        self.backend
            .prepare_task_terminal(task)
            .unwrap_or_else(|message| {
                eprintln!(
                    "[task-cli] 세션을 준비하지 못했습니다 {{ taskId: {:?}, error: {message:?} }}",
                    task.id
                );
                CliTaskSession {
                    is_prepared: false,
                    error: Some(message),
                    ..CliTaskSession::default()
                }
            })
    }

    /// task를 삭제한다. 브랜치를 주지 않으면 요청한 worktree의 task 자신이 대상이다.
    /// taskId는 client마다 다르므로 다른 task는 같은 프로젝트 안의 브랜치 이름으로 가리킨다.
    pub fn delete_task_from_cli(
        &self,
        authorization_header: Option<&str>,
        body: &RequestBody,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<DeletedCliTask, TaskCliError> {
        let AuthorizedTaskContext {
            context_task,
            project,
        } = self.authorize_task_context(authorization_header, body, source_ssh_hosts)?;
        if body.get("confirmed") != Some(&serde_json::Value::Bool(true)) {
            return Err(TaskCliError::request(
                "삭제를 확인하는 confirmed: true가 필요합니다.",
                400,
            ));
        }

        let target_task = self.resolve_branch_target(body, context_task, &project)?;
        if target_task.branch_name.as_deref() == Some(project.default_branch.as_str()) {
            return Err(TaskCliError::request(
                format!(
                    "프로젝트 기본 브랜치({})의 task는 CLI로 지울 수 없습니다. KanVibe 화면에서 지우세요.",
                    project.default_branch
                ),
                403,
            ));
        }

        let deletion = self.delete_target_task(body, &target_task.id)?;
        if !deletion.is_deleted {
            return Err(TaskCliError::request(
                format!("task를 찾을 수 없습니다: {}", target_task.id),
                404,
            ));
        }

        Ok(DeletedCliTask {
            id: target_task.id,
            branch_name: target_task.branch_name,
            is_deleted: true,
            peers: deletion.peer_results,
        })
    }

    /// 다른 client가 만든 worktree를 이 client의 task로 등록한다. worktree는 만들지 않는다.
    /// 배경 동기화가 새 worktree를 등록하는 경로를 그대로 써서 hooks 설치와 상태 복원이 같은 규칙을 따르게 한다.
    pub fn register_task_from_peer(
        &self,
        authorization_header: Option<&str>,
        body: &RequestBody,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<RegisteredPeerTask, TaskCliError> {
        let AuthorizedTaskContext { project, .. } =
            self.authorize_task_context(authorization_header, body, source_ssh_hosts)?;
        let branch_name = read_git_ref_field(body, "branchName")?;

        let existing_task_id = self
            .backend
            .task_id_by_project_and_branch(&project.id, &branch_name)
            .map_err(TaskCliError::Failed)?;
        if let Some(id) = existing_task_id {
            return Ok(RegisteredPeerTask { id, branch_name });
        }

        let has_changed = self
            .backend
            .sync_registered_project_worktrees(&project)
            .map_err(TaskCliError::Failed)?;
        if has_changed {
            self.backend.broadcast_board_update();
        }

        let registered_task_id = self
            .backend
            .task_id_by_project_and_branch(&project.id, &branch_name)
            .map_err(TaskCliError::Failed)?;
        let Some(id) = registered_task_id else {
            return Err(TaskCliError::request(
                format!("프로젝트에서 {branch_name} 브랜치의 worktree를 찾지 못했습니다."),
                404,
            ));
        };
        Ok(RegisteredPeerTask { id, branch_name })
    }

    /// 토큰을 먼저 확인해 자격 없는 요청에는 task가 있는지조차 알려 주지 않는다.
    /// 터널로 온 요청은 그 터널 호스트의 task 토큰만 인정해, 다른 호스트에서 새어 나온 토큰으로 이 호스트의 task를 다루지 못하게 한다.
    fn authorize_task_context(
        &self,
        authorization_header: Option<&str>,
        body: &RequestBody,
        source_ssh_hosts: Option<&HashSet<String>>,
    ) -> Result<AuthorizedTaskContext, TaskCliError> {
        let context_task_id = read_required_string_field(body, "contextTaskId")?;
        let token = parse_bearer_token(authorization_header);
        let is_authorized = match token {
            Some(token) => self.check_task_cli_token(&context_task_id, token)?,
            None => false,
        };
        if !is_authorized {
            return Err(TaskCliError::request(AUTHENTICATION_FAILED_MESSAGE, 401));
        }

        let Some(context_task) = self
            .backend
            .find_task(&context_task_id)
            .map_err(TaskCliError::Failed)?
        else {
            return Err(TaskCliError::request(
                format!("task를 찾을 수 없습니다: {context_task_id}"),
                404,
            ));
        };
        let project = self.find_context_project(&context_task)?;
        if !is_task_reachable_from_tunnel(&context_task, project.as_ref(), source_ssh_hosts) {
            return Err(TaskCliError::request(AUTHENTICATION_FAILED_MESSAGE, 401));
        }
        let Some(project) = project else {
            return Err(TaskCliError::request(
                "프로젝트에 연결되지 않은 task에서는 task를 만들거나 지울 수 없습니다.",
                409,
            ));
        };
        Ok(AuthorizedTaskContext {
            context_task,
            project,
        })
    }

    /// 기본 정책은 변경 있는 worktree를 거부하고 다른 client에도 전파하는 것이다. `force`와 `propagateToPeers`로 바꾼다
    fn delete_target_task(
        &self,
        body: &RequestBody,
        task_id: &str,
    ) -> Result<TaskDeletion, TaskCliError> {
        let options = TaskDeletionOptions {
            should_reject_dirty_worktree: body.get("force") != Some(&serde_json::Value::Bool(true)),
            should_propagate_to_peers: body.get("propagateToPeers")
                != Some(&serde_json::Value::Bool(false)),
        };
        self.backend
            .delete_task_across_targets(task_id, &options)
            .map_err(|error| match error {
                TaskDeletionError::DirtyWorktree(message) => TaskCliError::request(
                    format!("{message} 그래도 지우려면 --force를 붙이세요."),
                    409,
                ),
                TaskDeletionError::Failed(message) => TaskCliError::Failed(message),
            })
    }

    fn find_context_project(
        &self,
        context_task: &KanbanTask,
    ) -> Result<Option<Project>, TaskCliError> {
        match context_task
            .project_id
            .as_deref()
            .filter(|id| !id.is_empty())
        {
            Some(project_id) => self
                .backend
                .find_project(project_id)
                .map_err(TaskCliError::Failed),
            None => Ok(None),
        }
    }

    /// 토큰 검증 중의 내부 오류는 인증 전이라 요청자가 누구인지 모른다.
    /// 라우팅은 오류 메시지를 그대로 돌려주므로, 저장소 경로 같은 원인은 로그에만 남기고 감춘 메시지로 바꾼다.
    fn check_task_cli_token(
        &self,
        context_task_id: &str,
        token: &str,
    ) -> Result<bool, TaskCliError> {
        self.backend
            .is_valid_task_cli_token(context_task_id, token)
            .map_err(|message| {
                eprintln!("[task-cli] CLI 토큰을 확인하지 못했습니다 {message}");
                TaskCliError::request(
                    "task CLI 인증을 확인하지 못했습니다. 잠시 뒤 다시 시도하세요.",
                    500,
                )
            })
    }

    fn resolve_creation_base(
        &self,
        body: &RequestBody,
        context_task: &KanbanTask,
        project: &Project,
    ) -> Result<CreationBase, TaskCliError> {
        let Some(from_task_id) = read_optional_string_field(body, "fromTaskId")? else {
            return Ok(CreationBase {
                base_branch: read_git_ref_field(body, "baseBranch")?,
                inherited_session_type: context_task.session_type,
            });
        };

        let selector = ProjectTaskSelector::Id(from_task_id.clone());
        let from_task = self.find_task_in_project(
            &selector,
            project,
            &format!("기준 task를 찾을 수 없습니다: {from_task_id}"),
        )?;
        let Some(base_branch) = from_task
            .branch_name
            .filter(|branch| request::is_safe_git_ref(branch))
        else {
            return Err(TaskCliError::request(
                "기준 task에 기준으로 쓸 수 있는 브랜치가 없습니다.",
                409,
            ));
        };
        Ok(CreationBase {
            base_branch,
            inherited_session_type: from_task.session_type,
        })
    }

    fn resolve_branch_target(
        &self,
        body: &RequestBody,
        context_task: KanbanTask,
        project: &Project,
    ) -> Result<KanbanTask, TaskCliError> {
        if !body.contains_key("branchName") {
            return Ok(context_task);
        }

        let branch_name = read_git_ref_field(body, "branchName")?;
        let not_found_message = format!("{branch_name} 브랜치의 task를 찾을 수 없습니다.");
        self.find_task_in_project(
            &ProjectTaskSelector::Branch(branch_name),
            project,
            &not_found_message,
        )
    }

    /// 다른 프로젝트의 task는 없는 것과 같게 다뤄 토큰 하나로 다룰 수 있는 범위를 프로젝트 안으로 묶는다
    fn find_task_in_project(
        &self,
        selector: &ProjectTaskSelector,
        project: &Project,
        not_found_message: &str,
    ) -> Result<KanbanTask, TaskCliError> {
        self.backend
            .find_project_task(&project.id, selector)
            .map_err(TaskCliError::Failed)?
            .ok_or_else(|| TaskCliError::request(not_found_message, 404))
    }
}
