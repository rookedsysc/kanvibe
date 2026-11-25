//! 서비스가 기대는 바깥 기능. 세션 기록·Codex app-server·hook 바인딩은 [`PaneChatSessionRecords`] 하나로,
//! 입력창 제안은 [`PaneChatSuggestionSource`]로, 시간은 [`PaneChatClock`]으로 받는다. 테스트는 각각 가짜를 넣는다.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kanvibe_ai::codex_app_server::{
    CodexAppServerSocket, CodexAppServerTarget, UnixSocketForwarder,
};
use kanvibe_ai::codex_thread::{
    CodexThreadInput, CodexThreadSettingsTarget, CodexUserInput, read_codex_empty_thread,
    read_codex_thread_settings, send_codex_thread_input, update_codex_thread_settings,
};
use kanvibe_ai::host::SessionHost;
use kanvibe_ai::jsonl::{read_json_lines_head, read_json_lines_tail};
use kanvibe_ai::pane_chat::model_settings::{PaneChatModelSettingChange, PaneChatModelSettings};
use kanvibe_ai::pane_chat::process::{PaneChatProcess, PaneChatProvider};
use kanvibe_ai::pane_chat::session_bindings::{
    PaneChatBindingQuery, PaneChatSessionBinding, PaneChatSessionBindings,
};
use kanvibe_ai::parse_cache::{ParseCacheKind, cached_or_parse};
use kanvibe_ai::types::{AggregatedAiSessionDetail, AiSessionReaderContext};
use kanvibe_ai::{AiError, AiResult};
use kanvibe_domain::pane_chat::FilteredPaneChatSuggestions;
use serde_json::Value;

use super::PaneChatError;
use crate::pane_chat::composer_catalog::{
    PaneChatCatalogContext, PaneChatComposerCatalog, PaneChatSuggestionKind,
};

/// 현재 모델 설정과 AI 질문 원문을 찾으려고 기록의 끝에서 읽는 줄 수. 최근 응답 하나만 들어 있으면 된다
const MODEL_STATE_TAIL_LINES: usize = 200;

/// 세션 기록을 찾는 범위(TS `{ worktreePath, repoPath, sshHost }`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatReaderScope {
    pub worktree_path: Option<String>,
    pub repo_path: Option<String>,
    pub ssh_host: Option<String>,
}

/// 주소로 고정한 Codex 대화 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexThreadAddress {
    pub scope: PaneChatReaderScope,
    /// CLI 프로세스가 명시한 app-server 주소(TS `codexServerSocket`). 바깥 `None`은 계정 기본 소켓, `Some(None)`은 쓸 수 없는 연결 방식이다
    pub socket: Option<Option<String>>,
    pub thread_id: String,
    pub source_ref: String,
}

/// Codex 대화에 한 번 보낼 입력
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexThreadDelivery {
    pub address: CodexThreadAddress,
    pub input: Vec<CodexUserInput>,
    pub client_user_message_id: String,
}

/// 보내기 직전에 대상을 다시 확인하는 콜백. 실패하면 보내지 않는다
pub type BeforeSend<'a> = &'a mut dyn FnMut() -> Result<(), PaneChatError>;

/// 대화 기록·Codex app-server·hook 바인딩. TS가 `@/lib/aiSessions/*`와 `hostFileAccess`에서 부르던 것들이다
pub trait PaneChatSessionRecords: Send + Sync {
    /// TS `read{Claude,Codex,Gemini,OpenCode}SessionDetail(context, sessionId, sourceRef, cursor, limit)`. 지침 문맥도 함께 읽는다
    fn read_session_detail(
        &self,
        provider: PaneChatProvider,
        scope: &PaneChatReaderScope,
        session_id: &str,
        source_ref: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Option<AggregatedAiSessionDetail>, String>;
    fn find_codex_main_thread_rollout(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
    ) -> Result<Option<String>, String>;
    /// 첫 입력 전 기록 파일이 없는 새 Codex 대화의 예정 경로. 어떤 실패도 `None`이다
    fn read_codex_empty_thread(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
        socket: &Option<Option<String>>,
    ) -> Option<String>;
    fn send_codex_thread_input(
        &self,
        delivery: &CodexThreadDelivery,
        before_send: BeforeSend<'_>,
        on_write: &mut dyn FnMut(),
    ) -> Result<(), PaneChatError>;
    fn read_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
    ) -> Result<PaneChatModelSettings, String>;
    fn update_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
        change: &PaneChatModelSettingChange,
        before_write: BeforeSend<'_>,
    ) -> Result<(), PaneChatError>;
    /// 기록 끝 200줄. 파일이 바뀌지 않았으면 원격에서 다시 받지 않는다(TS `getCachedOrParseTail`)
    fn read_recent_records(
        &self,
        source_ref: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String>;
    fn read_json_lines_head(
        &self,
        file_path: &str,
        max_lines: usize,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String>;
    fn home_directory(&self, ssh_host: Option<&str>) -> Result<String, String>;
    fn path_exists(&self, target_path: &str, ssh_host: Option<&str>) -> Result<bool, String>;
    fn read_text_file(&self, target_path: &str, ssh_host: Option<&str>) -> Result<String, String>;
    /// hook이 묶어 둔 대화(TS `findPaneChatSessionBinding`)
    fn find_session_binding(
        &self,
        query: &PaneChatBindingQuery,
    ) -> Result<Option<PaneChatSessionBinding>, String>;
    /// CLI가 남긴 프로세스 기록으로 찾은 대화(TS `findPaneChatProcessSessionBinding`)
    fn find_process_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        process: &PaneChatProcess,
    ) -> Option<PaneChatSessionBinding>;
    /// Herdr가 알려 준 대화 ID를 리더가 허용한 기록으로 잇는다(TS `findPaneChatReportedSessionBinding`)
    fn find_reported_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        provider: PaneChatProvider,
        session_id: &str,
    ) -> Result<Option<PaneChatSessionBinding>, String>;
}

/// 입력창 제안 목록(TS `readPaneChatSuggestions`)
pub trait PaneChatSuggestionSource: Send + Sync {
    fn read_pane_chat_suggestions(
        &self,
        context: &PaneChatCatalogContext,
        kind: PaneChatSuggestionKind,
        query: &str,
        limit: usize,
    ) -> Result<FilteredPaneChatSuggestions, String>;
}

impl PaneChatSuggestionSource for PaneChatComposerCatalog {
    fn read_pane_chat_suggestions(
        &self,
        context: &PaneChatCatalogContext,
        kind: PaneChatSuggestionKind,
        query: &str,
        limit: usize,
    ) -> Result<FilteredPaneChatSuggestions, String> {
        PaneChatComposerCatalog::read_pane_chat_suggestions(self, context, kind, query, limit)
            .map_err(|error| error.to_string())
    }
}

/// 서비스가 쓰는 시각과 대기. 붙여넣기·Enter 사이 대기 시간은 TS와 같고, 테스트는 기다리지 않는 시계를 넣는다
pub trait PaneChatClock: Send + Sync {
    /// 1970년부터의 밀리초(JS `Date.now()`)
    fn now_ms(&self) -> f64;
    fn sleep(&self, duration: Duration);
}

pub struct SystemPaneChatClock;

impl PaneChatClock for SystemPaneChatClock {
    fn now_ms(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |elapsed| elapsed.as_millis() as f64)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// 실제 구현. 세션 리더와 Codex app-server 연결은 `kanvibe_ai`의 것을 쓴다
pub struct AiPaneChatSessionRecords {
    host: SessionHost,
    forwarder: Option<Arc<dyn UnixSocketForwarder>>,
    bindings: Arc<PaneChatSessionBindings>,
}

impl AiPaneChatSessionRecords {
    /// `host`는 로컬 호스트다. 원격 태스크는 같은 실행기로 그 호스트를 본다
    pub fn new(
        host: SessionHost,
        forwarder: Option<Arc<dyn UnixSocketForwarder>>,
        bindings: Arc<PaneChatSessionBindings>,
    ) -> Self {
        Self {
            host,
            forwarder,
            bindings,
        }
    }

    fn host_for(&self, ssh_host: Option<&str>) -> SessionHost {
        self.host.with_ssh_host(ssh_host)
    }

    fn context(&self, scope: &PaneChatReaderScope) -> AiSessionReaderContext {
        let mut context = AiSessionReaderContext::new(
            self.host_for(scope.ssh_host.as_deref()),
            scope.worktree_path.as_deref(),
        );
        context.repo_path.clone_from(&scope.repo_path);
        context.include_instructions = true;
        context
    }

    fn server_target<'a>(
        &'a self,
        context: &'a AiSessionReaderContext,
        socket: &Option<Option<String>>,
    ) -> CodexAppServerTarget<'a> {
        let socket = match socket {
            None => CodexAppServerSocket::AccountDefault,
            Some(None) => CodexAppServerSocket::Unsupported,
            Some(Some(path)) => CodexAppServerSocket::Path(path.clone()),
        };
        CodexAppServerTarget {
            context,
            socket,
            forwarder: self.forwarder.as_deref(),
        }
    }
}

/// 콜백이 낸 서비스 오류를 `kanvibe_ai` 호출 밖으로 그대로 꺼낸다. 대상 변경 같은 오류의 갈래를 잃지 않기 위해서다
struct CallbackFailure(RefCell<Option<PaneChatError>>);

impl CallbackFailure {
    fn new() -> Self {
        Self(RefCell::new(None))
    }

    fn guard(&self, callback: BeforeSend<'_>) -> AiResult<()> {
        callback().map_err(|error| {
            let message = error.to_string();
            *self.0.borrow_mut() = Some(error);
            AiError::message(message)
        })
    }

    fn resolve(self, result: AiResult<()>) -> Result<(), PaneChatError> {
        match (result, self.0.into_inner()) {
            (Ok(()), _) => Ok(()),
            (Err(_), Some(callback_error)) => Err(callback_error),
            (Err(error), None) => Err(PaneChatError::Other(error.to_string())),
        }
    }
}

impl PaneChatSessionRecords for AiPaneChatSessionRecords {
    fn read_session_detail(
        &self,
        provider: PaneChatProvider,
        scope: &PaneChatReaderScope,
        session_id: &str,
        source_ref: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Option<AggregatedAiSessionDetail>, String> {
        let context = self.context(scope);
        let reader = match provider {
            PaneChatProvider::Claude => kanvibe_ai::claude_sessions::read_claude_session_detail,
            PaneChatProvider::Codex => kanvibe_ai::codex_sessions::read_codex_session_detail,
            PaneChatProvider::Gemini => kanvibe_ai::gemini_sessions::read_gemini_session_detail,
            PaneChatProvider::Opencode => {
                kanvibe_ai::opencode_sessions::read_opencode_session_detail
            }
        };
        reader(&context, session_id, Some(source_ref), cursor, Some(limit))
            .map_err(|error| error.to_string())
    }

    fn find_codex_main_thread_rollout(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
    ) -> Result<Option<String>, String> {
        kanvibe_ai::codex_sessions::find_codex_main_thread_rollout(&self.context(scope), thread_id)
            .map_err(|error| error.to_string())
    }

    fn read_codex_empty_thread(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
        socket: &Option<Option<String>>,
    ) -> Option<String> {
        let context = self.context(scope);
        read_codex_empty_thread(&self.server_target(&context, socket), thread_id)
    }

    fn send_codex_thread_input(
        &self,
        delivery: &CodexThreadDelivery,
        before_send: BeforeSend<'_>,
        on_write: &mut dyn FnMut(),
    ) -> Result<(), PaneChatError> {
        let address = &delivery.address;
        let context = self.context(&address.scope);
        let request = CodexThreadInput {
            target: self.server_target(&context, &address.socket),
            thread_id: address.thread_id.clone(),
            source_ref: address.source_ref.clone(),
            input: delivery.input.clone(),
            client_user_message_id: delivery.client_user_message_id.clone(),
        };
        let failure = CallbackFailure::new();
        let result = send_codex_thread_input(&request, || failure.guard(before_send), on_write);
        failure.resolve(result)
    }

    fn read_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
    ) -> Result<PaneChatModelSettings, String> {
        let context = self.context(&address.scope);
        let target = CodexThreadSettingsTarget {
            target: self.server_target(&context, &address.socket),
            thread_id: address.thread_id.clone(),
            source_ref: address.source_ref.clone(),
        };
        read_codex_thread_settings(&target).map_err(|error| error.to_string())
    }

    fn update_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
        change: &PaneChatModelSettingChange,
        before_write: BeforeSend<'_>,
    ) -> Result<(), PaneChatError> {
        let context = self.context(&address.scope);
        let target = CodexThreadSettingsTarget {
            target: self.server_target(&context, &address.socket),
            thread_id: address.thread_id.clone(),
            source_ref: address.source_ref.clone(),
        };
        let failure = CallbackFailure::new();
        let result = update_codex_thread_settings(&target, change, || failure.guard(before_write));
        failure.resolve(result)
    }

    fn read_recent_records(
        &self,
        source_ref: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        let host = self.host_for(ssh_host);
        cached_or_parse(ParseCacheKind::Tail, &host, source_ref, || {
            read_json_lines_tail(&host, source_ref, MODEL_STATE_TAIL_LINES)
        })
        .map(|records| records.as_ref().clone())
        .map_err(|error| error.to_string())
    }

    fn read_json_lines_head(
        &self,
        file_path: &str,
        max_lines: usize,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        read_json_lines_head(&self.host_for(ssh_host), file_path, max_lines)
            .map_err(|error| error.to_string())
    }

    fn home_directory(&self, ssh_host: Option<&str>) -> Result<String, String> {
        self.host_for(ssh_host)
            .home_directory()
            .map_err(|error| error.to_string())
    }

    fn path_exists(&self, target_path: &str, ssh_host: Option<&str>) -> Result<bool, String> {
        self.host_for(ssh_host)
            .path_exists(target_path)
            .map_err(|error| error.to_string())
    }

    fn read_text_file(&self, target_path: &str, ssh_host: Option<&str>) -> Result<String, String> {
        self.host_for(ssh_host)
            .read_text_file(target_path)
            .map_err(|error| error.to_string())
    }

    fn find_session_binding(
        &self,
        query: &PaneChatBindingQuery,
    ) -> Result<Option<PaneChatSessionBinding>, String> {
        self.bindings
            .find_pane_chat_session_binding(query)
            .map_err(|error| error.to_string())
    }

    fn find_process_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        process: &PaneChatProcess,
    ) -> Option<PaneChatSessionBinding> {
        self.bindings
            .find_pane_chat_process_session_binding(&self.context(scope), process)
    }

    fn find_reported_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        provider: PaneChatProvider,
        session_id: &str,
    ) -> Result<Option<PaneChatSessionBinding>, String> {
        self.bindings
            .find_pane_chat_reported_session_binding(&self.context(scope), provider, session_id)
            .map_err(|error| error.to_string())
    }
}
