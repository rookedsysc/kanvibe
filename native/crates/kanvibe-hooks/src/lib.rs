use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{SecondsFormat, Utc};
pub use kanvibe_ai::{AiSession, AiSessionProvider, aggregate_ai_sessions};
use kanvibe_core::KanvibeDb;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

pub const QA_SOCKET_ENV: &str = "KANVIBE_QA_SOCKET";
pub const HOOK_SERVER_PORT_ENV: &str = "KANVIBE_HOOK_SERVER_PORT";
pub const DEFAULT_HOOK_SERVER_PORT: u16 = 9736;
pub const DEV_HOOK_SERVER_PORT: u16 = 19736;
const MAX_HOOK_HEADER_BYTES: usize = 16 * 1024;
const MAX_HOOK_BODY_BYTES: usize = 64 * 1024;
pub const AI_PROVIDERS: &[AiSessionProvider] = &AiSessionProvider::ALL;

pub fn is_kanvibe_scoped_env(key: &str) -> bool {
    key == QA_SOCKET_ENV || key == HOOK_SERVER_PORT_ENV || key.starts_with("KANVIBE_")
}

pub fn local_hook_server_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

pub fn remote_hook_server_url(host: &str, port: u16) -> String {
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(_)) => format!("http://[{host}]:{port}"),
        _ => format!("http://{host}:{port}"),
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum HookHttpRoute {
    Start,
    Status,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookHttpRequest {
    pub route: HookHttpRoute,
    pub body: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookHttpResponse {
    pub status_code: u16,
    pub json_body: String,
}

impl HookHttpResponse {
    pub fn json(status_code: u16, json_body: impl Into<String>) -> Self {
        Self {
            status_code,
            json_body: json_body.into(),
        }
    }
}

pub struct HookHttpServer {
    local_addr: SocketAddr,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HookHttpServer {
    pub fn start(
        host: &str,
        port: u16,
        handler: Arc<dyn Fn(HookHttpRequest) -> HookHttpResponse + Send + Sync + 'static>,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(format!("{host}:{port}"))?;
        listener.set_nonblocking(true)?;
        let local_addr = listener.local_addr()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = Arc::clone(&stopping);
        let server_thread = thread::Builder::new()
            .name("kanvibe-hook-http".to_owned())
            .spawn(move || {
                while !thread_stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = handle_hook_connection(&mut stream, handler.as_ref());
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            local_addr,
            stopping,
            thread: Some(server_thread),
        })
    }

    pub const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        let wake_addr = if self.local_addr.ip().is_unspecified() {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), self.local_addr.port())
        } else {
            self.local_addr
        };
        let _ = TcpStream::connect_timeout(&wake_addr, Duration::from_millis(100));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for HookHttpServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_hook_connection(
    stream: &mut TcpStream,
    handler: &(dyn Fn(HookHttpRequest) -> HookHttpResponse + Send + Sync),
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let request = match read_hook_http_request(stream) {
        Ok(request) => request,
        Err(response) => return write_hook_http_response(stream, response),
    };
    let response = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/api/hooks/health") => HookHttpResponse::json(200, r#"{"success":true}"#),
        ("POST", "/api/hooks/start") => handler(HookHttpRequest {
            route: HookHttpRoute::Start,
            body: request.body,
        }),
        ("POST", "/api/hooks/status") => handler(HookHttpRequest {
            route: HookHttpRoute::Status,
            body: request.body,
        }),
        _ => HookHttpResponse::json(404, r#"{"success":false,"error":"Not found"}"#),
    };
    write_hook_http_response(stream, response)
}

struct ParsedHttpRequest {
    method: String,
    path: String,
    body: String,
}

fn read_hook_http_request(stream: &mut TcpStream) -> Result<ParsedHttpRequest, HookHttpResponse> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        if bytes.len() > MAX_HOOK_HEADER_BYTES {
            return Err(HookHttpResponse::json(
                431,
                r#"{"success":false,"error":"Request headers too large"}"#,
            ));
        }
        let read = stream.read(&mut buffer).map_err(|_| {
            HookHttpResponse::json(400, r#"{"success":false,"error":"Invalid request"}"#)
        })?;
        if read == 0 {
            return Err(HookHttpResponse::json(
                400,
                r#"{"success":false,"error":"Incomplete request"}"#,
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            let header_end = index + 4;
            if header_end > MAX_HOOK_HEADER_BYTES {
                return Err(HookHttpResponse::json(
                    431,
                    r#"{"success":false,"error":"Request headers too large"}"#,
                ));
            }
            break header_end;
        }
        if bytes.len() > MAX_HOOK_HEADER_BYTES {
            return Err(HookHttpResponse::json(
                431,
                r#"{"success":false,"error":"Request headers too large"}"#,
            ));
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| {
        HookHttpResponse::json(400, r#"{"success":false,"error":"Invalid headers"}"#)
    })?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    let path = request_parts.next().unwrap_or_default();
    let version = request_parts.next().unwrap_or_default();
    if request_parts.next().is_some()
        || !matches!(method, "GET" | "POST")
        || !version.starts_with("HTTP/1.")
        || !path.starts_with('/')
    {
        return Err(HookHttpResponse::json(
            400,
            r#"{"success":false,"error":"Invalid request line"}"#,
        ));
    }
    let mut content_length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let Some((name, value)) = line.split_once(':') else {
            return Err(HookHttpResponse::json(
                400,
                r#"{"success":false,"error":"Invalid header"}"#,
            ));
        };
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(HookHttpResponse::json(
                400,
                r#"{"success":false,"error":"Transfer-Encoding is not supported"}"#,
            ));
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(HookHttpResponse::json(
                    400,
                    r#"{"success":false,"error":"Duplicate Content-Length"}"#,
                ));
            }
            content_length = Some(value.trim().parse::<usize>().map_err(|_| {
                HookHttpResponse::json(400, r#"{"success":false,"error":"Invalid Content-Length"}"#)
            })?);
        }
    }
    let content_length = content_length.unwrap_or(0);
    if content_length > MAX_HOOK_BODY_BYTES {
        return Err(HookHttpResponse::json(
            413,
            r#"{"success":false,"error":"Request body too large"}"#,
        ));
    }
    let method = method.to_owned();
    let path = path.split('?').next().unwrap_or(path).to_owned();
    let expected_length = header_end + content_length;
    if bytes.len() > expected_length {
        return Err(HookHttpResponse::json(
            400,
            r#"{"success":false,"error":"Unexpected request bytes"}"#,
        ));
    }
    while bytes.len() < expected_length {
        let read = stream.read(&mut buffer).map_err(|_| {
            HookHttpResponse::json(400, r#"{"success":false,"error":"Invalid body"}"#)
        })?;
        if read == 0 {
            return Err(HookHttpResponse::json(
                400,
                r#"{"success":false,"error":"Incomplete body"}"#,
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > expected_length {
            return Err(HookHttpResponse::json(
                400,
                r#"{"success":false,"error":"Unexpected request bytes"}"#,
            ));
        }
    }
    let body = String::from_utf8(bytes[header_end..expected_length].to_vec()).map_err(|_| {
        HookHttpResponse::json(400, r#"{"success":false,"error":"Body must be UTF-8"}"#)
    })?;
    Ok(ParsedHttpRequest { method, path, body })
}

fn write_hook_http_response(
    stream: &mut TcpStream,
    response: HookHttpResponse,
) -> std::io::Result<()> {
    let reason = match response.status_code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "Response",
    };
    let body = response.json_body.as_bytes();
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        response.status_code,
        reason,
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

pub fn extract_shell_hook_server_url(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.strip_prefix("KANVIBE_URL="))
        .and_then(|value| {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    value
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                })
        })
        .map(ToOwned::to_owned)
}

pub fn extract_plugin_hook_server_url(content: &str) -> Option<String> {
    content
        .split("const KANVIBE_URL = \"")
        .nth(1)
        .and_then(|tail| tail.split('"').next())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookServerValidation {
    pub has_expected_hook_server_url: bool,
    pub has_reachable_hook_server: bool,
    pub expected_hook_server_url: Option<String>,
    pub configured_hook_server_url: Option<String>,
}

pub fn validate_hook_server_configuration(
    configured_urls: &[Option<String>],
    expected_url: Option<&str>,
    should_validate: bool,
    is_remote: bool,
    reachable: bool,
) -> HookServerValidation {
    let defined_urls = configured_urls
        .iter()
        .filter_map(|url| url.as_deref())
        .filter(|url| !url.is_empty())
        .collect::<Vec<_>>();
    let configured_hook_server_url = defined_urls.first().map(|url| (*url).to_owned());

    if !should_validate {
        return HookServerValidation {
            has_expected_hook_server_url: true,
            has_reachable_hook_server: true,
            expected_hook_server_url: None,
            configured_hook_server_url,
        };
    }

    let Some(expected_url) = expected_url else {
        return HookServerValidation {
            has_expected_hook_server_url: false,
            has_reachable_hook_server: false,
            expected_hook_server_url: None,
            configured_hook_server_url,
        };
    };

    let has_expected_hook_server_url = defined_urls.len() == configured_urls.len()
        && !defined_urls.is_empty()
        && defined_urls
            .iter()
            .all(|configured| is_expected_hook_server_url(configured, expected_url, is_remote));

    HookServerValidation {
        has_expected_hook_server_url,
        has_reachable_hook_server: has_expected_hook_server_url && reachable,
        expected_hook_server_url: Some(expected_url.to_owned()),
        configured_hook_server_url,
    }
}

fn is_expected_hook_server_url(configured: &str, expected: &str, is_remote: bool) -> bool {
    let Some(configured) = ParsedUrl::parse(configured) else {
        return false;
    };
    let Some(expected) = ParsedUrl::parse(expected) else {
        return false;
    };

    configured.protocol == expected.protocol
        && configured.port == expected.port
        && (is_remote || normalize_loopback(configured.host) == normalize_loopback(expected.host))
}

fn normalize_loopback(host: &str) -> &str {
    if host == "127.0.0.1" {
        "localhost"
    } else {
        host
    }
}

const HOOK_EXCLUDE_MARKER: &str = "# KanVibe AI hooks (auto-generated)";
const HOOK_EXCLUDE_PATTERNS: &[&str] = &[
    ".claude/hooks/",
    ".claude/settings.json",
    ".gemini/hooks/",
    ".gemini/settings.json",
    ".codex/hooks/",
    ".codex/hooks.json",
    ".codex/config.toml",
    ".opencode/plugins/",
    ".kanvibe/",
];
pub const PROVIDER_HOOK_EXCLUDE_LINES: &[&str] = &[
    HOOK_EXCLUDE_MARKER,
    ".claude/hooks/",
    ".claude/settings.json",
    ".gemini/hooks/",
    ".gemini/settings.json",
    ".codex/hooks/",
    ".codex/hooks.json",
    ".codex/config.toml",
    ".opencode/plugins/",
    ".kanvibe/",
];

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LocalHookInstallReport {
    pub written_files: Vec<PathBuf>,
}

pub const PRESERVABLE_PROVIDER_HOOK_PATHS: &[&str] = &[
    ".kanvibe/targets.json",
    ".claude/settings.json",
    ".gemini/settings.json",
    ".codex/hooks.json",
    ".codex/config.toml",
];

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RenderedHookFile {
    pub relative_path: String,
    pub content: String,
    pub executable: bool,
}

pub fn render_provider_hooks(
    existing_files: &BTreeMap<String, String>,
    task_id: &str,
    hook_server_url: &str,
) -> std::io::Result<Vec<RenderedHookFile>> {
    if existing_files
        .keys()
        .any(|path| !PRESERVABLE_PROVIDER_HOOK_PATHS.contains(&path.as_str()))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "remote hook input contains an unsupported path",
        ));
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staging_root = std::env::temp_dir().join(format!(
        "kanvibe-hook-render-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&staging_root)?;
    let result = (|| {
        for (relative_path, content) in existing_files {
            atomic_write(&staging_root.join(relative_path), content, false)?;
        }
        let report = install_local_provider_hooks(&staging_root, task_id, hook_server_url)?;
        report
            .written_files
            .into_iter()
            .map(|path| {
                let relative_path = path.strip_prefix(&staging_root).map_err(|_| {
                    std::io::Error::other("rendered hook escaped its staging directory")
                })?;
                Ok(RenderedHookFile {
                    relative_path: relative_path.to_string_lossy().into_owned(),
                    content: fs::read_to_string(&path)?,
                    executable: path.extension().and_then(|extension| extension.to_str())
                        == Some("sh"),
                })
            })
            .collect::<std::io::Result<Vec<_>>>()
    })();
    let _ = fs::remove_dir_all(&staging_root);
    result
}

pub fn install_local_provider_hooks(
    repository_path: impl AsRef<Path>,
    task_id: &str,
    hook_server_url: &str,
) -> std::io::Result<LocalHookInstallReport> {
    let repository_path = repository_path.as_ref();
    let task_id = task_id.trim();
    let hook_server_url = hook_server_url.trim().trim_end_matches('/');
    if !repository_path.is_dir()
        || task_id.is_empty()
        || task_id.contains(['\r', '\n'])
        || !matches!(
            hook_server_url
                .split_once("://")
                .map(|(scheme, rest)| (scheme, !rest.is_empty())),
            Some(("http" | "https", true))
        )
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid local hook target, task, or server URL",
        ));
    }

    let mut written_files = Vec::new();
    upsert_local_hook_target(
        repository_path,
        task_id,
        hook_server_url,
        &mut written_files,
    )?;
    install_shell_provider_hooks(
        repository_path,
        task_id,
        hook_server_url,
        &mut written_files,
    )?;
    install_opencode_hook(
        repository_path,
        task_id,
        hook_server_url,
        &mut written_files,
    )?;
    let _ = update_ai_hook_git_exclude(repository_path);
    Ok(LocalHookInstallReport { written_files })
}

fn upsert_local_hook_target(
    repository_path: &Path,
    task_id: &str,
    hook_server_url: &str,
    written_files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let path = repository_path.join(".kanvibe/targets.json");
    let mut targets = fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|value| value.get("targets").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|target| {
            let url = target.get("url")?.as_str()?.trim().trim_end_matches('/');
            let id = target.get("taskId")?.as_str()?.trim();
            (!url.is_empty() && !id.is_empty()).then(|| json!({ "url": url, "taskId": id }))
        })
        .collect::<Vec<_>>();
    let mut seen_task_ids = BTreeSet::new();
    targets.retain(|target| {
        target["taskId"]
            .as_str()
            .is_some_and(|task_id| seen_task_ids.insert(task_id.to_owned()))
    });
    targets.retain(|target| target["taskId"].as_str() != Some(task_id));
    targets.push(json!({ "url": hook_server_url, "taskId": task_id }));
    let content = serde_json::to_string_pretty(&json!({
        "schemaVersion": 1,
        "targets": targets,
    }))
    .map_err(std::io::Error::other)?;
    atomic_write(&path, &format!("{content}\n"), false)?;
    written_files.push(path);
    Ok(())
}

fn install_shell_provider_hooks(
    repository_path: &Path,
    task_id: &str,
    hook_server_url: &str,
    written_files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let claude_scripts = [
        ("kanvibe-prompt-hook.sh", "progress"),
        ("kanvibe-question-hook.sh", "pending"),
        ("kanvibe-stop-hook.sh", "review"),
    ];
    write_shell_scripts(
        &repository_path.join(".claude/hooks"),
        &claude_scripts,
        task_id,
        hook_server_url,
        false,
        written_files,
    )?;
    let claude_settings_path = repository_path.join(".claude/settings.json");
    let mut claude_settings = read_json_object(&claude_settings_path);
    upsert_json_hook(
        &mut claude_settings,
        "UserPromptSubmit",
        "kanvibe-prompt-hook.sh",
        None,
        r#""$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh"#,
        10,
    );
    upsert_json_hook(
        &mut claude_settings,
        "PreToolUse",
        "kanvibe-question-hook.sh",
        Some("AskUserQuestion"),
        r#""$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-question-hook.sh"#,
        10,
    );
    upsert_json_hook(
        &mut claude_settings,
        "PostToolUse",
        "kanvibe-prompt-hook.sh",
        Some("AskUserQuestion"),
        r#""$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-prompt-hook.sh"#,
        10,
    );
    upsert_json_hook(
        &mut claude_settings,
        "Stop",
        "kanvibe-stop-hook.sh",
        None,
        r#""$CLAUDE_PROJECT_DIR"/.claude/hooks/kanvibe-stop-hook.sh"#,
        10,
    );
    write_json_object(&claude_settings_path, claude_settings)?;
    written_files.push(claude_settings_path);

    let gemini_scripts = [
        ("kanvibe-prompt-hook.sh", "progress"),
        ("kanvibe-stop-hook.sh", "review"),
    ];
    write_shell_scripts(
        &repository_path.join(".gemini/hooks"),
        &gemini_scripts,
        task_id,
        hook_server_url,
        true,
        written_files,
    )?;
    let gemini_settings_path = repository_path.join(".gemini/settings.json");
    let mut gemini_settings = read_json_object(&gemini_settings_path);
    upsert_json_hook(
        &mut gemini_settings,
        "BeforeAgent",
        "kanvibe-prompt-hook.sh",
        Some("*"),
        r#""$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-prompt-hook.sh"#,
        10_000,
    );
    upsert_json_hook(
        &mut gemini_settings,
        "AfterAgent",
        "kanvibe-stop-hook.sh",
        Some("*"),
        r#""$GEMINI_PROJECT_DIR"/.gemini/hooks/kanvibe-stop-hook.sh"#,
        10_000,
    );
    write_json_object(&gemini_settings_path, gemini_settings)?;
    written_files.push(gemini_settings_path);

    let codex_scripts = [
        ("kanvibe-prompt-hook.sh", "progress"),
        ("kanvibe-permission-hook.sh", "pending"),
        ("kanvibe-pre-tool-hook.sh", "progress"),
        ("kanvibe-stop-hook.sh", "review"),
    ];
    write_shell_scripts(
        &repository_path.join(".codex/hooks"),
        &codex_scripts,
        task_id,
        hook_server_url,
        false,
        written_files,
    )?;
    let codex_hooks_path = repository_path.join(".codex/hooks.json");
    let mut codex_hooks = read_json_object(&codex_hooks_path);
    for (bucket, script_name, matcher) in [
        ("UserPromptSubmit", "kanvibe-prompt-hook.sh", None),
        (
            "PermissionRequest",
            "kanvibe-permission-hook.sh",
            Some("Bash"),
        ),
        ("PreToolUse", "kanvibe-pre-tool-hook.sh", Some("Bash")),
        ("Stop", "kanvibe-stop-hook.sh", None),
    ] {
        let command =
            format!(r#"bash "$(git rev-parse --show-toplevel)/.codex/hooks/{script_name}""#);
        upsert_json_hook(&mut codex_hooks, bucket, script_name, matcher, &command, 10);
    }
    write_json_object(&codex_hooks_path, codex_hooks)?;
    written_files.push(codex_hooks_path);
    let config_path = repository_path.join(".codex/config.toml");
    let config = upsert_codex_hooks_feature(&fs::read_to_string(&config_path).unwrap_or_default());
    atomic_write(&config_path, &config, false)?;
    written_files.push(config_path);
    Ok(())
}

fn write_shell_scripts(
    directory: &Path,
    scripts: &[(&str, &str)],
    task_id: &str,
    hook_server_url: &str,
    emits_json: bool,
    written_files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for (name, status) in scripts {
        let path = directory.join(name);
        atomic_write(
            &path,
            &shell_status_hook(task_id, hook_server_url, status, emits_json),
            true,
        )?;
        written_files.push(path);
    }
    Ok(())
}

fn shell_status_hook(
    task_id: &str,
    hook_server_url: &str,
    status: &str,
    emits_json: bool,
) -> String {
    let json_output = if emits_json { "\necho '{}'" } else { "" };
    format!(
        r#"#!/bin/bash
KANVIBE_URL={}
TASK_ID={}
KANVIBE_STATUS={}
KANVIBE_REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
KANVIBE_STATE_DIR="${{KANVIBE_REPO_ROOT}}/.kanvibe"
KANVIBE_TARGETS_FILE="${{KANVIBE_STATE_DIR}}/targets.json"
mkdir -p "${{KANVIBE_STATE_DIR}}" 2>/dev/null || true
printf '{{"schemaVersion":1,"status":"%s"}}\n' "${{KANVIBE_STATUS}}" > "${{KANVIBE_STATE_DIR}}/status.json" 2>/dev/null || true
KANVIBE_TARGET_ROWS="$(
  if [ -f "${{KANVIBE_TARGETS_FILE}}" ]; then
    grep -oE '"(url|taskId)"[[:space:]]*:[[:space:]]*"[^"]*"' "${{KANVIBE_TARGETS_FILE}}" 2>/dev/null \
      | awk '
          {{ s=$0; sub(/^"[^"]*"[[:space:]]*:[[:space:]]*"/,"",s); sub(/"$/,"",s) }}
          /^"url"/ {{ u=s; sub(/\/+$/,"",u); haveUrl=1; next }}
          /^"taskId"/ {{ if(haveUrl){{ if(u!="" && s!="" && !(s in seen)){{ seen[s]=1; print u "\t" s }} haveUrl=0 }} }}
        ' 2>/dev/null || true
  fi
)"
if [ -z "${{KANVIBE_TARGET_ROWS}}" ]; then
  KANVIBE_TARGET_ROWS="$(printf '%s\t%s\n' "${{KANVIBE_URL%/}}" "${{TASK_ID}}")"
fi
printf '%s\n' "${{KANVIBE_TARGET_ROWS}}" | while IFS="$(printf '\t')" read -r KANVIBE_TARGET_URL KANVIBE_TARGET_TASK_ID; do
  if [ -z "${{KANVIBE_TARGET_URL}}" ] || [ -z "${{KANVIBE_TARGET_TASK_ID}}" ]; then continue; fi
  curl -s -X POST "${{KANVIBE_TARGET_URL%/}}/api/hooks/status" \
    -H "Content-Type: application/json" \
    -d "{{\"taskId\": \"${{KANVIBE_TARGET_TASK_ID}}\", \"status\": \"${{KANVIBE_STATUS}}\"}}" \
    > /dev/null 2>&1 || true
done{json_output}
exit 0
"#,
        shell_single_quote(hook_server_url),
        shell_single_quote(task_id),
        shell_single_quote(status),
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn read_json_object(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<Value>(&content).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn upsert_json_hook(
    settings: &mut Map<String, Value>,
    bucket: &str,
    script_name: &str,
    matcher: Option<&str>,
    command: &str,
    timeout: u64,
) {
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let hooks = hooks.as_object_mut().expect("hooks normalized to object");
    let entries = hooks
        .entry(bucket)
        .or_insert_with(|| Value::Array(Vec::new()));
    if !entries.is_array() {
        *entries = Value::Array(Vec::new());
    }
    let entries = entries
        .as_array_mut()
        .expect("hook bucket normalized to array");
    entries.retain(|entry| !entry.to_string().contains(script_name));
    let mut entry = Map::new();
    if let Some(matcher) = matcher {
        entry.insert("matcher".to_owned(), Value::String(matcher.to_owned()));
    }
    entry.insert(
        "hooks".to_owned(),
        json!([{ "type": "command", "command": command, "timeout": timeout }]),
    );
    entries.push(Value::Object(entry));
}

fn write_json_object(path: &Path, object: Map<String, Value>) -> std::io::Result<()> {
    let content =
        serde_json::to_string_pretty(&Value::Object(object)).map_err(std::io::Error::other)?;
    atomic_write(path, &format!("{content}\n"), false)
}

fn upsert_codex_hooks_feature(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    let mut lines = normalized.lines().map(str::to_owned).collect::<Vec<_>>();
    let Some(start) = lines.iter().position(|line| line.trim() == "[features]") else {
        let prefix = normalized.trim_end();
        return if prefix.is_empty() {
            "[features]\nhooks = true\n".to_owned()
        } else {
            format!("{prefix}\n\n[features]\nhooks = true\n")
        };
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.trim_start().starts_with('['))
        .map_or(lines.len(), |(index, _)| index);
    let retained = lines[(start + 1)..end]
        .iter()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("hooks =")
                && !line.starts_with("codex_hooks =")
                && !line.starts_with("codex_hook =")
        })
        .cloned()
        .collect::<Vec<_>>();
    lines.splice(
        (start + 1)..end,
        std::iter::once("hooks = true".to_owned()).chain(retained),
    );
    format!("{}\n", lines.join("\n").trim_end())
}

fn install_opencode_hook(
    repository_path: &Path,
    task_id: &str,
    hook_server_url: &str,
    written_files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let path = repository_path.join(".opencode/plugins/kanvibe-plugin.ts");
    let url = serde_json::to_string(hook_server_url).map_err(std::io::Error::other)?;
    let task = serde_json::to_string(task_id).map_err(std::io::Error::other)?;
    let content = format!(
        r#"import type {{ Plugin }} from "@opencode-ai/plugin";
import {{ mkdirSync, readFileSync, writeFileSync }} from "fs";
import {{ dirname, resolve }} from "path";
import {{ fileURLToPath }} from "url";

export const KanvibePlugin: Plugin = async () => {{
  const KANVIBE_URL = {url};
  const TASK_ID = {task};
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
  const stateDir = resolve(root, ".kanvibe");
  const update = async (status: string) => {{
    try {{
      mkdirSync(stateDir, {{ recursive: true }});
      writeFileSync(resolve(stateDir, "status.json"), JSON.stringify({{ schemaVersion: 1, status, updatedAt: new Date().toISOString() }}, null, 2) + "\n");
    }} catch {{}}
    let targets = [{{ url: KANVIBE_URL, taskId: TASK_ID }}];
    try {{
      const parsed = JSON.parse(readFileSync(resolve(stateDir, "targets.json"), "utf8"));
      if (Array.isArray(parsed.targets) && parsed.targets.length) targets = parsed.targets;
    }} catch {{}}
    for (const target of targets) {{
      try {{
        await fetch(String(target.url).replace(/\/+$/, "") + "/api/hooks/status", {{
          method: "POST",
          headers: {{ "Content-Type": "application/json" }},
          body: JSON.stringify({{ taskId: target.taskId, status }}),
        }});
      }} catch {{}}
    }}
  }};
  return {{ event: async ({{ event }}) => {{
    if (event.type === "message.updated" || event.type === "question.replied") await update("progress");
    if (event.type === "question.asked") await update("pending");
    if (event.type === "session.idle") await update("review");
    if (event.type === "session.deleted") await update("done");
  }} }};
}};
"#
    );
    atomic_write(&path, &content, false)?;
    written_files.push(path);
    Ok(())
}

fn update_ai_hook_git_exclude(repository_path: &Path) -> std::io::Result<()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_path)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            "failed to resolve Git common directory",
        ));
    }
    let common_dir = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let exclude_path = common_dir.join("info/exclude");
    let current = fs::read_to_string(&exclude_path).unwrap_or_default();
    let mut preserved = Vec::new();
    let mut skipping = false;
    for line in current.lines() {
        if line == HOOK_EXCLUDE_MARKER {
            skipping = true;
            continue;
        }
        if HOOK_EXCLUDE_PATTERNS.contains(&line) {
            continue;
        }
        if skipping && (HOOK_EXCLUDE_PATTERNS.contains(&line) || line.is_empty()) {
            continue;
        }
        skipping = false;
        preserved.push(line);
    }
    let preserved = preserved.join("\n").trim_end().to_owned();
    let block = std::iter::once(HOOK_EXCLUDE_MARKER)
        .chain(HOOK_EXCLUDE_PATTERNS.iter().copied())
        .collect::<Vec<_>>()
        .join("\n");
    let content = if preserved.is_empty() {
        format!("{block}\n")
    } else {
        format!("{preserved}\n\n{block}\n")
    };
    atomic_write(&exclude_path, &content, false)
}

fn atomic_write(path: &Path, content: &str, executable: bool) -> std::io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "hook file has no parent",
        ));
    };
    fs::create_dir_all(parent)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("kanvibe-hook"),
        std::process::id(),
        nonce
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        #[cfg(unix)]
        if executable {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))?;
        }
        #[cfg(not(unix))]
        let _ = executable;
        fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ParsedUrl<'a> {
    protocol: &'a str,
    host: &'a str,
    port: &'a str,
}

impl<'a> ParsedUrl<'a> {
    fn parse(url: &'a str) -> Option<Self> {
        let (protocol, rest) = url.split_once("://")?;
        let authority = rest.split('/').next().unwrap_or(rest);
        let (host, port) = authority.rsplit_once(':')?;
        Some(Self {
            protocol,
            host,
            port,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BoardEvent {
    BoardUpdated,
    TaskStatusChanged {
        task_id: String,
        task_title: String,
        new_status: String,
    },
    TaskHookInstallFailed {
        task_id: String,
        task_title: String,
        error: String,
    },
    BackgroundSyncReviewNeeded {
        merged_pull_request_count: usize,
        registered_worktree_count: usize,
        failure_count: usize,
    },
}

pub const APP_NOTIFICATIONS_KEY: &str = "app_notifications";
pub const MAX_APP_NOTIFICATIONS: usize = 100;
const NOTIFICATION_DEDUPE_WINDOW: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppNotification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub task_id: Option<String>,
    pub relative_path: String,
    pub locale: String,
    pub is_read: bool,
    pub created_at: String,
    pub dedupe_key: String,
    pub action: Option<Value>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NotificationDraft {
    pub title: String,
    pub body: String,
    pub task_id: Option<String>,
    pub relative_path: Option<String>,
    pub locale: String,
    pub dedupe_key: String,
    pub action: Option<Value>,
}

impl NotificationDraft {
    fn resolved_relative_path(&self) -> String {
        if let Some(task_id) = &self.task_id {
            return format!("/{}/task/{task_id}", self.locale);
        }

        self.relative_path
            .clone()
            .unwrap_or_else(|| format!("/{}", self.locale))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NotificationCreation {
    pub created: bool,
    pub notification: AppNotification,
}

#[derive(Debug)]
pub enum NotificationStoreError {
    Database(rusqlite::Error),
    Serialization(serde_json::Error),
    LockPoisoned,
}

impl std::fmt::Display for NotificationStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "notification database error: {error}"),
            Self::Serialization(error) => {
                write!(formatter, "notification serialization error: {error}")
            }
            Self::LockPoisoned => formatter.write_str("notification dedupe lock was poisoned"),
        }
    }
}

impl std::error::Error for NotificationStoreError {}

impl From<rusqlite::Error> for NotificationStoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for NotificationStoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

#[derive(Debug)]
pub struct NotificationStore {
    database_path: PathBuf,
    started_at: Instant,
    operation: Mutex<()>,
    recent_keys: Mutex<BTreeMap<String, Duration>>,
}

impl NotificationStore {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
            started_at: Instant::now(),
            operation: Mutex::new(()),
            recent_keys: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn create(
        &self,
        draft: NotificationDraft,
    ) -> Result<NotificationCreation, NotificationStoreError> {
        self.create_with_metadata(
            draft,
            Uuid::new_v4().to_string(),
            Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            self.started_at.elapsed(),
        )
    }

    fn create_with_metadata(
        &self,
        draft: NotificationDraft,
        id: String,
        created_at: String,
        now: Duration,
    ) -> Result<NotificationCreation, NotificationStoreError> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| NotificationStoreError::LockPoisoned)?;
        let mut recent_keys = self
            .recent_keys
            .lock()
            .map_err(|_| NotificationStoreError::LockPoisoned)?;
        recent_keys
            .retain(|_, timestamp| now.saturating_sub(*timestamp) <= NOTIFICATION_DEDUPE_WINDOW);

        let notifications = self.read()?;
        if recent_keys.contains_key(&draft.dedupe_key)
            && let Some(notification) = notifications
                .iter()
                .find(|notification| notification.dedupe_key == draft.dedupe_key)
        {
            return Ok(NotificationCreation {
                created: false,
                notification: notification.clone(),
            });
        }

        recent_keys.insert(draft.dedupe_key.clone(), now);
        let relative_path = draft.resolved_relative_path();
        let notification = AppNotification {
            id,
            title: draft.title,
            body: draft.body,
            task_id: draft.task_id,
            relative_path,
            locale: draft.locale,
            is_read: false,
            created_at,
            dedupe_key: draft.dedupe_key,
            action: draft.action,
        };
        let mut next_notifications = Vec::with_capacity(MAX_APP_NOTIFICATIONS);
        next_notifications.push(notification.clone());
        next_notifications.extend(
            notifications
                .into_iter()
                .take(MAX_APP_NOTIFICATIONS.saturating_sub(1)),
        );
        self.write(&next_notifications)?;

        Ok(NotificationCreation {
            created: true,
            notification,
        })
    }

    pub fn list(&self) -> Result<Vec<AppNotification>, NotificationStoreError> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| NotificationStoreError::LockPoisoned)?;
        self.read()
    }

    fn read(&self) -> Result<Vec<AppNotification>, NotificationStoreError> {
        let database = KanvibeDb::open_read_only(&self.database_path)?;
        let Some(value) = database.get_app_setting(APP_NOTIFICATIONS_KEY)? else {
            return Ok(Vec::new());
        };
        let mut notifications =
            serde_json::from_str::<Vec<AppNotification>>(&value).unwrap_or_default();
        notifications.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(notifications)
    }

    pub fn get(&self, id: &str) -> Result<Option<AppNotification>, NotificationStoreError> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| NotificationStoreError::LockPoisoned)?;
        Ok(self
            .read()?
            .into_iter()
            .find(|notification| notification.id == id))
    }

    pub fn mark_read(&self, id: &str) -> Result<bool, NotificationStoreError> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| NotificationStoreError::LockPoisoned)?;
        let mut notifications = self.read()?;
        let mut changed = false;
        for notification in &mut notifications {
            if notification.id == id && !notification.is_read {
                notification.is_read = true;
                changed = true;
            }
        }
        if changed {
            self.write(&notifications)?;
        }
        Ok(changed)
    }

    pub fn mark_all_read(&self) -> Result<usize, NotificationStoreError> {
        let _operation = self
            .operation
            .lock()
            .map_err(|_| NotificationStoreError::LockPoisoned)?;
        let mut notifications = self.read()?;
        let mut changed = 0;
        for notification in &mut notifications {
            if !notification.is_read {
                notification.is_read = true;
                changed += 1;
            }
        }
        if changed > 0 {
            self.write(&notifications)?;
        }
        Ok(changed)
    }

    fn write(&self, notifications: &[AppNotification]) -> Result<(), NotificationStoreError> {
        let database = KanvibeDb::open_read_write(&self.database_path)?;
        let value = serde_json::to_string(notifications)?;
        database.set_app_setting(APP_NOTIFICATIONS_KEY, &value)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct NotificationCenter {
    notifications: Vec<AppNotification>,
}

impl NotificationCenter {
    pub fn push_event(&mut self, event: BoardEvent) -> Option<&AppNotification> {
        let notification = notification_from_board_event(event)?;
        self.notifications.push(notification);
        self.notifications.last()
    }

    pub fn unread_count(&self) -> usize {
        self.notifications
            .iter()
            .filter(|notification| !notification.is_read)
            .count()
    }

    pub fn list(&self) -> &[AppNotification] {
        &self.notifications
    }

    pub fn mark_all_read(&mut self) {
        for notification in &mut self.notifications {
            notification.is_read = true;
        }
    }
}

pub fn notification_from_board_event(event: BoardEvent) -> Option<AppNotification> {
    match event {
        BoardEvent::BoardUpdated => None,
        BoardEvent::TaskStatusChanged {
            task_id,
            task_title,
            new_status,
        } => Some(AppNotification {
            id: format!("task-status:{task_id}:{new_status}"),
            title: "Task status changed".to_owned(),
            body: format!("{task_title} moved to {new_status}"),
            task_id: Some(task_id.clone()),
            relative_path: format!("/en/task/{task_id}"),
            locale: "en".to_owned(),
            is_read: false,
            created_at: String::new(),
            dedupe_key: format!("task-status:{task_id}:{new_status}"),
            action: None,
        }),
        BoardEvent::TaskHookInstallFailed {
            task_id,
            task_title,
            error,
        } => Some(AppNotification {
            id: format!("hook-install-failed:{task_id}"),
            title: "Hook install failed".to_owned(),
            body: format!("{task_title}: {error}"),
            task_id: Some(task_id.clone()),
            relative_path: format!("/en/task/{task_id}"),
            locale: "en".to_owned(),
            is_read: false,
            created_at: String::new(),
            dedupe_key: format!("hook-install-failed:{task_id}"),
            action: None,
        }),
        BoardEvent::BackgroundSyncReviewNeeded {
            merged_pull_request_count,
            registered_worktree_count,
            failure_count,
        } => Some(AppNotification {
            id: "background-sync-review-needed".to_owned(),
            title: "Background sync needs review".to_owned(),
            body: format!(
                "{merged_pull_request_count} merged PRs, {registered_worktree_count} worktrees, {failure_count} failures"
            ),
            task_id: None,
            relative_path: "/en".to_owned(),
            locale: "en".to_owned(),
            is_read: false,
            created_at: String::new(),
            dedupe_key: "background-sync-review-needed".to_owned(),
            action: Some(json!({
                "type": "background-sync-review",
            })),
        }),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookProviderStatus {
    pub provider: AiSessionProvider,
    pub installed: bool,
    pub has_expected_hook_server_url: bool,
    pub has_reachable_hook_server: bool,
}

const CLAUDE_HOOK_FILES: &[&str] = &[
    ".claude/hooks/kanvibe-prompt-hook.sh",
    ".claude/hooks/kanvibe-question-hook.sh",
    ".claude/hooks/kanvibe-stop-hook.sh",
    ".claude/settings.json",
];
const GEMINI_HOOK_FILES: &[&str] = &[
    ".gemini/hooks/kanvibe-prompt-hook.sh",
    ".gemini/hooks/kanvibe-stop-hook.sh",
    ".gemini/settings.json",
];
const CODEX_HOOK_FILES: &[&str] = &[
    ".codex/hooks/kanvibe-prompt-hook.sh",
    ".codex/hooks/kanvibe-permission-hook.sh",
    ".codex/hooks/kanvibe-pre-tool-hook.sh",
    ".codex/hooks/kanvibe-stop-hook.sh",
    ".codex/hooks.json",
    ".codex/config.toml",
];
const OPENCODE_HOOK_FILES: &[&str] = &[".opencode/plugins/kanvibe-plugin.ts"];

pub const fn provider_hook_required_paths(provider: AiSessionProvider) -> &'static [&'static str] {
    match provider {
        AiSessionProvider::Claude => CLAUDE_HOOK_FILES,
        AiSessionProvider::Codex => CODEX_HOOK_FILES,
        AiSessionProvider::Gemini => GEMINI_HOOK_FILES,
        AiSessionProvider::OpenCode => OPENCODE_HOOK_FILES,
    }
}

pub fn inspect_provider_hook_status(
    provider: AiSessionProvider,
    files: &BTreeMap<String, String>,
    expected_url: Option<&str>,
    is_remote: bool,
    reachable: bool,
) -> HookProviderStatus {
    let required_paths = provider_hook_required_paths(provider);
    let installed = required_paths.iter().all(|path| files.contains_key(*path));
    let configured_urls = required_paths
        .iter()
        .filter_map(|path| {
            let content = files.get(*path)?;
            if path.ends_with(".sh") {
                Some(extract_shell_hook_server_url(content))
            } else if path.ends_with("kanvibe-plugin.ts") {
                Some(extract_plugin_hook_server_url(content))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let validation = validate_hook_server_configuration(
        &configured_urls,
        expected_url,
        true,
        is_remote,
        reachable,
    );
    HookProviderStatus {
        provider,
        installed,
        has_expected_hook_server_url: validation.has_expected_hook_server_url,
        has_reachable_hook_server: validation.has_reachable_hook_server,
    }
}

pub fn hook_status_visible(statuses: &[HookProviderStatus]) -> bool {
    AI_PROVIDERS
        .iter()
        .all(|provider| statuses.iter().any(|status| status.provider == *provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send_request(address: SocketAddr, request: &str) -> String {
        let mut stream = TcpStream::connect(address).expect("connect to hook server");
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("set read timeout");
        stream.write_all(request.as_bytes()).expect("write request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        response
    }

    #[test]
    fn hook_http_server_routes_health_and_status_requests() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let handler_received = Arc::clone(&received);
        let server = HookHttpServer::start(
            "127.0.0.1",
            0,
            Arc::new(move |request| {
                handler_received
                    .lock()
                    .expect("capture request")
                    .push(request);
                HookHttpResponse::json(200, r#"{"success":true,"data":{"status":"review"}}"#)
            }),
        )
        .expect("start server");

        let health = send_request(
            server.local_addr(),
            "GET /api/hooks/health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(health.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(health.ends_with(r#"{"success":true}"#));

        let body = r#"{"taskId":"task-1","status":"review"}"#;
        let status = send_request(
            server.local_addr(),
            &format!(
                "POST /api/hooks/status?source=test HTTP/1.1\r\nHost: localhost\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert!(status.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(status.ends_with(r#"{"success":true,"data":{"status":"review"}}"#));
        assert_eq!(
            *received.lock().expect("read captured request"),
            vec![HookHttpRequest {
                route: HookHttpRoute::Status,
                body: body.to_owned(),
            }]
        );
    }

    #[test]
    fn hook_http_server_rejects_oversized_payload_before_handler() {
        let invocation_count = Arc::new(Mutex::new(0_u8));
        let handler_invocation_count = Arc::clone(&invocation_count);
        let server = HookHttpServer::start(
            "127.0.0.1",
            0,
            Arc::new(move |_| {
                *handler_invocation_count.lock().expect("count invocation") += 1;
                HookHttpResponse::json(200, "{}")
            }),
        )
        .expect("start server");

        let response = send_request(
            server.local_addr(),
            &format!(
                "POST /api/hooks/start HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
                MAX_HOOK_BODY_BYTES + 1
            ),
        );
        assert!(response.starts_with("HTTP/1.1 413 Payload Too Large\r\n"));
        assert_eq!(*invocation_count.lock().expect("read invocation count"), 0);
    }

    #[test]
    fn local_provider_install_is_idempotent_preserves_settings_and_posts_status() {
        let root = std::env::temp_dir().join(format!(
            "kanvibe-provider-hooks-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(root.join(".claude")).expect("create hook fixture");
        fs::write(
            root.join(".claude/settings.json"),
            r#"{"custom":true,"hooks":{"Stop":[{"hooks":[{"type":"command","command":"custom-stop","timeout":3}]}]}}"#,
        )
        .expect("write existing Claude settings");
        fs::create_dir_all(root.join(".codex")).expect("create Codex fixture");
        fs::write(
            root.join(".codex/config.toml"),
            "[features]\nother = true\nhooks = false\n\n[profile]\nname = \"dev\"\n",
        )
        .expect("write existing Codex config");
        let git = Command::new("git")
            .arg("-C")
            .arg(&root)
            .arg("init")
            .output()
            .expect("initialize fixture repository");
        assert!(git.status.success());

        let received = Arc::new(Mutex::new(Vec::new()));
        let handler_received = Arc::clone(&received);
        let server = HookHttpServer::start(
            "127.0.0.1",
            0,
            Arc::new(move |request| {
                handler_received
                    .lock()
                    .expect("capture provider request")
                    .push(request);
                HookHttpResponse::json(200, r#"{"success":true}"#)
            }),
        )
        .expect("start provider hook server");
        let url = format!("http://{}", server.local_addr());

        let first =
            install_local_provider_hooks(&root, "task-1", &url).expect("install provider hooks");
        assert_eq!(first.written_files.len(), 15);
        install_local_provider_hooks(&root, "task-1", &url)
            .expect("repeat same provider installation");
        install_local_provider_hooks(&root, "task-2", &url)
            .expect("rebind provider hooks and retain target fan-out");

        let claude_settings: Value = serde_json::from_str(
            &fs::read_to_string(root.join(".claude/settings.json")).expect("read Claude settings"),
        )
        .expect("parse Claude settings");
        assert_eq!(claude_settings["custom"], true);
        let stop_entries = claude_settings["hooks"]["Stop"]
            .as_array()
            .expect("Claude Stop entries");
        assert_eq!(stop_entries.len(), 2);
        assert!(
            stop_entries
                .iter()
                .any(|entry| entry.to_string().contains("custom-stop"))
        );

        let targets: Value = serde_json::from_str(
            &fs::read_to_string(root.join(".kanvibe/targets.json")).expect("read targets state"),
        )
        .expect("parse targets state");
        assert_eq!(targets["targets"].as_array().expect("target list").len(), 2);
        let config =
            fs::read_to_string(root.join(".codex/config.toml")).expect("read Codex config");
        assert!(config.contains("other = true"));
        assert!(config.contains("hooks = true"));
        assert!(config.contains("[profile]"));
        let exclude =
            fs::read_to_string(root.join(".git/info/exclude")).expect("read common Git exclude");
        assert_eq!(exclude.matches(HOOK_EXCLUDE_MARKER).count(), 1);
        assert_eq!(exclude.matches(".opencode/plugins/").count(), 1);

        let execution = Command::new("bash")
            .arg(root.join(".claude/hooks/kanvibe-stop-hook.sh"))
            .current_dir(&root)
            .output()
            .expect("execute installed provider hook");
        assert!(execution.status.success());
        let request = received
            .lock()
            .expect("read provider request")
            .last()
            .cloned()
            .expect("provider hook posted status");
        assert_eq!(request.route, HookHttpRoute::Status);
        assert!(request.body.contains(r#""taskId": "task-2""#));
        assert!(request.body.contains(r#""status": "review""#));
        let state = fs::read_to_string(root.join(".kanvibe/status.json"))
            .expect("read persisted hook state");
        assert!(state.contains(r#""status":"review""#));

        drop(server);
        fs::remove_dir_all(root).expect("remove provider hook fixture");
    }

    #[test]
    fn provider_hook_renderer_preserves_remote_settings_and_target_fan_out() {
        let existing = BTreeMap::from([
            (
                ".claude/settings.json".to_owned(),
                r#"{"custom":true,"hooks":{"Stop":[{"hooks":[{"type":"command","command":"custom-stop"}]}]}}"#
                    .to_owned(),
            ),
            (
                ".codex/config.toml".to_owned(),
                "[features]\nother = true\n\n[profile]\nname = \"qa\"\n".to_owned(),
            ),
            (
                ".kanvibe/targets.json".to_owned(),
                r#"{"schemaVersion":1,"targets":[{"url":"http://old:9736","taskId":"old-task"}]}"#
                    .to_owned(),
            ),
        ]);

        let rendered = render_provider_hooks(&existing, "remote-task", "http://192.0.2.10:9736")
            .expect("render remote provider hooks");

        assert_eq!(rendered.len(), 15);
        let claude = rendered
            .iter()
            .find(|file| file.relative_path == ".claude/settings.json")
            .expect("rendered Claude settings");
        assert!(claude.content.contains(r#""custom": true"#));
        assert!(claude.content.contains("custom-stop"));
        let targets = rendered
            .iter()
            .find(|file| file.relative_path == ".kanvibe/targets.json")
            .expect("rendered target fan-out");
        assert!(targets.content.contains("old-task"));
        assert!(targets.content.contains("remote-task"));
        let codex = rendered
            .iter()
            .find(|file| file.relative_path == ".codex/config.toml")
            .expect("rendered Codex config");
        assert!(codex.content.contains("other = true"));
        assert!(codex.content.contains("[profile]"));
        assert!(
            rendered
                .iter()
                .filter(|file| file.relative_path.ends_with(".sh"))
                .all(|file| file.executable)
        );
    }

    #[test]
    fn hook_env_contract_is_kanvibe_scoped() {
        assert!(is_kanvibe_scoped_env(QA_SOCKET_ENV));
        assert!(is_kanvibe_scoped_env(HOOK_SERVER_PORT_ENV));
        assert!(!is_kanvibe_scoped_env("PORT"));
    }

    #[test]
    fn hook_server_url_validation_matches_local_and_remote_rules() {
        assert_eq!(
            remote_hook_server_url("2001:db8::10", 9736),
            "http://[2001:db8::10]:9736"
        );
        let local = validate_hook_server_configuration(
            &[Some("http://127.0.0.1:9736".to_owned())],
            Some("http://localhost:9736"),
            true,
            false,
            true,
        );
        assert!(local.has_expected_hook_server_url);
        assert!(local.has_reachable_hook_server);

        let remote = validate_hook_server_configuration(
            &[Some("http://100.83.96.29:9736".to_owned())],
            Some("http://10.0.0.42:9736"),
            true,
            true,
            true,
        );
        assert!(remote.has_expected_hook_server_url);
    }

    #[test]
    fn hook_url_extractors_read_shell_and_plugin_content() {
        assert_eq!(
            extract_shell_hook_server_url("KANVIBE_URL=\"http://localhost:9736\"\n"),
            Some("http://localhost:9736".to_owned())
        );
        assert_eq!(
            extract_plugin_hook_server_url("const KANVIBE_URL = \"http://localhost:9736\";"),
            Some("http://localhost:9736".to_owned())
        );
    }

    #[test]
    fn ai_session_aggregation_sorts_and_reports_provider_sources() {
        let result = aggregate_ai_sessions(vec![
            AiSession {
                id: "old".to_owned(),
                provider: AiSessionProvider::Claude,
                started_at: None,
                updated_at: Some("2026-07-08T00:00:00Z".to_owned()),
                matched_path: "/tmp/old".to_owned(),
                title: None,
                first_user_prompt: None,
                message_count: 2,
                source_ref: "/tmp/old.jsonl".to_owned(),
            },
            AiSession {
                id: "new".to_owned(),
                provider: AiSessionProvider::Codex,
                started_at: None,
                updated_at: Some("2026-07-08T01:00:00Z".to_owned()),
                matched_path: "/tmp/new".to_owned(),
                title: None,
                first_user_prompt: None,
                message_count: 4,
                source_ref: "/tmp/new.jsonl".to_owned(),
            },
        ]);

        assert_eq!(result.sessions[0].id, "new");
        assert_eq!(result.sources.len(), 4);
        assert_eq!(result.sources[0].provider, AiSessionProvider::Claude);
        assert_eq!(result.sources[0].session_count, 1);
    }

    #[test]
    fn notification_center_converts_board_events_and_marks_read() {
        let mut center = NotificationCenter::default();
        center.push_event(BoardEvent::TaskHookInstallFailed {
            task_id: "task-1".to_owned(),
            task_title: "Install hooks".to_owned(),
            error: "missing config".to_owned(),
        });

        assert_eq!(center.unread_count(), 1);
        assert_eq!(center.list()[0].title, "Hook install failed");
        center.mark_all_read();
        assert_eq!(center.unread_count(), 0);
    }

    #[test]
    fn persistent_notification_store_matches_electron_contract() {
        let path = writable_notification_seed_copy("notification-contract");
        let store = NotificationStore::new(&path);
        let first = NotificationDraft {
            title: "Review background sync".to_owned(),
            body: "One pull request needs review".to_owned(),
            task_id: Some("task-1".to_owned()),
            relative_path: Some("src/main.rs".to_owned()),
            locale: "en".to_owned(),
            dedupe_key: "background-sync::task-1".to_owned(),
            action: Some(json!({
                "type": "background-sync-review",
                "taskId": "task-1",
            })),
        };

        let created = store
            .create_with_metadata(
                first.clone(),
                "notification-1".to_owned(),
                "2026-07-29T10:00:00Z".to_owned(),
                Duration::from_secs(10),
            )
            .expect("create first notification");
        let duplicate = store
            .create_with_metadata(
                first,
                "notification-duplicate".to_owned(),
                "2026-07-29T10:00:04Z".to_owned(),
                Duration::from_secs(14),
            )
            .expect("dedupe notification");

        assert!(created.created);
        assert!(!duplicate.created);
        assert_eq!(duplicate.notification.id, "notification-1");

        for index in 2..=101 {
            store
                .create_with_metadata(
                    NotificationDraft {
                        title: format!("Notification {index}"),
                        body: format!("Body {index}"),
                        task_id: None,
                        relative_path: None,
                        locale: "ko".to_owned(),
                        dedupe_key: format!("notification::{index}"),
                        action: None,
                    },
                    format!("notification-{index}"),
                    format!("2026-07-29T10:{:02}:{:02}Z", index / 60, index % 60),
                    Duration::from_secs(20 + index),
                )
                .expect("create capped notification");
        }

        let notifications = store.list().expect("list notifications");
        assert_eq!(notifications.len(), MAX_APP_NOTIFICATIONS);
        assert_eq!(notifications[0].id, "notification-101");
        assert_eq!(notifications[99].id, "notification-2");
        assert!(!notifications[0].is_read);

        let database = kanvibe_core::KanvibeDb::open_read_only(&path).expect("open seed copy");
        let persisted = database
            .get_app_setting(APP_NOTIFICATIONS_KEY)
            .expect("read setting")
            .expect("notification setting");
        assert!(persisted.contains("\"taskId\""));
        assert!(persisted.contains("\"relativePath\""));
        assert!(persisted.contains("\"isRead\""));
        assert!(persisted.contains("\"createdAt\""));
        assert!(persisted.contains("\"dedupeKey\""));
        assert!(!persisted.contains("\"task_id\""));
    }

    #[test]
    fn persistent_notification_store_marks_read_and_recovers_from_malformed_json() {
        let path = writable_notification_seed_copy("notification-read");
        let store = NotificationStore::new(&path);
        store
            .create_with_metadata(
                NotificationDraft {
                    title: "First".to_owned(),
                    body: "First body".to_owned(),
                    task_id: None,
                    relative_path: None,
                    locale: "en".to_owned(),
                    dedupe_key: "first".to_owned(),
                    action: None,
                },
                "notification-1".to_owned(),
                "2026-07-29T10:00:00Z".to_owned(),
                Duration::from_secs(1),
            )
            .expect("create first");
        store
            .create_with_metadata(
                NotificationDraft {
                    title: "Second".to_owned(),
                    body: "Second body".to_owned(),
                    task_id: None,
                    relative_path: None,
                    locale: "en".to_owned(),
                    dedupe_key: "second".to_owned(),
                    action: None,
                },
                "notification-2".to_owned(),
                "2026-07-29T10:01:00Z".to_owned(),
                Duration::from_secs(10),
            )
            .expect("create second");

        assert!(store.mark_read("notification-1").expect("mark one read"));
        assert!(!store.mark_read("missing").expect("ignore missing"));
        let notifications = store.list().expect("list after mark");
        assert!(notifications[1].is_read);
        assert!(!notifications[0].is_read);

        assert_eq!(store.mark_all_read().expect("mark all read"), 1);
        assert!(
            store
                .list()
                .expect("list all read")
                .iter()
                .all(|notification| notification.is_read)
        );

        let database = kanvibe_core::KanvibeDb::open_read_write(&path).expect("open seed copy");
        database
            .set_app_setting(APP_NOTIFICATIONS_KEY, "{not-json")
            .expect("write malformed setting");
        assert!(store.list().expect("malformed JSON falls back").is_empty());
    }

    fn writable_notification_seed_copy(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kanvibe-hooks-{name}-{}-{unique}.sqlite",
            std::process::id()
        ));
        let seed =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../qa/seed/kanvibe-seed.sqlite");
        fs::copy(seed, &path).expect("copy seed database");
        path
    }

    #[test]
    fn hook_status_requires_all_ai_providers() {
        let statuses = AI_PROVIDERS
            .iter()
            .map(|provider| HookProviderStatus {
                provider: *provider,
                installed: true,
                has_expected_hook_server_url: true,
                has_reachable_hook_server: true,
            })
            .collect::<Vec<_>>();

        assert!(hook_status_visible(&statuses));
    }

    #[test]
    fn provider_hook_inspection_requires_every_file_and_expected_callback() {
        let mut files = BTreeMap::new();
        for path in provider_hook_required_paths(AiSessionProvider::Claude) {
            files.insert(
                (*path).to_owned(),
                if path.ends_with(".sh") {
                    "KANVIBE_URL=\"http://localhost:9736\"\n".to_owned()
                } else {
                    "{}\n".to_owned()
                },
            );
        }

        let status = inspect_provider_hook_status(
            AiSessionProvider::Claude,
            &files,
            Some("http://127.0.0.1:9736"),
            false,
            true,
        );
        assert!(status.installed);
        assert!(status.has_expected_hook_server_url);
        assert!(status.has_reachable_hook_server);

        files.remove(".claude/hooks/kanvibe-stop-hook.sh");
        assert!(
            !inspect_provider_hook_status(
                AiSessionProvider::Claude,
                &files,
                Some("http://localhost:9736"),
                false,
                true,
            )
            .installed
        );
    }
}
