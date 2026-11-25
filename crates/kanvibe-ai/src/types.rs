//! AI 세션 목록·상세·실시간 세션·호출 그래프 모델. 모바일 API와 화면으로 나가는 값이라 TS와 같은 camelCase 이름을 쓴다.

use serde::{Deserialize, Serialize};

use crate::host::SessionHost;
use crate::providers::AiSessionProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiSessionMatchScope {
    Worktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMessageRole {
    User,
    Assistant,
    Tool,
    System,
    Developer,
    Reasoning,
    Unknown,
}

impl AiMessageRole {
    pub fn parse(value: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiAttachmentKind {
    Image,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMessageAttachment {
    pub id: String,
    pub kind: AiAttachmentKind,
    pub name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionAttachmentContent {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiFileChange {
    Created,
    Modified,
}

/// AI가 만들거나 고친 파일 하나. `path`는 에이전트가 기록한 그대로라 대개 절대 경로다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiMessageFile {
    pub path: String,
    pub change: AiFileChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedAiMessage {
    pub role: AiMessageRole,
    pub timestamp: Option<String>,
    pub text: String,
    pub full_text: String,
    pub is_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<AiMessageAttachment>>,
    /// 도구 호출 메시지일 때 그 도구의 이름
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// 이 도구 호출이 만들거나 고친 파일
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<AiMessageFile>>,
}

impl AggregatedAiMessage {
    /// 테스트와 리더가 쓰는 짧은 생성자. 미리보기와 전문이 같은 메시지다
    pub fn plain(role: AiMessageRole, timestamp: Option<&str>, text: &str) -> Self {
        Self {
            role,
            timestamp: timestamp.map(str::to_string),
            text: text.to_string(),
            full_text: text.to_string(),
            is_truncated: false,
            attachments: None,
            tool_name: None,
            files: None,
        }
    }

    pub fn attachment_list(&self) -> &[AiMessageAttachment] {
        self.attachments.as_deref().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedAiSession {
    pub id: String,
    pub provider: AiSessionProvider,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub matched_path: Option<String>,
    pub match_scope: AiSessionMatchScope,
    pub title: Option<String>,
    pub first_user_prompt: Option<String>,
    pub message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedAiSessionDetail {
    pub session_id: String,
    pub provider: AiSessionProvider,
    pub title: Option<String>,
    pub matched_path: Option<String>,
    pub source_ref: Option<String>,
    pub messages: Vec<AggregatedAiMessage>,
    pub next_cursor: Option<String>,
}

impl AggregatedAiSessionDetail {
    /// TS `createSessionDetail()` 기본값. provider 기본은 claude다
    pub fn empty(provider: AiSessionProvider) -> Self {
        Self {
            session_id: String::new(),
            provider,
            title: None,
            matched_path: None,
            source_ref: None,
            messages: Vec::new(),
            next_cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionSourceStatus {
    pub provider: AiSessionProvider,
    pub available: bool,
    pub session_count: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregatedAiSessionsResult {
    pub is_remote: bool,
    pub target_path: Option<String>,
    pub repo_path: Option<String>,
    pub sessions: Vec<AggregatedAiSession>,
    pub sources: Vec<AiSessionSourceStatus>,
    pub next_cursor: Option<String>,
}

/// 세션이 지금 돌고 있는지. `running`은 에이전트 프로세스가 붙어 있거나 기록이 방금 갱신됐다는 뜻이고,
/// `idle`은 세션은 남아 있지만 최근 활동이 없다는 뜻이다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LiveAiSessionState {
    Running,
    Idle,
}

/// 지금 돌고 있는 서브에이전트 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAiSubtask {
    pub id: String,
    pub name: Option<String>,
    pub last_active_at: Option<String>,
}

/// 세션이 붙어 있는 tmux window. 세션을 누르면 이 window로 전환한다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSessionTerminalWindow {
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAiSession {
    pub provider: AiSessionProvider,
    pub session_id: Option<String>,
    /// 세션이 지금 붙들고 있는 작업. 가장 최근 사용자 요청에서 뽑는다
    pub current_task: Option<String>,
    pub state: LiveAiSessionState,
    pub last_active_at: Option<String>,
    pub running_subtasks: Vec<LiveAiSubtask>,
    pub terminal_window: Option<LiveSessionTerminalWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveAiSessionsResult {
    pub task_id: String,
    pub is_remote: bool,
    pub sessions: Vec<LiveAiSession>,
}

/// 실행중 판정에 쓰는 두 시간창. `running_window_ms`는 "방금 움직였다"를,
/// `recent_window_ms`는 "유휴 상태로 보여 줄 만큼 최근이다"를 가른다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveAiSessionWindows {
    pub running_window_ms: u64,
    pub recent_window_ms: u64,
}

/// provider 리더가 돌려주는 실행중 세션 스냅샷
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveProviderSnapshot {
    pub session_id: Option<String>,
    pub current_task: Option<String>,
    pub last_active_at: Option<String>,
    pub running_subtasks: Vec<LiveAiSubtask>,
}

/// 세션이 띄운 에이전트 하나. 서브에이전트가 또 서브에이전트를 띄우므로 재귀 구조다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCallNode {
    pub id: String,
    /// Explore, general-purpose 같은 에이전트 종류
    pub agent_type: Option<String>,
    /// 그 에이전트가 따르고 있는 skill
    pub skill: Option<String>,
    /// 위임받은 작업
    pub task: Option<String>,
    pub started_at: Option<String>,
    /// 아직 돌고 있으면 `None`
    pub ended_at: Option<String>,
    pub children: Vec<AgentCallNode>,
}

/// 세션 하나의 호출 그래프. 기록 전문을 훑어야 만들 수 있어 폴링에 얹지 않고 상세보기를 열 때만 만든다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCallGraph {
    pub provider: AiSessionProvider,
    pub session_id: String,
    pub started_at: Option<String>,
    /// 그래프를 읽은 시각. 아직 도는 막대의 오른쪽 끝이 된다
    pub read_at: String,
    pub roots: Vec<AgentCallNode>,
}

/// tmux pane에서 관측한 실행중 에이전트. 보드 배지는 이 정보만으로 그린다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningAgentPane {
    pub provider: AiSessionProvider,
    pub worktree_path: String,
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
}

/// 리더 하나가 받는 조회 조건. 원격 여부와 홈 디렉터리·환경 변수 재정의는 `host`가 들고 있다
#[derive(Clone)]
pub struct AiSessionReaderContext {
    pub host: SessionHost,
    pub worktree_path: Option<String>,
    pub repo_path: Option<String>,
    pub query: Option<String>,
    pub roles: Option<Vec<AiMessageRole>>,
    /// 채팅 화면에서 지침 표시를 고를 수 있도록 자동 주입 문맥도 함께 읽는다
    pub include_instructions: bool,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

impl AiSessionReaderContext {
    pub fn new(host: SessionHost, worktree_path: Option<&str>) -> Self {
        Self {
            host,
            worktree_path: worktree_path.map(str::to_string),
            repo_path: None,
            query: None,
            roles: None,
            include_instructions: false,
            cursor: None,
            limit: None,
        }
    }

    pub fn ssh_host(&self) -> Option<&str> {
        self.host.ssh_host()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionReaderResult {
    pub provider: AiSessionProvider,
    pub available: bool,
    pub session_count: u64,
    pub reason: Option<String>,
    pub sessions: Vec<AggregatedAiSession>,
}

pub type AiSessionDetailReaderResult = AggregatedAiSessionDetail;
