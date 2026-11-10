use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::{fs::FileTypeExt, net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_SOCKET_RESPONSE_BYTES: usize = 1024 * 1024;
const AGENT_PROVIDERS: [&str; 4] = ["claude", "codex", "opencode", "gemini"];
const EXECUTABLE: &str = r#""$(command -v herdr || printf '%s' "$HOME/.local/bin/herdr")""#;

/// 테스트가 실제 셸 대신 실행된 명령 문자열을 기록할 수 있도록 실행기를 주입받는다.
pub type Shell<'a> = &'a (dyn Fn(&str, Option<&str>, Duration) -> Result<String, String> + Sync);

pub fn system_shell(
    command: &str,
    host: Option<&str>,
    timeout: Duration,
) -> Result<String, String> {
    crate::runtime::run_shell_timeout(command, host, timeout)
}

/// KanVibe 안에서 열린 Herdr pane이 바깥 세션 변수를 물려받아 엉뚱한 서버를 가리키지 않도록 지운다.
pub fn command(session: &str, args: &[&str]) -> String {
    let q = crate::runtime::quote;
    let arguments = args.iter().map(|arg| q(arg)).collect::<Vec<_>>().join(" ");
    format!(
        "env -u HERDR_ENV -u HERDR_SESSION -u HERDR_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID {EXECUTABLE} --session {} {arguments}",
        q(session)
    )
}

pub fn cleanup_command(session: &str) -> String {
    format!(
        "{} >/dev/null 2>&1 || true; {}",
        command(session, &["session", "stop", session]),
        command(session, &["session", "delete", session])
    )
}

/// Herdr 세션 이름 규칙을 벗어나는 이름은 해시 접미사로 구분되는 결정적 ASCII 이름으로 바꾼다.
pub fn sanitize_session_name(name: &str) -> String {
    let is_allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    if (1..=40).contains(&name.len()) && name.chars().all(is_allowed) && name != "." && name != ".."
    {
        return name.to_owned();
    }
    let digest = hex::encode(Sha256::digest(name.as_bytes()));
    let prefix: String = name
        .chars()
        .map(|c| if is_allowed(c) { c } else { '-' })
        .take(23)
        .collect();
    format!("{prefix}-{}", &digest[..16])
}

fn read_result(output: &str) -> Result<Value, String> {
    let response: Value = serde_json::from_str(output.trim())
        .map_err(|e| format!("Herdr 응답을 해석하지 못했습니다: {e}"))?;
    if let Some(error) = response.get("error").filter(|e| !e.is_null()) {
        let message = error["message"]
            .as_str()
            .or_else(|| error["code"].as_str())
            .unwrap_or("알 수 없는 오류");
        return Err(format!("Herdr: {message}"));
    }
    match response.get("result") {
        Some(result) if !result.is_null() => Ok(result.clone()),
        _ => Err("Herdr 응답에 result가 없습니다.".into()),
    }
}

pub fn run(
    session: &str,
    args: &[&str],
    host: Option<&str>,
    shell: Shell,
) -> Result<Value, String> {
    let output = shell(&command(session, args), host, COMMAND_TIMEOUT)?;
    // send-text는 전송에 성공해도 본문을 출력하지 않는다.
    if output.trim().is_empty() && args.starts_with(&["pane", "send-text"]) {
        return Ok(Value::Null);
    }
    read_result(&output)
}

pub fn read_tabs(session: &str, host: Option<&str>, shell: Shell) -> Result<Vec<Value>, String> {
    let result = run(session, &["tab", "list"], host, shell)?;
    let tabs = result["tabs"]
        .as_array()
        .ok_or("Herdr 탭 목록이 올바르지 않습니다.")?;
    Ok(tabs
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            json!({"id":tab["tab_id"],"nativeIndex":index,"name":tab["label"],"isActive":tab["focused"].as_bool().unwrap_or(false)})
        })
        .collect())
}

pub fn mirror_panes(session: &str, host: Option<&str>, shell: Shell) -> Result<Vec<Value>, String> {
    let result = run(session, &["pane", "list"], host, shell)?;
    let panes = result["panes"]
        .as_array()
        .ok_or("Herdr pane 목록이 올바르지 않습니다.")?;
    let mut mirrors = Vec::new();
    for tab in read_tabs(session, host, shell)? {
        let tab_panes: Vec<_> = panes
            .iter()
            .filter(|pane| {
                pane["tab_id"] == tab["id"]
                    && !pane["restore_error"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
            })
            .collect();
        let Some(first) = tab_panes.first() else {
            continue;
        };
        let pane_id = first["pane_id"]
            .as_str()
            .ok_or("Herdr pane ID가 없습니다.")?;
        let result = run(session, &["pane", "layout", "--pane", pane_id], host, shell)?;
        let layout = &result["layout"];
        for pane in tab_panes {
            let pane_id = pane["pane_id"]
                .as_str()
                .ok_or("Herdr pane ID가 없습니다.")?;
            let rect = layout["panes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|entry| entry["pane_id"] == pane_id)
                .map(|entry| &entry["rect"]);
            let zoomed = layout["zoomed"] == true && layout["focused_pane_id"] == pane_id;
            // process-info는 표시용이라 조회 실패가 살아 있는 pane을 숨기지 않게 한다.
            let process = run(
                session,
                &["pane", "process-info", "--pane", pane_id],
                host,
                shell,
            )
            .ok()
            .and_then(|value| {
                value["process_info"]["foreground_processes"]
                    .as_array()
                    .and_then(|items| items.last())
                    .and_then(|item| item["name"].as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
            mirrors.push(json!({
                "id":pane_id,"tabId":tab["id"],"tabName":tab["name"],"command":process,
                "left":rect.and_then(|r| r["x"].as_u64()).unwrap_or(0),
                "top":rect.and_then(|r| r["y"].as_u64()).unwrap_or(0),
                "width":if zoomed {layout["area"]["width"].clone()} else {rect.map(|r| r["width"].clone()).unwrap_or_else(||layout["area"]["width"].clone())},
                "height":pane["scroll"]["viewport_rows"]
            }));
        }
    }
    Ok(mirrors)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaneExtent {
    pub history_rows: u64,
    pub viewport_rows: u64,
}
pub fn pane_extent(
    session: &str,
    pane: &str,
    host: Option<&str>,
    shell: Shell,
) -> Result<PaneExtent, String> {
    let result = run(session, &["pane", "get", pane], host, shell)?;
    Ok(PaneExtent {
        history_rows: result["pane"]["scroll"]["max_offset_from_bottom"]
            .as_u64()
            .ok_or("Herdr 스크롤 이력이 없습니다.")?,
        viewport_rows: result["pane"]["scroll"]["viewport_rows"]
            .as_u64()
            .ok_or("Herdr 화면 크기가 없습니다.")?,
    })
}

pub fn recent_screen_command(session: &str, pane: &str) -> String {
    command(
        session,
        &[
            "pane", "read", pane, "--source", "recent", "--lines", "2000", "--format", "ansi",
        ],
    )
}
pub fn observe_command(session: &str, pane: &str, cols: u64, rows: u64) -> String {
    command(
        session,
        &[
            "terminal",
            "session",
            "observe",
            pane,
            "--cols",
            &cols.to_string(),
            "--rows",
            &rows.to_string(),
        ],
    )
}
#[derive(Debug, PartialEq)]
pub enum ObserveRecord {
    Frame { full: bool, output: Vec<u8> },
    Closed,
}
pub fn parse_observe_record(line: &str) -> Option<ObserveRecord> {
    use base64::Engine;
    let record: Value = serde_json::from_str(line).ok()?;
    match record["type"].as_str()? {
        "terminal.closed" => Some(ObserveRecord::Closed),
        "terminal.frame" => Some(ObserveRecord::Frame {
            full: record["full"] == true,
            output: base64::engine::general_purpose::STANDARD
                .decode(record["bytes"].as_str()?)
                .ok()?,
        }),
        _ => None,
    }
}
pub fn write_pane_command(session: &str, pane: &str, input: &str) -> String {
    let mut commands = Vec::new();
    for (index, text) in input.split('\0').enumerate() {
        if index > 0 {
            commands.push(command(session, &["pane", "send-keys", pane, "ctrl+space"]));
        }
        if !text.is_empty() {
            commands.push(command(session, &["pane", "send-text", pane, text]));
        }
    }
    commands.join(" && ")
}
pub fn pane_shortcut_command(session: &str, pane: &str, action: &str) -> Option<String> {
    let args = match action {
        "split-right" => vec!["pane", "split", pane, "--direction", "right", "--no-focus"],
        "split-down" => vec!["pane", "split", pane, "--direction", "down", "--no-focus"],
        "toggle-fullscreen" => vec!["pane", "zoom", pane, "--toggle"],
        _ => return None,
    };
    Some(command(session, &args))
}

fn is_transport_error(error: &str, host: Option<&str>) -> bool {
    host.is_some() && crate::dependencies::is_ssh_transport_error(error)
}

struct Bootstrap {
    result: Mutex<Option<Result<(), String>>>,
    finished: Condvar,
}
static BOOTSTRAPS: OnceLock<Mutex<HashMap<String, Arc<Bootstrap>>>> = OnceLock::new();

/// 같은 세션에 동시에 들어온 연결은 먼저 시작한 준비 작업의 결과를 함께 기다려, 레이아웃이 두 번 만들어지지 않게 한다.
pub fn ensure_session(
    session: &str,
    cwd: &str,
    layout: &Value,
    host: Option<&str>,
    shell: Shell,
) -> Result<(), String> {
    let key = json!([host, session]).to_string();
    let running = BOOTSTRAPS.get_or_init(Mutex::default);
    let (bootstrap, is_leader) = {
        let mut running = running.lock().unwrap_or_else(PoisonError::into_inner);
        match running.get(&key) {
            Some(current) => (current.clone(), false),
            None => {
                let created = Arc::new(Bootstrap {
                    result: Mutex::new(None),
                    finished: Condvar::new(),
                });
                running.insert(key.clone(), created.clone());
                (created, true)
            }
        }
    };
    if !is_leader {
        let mut result = bootstrap
            .result
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while result.is_none() {
            result = bootstrap
                .finished
                .wait(result)
                .unwrap_or_else(PoisonError::into_inner);
        }
        return result.clone().unwrap_or(Ok(()));
    }
    let result = bootstrap_session(session, cwd, layout, host, shell);
    running
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&key);
    *bootstrap
        .result
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(result.clone());
    bootstrap.finished.notify_all();
    result
}

fn list_workspaces(session: &str, host: Option<&str>, shell: Shell) -> Result<Vec<Value>, String> {
    run(session, &["workspace", "list"], host, shell)?["workspaces"]
        .as_array()
        .cloned()
        .ok_or_else(|| "Herdr 워크스페이스 목록이 올바르지 않습니다.".into())
}

/// 이미 워크스페이스가 있는 세션은 다시 연결만 하고, 레이아웃과 시작 명령은 처음 만들 때 한 번만 적용한다.
fn bootstrap_session(
    session: &str,
    cwd: &str,
    layout: &Value,
    host: Option<&str>,
    shell: Shell,
) -> Result<(), String> {
    let workspaces = match list_workspaces(session, host, shell) {
        Ok(workspaces) => workspaces,
        Err(error) => {
            if is_transport_error(&error, host) {
                return Err(error);
            }
            shell(
                &format!(
                    "nohup {} </dev/null >/dev/null 2>&1 &",
                    command(session, &["server"])
                ),
                host,
                START_TIMEOUT,
            )?;
            let deadline = Instant::now() + START_TIMEOUT;
            loop {
                match list_workspaces(session, host, shell) {
                    Ok(workspaces) => break workspaces,
                    Err(error) => {
                        if is_transport_error(&error, host) || Instant::now() >= deadline {
                            return Err(error);
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }
    };
    if !workspaces.is_empty() {
        return Ok(());
    }
    let created = run(
        session,
        &["workspace", "create", "--cwd", cwd, "--focus"],
        host,
        shell,
    )?;
    let root_pane = created["root_pane"]["pane_id"]
        .as_str()
        .ok_or("Herdr 응답에 root_pane이 없습니다.")?;
    if let Err(error) = apply_layout(session, root_pane, cwd, layout, host, shell) {
        shell(&cleanup_command(session), host, START_TIMEOUT)?;
        return Err(error);
    }
    Ok(())
}

fn apply_layout(
    session: &str,
    root_pane: &str,
    cwd: &str,
    layout: &Value,
    host: Option<&str>,
    shell: Shell,
) -> Result<(), String> {
    if layout.is_null() {
        return Ok(());
    }
    let split = |pane: &str, direction: &str| -> Result<String, String> {
        let result = run(
            session,
            &[
                "pane",
                "split",
                pane,
                "--direction",
                direction,
                "--cwd",
                cwd,
            ],
            host,
            shell,
        )?;
        result["pane"]["pane_id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| "Herdr 응답에 pane이 없습니다.".into())
    };
    let root = root_pane.to_owned();
    let pane_ids = match layout["layoutType"].as_str().unwrap_or("single") {
        "horizontal_2" => vec![root.clone(), split(&root, "down")?],
        "vertical_2" => vec![root.clone(), split(&root, "right")?],
        "left_right_tb" => {
            let right = split(&root, "right")?;
            let right_bottom = split(&right, "down")?;
            vec![root, right, right_bottom]
        }
        "left_tb_right" => {
            let right = split(&root, "right")?;
            let left_bottom = split(&root, "down")?;
            vec![root, left_bottom, right]
        }
        "quad" => {
            let right = split(&root, "right")?;
            let left_bottom = split(&root, "down")?;
            let right_bottom = split(&right, "down")?;
            vec![root, right, left_bottom, right_bottom]
        }
        _ => vec![root],
    };
    for pane in layout["panes"].as_array().into_iter().flatten() {
        let command = pane["command"].as_str().unwrap_or("");
        let target = pane["position"]
            .as_u64()
            .and_then(|position| pane_ids.get(position as usize));
        let Some(target) = target.filter(|_| !command.trim().is_empty()) else {
            continue;
        };
        // 막 뜬 로그인 셸에는 bracketed paste 대신 원문과 Enter를 한 번에 버퍼링한다.
        let text = format!("{command}\r");
        run(session, &["pane", "send-text", target, &text], host, shell)?;
    }
    Ok(())
}

fn list_sessions(host: Option<&str>, shell: Shell) -> Result<Vec<Value>, String> {
    let output = shell(
        &command("default", &["session", "list", "--json"]),
        host,
        COMMAND_TIMEOUT,
    )?;
    serde_json::from_str::<Value>(output.trim())
        .ok()
        .and_then(|response| response["sessions"].as_array().cloned())
        .ok_or_else(|| "Herdr 세션 목록이 올바르지 않습니다.".into())
}

fn request_socket(
    path: &Path,
    method: &str,
    params: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    let deadline = Instant::now() + timeout;
    let mut stream = UnixStream::connect(path).map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let request = json!({"id":"kanvibe","method":method,"params":params});
    stream
        .write_all(format!("{request}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line = String::from_utf8_lossy(&buffer[..end]).into_owned();
            let response: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if response["id"] != "kanvibe" {
                return Err("Herdr 소켓 응답 ID가 일치하지 않습니다.".into());
            }
            return read_result(&line);
        }
        if buffer.len() > MAX_SOCKET_RESPONSE_BYTES {
            return Err("Herdr 응답 크기 초과".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Herdr 소켓 응답 시간 초과".into());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        match stream.read(&mut chunk) {
            Ok(0) => return Err("Herdr 소켓 응답이 끊겼습니다.".into()),
            Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return Err("Herdr 소켓 응답 시간 초과".into())
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn tunnel_args(host: &str, local_socket: &Path, remote_socket: &str) -> Vec<String> {
    let forward = format!("{}:{remote_socket}", local_socket.to_string_lossy());
    [
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-N",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-L",
        &forward,
        "--",
        host,
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn create_private_directory() -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let directory = std::env::temp_dir().join(format!("kvh-{}", &suffix[..12]));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|e| e.to_string())?;
    Ok(directory)
}

/// 원격 Herdr 소켓은 이 요청 하나만을 위한 SSH 포워딩으로 열고, 끝나면 터널과 임시 소켓 경로를 모두 치운다.
fn request_through_tunnel(
    program: &str,
    host: &str,
    remote_socket: &str,
    method: &str,
    params: &Value,
    start_timeout: Duration,
) -> Result<Value, String> {
    let directory = create_private_directory()?;
    let local_socket = directory.join("api");
    let spawned = Command::new(program)
        .args(tunnel_args(host, &local_socket, remote_socket))
        .env_clear()
        .envs(crate::runtime::shell_environment())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut tunnel = match spawned {
        Ok(tunnel) => tunnel,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&directory);
            return Err(error.to_string());
        }
    };
    let result = (|| {
        let deadline = Instant::now() + start_timeout;
        loop {
            let is_ready = std::fs::metadata(&local_socket)
                .map(|m| m.file_type().is_socket())
                .unwrap_or(false);
            if is_ready {
                break;
            }
            if tunnel.try_wait().map_err(|e| e.to_string())?.is_some() {
                let mut stderr = String::new();
                if let Some(mut pipe) = tunnel.stderr.take() {
                    let _ = pipe.read_to_string(&mut stderr);
                }
                let message = stderr.trim();
                return Err(if message.is_empty() {
                    "Herdr SSH 소켓 연결이 종료됐습니다.".to_string()
                } else {
                    message.to_string()
                });
            }
            if Instant::now() >= deadline {
                return Err("Herdr SSH 소켓 연결 시간 초과".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        request_socket(&local_socket, method, params, COMMAND_TIMEOUT)
    })();
    let _ = tunnel.kill();
    let _ = tunnel.wait();
    let _ = std::fs::remove_dir_all(&directory);
    result
}

/// Herdr CLI에는 탭 순서 변경 명령이 없어서 실행 중인 세션의 로컬 API 소켓에 tab.move를 직접 보낸다.
pub fn move_tab(
    session: &str,
    tab_id: &str,
    insert_index: usize,
    host: Option<&str>,
    shell: Shell,
) -> Result<(), String> {
    let sessions = list_sessions(host, shell)?;
    let socket_path = sessions
        .iter()
        .find(|entry| entry["name"] == session && entry["running"] == true)
        .and_then(|entry| entry["socket_path"].as_str())
        .ok_or("Herdr 세션을 찾을 수 없습니다.")?;
    let params = json!({"tab_id":tab_id,"insert_index":insert_index});
    match host {
        None => request_socket(Path::new(socket_path), "tab.move", &params, COMMAND_TIMEOUT),
        Some(host) => {
            request_through_tunnel("ssh", host, socket_path, "tab.move", &params, START_TIMEOUT)
        }
    }
    .map(|_| ())
}

fn workspace_of(tab_id: &str) -> &str {
    tab_id.split(':').next().unwrap_or("")
}

/// 탭 바는 전체 목록의 배열 위치를 넘기지만 Herdr는 워크스페이스 안의 삽입 경계로 옮기므로 여기서 바꿔 준다.
fn move_insert_index(tabs: &[Value], tab_id: &str, target_index: usize) -> Result<usize, String> {
    let current = tabs
        .iter()
        .find(|tab| tab["id"] == tab_id)
        .ok_or("탭을 찾을 수 없습니다.")?;
    let destination = &tabs[target_index.min(tabs.len() - 1)];
    let workspace = workspace_of(current["id"].as_str().unwrap_or(""));
    let destination_id = destination["id"].as_str().unwrap_or("");
    if workspace_of(destination_id) != workspace {
        return Err("Herdr 탭은 같은 워크스페이스 안에서 이동할 수 있습니다.".into());
    }
    let workspace_tabs: Vec<&str> = tabs
        .iter()
        .filter_map(|tab| tab["id"].as_str())
        .filter(|id| workspace_of(id) == workspace)
        .collect();
    let current_index = workspace_tabs.iter().position(|id| *id == tab_id);
    let destination_index = workspace_tabs.iter().position(|id| *id == destination_id);
    match (current_index, destination_index) {
        (Some(current), Some(destination)) if current < destination => Ok(destination + 1),
        (Some(_), Some(destination)) => Ok(destination),
        _ => Err("탭을 찾을 수 없습니다.".into()),
    }
}

pub fn tabs_dispatch(
    method: &str,
    args: &[Value],
    session: &str,
    worktree: Option<&str>,
    host: Option<&str>,
    shell: Shell,
) -> Value {
    let result = tab_action(method, args, session, worktree, host, shell);
    match (method, result) {
        (_, Ok(value)) => value,
        ("listTerminalTabs", Err(error)) => json!({"ok":false,"tabs":[],"error":error}),
        (_, Err(error)) => json!({"ok":false,"error":error}),
    }
}

fn tab_action(
    method: &str,
    args: &[Value],
    session: &str,
    worktree: Option<&str>,
    host: Option<&str>,
    shell: Shell,
) -> Result<Value, String> {
    let tab = args.get(1).and_then(Value::as_str).unwrap_or("");
    match method {
        "listTerminalTabs" => Ok(json!({"ok":true,"tabs":read_tabs(session, host, shell)?})),
        "createTerminalTab" => {
            let tabs = read_tabs(session, host, shell)?;
            let workspace = tabs
                .iter()
                .find(|tab| tab["isActive"] == true)
                .or(tabs.first())
                .and_then(|tab| tab["id"].as_str())
                .map(workspace_of)
                .filter(|workspace| !workspace.is_empty())
                .ok_or("Herdr 워크스페이스를 찾을 수 없습니다.")?;
            let mut create = vec!["tab", "create", "--workspace", workspace];
            if let Some(path) = worktree {
                create.extend(["--cwd", path]);
            }
            create.push("--focus");
            run(session, &create, host, shell)?;
            Ok(json!({"ok":true}))
        }
        "selectTerminalTab" => {
            run(session, &["tab", "focus", tab], host, shell)?;
            Ok(json!({"ok":true}))
        }
        "renameTerminalTab" => {
            let name = args.get(2).and_then(Value::as_str).unwrap_or("").trim();
            if name.is_empty() {
                return Err("탭 이름은 비워 둘 수 없습니다.".into());
            }
            run(session, &["tab", "rename", tab, name], host, shell)?;
            Ok(json!({"ok":true}))
        }
        "closeTerminalTab" => {
            // 마지막 탭을 지우면 세션까지 사라지므로 창만 닫고, 세션 정리는 태스크 삭제 경로에 맡긴다.
            let tabs = read_tabs(session, host, shell).unwrap_or_default();
            if tabs.len() <= 1 {
                return Ok(json!({"ok":true,"shouldCloseWindow":true}));
            }
            if !tabs.iter().any(|entry| entry["id"] == tab) {
                return Err("탭을 찾을 수 없습니다.".into());
            }
            run(session, &["tab", "close", tab], host, shell)?;
            Ok(json!({"ok":true,"shouldCloseWindow":false}))
        }
        "moveTerminalTab" => {
            let target_index = args
                .get(2)
                .and_then(Value::as_u64)
                .ok_or("이동할 위치가 올바르지 않습니다.")? as usize;
            let tabs = read_tabs(session, host, shell)?;
            let insert_index = move_insert_index(&tabs, tab, target_index)?;
            move_tab(session, tab, insert_index, host, shell)?;
            Ok(json!({"ok":true}))
        }
        _ => Err(format!("알 수 없는 탭 메서드: {method}")),
    }
}

/// 다른 세션 하나가 응답하지 않아도 나머지 세션의 에이전트는 계속 보이도록 세션별 실패를 버린다.
pub fn running_agent_panes(host: Option<&str>, shell: Shell) -> Vec<Value> {
    let sessions = list_sessions(host, shell).unwrap_or_default();
    let running: Vec<&str> = sessions
        .iter()
        .filter(|session| session["running"] == true)
        .filter_map(|session| session["name"].as_str())
        .collect();
    std::thread::scope(|scope| {
        let workers: Vec<_> = running
            .iter()
            .map(|session| scope.spawn(move || session_agent_panes(session, host, shell)))
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().ok().and_then(Result::ok).unwrap_or_default())
            .collect()
    })
}

fn session_agent_panes(
    session: &str,
    host: Option<&str>,
    shell: Shell,
) -> Result<Vec<Value>, String> {
    let agents = run(session, &["agent", "list"], host, shell)?;
    let tabs = read_tabs(session, host, shell)?;
    Ok(agents["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|agent| {
            let provider = agent["agent"].as_str().filter(|p| AGENT_PROVIDERS.contains(p))?;
            let worktree = [&agent["foreground_cwd"], &agent["cwd"]]
                .into_iter()
                .filter_map(Value::as_str)
                .find(|path| !path.is_empty())?;
            let tab_id = agent["tab_id"].as_str().unwrap_or("");
            let tab_name = tabs
                .iter()
                .find(|tab| tab["id"] == tab_id)
                .and_then(|tab| tab["name"].as_str())
                .unwrap_or(tab_id);
            Some(json!({"provider":provider,"sessionName":session,"windowId":tab_id,"windowName":tab_name,"worktreePath":worktree}))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn result(value: Value) -> String {
        json!({"id":"test","result":value}).to_string()
    }

    type Respond = Box<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
    type LayoutCase<'a> = (&'a str, Vec<(&'a str, &'a str)>, Vec<&'a str>);

    /// 실행된 명령을 기록하면서 응답 규칙을 적용하는 가짜 셸.
    struct Recorder {
        calls: Mutex<Vec<(String, Option<String>)>>,
        respond: Respond,
    }
    impl Recorder {
        fn new(respond: impl Fn(&str) -> Result<String, String> + Send + Sync + 'static) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                respond: Box::new(respond),
            }
        }
        fn shell(
            &self,
        ) -> impl Fn(&str, Option<&str>, Duration) -> Result<String, String> + Sync + '_ {
            move |command, host, _| {
                self.calls
                    .lock()
                    .unwrap()
                    .push((command.to_owned(), host.map(String::from)));
                (self.respond)(command)
            }
        }
        fn commands(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.0.clone())
                .collect()
        }
    }

    #[test]
    fn mirror_panes_keep_tab_order_geometry_zoom_and_foreground_process() {
        for zoomed in [false, true] {
            let shell = |script: &str, host: Option<&str>, _: Duration| {
                assert_eq!(host, Some("remote"));
                let result = if script.contains("'pane' 'list'") {
                    json!({"panes":[
                        {"pane_id":"w1:p2","tab_id":"w1:t2","scroll":{"viewport_rows":38}},
                        {"pane_id":"w1:p1","tab_id":"w1:t1","scroll":{"viewport_rows":38}},
                        {"pane_id":"w1:p3","tab_id":"w1:t1","scroll":{"viewport_rows":38}},
                        {"pane_id":"w1:p4","tab_id":"w1:t1","restore_error":"Directory unavailable"}
                    ]})
                } else if script.contains("'tab' 'list'") {
                    json!({"tabs":[{"tab_id":"w1:t1","label":"work"},{"tab_id":"w1:t2","label":"logs"}]})
                } else if script.contains("'pane' 'layout'") {
                    json!({"layout":{"area":{"width":120},"focused_pane_id":"w1:p1","zoomed":zoomed,"panes":[
                        {"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":60}},
                        {"pane_id":"w1:p3","rect":{"x":60,"y":0,"width":60}},
                        {"pane_id":"w1:p2","rect":{"x":0,"y":0,"width":120}}
                    ]}})
                } else if script.ends_with("'w1:p3'") {
                    json!({"process_info":{"foreground_processes":[{"name":"shell"},{"name":"vim"}]}})
                } else if script.ends_with("'w1:p2'") {
                    return Err("process unavailable".to_owned());
                } else {
                    json!({"process_info":{"foreground_processes":[{"name":"zsh"}]}})
                };
                Ok(json!({"result":result}).to_string())
            };
            assert_eq!(
                mirror_panes("feature", Some("remote"), &shell).unwrap(),
                vec![
                    json!({"id":"w1:p1","tabId":"w1:t1","tabName":"work","command":"zsh","left":0,"top":0,"width":if zoomed {120} else {60},"height":38}),
                    json!({"id":"w1:p3","tabId":"w1:t1","tabName":"work","command":"vim","left":60,"top":0,"width":60,"height":38}),
                    json!({"id":"w1:p2","tabId":"w1:t2","tabName":"logs","command":"","left":0,"top":0,"width":120,"height":38}),
                ]
            );
        }
    }
    #[test]
    fn pane_extent_keeps_history_and_native_viewport_rows() {
        let shell = |script: &str, host: Option<&str>, _: Duration| {
            assert_eq!(script, command("feature", &["pane", "get", "w1:p1"]));
            assert_eq!(host, Some("remote"));
            Ok(json!({"result":{"pane":{"scroll":{"max_offset_from_bottom":1024,"viewport_rows":38}}}}).to_string())
        };
        assert_eq!(
            pane_extent("feature", "w1:p1", Some("remote"), &shell).unwrap(),
            PaneExtent {
                history_rows: 1024,
                viewport_rows: 38
            }
        );
    }
    #[test]
    fn observe_records_preserve_ansi_utf8_and_distinguish_full_delta_close() {
        use base64::Engine;
        let output = "한글\x1b[?1049h".as_bytes();
        for full in [false, true] {
            let record = json!({"type":"terminal.frame","bytes":base64::engine::general_purpose::STANDARD.encode(output),"full":full});
            assert_eq!(
                parse_observe_record(&record.to_string()),
                Some(ObserveRecord::Frame {
                    full,
                    output: output.to_vec()
                })
            );
        }
        assert_eq!(
            parse_observe_record(r#"{"type":"terminal.closed"}"#),
            Some(ObserveRecord::Closed)
        );
        for invalid in [
            "not-json",
            r#"{"type":"terminal.frame","bytes":2}"#,
            r#"{"type":"terminal.frame","bytes":"%bad"}"#,
            r#"{"type":"ignored"}"#,
        ] {
            assert_eq!(parse_observe_record(invalid), None);
        }
    }
    #[test]
    fn mobile_pane_commands_keep_literal_input_nul_and_native_observer_size() {
        let script = write_pane_command("feature", "w1:p1", "--help\0한글\r\x03");
        assert_eq!(
            script,
            [
                command("feature", &["pane", "send-text", "w1:p1", "--help"]),
                command("feature", &["pane", "send-keys", "w1:p1", "ctrl+space"]),
                command("feature", &["pane", "send-text", "w1:p1", "한글\r\x03"])
            ]
            .join(" && ")
        );
        assert_eq!(write_pane_command("feature", "w1:p1", ""), "");
        assert_eq!(
            observe_command("feature", "w1:p1", 132, 38),
            command(
                "feature",
                &["terminal", "session", "observe", "w1:p1", "--cols", "132", "--rows", "38"]
            )
        );
        assert_eq!(
            recent_screen_command("feature", "w1:p1"),
            command(
                "feature",
                &[
                    "pane", "read", "w1:p1", "--source", "recent", "--lines", "2000", "--format",
                    "ansi"
                ]
            )
        );
        for action in ["scroll-up", "copy-mode", "toggle-floating", "unknown"] {
            assert_eq!(pane_shortcut_command("feature", "w1:p1", action), None);
        }
        assert_eq!(
            pane_shortcut_command("feature", "w1:p1", "split-right"),
            Some(command(
                "feature",
                &[
                    "pane",
                    "split",
                    "w1:p1",
                    "--direction",
                    "right",
                    "--no-focus"
                ]
            ))
        );
        assert_eq!(
            pane_shortcut_command("feature", "w1:p1", "split-down"),
            Some(command(
                "feature",
                &[
                    "pane",
                    "split",
                    "w1:p1",
                    "--direction",
                    "down",
                    "--no-focus"
                ]
            ))
        );
        assert_eq!(
            pane_shortcut_command("feature", "w1:p1", "toggle-fullscreen"),
            Some(command("feature", &["pane", "zoom", "w1:p1", "--toggle"]))
        );
    }

    #[test]
    fn valid_session_names_are_kept_and_others_become_deterministic_ascii() {
        for name in ["normal-branch", &"a".repeat(40)] {
            assert_eq!(sanitize_session_name(name), name);
        }
        let pattern = regex::Regex::new(r"^[a-zA-Z0-9._-]{1,40}$").unwrap();
        for name in ["", ".", "..", "한글/브랜치", &"a".repeat(65)] {
            let sanitized = sanitize_session_name(name);
            assert!(pattern.is_match(&sanitized), "{sanitized}");
            assert_eq!(sanitize_session_name(name), sanitized);
        }
        assert_ne!(
            sanitize_session_name(&format!("{}x", "a".repeat(80))),
            sanitize_session_name(&format!("{}y", "a".repeat(80)))
        );
    }

    #[test]
    fn command_isolates_outer_herdr_variables_and_falls_back_to_user_install() {
        let built = command("proj-main", &["tab", "list"]);
        assert!(built.starts_with("env -u HERDR_ENV -u HERDR_SESSION -u HERDR_SOCKET_PATH"));
        assert!(built.contains("$HOME/.local/bin/herdr"));
        assert!(built.ends_with("--session 'proj-main' 'tab' 'list'"));
        let cleanup = cleanup_command("feature");
        assert!(cleanup.contains("'session' 'stop' 'feature' >/dev/null 2>&1 || true;"));
        assert!(cleanup.ends_with("'session' 'delete' 'feature'"));
    }

    #[test]
    fn reattaching_existing_session_does_not_replay_layout_or_commands() {
        let recorder = Recorder::new(|_| Ok(result(json!({"workspaces":[{"workspace_id":"w1"}]}))));
        let layout =
            json!({"layoutType":"quad","panes":[{"position":0,"command":"do-not-repeat"}]});
        ensure_session("reattach", "/work", &layout, None, &recorder.shell()).unwrap();
        assert_eq!(
            recorder.commands(),
            vec![command("reattach", &["workspace", "list"])]
        );
    }

    #[test]
    fn every_layout_sends_position_commands_to_matching_panes() {
        let cases: [LayoutCase; 6] = [
            ("single", vec![], vec!["w1:p1"]),
            (
                "horizontal_2",
                vec![("w1:p1", "down")],
                vec!["w1:p1", "w1:p2"],
            ),
            (
                "vertical_2",
                vec![("w1:p1", "right")],
                vec!["w1:p1", "w1:p2"],
            ),
            (
                "left_right_tb",
                vec![("w1:p1", "right"), ("w1:p2", "down")],
                vec!["w1:p1", "w1:p2", "w1:p3"],
            ),
            (
                "left_tb_right",
                vec![("w1:p1", "right"), ("w1:p1", "down")],
                vec!["w1:p1", "w1:p3", "w1:p2"],
            ),
            (
                "quad",
                vec![("w1:p1", "right"), ("w1:p1", "down"), ("w1:p2", "down")],
                vec!["w1:p1", "w1:p2", "w1:p3", "w1:p4"],
            ),
        ];
        for (layout_type, splits, ids) in cases {
            let next_pane = AtomicUsize::new(2);
            let recorder = Recorder::new(move |command| {
                Ok(if command.contains("'workspace' 'list'") {
                    result(json!({"workspaces":[]}))
                } else if command.contains("'workspace' 'create'") {
                    result(json!({"root_pane":{"pane_id":"w1:p1"}}))
                } else if command.contains("'pane' 'split'") {
                    let pane = next_pane.fetch_add(1, Ordering::SeqCst);
                    result(json!({"pane":{"pane_id":format!("w1:p{pane}")}}))
                } else {
                    String::new()
                })
            });
            let session = format!("layout-{layout_type}");
            let panes: Vec<Value> = (0..ids.len())
                .map(|position| json!({"position":position,"command":format!("echo pane-{position}")}))
                .collect();
            let layout = json!({"layoutType":layout_type,"panes":panes});
            ensure_session(
                &session,
                "/work tree",
                &layout,
                Some("remote"),
                &recorder.shell(),
            )
            .unwrap();
            let commands = recorder.commands();
            assert!(commands.contains(&command(
                &session,
                &["workspace", "create", "--cwd", "/work tree", "--focus"]
            )));
            let expected_splits: Vec<String> = splits
                .iter()
                .map(|(id, direction)| {
                    command(
                        &session,
                        &[
                            "pane",
                            "split",
                            id,
                            "--direction",
                            direction,
                            "--cwd",
                            "/work tree",
                        ],
                    )
                })
                .collect();
            let actual_splits: Vec<String> = commands
                .iter()
                .filter(|c| c.contains("'pane' 'split'"))
                .cloned()
                .collect();
            assert_eq!(actual_splits, expected_splits, "{layout_type}");
            let expected_sends: Vec<String> = ids
                .iter()
                .enumerate()
                .map(|(position, id)| {
                    command(
                        &session,
                        &["pane", "send-text", id, &format!("echo pane-{position}\r")],
                    )
                })
                .collect();
            let actual_sends: Vec<String> = commands
                .iter()
                .filter(|c| c.contains("'pane' 'send-text'"))
                .cloned()
                .collect();
            assert_eq!(actual_sends, expected_sends, "{layout_type}");
            assert!(recorder
                .calls
                .lock()
                .unwrap()
                .iter()
                .all(|c| c.1.as_deref() == Some("remote")));
        }
    }

    #[test]
    fn concurrent_connections_share_one_bootstrap() {
        let recorder = Recorder::new(|_| {
            std::thread::sleep(Duration::from_millis(200));
            Ok(result(json!({"workspaces":[{"workspace_id":"w1"}]})))
        });
        let shell = recorder.shell();
        std::thread::scope(|scope| {
            let first =
                scope.spawn(|| ensure_session("concurrent", "/work", &Value::Null, None, &shell));
            let second =
                scope.spawn(|| ensure_session("concurrent", "/work", &Value::Null, None, &shell));
            first.join().unwrap().unwrap();
            second.join().unwrap().unwrap();
        });
        assert_eq!(recorder.commands().len(), 1);
    }

    #[test]
    fn ssh_transport_failure_does_not_start_remote_server() {
        let recorder = Recorder::new(|_| Err("Connection refused (exit 255)".into()));
        let error = ensure_session(
            "transport",
            "/work",
            &Value::Null,
            Some("remote"),
            &recorder.shell(),
        )
        .unwrap_err();
        assert!(error.contains("exit 255"));
        assert_eq!(recorder.commands().len(), 1);
    }

    #[test]
    fn missing_server_is_started_in_background_then_polled() {
        let started = AtomicUsize::new(0);
        let recorder = Recorder::new(move |command| {
            if command.starts_with("nohup ") {
                started.fetch_add(1, Ordering::SeqCst);
                return Ok(String::new());
            }
            if started.load(Ordering::SeqCst) == 0 {
                return Err("no server (exit 1)".into());
            }
            Ok(result(json!({"workspaces":[{"workspace_id":"w1"}]})))
        });
        ensure_session("start", "/work", &Value::Null, None, &recorder.shell()).unwrap();
        let commands = recorder.commands();
        assert_eq!(
            commands[1],
            format!(
                "nohup {} </dev/null >/dev/null 2>&1 &",
                command("start", &["server"])
            )
        );
        assert_eq!(commands.len(), 3);
    }

    #[test]
    fn failed_initial_layout_cleans_up_new_session() {
        let recorder = Recorder::new(|command| {
            if command.contains("'workspace' 'list'") {
                Ok(result(json!({"workspaces":[]})))
            } else if command.contains("'workspace' 'create'") {
                Ok(result(json!({"root_pane":{"pane_id":"w1:p1"}})))
            } else if command.contains("'pane' 'split'") {
                Err("split failed".into())
            } else {
                Ok(String::new())
            }
        });
        let layout = json!({"layoutType":"quad","panes":[]});
        let error =
            ensure_session("cleanup", "/work", &layout, None, &recorder.shell()).unwrap_err();
        assert_eq!(error, "split failed");
        assert_eq!(
            recorder.commands().last().unwrap(),
            &cleanup_command("cleanup")
        );
    }

    #[test]
    fn cli_api_errors_are_not_mistaken_for_empty_lists() {
        let recorder = Recorder::new(|_| {
            Ok(json!({"error":{"code":"not_found","message":"missing session"}}).to_string())
        });
        let error = read_tabs("feature", None, &recorder.shell()).unwrap_err();
        assert!(error.contains("missing session"));
    }

    fn serve_once(
        directory: &Path,
        response: &'static [&'static str],
    ) -> (PathBuf, std::thread::JoinHandle<Value>) {
        let socket = directory.join("api");
        let listener = UnixListener::bind(&socket).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
                request.push(byte[0]);
            }
            for part in response {
                stream.write_all(part.as_bytes()).unwrap();
                stream.flush().unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
            serde_json::from_slice(&request).unwrap()
        });
        (socket, handle)
    }

    fn sessions_with_socket(socket: &Path, running: bool) -> Recorder {
        let listing =
            json!({"sessions":[{"name":"feature","running":running,"socket_path":socket}]})
                .to_string();
        Recorder::new(move |_| Ok(listing.clone()))
    }

    #[test]
    fn fragmented_socket_response_sends_single_exact_tab_move() {
        let directory = tempfile::tempdir().unwrap();
        let (socket, server) = serve_once(
            directory.path(),
            &["{\"id\":\"kanvibe\",\"res", "ult\":{\"type\":\"ok\"}}\n"],
        );
        let recorder = sessions_with_socket(&socket, true);
        move_tab("feature", "w1:t1", 3, None, &recorder.shell()).unwrap();
        assert_eq!(
            server.join().unwrap(),
            json!({"id":"kanvibe","method":"tab.move","params":{"tab_id":"w1:t1","insert_index":3}})
        );
    }

    #[test]
    fn socket_error_response_reaches_caller() {
        let directory = tempfile::tempdir().unwrap();
        let (socket, _server) = serve_once(directory.path(), &["{\"id\":\"kanvibe\",\"error\":{\"code\":\"not_found\",\"message\":\"tab missing\"}}\n"]);
        let recorder = sessions_with_socket(&socket, true);
        let error = move_tab("feature", "w1:t1", 3, None, &recorder.shell()).unwrap_err();
        assert!(error.contains("tab missing"));
    }

    #[test]
    fn stopped_session_receives_no_socket_request() {
        let recorder = sessions_with_socket(Path::new("/missing"), false);
        let error = move_tab("feature", "w1:t1", 0, None, &recorder.shell()).unwrap_err();
        assert!(error.contains("세션을 찾을 수 없습니다"));
    }

    #[test]
    fn unresponsive_socket_times_out_without_resending() {
        let directory = tempfile::tempdir().unwrap();
        let socket = directory.path().join("api");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let counter = requests.clone();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 1024];
            while let Ok(count) = stream.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                counter.fetch_add(
                    buffer[..count].iter().filter(|b| **b == b'\n').count(),
                    Ordering::SeqCst,
                );
            }
        });
        let error = request_socket(&socket, "tab.move", &json!({}), Duration::from_millis(300))
            .unwrap_err();
        assert!(error.contains("시간 초과"));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn running_agents_are_collected_even_when_another_session_fails() {
        let recorder = Recorder::new(|command| {
            if command.contains("'session' 'list'") {
                return Ok(json!({"sessions":[{"name":"feature","running":true},{"name":"broken","running":true}]}).to_string());
            }
            if command.contains("--session 'broken'") {
                return Err("offline".into());
            }
            if command.contains("'agent' 'list'") {
                return Ok(result(
                    json!({"agents":[{"agent":"codex","tab_id":"w1:t1","cwd":"/old","foreground_cwd":"/work"},{"agent":null,"cwd":"/work"}]}),
                ));
            }
            Ok(result(
                json!({"tabs":[{"tab_id":"w1:t1","label":"coding","focused":true}]}),
            ))
        });
        assert_eq!(
            running_agent_panes(None, &recorder.shell()),
            vec![
                json!({"provider":"codex","sessionName":"feature","windowId":"w1:t1","windowName":"coding","worktreePath":"/work"})
            ]
        );
    }

    fn write_fake_ssh(directory: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join("fake-ssh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn forwarded_local_socket(args: &str) -> PathBuf {
        let lines: Vec<&str> = args.lines().collect();
        let forward = lines[lines.iter().position(|l| *l == "-L").unwrap() + 1];
        PathBuf::from(forward.rsplit_once(':').unwrap().0)
    }

    #[test]
    fn ssh_tunnel_uses_isolated_forwarding_and_cleans_up_process_and_socket_directory() {
        let directory = tempfile::tempdir().unwrap();
        let args_file = directory.path().join("args");
        let program = write_fake_ssh(
            directory.path(),
            &format!(
                "printf '%s\\n' \"$@\" > {}\nexec sleep 30",
                crate::runtime::quote(&args_file.to_string_lossy())
            ),
        );
        let watched = args_file.clone();
        let server = std::thread::spawn(move || {
            let args = loop {
                if let Ok(args) = std::fs::read_to_string(&watched) {
                    if args.ends_with("remote\n") {
                        break args;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            let listener = UnixListener::bind(forwarded_local_socket(&args)).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            let mut byte = [0u8; 1];
            while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {}
            stream
                .write_all(b"{\"id\":\"kanvibe\",\"result\":{\"type\":\"ok\"}}\n")
                .unwrap();
            args
        });
        let started = Instant::now();
        request_through_tunnel(
            &program,
            "remote",
            "/remote/config/herdr.sock",
            "tab.move",
            &json!({}),
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(10));
        let args = server.join().unwrap();
        for expected in [
            "BatchMode=yes",
            "ControlMaster=no",
            "ControlPath=none",
            "ExitOnForwardFailure=yes",
            "-N",
        ] {
            assert!(args.lines().any(|line| line == expected), "{expected}");
        }
        assert!(args.contains(":/remote/config/herdr.sock\n"));
        assert_eq!(args.lines().last(), Some("remote"));
        assert!(!forwarded_local_socket(&args).parent().unwrap().exists());
    }

    #[test]
    fn ssh_failure_is_reported_and_temporary_socket_path_removed() {
        let directory = tempfile::tempdir().unwrap();
        let args_file = directory.path().join("args");
        let program = write_fake_ssh(
            directory.path(),
            &format!(
                "printf '%s\\n' \"$@\" > {}\necho 'ssh unavailable' >&2\nexit 255",
                crate::runtime::quote(&args_file.to_string_lossy())
            ),
        );
        let error = request_through_tunnel(
            &program,
            "remote",
            "/remote/herdr.sock",
            "tab.move",
            &json!({}),
            Duration::from_secs(10),
        )
        .unwrap_err();
        assert_eq!(error, "ssh unavailable");
        let args = std::fs::read_to_string(args_file).unwrap();
        assert!(!forwarded_local_socket(&args).parent().unwrap().exists());
    }

    #[test]
    fn unready_ssh_tunnel_times_out_and_cleans_up() {
        let directory = tempfile::tempdir().unwrap();
        let args_file = directory.path().join("args");
        let program = write_fake_ssh(
            directory.path(),
            &format!(
                "printf '%s\\n' \"$@\" > {}\nexec sleep 30",
                crate::runtime::quote(&args_file.to_string_lossy())
            ),
        );
        let started = Instant::now();
        let error = request_through_tunnel(
            &program,
            "remote",
            "/remote/herdr.sock",
            "tab.move",
            &json!({}),
            Duration::from_secs(2),
        )
        .unwrap_err();
        assert!(error.contains("시간 초과"));
        assert!(started.elapsed() < Duration::from_secs(6));
        let args = std::fs::read_to_string(args_file).unwrap();
        assert!(!forwarded_local_socket(&args).parent().unwrap().exists());
    }

    fn workspace_tabs() -> Value {
        json!({"tabs":[
            {"tab_id":"w1:t1","label":"same","focused":false},
            {"tab_id":"w1:t2","label":"same","focused":false},
            {"tab_id":"w2:t1","label":"same","focused":false},
            {"tab_id":"w2:t2","label":"same","focused":true},
        ]})
    }

    #[test]
    fn tabs_keep_native_ids_for_duplicate_names_and_active_state() {
        let recorder = Recorder::new(|_| {
            Ok(result(json!({"tabs":[
                {"tab_id":"w1:t1","number":1,"label":"shell","focused":true},
                {"tab_id":"w1:t2","number":2,"label":"shell","focused":false},
            ]})))
        });
        let listed = tabs_dispatch(
            "listTerminalTabs",
            &[json!("task-1")],
            "proj-main",
            None,
            None,
            &recorder.shell(),
        );
        assert_eq!(
            listed,
            json!({"ok":true,"tabs":[
                {"id":"w1:t1","nativeIndex":0,"name":"shell","isActive":true},
                {"id":"w1:t2","nativeIndex":1,"name":"shell","isActive":false},
            ]})
        );
        assert!(recorder.commands()[0].ends_with("--session 'proj-main' 'tab' 'list'"));
    }

    #[test]
    fn new_tab_is_created_in_active_workspace() {
        let recorder = Recorder::new(|_| Ok(result(workspace_tabs())));
        let created = tabs_dispatch(
            "createTerminalTab",
            &[json!("task-1")],
            "proj-main",
            Some("/work/tree"),
            Some("remote-herdr"),
            &recorder.shell(),
        );
        assert_eq!(created, json!({"ok":true}));
        assert!(recorder.commands().iter().any(
            |c| c.ends_with("'tab' 'create' '--workspace' 'w2' '--cwd' '/work/tree' '--focus'")
        ));
        assert!(recorder
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|c| c.1.as_deref() == Some("remote-herdr")));
    }

    #[test]
    fn move_converts_to_workspace_insert_boundary_and_rejects_cross_workspace() {
        let tabs: Vec<Value> = workspace_tabs()["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, tab)| json!({"id":tab["tab_id"],"nativeIndex":index,"name":tab["label"],"isActive":tab["focused"]}))
            .collect();
        assert_eq!(move_insert_index(&tabs, "w2:t2", 2), Ok(0));
        assert_eq!(move_insert_index(&tabs, "w2:t1", 3), Ok(2));
        assert!(move_insert_index(&tabs, "w2:t2", 0)
            .unwrap_err()
            .contains("같은 워크스페이스"));
        let recorder = Recorder::new(|_| Ok(result(workspace_tabs())));
        let rejected = tabs_dispatch(
            "moveTerminalTab",
            &[json!("task-1"), json!("w2:t2"), json!(0)],
            "proj-main",
            None,
            None,
            &recorder.shell(),
        );
        assert_eq!(rejected["ok"], false);
        assert!(!recorder
            .commands()
            .iter()
            .any(|c| c.contains("'session' 'list'")));
    }

    #[test]
    fn last_tab_closes_window_without_deleting_tab() {
        let recorder = Recorder::new(|_| {
            Ok(result(
                json!({"tabs":[{"tab_id":"w1:t1","label":"same","focused":false}]}),
            ))
        });
        let closed = tabs_dispatch(
            "closeTerminalTab",
            &[json!("task-1"), json!("w1:t1")],
            "proj-main",
            None,
            None,
            &recorder.shell(),
        );
        assert_eq!(closed, json!({"ok":true,"shouldCloseWindow":true}));
        assert!(!recorder
            .commands()
            .iter()
            .any(|c| c.contains("'tab' 'close'")));
    }

    #[test]
    fn duplicate_names_still_target_exact_tab_by_id() {
        let recorder = Recorder::new(|command| {
            Ok(if command.contains("'tab' 'list'") {
                result(workspace_tabs())
            } else {
                result(json!({"type":"ok"}))
            })
        });
        let shell = recorder.shell();
        assert_eq!(
            tabs_dispatch(
                "selectTerminalTab",
                &[json!("task-1"), json!("w2:t2")],
                "proj-main",
                None,
                None,
                &shell
            ),
            json!({"ok":true})
        );
        assert_eq!(
            tabs_dispatch(
                "renameTerminalTab",
                &[json!("task-1"), json!("w2:t2"), json!(" new name ")],
                "proj-main",
                None,
                None,
                &shell
            ),
            json!({"ok":true})
        );
        assert_eq!(
            tabs_dispatch(
                "closeTerminalTab",
                &[json!("task-1"), json!("w2:t2")],
                "proj-main",
                None,
                None,
                &shell
            ),
            json!({"ok":true,"shouldCloseWindow":false})
        );
        let commands = recorder.commands();
        assert!(commands
            .iter()
            .any(|c| c.ends_with("'tab' 'focus' 'w2:t2'")));
        assert!(commands
            .iter()
            .any(|c| c.ends_with("'tab' 'rename' 'w2:t2' 'new name'")));
        assert!(commands
            .iter()
            .any(|c| c.ends_with("'tab' 'close' 'w2:t2'")));
    }
}
