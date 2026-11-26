//! OpenCode가 KanVibe 플러그인을 실제로 로드했는지 `opencode debug config`로 확인한다(TS `openCodePluginRegistry.ts`).
//! 같은 저장소에 대한 조회는 진행 중인 것을 함께 쓰고, 끝난 결과(실패 포함)는 5초 동안 다시 쓴다.
//! 설치 직후에는 이전 결과를 버려 다음 조회가 새 설정을 읽게 한다.

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use crate::shell_environment::ShellEnvironment;

const REGISTRATION_CACHE_TTL: Duration = Duration::from_millis(5000);
const DEBUG_CONFIG_TIMEOUT: Duration = Duration::from_millis(3000);
const DEBUG_CONFIG_MAX_BUFFER_BYTES: usize = 1024 * 1024;
/// 자식이 끝나거나 죽은 뒤 출력 파이프가 닫히기를 기다리는 시간
const STREAM_CLOSE_GRACE: Duration = Duration::from_millis(500);

static PLUGIN_BLOCK_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""plugin"\s*:\s*\[([\s\S]*?)\]\s*,\s*"[^"]+"\s*:"#).expect("유효한 정규식")
});
static QUOTED_STRING_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""((?:\\.|[^"\\])*)""#).expect("유효한 정규식"));
static KANVIBE_PLUGIN_URL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)/kanvibe-plugin\.(?:[cm]?js|ts)$").expect("유효한 정규식"));

/// `opencode debug config` 실행. 실패하면 Node `execFile` 오류 메시지를 돌려준다
pub trait OpenCodeDebugConfigCommand: Send + Sync {
    fn run(&self, working_directory: &str) -> Result<String, String>;
}

/// OpenCode 등록 조회. 상태 판정과 설치기는 이 trait만 받는다
pub trait OpenCodePluginRegistration: Send + Sync {
    /// 저장소에서 OpenCode가 로드하는 KanVibe 플러그인 URL 목록
    fn registered_kanvibe_plugin_urls(&self, repo_path: &str) -> Result<Vec<String>, String>;
    /// 설치로 바뀐 저장소의 이전 조회 결과를 버린다
    fn invalidate(&self, repo_path: &str);
}

/// 실제 `opencode` CLI를 좁힌 로컬 셸 환경으로 실행한다. 3초가 지나면 SIGKILL로 끝낸다
pub struct SystemOpenCodeDebugConfigCommand {
    environment: ShellEnvironment,
}

impl SystemOpenCodeDebugConfigCommand {
    /// `environment`는 `create_local_shell_environment`로 만든 값이어야 한다
    pub fn new(environment: ShellEnvironment) -> Self {
        Self { environment }
    }
}

impl OpenCodeDebugConfigCommand for SystemOpenCodeDebugConfigCommand {
    fn run(&self, working_directory: &str) -> Result<String, String> {
        // 자손이 출력 파이프를 물려받아도 한 번에 끝낼 수 있게 자기 프로세스 그룹으로 띄운다
        let mut child = Command::new("opencode")
            .args(["debug", "config"])
            .current_dir(working_directory)
            .env_clear()
            .envs(&self.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| describe_spawn_error(&error))?;
        let stdout = read_stream(child.stdout.take().expect("piped stdout"));
        let stderr = read_stream(child.stderr.take().expect("piped stderr"));
        let process_group_id = child.id();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || drop(sender.send(child.wait())));
        let status = receiver.recv_timeout(DEBUG_CONFIG_TIMEOUT).ok();
        if status.is_none() {
            kill_process_group(process_group_id);
        }
        // 파이프를 쥔 자손이 남아도 읽기를 기다리는 시간은 늘 상한 안에 둔다
        let streams_deadline = Instant::now() + STREAM_CLOSE_GRACE;
        let stdout = receive_before(&stdout, streams_deadline);
        let stderr = receive_before(&stderr, streams_deadline);
        let Some(stdout) = stdout else {
            kill_process_group(process_group_id);
            return Err(format!(
                "Command failed: opencode debug config\n{}",
                String::from_utf8_lossy(&stderr.unwrap_or_default())
            ));
        };
        let stderr = String::from_utf8_lossy(&stderr.unwrap_or_default()).into_owned();
        if stdout.len() > DEBUG_CONFIG_MAX_BUFFER_BYTES {
            return Err("stdout maxBuffer length exceeded".into());
        }
        match status {
            Some(Ok(status)) if status.success() => {
                Ok(String::from_utf8_lossy(&stdout).into_owned())
            }
            _ => Err(format!("Command failed: opencode debug config\n{stderr}")),
        }
    }
}

/// `opencode`와 그 자손 전체에 SIGKILL을 보낸다
fn kill_process_group(process_group_id: u32) {
    // SAFETY: `process_group(0)`으로 만든, 이 호출이 띄운 자식의 프로세스 그룹에만 신호를 보낸다
    unsafe { libc::kill(-(process_group_id as libc::pid_t), libc::SIGKILL) };
}

fn receive_before(stream: &mpsc::Receiver<Vec<u8>>, deadline: Instant) -> Option<Vec<u8>> {
    stream
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
}

/// Node `execFile`의 spawn 실패 메시지(`spawn opencode ENOENT` 등)
fn describe_spawn_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "spawn opencode ENOENT".into(),
        std::io::ErrorKind::PermissionDenied => "spawn opencode EACCES".into(),
        _ => format!("spawn opencode {error}"),
    }
}

fn read_stream(mut stream: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        drop(sender.send(bytes));
    });
    receiver
}

/// 조회 하나의 결과와 수명. `expires_at`이 없으면 아직 진행 중이다
struct RegistrationQuery {
    outcome: Mutex<Option<Result<Vec<String>, String>>>,
    finished: Condvar,
    expires_at: Mutex<Option<Instant>>,
    is_invalidated: Mutex<bool>,
}

impl RegistrationQuery {
    fn new() -> Self {
        Self {
            outcome: Mutex::new(None),
            finished: Condvar::new(),
            expires_at: Mutex::new(None),
            is_invalidated: Mutex::new(false),
        }
    }

    fn wait(&self) -> Result<Vec<String>, String> {
        let mut outcome = self.outcome.lock().unwrap();
        while outcome.is_none() {
            outcome = self.finished.wait(outcome).unwrap();
        }
        outcome.clone().expect("방금 확인했다")
    }

    fn is_fresh(&self, now: Instant) -> bool {
        self.expires_at
            .lock()
            .unwrap()
            .is_none_or(|expires_at| expires_at > now)
    }
}

/// 같은 저장소의 조회를 공유·캐시하는 등록 조회기
pub struct OpenCodePluginRegistry {
    command: Arc<dyn OpenCodeDebugConfigCommand>,
    clock: Arc<dyn Fn() -> Instant + Send + Sync>,
    queries: Mutex<HashMap<String, Arc<RegistrationQuery>>>,
}

impl OpenCodePluginRegistry {
    pub fn new(command: Arc<dyn OpenCodeDebugConfigCommand>) -> Self {
        Self::with_clock(command, Arc::new(Instant::now))
    }

    /// 5초 캐시를 시험하려고 시계를 바꿔 끼운다
    pub fn with_clock(
        command: Arc<dyn OpenCodeDebugConfigCommand>,
        clock: Arc<dyn Fn() -> Instant + Send + Sync>,
    ) -> Self {
        Self {
            command,
            clock,
            queries: Mutex::new(HashMap::new()),
        }
    }

    /// 진행 중이거나 아직 유효한 조회가 있으면 그 결과를 쓰고, 버려진 조회는 끝나기를 기다린 뒤 새로 묻는다
    fn query(&self, key: &str) -> Result<Vec<String>, String> {
        loop {
            let now = (self.clock)();
            let mut queries = self.queries.lock().unwrap();
            let existing = queries.get(key).cloned();
            if let Some(existing) = existing {
                if *existing.is_invalidated.lock().unwrap() {
                    drop(queries);
                    let _ = existing.wait();
                    self.forget_if_current(key, &existing);
                    continue;
                }
                if existing.is_fresh(now) {
                    drop(queries);
                    return existing.wait();
                }
            }
            queries.retain(|_, query| query.is_fresh(now));
            let query = Arc::new(RegistrationQuery::new());
            queries.insert(key.to_string(), Arc::clone(&query));
            drop(queries);
            return self.run_query(key, &query);
        }
    }

    fn forget_if_current(&self, key: &str, query: &Arc<RegistrationQuery>) {
        let mut queries = self.queries.lock().unwrap();
        if queries
            .get(key)
            .is_some_and(|current| Arc::ptr_eq(current, query))
        {
            queries.remove(key);
        }
    }

    fn run_query(&self, key: &str, query: &RegistrationQuery) -> Result<Vec<String>, String> {
        let outcome = self
            .command
            .run(key)
            .map(|output| extract_registered_kanvibe_plugin_urls(&output));
        *query.expires_at.lock().unwrap() = Some((self.clock)() + REGISTRATION_CACHE_TTL);
        *query.outcome.lock().unwrap() = Some(outcome.clone());
        query.finished.notify_all();
        outcome
    }
}

impl OpenCodePluginRegistration for OpenCodePluginRegistry {
    fn registered_kanvibe_plugin_urls(&self, repo_path: &str) -> Result<Vec<String>, String> {
        self.query(&resolve_absolute_path(repo_path))
    }

    fn invalidate(&self, repo_path: &str) {
        let queries = self.queries.lock().unwrap();
        if let Some(query) = queries.get(&resolve_absolute_path(repo_path)) {
            *query.is_invalidated.lock().unwrap() = true;
        }
    }
}

/// Node `path.resolve`. 상대 경로는 현재 작업 디렉터리 기준이고, `.`·`..`과 끝의 `/`를 정리한다
pub fn resolve_absolute_path(path: &str) -> String {
    let base = if path.starts_with('/') {
        String::new()
    } else {
        std::env::current_dir()
            .map(|directory| directory.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in base.split('/').chain(path.split('/')) {
        match segment {
            "" | "." => {}
            ".." => drop(segments.pop()),
            _ => segments.push(segment),
        }
    }
    format!("/{}", segments.join("/"))
}

/// Electron(Node 20) `pathToFileURL(path).href`. 경로를 절대 경로로 풀고 URL 경로에 쓸 수 없는 문자를 퍼센트 인코딩한다
pub fn file_url_from_path(path: &str) -> String {
    let resolved = resolve_absolute_path(path);
    let mut encoded = String::from("file://");
    for byte in resolved.bytes() {
        let should_encode = byte <= 0x20
            || byte >= 0x7F
            || matches!(
                byte,
                b'"' | b'#' | b'%' | b'<' | b'>' | b'?' | b'\\' | b'`' | b'{' | b'}'
            );
        if should_encode {
            encoded.push_str(&format!("%{byte:02X}"));
        } else {
            encoded.push(byte as char);
        }
    }
    encoded
}

fn plugin_entries(config: &Value) -> Vec<String> {
    match config.get("plugin") {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// OpenCode 설정의 plugin 목록. 출력 뒤쪽에 잘못된 JSON이 섞이면 맨 위 plugin 블록만 읽는다
pub fn extract_registered_plugin_urls(output: &str) -> Vec<String> {
    if let Ok(config) = serde_json::from_str::<Value>(output) {
        let entries = plugin_entries(&config);
        if !entries.is_empty() {
            return entries;
        }
    }
    let Some(plugin_block) = PLUGIN_BLOCK_PATTERN
        .captures(output)
        .and_then(|captures| captures.get(1))
        .filter(|block| !block.as_str().is_empty())
    else {
        return Vec::new();
    };
    QUOTED_STRING_PATTERN
        .captures_iter(plugin_block.as_str())
        .filter_map(|captures| {
            serde_json::from_str::<String>(&format!("\"{}\"", &captures[1])).ok()
        })
        .collect()
}

pub fn is_kanvibe_plugin_url(value: &str) -> bool {
    KANVIBE_PLUGIN_URL_PATTERN.is_match(value)
}

pub fn extract_registered_kanvibe_plugin_urls(output: &str) -> Vec<String> {
    extract_registered_plugin_urls(output)
        .into_iter()
        .filter(|value| is_kanvibe_plugin_url(value))
        .collect()
}

/// 설치한 플러그인 경로가 OpenCode에 등록돼 있는지. 조회 실패는 등록되지 않은 것으로 본다
pub fn is_open_code_plugin_registered(
    registration: &dyn OpenCodePluginRegistration,
    repo_path: &str,
    plugin_path: &str,
) -> bool {
    let expected_plugin_url = file_url_from_path(plugin_path);
    registration
        .registered_kanvibe_plugin_urls(repo_path)
        .is_ok_and(|urls| urls.contains(&expected_plugin_url))
}
