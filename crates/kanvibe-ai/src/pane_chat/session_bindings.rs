//! pane을 소유한 CLI 프로세스와 그 대화 기록을 잇는다.
//!
//! 대화 ID는 hook payload나 CLI가 남긴 프로세스 기록에서 받고, 실행 ID(PID·시작 시각)는 실제 에이전트 조상 프로세스에서만 얻는다.
//! hook이 보낸 파일 경로는 열지 않고, 세션 리더가 허용한 기록만 쓴다. 세션 리더는 [`PaneChatSessionCatalog`]로 주입한다.

use std::sync::{Arc, Mutex};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

use super::process::{PaneChatProcess, PaneChatProvider, is_pane_chat_provider_process};
use crate::error::AiResult;
use crate::host::CommandOptions;
use crate::javascript::{JS_WHITESPACE_CLASS, js_trim, js_trim_end, now_ms};
use crate::node_path;
use crate::session_listing::determine_match_scope;
use crate::types::{AggregatedAiSession, AiSessionReaderContext};

/// provider별 세션 목록 리더. 통합 계층이 Claude·Codex·Gemini·OpenCode 리더로 구현한다
pub trait PaneChatSessionCatalog: Send + Sync {
    /// TS `sessionReaders[provider](context)`. 검색어 없이 이 worktree의 세션을 돌려준다
    fn read_sessions(
        &self,
        provider: PaneChatProvider,
        context: &AiSessionReaderContext,
    ) -> AiResult<Vec<AggregatedAiSession>>;
    /// TS `findCodexMainThreadRollout`. 대화 ID가 가리키는 메인 thread의 rollout 경로
    fn find_codex_main_thread_rollout(
        &self,
        context: &AiSessionReaderContext,
        thread_id: &str,
    ) -> AiResult<Option<String>>;
}

/// hook을 받은 Task와 그 조회 조건
#[derive(Clone)]
pub struct PaneChatBindingContext {
    pub task_id: String,
    pub reader: AiSessionReaderContext,
}

/// 바인딩을 찾을 pane 프로세스. 호출자가 현재 pane의 PID와 시작 시각을 검증한 뒤 넘긴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneChatBindingQuery {
    pub task_id: String,
    pub ssh_host: Option<String>,
    pub provider: PaneChatProvider,
    pub provider_pid: i64,
    pub process_start_time: String,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// pane과 이어진 대화. `is_pending`은 기록 파일이 아직 없거나 리더가 아직 찾지 못한 새 세션, `is_empty`는 세션을 고르기 전의 OpenCode 홈 화면이다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatSessionBinding {
    pub session_id: String,
    pub source_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(default, rename = "pending", skip_serializing_if = "is_false")]
    pub is_pending: bool,
    #[serde(default, rename = "empty", skip_serializing_if = "is_false")]
    pub is_empty: bool,
}

impl PaneChatSessionBinding {
    fn exact(session_id: &str, source_ref: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            source_ref: source_ref.to_string(),
            revision: None,
            is_pending: false,
            is_empty: false,
        }
    }
}

const MAX_BINDINGS: usize = 256;
const BINDING_TTL_MS: f64 = 24.0 * 60.0 * 60.0 * 1_000.0;
const MAX_HOOK_BASE64_LENGTH: usize = 96 * 1_024;
const MAX_ANCESTRY_DEPTH: usize = 32;
const PROCESS_PROBE_TIMEOUT_MS: u64 = 3_000;
const OPENCODE_ROUTE_MAX_AGE_MS: f64 = 5_000.0;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const PROCESS_LIST_COMMAND: &str = "ps -wwaxo pid=,ppid=,lstart=,ucomm=,args=";

fn compile(pattern: &str) -> Regex {
    Regex::new(
        &pattern
            .replace("\\S", &format!("[^{JS_WHITESPACE_CLASS}]"))
            .replace("\\s", &format!("[{JS_WHITESPACE_CLASS}]")),
    )
    .expect("세션 바인딩 패턴")
}

static SESSION_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._:-]{1,200}$").expect("세션 ID 패턴"));
static PROCESS_LINE: LazyLock<Regex> = LazyLock::new(|| {
    compile(
        r"^\s*([0-9]+)\s+([0-9]+)\s+(\S+\s+\S+\s+\S+\s+\S+\s+\S+)\s+(\S+)(?:\s+([^\n\r\x{2028}\x{2029}]*))?$",
    )
});
static WHITESPACE_RUN: LazyLock<Regex> = LazyLock::new(|| compile(r"\s+"));
static CODEX_SHARED_SERVER: LazyLock<Regex> =
    LazyLock::new(|| compile(r"(?:^|\s)(?:app-server|--managed-daemon)(?:\s|$)"));
static GEMINI_TRANSCRIPT_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^session-[A-Za-z0-9._-]+\.(?:json|jsonl)$").expect("Gemini 기록 이름 패턴")
});
static OPENCODE_SESSION_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ses_[A-Za-z0-9]+$").expect("OpenCode 세션 패턴"));

/// JS `value.trim().replace(/\s+/g, " ")`
fn normalize_start_time(value: &str) -> String {
    WHITESPACE_RUN.replace_all(js_trim(value), " ").into_owned()
}

fn is_safe_integer(value: Option<&Value>) -> Option<f64> {
    let number = value?.as_f64()?;
    (number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER).then_some(number)
}

fn is_session_id(value: &str) -> bool {
    SESSION_ID.is_match(value)
}

#[derive(Clone, PartialEq, Eq)]
struct BindingKey {
    task_id: String,
    ssh_host: String,
    provider: PaneChatProvider,
    provider_pid: i64,
    process_start_time: String,
}

impl BindingKey {
    fn from_query(query: &PaneChatBindingQuery) -> Self {
        Self {
            task_id: query.task_id.clone(),
            ssh_host: query.ssh_host.clone().unwrap_or_default(),
            provider: query.provider,
            provider_pid: query.provider_pid,
            process_start_time: normalize_start_time(&query.process_start_time),
        }
    }
}

#[derive(Clone)]
struct SessionBinding {
    context: PaneChatBindingContext,
    provider: PaneChatProvider,
    session_id: String,
    recorded_at: f64,
    observation: u64,
    revision: u64,
    source_ref: Option<String>,
    is_pending: bool,
}

#[derive(Default)]
struct BindingTable {
    /// 넣은 순서. 가득 차면 앞의 것부터 버린다
    entries: Vec<(BindingKey, SessionBinding)>,
    observation_sequence: u64,
}

impl BindingTable {
    fn get(&self, key: &BindingKey) -> Option<&SessionBinding> {
        self.entries
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, binding)| binding)
    }

    fn remove(&mut self, key: &BindingKey) {
        self.entries.retain(|(known, _)| known != key);
    }

    /// 새 hook 관측을 넣는다. 만료된 항목은 치우고, 더 나중 관측이 이미 있으면 버린다.
    /// 같은 대화가 다시 관측되면 revision과 이미 찾은 기록 경로를 이어받는다
    fn store(&mut self, key: BindingKey, mut binding: SessionBinding) {
        let now = binding.recorded_at;
        self.entries
            .retain(|(_, known)| now - known.recorded_at <= BINDING_TTL_MS);
        let previous = self.get(&key).cloned();
        if previous
            .as_ref()
            .is_some_and(|previous| previous.observation > binding.observation)
        {
            return;
        }
        if let Some(previous) =
            previous.filter(|previous| previous.session_id == binding.session_id)
        {
            binding.revision = previous.revision;
            if binding.source_ref.is_none() {
                binding.source_ref = previous.source_ref;
            }
        }
        self.remove(&key);
        self.entries.push((key, binding));
        let overflow = self.entries.len().saturating_sub(MAX_BINDINGS);
        self.entries.drain(..overflow);
    }
}

/// `ps` 줄 하나
struct AncestorRow {
    pid: i64,
    parent_pid: i64,
    start_time: String,
    command: String,
    args: String,
}

/// hook 프로세스에서 위로 올라가며 그 provider의 CLI 프로세스를 찾는다.
/// 공유 서버의 hook은 최초 클라이언트 환경을 물려받으므로 pane 소유권을 증명하지 못한다
fn find_provider_ancestor(
    output: &str,
    hook_pid: i64,
    provider: PaneChatProvider,
) -> Option<AncestorRow> {
    let mut processes: Vec<AncestorRow> = Vec::new();
    for line in output.split('\n') {
        let Some(found) = PROCESS_LINE.captures(line) else {
            continue;
        };
        let pid = found[1].parse().unwrap_or(0);
        processes.retain(|row| row.pid != pid);
        processes.push(AncestorRow {
            pid,
            parent_pid: found[2].parse().unwrap_or(0),
            start_time: WHITESPACE_RUN.replace_all(&found[3], " ").into_owned(),
            command: found[4].to_string(),
            args: found
                .get(5)
                .map_or_else(String::new, |args| args.as_str().to_string()),
        });
    }
    let mut visited = Vec::new();
    let mut pid = hook_pid;
    for _ in 0..MAX_ANCESTRY_DEPTH {
        if pid <= 1 || visited.contains(&pid) {
            return None;
        }
        visited.push(pid);
        let index = processes.iter().position(|row| row.pid == pid)?;
        if is_pane_chat_provider_process(
            &processes[index].command,
            &processes[index].args,
            provider,
        ) {
            let is_shared_server = provider == PaneChatProvider::Codex
                && CODEX_SHARED_SERVER.is_match(&processes[index].args);
            return (!is_shared_server).then(|| processes.swap_remove(index));
        }
        pid = processes[index].parent_pid;
    }
    None
}

/// Node `Buffer.from(text, "base64")`. 알파벳 밖 글자와 남는 비트는 버리고, URL용 글자와 빠진 패딩도 받아 준다
fn decode_node_base64(text: &str) -> Vec<u8> {
    use base64::Engine;
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    let mut symbols: String = text
        .chars()
        .take_while(|character| *character != '=')
        .filter_map(|character| match character {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' => Some(character),
            '-' => Some('+'),
            '_' => Some('/'),
            _ => None,
        })
        .collect();
    if symbols.len() % 4 == 1 {
        symbols.pop();
    }
    let config = GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent);
    GeneralPurpose::new(&base64::alphabet::STANDARD, config)
        .decode(symbols)
        .unwrap_or_default()
}

/// hook metadata를 검증해 그 payload를 돌려준다. 받지 않을 metadata면 `None`이다
fn read_hook_payload(
    metadata: &Value,
    context: &PaneChatBindingContext,
) -> Option<(PaneChatProvider, i64, Value)> {
    let input = metadata.as_object()?;
    let provider = match input.get("provider").and_then(Value::as_str)? {
        "claude" => PaneChatProvider::Claude,
        "codex" => PaneChatProvider::Codex,
        "gemini" => PaneChatProvider::Gemini,
        _ => return None,
    };
    let hook_pid = is_safe_integer(input.get("hookPid")).filter(|pid| *pid > 1.0)? as i64;
    let encoded = input
        .get("payloadBase64")?
        .as_str()
        .filter(|encoded| encoded.len() <= MAX_HOOK_BASE64_LENGTH)?;
    let payload: Value =
        serde_json::from_str(&String::from_utf8_lossy(&decode_node_base64(encoded))).ok()?;
    let record = payload.as_object()?;
    let is_codex_child = provider == PaneChatProvider::Codex
        && crate::javascript::is_present_truthy(record.get("agent_id"));
    let session_id = record
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| is_session_id(id));
    let cwd = record.get("cwd").and_then(Value::as_str);
    let is_in_task = determine_match_scope(cwd, &context.reader).is_some();
    (!is_codex_child && session_id.is_some() && is_in_task).then_some((provider, hook_pid, payload))
}

/// Gemini hook이 알려 준 기록 경로가 이 호스트의 Gemini 기록 자리이면 그 경로. 파일은 열지 않는다
fn gemini_pending_source(
    payload: &Value,
    context: &PaneChatBindingContext,
) -> AiResult<Option<String>> {
    let Some(transcript_path) = payload.get("transcript_path").and_then(Value::as_str) else {
        return Ok(None);
    };
    let home = context.reader.host.home_directory()?;
    let root = node_path::join(&[&home, ".gemini", "tmp"]);
    let normalized = node_path::normalize(transcript_path);
    let is_gemini_chat = normalized.starts_with(&format!("{root}/"))
        && GEMINI_TRANSCRIPT_NAME.is_match(&node_path::basename(&normalized))
        && node_path::basename(&node_path::dirname(&normalized)) == "chats";
    Ok(is_gemini_chat.then_some(normalized))
}

/// hook이 대화를 새로 묶었을 때 그 Task ID로 부르는 알림. pane 채팅이 다음 주기를 기다리지 않고 그 Task의 pane을 다시 확인하게 한다
pub type PaneChatBindingListener = Arc<dyn Fn(&str) + Send + Sync>;

/// pane 채팅의 대화 바인딩 목록. TS 모듈 전역 `bindings`를 앱이 하나 들고 쓰는 값으로 옮겼다
pub struct PaneChatSessionBindings {
    catalog: Arc<dyn PaneChatSessionCatalog>,
    table: Mutex<BindingTable>,
    listeners: Mutex<Vec<PaneChatBindingListener>>,
}

impl PaneChatSessionBindings {
    pub fn new(catalog: Arc<dyn PaneChatSessionCatalog>) -> Self {
        Self {
            catalog,
            table: Mutex::new(BindingTable::default()),
            listeners: Mutex::new(Vec::new()),
        }
    }

    /// hook 바인딩을 저장할 때마다 부를 알림을 더한다. 앱이 조립할 때 한 번 건다
    pub fn on_binding_recorded(&self, listener: PaneChatBindingListener) {
        self.listeners
            .lock()
            .expect("바인딩 알림 목록")
            .push(listener);
    }

    fn announce_recorded(&self, task_id: &str) {
        let listeners = self.listeners.lock().expect("바인딩 알림 목록").clone();
        for listener in listeners {
            listener(task_id);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BindingTable> {
        self.table.lock().expect("바인딩 목록")
    }

    /// 대화 ID는 hook에서 받고, 실행 ID는 실제 에이전트 조상 프로세스에서만 얻는다.
    /// 채팅 메타데이터를 얻지 못해도 기존 태스크 상태 hook은 성공해야 하므로 실패는 조용히 무시한다
    pub fn record_pane_chat_session_binding(
        &self,
        context: &PaneChatBindingContext,
        metadata: &Value,
    ) {
        let observation = {
            let mut table = self.lock();
            table.observation_sequence += 1;
            table.observation_sequence
        };
        let Some((provider, hook_pid, payload)) = read_hook_payload(metadata, context) else {
            return;
        };
        let _ = self.record_hook_payload(context, observation, provider, hook_pid, &payload);
    }

    fn record_hook_payload(
        &self,
        context: &PaneChatBindingContext,
        observation: u64,
        provider: PaneChatProvider,
        hook_pid: i64,
        payload: &Value,
    ) -> AiResult<()> {
        let options = CommandOptions {
            max_buffer_bytes: None,
            timeout_ms: Some(PROCESS_PROBE_TIMEOUT_MS),
        };
        let output = context.reader.host.run(PROCESS_LIST_COMMAND, options)?;
        let Some(process) = find_provider_ancestor(&output, hook_pid, provider) else {
            return Ok(());
        };
        let pending_source = if provider == PaneChatProvider::Gemini {
            gemini_pending_source(payload, context)?
        } else {
            None
        };
        let session_id = payload["session_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let key = BindingKey {
            task_id: context.task_id.clone(),
            ssh_host: context.reader.ssh_host().unwrap_or_default().to_string(),
            provider,
            provider_pid: process.pid,
            process_start_time: normalize_start_time(&process.start_time),
        };
        let pending_binding = SessionBinding {
            context: context.clone(),
            provider,
            session_id,
            recorded_at: now_ms(),
            observation,
            revision: observation,
            is_pending: pending_source.is_some(),
            source_ref: pending_source,
        };
        self.lock().store(key, pending_binding);
        self.announce_recorded(&context.task_id);
        Ok(())
    }

    /// 호출자가 현재 pane의 PID와 시작 시각을 검증한 뒤 조회한다
    pub fn find_pane_chat_session_binding(
        &self,
        query: &PaneChatBindingQuery,
    ) -> AiResult<Option<PaneChatSessionBinding>> {
        let key = BindingKey::from_query(query);
        let Some(entry) = self.lock().get(&key).cloned() else {
            return Ok(None);
        };
        if now_ms() - entry.recorded_at > BINDING_TTL_MS {
            return Ok(None);
        }
        let source_ref = match entry.source_ref.clone() {
            Some(source_ref) => source_ref,
            None => match self.resolve_reader_source(&key, &entry)? {
                Some(source_ref) => source_ref,
                None => return Ok(None),
            },
        };
        Ok(Some(PaneChatSessionBinding {
            session_id: entry.session_id,
            source_ref,
            revision: Some(entry.revision),
            is_pending: entry.is_pending,
            is_empty: false,
        }))
    }

    /// HTTP hook이 보낸 경로는 열지 않고, 기존 공급자 탐색이 허용한 파일만 쓴다. 그 사이 바인딩이 바뀌었으면 `None`이다
    fn resolve_reader_source(
        &self,
        key: &BindingKey,
        entry: &SessionBinding,
    ) -> AiResult<Option<String>> {
        let mut context = entry.context.reader.clone();
        context.query = None;
        let sessions = self.catalog.read_sessions(entry.provider, &context)?;
        let mut table = self.lock();
        let Some((_, current)) = table.entries.iter_mut().find(|(known, _)| known == key) else {
            return Ok(None);
        };
        if current.observation != entry.observation {
            return Ok(None);
        }
        let exact = sessions
            .into_iter()
            .find(|session| {
                session.id == entry.session_id
                    && session
                        .source_ref
                        .as_deref()
                        .is_some_and(|source| !source.is_empty())
            })
            .and_then(|session| session.source_ref);
        current.source_ref.clone_from(&exact);
        Ok(exact)
    }

    /// CLI가 알려 준 대화 ID가 이 provider의 리더가 허용한 기록에 있으면 그 기록으로 잇는다
    pub fn find_pane_chat_reported_session_binding(
        &self,
        context: &AiSessionReaderContext,
        provider: PaneChatProvider,
        session_id: &str,
    ) -> AiResult<Option<PaneChatSessionBinding>> {
        if !is_session_id(session_id) {
            return Ok(None);
        }
        if provider == PaneChatProvider::Codex {
            let source_ref = self
                .catalog
                .find_codex_main_thread_rollout(context, session_id)?;
            return Ok(source_ref
                .filter(|source_ref| !source_ref.is_empty())
                .map(|source_ref| PaneChatSessionBinding::exact(session_id, &source_ref)));
        }
        let mut context = context.clone();
        context.query = None;
        let sessions = self.catalog.read_sessions(provider, &context)?;
        Ok(sessions
            .iter()
            .find(|session| {
                session.id == session_id
                    && session
                        .source_ref
                        .as_deref()
                        .is_some_and(|source| !source.is_empty())
            })
            .map(|session| {
                PaneChatSessionBinding::exact(
                    session_id,
                    session.source_ref.as_deref().unwrap_or_default(),
                )
            }))
    }

    /// hook 없이 프로세스 기록만으로 pane의 대화를 찾는다. OpenCode는 KanVibe 플러그인이 남긴 경로 기록을,
    /// Claude는 CLI가 남긴 `~/.claude/sessions/<pid>.json`과 실제 프로세스 시작 표시를 대조한다. 읽지 못하면 `None`이다
    pub fn find_pane_chat_process_session_binding(
        &self,
        context: &AiSessionReaderContext,
        process: &PaneChatProcess,
    ) -> Option<PaneChatSessionBinding> {
        match process.provider {
            PaneChatProvider::Opencode => read_opencode_route(context, process),
            PaneChatProvider::Claude if process.pid > 1 => self
                .read_claude_process_record(context, process)
                .ok()
                .flatten(),
            _ => None,
        }
    }

    fn read_claude_process_record(
        &self,
        context: &AiSessionReaderContext,
        process: &PaneChatProcess,
    ) -> AiResult<Option<PaneChatSessionBinding>> {
        let home = context.host.home_directory()?;
        let record_path = node_path::join(&[
            &home,
            ".claude",
            "sessions",
            &format!("{}.json", process.pid),
        ]);
        let Ok(record) = serde_json::from_str::<Value>(&context.host.read_text_file(&record_path)?)
        else {
            return Ok(None);
        };
        let Some(descriptor) = read_claude_process_descriptor(&record, process.pid, context) else {
            return Ok(None);
        };
        if !is_same_process_incarnation(context, process.pid, &descriptor)? {
            return Ok(None);
        }
        let project_directory = node_path::normalize(&descriptor.cwd).replace(['/', '_'], "-");
        let source_ref = node_path::join(&[
            &home,
            ".claude",
            "projects",
            &project_directory,
            &format!("{}.jsonl", descriptor.session_id),
        ]);
        if context.host.path_exists(&source_ref)? {
            return Ok(Some(PaneChatSessionBinding::exact(
                &descriptor.session_id,
                &source_ref,
            )));
        }
        let pending = PaneChatSessionBinding {
            is_pending: true,
            ..PaneChatSessionBinding::exact(&descriptor.session_id, &source_ref)
        };
        if !has_transcript_named_after(context, &home, &descriptor.session_id)? {
            return Ok(Some(pending));
        }
        let existing = self.find_pane_chat_reported_session_binding(
            context,
            PaneChatProvider::Claude,
            &descriptor.session_id,
        )?;
        Ok(Some(existing.unwrap_or(pending)))
    }
}

/// 그 대화 ID 이름의 기록 파일이 Claude 프로젝트 디렉터리 어디든 있는지. 이름만 훑고 내용은 읽지 않는다.
///
/// 첫 입력 전의 새 대화는 기록 파일이 없다. 이때 세션 리더로 찾으면 그 작업 폴더의 기록이 아직 없어 모든 프로젝트의 기록을
/// 열어 보게 되고, 기록이 많은 머신에서는 조회 한 번이 몇 초 걸린다. 같은 이름의 파일이 어디에도 없으면 리더도 그 대화를
/// 찾지 못하므로 바로 대기 중인 새 대화로 잇는다. 다른 대화의 기록은 이 판정으로 연결되지 않는다
fn has_transcript_named_after(
    context: &AiSessionReaderContext,
    home: &str,
    session_id: &str,
) -> AiResult<bool> {
    let projects = node_path::join(&[home, ".claude", "projects"]);
    let file_name = format!("{session_id}.jsonl");
    let found = context
        .host
        .list_files_recursively_by_suffix(&projects, &file_name)?;
    Ok(found
        .iter()
        .any(|path| node_path::basename(path) == file_name))
}

/// Claude Code가 남긴 실행 중 프로세스 기록에서 쓰는 값
struct ClaudeProcessDescriptor {
    session_id: String,
    cwd: String,
    proc_start: String,
    pid_domain: String,
}

fn read_claude_process_descriptor(
    record: &Value,
    pid: i64,
    context: &AiSessionReaderContext,
) -> Option<ClaudeProcessDescriptor> {
    let non_empty = |key: &str| {
        record
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    let is_same_pid = record.get("pid").and_then(Value::as_f64) == Some(pid as f64);
    let is_interactive = record.get("kind").and_then(Value::as_str) == Some("interactive");
    let session_id = record
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|id| is_session_id(id))?;
    let cwd = record
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| node_path::is_absolute(cwd))?;
    let is_in_task = determine_match_scope(Some(cwd), context).is_some();
    (is_same_pid && is_interactive && is_in_task)
        .then(|| ClaudeProcessDescriptor {
            session_id: session_id.to_string(),
            cwd: cwd.to_string(),
            proc_start: non_empty("procStart").unwrap_or_default().to_string(),
            pid_domain: non_empty("pidDomain").unwrap_or_default().to_string(),
        })
        .filter(|descriptor| !descriptor.proc_start.is_empty() && !descriptor.pid_domain.is_empty())
}

/// 기록의 시작 표시와 PID 영역이 지금 그 PID로 도는 프로세스와 같은지. Linux는 `/proc/<pid>/stat`의 22번째 값과
/// 머신·PID 네임스페이스를, `/proc`가 없는 macOS는 UTC 시작 시각과 플랫폼 이름("darwin")을 견준다
fn is_same_process_incarnation(
    context: &AiSessionReaderContext,
    pid: i64,
    descriptor: &ClaudeProcessDescriptor,
) -> AiResult<bool> {
    let probe_command = [
        format!("if test -r /proc/{pid}/stat; then"),
        format!("  cat /proc/{pid}/stat"),
        "  printf 'linux:%s:%s\\n' \"$(cat /etc/machine-id)\" \"$(readlink /proc/self/ns/pid)\""
            .to_string(),
        "else".to_string(),
        format!("  LC_ALL=C TZ=UTC ps -o lstart= -p {pid}"),
        "  uname -s | tr '[:upper:]' '[:lower:]'".to_string(),
        "fi".to_string(),
    ]
    .join("\n");
    let options = CommandOptions {
        max_buffer_bytes: None,
        timeout_ms: Some(PROCESS_PROBE_TIMEOUT_MS),
    };
    let probe = context.host.run(&probe_command, options)?;
    let trimmed = js_trim_end(&probe);
    let mut lines = trimmed.split('\n');
    let process_record = lines.next().unwrap_or_default();
    let domain = lines.next();
    let marker = match process_record.rfind(") ") {
        Some(index) => WHITESPACE_RUN
            .split(&process_record[index + 2..])
            .nth(19)
            .map(str::to_string),
        None => Some(js_trim(process_record).to_string()),
    };
    Ok(marker.as_deref() == Some(descriptor.proc_start.as_str())
        && domain == Some(descriptor.pid_domain.as_str()))
}

/// KanVibe OpenCode 플러그인이 `<worktree>/.kanvibe/pane-chat/opencode-<pid>.json`에 남긴 현재 세션.
/// 같은 PID·시작 시각·작업 경로이고 5초 안에 갱신된 기록만 믿는다
fn read_opencode_route(
    context: &AiSessionReaderContext,
    process: &PaneChatProcess,
) -> Option<PaneChatSessionBinding> {
    let workspace = context
        .worktree_path
        .as_deref()
        .or(context.repo_path.as_deref())
        .filter(|workspace| !workspace.is_empty())?;
    let source_ref = node_path::join(&[
        workspace,
        ".kanvibe",
        "pane-chat",
        &format!("opencode-{}.json", process.pid),
    ]);
    let record: Value =
        serde_json::from_str(&context.host.read_text_file(&source_ref).ok()?).ok()?;
    let is_same_process = record.get("pid").and_then(Value::as_f64) == Some(process.pid as f64)
        && record.get("processStartTime").and_then(Value::as_str)
            == Some(normalize_start_time(&process.process_start_time).as_str());
    let is_in_task =
        determine_match_scope(record.get("cwd").and_then(Value::as_str), context).is_some();
    let revision =
        is_safe_integer(record.get("revision")).filter(|revision| *revision >= 1.0)? as u64;
    let updated_at = record.get("updatedAt").and_then(Value::as_f64)?;
    if !is_same_process || !is_in_task || (now_ms() - updated_at).abs() > OPENCODE_ROUTE_MAX_AGE_MS
    {
        return None;
    }
    match record.get("sessionId") {
        Some(Value::Null) => Some(PaneChatSessionBinding {
            revision: Some(revision),
            is_empty: true,
            ..PaneChatSessionBinding::exact(&format!("opencode-home:{}", process.pid), &source_ref)
        }),
        Some(Value::String(session_id)) if OPENCODE_SESSION_ID.is_match(session_id) => {
            Some(PaneChatSessionBinding {
                revision: Some(revision),
                ..PaneChatSessionBinding::exact(session_id, session_id)
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
