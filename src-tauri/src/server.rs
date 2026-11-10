#[path = "pane_stream.rs"]
mod pane_stream;
use crate::{ai::text, integrations};
use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Query, State,
    },
    http::{HeaderMap, Request, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        mpsc as std_mpsc, Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tauri::{AppHandle, Listener};
use tokio::sync::{broadcast, mpsc};
static PORT: AtomicU16 = AtomicU16::new(0);
static EVENTS: OnceLock<broadcast::Sender<Value>> = OnceLock::new();
pub fn broadcast(event: Value) {
    let _ = EVENTS.get_or_init(|| broadcast::channel(256).0).send(event);
}
pub fn hook_url(host: Option<&str>) -> Result<String, String> {
    let port = PORT.load(Ordering::Relaxed);
    let port = if port == 0 {
        if cfg!(debug_assertions) {
            19736
        } else {
            9736
        }
    } else {
        port
    };
    let address = if let Some(host) = host {
        let output = std::process::Command::new("ssh")
            .args(["-G", "--", host])
            .output()
            .map_err(|e| e.to_string())?;
        let config = String::from_utf8_lossy(&output.stdout);
        let remote = config
            .lines()
            .find_map(|l| l.strip_prefix("hostname "))
            .unwrap_or(host);
        let remote_port = config
            .lines()
            .find_map(|l| l.strip_prefix("port "))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(22);
        let destination = (remote, remote_port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .find(|a| a.is_ipv4())
            .ok_or("원격 호스트의 IPv4 주소를 찾을 수 없습니다")?;
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        socket.connect(destination).map_err(|e| e.to_string())?;
        socket
            .local_addr()
            .map_err(|e| e.to_string())?
            .ip()
            .to_string()
    } else {
        "localhost".into()
    };
    Ok(format!("http://{address}:{port}"))
}
pub fn start(app: AppHandle) -> Result<(), String> {
    let port = std::env::var("KANVIBE_HOOK_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(if cfg!(debug_assertions) { 19736 } else { 9736 });
    let listener = std::net::TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| format!("Hook server port {port}: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    PORT.store(port, Ordering::Relaxed);
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let status_events = start_status_event_worker(app.clone());
    app.listen("kanvibe:board-event", move |event| {
        let Ok(event) = serde_json::from_str::<Value>(event.payload()) else {
            return;
        };
        if event["type"] == "task-status-changed" {
            let _ = status_events.send(integrations::mobile_notification(&event));
        } else if event["type"] == "board-updated" {
            broadcast(json!({"type":"board-updated"}));
        }
    });
    app.listen("kanvibe:notifications-changed", |_| {
        broadcast(json!({"type":"notifications-changed"}))
    });
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Hook server runtime: {e}");
                return;
            }
        };
        runtime.block_on(async move {
            let listener = match tokio::net::TcpListener::from_std(listener) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Hook listener: {e}");
                    return;
                }
            };
            let router = Router::new()
                .route("/api/mobile/stream", get(stream_upgrade))
                .route("/api/mobile/events", get(events_upgrade))
                .fallback(endpoint)
                .with_state(app);
            if let Err(e) = axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                eprintln!("Hook server: {e}");
            }
        });
    });
    Ok(())
}
/// 알림 설정은 DB에서 읽으므로 이벤트 발행자를 막지 않도록 별도 스레드에서 읽고, 큐로 상태 변경 순서를 지킨다.
fn start_status_event_worker(app: AppHandle) -> std_mpsc::Sender<Value> {
    let (sender, events) = std_mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for mobile in events {
            integrations::publish_status_event(
                mobile,
                integrations::notification_settings(&app),
                |event| broadcast(event.clone()),
                |event| integrations::deliver_push(app.clone(), event),
            );
        }
    });
    sender
}
fn response(status: u16, value: Value) -> Response {
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        Json(value),
    )
        .into_response()
}
async fn endpoint(
    State(app): State<AppHandle>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request<Body>,
) -> Response {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or("").to_owned();
    let auth = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let body = match to_bytes(request.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return response(
                413,
                json!({"success":false,"error":"요청 본문이 너무 큽니다"}),
            )
        }
    };
    let body = match parse_request_body(&method, &path, &body) {
        Some(body) => body,
        None => return response(400, json!({"success":false,"error":"잘못된 요청입니다"})),
    };
    match tokio::task::spawn_blocking(move || {
        handle(
            &app,
            &method,
            &path,
            &query,
            auth.as_deref(),
            body,
            peer.ip().is_loopback(),
        )
    })
    .await
    {
        Ok(Ok((status, value))) => response(status, value),
        Ok(Err(e)) => {
            eprintln!("Hook request failed: {e}");
            response(500, json!({"success":false,"error":"서버 오류"}))
        }
        Err(e) => {
            eprintln!("Hook request panicked: {e}");
            response(500, json!({"success":false,"error":"서버 오류"}))
        }
    }
}
const NOTIFICATION_SETTINGS_PATH: &str = "/api/mobile/notification-settings";
#[derive(Debug, PartialEq)]
struct RequestBody {
    payload: Value,
    is_malformed: bool,
}
/// 대부분의 경로는 인증 전에 객체가 아닌 본문을 거절한다. 알림 설정 저장만은 인증과 값 검증 뒤에 응답을 정한다.
fn parse_request_body(method: &str, path: &str, body: &[u8]) -> Option<RequestBody> {
    if body.is_empty() {
        return Some(RequestBody {
            payload: Value::Null,
            is_malformed: false,
        });
    }
    let is_settings_update = method == "PATCH" && path == NOTIFICATION_SETTINGS_PATH;
    match serde_json::from_slice::<Value>(body) {
        Ok(payload) if payload.is_object() || is_settings_update => Some(RequestBody {
            payload,
            is_malformed: false,
        }),
        Err(_) if is_settings_update => Some(RequestBody {
            payload: Value::Null,
            is_malformed: true,
        }),
        _ => None,
    }
}
fn call(app: &AppHandle, namespace: &str, method: &str, args: Vec<Value>) -> Result<Value, String> {
    crate::core::dispatch(app, namespace, method, args)
}
fn decode(segment: &str) -> Result<String, String> {
    url::form_urlencoded::parse(format!("x={}", segment.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .ok_or("잘못된 경로입니다".into())
}
fn handle(
    app: &AppHandle,
    method: &str,
    path: &str,
    query: &str,
    auth: Option<&str>,
    body: RequestBody,
    loopback: bool,
) -> Result<(u16, Value), String> {
    let RequestBody {
        payload,
        is_malformed,
    } = body;
    if method == "GET" && path == "/api/hooks/health" {
        return Ok((200, json!({"success":true})));
    }
    if method == "POST" && (path == "/api/hooks/start" || path == "/api/hooks/status") {
        let value = call(
            app,
            "hooks",
            if path.ends_with("/start") {
                "startHookTask"
            } else {
                "updateHookTaskStatus"
            },
            vec![payload],
        )?;
        return Ok((value["status"].as_u64().unwrap_or(200) as u16, value));
    }
    if !path.starts_with("/api/mobile/") {
        return Ok((404, json!({"success":false,"error":"Not found"})));
    }
    if method == "POST"
        && path == "/api/mobile/dev/pairing-code"
        && cfg!(debug_assertions)
        && loopback
    {
        let mut code = integrations::dispatch(app, "mobileBridge", "startMobilePairing", vec![])?;
        code["success"] = json!(true);
        return Ok((200, code));
    }
    if method == "POST" && path == "/api/mobile/pair" {
        return match integrations::pair_device(
            app,
            text(&payload, "code"),
            text(&payload, "deviceName"),
        )? {
            Some(token) => Ok((200, json!({"success":true,"token":token}))),
            None => Ok((
                401,
                json!({"success":false,"error":"코드가 맞지 않거나 만료되었습니다"}),
            )),
        };
    }
    let Some(device) = integrations::authorize(app, auth)? else {
        return Ok((
            401,
            json!({"success":false,"error":"연결되지 않은 기기입니다"}),
        ));
    };
    if method == "DELETE" && path == "/api/mobile/device" {
        let removed = integrations::unpair(app, &device)?;
        return Ok((if removed { 200 } else { 401 }, json!({"success":removed})));
    }
    if method == "GET" && path == "/api/mobile/board" {
        return Ok((
            200,
            json!({"success":true,"board":integrations::mobile_board(app)?}),
        ));
    }
    if path == "/api/mobile/notifications" {
        return match method {
            "PUT" => {
                if !integrations::valid_subscription(&payload) {
                    return Ok((
                        400,
                        json!({"success":false,"error":"알림 구독 값이 올바르지 않습니다"}),
                    ));
                }
                let saved = integrations::subscription(app, &device, Some(&payload))?;
                Ok((if saved { 200 } else { 401 }, json!({"success":saved})))
            }
            "DELETE" => {
                let saved = integrations::subscription(app, &device, None)?;
                Ok((if saved { 200 } else { 401 }, json!({"success":saved})))
            }
            "GET" => {
                let notifications = crate::runtime::notification(app, "listNotifications", vec![])?;
                Ok((200, json!({"success":true,"notifications":notifications})))
            }
            _ => Ok((404, json!({"success":false,"error":"Not found"}))),
        };
    }
    if path == NOTIFICATION_SETTINGS_PATH && method == "GET" {
        let settings = integrations::notification_settings(app)?;
        return Ok((200, json!({"success":true,"settings":settings})));
    }
    if path == NOTIFICATION_SETTINGS_PATH && method == "PATCH" {
        if is_malformed {
            return Ok((400, json!({"success":false,"error":"잘못된 요청입니다"})));
        }
        return Ok(
            match integrations::update_notification_settings(app, &payload)? {
                Some(settings) => (200, json!({"success":true,"settings":settings})),
                None => (
                    400,
                    json!({"success":false,"error":"알림 상태 설정이 올바르지 않습니다."}),
                ),
            },
        );
    }
    if method == "POST" && path == "/api/mobile/notifications/read-all" {
        crate::runtime::notification(app, "markAllNotificationsRead", vec![])?;
        return Ok((200, json!({"success":true})));
    }
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    if parts.len() == 5 && parts[2] == "notifications" && parts[4] == "read" && method == "PATCH" {
        let notification = crate::runtime::notification(
            app,
            "markNotificationRead",
            vec![json!(decode(parts[3])?)],
        )?;
        return Ok((
            if notification.is_null() { 404 } else { 200 },
            json!({"success":!notification.is_null(),"notification":notification}),
        ));
    }
    if method == "POST" && path == "/api/mobile/tasks" {
        return match integrations::mobile_create(app, &payload) {
            Ok(task) => Ok((200, json!({"success":true,"task":task}))),
            Err(e) => Ok((400, json!({"success":false,"error":e}))),
        };
    }
    if method == "GET"
        && (parts.len() == 5 || parts.len() == 7)
        && parts[2] == "tasks"
        && parts[4] == "ai-sessions"
    {
        return ai_history(app, &parts, query);
    }
    if parts.len() >= 4 && parts[2] == "tasks" {
        let id = decode(parts[3])?;
        if method == "GET" && parts.len() == 4 {
            let task = integrations::mobile_task(app, &id)?;
            return Ok((
                if task.is_null() { 404 } else { 200 },
                json!({"success":!task.is_null(),"task":task}),
            ));
        }
        if method == "POST" && parts.len() == 7 && parts[4] == "panes" && parts[6] == "shortcuts" {
            let action = text(&payload, "action");
            if action.is_empty() {
                return Ok((
                    400,
                    json!({"success":false,"error":"단축키 동작이 필요합니다"}),
                ));
            }
            let pane = decode(parts[5])?;
            let result = integrations::mobile_shortcut(app, &id, &pane, action);
            if let Err(error) = &result {
                if [
                    "지원하지 않는 단축키입니다",
                    "일반 터미널에서는 세션 제어 단축키를 사용할 수 없습니다",
                    "이 세션에서 지원하지 않는 단축키입니다",
                ]
                .contains(&error.as_str())
                {
                    return Ok((400, json!({"success":false,"error":error})));
                }
            }
            return surface_result(result, true);
        }
        if parts.len() == 5 {
            match (method, parts[4]) {
                ("PATCH", "status") => {
                    let status = text(&payload, "status");
                    if !["todo", "progress", "pending", "review", "done"].contains(&status) {
                        return Ok((400, json!({"success":false,"error":"잘못된 상태 값입니다"})));
                    }
                    let task = call(
                        app,
                        "kanban",
                        "updateTaskStatus",
                        vec![json!(id), json!(status)],
                    )?;
                    return Ok((
                        if task.is_null() { 404 } else { 200 },
                        json!({"success":!task.is_null(),"task":task}),
                    ));
                }
                ("GET", "surfaces") => {
                    return surface_result(integrations::surfaces(app, &id), false)
                }
                ("POST", "tabs") => {
                    return surface_result(integrations::mobile_tab(app, &id), true)
                }
                _ => {}
            }
        }
    }
    if method == "GET" && parts.len() == 5 && parts[2] == "projects" && parts[4] == "branches" {
        let branches = call(
            app,
            "project",
            "getProjectBranches",
            vec![json!(decode(parts[3])?)],
        )?;
        return Ok((200, json!({"success":true,"branches":branches})));
    }
    Ok((404, json!({"success":false,"error":"Not found"})))
}
const AI_MESSAGE_ROLES: [&str; 7] = [
    "user",
    "assistant",
    "tool",
    "system",
    "developer",
    "reasoning",
    "unknown",
];
const AI_HISTORY_PAGE_LIMIT: u64 = 100;
const MAX_SAFE_CURSOR: u64 = 9_007_199_254_740_991;
#[derive(Debug, PartialEq)]
struct HistoryOptions {
    query: Option<String>,
    cursor: Option<String>,
    limit: u64,
    roles: Option<Vec<String>>,
}
/// 모바일 History 조회 값을 검증한다. 파일 경로(sourceRef)는 네트워크에서 받지 않는다.
fn history_options(query: &str, provider: Option<&str>) -> Option<HistoryOptions> {
    let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let get = |name: &str| {
        params
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let number = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    let limit =
        number(get("limit").unwrap_or("20")).filter(|n| (1..=AI_HISTORY_PAGE_LIMIT).contains(n))?;
    let cursor = get("cursor");
    if cursor.is_some_and(|c| number(c).is_none_or(|n| n > MAX_SAFE_CURSOR)) {
        return None;
    }
    let roles: Option<Vec<String>> =
        get("roles").map(|r| r.split(',').map(str::to_owned).collect());
    let invalid_role = roles
        .iter()
        .flatten()
        .any(|role| !AI_MESSAGE_ROLES.contains(&role.as_str()));
    let unknown_provider = provider.is_some_and(|p| !crate::ai::sessions::PROVIDERS.contains(&p));
    if invalid_role || unknown_provider || get("sourceRef").is_some() {
        return None;
    }
    Some(HistoryOptions {
        query: get("query")
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(str::to_owned),
        cursor: cursor.map(str::to_owned),
        limit,
        roles,
    })
}
fn without_source_ref(mut value: Value) -> Value {
    if let Some(record) = value.as_object_mut() {
        record.remove("sourceRef");
    }
    value
}
/// 태스크의 Agent 대화 목록(경로 길이 5)과 한 대화의 상세(길이 7)를 돌려준다.
/// 상세는 리더가 매번 태스크 경로와 세션 ID로 다시 찾으므로 목록의 파일 경로를 기억하지 않는다.
fn ai_history(app: &AppHandle, parts: &[&str], query: &str) -> Result<(u16, Value), String> {
    let task_id = decode(parts[3])?;
    let target = if parts.len() == 7 {
        Some((decode(parts[5])?, decode(parts[6])?))
    } else {
        None
    };
    let Some(options) = history_options(query, target.as_ref().map(|(p, _)| p.as_str())) else {
        return Ok((
            400,
            json!({"success":false,"error":"조회 값이 올바르지 않습니다"}),
        ));
    };
    if integrations::mobile_task(app, &task_id)?.is_null() {
        return Ok((404, json!({"success":false,"error":"없는 태스크입니다"})));
    }
    let Some((provider, session_id)) = target else {
        let args = vec![
            json!(task_id),
            json!(options.query),
            json!(options.cursor),
            json!(options.limit),
        ];
        let mut history = call(app, "project", "getTaskAiSessions", args)?;
        if let Some(sessions) = history["sessions"].as_array_mut() {
            for session in sessions.iter_mut() {
                *session = without_source_ref(session.take());
            }
        }
        return Ok((200, json!({"success":true,"history":history})));
    };
    let args = vec![
        json!(task_id),
        json!(provider),
        json!(session_id),
        Value::Null,
        json!(options.cursor),
        json!(options.limit),
        json!(options.query),
        json!(options.roles),
    ];
    let detail = call(app, "project", "getTaskAiSessionDetail", args)?;
    if detail.is_null() {
        return Ok((404, json!({"success":false,"error":"없는 대화입니다"})));
    }
    Ok((
        200,
        json!({"success":true,"detail":without_source_ref(detail)}),
    ))
}
fn surface_result(result: Result<Value, String>, tab: bool) -> Result<(u16, Value), String> {
    match result {
        Ok(mut v) => {
            if tab {
                v["success"] = json!(true);
                Ok((200, v))
            } else {
                Ok((200, json!({"success":true,"surfaces":v})))
            }
        }
        Err(reason)
            if [
                "no-session",
                "session-not-running",
                "zellij-too-old",
                "herdr-missing",
                "remote-unreachable",
            ]
            .contains(&reason.as_str()) =>
        {
            Ok((409, json!({"success":false,"reason":reason})))
        }
        Err(e) => Err(e),
    }
}
async fn upgrade_auth(app: &AppHandle, headers: &HeaderMap) -> Result<(String, String), Response> {
    let auth = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let app = app.clone();
    let auth_copy = auth.clone();
    match tokio::task::spawn_blocking(move || integrations::authorize(&app, Some(&auth_copy))).await
    {
        Ok(Ok(Some(id))) => Ok((id, auth)),
        _ => Err(response(
            401,
            json!({"success":false,"error":"연결되지 않은 기기입니다"}),
        )),
    }
}
async fn events_upgrade(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let (device, auth) = match upgrade_auth(&app, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    ws.on_upgrade(move |socket| events_socket(app, socket, device, auth))
        .into_response()
}
async fn stream_upgrade(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    let (device, auth) = match upgrade_auth(&app, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (Some(task), Some(pane)) = (query.get("taskId"), query.get("paneId")) else {
        return response(
            400,
            json!({"success":false,"error":"taskId and paneId required"}),
        );
    };
    let app_check = app.clone();
    let task = task.clone();
    let pane = pane.clone();
    let task_check = task.clone();
    let pane_check = pane.clone();
    if !matches!(
        tokio::task::spawn_blocking(move || integrations::verify_pane(
            &app_check,
            &task_check,
            &pane_check
        ))
        .await,
        Ok(Ok(_))
    ) {
        return response(409, json!({"success":false,"reason":"session-not-running"}));
    }
    ws.on_upgrade(move |socket| pane_socket(app, socket, device, auth, task, pane))
        .into_response()
}
async fn close(socket: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}
async fn events_socket(app: AppHandle, mut socket: WebSocket, device: String, auth: String) {
    let mut events = EVENTS.get_or_init(|| broadcast::channel(256).0).subscribe();
    if integrations::authorize(&app, Some(&auth))
        .ok()
        .flatten()
        .as_deref()
        != Some(&device)
    {
        close(&mut socket, 4001, "device-unpaired").await;
        return;
    }
    let _ = socket
        .send(Message::Text(
            json!({"type":"connected"}).to_string().into(),
        ))
        .await;
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    let mut alive = true;
    loop {
        tokio::select! {event=events.recv()=>match event{Ok(v) if v["type"]=="device-unpaired"=>{if text(&v,"deviceId")==device{close(&mut socket,4001,"device-unpaired").await;break}},Ok(v)=>if socket.send(Message::Text(v.to_string().into())).await.is_err(){break},Err(broadcast::error::RecvError::Lagged(_))=>{let _=socket.send(Message::Text(json!({"type":"board-updated"}).to_string().into())).await;},Err(_)=>break},message=socket.recv()=>match message{Some(Ok(Message::Pong(_)))=>alive=true,Some(Ok(Message::Ping(data)))=>{let _=socket.send(Message::Pong(data)).await;},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,_=>{}},_=ping.tick()=>{if !alive{break}alive=false;if socket.send(Message::Ping(vec![].into())).await.is_err(){break}}}
    }
}
#[derive(Clone)]
struct Viewport {
    kind: String,
    host: Option<String>,
    pane: String,
    tab: String,
    original: Value,
    clients: Vec<(String, String, u16, u16)>,
}
static VIEWPORTS: OnceLock<Mutex<HashMap<String, Viewport>>> = OnceLock::new();
fn resize_mobile(
    app: &AppHandle,
    task: &str,
    pane: &str,
    client: &str,
    cols: u16,
    rows: u16,
) -> Result<Option<String>, String> {
    if !(1..=500).contains(&cols) || !(1..=500).contains(&rows) {
        return Err("터미널 크기가 유효하지 않습니다".into());
    }
    let target = integrations::verify_pane(app, task, pane)?;
    if matches!(target.kind.as_str(), "zellij" | "herdr") {
        return Err("session-not-running".into());
    }
    let tab = if target.kind == "terminal" {
        pane.into()
    } else {
        crate::git::run(
            &format!(
                "tmux display-message -p -t {} '#{{window_id}}'",
                crate::runtime::quote(pane)
            ),
            target.host.as_deref(),
        )?
        .trim()
        .to_string()
    };
    let mut all = VIEWPORTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|e| e.to_string())?;
    let key = format!("{task}#{tab}");
    if !all.contains_key(&key) {
        let original = if target.kind == "terminal" {
            let (c, r) = crate::terminal::dimensions(app, task, Some(pane))?;
            json!({"cols":c,"rows":r})
        } else {
            let output=crate::git::run(&format!("tmux display-message -p -t {tab} '#{{window_width}}\t#{{window_height}}\t#{{window_layout}}\t#{{window_zoomed_flag}}' && tmux show-options -wqv -t {tab} window-size",tab=crate::runtime::quote(&tab)),target.host.as_deref())?;
            let mut lines = output.lines();
            let fields: Vec<_> = lines.next().unwrap_or("").split('\t').collect();
            if fields.len() != 4 {
                return Err("tmux 창 크기를 읽지 못했습니다".into());
            }
            json!({"cols":fields[0].parse::<u16>().map_err(|e|e.to_string())?,"rows":fields[1].parse::<u16>().map_err(|e|e.to_string())?,"layout":fields[2],"zoomed":fields[3]=="1","windowSize":lines.next().filter(|s|!s.is_empty())})
        };
        all.insert(
            key.clone(),
            Viewport {
                kind: target.kind.clone(),
                host: target.host.clone(),
                pane: pane.into(),
                tab,
                original,
                clients: vec![],
            },
        );
    }
    let viewport = all.get_mut(&key).unwrap();
    viewport.clients.retain(|(id, _, _, _)| id != client);
    viewport
        .clients
        .push((client.into(), pane.into(), cols, rows));
    viewport.pane = pane.into();
    apply_resize(app, task, viewport, cols, rows)?;
    resized_pane_snapshot(&target, |target| {
        integrations::snapshot_verified(target, pane)
    })
}
/// tmux는 크기가 바뀌면 화면을 다시 배치하므로 바뀐 크기로 다시 읽어 첫 화면부터 어긋나지 않게 한다.
fn resized_pane_snapshot(
    target: &integrations::MirrorTarget,
    read_snapshot: impl FnOnce(&integrations::MirrorTarget) -> Result<String, String>,
) -> Result<Option<String>, String> {
    if target.kind == "tmux" {
        read_snapshot(target).map(Some)
    } else {
        Ok(None)
    }
}
fn apply_resize(
    app: &AppHandle,
    task: &str,
    v: &Viewport,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    if v.kind == "terminal" {
        return crate::terminal::resize_client(
            app,
            task,
            Some(&v.pane),
            "mobile:viewport",
            cols,
            rows,
        );
    }
    let q = crate::runtime::quote;
    let unzoom = q(&format!("resize-pane -Z -t {}", q(&v.tab)));
    let geometry=crate::git::run(&format!("tmux if-shell -F -t {tab} '#{{window_zoomed_flag}}' {unzoom} && tmux display-message -p -t {pane} '#{{window_width}} #{{window_height}} #{{pane_width}} #{{pane_height}}'",tab=q(&v.tab),pane=q(&v.pane)),v.host.as_deref())?;
    let fields: Vec<i32> = geometry
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    if fields.len() != 4 || fields.iter().any(|n| *n <= 0) {
        return Err("tmux pane 크기를 읽지 못했습니다".into());
    }
    let width = (fields[0] - fields[2] + cols as i32).max(1);
    let height = (fields[1] - fields[3] + rows as i32).max(1);
    crate::git::run(&format!("tmux resize-window -t {pane} -x {width} -y {height} && tmux resize-pane -t {pane} -x {cols} -y {rows}",pane=q(&v.pane)),v.host.as_deref()).map(|_|())
}
fn restore_viewport(app: &AppHandle, task: &str, v: &Viewport) -> Result<(), String> {
    let cols = v.original["cols"].as_u64().unwrap_or(80) as u16;
    let rows = v.original["rows"].as_u64().unwrap_or(24) as u16;
    if v.kind == "terminal" {
        return crate::terminal::release_mobile_viewport(app, task, Some(&v.pane), cols, rows);
    }
    let q = crate::runtime::quote;
    let tab = q(&v.tab);
    let policy = if let Some(policy) = v.original["windowSize"].as_str() {
        format!("tmux set-option -w -t {tab} window-size {}", q(policy))
    } else {
        format!("tmux set-option -w -u -t {tab} window-size")
    };
    let zoom = if v.original["zoomed"] == true {
        format!(
            "tmux if-shell -F -t {tab} '#{{!: window_zoomed_flag}}' {} || exit $?;",
            q(&format!("resize-pane -Z -t {tab}"))
        )
        .replace("!: window_zoomed_flag", "!:#{window_zoomed_flag}")
    } else {
        String::new()
    };
    crate::git::run(&format!("kanvibe_restore_status=0; tmux resize-window -t {tab} -x {cols} -y {rows} && tmux select-layout -t {tab} {} || kanvibe_restore_status=$?; {policy} || exit $?; {zoom} exit \"$kanvibe_restore_status\"",q(text(&v.original,"layout"))),v.host.as_deref()).map(|_|())
}
fn release_mobile(app: &AppHandle, task: &str, _pane: &str, client: &str) {
    let mut all = VIEWPORTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    let Some(key) = all
        .iter()
        .find(|(_, v)| v.clients.iter().any(|(id, _, _, _)| id == client))
        .map(|(k, _)| k.clone())
    else {
        return;
    };
    let v = all.get_mut(&key).unwrap();
    v.clients.retain(|(id, _, _, _)| id != client);
    if let Some((_, pane, cols, rows)) = v.clients.last().cloned() {
        v.pane = pane;
        let _ = apply_resize(app, task, v, cols, rows);
    } else {
        let _ = restore_viewport(app, task, v);
        all.remove(&key);
    }
}
fn decode_utf8(pending: &mut Vec<u8>) -> String {
    let mut result = String::new();
    loop {
        match std::str::from_utf8(pending) {
            Ok(text) => {
                result.push_str(text);
                pending.clear();
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    result.push_str(std::str::from_utf8(&pending[..valid]).unwrap());
                    pending.drain(..valid);
                }
                match error.error_len() {
                    Some(length) => {
                        result.push('�');
                        pending.drain(..length);
                    }
                    None => break,
                }
            }
        }
    }
    result
}
async fn pane_socket(
    app: AppHandle,
    mut socket: WebSocket,
    device: String,
    auth: String,
    task: String,
    pane: String,
) {
    let client = format!("mobile:{}", uuid::Uuid::new_v4());
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(256);
    let overflow = Arc::new(AtomicBool::new(false));
    let mut pending_utf8 = Vec::new();
    let target = match integrations::verify_pane(&app, &task, &pane) {
        Ok(t) => t,
        Err(_) => {
            close(&mut socket, 4000, "session-not-running").await;
            return;
        }
    };
    let plain = target.kind == "terminal";
    if plain {
        let app_start = app.clone();
        let task_start = task.clone();
        let pane_start = pane.clone();
        let client_start = client.clone();
        let overflow_sender = overflow.clone();
        let output = tokio::task::spawn_blocking(move || {
            let dims = crate::terminal::dimensions(&app_start, &task_start, Some(&pane_start))
                .unwrap_or((80, 24));
            crate::terminal::open(
                &app_start,
                &client_start,
                &task_start,
                Some(&pane_start),
                dims.0,
                dims.1,
            )?;
            crate::terminal::subscribe(
                &app_start,
                &task_start,
                Some(&pane_start),
                &client_start,
                Arc::new(move |bytes| {
                    if tx.try_send(bytes).is_err() {
                        overflow_sender.store(true, Ordering::Relaxed);
                    }
                }),
            )
        })
        .await;
        match output {
            Ok(Ok(snapshot)) => {
                if socket
                    .send(Message::Text(
                        String::from_utf8_lossy(&snapshot).into_owned().into(),
                    ))
                    .await
                    .is_err()
                {
                    crate::terminal::unsubscribe(&app, &task, Some(&pane), &client);
                    let _ = crate::terminal::detach(&app, &client, &task, Some(&pane));
                    return;
                }
            }
            _ => {
                close(&mut socket, 4000, "session-not-running").await;
                return;
            }
        }
    }
    let mut fifo = if matches!(target.kind.as_str(), "tmux" | "herdr") {
        let target_start = target.clone();
        let pane_start = pane.clone();
        let prepared = tokio::task::spawn_blocking(move || {
            let snapshot = integrations::snapshot_verified(&target_start, &pane_start)?;
            let subscription = pane_stream::subscribe(&target_start, &pane_start)?;
            Ok::<_, String>((snapshot, subscription))
        })
        .await;
        match prepared {
            Ok(Ok((snapshot, subscription))) => {
                if socket.send(Message::Text(snapshot.into())).await.is_err() {
                    let _ = tokio::task::spawn_blocking(move || drop(subscription)).await;
                    return;
                }
                Some(subscription)
            }
            _ => {
                close(&mut socket, 4000, "session-not-running").await;
                return;
            }
        }
    } else {
        None
    };
    let fifo_overflow = fifo.as_ref().map(|s| s.overflow.clone());
    let mut events = EVENTS.get_or_init(|| broadcast::channel(256).0).subscribe();
    let mut poll = tokio::time::interval(Duration::from_millis(if target.host.is_some() {
        750
    } else {
        250
    }));
    let mut previous = String::new();
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    let mut alive = true;
    if integrations::authorize(&app, Some(&auth))
        .ok()
        .flatten()
        .as_deref()
        != Some(&device)
    {
        close(&mut socket, 4001, "device-unpaired").await;
    } else {
        loop {
            tokio::select! {bytes=rx.recv(),if plain=>{match bytes{Some(bytes)=>{if overflow.load(Ordering::Relaxed){close(&mut socket,4000,"slow-consumer").await;break}pending_utf8.extend_from_slice(&bytes);let text=decode_utf8(&mut pending_utf8);if !text.is_empty()&&socket.send(Message::Text(text.into())).await.is_err(){break}},None=>break}},bytes=async{fifo.as_mut().unwrap().output.recv().await},if fifo.is_some()=>{
                match bytes {
                    Some(bytes)=>{if fifo_overflow.as_ref().is_some_and(|flag|flag.load(Ordering::Acquire)){close(&mut socket,4000,"slow-consumer").await;break}pending_utf8.extend_from_slice(&bytes);let text=decode_utf8(&mut pending_utf8);if !text.is_empty()&&socket.send(Message::Text(text.into())).await.is_err(){break}},
                    None=>{close(&mut socket,4000,"session-not-running").await;break}
                }
            },_=poll.tick(),if target.kind=="zellij"=>{let target=target.clone();let p=pane.clone();match tokio::task::spawn_blocking(move||integrations::snapshot_verified(&target,&p)).await{Ok(Ok(snapshot))=>{if snapshot!=previous{previous=snapshot.clone();if socket.send(Message::Text(snapshot.into())).await.is_err(){break}}},_=>{close(&mut socket,4000,"session-not-running").await;break}}},event=events.recv()=>{if let Ok(event)=event{if event["type"]=="device-unpaired"&&text(&event,"deviceId")==device{close(&mut socket,4001,"device-unpaired").await;break}}},message=socket.recv()=>{match message{Some(Ok(Message::Text(input)))=>{let a=app.clone();let t=task.clone();let p=pane.clone();let input=input.to_string();let target=target.clone();let _=tokio::task::spawn_blocking(move||integrations::write_pane_verified(&a,&t,&p,&input,&target)).await;},Some(Ok(Message::Binary(bytes)))=>{if bytes.len()>256{continue}let Ok(v)=serde_json::from_slice::<Value>(&bytes)else{continue};if v["type"]!="resize"{continue}let (Some(cols),Some(rows))=(v["cols"].as_u64(),v["rows"].as_u64())else{continue};if !(1..=500).contains(&cols)||!(1..=500).contains(&rows){continue}let a=app.clone();let t=task.clone();let p=pane.clone();let c=client.clone();match tokio::task::spawn_blocking(move||resize_mobile(&a,&t,&p,&c,cols as u16,rows as u16)).await{Ok(Ok(Some(snapshot)))=>{if socket.send(Message::Text(snapshot.into())).await.is_err(){break}},Ok(Ok(None))=>{},_=>{close(&mut socket,4000,"resize-failed").await;break}}},Some(Ok(Message::Pong(_)))=>alive=true,Some(Ok(Message::Ping(data)))=>{let _=socket.send(Message::Pong(data)).await;},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break}},_=ping.tick()=>{if !alive{break}alive=false;if socket.send(Message::Ping(vec![].into())).await.is_err(){break}}}
        }
    }
    if let Some(subscription) = fifo.take() {
        let _ = tokio::task::spawn_blocking(move || drop(subscription)).await;
    }
    if plain {
        crate::terminal::unsubscribe(&app, &task, Some(&pane), &client);
        let _ = crate::terminal::detach(&app, &client, &task, Some(&pane));
    }
    let a = app.clone();
    let _ = tokio::task::spawn_blocking(move || release_mobile(&a, &task, &pane, &client)).await;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_does_not_turn_plus_into_space() {
        assert_eq!(decode("a%2Fb+c").unwrap(), "a/b+c");
    }
    #[test]
    fn history_options_pass_search_page_and_roles() {
        assert_eq!(
            history_options("query=%20fix%20&cursor=20&limit=10", None),
            Some(HistoryOptions {
                query: Some("fix".into()),
                cursor: Some("20".into()),
                limit: 10,
                roles: None
            })
        );
        let detail = history_options("roles=user,assistant", Some("hermes")).unwrap();
        assert_eq!(detail.limit, 20);
        assert_eq!(detail.query, None);
        assert_eq!(detail.roles, Some(vec!["user".into(), "assistant".into()]));
    }
    #[test]
    fn history_options_reject_invalid_values() {
        for query in [
            "limit=0",
            "limit=101",
            "limit=2x",
            "cursor=-1",
            "cursor=garbage",
            "cursor=9007199254740992",
            "roles=invalid",
            "sourceRef=/private/file",
        ] {
            assert_eq!(history_options(query, Some("hermes")), None, "{query}");
        }
        assert_eq!(history_options("", Some("unknown")), None);
        assert!(history_options("", Some("zcode")).is_some());
    }
    #[test]
    fn history_responses_hide_source_paths() {
        let value = without_source_ref(json!({"id":"s1","sourceRef":"/trusted/rollout.jsonl"}));
        assert_eq!(value, json!({"id":"s1"}));
    }
    #[test]
    fn notification_settings_body_distinguishes_malformed_from_invalid() {
        let parse = |body: &[u8]| parse_request_body("PATCH", NOTIFICATION_SETTINGS_PATH, body);
        assert_eq!(
            parse(b"{not-json"),
            Some(RequestBody {
                payload: Value::Null,
                is_malformed: true
            })
        );
        for (body, payload) in [
            (&b"null"[..], Value::Null),
            (&b"[]"[..], json!([])),
            (&b""[..], Value::Null),
        ] {
            assert_eq!(
                parse(body),
                Some(RequestBody {
                    payload,
                    is_malformed: false
                })
            );
        }
    }
    #[test]
    fn other_routes_still_reject_non_object_bodies_before_auth() {
        for (method, path) in [
            ("GET", NOTIFICATION_SETTINGS_PATH),
            ("POST", "/api/mobile/tasks"),
        ] {
            assert_eq!(parse_request_body(method, path, b"[]"), None);
            assert_eq!(parse_request_body(method, path, b"{not-json"), None);
        }
    }
    #[test]
    fn only_tmux_resize_reads_a_fresh_snapshot() {
        let target = |kind: &str| integrations::MirrorTarget {
            kind: kind.into(),
            name: "session".into(),
            host: None,
        };
        assert_eq!(
            resized_pane_snapshot(&target("tmux"), |_| Ok("history".into())),
            Ok(Some("history".into()))
        );
        assert_eq!(
            resized_pane_snapshot(&target("tmux"), |_| Err("capture failed".into())),
            Err("capture failed".to_string())
        );
        assert_eq!(
            resized_pane_snapshot(&target("terminal"), |_| panic!("must not read")),
            Ok(None)
        );
    }
    #[test]
    fn missing_herdr_reports_surface_conflict() {
        assert_eq!(
            surface_result(Err("herdr-missing".into()), false).unwrap(),
            (409, json!({"success":false,"reason":"herdr-missing"}))
        );
    }
    #[test]
    fn unavailable_surface_uses_conflict() {
        let (status, body) = surface_result(Err("remote-unreachable".into()), false).unwrap();
        assert_eq!(status, 409);
        assert_eq!(body["reason"], "remote-unreachable");
    }
}
#[cfg(test)]
mod stream_regressions {
    use super::*;
    #[test]
    fn mobile_text_frames_preserve_split_utf8() {
        let bytes = "한글💻".as_bytes();
        let mut pending = vec![];
        let mut result = String::new();
        for byte in bytes {
            pending.push(*byte);
            result.push_str(&decode_utf8(&mut pending));
        }
        assert_eq!(result, "한글💻");
        assert!(pending.is_empty());
    }
}
