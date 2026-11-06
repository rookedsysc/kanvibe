use crate::ai::{client, now, set_setting, setting, text};
use chrono::Utc;
use rand::Rng;
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
struct Pairing {
    code: Option<String>,
    expires: i64,
    failures: usize,
}
static PAIRING: OnceLock<Mutex<Pairing>> = OnceLock::new();
static DEVICES_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn pairing() -> &'static Mutex<Pairing> {
    PAIRING.get_or_init(|| Mutex::new(Pairing::default()))
}
fn secret_equal(a: &str, b: &str) -> bool {
    use sha2::{Digest, Sha256};
    let a = Sha256::digest(a.as_bytes());
    let b = Sha256::digest(b.as_bytes());
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
fn issue_code() -> Value {
    let mut p = pairing().lock().unwrap();
    p.code = Some(format!("{:06}", rand::rng().random_range(0..1000000)));
    p.expires = Utc::now().timestamp_millis() + 300000;
    p.failures = 0;
    json!({"code":p.code,"expiresAt":p.expires})
}
fn redeem(p: &mut Pairing, code: &str, time: i64) -> bool {
    if p.expires > time && p.code.as_deref().is_some_and(|c| secret_equal(c, code)) {
        p.code = None;
        return true;
    }
    p.failures += 1;
    if p.failures >= 5 {
        p.code = None;
    }
    false
}
fn read_devices(app: &AppHandle) -> Result<Vec<Value>, String> {
    Ok(setting(app, "mobile_paired_devices")?
        .and_then(|s| serde_json::from_str::<Vec<Value>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|v| {
            ["deviceId", "token", "deviceName", "pairedAt"]
                .iter()
                .all(|k| v[*k].is_string())
        })
        .collect())
}
fn save_devices(app: &AppHandle, devices: &[Value]) -> Result<(), String> {
    set_setting(app, "mobile_paired_devices", &json!(devices).to_string())
}
pub fn authorize(app: &AppHandle, authorization: Option<&str>) -> Result<Option<String>, String> {
    let token = authorization
        .and_then(|s| s.split_once(' '))
        .filter(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("bearer")
                && !token.is_empty()
                && !token.chars().any(char::is_whitespace)
        })
        .map(|(_, token)| token);
    let Some(token) = token else { return Ok(None) };
    Ok(read_devices(app)?
        .iter()
        .find(|d| secret_equal(text(d, "token"), token))
        .map(|d| text(d, "deviceId").to_string()))
}
pub fn pair_device(app: &AppHandle, code: &str, name: &str) -> Result<Option<String>, String> {
    let success = redeem(
        &mut *pairing().lock().map_err(|e| e.to_string())?,
        code,
        Utc::now().timestamp_millis(),
    );
    if !success {
        std::thread::sleep(Duration::from_millis(300));
        return Ok(None);
    }
    let _guard = DEVICES_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|e| e.to_string())?;
    let mut devices = read_devices(app)?;
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    let token = hex::encode(bytes);
    devices.push(json!({"deviceId":uuid::Uuid::new_v4().simple().to_string()[..16].to_string(),"token":token,"deviceName":if name.is_empty(){"모바일 기기".to_string()}else{name.chars().take(64).collect()},"pairedAt":now()}));
    save_devices(app, &devices)?;
    Ok(Some(token))
}
pub fn unpair(app: &AppHandle, id: &str) -> Result<bool, String> {
    let _guard = DEVICES_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|e| e.to_string())?;
    let mut devices = read_devices(app)?;
    let before = devices.len();
    devices.retain(|d| text(d, "deviceId") != id);
    if devices.len() == before {
        return Ok(false);
    }
    save_devices(app, &devices)?;
    crate::server::broadcast(json!({"type":"device-unpaired","deviceId":id}));
    Ok(true)
}
pub fn valid_subscription(v: &Value) -> bool {
    let token = text(v, "pushToken");
    let id = text(v, "connectionId");
    !token.is_empty()
        && token.len() <= 4096
        && !token.chars().any(char::is_whitespace)
        && !id.is_empty()
        && id.len() <= 128
        && !id.chars().any(char::is_whitespace)
        && ["android", "ios"].contains(&text(v, "platform"))
}
pub fn subscription(app: &AppHandle, id: &str, value: Option<&Value>) -> Result<bool, String> {
    if value.is_some_and(|v| !valid_subscription(v)) {
        return Err("알림 구독 값이 올바르지 않습니다".into());
    }
    let _guard = DEVICES_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|e| e.to_string())?;
    let mut devices = read_devices(app)?;
    let Some(device) = devices.iter_mut().find(|d| text(d, "deviceId") == id) else {
        return Ok(false);
    };
    if let Some(v) = value {
        device["notification"] = json!({"pushToken":v["pushToken"],"platform":v["platform"],"connectionId":v["connectionId"]});
    } else {
        device.as_object_mut().unwrap().remove("notification");
    }
    save_devices(app, &devices)?;
    Ok(true)
}
fn truncate_encoded(s: &str, max: usize) -> String {
    let encoded = |s: &str| {
        serde_json::to_string(s)
            .map(|s| s.len().saturating_sub(2))
            .unwrap_or(0)
    };
    if encoded(s) <= max {
        return s.into();
    }
    let mut result = String::new();
    for ch in s.chars() {
        let next = encoded(&ch.to_string());
        if encoded(&result) + next > max.saturating_sub(3) {
            break;
        }
        result.push(ch);
    }
    result.push('…');
    result
}
pub fn mobile_notification(event: &Value) -> Value {
    let status = text(event, "newStatus");
    let title = format!(
        "{} — {}",
        text(event, "projectName"),
        text(event, "branchName")
    );
    let mut body = format!("{}: {}로 변경", text(event, "taskTitle"), status);
    if let Some(description) = event["description"].as_str().filter(|s| !s.is_empty()) {
        body.push('\n');
        body.push_str(description);
    }
    json!({"type":"task-status-changed","eventId":uuid::Uuid::new_v4().to_string(),"taskId":event["taskId"],"title":truncate_encoded(&title,128),"body":truncate_encoded(&body,700),"status":status,"createdAt":now()})
}
pub fn deliver_push(app: AppHandle, event: Value) {
    let Ok(url) = std::env::var("KANVIBE_MOBILE_PUSH_RELAY_URL") else {
        return;
    };
    let Ok(token) = std::env::var("KANVIBE_MOBILE_PUSH_RELAY_TOKEN") else {
        return;
    };
    std::thread::spawn(move || {
        let Ok(devices) = read_devices(&app) else {
            return;
        };
        let Ok(client) = client() else { return };
        for device in devices {
            let sub = &device["notification"];
            if !valid_subscription(sub) {
                continue;
            }
            let mut data = event.clone();
            data["connectionId"] = sub["connectionId"].clone();
            let request=client.post(&url).timeout(Duration::from_secs(5)).bearer_auth(&token).json(&json!({"token":sub["pushToken"],"notification":{"title":event["title"],"body":event["body"]},"data":data}));
            if let Ok(response) = request.send() {
                if response.status().as_u16() == 410 {
                    let _guard = DEVICES_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
                    if let Ok(mut devices) = read_devices(&app) {
                        if let Some(d) = devices.iter_mut().find(|d| {
                            d["deviceId"] == device["deviceId"]
                                && d["notification"]["pushToken"] == sub["pushToken"]
                        }) {
                            d.as_object_mut().unwrap().remove("notification");
                            let _ = save_devices(&app, &devices);
                        }
                    }
                }
            }
        }
    });
}
fn call(app: &AppHandle, namespace: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
    crate::core::dispatch(app, namespace, method, args)
}
pub fn mobile_board(app: &AppHandle) -> Result<Value, String> {
    let mut board = call(app, "kanban", "getTasksByStatus", vec![])?;
    board["projects"] = call(app, "project", "getAllProjects", vec![])?;
    board["defaultSessionType"] = call(app, "appSettings", "getDefaultSessionType", vec![])?;
    Ok(board)
}
pub fn mobile_task(app: &AppHandle, id: &str) -> Result<Value, String> {
    let mut task = call(app, "kanban", "getTaskById", vec![json!(id)])?;
    if task.is_object() {
        task["projectName"] = task["project"]["name"].clone();
    }
    Ok(task)
}
pub fn mobile_create(app: &AppHandle, draft: &Value) -> Result<Value, String> {
    let mut draft = draft.clone();
    for (key, values) in [
        (
            "status",
            vec!["todo", "progress", "pending", "review", "done"],
        ),
        ("sessionType", vec!["terminal", "tmux", "zellij"]),
        ("priority", vec!["low", "medium", "high"]),
    ] {
        if let Some(v) = draft.get(key).filter(|v| !v.is_null()) {
            if !values.contains(&v.as_str().unwrap_or("")) {
                return Err(format!("잘못된 {key} 값입니다"));
            }
        }
    }
    for key in ["branchName", "baseBranch"] {
        if let Some(branch) = draft[key].as_str() {
            if branch.starts_with('-') || branch.contains(['\r', '\n', '\0']) {
                return Err("브랜치 이름이 올바르지 않습니다".into());
            }
        }
    }
    if draft.get("sessionType").is_none() {
        draft["sessionType"] = call(app, "appSettings", "getDefaultSessionType", vec![])?;
    }
    call(app, "kanban", "createTask", vec![draft])
}
#[derive(Clone)]
pub struct MirrorTarget {
    pub kind: String,
    pub name: String,
    pub host: Option<String>,
}
pub fn target(app: &AppHandle, id: &str) -> Result<MirrorTarget, String> {
    let task = app
        .state::<crate::database::Database>()
        .task(id)?
        .ok_or("no-session")?;
    let kind = text(&task, "sessionType");
    let name = text(&task, "sessionName");
    if kind.is_empty() || name.is_empty() {
        return Err("no-session".into());
    }
    let host = task["sshHost"]
        .as_str()
        .or_else(|| task["project"]["sshHost"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    if let Some(host) = &host {
        crate::git::run("true", Some(host)).map_err(|_| "remote-unreachable".to_string())?;
    }
    Ok(MirrorTarget {
        kind: kind.into(),
        name: name.into(),
        host,
    })
}
fn q(s: &str) -> String {
    crate::runtime::quote(s)
}
fn needs_session_connection(task: &Value) -> bool {
    (text(task, "sessionType").is_empty() || text(task, "sessionName").is_empty())
        && !text(task, "projectId").is_empty()
        && (!text(task, "branchName").is_empty() || !text(task, "baseBranch").is_empty())
}
static SESSION_CONNECTION: OnceLock<Mutex<()>> = OnceLock::new();
pub fn surfaces(app: &AppHandle, id: &str) -> Result<Value, String> {
    {
        let _guard = SESSION_CONNECTION
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|e| e.to_string())?;
        let task = app
            .state::<crate::database::Database>()
            .task(id)?
            .ok_or("no-session")?;
        if needs_session_connection(&task) {
            let kind = if text(&task, "sessionType").is_empty() {
                call(app, "appSettings", "getDefaultSessionType", vec![])?
            } else {
                task["sessionType"].clone()
            };
            let connected = call(
                app,
                "kanban",
                "connectTerminalSession",
                vec![json!(id), kind],
            )?;
            if connected.is_null() {
                return Err("session-not-running".into());
            }
        }
    }
    surfaces_for_target(app, id, target(app, id)?)
}
fn surfaces_for_target(app: &AppHandle, id: &str, t: MirrorTarget) -> Result<Value, String> {
    if t.kind == "terminal" {
        let tabs = crate::terminal::tabs_dispatch(app, "listTerminalTabs", vec![json!(id)])?;
        let items = tabs
            .get("tabs")
            .or_else(|| tabs.get("entries"))
            .unwrap_or(&tabs)
            .as_array()
            .cloned()
            .unwrap_or_default();
        let tabs:Vec<_>=items.iter().map(|tab|{let tabid=text(tab,"id");let (cols,rows)=crate::terminal::dimensions(app,id,Some(tabid)).unwrap_or((80,24));json!({"id":tabid,"name":tab["name"],"panes":[{"id":tabid,"tabId":tabid,"tabName":tab["name"],"command":"","left":0,"top":0,"width":cols,"height":rows}]})}).collect();
        return Ok(json!({"taskId":id,"sessionType":t.kind,"canResize":true,"tabs":tabs}));
    }
    // 모바일 pane 미러는 tmux·zellij만 지원한다. Herdr 세션을 zellij로 오인해 띄우지 않는다.
    if t.kind == "herdr" {
        return Err("session-not-running".into());
    }
    let (command, fmt) = if t.kind == "tmux" {
        (format!("tmux list-panes -s -t {} -F '#{{pane_id}}\t#{{window_id}}\t#{{window_name}}\t#{{pane_current_command}}\t#{{pane_left}}\t#{{pane_top}}\t#{{pane_width}}\t#{{pane_height}}'",q(&t.name)),true)
    } else {
        (
            format!(
                "zellij --session {} action list-panes --json -a",
                q(&t.name)
            ),
            false,
        )
    };
    let output = match crate::git::run(&command, t.host.as_deref()) {
        Ok(out) => out,
        Err(_) => {
            crate::terminal::open(app, "mobile:bootstrap", id, None, 80, 24)
                .map_err(|_| "session-not-running".to_string())?;
            std::thread::sleep(Duration::from_millis(400));
            let result = crate::git::run(&command, t.host.as_deref()).map_err(|_| {
                if t.kind == "zellij" {
                    "zellij-too-old".to_string()
                } else {
                    "session-not-running".to_string()
                }
            });
            let _ = crate::terminal::detach(app, "mobile:bootstrap", id, None);
            result?
        }
    };
    let panes: Vec<Value> = if fmt {
        output.lines().filter_map(|line|{let v:Vec<_>=line.split('\t').collect();if v.len()!=8{return None}Some(json!({"id":v[0],"tabId":v[1],"tabName":v[2],"command":v[3],"left":v[4].parse::<u16>().ok()?,"top":v[5].parse::<u16>().ok()?,"width":v[6].parse::<u16>().ok()?,"height":v[7].parse::<u16>().ok()?}))}).collect()
    } else {
        serde_json::from_str::<Vec<Value>>(&output).map_err(|_|"zellij-too-old".to_string())?.iter().filter(|p|p["is_plugin"]==false&&p["id"].is_number()).map(|p|json!({"id":format!("terminal_{}",p["id"]),"tabId":p["tab_id"].to_string(),"tabName":p["tab_name"],"command":p.get("terminal_command").unwrap_or(&p["title"]),"left":p["pane_x"],"top":p["pane_y"],"width":p["pane_columns"],"height":p["pane_rows"]})).collect()
    };
    if panes.is_empty() {
        return Err("session-not-running".into());
    }
    let mut tabs: Vec<Value> = Vec::new();
    for pane in panes {
        if let Some(tab) = tabs.iter_mut().find(|t| t["id"] == pane["tabId"]) {
            tab["panes"].as_array_mut().unwrap().push(pane);
        } else {
            tabs.push(json!({"id":pane["tabId"],"name":pane["tabName"],"panes":[pane]}));
        }
    }
    Ok(json!({"taskId":id,"sessionType":t.kind,"canResize":t.kind=="tmux","tabs":tabs}))
}
pub fn verify_pane(app: &AppHandle, task: &str, pane: &str) -> Result<MirrorTarget, String> {
    let surfaces = surfaces_for_target(app, task, target(app, task)?)?;
    let found = surfaces["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["panes"].as_array().into_iter().flatten())
        .any(|p| text(p, "id") == pane);
    if !found {
        return Err("session-not-running".into());
    }
    target(app, task)
}
pub fn snapshot_verified(target: &MirrorTarget, pane: &str) -> Result<String, String> {
    let command = if target.kind == "tmux" {
        format!("tmux capture-pane -p -e -t {pane} && tmux display-message -p -t {pane} '#{{cursor_x}} #{{cursor_y}}'",pane=q(pane))
    } else {
        if pane.trim_start_matches("terminal_").parse::<u64>().is_err() {
            return Err("session-not-running".into());
        }
        format!(
            "zellij --session {} action dump-screen -p {} -a",
            q(&target.name),
            q(pane)
        )
    };
    let output = crate::git::run(&command, target.host.as_deref())?;
    if target.kind == "tmux" {
        let output = output.trim_end_matches('\n');
        if let Some((screen, cursor)) = output.rsplit_once('\n') {
            let pos: Vec<_> = cursor
                .split_whitespace()
                .filter_map(|s| s.parse::<u16>().ok())
                .collect();
            if pos.len() == 2 {
                return Ok(format!(
                    "\x1b[H\x1b[2J\x1b[3J{}\x1b[{};{}H",
                    screen.replace('\n', "\r\n"),
                    pos[1] + 1,
                    pos[0] + 1
                ));
            }
        }
    }
    Ok(format!(
        "\x1b[H\x1b[2J\x1b[3J{}",
        output.replace('\n', "\r\n")
    ))
}
pub fn write_pane_verified(
    app: &AppHandle,
    task: &str,
    pane: &str,
    input: &str,
    target: &MirrorTarget,
) -> Result<(), String> {
    if target.kind == "terminal" {
        return crate::terminal::write(app, task, Some(pane), input);
    }
    if input.is_empty() {
        return Ok(());
    }
    let command = if target.kind == "tmux" {
        let hex = input
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        format!("tmux send-keys -t {} -H {hex}", q(pane))
    } else {
        if pane.trim_start_matches("terminal_").parse::<u64>().is_err() {
            return Err("session-not-running".into());
        }
        format!(
            "zellij --session {} action write -p {} {}",
            q(&target.name),
            q(pane),
            input
                .as_bytes()
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    crate::git::run(&command, target.host.as_deref()).map(|_| ())
}
pub fn mobile_tab(app: &AppHandle, id: &str) -> Result<Value, String> {
    let before = surfaces_for_target(app, id, target(app, id)?)?;
    let prior: HashSet<_> = before["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| text(t, "id").to_owned())
        .collect();
    let result = crate::terminal::tabs_dispatch(app, "createTerminalTab", vec![json!(id)])?;
    if result["ok"] == false {
        return Err(text(&result, "error").into());
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let after = surfaces_for_target(app, id, target(app, id)?)?;
        if let Some(pane) = after["tabs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| !prior.contains(text(t, "id")))
            .and_then(|t| t["panes"].as_array())
            .and_then(|p| p.first())
        {
            return Ok(json!({"paneId":pane["id"],"surfaces":after}));
        }
        if Instant::now() > deadline {
            return Err("추가한 터미널 탭을 찾을 수 없습니다".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
#[derive(Default)]
struct ReleaseState {
    cached: Option<(Instant, Value)>,
    shown: HashSet<String>,
}
static RELEASE: OnceLock<Mutex<ReleaseState>> = OnceLock::new();
fn release_version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim_start_matches('v')).ok()
}
fn select_release(releases: &Value, current: &str) -> Option<Value> {
    let current = release_version(current)?;
    releases.as_array()?.iter().filter(|r|r["draft"]!=true&&r["prerelease"]!=true).filter_map(|r|{let tag=text(r,"tag_name");let version=release_version(tag)?;if version<=current||text(r,"html_url").is_empty(){return None}Some((version,json!({"version":tag.trim_start_matches('v'),"tagName":tag,"name":if text(r,"name").trim().is_empty(){tag}else{text(r,"name").trim()},"body":text(r,"body").trim(),"htmlUrl":r["html_url"],"publishedAt":r.get("published_at").unwrap_or(&Value::Null)})))}).max_by(|a,b|a.0.cmp(&b.0)).map(|(_,r)|r)
}
fn release_check(app: &AppHandle) -> Result<Value, String> {
    let mut state = RELEASE
        .get_or_init(|| Mutex::new(ReleaseState::default()))
        .lock()
        .map_err(|e| e.to_string())?;
    let mut result = if let Some((time, cached)) = &state.cached {
        if time.elapsed() < Duration::from_secs(3600) {
            cached.clone()
        } else {
            Value::Null
        }
    } else {
        Value::Null
    };
    if result.is_null() {
        let version = app.package_info().version.to_string();
        let fetched = client()?
            .get("https://api.github.com/repos/rookedsysc/kanvibe/releases")
            .header("Accept", "application/vnd.github+json")
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|r| r.json::<Value>());
        result = match fetched {
            Ok(releases) if releases.is_array() => {
                let release = select_release(&releases, &version);
                json!({"currentVersion":version,"isUpdateAvailable":release.is_some(),"release":release})
            }
            Ok(_) => {
                json!({"currentVersion":version,"isUpdateAvailable":false,"release":null,"error":"GitHub releases response was not an array"})
            }
            Err(e) => {
                json!({"currentVersion":version,"isUpdateAvailable":false,"release":null,"error":e.to_string()})
            }
        };
        if result.get("error").is_none() {
            state.cached = Some((Instant::now(), result.clone()));
        }
    }
    if result["release"]["version"]
        .as_str()
        .is_some_and(|v| state.shown.contains(v))
    {
        result["release"] = Value::Null;
        result["isUpdateAvailable"] = json!(false);
    }
    Ok(result)
}
static PULL_FAILURES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
fn pull_tasks(app: &AppHandle) -> Result<Vec<Value>, String> {
    let db = app.state::<crate::database::Database>();
    let mut tasks = db.tasks()?;
    tasks.sort_by(|a, b| text(a, "updatedAt").cmp(text(b, "updatedAt")));
    let mut results = Vec::new();
    for task in tasks {
        if !["todo", "progress", "pending", "review"].contains(&text(&task, "status"))
            || text(&task, "branchName").is_empty()
            || text(&task, "worktreePath").is_empty()
        {
            continue;
        }
        let project = db.project(text(&task, "projectId"))?.unwrap_or(Value::Null);
        if task["branchName"] == project["defaultBranch"] {
            continue;
        }
        let path = text(&task, "worktreePath");
        let branch = text(&task, "branchName");
        let host = task["sshHost"]
            .as_str()
            .or_else(|| project["sshHost"].as_str());
        let key = format!(
            "{}:{branch}:{path}:{}",
            text(&task, "id"),
            host.unwrap_or("")
        );
        let operation = (|| -> Result<Option<String>, String> {
            let probe=crate::git::run(&format!("git -C {} ls-remote --exit-code --heads origin {} >/dev/null; result=$?; if [ \"$result\" -eq 0 ]; then printf exists; elif [ \"$result\" -eq 2 ]; then printf missing; else exit \"$result\"; fi",q(path),q(&format!("refs/heads/{branch}"))),host)?;
            if probe.trim() != "exists" {
                return Ok(None);
            }
            let output = crate::git::git(path, "pull --ff-only", host)?;
            if output.to_lowercase().contains("already up to date")
                || output.to_lowercase().contains("already up-to-date")
            {
                Ok(None)
            } else {
                Ok(Some(output))
            }
        })();
        match operation {
            Ok(output) => {
                PULL_FAILURES
                    .get_or_init(|| Mutex::new(HashSet::new()))
                    .lock()
                    .unwrap()
                    .remove(&key);
                if let Some(output) = output {
                    results.push(json!({"taskId":task["id"],"taskTitle":task["title"],"branchName":branch,"worktreePath":path,"sshHost":host,"status":"updated","summary":output.trim()}));
                }
            }
            Err(error) => {
                if !error.contains("couldn't find remote ref")
                    && PULL_FAILURES
                        .get_or_init(|| Mutex::new(HashSet::new()))
                        .lock()
                        .unwrap()
                        .insert(key)
                {
                    results.push(json!({"taskId":task["id"],"taskTitle":task["title"],"branchName":branch,"worktreePath":path,"sshHost":host,"status":"failed","summary":error}));
                }
            }
        }
    }
    Ok(results)
}
static SYNC: OnceLock<Mutex<()>> = OnceLock::new();
fn sync_now(app: &AppHandle) -> Result<Value, String> {
    let _guard = SYNC
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|e| e.to_string())?;
    let worktrees = crate::core::sync_worktrees(app);
    let prs = crate::core::sync_pull_requests(app);
    let mut failures = Vec::new();
    let w = match worktrees {
        Ok(v) => v,
        Err(e) => {
            failures.push(json!({"operation":"worktree-sync","target":"등록 프로젝트 worktree sync","reason":e}));
            json!({})
        }
    };
    let p = match prs {
        Ok(v) => v,
        Err(e) => {
            failures
                .push(json!({"operation":"pull-request-sync","target":"PR 상태 sync","reason":e}));
            json!({})
        }
    };
    for error in w["errors"].as_array().into_iter().flatten() {
        failures.push(json!({"operation":"worktree-sync","target":"등록 프로젝트 worktree sync","reason":error}));
    }
    for failure in p["failures"].as_array().into_iter().flatten() {
        failures.push(failure.clone());
    }
    let pulled = match pull_tasks(app) {
        Ok(v) => v,
        Err(error) => {
            failures.push(json!({"operation":"task-pull-sync","target":"active task pull sync","reason":error}));
            vec![]
        }
    };
    let updated = pulled.iter().any(|p| p["status"] == "updated")
        || w["changed"] == true
        || p["updatedTaskIds"]
            .as_array()
            .is_some_and(|a| !a.is_empty());
    let review = w["registeredWorktrees"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
        || p["mergedPullRequests"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        || !pulled.is_empty()
        || !failures.is_empty();
    if review {
        let event = json!({"type":"background-sync-review-needed","registeredWorktrees":w.get("registeredWorktrees").unwrap_or(&json!([])),"mergedPullRequests":p.get("mergedPullRequests").unwrap_or(&json!([])),"pulledTasks":pulled,"failures":failures});
        app.emit("kanvibe:board-event", event)
            .map_err(|e| e.to_string())?;
    }
    if updated {
        app.emit("kanvibe:board-event", json!({"type":"board-updated"}))
            .map_err(|e| e.to_string())?;
    }
    Ok(json!({"reviewNeeded":review,"boardUpdated":updated}))
}
const INITIAL_BACKGROUND_SYNC_DELAY: Duration = Duration::from_secs(20);
/// 백그라운드 sync 스레드를 깨운 설정 변경. 켜짐 변경은 첫 지연부터, 주기 변경은 새 주기로 다시 예약한다.
#[derive(Clone, Copy)]
pub enum BackgroundSyncWake {
    EnabledChanged,
    IntervalChanged,
}
static BACKGROUND: OnceLock<(Mutex<Option<BackgroundSyncWake>>, std::sync::Condvar)> =
    OnceLock::new();
fn background_signal() -> &'static (Mutex<Option<BackgroundSyncWake>>, std::sync::Condvar) {
    BACKGROUND.get_or_init(|| (Mutex::new(None), std::sync::Condvar::new()))
}
pub fn wake_background(reason: BackgroundSyncWake) {
    let (pending, wake) = background_signal();
    *pending.lock().unwrap() = Some(reason);
    wake.notify_all();
}
/// 자동 sync는 사용자가 명시적으로 켠 경우에만 동작한다.
fn background_sync_enabled(app: &AppHandle) -> bool {
    setting(app, "background_sync_enabled")
        .ok()
        .flatten()
        .as_deref()
        == Some("true")
}
fn background_sync_interval(app: &AppHandle) -> Duration {
    Duration::from_millis(
        setting(app, "background_sync_interval_ms")
            .ok()
            .flatten()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(600000)
            .max(1000),
    )
}
/// 꺼진 동안에는 타이머 없이 설정 변경 신호만 기다리고, 켜져 있을 때만 다음 sync를 예약한다.
fn next_background_delay(
    is_enabled: bool,
    wake: Option<BackgroundSyncWake>,
    interval: Duration,
) -> Option<Duration> {
    if !is_enabled {
        return None;
    }
    match wake {
        Some(BackgroundSyncWake::EnabledChanged) => Some(INITIAL_BACKGROUND_SYNC_DELAY),
        Some(BackgroundSyncWake::IntervalChanged) | None => Some(interval),
    }
}
pub fn start_background(app: AppHandle) {
    std::thread::spawn(move || {
        let (pending, wake) = background_signal();
        let mut delay = background_sync_enabled(&app).then_some(INITIAL_BACKGROUND_SYNC_DELAY);
        loop {
            let guard = pending.lock().unwrap();
            let (mut guard, timed_out) = match delay {
                Some(duration) => {
                    let (guard, timeout) = wake
                        .wait_timeout_while(guard, duration, |p| p.is_none())
                        .unwrap();
                    (guard, timeout.timed_out())
                }
                None => (wake.wait_while(guard, |p| p.is_none()).unwrap(), false),
            };
            let reason = guard.take();
            drop(guard);
            let is_enabled = background_sync_enabled(&app);
            if !timed_out || !is_enabled {
                delay = next_background_delay(is_enabled, reason, background_sync_interval(&app));
                continue;
            }
            if let Err(error) = sync_now(&app) {
                eprintln!("Background sync failed: {error}");
            }
            // 실행 중에 온 설정 변경은 아래에서 최신 설정을 다시 읽어 반영하므로 신호는 버린다.
            pending.lock().unwrap().take();
            delay = next_background_delay(
                background_sync_enabled(&app),
                None,
                background_sync_interval(&app),
            );
        }
    });
}
pub fn dispatch(
    app: &AppHandle,
    namespace: &str,
    method: &str,
    args: Vec<Value>,
) -> Result<Value, String> {
    match(namespace,method){("mobileBridge","getMobileBoard")=>mobile_board(app),("mobileBridge","startMobilePairing")=>Ok(issue_code()),("mobileBridge","stopMobilePairing")=>{pairing().lock().map_err(|e|e.to_string())?.code=None;Ok(Value::Null)},("mobileBridge","readMobilePairingCode")=>{let p=pairing().lock().map_err(|e|e.to_string())?;Ok(if p.expires>Utc::now().timestamp_millis(){json!(p.code)}else{Value::Null})},("mobileBridge","listPairedMobileDevices")=>Ok(json!(read_devices(app)?.iter().map(|d|json!({"deviceId":d["deviceId"],"deviceName":d["deviceName"],"pairedAt":d["pairedAt"]})).collect::<Vec<_>>())),("mobileBridge","unpairMobileDevice")=>{unpair(app,args.first().and_then(Value::as_str).unwrap_or(""))?;Ok(Value::Null)},("releaseUpdates","checkForReleaseUpdate")=>release_check(app),("releaseUpdates","claimReleaseUpdateVersion")=>{let version=args.first().and_then(Value::as_str).unwrap_or("");Ok(json!(!version.is_empty()&&RELEASE.get_or_init(||Mutex::new(ReleaseState::default())).lock().map_err(|e|e.to_string())?.shown.insert(version.into())))},("backgroundTaskSync","runBackgroundTaskSyncNow")=>sync_now(app),_=>Err(format!("Unknown integration {namespace}.{method}"))}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_background_sync_waits_without_timer() {
        let interval = Duration::from_secs(60);
        assert_eq!(next_background_delay(false, None, interval), None);
        assert_eq!(
            next_background_delay(false, Some(BackgroundSyncWake::IntervalChanged), interval),
            None
        );
        assert_eq!(
            next_background_delay(true, Some(BackgroundSyncWake::EnabledChanged), interval),
            Some(INITIAL_BACKGROUND_SYNC_DELAY)
        );
        assert_eq!(
            next_background_delay(true, Some(BackgroundSyncWake::IntervalChanged), interval),
            Some(interval)
        );
        assert_eq!(next_background_delay(true, None, interval), Some(interval));
    }
    #[test]
    fn pairing_is_single_use_and_limited() {
        let mut p = Pairing {
            code: Some("012345".into()),
            expires: 100,
            failures: 0,
        };
        assert!(redeem(&mut p, "012345", 99));
        assert!(!redeem(&mut p, "012345", 99));
        let mut p = Pairing {
            code: Some("012345".into()),
            expires: 100,
            failures: 0,
        };
        for _ in 0..5 {
            assert!(!redeem(&mut p, "x", 99));
        }
        assert!(!redeem(&mut p, "012345", 99));
    }
    #[test]
    fn expiry_blocks_pairing() {
        let mut p = Pairing {
            code: Some("123456".into()),
            expires: 100,
            failures: 0,
        };
        assert!(!redeem(&mut p, "123456", 100));
    }
    #[test]
    fn subscription_rejects_overlong_byte_id() {
        assert!(!valid_subscription(
            &json!({"pushToken":"x","platform":"ios","connectionId":"가".repeat(50)})
        ));
        assert!(valid_subscription(
            &json!({"pushToken":"x","platform":"android","connectionId":"a"})
        ));
    }
    #[test]
    fn release_version_order_is_numeric() {
        let release=select_release(&json!([{"tag_name":"v1.9.0","html_url":"https://x"},{"tag_name":"v1.10.0","html_url":"https://y"},{"tag_name":"v2.0.0-beta.1","html_url":"https://z","prerelease":true}]),"1.0.0").unwrap();
        assert_eq!(release["version"], "1.10.0");
    }
}
#[cfg(test)]
mod notification_regressions {
    use super::*;
    #[test]
    fn mobile_notification_text_matches_existing_korean_contract() {
        let event = mobile_notification(
            &json!({"taskId":"task","projectName":"KanVibe","branchName":"main","taskTitle":"fix terminal","newStatus":"review","description":"ready"}),
        );
        assert_eq!(event["title"], "KanVibe — main");
        assert_eq!(event["body"], "fix terminal: review로 변경\nready");
    }
    #[test]
    fn unicode_and_json_escape_notification_truncation_respects_bytes() {
        let text = "한글\"\n".repeat(200);
        let result = truncate_encoded(&text, 128);
        assert!(serde_json::to_string(&result).unwrap().len() - 2 <= 128);
        assert!(result.ends_with('…'));
    }
}

#[cfg(test)]
mod session_connection_regressions {
    use super::*;
    #[test]
    fn mobile_surfaces_recovers_each_missing_session_field_only_for_connectable_tasks() {
        for (kind, name) in [
            (Value::Null, json!("session")),
            (json!("terminal"), Value::Null),
            (Value::Null, Value::Null),
        ] {
            let mut task = json!({"projectId":"project", "branchName":"feature", "sessionType":kind,"sessionName":name});
            assert!(needs_session_connection(&task));
            task["projectId"] = Value::Null;
            assert!(!needs_session_connection(&task));
            task["projectId"] = json!("project");
            task["branchName"] = Value::Null;
            assert!(!needs_session_connection(&task));
            task["baseBranch"] = json!("main");
            assert!(needs_session_connection(&task));
        }
        assert!(!needs_session_connection(
            &json!({"projectId":"p","branchName":"main","sessionType":"tmux","sessionName":"existing"})
        ));
    }
}
