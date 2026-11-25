//! 라우트가 부르는 데스크톱 기능의 경계(TS `hookServer.js`가 라우트에 넘기던 `mobileBridge` 객체).
//!
//! TS는 서비스 모듈 여러 개를 한 객체로 펼쳐 라우트에 넘겼다. 여기서는 라우트가 실제로 부르는 메서드만 trait로 두고,
//! 결과는 TS가 `writeJson`에 넣던 값과 같은 JSON으로 받는다. 라우트는 그 값을 응답 틀에 끼우기만 하므로
//! 응답 바이트가 서비스 구현이 아니라 이 경계에서 정해진다. 메서드는 모두 블로킹이고 서버가 `spawn_blocking`에서 부른다.

use std::sync::Arc;

use serde_json::{Map, Value};

/// 라우트가 오류 이름으로 가려 응답을 고르던 TS 예외들. 메시지와 사유는 TS 문구 그대로다
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BridgeError {
    /// `MobileTaskInputError` → 400
    #[error("{0}")]
    TaskInput(String),
    /// `MobileFeatureError`. 400·404·409만 그대로 내보내고 나머지는 500이다
    #[error("{message}")]
    Feature { status: u16, message: String },
    /// `MobileSessionDependencyMissingError` → 409
    #[error("{0}")]
    SessionDependencyMissing(String),
    /// `SurfaceUnavailableError` → 409 `{reason}`
    #[error("{0}")]
    SurfaceUnavailable(String),
    /// `TaskWorkspaceFileError`(`outside-worktree`·`not-found`·`too-large`·`unreadable`)
    #[error("{0}")]
    TaskWorkspaceFile(String),
    /// `PaneChatInputError` → 400
    #[error("{0}")]
    PaneChatInput(String),
    /// `PaneChatTargetChangedError` → 409 `target-changed`
    #[error("{0}")]
    PaneChatTargetChanged(String),
    /// `PaneChatDeliveryUncertainError` → 409 `delivery-uncertain`
    #[error("{0}")]
    PaneChatDeliveryUncertain(String),
    /// AI 서비스 오류. `status`가 400·403·404·409·429면 메시지를 그대로 내보낸다
    #[error("{message}")]
    Ai {
        status: Option<u16>,
        message: String,
    },
    /// `MobileDiagramError`(400·422·429·500·503)
    #[error("{message}")]
    Diagram { status: u16, message: String },
    /// `DirtyWorktreeError`
    #[error("{0}")]
    DirtyWorktree(String),
    /// `InvalidWorktreeError`
    #[error("{0}")]
    InvalidWorktree(String),
    /// `PaneNotFoundError`
    #[error("{0}")]
    PaneNotFound(String),
    /// `LastPaneError`
    #[error("{0}")]
    LastPane(String),
    /// 이름 없는 예외. 응답에는 원문을 싣지 않고 "서버 오류"로 감춘다
    #[error("{0}")]
    Internal(String),
}

impl BridgeError {
    pub fn not_wired(method: &str) -> Self {
        Self::Internal(format!("{method} is not wired"))
    }
}

pub type BridgeResult<T> = Result<T, BridgeError>;

/// 등록을 푸는 핸들. `stop`을 부르거나 버리면 풀린다
pub struct Subscription(Option<Box<dyn FnOnce() + Send>>);

impl Subscription {
    pub fn new(stop: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(stop)))
    }

    pub fn none() -> Self {
        Self(None)
    }

    pub fn stop(mut self) {
        self.release();
    }

    fn release(&mut self) {
        if let Some(stop) = self.0.take() {
            stop();
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.release();
    }
}

pub type TextSink = Arc<dyn Fn(String) + Send + Sync>;
pub type SignalSink = Arc<dyn Fn() + Send + Sync>;
pub type EventSink = Arc<dyn Fn(Value) + Send + Sync>;
pub type DeviceSink = Arc<dyn Fn(String) + Send + Sync>;
pub type CloseSink = Arc<dyn Fn(u16, String) + Send + Sync>;

/// 모바일 크기 요청으로 잡은 pane 화면 크기. 놓으면 원래 배치로 돌아간다
pub trait PaneViewport: Send + Sync {
    /// 바뀐 크기의 첫 화면이 있으면 돌려준다
    fn resize(&self, cols: u16, rows: u16) -> BridgeResult<Option<String>>;
    /// 휠 요청. 화면이 평소 출력 흐름으로 오지 않는 세션이면 바뀐 화면을 돌려준다
    fn scroll(&self, request: &Value) -> BridgeResult<Option<String>>;
    fn release(&self) -> BridgeResult<()>;
}

/// 원격 Herdr pane을 B의 agent로 그대로 넘기는 중계
pub trait RemotePaneRelay: Send + Sync {
    fn write(&self, data: &[u8], is_binary: bool);
    fn close(&self);
}

/// AI 계정 로그인 PTY가 소켓에 보내는 이벤트
pub trait AiLoginEvents: Send + Sync {
    fn is_open(&self) -> bool;
    /// `{type:"output"|"exit"|"error", ...}`
    fn on_event(&self, event: Value);
}

/// 기능 경로(`mobileFeatureRoutes.js`) 하나. 경로·쿼리·본문을 해석한 값만 싣는다
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureCall {
    SearchTasks,
    DoneTasks {
        offset: u64,
        limit: u64,
    },
    UpdateTaskDetails {
        task_id: String,
        body: Value,
    },
    BranchTask {
        task_id: String,
        body: Value,
    },
    RefreshTaskPr {
        task_id: String,
    },
    Hosts,
    UpdateProjectColor {
        project_id: String,
        body: Value,
    },
    ProjectTasks {
        project_id: String,
    },
    ProjectBranchTask {
        project_id: String,
        branch: Option<String>,
    },
    DeleteProject {
        project_id: String,
    },
    DiffStats,
    TaskDiff {
        task_id: String,
    },
    TaskDiffFile {
        task_id: String,
        path: Option<String>,
    },
    SaveTaskDiffFile {
        task_id: String,
        path: Option<String>,
        body: Value,
    },
    RenameTab {
        task_id: String,
        tab_id: String,
        body: Value,
    },
    MoveTab {
        task_id: String,
        tab_id: String,
        body: Value,
    },
    PastePaneImage {
        task_id: String,
        pane_id: String,
        body: Value,
    },
    PaneLayouts,
    SavePaneLayout {
        body: Value,
    },
    DeletePaneLayout {
        layout_id: String,
    },
    Settings,
    UpdateSettings {
        body: Value,
    },
    SessionChangeTargets {
        session_type: Option<String>,
    },
    ChangeAllSessions {
        body: Value,
    },
    RunSync,
    SyncStatus,
    StartSync,
    SyncReview {
        notification_id: String,
    },
    DeleteSyncReviewTasks {
        notification_id: String,
        body: Value,
    },
    MarkTaskNotificationsRead {
        task_id: String,
    },
}

/// pane chat 경로의 동작(`…/pane-chat/<action>`)
#[derive(Debug, Clone, PartialEq)]
pub enum PaneChatAction {
    StageAttachment {
        attachment: Value,
    },
    RemoveAttachment {
        id: Value,
    },
    /// `attachment_ids`·`request_id`는 둘 다 없으면 TS가 인자를 넘기지 않았다
    Prompt {
        text: Value,
        extra: Option<(Value, Value)>,
    },
    Interrupt,
    ModelPicker,
    ModelSetting {
        change: Value,
    },
    Key {
        key: Value,
    },
}

/// pane chat 제안 조회 인자
#[derive(Debug, Clone, PartialEq)]
pub struct PaneChatSuggestionQuery {
    pub target_token: String,
    pub kind: Option<String>,
    pub query: String,
    pub limit: Option<f64>,
}

/// AI 대화 기록 조회 인자
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiHistoryQuery {
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub limit: u64,
    pub roles: Option<Vec<String>>,
}

/// 기기 인증·페어링·알림 구독
pub trait MobileDeviceAccess: Send + Sync {
    /// 토큰 주인 기기 ID. 없으면 `None`
    fn authorize_mobile_request(&self, authorization: Option<&str>)
    -> BridgeResult<Option<String>>;
    /// `{code, expiresAt}`
    fn start_mobile_pairing(&self) -> BridgeResult<Value>;
    fn pair_mobile_device(&self, code: &str, device_name: &str) -> BridgeResult<Option<String>>;
    fn unpair_mobile_device_by_token(&self, authorization: Option<&str>) -> BridgeResult<bool>;
    /// `subscription`은 라우트가 검증한 `{pushToken, platform, connectionId}`
    fn update_mobile_notification_subscription(
        &self,
        authorization: Option<&str>,
        subscription: Value,
    ) -> BridgeResult<bool>;
    fn delete_mobile_notification_subscription(
        &self,
        authorization: Option<&str>,
    ) -> BridgeResult<bool>;
    fn on_mobile_device_unpaired(&self, listener: DeviceSink) -> Subscription;
}

/// 모바일 HTTP 경로와 소켓이 부르는 기능 전부
pub trait MobileBridge: MobileDeviceAccess {
    fn get_mobile_board(&self, is_deduplicated: bool) -> BridgeResult<Value>;
    fn get_mobile_directories(&self, path: &str) -> BridgeResult<Value>;
    fn get_mobile_remote_directories(&self, path: &str, ssh_host: &str) -> BridgeResult<Value>;
    fn scan_mobile_projects(&self, root_path: &str) -> BridgeResult<Value>;
    fn scan_mobile_remote_projects(&self, root_path: &str, ssh_host: &Value)
    -> BridgeResult<Value>;
    fn get_mobile_notifications(&self) -> BridgeResult<Value>;
    fn get_mobile_notification_settings(&self) -> BridgeResult<Value>;
    fn update_mobile_notification_settings(&self, body: &Value) -> BridgeResult<Value>;
    fn mark_all_mobile_notifications_read(&self) -> BridgeResult<()>;
    fn mark_mobile_notification_read(&self, notification_id: &str) -> BridgeResult<Option<Value>>;
    fn on_mobile_notifications_changed(&self, listener: SignalSink) -> Subscription;
    /// 알림 서비스가 이벤트 소켓에 뿌리는 이벤트(`task-status-changed` 등)
    fn subscribe_to_mobile_events(&self, listener: EventSink) -> Subscription;

    fn is_mobile_ai_session_provider(&self, provider: &str) -> bool;
    fn get_mobile_ai_session_attachment(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
        attachment_id: &str,
        source_ref: Option<&str>,
    ) -> BridgeResult<Option<Value>>;
    fn get_mobile_task_file(&self, task_id: &str, file_path: &str) -> BridgeResult<Value>;
    fn get_mobile_task(&self, task_id: &str) -> BridgeResult<Option<Value>>;
    fn get_mobile_ai_sessions(&self, task_id: &str, query: &AiHistoryQuery) -> BridgeResult<Value>;
    fn get_mobile_ai_session_detail(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
        query: &AiHistoryQuery,
    ) -> BridgeResult<Option<Value>>;

    fn create_mobile_task(&self, draft: &Value) -> BridgeResult<Value>;
    fn update_mobile_task_status(&self, task_id: &str, status: &str)
    -> BridgeResult<Option<Value>>;
    fn change_mobile_task_session_type(
        &self,
        task_id: &str,
        session_type: &str,
        should_close_existing_session: bool,
    ) -> BridgeResult<Option<Value>>;
    fn is_mobile_task_session_alive(&self, task_id: &str) -> BridgeResult<bool>;
    fn get_mobile_project_branches(&self, project_id: &str) -> BridgeResult<Value>;
    /// 결과 객체의 필드를 `{success:true}` 옆에 펼친다
    fn run_mobile_feature(&self, call: FeatureCall) -> BridgeResult<Map<String, Value>>;
    /// `{imageBase64, width, height, mimeType, naturalWidth?}`
    fn render_mobile_diagram(&self, body: &Value) -> BridgeResult<Map<String, Value>>;

    fn get_mobile_running_agent_panes(&self) -> BridgeResult<Value>;
    fn get_mobile_live_ai_sessions(&self, task_id: &str) -> BridgeResult<Value>;
    fn get_mobile_agent_call_graph(
        &self,
        task_id: &str,
        provider: &str,
        session_id: &str,
    ) -> BridgeResult<Value>;
    fn list_mobile_ai_accounts(&self) -> BridgeResult<Value>;
    fn add_mobile_ai_account(&self, body: &Value) -> BridgeResult<Map<String, Value>>;
    fn remove_mobile_ai_account(&self, account_id: &str) -> BridgeResult<Map<String, Value>>;
    fn get_mobile_ai_usage_snapshot(&self) -> BridgeResult<Value>;
    fn open_mobile_ai_login(
        &self,
        device_id: &str,
        socket_id: &str,
        account_id: &str,
        size: (u16, u16),
        events: Arc<dyn AiLoginEvents>,
    ) -> BridgeResult<()>;
    fn write_mobile_ai_login(&self, device_id: &str, socket_id: &str, data: &str);
    fn resize_mobile_ai_login(&self, device_id: &str, socket_id: &str, cols: u16, rows: u16);
    fn close_mobile_ai_login(&self, device_id: &str, socket_id: &str);
    fn close_mobile_ai_logins_for_device(&self, device_id: &str);
    fn close_all_mobile_ai_logins(&self);

    fn get_pane_chat_suggestions(
        &self,
        task_id: &str,
        query: &PaneChatSuggestionQuery,
    ) -> BridgeResult<Map<String, Value>>;
    fn get_pane_chat_agent_provider(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value>;
    fn get_sequenced_pane_chat_snapshot(
        &self,
        task_id: &str,
        pane_id: Option<&str>,
        should_include_screen: bool,
    ) -> BridgeResult<Value>;
    /// 응답 본문 전체(`{success:true, ...}`)를 돌려준다
    fn run_pane_chat_action(
        &self,
        task_id: &str,
        target_token: &str,
        action: PaneChatAction,
    ) -> BridgeResult<Value>;
    /// `[{taskId, paneId, freshness}]`
    fn set_pane_chat_subscriptions(
        &self,
        owner: u64,
        targets: &Value,
        send: EventSink,
    ) -> BridgeResult<Value>;
    fn clear_pane_chat_subscriptions(&self, owner: u64);
    fn get_pane_chat_modes(&self, task_id: &str) -> BridgeResult<Value>;
    fn set_pane_chat_mode(
        &self,
        task_id: &str,
        pane_id: &str,
        is_enabled: bool,
    ) -> BridgeResult<()>;

    /// `None`이면 이 데스크탑이 직접 연결 권한을 낼 수 없다(TS `typeof … !== "function"`)
    fn prepare_mobile_pane_connection(
        &self,
        task_id: &str,
        pane_id: &str,
        device_id: &str,
    ) -> BridgeResult<Value>;
    fn get_task_surfaces(
        &self,
        task_id: &str,
        minimal: Option<Option<String>>,
    ) -> BridgeResult<Value>;
    fn execute_mobile_terminal_shortcut(
        &self,
        task_id: &str,
        pane_id: &str,
        action: &str,
    ) -> BridgeResult<Map<String, Value>>;
    /// `{surfaces, paneId}`
    fn create_mobile_terminal_tab(&self, task_id: &str) -> BridgeResult<Map<String, Value>>;
    fn get_mobile_pane_close_info(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value>;
    fn close_mobile_pane(&self, task_id: &str, pane_id: &str) -> BridgeResult<Value>;
    /// 지운 태스크가 없으면 `None`, 있으면 전파받지 못한 데스크탑 수
    fn delete_mobile_task(&self, task_id: &str, is_forced: bool) -> BridgeResult<Option<u64>>;
    fn remove_mobile_task_registration(&self, task_id: &str) -> BridgeResult<bool>;

    fn watch_task_session_change(&self, task_id: &str, on_changed: SignalSink) -> Subscription;
    fn subscribe_to_pane(
        &self,
        task_id: &str,
        pane_id: &str,
        on_chunk: TextSink,
        on_end: SignalSink,
    ) -> BridgeResult<Subscription>;
    fn claim_pane_viewport(
        &self,
        task_id: &str,
        pane_id: &str,
    ) -> BridgeResult<Box<dyn PaneViewport>>;
    fn refresh_pane_history(&self, task_id: &str, pane_id: &str) -> BridgeResult<()>;
    fn write_to_pane(&self, task_id: &str, pane_id: &str, input: &str) -> BridgeResult<()>;
    /// 원격 Herdr가 아니면 `None`을 돌려 로컬 미러로 비춘다
    fn open_remote_mobile_pane_stream(
        &self,
        target: RemotePaneTarget,
        on_chunk: TextSink,
        on_close: CloseSink,
    ) -> BridgeResult<Option<Box<dyn RemotePaneRelay>>>;
}

/// 원격 pane 중계를 열 대상
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePaneTarget {
    pub task_id: String,
    pub pane_id: String,
    pub device_id: String,
    pub grant_id: Option<String>,
}
