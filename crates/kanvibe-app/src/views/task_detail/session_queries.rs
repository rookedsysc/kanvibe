//! 작업 상세의 AI 세션 조회(TS `project.getTaskLiveAiSessions`·`getTaskAgentCallGraph`·`getTaskAiSessions`·
//! `getTaskAiSessionDetail`, `diff.readTaskWorkspaceFile`). 런타임의 `ProjectService`·`DiffService`를 부른다.

use std::sync::Arc;

use gpui_kit::{App, AppContext, Task};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult, AiMessageRole,
    AiSessionAttachmentContent, LiveAiSession,
};
use kanvibe_runtime::DesktopRuntime;
use kanvibe_services::diff::TaskWorkspaceFileFailure;
use kanvibe_services::project::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest,
};

use crate::services::{AppServices, ServiceFailure, ServiceResult, apply_fault};

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

fn service_failure(error: impl ToString) -> ServiceFailure {
    ServiceFailure {
        reason: error.to_string(),
    }
}

/// 고장 판정 뒤 백그라운드에서 `work`를 돌린다. DB가 없는 실행은 `empty`를 돌려준다
fn spawn_session_call<T: Send + 'static>(
    service: &str,
    arguments: &serde_json::Value,
    cx: &mut App,
    empty: T,
    work: impl FnOnce(&DesktopRuntime) -> ServiceResult<T> + Send + 'static,
) -> Task<ServiceResult<T>> {
    let services = cx.global::<AppServices>();
    let fault = services.check_fault(service, &arguments.to_string());
    let runtime = services.runtime().map(Arc::clone);
    let executor = cx.background_executor().clone();
    cx.background_spawn(async move {
        apply_fault(fault, &executor).await?;
        match runtime {
            Some(runtime) => work(&runtime),
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
        move |runtime| {
            runtime
                .project()
                .get_task_live_ai_sessions(&task_id)
                .map(|result| result.sessions)
                .map_err(service_failure)
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
        move |runtime| {
            runtime
                .project()
                .get_task_agent_call_graph(&task_id, provider, &session_id)
                .map_err(service_failure)
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
        empty,
        move |runtime| {
            runtime
                .project()
                .get_task_ai_sessions(&task_id, &request)
                .map_err(service_failure)
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
        move |runtime| {
            let request = AiSessionDetailRequest {
                provider: query.provider,
                session_id: query.session_id,
                source_ref: query.source_ref,
                cursor: query.cursor,
                limit: query.limit,
                query: query.query,
                roles: query.roles,
            };
            runtime
                .project()
                .get_task_ai_session_detail(&task_id, &request)
                .map_err(service_failure)
        },
    )
}

/// 대화 첨부 하나의 내용(TS `getTaskAiSessionAttachment`). 없거나 못 읽으면 `None`
pub fn load_session_attachment(
    task_id: &str,
    request: AiSessionAttachmentRequest,
    cx: &mut App,
) -> Task<ServiceResult<Option<AiSessionAttachmentContent>>> {
    let task_id = task_id.to_string();
    let arguments = serde_json::json!([
        task_id,
        request.provider.id(),
        request.session_id,
        request.attachment_id,
        request.source_ref
    ]);
    spawn_session_call(
        "project.getTaskAiSessionAttachment",
        &arguments,
        cx,
        None,
        move |runtime| {
            runtime
                .project()
                .get_task_ai_session_attachment(&task_id, &request)
                .map_err(service_failure)
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
        move |runtime| {
            Ok(runtime
                .diff()
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
