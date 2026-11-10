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
    json!({"type":"task-status-changed","eventId":uuid::Uuid::new_v4().to_string(),"taskId":event["taskId"],"title":truncate_encoded(&title,128),"body":truncate_encoded(&body,700),"status":status,"createdAt":now(),"shouldNotify":false})
}
/// 설정을 읽지 못해도 보드 동기화는 이어져야 하므로 푸시만 막고 이벤트는 그대로 내보낸다.
pub fn publish_status_event(
    mut mobile: Value,
    settings: Result<Value, String>,
    broadcast: impl FnOnce(&Value),
    push: impl FnOnce(Value),
) {
    let should_notify = match settings {
        Ok(settings) => {
            settings["isEnabled"] == true
                && settings["enabledStatuses"]
                    .as_array()
                    .is_some_and(|statuses| statuses.contains(&mobile["status"]))
        }
        Err(_) => {
            eprintln!("[kanvibe] Mobile notification settings could not be read");
            false
        }
    };
    mobile["shouldNotify"] = json!(should_notify);
    broadcast(&mobile);
    if should_notify {
        push(mobile);
    }
}
fn push_data(event: &Value, connection_id: &Value) -> Value {
    let mut data = event.clone();
    if let Some(fields) = data.as_object_mut() {
        fields.remove("shouldNotify");
    }
    data["connectionId"] = connection_id.clone();
    data
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
            let data = push_data(&event, &sub["connectionId"]);
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
const NOTIFICATION_STATUSES: [&str; 5] = ["todo", "progress", "pending", "review", "done"];
/// 저장 전에 전체를 검증해 잘못된 값이 설정 일부만 바꾸지 않게 한다.
fn notification_settings_input(input: &Value) -> Option<(bool, Vec<&str>)> {
    let is_enabled = input["isEnabled"].as_bool()?;
    let mut statuses = Vec::new();
    for status in input["enabledStatuses"].as_array()? {
        let status = status
            .as_str()
            .filter(|status| NOTIFICATION_STATUSES.contains(status))?;
        if !statuses.contains(&status) {
            statuses.push(status);
        }
    }
    Some((is_enabled, statuses))
}
pub fn notification_settings(app: &AppHandle) -> Result<Value, String> {
    call(app, "appSettings", "getNotificationSettings", vec![])
}
/// 입력이 올바르지 않으면 저장하지 않고 None을 돌려준다.
pub fn update_notification_settings(
    app: &AppHandle,
    input: &Value,
) -> Result<Option<Value>, String> {
    let Some((is_enabled, statuses)) = notification_settings_input(input) else {
        return Ok(None);
    };
    call(
        app,
        "appSettings",
        "setNotificationStatuses",
        vec![json!(statuses)],
    )?;
    call(
        app,
        "appSettings",
        "setNotificationEnabled",
        vec![json!(is_enabled)],
    )?;
    notification_settings(app).map(Some)
}
pub fn mobile_task(app: &AppHandle, id: &str) -> Result<Value, String> {
    let mut task = call(app, "kanban", "getTaskById", vec![json!(id)])?;
    if task.is_object() {
        task["projectName"] = task["project"]["name"].clone();
    }
    Ok(task)
}
fn validate_mobile_draft(draft: &Value) -> Result<(), String> {
    match &draft["priority"] {
        Value::Null | Value::Bool(false) => {},
        Value::Number(number) if number.as_f64()==Some(0.0) => {},
        Value::String(priority) if priority.is_empty() || ["low","medium","high"].contains(&priority.as_str()) => {},
        _ => return Err("우선순위 값이 올바르지 않습니다".into()),
    }
    for (key, label) in [
        ("branchName", "브랜치 이름"),
        ("baseBranch", "베이스 브랜치"),
    ] {
        if let Some(branch) = draft[key].as_str().filter(|branch| !branch.is_empty()) {
            if !crate::git::is_safe_git_ref(branch) {
                return Err(format!("{label}에 쓸 수 없는 문자가 있습니다"));
            }
        }
    }
    Ok(())
}
fn mobile_creation_input(draft: &Value, default_kind: &Value) -> Result<Value, String> {
    let mut input = json!({"title":draft["branchName"],"branchName":draft["branchName"],"projectId":draft["projectId"],"sessionType":default_kind});
    for field in ["baseBranch", "description", "priority"] {
        if let Some(value) = draft.get(field) {
            input[field] = value.clone();
        }
    }
    validate_mobile_draft(&input)?;
    Ok(input)
}
pub fn mobile_create(app: &AppHandle, draft: &Value) -> Result<Value, String> {
    let default_kind = call(app, "appSettings", "getDefaultSessionType", vec![])?;
    let input = mobile_creation_input(draft, &default_kind)?;
    let created = call(app, "kanban", "createTask", vec![input])?;
    if !text(&created, "projectId").is_empty()
        && !text(&created, "branchName").is_empty()
        && !text(&created, "worktreePath").is_empty()
    {
        if let Some(project) = app
            .state::<crate::database::Database>()
            .project(text(&created, "projectId"))?
        {
            let peers = crate::task_targets::propagate_registration(
                text(&project, "repoPath"),
                text(&created, "branchName"),
                project["sshHost"].as_str(),
            );
            if peers.iter().any(|peer| peer["isSuccess"] != true) {
                eprintln!(
                    "[mobile] 다른 데스크탑에 task를 등록하지 못했습니다: {}",
                    created["id"]
                );
            }
        }
    }
    Ok(created)
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
    if t.kind == "herdr" {
        let panes = match crate::herdr::mirror_panes(
            &t.name,
            t.host.as_deref(),
            &crate::herdr::system_shell,
        ) {
            Ok(panes) => panes,
            Err(_) => {
                let dependency = crate::dependencies::dispatch(
                    "sessionDependency",
                    "getSessionDependencyStatus",
                    vec![json!("herdr"), json!(t.host)],
                )?;
                if dependency["available"] != true {
                    return Err("herdr-missing".into());
                }
                let result = (|| {
                    crate::terminal::open(app, "mobile:bootstrap", id, None, 80, 24)
                        .map_err(|_| "session-not-running".to_string())?;
                    crate::herdr::mirror_panes(
                        &t.name,
                        t.host.as_deref(),
                        &crate::herdr::system_shell,
                    )
                    .map_err(|_| "session-not-running".to_string())
                })();
                let _ = crate::terminal::detach(app, "mobile:bootstrap", id, None);
                result?
            }
        };
        return group_mirror_panes(id, &t, panes);
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
    group_mirror_panes(id, &t, panes)
}
fn group_mirror_panes(id: &str, target: &MirrorTarget, panes: Vec<Value>) -> Result<Value, String> {
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
    Ok(json!({"taskId":id,"sessionType":target.kind,"canResize":target.kind=="tmux","tabs":tabs}))
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
const MAX_MIRROR_LINES: usize = 2000;
/// 화면과 스크롤백을 지우고 커서를 맨 위로 보낸다. 덤프를 받을 때마다 이어 붙이지 않고 다시 그리기 위해서다.
const CLEAR_SCREEN_AND_SCROLLBACK: &str = "\x1b[H\x1b[2J\x1b[3J";
/// pane 보이는 영역 안의 칸 좌표(0부터)와 그 영역의 높이.
#[derive(Debug, PartialEq)]
struct PaneCursor {
    x: u32,
    y: u32,
    pane_height: u32,
}
#[derive(Debug, PartialEq)]
struct TmuxPaneState {
    cursor: PaneCursor,
    restore_sequences: String,
}
fn tmux_capture_command(pane: &str) -> String {
    format!(
        "tmux capture-pane -pe -S -{MAX_MIRROR_LINES} -t {}",
        q(pane)
    )
}
/// 커서와 접속 전에 활성화된 화면·마우스 모드를 함께 읽는다.
fn tmux_pane_state_command(pane: &str) -> String {
    format!("tmux display-message -p -t {} '#{{cursor_x}} #{{cursor_y}} #{{pane_height}} #{{alternate_on}} #{{mouse_standard_flag}} #{{mouse_button_flag}} #{{mouse_all_flag}} #{{mouse_sgr_flag}} #{{mouse_utf8_flag}}'",q(pane))
}
fn parse_tmux_pane_state(output: &str) -> Option<TmuxPaneState> {
    let captures =
        regex::Regex::new(r"^(\d+) (\d+) (\d+) ([01]) ([01]) ([01]) ([01]) ([01]) ([01])$")
            .unwrap()
            .captures(output.trim())?;
    let is_on = |index: usize| &captures[index] == "1";
    let mouse_mode = if is_on(7) {
        Some(1003)
    } else if is_on(6) {
        Some(1002)
    } else if is_on(5) {
        Some(1000)
    } else {
        None
    };
    let mouse_encoding = if is_on(8) {
        Some(1006)
    } else if is_on(9) {
        Some(1005)
    } else {
        None
    };
    let mut restore_sequences = format!(
        "\x1b[?1049{}\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l",
        if is_on(4) { "h" } else { "l" }
    );
    for mode in [mouse_mode, mouse_encoding].into_iter().flatten() {
        restore_sequences.push_str(&format!("\x1b[?{mode}h"));
    }
    Some(TmuxPaneState {
        cursor: PaneCursor {
            x: captures[1].parse().ok()?,
            y: captures[2].parse().ok()?,
            pane_height: captures[3].parse().ok()?,
        },
        restore_sequences,
    })
}
/// 덤프 앞에는 스크롤백이 붙고 모바일 화면 높이도 pane과 다를 수 있어 절대 좌표로는 커서 줄을 알 수 없다.
/// 덤프의 마지막 줄은 언제나 pane 보이는 영역의 맨 아랫줄이므로, 다 쓴 뒤 그 줄에서 위로 올라가 커서 줄을 찾는다.
fn cursor_move_from_last_line(cursor: &PaneCursor) -> String {
    let rows_above_last_line = i64::from(cursor.pane_height) - 1 - i64::from(cursor.y);
    let move_up = if rows_above_last_line > 0 {
        format!("\x1b[{rows_above_last_line}A")
    } else {
        String::new()
    };
    format!("{move_up}\x1b[{}G", cursor.x + 1)
}
/// PTY가 아니면 줄바꿈이 줄 처음으로 돌아가지 않아 화면이 계단처럼 밀리므로 줄마다 CR을 붙인다.
fn build_screen_redraw(screen: &str, cursor: Option<&PaneCursor>) -> String {
    let lines: Vec<&str> = screen
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let kept_lines = &lines[lines.len().saturating_sub(MAX_MIRROR_LINES)..];
    format!(
        "{CLEAR_SCREEN_AND_SCROLLBACK}{}{}",
        kept_lines.join("\r\n"),
        cursor.map(cursor_move_from_last_line).unwrap_or_default()
    )
}
fn tmux_snapshot(output: &str) -> String {
    let parsed = output.rfind('\n').and_then(|line_start| {
        parse_tmux_pane_state(&output[line_start + 1..]).map(|state| (&output[..line_start], state))
    });
    match parsed {
        Some((screen, state)) => format!(
            "{}{}",
            state.restore_sequences,
            build_screen_redraw(screen, Some(&state.cursor))
        ),
        None => build_screen_redraw(output, None),
    }
}
/// 원격 전송량과 실행 버퍼를 제한하되 dump 실패를 tail의 성공으로 덮지 않는다.
fn zellij_dump_command(session: &str, pane: &str) -> String {
    format!(
        "kanvibe_mobile_dump=$(zellij --session {} action dump-screen -p {} -a --full) && printf '%s\\n' \"$kanvibe_mobile_dump\" | tail -n {MAX_MIRROR_LINES}",
        q(session),
        q(pane)
    )
}
pub fn snapshot_verified(target: &MirrorTarget, pane: &str) -> Result<String, String> {
    if target.kind == "tmux" {
        // 화면과 커서를 한 번에 읽어 원격 왕복을 줄이고 둘 사이에 화면이 바뀌는 틈도 좁힌다.
        let command = format!(
            "{} && {}",
            tmux_capture_command(pane),
            tmux_pane_state_command(pane)
        );
        let output = crate::git::run(&command, target.host.as_deref())?;
        return Ok(tmux_snapshot(output.trim()));
    }
    if target.kind == "herdr" {
        let command = crate::herdr::recent_screen_command(&target.name, pane);
        let output = crate::git::run(&command, target.host.as_deref())?;
        return Ok(build_screen_redraw(&output, None));
    }
    if pane.trim_start_matches("terminal_").parse::<u64>().is_err() {
        return Err("session-not-running".into());
    }
    let command = zellij_dump_command(&target.name, pane);
    let output = crate::git::run(&command, target.host.as_deref())?;
    Ok(build_screen_redraw(output.trim(), None))
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
    } else if target.kind == "herdr" {
        crate::herdr::write_pane_command(&target.name, pane, input)
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
fn pane_shortcut_command(
    kind: &str,
    session: &str,
    pane: &str,
    tab: &str,
    action: &str,
) -> Option<String> {
    let target = q(pane);
    if kind == "herdr" {
        return crate::herdr::pane_shortcut_command(session, pane, action);
    }
    if kind == "tmux" {
        return Some(match action {
            "split-right" => format!("tmux split-window -h -d -t {target}"),
            "split-down" => format!("tmux split-window -v -d -t {target}"),
            "toggle-fullscreen" => format!("tmux resize-pane -Z -t {target}"),
            "copy-mode" => format!("tmux copy-mode -t {target}"),
            "scroll-up" => {
                format!("tmux copy-mode -t {target} && tmux send-keys -X -t {target} page-up")
            }
            "scroll-down" => {
                format!("tmux copy-mode -t {target} && tmux send-keys -X -t {target} page-down")
            }
            "scroll-bottom" => {
                format!("tmux copy-mode -t {target} && tmux send-keys -X -t {target} cancel")
            }
            _ => return None,
        });
    }
    if kind != "zellij" {
        return None;
    }
    let prefix = format!("zellij --session {} action", q(session));
    if matches!(action, "split-right" | "split-down") {
        let direction = if action == "split-right" {
            "right"
        } else {
            "down"
        };
        return Some(format!(
            "{prefix} new-pane --direction {direction} --tab-id {} --no-focus",
            q(tab)
        ));
    }
    let command = match action {
        "toggle-fullscreen" => "toggle-fullscreen",
        "toggle-floating" => "toggle-pane-embed-or-floating",
        "scroll-up" => "page-scroll-up",
        "scroll-down" => "page-scroll-down",
        "scroll-bottom" => "scroll-to-bottom",
        _ => return None,
    };
    Some(format!("{prefix} {command} --pane-id {target}"))
}
pub fn mobile_shortcut(
    app: &AppHandle,
    task: &str,
    pane_id: &str,
    action: &str,
) -> Result<Value, String> {
    if pane_shortcut_command("tmux", "", pane_id, "", action).is_none()
        && pane_shortcut_command("zellij", "", pane_id, "", action).is_none()
    {
        return Err("지원하지 않는 단축키입니다".into());
    }
    let target = target(app, task)?;
    if target.kind == "terminal" {
        return Err("일반 터미널에서는 세션 제어 단축키를 사용할 수 없습니다".into());
    }
    let before = surfaces_for_target(app, task, target.clone())?;
    let panes: Vec<_> = before["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|tab| tab["panes"].as_array().into_iter().flatten())
        .collect();
    let pane = panes
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .ok_or("session-not-running")?;
    let command = pane_shortcut_command(
        &target.kind,
        &target.name,
        pane_id,
        text(pane, "tabId"),
        action,
    )
    .ok_or("이 세션에서 지원하지 않는 단축키입니다")?;
    crate::git::run(&command, target.host.as_deref())?;
    let mut result = json!({});
    if target.kind == "tmux" && (action == "copy-mode" || action.starts_with("scroll-")) {
        // pipe-pane에는 복사 모드 화면이 나오지 않아 해당 화면을 별도로 돌려준다.
        let capture = if action == "scroll-bottom" {
            tmux_capture_command(pane_id)
        } else {
            format!("tmux capture-pane -peM -t {}", q(pane_id))
        };
        result["screen"] = json!(build_screen_redraw(
            &crate::git::run(&capture, target.host.as_deref())?,
            None
        ));
    }
    let prior: HashSet<_> = panes
        .iter()
        .filter_map(|pane| pane["id"].as_str())
        .collect();
    let tab = text(pane, "tabId");
    let split = matches!(action, "split-right" | "split-down");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let surfaces = surfaces(app, task)?;
        let created = surfaces["tabs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["id"] == tab)
            .flat_map(|entry| entry["panes"].as_array().into_iter().flatten())
            .any(|pane| !prior.contains(text(pane, "id")));
        if !split || created {
            result["surfaces"] = surfaces;
            return Ok(result);
        }
        if Instant::now() >= deadline {
            return Err("추가한 pane을 찾을 수 없습니다.".into());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub const SUPPORTED_MOBILE_COMMANDS: [&str; 3] = ["delete-task", "get-tab-close-info", "close-tab"];
fn command_failure(request_id: &Value, code: &str, message: &str) -> Value {
    json!({"type":"command-result","requestId":request_id,"ok":false,"code":code,"error":message})
}
fn mobile_command_error(request_id: &Value, error: &str) -> Value {
    let (code, message) = if error.starts_with("커밋하지 않은 변경이 있어 삭제하지 않았습니다:")
    {
        ("dirty-worktree", "커밋하지 않은 변경이 있습니다")
    } else if let Some(message) = error.strip_prefix("PaneNotFoundError:") {
        ("pane-not-found", message)
    } else if let Some(message) = error.strip_prefix("LastPaneError:") {
        ("last-tab", message)
    } else if error == "task-not-found" {
        ("task-not-found", "없는 태스크입니다")
    } else if [
        "no-session",
        "session-not-running",
        "zellij-too-old",
        "herdr-missing",
        "remote-unreachable",
    ]
    .contains(&error)
    {
        ("session-unavailable", "세션을 사용할 수 없습니다")
    } else {
        eprintln!("[kanvibe] Mobile command failed: {error}");
        ("server-error", "서버 오류")
    };
    command_failure(request_id, code, message)
}
fn run_mobile_command_with(
    raw: &[u8],
    execute: impl FnOnce(&str, &Value) -> Result<Value, String>,
) -> Option<Value> {
    if raw.len() > 1024 {
        return None;
    }
    let message: Value = serde_json::from_slice(raw).ok()?;
    if !message.is_object() || message["type"] != "command" {
        return None;
    }
    let id = message["requestId"].as_str()?;
    if id.is_empty() || id.encode_utf16().count() > 64 {
        return None;
    }
    let request_id = &message["requestId"];
    let Some(command) = message["command"].as_str() else {
        return Some(command_failure(
            request_id,
            "invalid-command",
            "명령 형식이 올바르지 않습니다",
        ));
    };
    if !SUPPORTED_MOBILE_COMMANDS.contains(&command) {
        return Some(command_failure(
            request_id,
            "unsupported-command",
            "이 데스크탑이 지원하지 않는 명령입니다",
        ));
    }
    let required = if command == "delete-task" {
        &["taskId"][..]
    } else {
        &["taskId", "paneId"][..]
    };
    if required
        .iter()
        .any(|key| message[*key].as_str().is_none_or(str::is_empty))
        || (command == "delete-task"
            && message
                .get("isForced")
                .is_some_and(|value| !value.is_boolean()))
    {
        return Some(command_failure(
            request_id,
            "invalid-command",
            "명령 형식이 올바르지 않습니다",
        ));
    }
    Some(match execute(command, &message) {
        Ok(payload) => {
            let mut result = json!({"type":"command-result","requestId":request_id,"ok":true});
            if let Some(fields) = payload.as_object() {
                result.as_object_mut().unwrap().extend(fields.clone());
            }
            result
        }
        Err(error) => mobile_command_error(request_id, &error),
    })
}
pub fn run_mobile_command(app: &AppHandle, raw: &[u8]) -> Option<Value> {
    run_mobile_command_with(raw, |command, message| match command {
        "delete-task" => {
            let result = call(
                app,
                "kanban",
                "deleteTaskAcrossTargets",
                vec![
                    message["taskId"].clone(),
                    json!({"shouldRejectDirtyWorktree":message["isForced"]!=true,"shouldPropagateToPeers":true}),
                ],
            )?;
            if result["isDeleted"] != true {
                return Err("task-not-found".into());
            }
            let failed = result["peerResults"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|peer| peer["isSuccess"] != true)
                .count();
            Ok(json!({"failedPeerCount":failed}))
        }
        "get-tab-close-info" => {
            mobile_pane_close_info(app, text(message, "taskId"), text(message, "paneId"))
        }
        "close-tab" => close_mobile_pane(app, text(message, "taskId"), text(message, "paneId")),
        _ => unreachable!(),
    })
}
fn surface_panes(surfaces: &Value) -> Vec<&Value> {
    surfaces["tabs"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|tab| tab["panes"].as_array().into_iter().flatten())
        .collect()
}
fn mobile_pane_for_close<'a>(
    surfaces: &'a Value,
    pane_id: &str,
    should_close: bool,
) -> Result<&'a Value, String> {
    let panes = surface_panes(surfaces);
    let pane = panes
        .iter()
        .find(|pane| pane["id"] == pane_id)
        .copied()
        .ok_or("PaneNotFoundError:이미 닫혔거나 이 태스크에 없는 탭입니다")?;
    if should_close && panes.len() == 1 {
        return Err("LastPaneError:마지막 탭은 닫을 수 없습니다".into());
    }
    Ok(pane)
}
fn close_pane_command(target: &MirrorTarget, pane: &str) -> String {
    match target.kind.as_str() {
        "herdr" => crate::herdr::command(&target.name, &["pane", "close", pane]),
        "zellij" => format!(
            "zellij --session {} action close-pane --pane-id {}",
            q(&target.name),
            q(pane)
        ),
        _ => format!("tmux kill-pane -t {}", q(pane)),
    }
}
pub fn close_mobile_pane(app: &AppHandle, task_id: &str, pane_id: &str) -> Result<Value, String> {
    surfaces(app, task_id)?;
    crate::terminal::with_task_session_mutation(app, task_id, || {
        let target = target(app, task_id)?;
        let before = surfaces_for_target(app, task_id, target.clone())?;
        let pane = mobile_pane_for_close(&before, pane_id, true)?;
        if target.kind == "terminal" {
            let result = crate::terminal::tabs_dispatch(
                app,
                "closeTerminalTab",
                vec![json!(task_id), pane["tabId"].clone()],
            )?;
            if result["ok"] != true {
                return Err(text(&result, "error").to_owned());
            }
        } else {
            crate::git::run(
                &close_pane_command(&target, pane_id),
                target.host.as_deref(),
            )?;
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let after = surfaces_for_target(app, task_id, target.clone())?;
            if !surface_panes(&after)
                .iter()
                .any(|pane| pane["id"] == pane_id)
            {
                return Ok(json!({"surfaces":after}));
            }
            if Instant::now() >= deadline {
                return Err("탭이 닫히지 않았습니다.".into());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    })
}
fn mobile_running_process_names(command: Option<&str>) -> Vec<String> {
    command
        .filter(|command| {
            !command.is_empty() && !["sh", "bash", "zsh", "fish", "dash"].contains(command)
        })
        .map(|command| vec![command.to_owned()])
        .unwrap_or_default()
}
pub fn mobile_pane_close_info(
    app: &AppHandle,
    task_id: &str,
    pane_id: &str,
) -> Result<Value, String> {
    let surfaces = surfaces(app, task_id)?;
    let pane = mobile_pane_for_close(&surfaces, pane_id, false)?;
    let local = if surfaces["sessionType"] == "terminal" {
        crate::terminal::local_tab_process_name(app, task_id, text(pane, "tabId"))
    } else {
        None
    };
    let command = if surfaces["sessionType"] == "terminal" {
        local.as_deref()
    } else {
        pane["command"].as_str()
    };
    Ok(json!({"processNames":mobile_running_process_names(command)}))
}
pub fn mobile_change_session(
    app: &AppHandle,
    id: &str,
    kind: &str,
    close: bool,
) -> Result<Value, String> {
    if !["terminal", "tmux", "zellij", "herdr"].contains(&kind) {
        return Err("세션 타입 값이 올바르지 않습니다".into());
    }
    let task = mobile_task(app, id)?;
    if task.is_null() {
        return Ok(Value::Null);
    }
    let status = crate::dependencies::dispatch(
        "sessionDependency",
        "getSessionDependencyStatus",
        vec![json!(kind), task["sshHost"].clone()],
    )?;
    if status["available"] != true && !text(&status, "toolName").is_empty() {
        return Err(format!("MobileSessionDependencyMissingError:{}이(가) 설치되어 있지 않습니다. 데스크탑에서 설치한 뒤 다시 시도해 주세요.",text(&status,"toolName")));
    }
    let changed = call(
        app,
        "kanban",
        "changeTaskSessionType",
        vec![json!(id), json!(kind), json!(close)],
    )?;
    if changed.is_null() {
        return Err(format!("세션 타입 변경 실패: {id}"));
    }
    Ok(changed)
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
    if namespace=="mobileBridge" {
        match method {
            "readMobileConnectionSettings"=>{
                let port=crate::server::mobile_tls_port();
                return Ok(json!({"mobileTlsPort":port,"isLanPlaintextAllowed":call(app,"appSettings","getLanPlaintextAllowed",vec![])?}));
            }
            "updateLanPlaintextAllowed"=>return call(app,"appSettings","setLanPlaintextAllowed",args),
            _=>{},
        }
    }
    match(namespace,method){("mobileBridge","getMobileBoard")=>mobile_board(app),("mobileBridge","startMobilePairing")=>Ok(issue_code()),("mobileBridge","stopMobilePairing")=>{pairing().lock().map_err(|e|e.to_string())?.code=None;Ok(Value::Null)},("mobileBridge","readMobilePairingCode")=>{let p=pairing().lock().map_err(|e|e.to_string())?;Ok(if p.expires>Utc::now().timestamp_millis(){json!(p.code)}else{Value::Null})},("mobileBridge","listPairedMobileDevices")=>Ok(json!(read_devices(app)?.iter().map(|d|json!({"deviceId":d["deviceId"],"deviceName":d["deviceName"],"pairedAt":d["pairedAt"]})).collect::<Vec<_>>())),("mobileBridge","unpairMobileDevice")=>{unpair(app,args.first().and_then(Value::as_str).unwrap_or(""))?;Ok(Value::Null)},("releaseUpdates","checkForReleaseUpdate")=>release_check(app),("releaseUpdates","claimReleaseUpdateVersion")=>{let version=args.first().and_then(Value::as_str).unwrap_or("");Ok(json!(!version.is_empty()&&RELEASE.get_or_init(||Mutex::new(ReleaseState::default())).lock().map_err(|e|e.to_string())?.shown.insert(version.into())))},("backgroundTaskSync","runBackgroundTaskSyncNow")=>sync_now(app),_=>Err(format!("Unknown integration {namespace}.{method}"))}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mobile_pane_close_keeps_last_pane_and_foreign_panes_safe() {
        let single =
            json!({"tabs":[{"id":"tab","panes":[{"id":"%1","tabId":"tab","command":"zsh"}]}]});
        assert_eq!(
            mobile_pane_for_close(&single, "%1", true),
            Err("LastPaneError:마지막 탭은 닫을 수 없습니다".into())
        );
        assert_eq!(
            mobile_pane_for_close(&single, "%2", true),
            Err("PaneNotFoundError:이미 닫혔거나 이 태스크에 없는 탭입니다".into())
        );
        let multiple = json!({"tabs":[{"id":"tab","panes":[{"id":"%1","tabId":"tab"},{"id":"%2","tabId":"tab"}]}]});
        assert_eq!(
            mobile_pane_for_close(&multiple, "%2", true).unwrap()["id"],
            "%2"
        );
        for shell in [
            None,
            Some(""),
            Some("sh"),
            Some("bash"),
            Some("zsh"),
            Some("fish"),
            Some("dash"),
        ] {
            assert!(mobile_running_process_names(shell).is_empty());
        }
        assert_eq!(mobile_running_process_names(Some("vim")), vec!["vim"]);
        let target = |kind: &str| MirrorTarget {
            kind: kind.into(),
            name: "session".into(),
            host: None,
        };
        assert_eq!(
            close_pane_command(&target("tmux"), "%2"),
            "tmux kill-pane -t '%2'"
        );
        assert_eq!(
            close_pane_command(&target("zellij"), "terminal_2"),
            "zellij --session 'session' action close-pane --pane-id 'terminal_2'"
        );
        assert_eq!(
            close_pane_command(&target("herdr"), "w1:p2"),
            crate::herdr::command("session", &["pane", "close", "w1:p2"])
        );
    }
    #[test]
    fn mobile_commands_ignore_unreplyable_messages_and_validate_request_boundaries() {
        for raw in [
            "bad-json",
            "[]",
            "null",
            r#"{"type":"command","requestId":""}"#,
            r#"{"type":"other","requestId":"a"}"#,
        ] {
            assert_eq!(
                run_mobile_command_with(raw.as_bytes(), |_, _| panic!(
                    "unreplyable command must not execute"
                )),
                None
            );
        }
        let too_long =
            serde_json::to_vec(&json!({"type":"command","requestId":"x".repeat(65)})).unwrap();
        assert_eq!(
            run_mobile_command_with(&too_long, |_, _| panic!(
                "invalid request ID must not execute"
            )),
            None
        );
        let unsupported = br#"{"type":"command","requestId":"r","command":"toString"}"#;
        assert_eq!(
            run_mobile_command_with(unsupported, |_, _| panic!(
                "unsupported command must not execute"
            )),
            Some(
                json!({"type":"command-result","requestId":"r","ok":false,"code":"unsupported-command","error":"이 데스크탑이 지원하지 않는 명령입니다"})
            )
        );
        let invalid=br#"{"type":"command","requestId":"r","command":"delete-task","taskId":"x","isForced":"true"}"#;
        assert_eq!(
            run_mobile_command_with(invalid, |_, _| panic!(
                "invalid forced flag must not execute"
            ))
            .unwrap()["code"],
            "invalid-command"
        );
    }
    #[test]
    fn mobile_commands_correlate_success_and_map_guard_errors_without_leaking_internals() {
        let raw=br#"{"type":"command","requestId":"r","command":"delete-task","taskId":"x","isForced":true}"#;
        let success = run_mobile_command_with(raw, |command, message| {
            assert_eq!(command, "delete-task");
            assert_eq!(message["taskId"], "x");
            assert_eq!(message["isForced"], true);
            Ok(json!({"failedPeerCount":2}))
        })
        .unwrap();
        assert_eq!(
            success,
            json!({"type":"command-result","requestId":"r","ok":true,"failedPeerCount":2})
        );
        for (error, code) in [
            (
                "커밋하지 않은 변경이 있어 삭제하지 않았습니다: /repo",
                "dirty-worktree",
            ),
            (
                "PaneNotFoundError:이미 닫혔거나 이 태스크에 없는 탭입니다",
                "pane-not-found",
            ),
            ("LastPaneError:마지막 탭은 닫을 수 없습니다", "last-tab"),
            ("remote-unreachable", "session-unavailable"),
            ("secret diagnostic detail", "server-error"),
        ] {
            let result = run_mobile_command_with(raw, |_, _| Err(error.into())).unwrap();
            assert_eq!(result["requestId"], "r");
            assert_eq!(result["ok"], false);
            assert_eq!(result["code"], code);
            assert!(!text(&result, "error").contains("diagnostic"));
        }
    }

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
    fn publish_for(settings: Result<Value, String>, status: &str) -> (Value, Option<Value>) {
        let event = mobile_notification(&json!({"taskId":"task","newStatus":status}));
        let mut broadcasted = Value::Null;
        let mut pushed = None;
        publish_status_event(
            event,
            settings,
            |event| broadcasted = event.clone(),
            |event| pushed = Some(event),
        );
        (broadcasted, pushed)
    }
    #[test]
    fn selected_statuses_push_and_every_status_still_broadcasts() {
        for is_enabled in [false, true] {
            let settings = json!({"isEnabled":is_enabled,"enabledStatuses":["review"]});
            let (review, review_push) = publish_for(Ok(settings.clone()), "review");
            assert_eq!(review["status"], "review");
            assert_eq!(review["shouldNotify"], is_enabled);
            assert_eq!(review_push.is_some(), is_enabled);
            let (done, done_push) = publish_for(Ok(settings), "done");
            assert_eq!(done["status"], "done");
            assert_eq!(done["shouldNotify"], false);
            assert!(done_push.is_none());
        }
    }
    #[test]
    fn unreadable_settings_broadcast_without_push() {
        let (event, pushed) = publish_for(Err("database unavailable".into()), "review");
        assert_eq!(event["type"], "task-status-changed");
        assert_eq!(event["shouldNotify"], false);
        assert!(pushed.is_none());
    }
    #[test]
    fn push_data_excludes_should_notify() {
        let event = json!({"type":"task-status-changed","taskId":"t","shouldNotify":true});
        assert_eq!(
            push_data(&event, &json!("connection-1")),
            json!({"type":"task-status-changed","taskId":"t","connectionId":"connection-1"})
        );
    }
    #[test]
    fn notification_settings_input_dedupes_and_keeps_empty_statuses() {
        let input = json!({"isEnabled":true,"enabledStatuses":["review","review","done"]});
        assert_eq!(
            notification_settings_input(&input),
            Some((true, vec!["review", "done"]))
        );
        let input = json!({"isEnabled":false,"enabledStatuses":[]});
        assert_eq!(notification_settings_input(&input), Some((false, vec![])));
    }
    #[test]
    fn notification_settings_input_rejects_invalid_values() {
        for input in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"isEnabled":"false","enabledStatuses":[]}),
            json!({"isEnabled":true,"enabledStatuses":"review"}),
            json!({"isEnabled":true,"enabledStatuses":["unknown"]}),
            json!({"isEnabled":true,"enabledStatuses":[null]}),
        ] {
            assert_eq!(notification_settings_input(&input), None, "{input}");
        }
    }
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

#[cfg(test)]
mod pane_mirror_regressions {
    use super::*;
    const CLEAR: &str = CLEAR_SCREEN_AND_SCROLLBACK;
    const NO_MODES: &str = "\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l";
    fn cursor(x: u32, y: u32, pane_height: u32) -> PaneCursor {
        PaneCursor { x, y, pane_height }
    }
    #[test]
    fn capture_keeps_scrollback_and_quotes_pane() {
        assert_eq!(
            tmux_capture_command("%19"),
            "tmux capture-pane -pe -S -2000 -t '%19'"
        );
        let command = tmux_capture_command("%1'; rm -rf /; '");
        assert!(command.starts_with("tmux capture-pane -pe -S -2000 -t '%1'"));
        assert!(!command.ends_with("rm -rf /;"));
    }
    #[test]
    fn pane_state_command_reads_cursor_height_and_modes() {
        assert_eq!(
            tmux_pane_state_command("%19"),
            "tmux display-message -p -t '%19' '#{cursor_x} #{cursor_y} #{pane_height} #{alternate_on} #{mouse_standard_flag} #{mouse_button_flag} #{mouse_all_flag} #{mouse_sgr_flag} #{mouse_utf8_flag}'"
        );
    }
    #[test]
    fn pane_state_parses_and_treats_malformed_lines_as_unknown() {
        assert_eq!(
            parse_tmux_pane_state("8 1 24 0 0 0 0 0 0"),
            Some(TmuxPaneState {
                cursor: cursor(8, 1, 24),
                restore_sequences: NO_MODES.into()
            })
        );
        for malformed in ["8 1 0 0 0 0 0 0", "8 1", "PANE_A", ""] {
            assert_eq!(parse_tmux_pane_state(malformed), None, "{malformed}");
        }
    }
    #[test]
    fn redraw_uses_relative_cursor_from_last_line() {
        assert_eq!(
            build_screen_redraw("$ ls\nREADME.md\n", Some(&cursor(2, 2, 3))),
            format!("{CLEAR}$ ls\r\nREADME.md\r\n\x1b[3G")
        );
        assert_eq!(
            build_screen_redraw("old-1\nold-2\n$ \n\n", Some(&cursor(2, 0, 3))),
            format!("{CLEAR}old-1\r\nold-2\r\n$ \r\n\r\n\x1b[2A\x1b[3G")
        );
    }
    #[test]
    fn redraw_keeps_last_2000_lines_without_doubling_cr() {
        let screen: Vec<String> = (0..2100).map(|line| format!("line-{line}")).collect();
        let expected: Vec<String> = (100..2100).map(|line| format!("line-{line}")).collect();
        assert_eq!(
            build_screen_redraw(&screen.join("\n"), None),
            format!("{CLEAR}{}", expected.join("\r\n"))
        );
        assert_eq!(
            build_screen_redraw("a\r\nb", None),
            format!("{CLEAR}a\r\nb")
        );
    }
    #[test]
    fn tmux_snapshot_restores_modes_before_redraw() {
        assert_eq!(
            tmux_snapshot("$ ls\nREADME.md\n\n2 2 3 0 0 0 0 0 0"),
            format!("{NO_MODES}{CLEAR}$ ls\r\nREADME.md\r\n\x1b[3G")
        );
        let snapshot = tmux_snapshot("ready\n2 0 1 1 1 0 0 1 0");
        assert!(snapshot.starts_with("\x1b[?1049h"));
        assert!(snapshot.contains("\x1b[?1000h"));
        assert!(snapshot.contains("\x1b[?1006h"));
        assert!(snapshot.contains("ready\x1b[3G"));
        assert!(!snapshot.contains("2 0 1 1 1 0 0 1 0"));
    }
    #[test]
    fn tmux_snapshot_without_state_line_keeps_whole_output() {
        assert_eq!(
            tmux_snapshot("only-screen\nsecond"),
            format!("{CLEAR}only-screen\r\nsecond")
        );
    }
    #[test]
    fn mouse_mode_prefers_all_events_and_sgr_encoding() {
        let state = parse_tmux_pane_state("0 0 1 0 1 1 1 1 1").unwrap();
        assert!(state.restore_sequences.ends_with("\x1b[?1003h\x1b[?1006h"));
        let state = parse_tmux_pane_state("0 0 1 0 0 1 0 0 1").unwrap();
        assert!(state.restore_sequences.ends_with("\x1b[?1002h\x1b[?1005h"));
    }
    fn run_with_stub(stub: &str, command: &str) -> std::process::Output {
        std::process::Command::new("/bin/sh")
            .args(["-c", &format!("{stub}\n{command}")])
            .output()
            .unwrap()
    }
    #[test]
    fn zellij_dump_is_full_tailed_and_preserves_dump_failure() {
        let command = zellij_dump_command("kanvibe-task", "terminal_1");
        assert_eq!(
            command,
            "kanvibe_mobile_dump=$(zellij --session 'kanvibe-task' action dump-screen -p 'terminal_1' -a --full) && printf '%s\\n' \"$kanvibe_mobile_dump\" | tail -n 2000"
        );
        let failed = run_with_stub("zellij() { echo partial; return 3; }", &command);
        assert_eq!(failed.status.code(), Some(3));
        assert!(failed.stdout.is_empty());
        let succeeded = run_with_stub(
            "zellij() { i=0; while [ \"$i\" -lt 2100 ]; do echo \"line-$i\"; i=$((i+1)); done; }",
            &command,
        );
        assert!(succeeded.status.success());
        let output = String::from_utf8(succeeded.stdout).unwrap();
        assert_eq!(output.lines().count(), 2000);
        assert_eq!(output.lines().next(), Some("line-100"));
    }
}

#[cfg(test)]
mod mobile_pane_regressions {
    use super::*;
    #[test]
    fn mobile_task_creation_uses_desktop_session_and_rejects_unsafe_refs() {
        for submitted in [json!("tmux"), json!("screen"), json!(3)] {
            let input=mobile_creation_input(&json!({"projectId":"p","branchName":"feature","baseBranch":"dev","sessionType":submitted,"sshHost":"prod"}),&json!("herdr")).unwrap();
            assert_eq!(input["sessionType"], "herdr");
            assert_eq!(input["title"], "feature");
            assert!(input.get("sshHost").is_none());
        }
        for branch in ["-bad", "a;id", "$(id)", "../../etc", "a`id`b"] {
            assert_eq!(
                validate_mobile_draft(&json!({"branchName":branch})),
                Err("브랜치 이름에 쓸 수 없는 문자가 있습니다".into())
            );
        }
        assert_eq!(
            validate_mobile_draft(&json!({"baseBranch":"main;id"})),
            Err("베이스 브랜치에 쓸 수 없는 문자가 있습니다".into())
        );
        for priority in [json!("urgent"),json!(3),json!(true),json!({})] {
            assert_eq!(validate_mobile_draft(&json!({"priority":priority})),Err("우선순위 값이 올바르지 않습니다".into()));
        }
    }

    #[test]
    fn shortcuts_keep_session_pane_tab_ownership_and_backend_support() {
        assert_eq!(
            pane_shortcut_command("tmux", "feature", "%1", "@2", "split-right"),
            Some("tmux split-window -h -d -t '%1'".into())
        );
        assert_eq!(
            pane_shortcut_command("tmux", "feature", "%1", "@2", "scroll-up"),
            Some("tmux copy-mode -t '%1' && tmux send-keys -X -t '%1' page-up".into())
        );
        assert_eq!(
            pane_shortcut_command("tmux", "feature", "%1", "@2", "scroll-down"),
            Some("tmux copy-mode -t '%1' && tmux send-keys -X -t '%1' page-down".into())
        );
        assert_eq!(
            pane_shortcut_command("tmux", "feature", "%1", "@2", "scroll-bottom"),
            Some("tmux copy-mode -t '%1' && tmux send-keys -X -t '%1' cancel".into())
        );
        assert_eq!(
            pane_shortcut_command("tmux", "feature", "%1", "@2", "copy-mode"),
            Some("tmux copy-mode -t '%1'".into())
        );
        assert_eq!(pane_shortcut_command("zellij", "feature", "terminal_1", "2", "split-down"), Some("zellij --session 'feature' action new-pane --direction down --tab-id '2' --no-focus".into()));
        assert_eq!(pane_shortcut_command("zellij", "feature", "terminal_1", "2", "toggle-floating"), Some("zellij --session 'feature' action toggle-pane-embed-or-floating --pane-id 'terminal_1'".into()));
        assert_eq!(
            pane_shortcut_command("herdr", "feature", "w1:p1", "w1:t1", "split-down"),
            crate::herdr::pane_shortcut_command("feature", "w1:p1", "split-down")
        );
        for kind in ["tmux", "zellij", "herdr", "terminal"] {
            assert_eq!(
                pane_shortcut_command(kind, "feature", "%1", "2", "invalid"),
                None
            );
        }
        assert_eq!(
            pane_shortcut_command("herdr", "feature", "w1:p1", "w1:t1", "scroll-up"),
            None
        );
        assert_eq!(
            pane_shortcut_command("zellij", "feature", "terminal_1", "2", "copy-mode"),
            None
        );
    }
    #[test]
    fn herdr_surfaces_group_in_tab_order_without_mobile_resize() {
        let target = MirrorTarget {
            kind: "herdr".into(),
            name: "feature".into(),
            host: None,
        };
        let panes = vec![
            json!({"id":"w1:p1","tabId":"w1:t1","tabName":"work"}),
            json!({"id":"w1:p2","tabId":"w1:t1","tabName":"work"}),
            json!({"id":"w1:p3","tabId":"w1:t2","tabName":"logs"}),
        ];
        let surfaces = group_mirror_panes("task", &target, panes.clone()).unwrap();
        assert_eq!(
            surfaces,
            json!({"taskId":"task","sessionType":"herdr","canResize":false,"tabs":[{"id":"w1:t1","name":"work","panes":panes[..2]},{"id":"w1:t2","name":"logs","panes":panes[2..]}]})
        );
        assert_eq!(
            group_mirror_panes("task", &target, vec![]),
            Err("session-not-running".into())
        );
    }
}
