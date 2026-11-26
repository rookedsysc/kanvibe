//! 서비스 테스트 공용 가짜와 TS `beforeEach` 기본값.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_ai::pane_chat::model_settings::{PaneChatModelSettingChange, PaneChatModelSettings};
use kanvibe_ai::pane_chat::process::{PaneChatProcess, PaneChatProvider};
use kanvibe_ai::pane_chat::session_bindings::{PaneChatBindingQuery, PaneChatSessionBinding};
use kanvibe_ai::pane_chat::snapshot::PaneChatLayout;
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{AggregatedAiMessage, AggregatedAiSessionDetail, AiMessageRole};
use kanvibe_domain::pane_chat::FilteredPaneChatSuggestions;
use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::TerminalTab;
use kanvibe_process::exec::ExecGitError;
use kanvibe_process::ssh_config::{SshConfigReader, SshHostConfig};
use serde_json::{Value, json};

use super::super::dependencies::{
    BeforeSend, CodexThreadAddress, CodexThreadDelivery, PaneChatClock, PaneChatReaderScope,
    PaneChatSessionRecords, PaneChatSuggestionSource,
};
use super::super::{PaneChatError, PaneChatService, PaneChatServiceDependencies};
use crate::pane_chat::attachments::{PaneChatAttachmentStore, RemoteAttachmentUploader};
use crate::pane_chat::composer_catalog::{PaneChatCatalogContext, PaneChatSuggestionKind};
use crate::pane_chat::surface::{
    HerdrPaneFrame, PaneChatMobileBridge, PaneChatSurface, PaneChatSurfaceError, PaneChatSurfaces,
    PaneChatTask, PaneChatTerminals,
};
use crate::terminal_registry::{HerdrSurfaceBounds, TaskTerminalAttachment, TerminalTabSnapshot};
use crate::test_support::ScriptedShellRunner;

pub(crate) type Shared<T> = Mutex<Arc<T>>;

fn shared<T: ?Sized>(value: Arc<T>) -> Shared<T> {
    Mutex::new(value)
}

fn current<T: ?Sized>(slot: &Shared<T>) -> Arc<T> {
    slot.lock().unwrap().clone()
}

/// TS `const surface` 기본값의 태스크
pub(crate) fn task() -> PaneChatTask {
    PaneChatTask {
        id: "task-chat".to_string(),
        session_type: Some(SessionType::Tmux),
        session_name: Some("kv-task".to_string()),
        ssh_host: None,
        worktree_path: Some("/repo".to_string()),
        project_repo_path: Some("/repo".to_string()),
    }
}

/// TS `const surface` 기본값. 커서 행과 화면 줄은 없다
pub(crate) fn surface() -> PaneChatSurface {
    PaneChatSurface {
        task: task(),
        pane_id: "%3".to_string(),
        root_pid: Some(100),
        agent_session: None,
        screen: "CLI screen".to_string(),
        input_line: Some("› ".to_string()),
        cursor_column: Some(2),
        cursor_row: None,
        viewport_lines: None,
        viewport_columns: None,
        is_input_placeholder: false,
        observation: None,
    }
}

/// `viewport_lines`를 `&str` 목록으로 쉽게 넣는다
pub(crate) fn lines(rows: &[&str]) -> Option<Vec<String>> {
    Some(rows.iter().map(|row| row.to_string()).collect())
}

/// pane root의 tty로 좁힌 프로세스 조회 명령인지. 다른 `ps` 호출(예: tty 단독 조회)과 구분한다
pub(crate) fn is_pane_process_table_command(command: &str) -> bool {
    command.contains("ucomm=,args=")
}

/// TS `processTable(start)`. zsh 아래 codex(파이썬 래퍼)와 claude가 있다
pub(crate) fn process_table(start: &str) -> String {
    format!(
        "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh\n200 100 200 200 ttys010 {start} /usr/bin/python3 python3 /tmp/codex\n300 100 300 200 ttys010 Thu Nov 12 18:00:00 2026 /tmp/claude /tmp/claude"
    )
}

pub(crate) const DEFAULT_PROCESS_START: &str = "Thu Nov 12 19:00:00 2026";

pub(crate) fn binding(session_id: &str, source_ref: &str) -> PaneChatSessionBinding {
    PaneChatSessionBinding {
        session_id: session_id.to_string(),
        source_ref: source_ref.to_string(),
        revision: None,
        is_pending: false,
        is_empty: false,
    }
}

pub(crate) fn message(role: AiMessageRole, text: &str) -> AggregatedAiMessage {
    AggregatedAiMessage::plain(role, None, text)
}

pub(crate) fn detail(
    session_id: &str,
    provider: AiSessionProvider,
    messages: Vec<AggregatedAiMessage>,
    next_cursor: Option<&str>,
) -> AggregatedAiSessionDetail {
    AggregatedAiSessionDetail {
        session_id: session_id.to_string(),
        provider,
        title: None,
        matched_path: None,
        source_ref: None,
        messages,
        next_cursor: next_cursor.map(str::to_string),
    }
}

/// TS beforeEach의 `readTextFile` 기본값. Codex 모델 캐시 파일이다
pub(crate) fn models_cache_json() -> String {
    json!({ "models": [
        { "slug": "gpt-sol", "display_name": "GPT Sol", "visibility": "list", "priority": 1, "supported_reasoning_levels": [{ "effort": "low" }, { "effort": "high" }], "service_tiers": [{ "id": "priority" }] },
        { "slug": "gpt-luna", "display_name": "GPT Luna", "visibility": "list", "priority": 2, "supported_reasoning_levels": [], "service_tiers": [] },
    ] })
    .to_string()
}

pub(crate) fn bracketed(text: &str) -> String {
    format!("\x1b[200~{text}\x1b[201~")
}

// ---- surface ----

type SurfaceResponder = dyn Fn(&str, Option<&str>, bool) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError>
    + Send
    + Sync;
type ScreenResponder =
    dyn Fn(&PaneChatSurface) -> Result<String, PaneChatSurfaceError> + Send + Sync;

/// `readPaneChatSurface`·`readPaneChatScreen`·`readPaneChatLayout` 가짜
pub(crate) struct FakeSurfaces {
    responder: Shared<SurfaceResponder>,
    calls: Mutex<Vec<(String, Option<String>, bool)>>,
    screen: Shared<ScreenResponder>,
    screen_calls: Mutex<Vec<String>>,
    layout: Mutex<Result<PaneChatLayout, PaneChatSurfaceError>>,
    layout_calls: Mutex<Vec<(String, Option<String>)>>,
}

impl FakeSurfaces {
    fn new() -> Self {
        let screen: Arc<ScreenResponder> =
            Arc::new(|surface: &PaneChatSurface| Ok(surface.screen.clone()));
        Self {
            responder: shared(
                Arc::new(|_: &str, _: Option<&str>, _: bool| Ok(Some(surface())))
                    as Arc<SurfaceResponder>,
            ),
            calls: Mutex::new(Vec::new()),
            screen: shared(screen),
            screen_calls: Mutex::new(Vec::new()),
            layout: Mutex::new(Ok(PaneChatLayout {
                active_pane_id: None,
                panes: Vec::new(),
            })),
            layout_calls: Mutex::new(Vec::new()),
        }
    }

    /// TS `mocks.surface.mockResolvedValue(value)`
    pub(crate) fn set(&self, value: Option<PaneChatSurface>) {
        self.respond(move |_, _, _| Ok(value.clone()));
    }

    /// TS `mocks.surface.mockImplementation(...)`
    pub(crate) fn respond(
        &self,
        responder: impl Fn(
            &str,
            Option<&str>,
            bool,
        ) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError>
        + Send
        + Sync
        + 'static,
    ) {
        *self.responder.lock().unwrap() = Arc::new(responder);
    }

    pub(crate) fn calls(&self) -> Vec<(String, Option<String>, bool)> {
        self.calls.lock().unwrap().clone()
    }

    pub(crate) fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }

    pub(crate) fn respond_screen(
        &self,
        responder: impl Fn(&PaneChatSurface) -> Result<String, PaneChatSurfaceError>
        + Send
        + Sync
        + 'static,
    ) {
        *self.screen.lock().unwrap() = Arc::new(responder);
    }

    /// `read_pane_chat_screen`이 받은 surface의 pane 식별자들
    pub(crate) fn screen_calls(&self) -> Vec<String> {
        self.screen_calls.lock().unwrap().clone()
    }

    pub(crate) fn set_layout(&self, layout: Result<PaneChatLayout, PaneChatSurfaceError>) {
        *self.layout.lock().unwrap() = layout;
    }

    pub(crate) fn layout_calls(&self) -> Vec<(String, Option<String>)> {
        self.layout_calls.lock().unwrap().clone()
    }
}

impl PaneChatSurfaces for FakeSurfaces {
    fn read_pane_chat_surface(
        &self,
        task_id: &str,
        requested_pane_id: Option<&str>,
        should_capture: bool,
    ) -> Result<Option<PaneChatSurface>, PaneChatSurfaceError> {
        self.calls.lock().unwrap().push((
            task_id.to_string(),
            requested_pane_id.map(str::to_string),
            should_capture,
        ));
        current(&self.responder)(task_id, requested_pane_id, should_capture)
    }

    fn read_pane_chat_screen(
        &self,
        surface: &PaneChatSurface,
    ) -> Result<String, PaneChatSurfaceError> {
        self.screen_calls
            .lock()
            .unwrap()
            .push(surface.pane_id.clone());
        current(&self.screen)(surface)
    }

    fn read_pane_chat_layout(
        &self,
        task_id: &str,
        terminal_tab_id: Option<&str>,
    ) -> Result<PaneChatLayout, PaneChatSurfaceError> {
        self.layout_calls
            .lock()
            .unwrap()
            .push((task_id.to_string(), terminal_tab_id.map(str::to_string)));
        self.layout.lock().unwrap().clone()
    }
}

// ---- terminals ----

/// `@/lib/terminal`의 `focusHerdrTaskPane`·`listLocalTerminalTabs` 가짜
#[derive(Default)]
pub(crate) struct FakeTerminals {
    pub tabs: Mutex<Vec<TerminalTab>>,
    pub focused_herdr_panes: Mutex<Vec<(String, String)>>,
}

pub(crate) fn terminal_tab(id: &str, is_active: bool) -> TerminalTab {
    TerminalTab {
        id: id.to_string(),
        native_index: 0,
        name: id.to_string(),
        is_active,
    }
}

impl PaneChatTerminals for FakeTerminals {
    fn list_local_terminal_tabs(&self, _task_id: &str) -> Vec<TerminalTab> {
        self.tabs.lock().unwrap().clone()
    }

    fn read_local_terminal_tab_snapshot(
        &self,
        _task_id: &str,
        _tab_id: &str,
    ) -> Option<TerminalTabSnapshot> {
        None
    }

    fn read_task_terminal_attachment(&self, _task_id: &str) -> Option<TaskTerminalAttachment> {
        None
    }

    fn herdr_task_active_pane_id(&self, _task_id: &str) -> Option<String> {
        None
    }

    fn herdr_task_surface_bounds(&self, _task_id: &str) -> Option<HerdrSurfaceBounds> {
        None
    }

    fn focus_herdr_task_pane(&self, task_id: &str, pane_id: &str) -> Result<(), String> {
        self.focused_herdr_panes
            .lock()
            .unwrap()
            .push((task_id.to_string(), pane_id.to_string()));
        Ok(())
    }
}

// ---- writeToPane ----

type WriteHook = dyn Fn(&str, &str, &str) -> Result<(), String> + Send + Sync;

/// `mobileBridgeService.writeToPane` 가짜. 실패한 호출도 vitest처럼 기록한다
pub(crate) struct FakeBridge {
    writes: Mutex<Vec<(String, String, String)>>,
    hook: Shared<WriteHook>,
    failures: Mutex<VecDeque<String>>,
}

impl FakeBridge {
    fn new() -> Self {
        Self {
            writes: Mutex::new(Vec::new()),
            hook: shared(Arc::new(|_: &str, _: &str, _: &str| Ok(())) as Arc<WriteHook>),
            failures: Mutex::new(VecDeque::new()),
        }
    }

    /// TS `mocks.write.mockImplementation(...)`
    pub(crate) fn respond(
        &self,
        hook: impl Fn(&str, &str, &str) -> Result<(), String> + Send + Sync + 'static,
    ) {
        *self.hook.lock().unwrap() = Arc::new(hook);
    }

    /// TS `mocks.write.mockRejectedValueOnce(new Error(message))`
    pub(crate) fn fail_once(&self, message: &str) {
        self.failures.lock().unwrap().push_back(message.to_string());
    }

    pub(crate) fn writes(&self) -> Vec<(String, String, String)> {
        self.writes.lock().unwrap().clone()
    }

    /// 쓴 바이트만
    pub(crate) fn written_bytes(&self) -> Vec<String> {
        self.writes()
            .into_iter()
            .map(|(_, _, bytes)| bytes)
            .collect()
    }

    pub(crate) fn clear(&self) {
        self.writes.lock().unwrap().clear();
    }
}

impl PaneChatMobileBridge for FakeBridge {
    fn read_herdr_pane_frame_for_chat(
        &self,
        _task_id: &str,
        _pane_id: &str,
    ) -> Result<HerdrPaneFrame, String> {
        Err("Herdr frame unavailable".to_string())
    }

    fn write_to_pane(&self, task_id: &str, pane_id: &str, bytes: &str) -> Result<(), String> {
        self.writes.lock().unwrap().push((
            task_id.to_string(),
            pane_id.to_string(),
            bytes.to_string(),
        ));
        if let Some(message) = self.failures.lock().unwrap().pop_front() {
            return Err(message);
        }
        current(&self.hook)(task_id, pane_id, bytes)
    }
}

// ---- session records ----

type BindingResponder =
    dyn Fn(&PaneChatBindingQuery) -> Result<Option<PaneChatSessionBinding>, String> + Send + Sync;
type ProcessBindingResponder =
    dyn Fn(&PaneChatReaderScope, &PaneChatProcess) -> Option<PaneChatSessionBinding> + Send + Sync;
type ReportedBindingResponder = dyn Fn(
        &PaneChatReaderScope,
        PaneChatProvider,
        &str,
    ) -> Result<Option<PaneChatSessionBinding>, String>
    + Send
    + Sync;
type DetailResponder =
    dyn Fn(&DetailCall) -> Result<Option<AggregatedAiSessionDetail>, String> + Send + Sync;
type RolloutResponder =
    dyn Fn(&PaneChatReaderScope, &str) -> Result<Option<String>, String> + Send + Sync;
type EmptyThreadResponder =
    dyn Fn(&PaneChatReaderScope, &str, &Option<Option<String>>) -> Option<String> + Send + Sync;
type ApiInputResponder = dyn Fn(&CodexThreadDelivery, BeforeSend<'_>, &mut dyn FnMut()) -> Result<(), PaneChatError>
    + Send
    + Sync;
type ThreadSettingsResponder =
    dyn Fn(&CodexThreadAddress) -> Result<PaneChatModelSettings, String> + Send + Sync;
type RecordsResponder = dyn Fn(&str, Option<&str>) -> Result<Vec<Value>, String> + Send + Sync;
type TextResponder = dyn Fn(&str, Option<&str>) -> Result<String, String> + Send + Sync;
type ExistsResponder = dyn Fn(&str, Option<&str>) -> Result<bool, String> + Send + Sync;

/// 대화 기록 조회 한 번(TS `reader(context, sessionId, sourceRef, cursor, limit)`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetailCall {
    pub provider: PaneChatProvider,
    pub scope: PaneChatReaderScope,
    pub session_id: String,
    pub source_ref: String,
    pub cursor: Option<String>,
    pub limit: usize,
}

/// 세션 리더·바인딩·Codex app-server 가짜. TS의 `mocks.binding`, `mocks.claude` 등을 한곳에 모았다
pub(crate) struct FakeSessions {
    pub binding: Shared<BindingResponder>,
    pub process_binding: Shared<ProcessBindingResponder>,
    pub reported_binding: Shared<ReportedBindingResponder>,
    pub detail: Shared<DetailResponder>,
    pub detail_calls: Mutex<Vec<DetailCall>>,
    pub rollout: Shared<RolloutResponder>,
    pub empty_thread: Shared<EmptyThreadResponder>,
    pub api_input: Shared<ApiInputResponder>,
    pub api_input_calls: Mutex<Vec<CodexThreadDelivery>>,
    pub thread_settings: Shared<ThreadSettingsResponder>,
    pub thread_settings_calls: Mutex<Vec<CodexThreadAddress>>,
    pub settings_updates: Mutex<Vec<(CodexThreadAddress, PaneChatModelSettingChange)>>,
    /// TS `modelMocks.tail`(기록 끝 200줄)
    pub tail: Shared<RecordsResponder>,
    pub tail_calls: Mutex<Vec<String>>,
    pub head: Shared<RecordsResponder>,
    pub read_text_file: Shared<TextResponder>,
    pub path_exists: Shared<ExistsResponder>,
}

impl FakeSessions {
    fn new() -> Self {
        let detail: Arc<DetailResponder> = Arc::new(|call: &DetailCall| {
            let provider = match call.provider {
                PaneChatProvider::Claude => AiSessionProvider::Claude,
                PaneChatProvider::Codex => AiSessionProvider::Codex,
                PaneChatProvider::Gemini => AiSessionProvider::Gemini,
                PaneChatProvider::Opencode => AiSessionProvider::Opencode,
            };
            let is_default_reader = matches!(
                call.provider,
                PaneChatProvider::Claude | PaneChatProvider::Codex
            );
            Ok(is_default_reader.then(|| detail("exact-session", provider, Vec::new(), None)))
        });
        let api_input: Arc<ApiInputResponder> = Arc::new(
            |_: &CodexThreadDelivery, before_send: BeforeSend<'_>, on_write: &mut dyn FnMut()| {
                before_send()?;
                on_write();
                Ok(())
            },
        );
        Self {
            binding: shared(Arc::new(|_: &PaneChatBindingQuery| {
                Ok(Some(binding("exact-session", "/allowed/exact.jsonl")))
            }) as Arc<BindingResponder>),
            process_binding: shared(
                Arc::new(|_: &PaneChatReaderScope, _: &PaneChatProcess| None)
                    as Arc<ProcessBindingResponder>,
            ),
            reported_binding: shared(Arc::new(
                |_: &PaneChatReaderScope, _: PaneChatProvider, _: &str| Ok(None),
            ) as Arc<ReportedBindingResponder>),
            detail: shared(detail),
            detail_calls: Mutex::new(Vec::new()),
            rollout: shared(
                Arc::new(|_: &PaneChatReaderScope, _: &str| Ok(None)) as Arc<RolloutResponder>
            ),
            empty_thread: shared(Arc::new(
                |_: &PaneChatReaderScope, _: &str, _: &Option<Option<String>>| None,
            ) as Arc<EmptyThreadResponder>),
            api_input: shared(api_input),
            api_input_calls: Mutex::new(Vec::new()),
            thread_settings: shared(Arc::new(|_: &CodexThreadAddress| {
                Err("thread settings unavailable".to_string())
            }) as Arc<ThreadSettingsResponder>),
            thread_settings_calls: Mutex::new(Vec::new()),
            settings_updates: Mutex::new(Vec::new()),
            tail: shared(
                Arc::new(|_: &str, _: Option<&str>| Err("No transcript tail".to_string()))
                    as Arc<RecordsResponder>,
            ),
            tail_calls: Mutex::new(Vec::new()),
            head: shared(
                Arc::new(|_: &str, _: Option<&str>| Ok(Vec::new())) as Arc<RecordsResponder>
            ),
            read_text_file: shared(
                Arc::new(|_: &str, _: Option<&str>| Ok(models_cache_json())) as Arc<TextResponder>
            ),
            path_exists: shared(Arc::new(|path: &str, _: Option<&str>| {
                Ok(std::path::Path::new(path).exists())
            }) as Arc<ExistsResponder>),
        }
    }

    /// TS `mocks.binding.mockResolvedValue(value)`
    pub(crate) fn set_binding(&self, value: Option<PaneChatSessionBinding>) {
        *self.binding.lock().unwrap() = Arc::new(move |_: &PaneChatBindingQuery| Ok(value.clone()));
    }

    /// TS `mocks.processBinding.mockResolvedValue(value)`
    pub(crate) fn set_process_binding(&self, value: Option<PaneChatSessionBinding>) {
        *self.process_binding.lock().unwrap() =
            Arc::new(move |_: &PaneChatReaderScope, _: &PaneChatProcess| value.clone());
    }

    /// 대화 기록 리더 응답. provider마다 다르게 답하려면 `call.provider`를 본다
    pub(crate) fn respond_detail(
        &self,
        responder: impl Fn(&DetailCall) -> Result<Option<AggregatedAiSessionDetail>, String>
        + Send
        + Sync
        + 'static,
    ) {
        *self.detail.lock().unwrap() = Arc::new(responder);
    }

    pub(crate) fn detail_calls(&self) -> Vec<DetailCall> {
        self.detail_calls.lock().unwrap().clone()
    }

    pub(crate) fn respond_rollout(
        &self,
        responder: impl Fn(&PaneChatReaderScope, &str) -> Result<Option<String>, String>
        + Send
        + Sync
        + 'static,
    ) {
        *self.rollout.lock().unwrap() = Arc::new(responder);
    }

    pub(crate) fn respond_api_input(
        &self,
        responder: impl Fn(
            &CodexThreadDelivery,
            BeforeSend<'_>,
            &mut dyn FnMut(),
        ) -> Result<(), PaneChatError>
        + Send
        + Sync
        + 'static,
    ) {
        *self.api_input.lock().unwrap() = Arc::new(responder);
    }

    pub(crate) fn api_input_calls(&self) -> Vec<CodexThreadDelivery> {
        self.api_input_calls.lock().unwrap().clone()
    }

    pub(crate) fn respond_thread_settings(
        &self,
        responder: impl Fn(&CodexThreadAddress) -> Result<PaneChatModelSettings, String>
        + Send
        + Sync
        + 'static,
    ) {
        *self.thread_settings.lock().unwrap() = Arc::new(responder);
    }

    /// TS `modelMocks.tail.mockResolvedValue(records)`
    pub(crate) fn set_tail(&self, records: Result<Vec<Value>, String>) {
        *self.tail.lock().unwrap() = Arc::new(move |_: &str, _: Option<&str>| records.clone());
    }

    pub(crate) fn respond_tail(
        &self,
        responder: impl Fn(&str, Option<&str>) -> Result<Vec<Value>, String> + Send + Sync + 'static,
    ) {
        *self.tail.lock().unwrap() = Arc::new(responder);
    }

    pub(crate) fn tail_calls(&self) -> Vec<String> {
        self.tail_calls.lock().unwrap().clone()
    }
}

impl PaneChatSessionRecords for FakeSessions {
    fn read_session_detail(
        &self,
        provider: PaneChatProvider,
        scope: &PaneChatReaderScope,
        session_id: &str,
        source_ref: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Option<AggregatedAiSessionDetail>, String> {
        let call = DetailCall {
            provider,
            scope: scope.clone(),
            session_id: session_id.to_string(),
            source_ref: source_ref.to_string(),
            cursor: cursor.map(str::to_string),
            limit,
        };
        self.detail_calls.lock().unwrap().push(call.clone());
        current(&self.detail)(&call)
    }

    fn find_codex_main_thread_rollout(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
    ) -> Result<Option<String>, String> {
        current(&self.rollout)(scope, thread_id)
    }

    fn read_codex_empty_thread(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
        socket: &Option<Option<String>>,
    ) -> Option<String> {
        current(&self.empty_thread)(scope, thread_id, socket)
    }

    fn send_codex_thread_input(
        &self,
        delivery: &CodexThreadDelivery,
        before_send: BeforeSend<'_>,
        on_write: &mut dyn FnMut(),
    ) -> Result<(), PaneChatError> {
        self.api_input_calls.lock().unwrap().push(delivery.clone());
        current(&self.api_input)(delivery, before_send, on_write)
    }

    fn read_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
    ) -> Result<PaneChatModelSettings, String> {
        self.thread_settings_calls
            .lock()
            .unwrap()
            .push(address.clone());
        current(&self.thread_settings)(address)
    }

    fn update_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
        change: &PaneChatModelSettingChange,
        before_write: BeforeSend<'_>,
    ) -> Result<(), PaneChatError> {
        before_write()?;
        self.settings_updates
            .lock()
            .unwrap()
            .push((address.clone(), change.clone()));
        Ok(())
    }

    fn read_recent_records(
        &self,
        source_ref: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        self.tail_calls.lock().unwrap().push(source_ref.to_string());
        current(&self.tail)(source_ref, ssh_host)
    }

    fn read_json_lines_head(
        &self,
        file_path: &str,
        _max_lines: usize,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        current(&self.head)(file_path, ssh_host)
    }

    fn home_directory(&self, _ssh_host: Option<&str>) -> Result<String, String> {
        Ok("/home".to_string())
    }

    fn path_exists(&self, target_path: &str, ssh_host: Option<&str>) -> Result<bool, String> {
        current(&self.path_exists)(target_path, ssh_host)
    }

    fn read_text_file(&self, target_path: &str, ssh_host: Option<&str>) -> Result<String, String> {
        current(&self.read_text_file)(target_path, ssh_host)
    }

    fn find_session_binding(
        &self,
        query: &PaneChatBindingQuery,
    ) -> Result<Option<PaneChatSessionBinding>, String> {
        current(&self.binding)(query)
    }

    fn find_process_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        process: &PaneChatProcess,
    ) -> Option<PaneChatSessionBinding> {
        current(&self.process_binding)(scope, process)
    }

    fn find_reported_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        provider: PaneChatProvider,
        session_id: &str,
    ) -> Result<Option<PaneChatSessionBinding>, String> {
        current(&self.reported_binding)(scope, provider, session_id)
    }
}

// ---- catalog ----

type CatalogResponder = dyn Fn(
        &PaneChatCatalogContext,
        PaneChatSuggestionKind,
        &str,
        usize,
    ) -> Result<FilteredPaneChatSuggestions, String>
    + Send
    + Sync;

/// TS `mocks.catalog`(`readPaneChatSuggestions`)
pub(crate) struct FakeCatalog {
    responder: Shared<CatalogResponder>,
    calls: Mutex<
        Vec<(
            PaneChatCatalogContext,
            PaneChatSuggestionKind,
            String,
            usize,
        )>,
    >,
}

impl FakeCatalog {
    fn new() -> Self {
        Self {
            responder: shared(Arc::new(
                |_: &PaneChatCatalogContext, _: PaneChatSuggestionKind, _: &str, _: usize| {
                    Ok(FilteredPaneChatSuggestions {
                        items: Vec::new(),
                        is_truncated: false,
                    })
                },
            ) as Arc<CatalogResponder>),
            calls: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn respond(
        &self,
        responder: impl Fn(
            &PaneChatCatalogContext,
            PaneChatSuggestionKind,
            &str,
            usize,
        ) -> Result<FilteredPaneChatSuggestions, String>
        + Send
        + Sync
        + 'static,
    ) {
        *self.responder.lock().unwrap() = Arc::new(responder);
    }

    pub(crate) fn calls(
        &self,
    ) -> Vec<(
        PaneChatCatalogContext,
        PaneChatSuggestionKind,
        String,
        usize,
    )> {
        self.calls.lock().unwrap().clone()
    }

    pub(crate) fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }
}

impl PaneChatSuggestionSource for FakeCatalog {
    fn read_pane_chat_suggestions(
        &self,
        context: &PaneChatCatalogContext,
        kind: PaneChatSuggestionKind,
        query: &str,
        limit: usize,
    ) -> Result<FilteredPaneChatSuggestions, String> {
        self.calls
            .lock()
            .unwrap()
            .push((context.clone(), kind, query.to_string(), limit));
        current(&self.responder)(context, kind, query, limit)
    }
}

// ---- clock ----

/// 기다리지 않는 시계. `sleep`은 가상 시각만 앞당기고 대기 시간을 기록한다(vi.useFakeTimers 대응)
pub(crate) struct ManualClock {
    now: Mutex<f64>,
    sleeps: Mutex<Vec<u64>>,
}

impl ManualClock {
    fn new() -> Self {
        Self {
            now: Mutex::new(1_800_000_000_000.0),
            sleeps: Mutex::new(Vec::new()),
        }
    }

    /// TS `vi.advanceTimersByTimeAsync(ms)`에서 시각이 흐르는 부분
    pub(crate) fn advance(&self, milliseconds: u64) {
        *self.now.lock().unwrap() += milliseconds as f64;
    }

    pub(crate) fn sleeps(&self) -> Vec<u64> {
        self.sleeps.lock().unwrap().clone()
    }
}

impl PaneChatClock for ManualClock {
    fn now_ms(&self) -> f64 {
        *self.now.lock().unwrap()
    }

    fn sleep(&self, duration: Duration) {
        let milliseconds = duration.as_millis() as u64;
        self.sleeps.lock().unwrap().push(milliseconds);
        self.advance(milliseconds);
    }
}

// ---- attachments ----

struct NoRemoteUploads;

impl RemoteAttachmentUploader for NoRemoteUploads {
    fn transfer_file_to_remote_host(
        &self,
        _config: &SshHostConfig,
        _content: &[u8],
        _extension: &str,
    ) -> Result<String, String> {
        Err("원격 업로드는 이 테스트에서 쓰지 않는다".to_string())
    }
}

// ---- harness ----

/// TS beforeEach 상태의 서비스와 그 가짜들
pub(crate) struct Harness {
    pub service: Arc<PaneChatService>,
    pub surfaces: Arc<FakeSurfaces>,
    pub runner: Arc<ScriptedShellRunner>,
    pub terminals: Arc<FakeTerminals>,
    pub bridge: Arc<FakeBridge>,
    pub sessions: Arc<FakeSessions>,
    pub catalog: Arc<FakeCatalog>,
    pub clock: Arc<ManualClock>,
    _data_directory: tempfile::TempDir,
}

impl Harness {
    pub(crate) fn new() -> Self {
        let data_directory = tempfile::tempdir().unwrap();
        let surfaces = Arc::new(FakeSurfaces::new());
        let runner = ScriptedShellRunner::new();
        let terminals = Arc::new(FakeTerminals::default());
        let bridge = Arc::new(FakeBridge::new());
        let sessions = Arc::new(FakeSessions::new());
        let catalog = Arc::new(FakeCatalog::new());
        let clock = Arc::new(ManualClock::new());
        let ssh_home: PathBuf = data_directory.path().join("home");
        let attachments = Arc::new(PaneChatAttachmentStore::new(
            data_directory.path().join("app-data"),
            runner.clone(),
            Arc::new(SshConfigReader::new(ssh_home.to_str().unwrap())),
            Arc::new(NoRemoteUploads),
        ));
        let service = Arc::new(PaneChatService::new(PaneChatServiceDependencies {
            surfaces: surfaces.clone(),
            runner: runner.clone(),
            terminals: terminals.clone(),
            mobile_bridge: bridge.clone(),
            sessions: sessions.clone(),
            suggestions: catalog.clone(),
            attachments,
            clock: clock.clone(),
        }));
        let harness = Self {
            service,
            surfaces,
            runner,
            terminals,
            bridge,
            sessions,
            catalog,
            clock,
            _data_directory: data_directory,
        };
        harness.respond_process_table(&process_table(DEFAULT_PROCESS_START));
        harness
    }

    /// TS 기본 `mocks.exec`: pane 프로세스 조회에는 `table`을, 그 밖의 명령에는 "No open transcript" 실패를 준다
    pub(crate) fn respond_process_table(&self, table: &str) {
        let table = table.to_string();
        self.runner.respond(move |command, _| {
            Some(match is_pane_process_table_command(command) {
                true => Ok(table.clone()),
                false => Err(ExecGitError::new("No open transcript")),
            })
        });
    }

    /// TS `mocks.exec.mockResolvedValue(output)`. 모든 명령이 같은 출력으로 성공한다
    pub(crate) fn respond_all_commands(&self, output: &str) {
        let output = output.to_string();
        self.runner.respond(move |_, _| Some(Ok(output.clone())));
    }

    /// pane 조회 명령만 걸러 낸 실행 기록
    pub(crate) fn process_lookups(&self) -> Vec<(String, Option<String>)> {
        self.runner
            .calls()
            .into_iter()
            .filter(|call| is_pane_process_table_command(&call.command))
            .map(|call| (call.command, call.ssh_host))
            .collect()
    }

    pub(crate) fn writes(&self) -> Vec<(String, String, String)> {
        self.bridge.writes()
    }

    /// `[("task-chat", "%3", bytes)]` 모양의 기대값
    pub(crate) fn expected_writes(
        task_id: &str,
        pane_id: &str,
        bytes: &[&str],
    ) -> Vec<(String, String, String)> {
        bytes
            .iter()
            .map(|bytes| (task_id.to_string(), pane_id.to_string(), bytes.to_string()))
            .collect()
    }
}

/// 테스트가 TS `toMatchObject({ name })`처럼 오류 갈래만 볼 때 쓴다
pub(crate) fn error_name(error: &PaneChatError) -> &'static str {
    match error {
        PaneChatError::Input(_) => "PaneChatInputError",
        PaneChatError::TargetChanged => "PaneChatTargetChangedError",
        PaneChatError::DeliveryUncertain { .. } => "PaneChatDeliveryUncertainError",
        PaneChatError::Other(_) => "Error",
    }
}

impl Harness {
    /// 서비스가 쓰는 세션 기록만 바꿔 끼운다. `wrap`은 기본 가짜를 받아 일부 메서드만 실제 구현으로 돌릴 수 있다
    pub(crate) fn with_session_records(
        wrap: impl FnOnce(Arc<FakeSessions>) -> Arc<dyn PaneChatSessionRecords>,
    ) -> Self {
        let mut harness = Self::new();
        let records = wrap(harness.sessions.clone());
        let ssh_home = harness._data_directory.path().join("home");
        let attachments = Arc::new(PaneChatAttachmentStore::new(
            harness._data_directory.path().join("app-data"),
            harness.runner.clone(),
            Arc::new(SshConfigReader::new(ssh_home.to_str().unwrap())),
            Arc::new(NoRemoteUploads),
        ));
        harness.service = Arc::new(PaneChatService::new(PaneChatServiceDependencies {
            surfaces: harness.surfaces.clone(),
            runner: harness.runner.clone(),
            terminals: harness.terminals.clone(),
            mobile_bridge: harness.bridge.clone(),
            sessions: records,
            suggestions: harness.catalog.clone(),
            attachments,
            clock: harness.clock.clone(),
        }));
        harness
    }
}
