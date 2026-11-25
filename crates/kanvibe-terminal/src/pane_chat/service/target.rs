//! 입력 대상의 확인과 lease. 같은 pane·같은 CLI 프로세스·같은 대화인지를 쓰기 직전마다 다시 확인한다.

use std::sync::Arc;
use std::time::Duration;

use kanvibe_ai::javascript::{js_trim, stringify};
use kanvibe_ai::pane_chat::codex_display::is_same_composer_text;
use kanvibe_ai::pane_chat::process::{PaneChatProcess, find_pane_chat_process};
use kanvibe_ai::pane_chat::snapshot::PaneChatInputState;
use kanvibe_domain::task::SessionType;
use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::worktree::quote_for_posix_shell;
use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

use super::codex_display::codex_display;
use super::input_state::{EMPTY_CODEX_COMPOSER_HINT, read_input_state};
use super::{PaneChatError, PaneChatService, lock, unconfirmed_input_error};
use crate::pane_chat::surface::PaneChatSurface;

const PROCESS_TABLE_COLUMNS: &str = "pid=,ppid=,pgid=,tpgid=,tty=,lstart=,ucomm=,args=";
pub(super) const PROCESS_PROBE_TIMEOUT: Duration = Duration::from_millis(5_000);
pub(super) const MAX_TARGET_LEASES: usize = 256;
const TARGET_LEASE_TTL_MS: f64 = 10.0 * 60.0 * 1_000.0;

static TERMINAL_DEVICE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:pts/[0-9]+|tty[a-zA-Z0-9]+)$").expect("tty 패턴"));
static TERMINAL_SIZE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "^[0-9]+[{}]+([0-9]+)$",
        kanvibe_ai::javascript::JS_WHITESPACE_CLASS
    ))
    .expect("stty 크기 패턴")
});

/// 입력을 보내도 된다고 확인한 대상. 대상이 바뀌면 lease를 회수해 같은 token으로 쓰지 못하게 한다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TargetLease {
    pub task_id: String,
    pub requested_pane_id: Option<String>,
    pub pane_id: String,
    pub identity: String,
    pub process: PaneChatProcess,
    pub conversation: Option<String>,
    pub conversation_key: Option<String>,
    pub ssh_host: Option<String>,
}

struct LeaseEntry {
    lease: Arc<TargetLease>,
    expires_at: f64,
}

/// 발급 순서를 지키는 lease 목록. 가득 차면 가장 먼저 발급한 것부터 버린다
#[derive(Default)]
pub(super) struct LeaseTable {
    entries: Vec<(String, LeaseEntry)>,
}

impl LeaseTable {
    fn get(&self, token: &str) -> Option<&LeaseEntry> {
        self.entries
            .iter()
            .find(|(candidate, _)| candidate == token)
            .map(|(_, entry)| entry)
    }

    fn remove(&mut self, token: &str) {
        self.entries.retain(|(candidate, _)| candidate != token);
    }
}

pub(super) fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn optional(value: &Option<String>) -> Value {
    value.as_deref().map_or(Value::Null, |text| json!(text))
}

fn session_type_value(surface: &PaneChatSurface) -> Value {
    surface
        .task
        .session_type
        .map_or(Value::Null, |session_type| json!(session_type.as_str()))
}

/// CLI 실행 하나를 가리키는 값. 태스크·pane·root PID·CLI PID와 시작 시각이 모두 같아야 같은 실행이다
pub(super) fn identity_for(surface: &PaneChatSurface, process: &PaneChatProcess) -> String {
    let task = &surface.task;
    let socket = process.codex_server_socket.clone().flatten();
    stringify(&json!([
        task.id,
        optional(&task.ssh_host),
        session_type_value(surface),
        optional(&task.session_name),
        optional(&task.worktree_path),
        optional(&task.project_repo_path),
        surface.pane_id,
        surface.root_pid,
        process.provider,
        process.pid,
        process.process_start_time,
        socket,
    ]))
}

/// host·작업·pane·대화 범위의 draft 키
pub(super) fn conversation_key_for(
    surface: &PaneChatSurface,
    process: &PaneChatProcess,
    conversation: Option<&str>,
) -> Option<String> {
    let (session_id, source_ref) = conversation_parts(conversation?);
    let task = &surface.task;
    Some(sha256_hex(&stringify(&json!([
        task.id,
        optional(&task.ssh_host),
        session_type_value(surface),
        optional(&task.session_name),
        optional(&task.worktree_path),
        optional(&task.project_repo_path),
        surface.pane_id,
        process.provider,
        session_id,
        source_ref,
    ]))))
}

/// `[sessionId, sourceRef, revision]` 대화 값에서 앞의 둘을 꺼낸다
pub(super) fn conversation_parts(conversation: &str) -> (String, String) {
    let parsed: Value = serde_json::from_str(conversation).unwrap_or(Value::Null);
    let part = |index: usize| parsed[index].as_str().unwrap_or_default().to_string();
    (part(0), part(1))
}

/// pane root와 같은 터미널에 붙은 프로세스만 조회한다. 판정은 root의 tty에 있는 자손만 보므로 전체 프로세스 표를 읽을 필요가 없다.
/// tty가 없거나 확인되지 않으면 아무것도 출력하지 않아 "CLI 없음"으로 판정된다
pub(super) fn build_pane_process_table_command(root_pid: i64) -> String {
    format!(
        "pane_tty=$(ps -o tty= -p {root_pid} | tr -d ' '); case \"$pane_tty\" in \"\"|\"?\"|\"??\") ;; *) ps -wwo {PROCESS_TABLE_COLUMNS} -t \"$pane_tty\" ;; esac"
    )
}

pub(super) fn probe_options() -> ExecGitOptions {
    ExecGitOptions {
        timeout: Some(PROCESS_PROBE_TIMEOUT),
        ..ExecGitOptions::default()
    }
}

impl PaneChatService {
    pub(super) fn exec_probe(
        &self,
        command: &str,
        ssh_host: Option<&str>,
    ) -> Result<String, PaneChatError> {
        self.runner
            .exec_git(command, ssh_host, &probe_options())
            .map_err(|error| PaneChatError::Other(error.message))
    }

    pub(super) fn inspect_process(
        &self,
        surface: &PaneChatSurface,
    ) -> Result<Option<PaneChatProcess>, PaneChatError> {
        let Some(root_pid) = surface.root_pid.filter(|pid| *pid != 0) else {
            return Ok(None);
        };
        let output = self.exec_probe(
            &build_pane_process_table_command(root_pid),
            surface.task.ssh_host(),
        )?;
        Ok(find_pane_chat_process(&output, root_pid))
    }

    /// 같은 pane을 화면까지 다시 읽고, 같은 CLI 실행이 아니면 대상이 바뀐 것으로 본다
    pub(super) fn same_execution_surface(
        &self,
        task_id: &str,
        pane_id: &str,
        identity: &str,
    ) -> Result<PaneChatSurface, PaneChatError> {
        let surface = self
            .surfaces
            .read_pane_chat_surface(task_id, Some(pane_id), true)?;
        let process = match &surface {
            Some(surface) => self.inspect_process(surface)?,
            None => None,
        };
        match (surface, process) {
            (Some(surface), Some(process)) if identity_for(&surface, &process) == identity => {
                Ok(surface)
            }
            _ => Err(PaneChatError::TargetChanged),
        }
    }

    pub(super) fn current_lease(&self, token: &str) -> Option<Arc<TargetLease>> {
        lock(&self.leases)
            .get(token)
            .map(|entry| entry.lease.clone())
    }

    pub(super) fn is_current_lease(&self, token: &str, lease: &Arc<TargetLease>) -> bool {
        self.current_lease(token)
            .is_some_and(|current| Arc::ptr_eq(&current, lease))
    }

    pub(super) fn revoke_lease(&self, token: &str) {
        lock(&self.leases).remove(token);
    }

    /// 같은 pane에서 다른 실행이나 다른 대화를 가리키는 lease를 회수한다
    pub(super) fn retire_changed_leases(
        &self,
        task_id: &str,
        pane_id: &str,
        identity: &str,
        conversation: Option<&str>,
    ) {
        let retired: Vec<String> = {
            let mut table = lock(&self.leases);
            let (retired, kept) =
                std::mem::take(&mut table.entries)
                    .into_iter()
                    .partition(|(_, entry)| {
                        let lease = &entry.lease;
                        lease.task_id == task_id
                            && lease.pane_id == pane_id
                            && (lease.identity != identity
                                || lease.conversation.as_deref() != conversation)
                    });
            table.entries = kept;
            retired.into_iter().map(|(token, _)| token).collect()
        };
        for token in retired {
            self.forget_receipts(&token);
        }
    }

    /// 같은 실행·요청 pane·대화의 살아 있는 lease가 있으면 그 token을 연장하고, 없으면 새로 발급한다
    pub(super) fn lease_target(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
        requested_pane_id: Option<&str>,
        conversation: Option<&str>,
    ) -> String {
        let now = self.now_ms();
        let identity = identity_for(surface, process);
        let mut forgotten = Vec::new();
        let token = {
            let mut table = lock(&self.leases);
            let renewed = renew_matching_lease(
                &mut table,
                &mut forgotten,
                now,
                (&identity, requested_pane_id, conversation),
            );
            renewed.unwrap_or_else(|| {
                if table.entries.len() >= MAX_TARGET_LEASES {
                    forgotten.push(table.entries.remove(0).0);
                }
                let token = uuid::Uuid::new_v4().to_string();
                let lease = TargetLease {
                    task_id: surface.task.id.clone(),
                    requested_pane_id: requested_pane_id.map(str::to_string),
                    pane_id: surface.pane_id.clone(),
                    identity,
                    process: process.clone(),
                    conversation: conversation.map(str::to_string),
                    conversation_key: conversation_key_for(surface, process, conversation),
                    ssh_host: surface.task.ssh_host.clone(),
                };
                let entry = LeaseEntry {
                    lease: Arc::new(lease),
                    expires_at: now + TARGET_LEASE_TTL_MS,
                };
                table.entries.push((token.clone(), entry));
                token
            })
        };
        for token in forgotten {
            self.forget_receipts(&token);
        }
        token
    }
}

/// TS처럼 앞에서부터 보며 만료된 lease를 지우다가 같은 대상의 lease를 만나면 연장하고 멈춘다
fn renew_matching_lease(
    table: &mut LeaseTable,
    forgotten: &mut Vec<String>,
    now: f64,
    (identity, requested_pane_id, conversation): (&str, Option<&str>, Option<&str>),
) -> Option<String> {
    let mut index = 0;
    while index < table.entries.len() {
        let (token, entry) = &mut table.entries[index];
        if entry.expires_at < now {
            forgotten.push(token.clone());
            table.entries.remove(index);
            continue;
        }
        let lease = &entry.lease;
        let is_same_target = lease.identity == identity
            && lease.requested_pane_id.as_deref() == requested_pane_id
            && lease.conversation.as_deref() == conversation;
        if is_same_target {
            entry.expires_at = now + TARGET_LEASE_TTL_MS;
            return Some(token.clone());
        }
        index += 1;
    }
    None
}

impl PaneChatService {
    /// lease가 가리키는 pane을 다시 읽어 같은 실행·같은 대화인지 확인한다.
    /// `expected_composer`를 주면 Codex 표시 대상의 입력란이 그 글 중 하나를 그대로 보여 주는지도 확인한다
    pub(super) fn assert_target(
        &self,
        task_id: &str,
        token: &str,
        expected_composer: Option<&[String]>,
    ) -> Result<Arc<TargetLease>, PaneChatError> {
        let lease = self.valid_lease(task_id, token)?;
        let (surface, process) = self.read_lease_target(task_id, token, &lease)?;
        let conversation = self
            .resolve_conversation(&surface, &process)
            .map(|binding| super::conversation::conversation_identity(binding.as_ref()))
            .map_err(|_| PaneChatError::TargetChanged)?;
        self.retire_changed_leases(
            task_id,
            &surface.pane_id,
            &lease.identity,
            conversation.as_deref(),
        );
        if conversation != lease.conversation {
            return Err(PaneChatError::TargetChanged);
        }
        let display = self.codex_display_execution(&surface, &process);
        let mut surface = surface;
        let mut composer_columns = surface.viewport_columns;
        if display.is_some() {
            let is_herdr = surface.task.session_type == Some(SessionType::Herdr);
            if let Some(expected) = expected_composer.filter(|_| is_herdr) {
                composer_columns = self.read_herdr_composer_columns(&surface, &process, expected);
            }
            surface = self.same_execution_surface(task_id, &lease.pane_id, &lease.identity)?;
            let binding = self.resolve_conversation(&surface, &process)?;
            if super::conversation::conversation_identity(binding.as_ref()) != lease.conversation {
                self.revoke_lease(token);
                return Err(PaneChatError::TargetChanged);
            }
            if !is_herdr {
                composer_columns = surface.viewport_columns;
            }
        }
        if !self.is_current_lease(token, &lease) {
            return Err(PaneChatError::TargetChanged);
        }
        if let (Some(expected), Some(_)) = (expected_composer, &display) {
            verify_codex_composer(&surface, &process, expected, composer_columns)?;
        }
        Ok(lease)
    }

    fn valid_lease(&self, task_id: &str, token: &str) -> Result<Arc<TargetLease>, PaneChatError> {
        let now = self.now_ms();
        let table = lock(&self.leases);
        match table.get(token) {
            Some(entry) if entry.lease.task_id == task_id && entry.expires_at >= now => {
                Ok(entry.lease.clone())
            }
            _ => Err(PaneChatError::TargetChanged),
        }
    }

    /// lease의 pane을 다시 읽는다. Codex 표시 대상은 대화가 화면 하단에서만 읽히므로 화면까지 읽는다
    fn read_lease_target(
        &self,
        task_id: &str,
        token: &str,
        lease: &TargetLease,
    ) -> Result<(PaneChatSurface, PaneChatProcess), PaneChatError> {
        let is_display = self.existing_codex_display(&lease.identity).is_some();
        let read = || -> Result<_, PaneChatError> {
            let surface = self.surfaces.read_pane_chat_surface(
                task_id,
                lease.requested_pane_id.as_deref(),
                is_display,
            )?;
            let process = match &surface {
                Some(surface) => self.inspect_process(surface)?,
                None => None,
            };
            Ok((surface, process))
        };
        let (surface, process) = read().map_err(|_| PaneChatError::TargetChanged)?;
        match (surface, process) {
            (Some(surface), Some(process))
                if identity_for(&surface, &process) == lease.identity =>
            {
                Ok((surface, process))
            }
            _ => {
                self.revoke_lease(token);
                Err(PaneChatError::TargetChanged)
            }
        }
    }

    /// Herdr의 관찰 프레임은 실제 PTY보다 넓게 패딩될 수 있어, 접힌 글을 비교할 때만 원래 PTY 너비를 읽는다.
    /// 너비를 얻지 못하면 줄바꿈을 추정하지 않는다
    fn read_herdr_composer_columns(
        &self,
        surface: &PaneChatSurface,
        process: &PaneChatProcess,
        expected: &[String],
    ) -> Option<usize> {
        let echoed = codex_display(surface)?.composer_text;
        if expected
            .iter()
            .any(|text| is_same_composer_text(&echoed, text, None))
        {
            return None;
        }
        let ssh_host = surface.task.ssh_host();
        let tty_output = self
            .exec_probe(&format!("ps -p {} -o tty=", process.pid), ssh_host)
            .ok()?;
        let tty = js_trim(&tty_output);
        if !TERMINAL_DEVICE.is_match(tty) {
            return None;
        }
        let device = quote_for_posix_shell(&format!("/dev/{tty}"));
        let size = self
            .exec_probe(
                &format!("stty -F {device} size 2>/dev/null || stty -f {device} size 2>/dev/null"),
                ssh_host,
            )
            .ok()?;
        let columns: usize = TERMINAL_SIZE.captures(js_trim(&size))?[1].parse().ok()?;
        (columns > 2 && columns <= 500).then_some(columns)
    }
}

/// Codex 입력란이 기대한 글 중 하나를 그대로 보여 주는지. 빈 입력란을 기대하면 Codex 고유의 빈 입력란 표시까지 본다
fn verify_codex_composer(
    surface: &PaneChatSurface,
    process: &PaneChatProcess,
    expected: &[String],
    composer_columns: Option<usize>,
) -> Result<(), PaneChatError> {
    let echoed = codex_display(surface).map(|display| display.composer_text);
    let expects_empty = expected.iter().any(String::is_empty)
        && expected
            .iter()
            .any(|text| text == EMPTY_CODEX_COMPOSER_HINT);
    let is_empty_unconfirmed =
        expects_empty && read_input_state(surface, process, true) != PaneChatInputState::Ready;
    let is_echoed = echoed.is_some_and(|echoed| {
        expected
            .iter()
            .any(|text| is_same_composer_text(&echoed, text, composer_columns))
    });
    if is_empty_unconfirmed || !is_echoed {
        return Err(unconfirmed_input_error());
    }
    Ok(())
}
