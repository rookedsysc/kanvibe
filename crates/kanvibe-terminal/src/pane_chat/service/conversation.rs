//! pane의 CLI가 지금 쓰고 있는 대화 기록 찾기와 그 기록에서 읽는 대화·모델 설정·질문 원문.
//!
//! 추정하지 않는다. Codex footer, hook 바인딩, CLI가 남긴 프로세스 기록, 열려 있는 기록 파일처럼 그 실행을 가리키는 증거로만 잇는다.

use kanvibe_ai::javascript::stringify;
use kanvibe_ai::node_path;
use kanvibe_ai::pane_chat::claude_prompt::{
    PaneChatPromptKind, PaneChatPromptScreen, apply_asked_questions, read_claude_asked_questions,
    read_claude_prompt,
};
use kanvibe_ai::pane_chat::codex_prompt::read_codex_prompt;
use kanvibe_ai::pane_chat::model_settings::{
    PaneChatModelOption, PaneChatModelProvider, PaneChatModelSettings,
    build_pane_chat_model_settings, parse_codex_model_catalog, read_claude_model_state,
    read_codex_model_state,
};
use kanvibe_ai::pane_chat::process::{PaneChatProcess, PaneChatProvider};
use kanvibe_ai::pane_chat::session_bindings::{PaneChatBindingQuery, PaneChatSessionBinding};
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatInputState, PaneChatPrompt, PaneChatTranscript, PaneChatTranscriptStatus,
};
use kanvibe_ai::providers::AiSessionProvider;
use serde_json::{Value, json};

use super::codex_display::codex_display;
use super::dependencies::PaneChatReaderScope;
use super::input_state::read_input_state;
use super::target::sha256_hex;
use super::{PaneChatError, PaneChatService, lock};
use crate::pane_chat::surface::{PaneChatSurface, PaneChatTask};

pub(super) const TRANSCRIPT_PAGE_LIMIT: usize = 100;
/// Codex 모델 목록은 CLI가 가끔 갱신하는 캐시라, 매번 다시 읽지 않고 이 시간 동안 재사용한다
const CODEX_MODEL_CATALOG_TTL_MS: f64 = 60_000.0;

pub(super) fn reader_scope(task: &PaneChatTask) -> PaneChatReaderScope {
    PaneChatReaderScope {
        worktree_path: task
            .worktree_path
            .clone()
            .or_else(|| task.project_repo_path.clone()),
        repo_path: task.project_repo_path.clone(),
        ssh_host: task.ssh_host.clone(),
    }
}

pub(super) fn snapshot_provider(provider: PaneChatProvider) -> AiSessionProvider {
    match provider {
        PaneChatProvider::Claude => AiSessionProvider::Claude,
        PaneChatProvider::Codex => AiSessionProvider::Codex,
        PaneChatProvider::Gemini => AiSessionProvider::Gemini,
        PaneChatProvider::Opencode => AiSessionProvider::Opencode,
    }
}

pub(super) fn model_provider(provider: PaneChatProvider) -> Option<PaneChatModelProvider> {
    match provider {
        PaneChatProvider::Claude => Some(PaneChatModelProvider::Claude),
        PaneChatProvider::Codex => Some(PaneChatModelProvider::Codex),
        _ => None,
    }
}

/// 같은 대화인지 견줄 값(`[sessionId, sourceRef, revision]`)
pub(super) fn conversation_identity(binding: Option<&PaneChatSessionBinding>) -> Option<String> {
    let binding = binding?;
    Some(stringify(&json!([
        binding.session_id,
        binding.source_ref,
        binding.revision
    ])))
}

pub(super) fn empty_transcript(reason: &str) -> PaneChatTranscript {
    PaneChatTranscript {
        status: PaneChatTranscriptStatus::Unavailable,
        reason: Some(reason.to_string()),
        session_id: None,
        messages: Vec::new(),
        next_cursor: None,
        epoch: None,
        seq: None,
    }
}

fn available_transcript(session_id: String) -> PaneChatTranscript {
    PaneChatTranscript {
        status: PaneChatTranscriptStatus::Available,
        reason: None,
        session_id: Some(session_id),
        messages: Vec::new(),
        next_cursor: None,
        epoch: None,
        seq: None,
    }
}

/// 읽은 대화와, 대화를 읽어 온 JSONL 파일. 연결하지 못했으면 파일은 `None`이다
pub(super) struct ReadTranscript {
    pub transcript: PaneChatTranscript,
    pub source_ref: Option<String>,
}

impl ReadTranscript {
    pub(super) fn failed() -> Self {
        Self {
            transcript: empty_transcript("transcript-read-failed"),
            source_ref: None,
        }
    }
}

fn exact_binding(session_id: &str, source_ref: &str) -> PaneChatSessionBinding {
    PaneChatSessionBinding {
        session_id: session_id.to_string(),
        source_ref: source_ref.to_string(),
        revision: None,
        is_pending: false,
        is_empty: false,
    }
}

fn other_error(message: String) -> PaneChatError {
    PaneChatError::Other(message)
}

/// footer로 찾은 결과. `Settled`는 그 결과로 끝내고, `NotShown`은 footer가 없어 다른 증거로 찾는다
enum DisplayedThread {
    Settled(Option<PaneChatSessionBinding>),
    NotShown,
}

impl PaneChatService {
    /// pane의 CLI 실행이 쓰고 있는 대화. 증거가 없으면 `None`이다
    pub(super) fn resolve_conversation(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
    ) -> Result<Option<PaneChatSessionBinding>, PaneChatError> {
        let scope = reader_scope(&surface.task);
        if let DisplayedThread::Settled(binding) =
            self.resolve_displayed_thread(&scope, surface, process)?
        {
            return Ok(binding);
        }
        let mut binding = self.sessions.find_process_session_binding(&scope, process);
        if process.provider == PaneChatProvider::Opencode {
            return Ok(binding);
        }
        if binding.is_none() {
            binding = self.find_hook_binding(surface, process)?;
        }
        if binding.is_none() {
            binding = self.find_reported_binding(&scope, surface, process)?;
        }
        let is_claude_or_codex = matches!(
            process.provider,
            PaneChatProvider::Claude | PaneChatProvider::Codex
        );
        if binding.is_none() && is_claude_or_codex {
            binding = self.find_open_transcript_binding(surface, process);
        }
        Ok(binding)
    }

    /// Codex 표시 대상이면 화면 footer(또는 그 footer를 가린 같은 선택 화면)의 대화 ID로 잇는다.
    /// 처음 보는 ID는 Codex 고유의 빈 입력란이 확인될 때만 받아들인다. 입력란의 글에 섞인 ID를 footer로 착각하지 않기 위해서다
    fn resolve_displayed_thread(
        &self,
        scope: &PaneChatReaderScope,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
    ) -> Result<DisplayedThread, PaneChatError> {
        let Some(display) = self.codex_display_execution(surface, process) else {
            return Ok(DisplayedThread::NotShown);
        };
        let mut state = lock(&display);
        let shown_id = codex_display(surface).and_then(|shown| shown.thread_id);
        let id = match &shown_id {
            Some(id) => Some(id.clone()),
            None => state.hold_thread_behind_prompt(surface, self.now_ms()),
        };
        let is_ready = || read_input_state(surface, process, true) == PaneChatInputState::Ready;
        let Some(id) = id else {
            state.observe_thread(None, surface.observation, self.now_ms())?;
            state.reason = match is_ready() {
                true => "codex-thread-unconfirmed",
                false => "codex-thread-input-busy",
            }
            .to_string();
            return Ok(DisplayedThread::NotShown);
        };
        if state.observed_thread.as_deref() != Some(id.as_str()) && !is_ready() {
            state.observe_thread(None, surface.observation, self.now_ms())?;
            state.reason = "codex-thread-input-busy".to_string();
            return Ok(DisplayedThread::Settled(None));
        }
        // 기록을 찾는 동안 상태를 잠가 두지 않는다. 그사이 더 최근 조회가 다른 대화를 보면, 늦게 끝난 이 조회는 아래 관찰에서 대상 변경으로 끝난다
        drop(state);
        let (source_ref, is_pending) = self.find_displayed_thread_record(scope, &id, process)?;
        let mut state = lock(&display);
        let Some(source_ref) = source_ref else {
            state.observe_thread(None, surface.observation, self.now_ms())?;
            state.reason = "codex-thread-record-missing".to_string();
            return Ok(DisplayedThread::Settled(None));
        };
        if shown_id.is_some() {
            state.observe_thread(Some(&id), surface.observation, self.now_ms())?;
        }
        Ok(DisplayedThread::Settled(Some(PaneChatSessionBinding {
            session_id: id,
            source_ref,
            revision: Some(state.revision),
            is_pending,
            is_empty: false,
        })))
    }

    /// 대화 ID의 메인 대화 기록. 첫 입력 전이라 기록이 아직 없는 새 대화는 app-server로 확인하고 대기 중으로 표시한다
    fn find_displayed_thread_record(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
        process: &PaneChatProcess,
    ) -> Result<(Option<String>, bool), PaneChatError> {
        let rollout = self
            .sessions
            .find_codex_main_thread_rollout(scope, thread_id)
            .map_err(other_error)?;
        if rollout.is_some() {
            return Ok((rollout, false));
        }
        let empty_thread =
            self.sessions
                .read_codex_empty_thread(scope, thread_id, &process.codex_server_socket);
        Ok((empty_thread, true))
    }

    fn find_hook_binding(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
    ) -> Result<Option<PaneChatSessionBinding>, PaneChatError> {
        let query = PaneChatBindingQuery {
            task_id: surface.task.id.clone(),
            ssh_host: surface.task.ssh_host.clone(),
            provider: process.provider,
            provider_pid: process.pid,
            process_start_time: process.process_start_time.clone(),
        };
        self.sessions
            .find_session_binding(&query)
            .map_err(other_error)
    }

    /// Herdr가 알려 준 같은 CLI의 대화 ID. Codex의 첫 입력 전 대화는 app-server에서 확인한다
    fn find_reported_binding(
        &self,
        scope: &PaneChatReaderScope,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
    ) -> Result<Option<PaneChatSessionBinding>, PaneChatError> {
        let Some(reported) = surface
            .agent_session
            .as_ref()
            .filter(|session| session.provider == snapshot_provider(process.provider))
        else {
            return Ok(None);
        };
        let binding = self
            .sessions
            .find_reported_session_binding(scope, process.provider, &reported.session_id)
            .map_err(other_error)?;
        if binding.is_some() || process.provider != PaneChatProvider::Codex {
            return Ok(binding);
        }
        let empty_thread = self.sessions.read_codex_empty_thread(
            scope,
            &reported.session_id,
            &process.codex_server_socket,
        );
        Ok(empty_thread.map(|source_ref| PaneChatSessionBinding {
            is_pending: true,
            ..exact_binding(&reported.session_id, &source_ref)
        }))
    }

    /// CLI가 열어 둔 기록 파일이 정확히 하나일 때만 잇는다.
    /// CLI가 쓰기 사이에 JSONL을 닫을 수 있다. 열린 파일이 없으면 다른 기록을 연결하지 않는다
    fn find_open_transcript_binding(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
    ) -> Option<PaneChatSessionBinding> {
        let ssh_host = surface.task.ssh_host();
        let output = self
            .exec_probe(&format!("lsof -a -p {} -Fn", process.pid), ssh_host)
            .ok()?;
        let home = self.sessions.home_directory(ssh_host).ok()?;
        let root = match process.provider {
            PaneChatProvider::Claude => node_path::join(&[&home, ".claude", "projects"]),
            _ => node_path::join(&[&home, ".codex", "sessions"]),
        };
        let mut candidates: Vec<&str> = Vec::new();
        for line in output.split('\n') {
            let is_transcript = line.starts_with(&format!("n{root}/")) && line.ends_with(".jsonl");
            if is_transcript && !candidates.contains(&&line[1..]) {
                candidates.push(&line[1..]);
            }
        }
        let mut exact_bindings = Vec::new();
        for source_ref in candidates {
            let records = self
                .sessions
                .read_json_lines_head(source_ref, TRANSCRIPT_PAGE_LIMIT, ssh_host)
                .ok()?;
            if let Some(session_id) = read_transcript_session_id(&records, process.provider) {
                exact_bindings.push(exact_binding(&session_id, source_ref));
            }
        }
        match exact_bindings.len() {
            1 => exact_bindings.pop(),
            _ => None,
        }
    }
}

/// 기록 앞부분에서 대화 ID를 읽는다. 하위 에이전트의 Codex 기록은 메인 대화가 아니므로 버린다
fn read_transcript_session_id(records: &[Value], provider: PaneChatProvider) -> Option<String> {
    let session_meta = records
        .iter()
        .find(|record| record.get("type").and_then(Value::as_str) == Some("session_meta"))
        .and_then(|record| record.get("payload"));
    let session_id = match provider {
        PaneChatProvider::Claude => records
            .iter()
            .find_map(|record| record.get("sessionId").and_then(Value::as_str)),
        _ => session_meta
            .and_then(|meta| meta.get("id"))
            .and_then(Value::as_str),
    };
    let is_subagent = provider == PaneChatProvider::Codex
        && session_meta
            .and_then(|meta| meta.get("source"))
            .and_then(Value::as_object)
            .is_some_and(|source| source.contains_key("subagent"));
    if is_subagent {
        return None;
    }
    session_id
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string)
}

impl PaneChatService {
    pub(super) fn read_transcript(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
        binding: Option<&PaneChatSessionBinding>,
    ) -> Result<ReadTranscript, PaneChatError> {
        let unavailable = || ReadTranscript {
            transcript: empty_transcript("exact-session-unavailable"),
            source_ref: None,
        };
        let Some(binding) = binding else {
            return Ok(unavailable());
        };
        let not_written_yet = || ReadTranscript {
            transcript: available_transcript(binding.session_id.clone()),
            source_ref: None,
        };
        if self.is_unwritten_binding(binding, surface.task.ssh_host.as_deref())? {
            return Ok(not_written_yet());
        }
        let detail = self
            .sessions
            .read_session_detail(
                process.provider,
                &reader_scope(&surface.task),
                &binding.session_id,
                &binding.source_ref,
                None,
                TRANSCRIPT_PAGE_LIMIT,
            )
            .map_err(other_error)?;
        let Some(detail) = detail else {
            return Ok(unavailable());
        };
        let mut transcript = available_transcript(detail.session_id);
        transcript.messages = detail.messages;
        transcript.next_cursor = detail.next_cursor;
        Ok(ReadTranscript {
            transcript,
            source_ref: Some(binding.source_ref.clone()),
        })
    }

    /// 세션을 고르기 전의 빈 대화이거나, 첫 입력 전이라 기록 파일이 아직 없는 대화인지
    fn is_unwritten_binding(
        &self,
        binding: &PaneChatSessionBinding,
        ssh_host: Option<&str>,
    ) -> Result<bool, PaneChatError> {
        if binding.is_empty {
            return Ok(true);
        }
        if !binding.is_pending {
            return Ok(false);
        }
        let exists = self
            .sessions
            .path_exists(&binding.source_ref, ssh_host)
            .map_err(other_error)?;
        Ok(!exists)
    }

    fn read_codex_model_catalog(
        &self,
        ssh_host: Option<&str>,
    ) -> Result<Vec<PaneChatModelOption>, PaneChatError> {
        let host_key = ssh_host.unwrap_or_default().to_string();
        let cached = lock(&self.codex_model_catalogs).get(&host_key).cloned();
        if let Some((read_at, models)) = cached
            && self.now_ms() - read_at < CODEX_MODEL_CATALOG_TTL_MS
        {
            return Ok(models);
        }
        let home = self
            .sessions
            .home_directory(ssh_host)
            .map_err(other_error)?;
        let cache_path = node_path::join(&[&home, ".codex", "models_cache.json"]);
        let text = self
            .sessions
            .read_text_file(&cache_path, ssh_host)
            .map_err(other_error)?;
        let cache: Value =
            serde_json::from_str(&text).map_err(|error| other_error(error.to_string()))?;
        let models = parse_codex_model_catalog(&cache);
        lock(&self.codex_model_catalogs).insert(host_key, (self.now_ms(), models.clone()));
        Ok(models)
    }

    /// 기록 끝부분의 현재 모델 설정과 고를 수 있는 선택지. 화면을 보는 동안 매초 불리므로 기록 파일이 바뀌지 않았으면 다시 받지 않는다
    pub(super) fn read_model_settings(
        &self,
        provider: PaneChatModelProvider,
        source_ref: &str,
        ssh_host: Option<&str>,
        is_pending: bool,
    ) -> Result<PaneChatModelSettings, PaneChatError> {
        let is_unwritten = is_pending
            && !self
                .sessions
                .path_exists(source_ref, ssh_host)
                .map_err(other_error)?;
        let records = match is_unwritten {
            true => Vec::new(),
            false => self
                .sessions
                .read_recent_records(source_ref, ssh_host)
                .map_err(other_error)?,
        };
        let models = match provider {
            PaneChatModelProvider::Codex => self.read_codex_model_catalog(ssh_host)?,
            PaneChatModelProvider::Claude => Vec::new(),
        };
        let state = match provider {
            PaneChatModelProvider::Claude => read_claude_model_state(&records),
            PaneChatModelProvider::Codex => read_codex_model_state(&records),
        };
        Ok(build_pane_chat_model_settings(provider, state, models))
    }

    /// AI의 질문은 대화 기록에 질문 원문이 남는다. 화면에서는 넘어간 라벨과 설명을 가를 수 없는 경우가 있어, 원문이 화면과 맞으면 그 라벨과 설명으로 보여 준다.
    /// 같은 화면인지 가리는 값은 화면에서 읽은 그대로 둔다. 기록은 화면보다 늦게 쓰일 수 있어 답을 보낼 때의 확인에는 쓰지 않는다
    pub(super) fn show_asked_question_text(
        &self,
        prompt: PaneChatPrompt,
        source_ref: &str,
        ssh_host: Option<&str>,
    ) -> PaneChatPrompt {
        if prompt.screen.kind != PaneChatPromptKind::Question {
            return prompt;
        }
        match self.sessions.read_recent_records(source_ref, ssh_host) {
            Ok(records) => PaneChatPrompt {
                screen: apply_asked_questions(
                    prompt.screen,
                    &read_claude_asked_questions(&records),
                ),
                id: prompt.id,
            },
            Err(_) => prompt,
        }
    }
}

/// 화면에 떠 있는 선택 화면. 화면 배치를 확인한 Claude와 Codex만 읽는다
fn read_prompt_screen(
    surface: &PaneChatSurface,
    process: &PaneChatProcess,
) -> Option<PaneChatPromptScreen> {
    let lines = surface.viewport_lines.as_ref()?;
    match process.provider {
        PaneChatProvider::Codex => read_codex_prompt(lines),
        PaneChatProvider::Claude => read_claude_prompt(
            lines,
            surface.cursor_row?,
            surface.cursor_column?,
            surface.viewport_columns,
        ),
        _ => None,
    }
}

/// 화면에 떠 있는 선택 화면을 읽고, 같은 화면인지 가릴 값을 붙인다.
/// 체크 상태는 그 값에 넣는다. 카드를 본 뒤 터미널에서 체크를 바꿨으면 같은 숫자 키가 그 선택을 되돌리기 때문이다.
/// 강조 위치와 단계 줄은 어느 키가 무엇을 고르는지를 바꾸지 않으므로 넣지 않는다
pub(super) fn read_shown_prompt(
    surface: &PaneChatSurface,
    process: &PaneChatProcess,
) -> Option<PaneChatPrompt> {
    let screen = read_prompt_screen(surface, process)?;
    let options: Vec<Value> = screen
        .options
        .iter()
        .map(|option| {
            json!([
                option.number,
                option.label,
                option.description,
                option.is_checked
            ])
        })
        .collect();
    let id = sha256_hex(&stringify(&json!([screen.title, screen.detail, options])));
    Some(PaneChatPrompt { screen, id })
}
