//! task와 연결된 AI 세션 조회. task의 worktree(없으면 프로젝트 루트)와 프로젝트 호스트를 범위로 넘긴다.

use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AgentCallGraph, AggregatedAiSessionDetail, AggregatedAiSessionsResult,
    AiSessionAttachmentContent, LiveAiSessionsResult, RunningAgentPane,
};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;

use super::ProjectService;
use super::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest, AiSessionScope,
};
use crate::error::{ServiceError, ServiceResult};

fn session_scope(task: &KanbanTask, project: &Project) -> AiSessionScope {
    AiSessionScope {
        worktree_path: task
            .worktree_path
            .clone()
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| project.repo_path.clone()),
        repo_path: project.repo_path.clone(),
        ssh_host: project.ssh_host.clone(),
    }
}

impl ProjectService {
    fn task_session_scope(&self, task_id: &str) -> ServiceResult<Option<AiSessionScope>> {
        Ok(self
            .task_with_project(task_id)?
            .map(|(task, project)| session_scope(&task, &project)))
    }

    /// task와 연결된 AI 세션 목록. 프로젝트가 없는 task는 빈 로컬 결과다
    pub fn get_task_ai_sessions(
        &self,
        task_id: &str,
        request: &AiSessionListRequest,
    ) -> ServiceResult<AggregatedAiSessionsResult> {
        let Some(scope) = self.task_session_scope(task_id)? else {
            return Ok(AggregatedAiSessionsResult {
                is_remote: false,
                target_path: None,
                repo_path: None,
                sessions: Vec::new(),
                sources: Vec::new(),
                next_cursor: None,
            });
        };
        self.ai_sessions
            .aggregate_sessions(&scope, request)
            .map_err(ServiceError::new)
    }

    pub fn get_task_ai_session_detail(
        &self,
        task_id: &str,
        request: &AiSessionDetailRequest,
    ) -> ServiceResult<Option<AggregatedAiSessionDetail>> {
        let Some(scope) = self.task_session_scope(task_id)? else {
            return Ok(None);
        };
        self.ai_sessions
            .session_detail(&scope, request)
            .map_err(ServiceError::new)
    }

    pub fn get_task_ai_session_attachment(
        &self,
        task_id: &str,
        request: &AiSessionAttachmentRequest,
    ) -> ServiceResult<Option<AiSessionAttachmentContent>> {
        let Some(scope) = self.task_session_scope(task_id)? else {
            return Ok(None);
        };
        self.ai_sessions
            .session_attachment(&scope, request)
            .map_err(ServiceError::new)
    }

    /// task 하나의 실행중 AI 세션. 보드 카드 focus와 상세 패널이 짧은 주기로 다시 부른다
    pub fn get_task_live_ai_sessions(&self, task_id: &str) -> ServiceResult<LiveAiSessionsResult> {
        let Some(scope) = self.task_session_scope(task_id)? else {
            return Ok(LiveAiSessionsResult {
                task_id: task_id.to_string(),
                is_remote: false,
                sessions: Vec::new(),
            });
        };
        Ok(LiveAiSessionsResult {
            task_id: task_id.to_string(),
            is_remote: scope
                .ssh_host
                .as_deref()
                .is_some_and(|host| !host.is_empty()),
            sessions: self.ai_sessions.live_sessions(&scope),
        })
    }

    /// 보드 배지가 쓰는 실행중 에이전트 pane. 카드마다 세션 파일을 뒤지지 않고 pane 조회 한 번으로 끝낸다
    pub fn get_running_agent_panes(&self) -> Vec<RunningAgentPane> {
        self.ai_sessions.running_agent_panes()
    }

    /// 세션 하나의 호출 그래프. 기록 전문을 훑으므로 상세보기를 열 때만 부른다
    pub fn get_task_agent_call_graph(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> ServiceResult<Option<AgentCallGraph>> {
        Ok(self.task_session_scope(task_id)?.map(|scope| {
            self.ai_sessions
                .agent_call_graph(&scope, provider, session_id)
        }))
    }
}
