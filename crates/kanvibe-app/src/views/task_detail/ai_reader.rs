//! 작업 상세의 AI 세션 조회(TS `project.getTaskLiveAiSessions`·`getTaskAgentCallGraph`·`getTaskAiSessions`·
//! `getTaskAiSessionDetail`, `diff.readTaskWorkspaceFile`).
//!
//! 통합 단계가 앱 조립 지점을 만들 때까지는 처음 부를 때 GPUI 전역으로 한 번 만든다(`TerminalRuntimeSlot`과 같은 임시 자리).
//! 프로세스 환경은 읽기만 하고, 세션 리더에는 provider 경로 재정의 변수만 넘긴다.

use std::collections::BTreeMap;
use std::sync::Arc;

use gpui_kit::{App, AppContext, Global, Task};
use kanvibe_ai::host::SessionHost;
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult, AiMessageRole,
    LiveAiSession,
};
use kanvibe_db::Database;
use kanvibe_db::repositories::{projects, tasks};
use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::shell_environment::{ShellPlatform, current_user_home_directory};
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_services::diff::{DiffService, TaskWorkspaceFileFailure};
use kanvibe_services::project::adapters::LocalTaskAiSessionReader;
use kanvibe_services::project::ports::{
    AiSessionDetailRequest, AiSessionListRequest, AiSessionScope, TaskAiSessionReader,
};
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::herdr::HerdrService;
use kanvibe_terminal::running_agent_panes::MultiplexerRunningAgentPanes;
use kanvibe_terminal::session_command_runner::ShellSessionCommandRunner;

use super::data::apply_fault;
use crate::qa::faults::FaultOutcome;
use crate::services::{AppServices, ServiceFailure, ServiceResult};

/// 세션 기록 경로를 바꾸는 변수. 이 이름만 리더에 넘긴다
const PROVIDER_PATH_VARIABLES: [&str; 8] = [
    "XDG_DATA_HOME",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "GROK_HOME",
    "HERMES_HOME",
    "JCODE_HOME",
    "KIMI_CODE_HOME",
    "KIRO_HOME",
];

/// 대화 상세 한 쪽의 조건(TS `getTaskAiSessionDetail` 인자)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionDetailQuery {
    pub provider: AiSessionProvider,
    pub session_id: String,
    pub source_ref: Option<String>,
    pub cursor: Option<String>,
    pub limit: usize,
    pub query: Option<String>,
    pub roles: Option<Vec<AiMessageRole>>,
}

/// 리더와 파일 미리보기 한 벌
pub struct TaskAiSessions {
    database: Arc<Database>,
    reader: Arc<dyn TaskAiSessionReader>,
    diff: Arc<DiffService>,
}

#[derive(Default)]
struct TaskAiSessionsSlot {
    sessions: Option<Arc<TaskAiSessions>>,
}

impl Global for TaskAiSessionsSlot {}

/// 처음 부를 때 만든다. DB가 없으면 `None`이다
fn task_ai_sessions(cx: &mut App) -> Option<Arc<TaskAiSessions>> {
    if let Some(sessions) = cx
        .try_global::<TaskAiSessionsSlot>()
        .and_then(|slot| slot.sessions.clone())
    {
        return Some(sessions);
    }
    let services = cx.global::<AppServices>();
    let database = services.database()?.clone();
    let clock = services.clock;
    let sessions = Arc::new(TaskAiSessions::for_current_process(
        database,
        Arc::new(move || clock.now_ms() as f64),
    ));
    cx.set_global(TaskAiSessionsSlot {
        sessions: Some(Arc::clone(&sessions)),
    });
    Some(sessions)
}

impl TaskAiSessions {
    fn for_current_process(
        database: Arc<Database>,
        clock: Arc<dyn Fn() -> f64 + Send + Sync>,
    ) -> Self {
        let parent_environment: BTreeMap<String, String> = std::env::vars().collect();
        let home_directory = current_user_home_directory();
        let environment = Arc::new(TerminalHostEnvironment::from_parent_environment(
            &parent_environment,
            ShellPlatform::current(),
            &home_directory,
        ));
        let runner = Arc::new(SystemShellCommandRunner::for_current_process());
        let files = Arc::new(HostFileAccess::new(
            runner.clone(),
            environment.home_directory.clone(),
        ));
        let herdr = Arc::new(HerdrService::new(
            files.clone(),
            Arc::clone(&environment),
            Arc::new(SshConfigReader::new(&environment.home_directory)),
        ));
        let provider_variables = parent_environment
            .into_iter()
            .filter(|(name, _)| PROVIDER_PATH_VARIABLES.contains(&name.as_str()))
            .collect();
        let host = SessionHost::local(
            environment.home_directory.clone(),
            provider_variables,
            Arc::new(ShellSessionCommandRunner::new(runner.clone())),
        );
        let panes = Arc::new(MultiplexerRunningAgentPanes::new(runner, herdr));
        Self {
            reader: Arc::new(LocalTaskAiSessionReader::new(host, panes, clock)),
            diff: Arc::new(DiffService::new(Arc::clone(&database), files)),
            database,
        }
    }

    /// task의 worktree(없으면 프로젝트 루트)와 프로젝트 호스트. 프로젝트가 없으면 `None`
    fn scope(&self, task_id: &str) -> ServiceResult<Option<AiSessionScope>> {
        let connection = self.database.connection();
        let failure = |error: kanvibe_db::DatabaseError| ServiceFailure {
            reason: error.to_string(),
        };
        let Some(task) = tasks::find_task(&connection, task_id).map_err(failure)? else {
            return Ok(None);
        };
        let Some(project_id) = task.project_id.as_deref() else {
            return Ok(None);
        };
        let Some(project) = projects::find_project(&connection, project_id).map_err(failure)?
        else {
            return Ok(None);
        };
        Ok(Some(AiSessionScope {
            worktree_path: task
                .worktree_path
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| project.repo_path.clone()),
            repo_path: project.repo_path,
            ssh_host: project.ssh_host,
        }))
    }
}

/// 고장 판정 뒤 백그라운드에서 `work`를 돌린다. DB가 없는 실행은 `empty`를 돌려준다
fn spawn_session_call<T: Send + 'static>(
    service: &str,
    arguments: &serde_json::Value,
    cx: &mut App,
    empty: T,
    work: impl FnOnce(&TaskAiSessions) -> ServiceResult<T> + Send + 'static,
) -> Task<ServiceResult<T>> {
    let fault = cx
        .global::<AppServices>()
        .faults()
        .map(|faults| faults.check(service, &arguments.to_string()))
        .unwrap_or(FaultOutcome::Proceed);
    let sessions = task_ai_sessions(cx);
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        apply_fault(fault, &executor).await?;
        match sessions {
            Some(sessions) => work(&sessions),
            None => Ok(empty),
        }
    })
}

/// 실행중 세션(TS `getTaskLiveAiSessions(taskId).sessions`)
pub fn load_live_sessions(task_id: &str, cx: &mut App) -> Task<ServiceResult<Vec<LiveAiSession>>> {
    let task_id = task_id.to_string();
    let arguments = serde_json::json!([task_id]);
    spawn_session_call(
        "project.getTaskLiveAiSessions",
        &arguments,
        cx,
        Vec::new(),
        move |sessions| {
            Ok(match sessions.scope(&task_id)? {
                Some(scope) => sessions.reader.live_sessions(&scope),
                None => Vec::new(),
            })
        },
    )
}

/// 세션 하나의 호출 그래프
pub fn load_agent_call_graph(
    task_id: &str,
    provider: AiSessionProvider,
    session_id: &str,
    cx: &mut App,
) -> Task<ServiceResult<Option<AgentCallGraph>>> {
    let task_id = task_id.to_string();
    let session_id = session_id.to_string();
    let arguments = serde_json::json!([task_id, provider.id(), session_id]);
    spawn_session_call(
        "project.getTaskAgentCallGraph",
        &arguments,
        cx,
        None,
        move |sessions| {
            Ok(sessions.scope(&task_id)?.map(|scope| {
                sessions
                    .reader
                    .agent_call_graph(&scope, provider, &session_id)
            }))
        },
    )
}

/// 대화 기록 목록 한 쪽
pub fn load_session_list(
    task_id: &str,
    request: AiSessionListRequest,
    cx: &mut App,
) -> Task<ServiceResult<AggregatedAiSessionsResult>> {
    let task_id = task_id.to_string();
    let arguments = serde_json::json!([task_id, request.query, request.cursor, request.limit]);
    let empty = AggregatedAiSessionsResult {
        is_remote: false,
        target_path: None,
        repo_path: None,
        sessions: Vec::new(),
        sources: Vec::new(),
        next_cursor: None,
    };
    spawn_session_call(
        "project.getTaskAiSessions",
        &arguments,
        cx,
        empty.clone(),
        move |sessions| match sessions.scope(&task_id)? {
            Some(scope) => sessions
                .reader
                .aggregate_sessions(&scope, &request)
                .map_err(|reason| ServiceFailure { reason }),
            None => Ok(empty),
        },
    )
}

/// 대화 상세 한 쪽
pub fn load_session_detail(
    task_id: &str,
    query: SessionDetailQuery,
    cx: &mut App,
) -> Task<ServiceResult<Option<AggregatedAiSessionDetail>>> {
    let task_id = task_id.to_string();
    let arguments = serde_json::json!([task_id, query.provider.id(), query.session_id]);
    spawn_session_call(
        "project.getTaskAiSessionDetail",
        &arguments,
        cx,
        None,
        move |sessions| {
            let Some(scope) = sessions.scope(&task_id)? else {
                return Ok(None);
            };
            let request = AiSessionDetailRequest {
                provider: query.provider,
                session_id: query.session_id,
                source_ref: query.source_ref,
                cursor: query.cursor,
                limit: query.limit,
                query: query.query,
                roles: query.roles,
            };
            sessions
                .reader
                .session_detail(&scope, &request)
                .map_err(|reason| ServiceFailure { reason })
        },
    )
}

/// 작업 폴더 안 파일 하나의 내용. 조회 자체가 실패하면 `Unreadable`이다
pub fn read_workspace_file(
    task_id: &str,
    file_path: &str,
    cx: &mut App,
) -> Task<Result<String, TaskWorkspaceFileFailure>> {
    let task_id = task_id.to_string();
    let file_path = file_path.to_string();
    let arguments = serde_json::json!([task_id, file_path]);
    let read = spawn_session_call(
        "diff.readTaskWorkspaceFile",
        &arguments,
        cx,
        Err(TaskWorkspaceFileFailure::NotFound),
        move |sessions| {
            Ok(sessions
                .diff
                .read_task_workspace_file(&task_id, &file_path)
                .map(|file| file.content))
        },
    );
    cx.background_spawn(async move { workspace_read_outcome(read.await) })
}

/// 조회 자체가 실패하면(연결·고장) 읽지 못한 파일로 본다
fn workspace_read_outcome(
    result: ServiceResult<Result<String, TaskWorkspaceFileFailure>>,
) -> Result<String, TaskWorkspaceFileFailure> {
    result.unwrap_or(Err(TaskWorkspaceFileFailure::Unreadable))
}

#[cfg(test)]
mod tests {
    use super::*;

    // parity: TS-renderer-AiChatFilePreview-08c6f1
    #[test]
    fn failed_request_reads_as_an_unreadable_file() {
        let failed = Err(ServiceFailure {
            reason: "ssh: connection refused".into(),
        });
        assert_eq!(
            workspace_read_outcome(failed),
            Err(TaskWorkspaceFileFailure::Unreadable)
        );
        assert_eq!(
            workspace_read_outcome(Ok(Ok("본문".into()))),
            Ok("본문".into())
        );
        assert_eq!(
            workspace_read_outcome(Ok(Err(TaskWorkspaceFileFailure::TooLarge))),
            Err(TaskWorkspaceFileFailure::TooLarge)
        );
    }
}
