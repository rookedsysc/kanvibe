use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Cursor,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow};
use tauri_plugin_notification::NotificationExt;

#[derive(Default)]
pub struct NativeState {
    #[cfg(target_os = "macos")]
    pub last_window_closing: std::sync::atomic::AtomicBool,
    pending_activation: Mutex<HashMap<String, Value>>,
    notification_lock: Mutex<()>,
    notification_ids: Mutex<HashMap<i32, String>>,
}
#[cfg(target_os = "macos")]
impl NativeState {
    pub fn take_last_window_close(&self, code: Option<i32>) -> bool {
        self.last_window_closing
            .swap(false, std::sync::atomic::Ordering::SeqCst)
            && code.is_none()
    }
}
#[cfg(target_os = "macos")]
pub fn desktop_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, PredefinedMenuItem as Item, Submenu};
    // 네이티브 Cmd+W 메뉴를 두면 WebView의 터미널 탭 닫기보다 먼저 창 전체가 닫힌다.
    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                "KanVibe",
                true,
                &[
                    &Item::about(app, None, None)?,
                    &Item::separator(app)?,
                    &Item::services(app, None)?,
                    &Item::hide(app, None)?,
                    &Item::hide_others(app, None)?,
                    &Item::separator(app)?,
                    &Item::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &Item::undo(app, None)?,
                    &Item::redo(app, None)?,
                    &Item::separator(app)?,
                    &Item::cut(app, None)?,
                    &Item::copy(app, None)?,
                    &Item::paste(app, None)?,
                    &Item::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Window",
                true,
                &[&Item::minimize(app, None)?, &Item::maximize(app, None)?],
            )?,
        ],
    )
}
pub fn close_window(app: &AppHandle, label: &str) {
    if let Ok(mut pending) = app.state::<NativeState>().pending_activation.lock() {
        pending.remove(label);
    }
}
pub fn initialize(app: &AppHandle) -> Result<(), String> {
    let handle = app.clone();
    app.notification()
        .on_action(move |action| {
            let Some(notification) = action.notification() else {
                return;
            };
            let state = handle.state::<NativeState>();
            let id = state
                .notification_ids
                .lock()
                .ok()
                .and_then(|m| m.get(&notification.id()).cloned());
            if let Some(id) = id {
                let handle = handle.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(error) =
                        self::notification(&handle, "activateNotification", vec![json!(id)])
                    {
                        eprintln!("[kanvibe] notification activation: {error}");
                    }
                });
            }
        })
        .map_err(err)?;
    crate::notifications::initialize(app);
    Ok(())
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub fn home() -> String {
    std::env::var("HOME")
        .ok()
        .or_else(|| dirs::home_dir().map(|v| v.to_string_lossy().into_owned()))
        .unwrap_or("/".into())
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub fn shell_environment() -> HashMap<String, String> {
    let mut values: HashMap<_, _> = std::env::vars()
        .filter(|(key, _)| {
            !matches!(key.as_str(), "PORT" | "HOST" | "NODE_ENV") && !key.starts_with("KANVIBE_")
        })
        .collect();
    let h = home();
    let mut paths: Vec<String> = values
        .get("PATH")
        .map(|v| {
            v.split(':')
                .filter(|v| !v.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        for p in [
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
            "/opt/local/bin",
            "/opt/local/sbin",
        ] {
            if !paths.iter().any(|v| v == p) {
                paths.push(p.into());
            }
        }
        for p in [
            ".local/bin",
            ".cargo/bin",
            ".opencode/bin",
            "Library/pnpm",
            ".bun/bin",
        ] {
            let full = format!("{h}/{p}");
            if !paths.contains(&full) {
                paths.push(full);
            }
        }
    }
    let user_bin = format!("{h}/bin");
    if !paths.contains(&user_bin) {
        paths.push(user_bin);
    }
    values.insert("PATH".into(), paths.join(":"));
    values.insert("HOME".into(), h);
    values.entry("LANG".into()).or_insert("en_US.UTF-8".into());
    values
        .entry("LC_ALL".into())
        .or_insert("en_US.UTF-8".into());
    values
}
pub fn run_shell(command: &str, host: Option<&str>) -> Result<String, String> {
    run_shell_timeout(command, host, Duration::from_secs(30))
}
pub fn run_shell_timeout(
    command: &str,
    host: Option<&str>,
    timeout: Duration,
) -> Result<String, String> {
    run_shell_input(command, host, None, timeout)
}
pub fn run_shell_input(
    command: &str,
    host: Option<&str>,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut cmd = if let Some(host) = host {
        let mut c = Command::new("ssh");
        c.args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "--",
            host,
        ]);
        c.arg(format!("sh -lc {}", quote(command)));
        c
    } else {
        let mut c = Command::new("/bin/sh");
        c.args(["-lc", command]);
        c
    };
    cmd.env_clear()
        .envs(shell_environment())
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    // macOS에는 pipe2가 없어 파이프 생성과 CLOEXEC 설정 사이에 다른 스레드가 자식을 띄우면 출력 파이프가 그 자식에게 샌다.
    // 잠금을 기다리는 형제 명령이 파이프를 물면 잠금을 쥔 명령이 끝나지 못해 서로 막히므로 생성 구간을 줄 세운다.
    static SPAWN_LOCK: Mutex<()> = Mutex::new(());
    let spawned = {
        let _guard = SPAWN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        cmd.spawn()
    };
    let mut child = spawned.map_err(err)?;
    let pid = child.id();
    if let Some(input) = input {
        let bytes = input.to_vec();
        let mut stdin = child.stdin.take().ok_or("stdin missing")?;
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }
    let read = |stream: Box<dyn Read + Send>| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(err)
                .and_then(|_| {
                    if bytes.len() > 32 * 1024 * 1024 {
                        Err("명령 출력이 32MB를 초과했습니다.".into())
                    } else {
                        Ok(String::from_utf8_lossy(&bytes).into_owned())
                    }
                });
            let _ = tx.send(result);
        });
        rx
    };
    let output = read(Box::new(child.stdout.take().ok_or("stdout missing")?));
    let error = read(Box::new(child.stderr.take().ok_or("stderr missing")?));
    let terminate = |child: &mut std::process::Child| {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    };
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(err)? {
            break status;
        }
        if start.elapsed() >= timeout {
            terminate(&mut child);
            return Err("명령 실행 시간이 초과되었습니다.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let remaining = || timeout.saturating_sub(start.elapsed());
    let stdout = match output.recv_timeout(remaining()) {
        Ok(result) => result?,
        Err(_) => {
            terminate(&mut child);
            return Err("명령 출력 대기 시간이 초과되었습니다.".into());
        }
    };
    let stderr = match error.recv_timeout(remaining()) {
        Ok(result) => result?,
        Err(_) => {
            terminate(&mut child);
            return Err("명령 오류 출력 대기 시간이 초과되었습니다.".into());
        }
    };
    if status.success() {
        Ok(stdout)
    } else {
        Err(format!(
            "{} (exit {})",
            stderr.trim(),
            status.code().unwrap_or(-1)
        ))
    }
}
pub fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("KANVIBE_DB_PATH") {
        if !path.is_empty() {
            return Ok(path.into());
        }
    }
    let root = if let Ok(path) = std::env::var("KANVIBE_APP_DATA_DIR") {
        PathBuf::from(path)
    } else {
        // Electron이 사용하던 userData 디렉터리를 먼저 선택해 기존 데이터를 이어받는다.
        let legacy = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(home()))
            .join("kanvibe");
        let product = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(home()))
            .join("KanVibe");
        if legacy.join("kanvibe.db").exists() {
            legacy
        } else if product.join("kanvibe.db").exists() {
            product
        } else {
            app.path().app_data_dir().map_err(err)?
        }
    };
    std::fs::create_dir_all(&root).map_err(err)?;
    Ok(root.join("kanvibe.db"))
}
fn string(args: &[Value], i: usize) -> Result<&str, String> {
    args.get(i)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("인자 {i}가 필요합니다."))
}
fn optional(args: &[Value], i: usize) -> Option<&str> {
    args.get(i).and_then(Value::as_str)
}
fn number(args: &[Value], i: usize, default: u16) -> u16 {
    args.get(i)
        .and_then(Value::as_u64)
        .map(|v| v.clamp(1, u16::MAX as u64) as u16)
        .unwrap_or(default)
}

/// 렌더러는 open 응답을 기다리지 않고 입력·크기·닫기를 보낼 수 있다. 아직 없거나 이미 끝난 세션이면 할 일이 없다.
fn ignore_missing_session(result: Result<(), String>) -> Result<Value, String> {
    match result {
        Err(error) if error != crate::terminal::SESSION_NOT_FOUND => Err(error),
        _ => Ok(Value::Null),
    }
}

pub fn native(
    app: &AppHandle,
    window: &WebviewWindow,
    method: &str,
    args: Vec<Value>,
) -> Result<Value, String> {
    let label = window.label();
    match method {
        "openTerminal" => crate::terminal::open(
            app,
            label,
            string(&args, 0)?,
            optional(&args, 1),
            number(&args, 2, 120),
            number(&args, 3, 30),
        )
        .or_else(|e| Ok(json!({"ok":false,"error":e}))),
        "writeTerminal" => ignore_missing_session(crate::terminal::write(
            app,
            string(&args, 0)?,
            optional(&args, 1),
            string(&args, 2)?,
        )),
        "resizeTerminal" => ignore_missing_session(crate::terminal::resize_client(
            app,
            string(&args, 0)?,
            optional(&args, 1),
            label,
            number(&args, 2, 120),
            number(&args, 3, 30),
        )),
        "closeTerminal" => ignore_missing_session(crate::terminal::close(
            app,
            label,
            string(&args, 0)?,
            optional(&args, 1),
        )),
        "focusTerminal" => {
            window.set_focus().map_err(err)?;
            Ok(Value::Null)
        }
        "openAiAccountLogin" => crate::terminal::open_login(
            app,
            label,
            string(&args, 0)?,
            string(&args, 1)?,
            number(&args, 2, 120),
            number(&args, 3, 30),
        )
        .or_else(|e| Ok(json!({"ok":false,"error":e}))),
        "writeAiAccountLogin" | "resizeAiAccountLogin" | "closeAiAccountLogin" => {
            let action = if method.starts_with("write") {
                "write"
            } else if method.starts_with("resize") {
                "resize"
            } else {
                "close"
            };
            ignore_missing_session(crate::terminal::login_action(
                app,
                label,
                string(&args, 0)?,
                action,
                optional(&args, 1).unwrap_or(""),
                number(&args, 1, 120),
                number(&args, 2, 30),
            ))
        }
        "writeSystemClipboard" => {
            arboard::Clipboard::new()
                .map_err(err)?
                .set_text(string(&args, 0)?)
                .map_err(err)?;
            Ok(Value::Null)
        }
        "readClipboardImage" => clipboard_image(),
        "pasteImageToRemoteTerminal" => paste_image(app, string(&args, 0)?, string(&args, 1)?)
            .or_else(|e| Ok(json!({"ok":false,"error":e}))),
        "focusExistingInternalRoute" => Ok(json!(focus_existing(app, window, string(&args, 0)?)?)),
        "openInternalRouteInNewWindow" => {
            open_route(app, string(&args, 0)?)?;
            Ok(Value::Null)
        }
        "createNewWindow" => {
            create_route_window(app, string(&args, 0)?)?;
            Ok(Value::Null)
        }
        "openExternal" => {
            let url = string(&args, 0)?;
            let parsed = url::Url::parse(url).map_err(err)?;
            if !matches!(
                parsed.scheme(),
                "https" | "http" | "mailto" | "vscode" | "cursor"
            ) {
                return Err("지원하지 않는 외부 URL입니다.".into());
            }
            tauri_plugin_opener::OpenerExt::opener(app)
                .open_url(url, None::<&str>)
                .map_err(err)?;
            Ok(Value::Null)
        }
        "closeCurrentWindow" => {
            window.close().map_err(err)?;
            Ok(Value::Null)
        }
        "startWindowDragging" => {
            window.start_dragging().map_err(err)?;
            Ok(Value::Null)
        }
        "toggleWindowMaximize" => {
            if window.is_maximized().map_err(err)? {
                window.unmaximize().map_err(err)?;
            } else {
                window.maximize().map_err(err)?;
            }
            Ok(Value::Null)
        }
        "notifyShortcutBindingsChanged" => {
            app.emit("kanvibe:shortcut-bindings-changed", ())
                .map_err(err)?;
            Ok(Value::Null)
        }
        "notifyShortcutCaptureChanged" => Ok(Value::Null),
        "logRendererError" => {
            eprintln!("[kanvibe renderer] {}", json!(args));
            Ok(Value::Null)
        }
        "consumePendingNotificationActivation" => notification(app, method, vec![json!(label)]),
        "activateNotification" => {
            let mut args = args;
            args.push(json!(label));
            notification(app, method, args)
        }
        "showNotification"
        | "listNotifications"
        | "markNotificationRead"
        | "markTaskNotificationsRead"
        | "markAllNotificationsRead" => notification(app, method, args),
        _ => Err(format!("알 수 없는 데스크톱 메서드: {method}")),
    }
}
fn clipboard_image() -> Result<Value, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(err)?;
    let image = match clipboard.get_image() {
        Ok(v) => v,
        Err(arboard::Error::ContentNotAvailable) => return Ok(Value::Null),
        Err(e) => return Err(err(e)),
    };
    let rgba = image::RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )
    .ok_or("잘못된 클립보드 이미지")?;
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(err)?;
    Ok(json!(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(bytes.into_inner())
    )))
}
fn paste_image(app: &AppHandle, id: &str, url: &str) -> Result<Value, String> {
    if !url.starts_with("data:image/") {
        return Err("잘못된 이미지 데이터입니다.".into());
    }
    let (_, payload) = url.split_once(',').ok_or("잘못된 이미지 데이터입니다.")?;
    let bytes = STANDARD.decode(payload).map_err(err)?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("이미지가 너무 큽니다.".into());
    }
    image::load_from_memory(&bytes).map_err(err)?;
    let task = crate::core::dispatch(app, "kanban", "getTaskById", vec![json!(id)])?;
    let host = task["sshHost"].as_str().ok_or("원격 태스크가 아닙니다.")?;
    let name = format!("kanvibe-paste-{}.png", uuid::Uuid::new_v4());
    let local = std::env::temp_dir().join(&name);
    std::fs::write(&local, bytes).map_err(err)?;
    let remote = format!("/tmp/{name}");
    let status = Command::new("scp")
        .args(["-q", "--"])
        .arg(&local)
        .arg(format!("{host}:{remote}"))
        .env_clear()
        .envs(shell_environment())
        .output();
    let _ = std::fs::remove_file(&local);
    let output = status.map_err(err)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(json!({"ok":true,"remotePath":remote}))
}
fn normalize_route(route: &str) -> Result<String, String> {
    let route = route.trim_start_matches('#');
    let route = route.strip_prefix("/#").unwrap_or(route);
    if !route.starts_with('/') || route.starts_with("//") || route.contains(['\r', '\n']) {
        return Err("잘못된 앱 경로입니다.".into());
    }
    Ok(route.into())
}
fn focus_existing(app: &AppHandle, current: &WebviewWindow, route: &str) -> Result<bool, String> {
    let route = normalize_route(route)?;
    for window in app.webview_windows().values() {
        if window.label() == current.label() {
            continue;
        }
        if window.url().map_err(err)?.fragment() == Some(route.as_str()) {
            window.unminimize().map_err(err)?;
            window.show().map_err(err)?;
            window.set_focus().map_err(err)?;
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn open_route(app: &AppHandle, route: &str) -> Result<String, String> {
    let route = normalize_route(route)?;
    for window in app.webview_windows().values() {
        if window.url().map_err(err)?.fragment() == Some(route.as_str()) {
            window.unminimize().map_err(err)?;
            window.show().map_err(err)?;
            window.set_focus().map_err(err)?;
            return Ok(window.label().to_owned());
        }
    }
    create_route_window(app, &route)
}
fn route_webview_url(route: &str, renderer: Option<&str>) -> Result<WebviewUrl, String> {
    let route = normalize_route(route)?;
    if let Some(renderer) = renderer {
        let mut url = url::Url::parse(renderer).map_err(err)?;
        url.set_fragment(Some(&route));
        Ok(WebviewUrl::External(url))
    } else {
        Ok(WebviewUrl::App(format!("index.html#{route}").into()))
    }
}
fn create_route_window(app: &AppHandle, route: &str) -> Result<String, String> {
    let label = format!("task-{}", uuid::Uuid::new_v4());
    let renderer = std::env::var("KANVIBE_RENDERER_URL").ok();
    let url = route_webview_url(route, renderer.as_deref())?;
    let builder = tauri::WebviewWindowBuilder::new(app, label.clone(), url)
        .title("KanVibe")
        .inner_size(1600., 1000.)
        .min_inner_size(640., 480.);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    builder.build().map_err(err)?;
    Ok(label)
}
fn read_notifications(app: &AppHandle) -> Result<Vec<Value>, String> {
    let raw = crate::core::dispatch(
        app,
        "appSettings",
        "getAppSetting",
        vec![json!("app_notifications")],
    )?;
    Ok(raw
        .as_str()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default())
}
fn save_notifications(app: &AppHandle, notifications: &[Value]) -> Result<(), String> {
    crate::core::dispatch(
        app,
        "appSettings",
        "setAppSetting",
        vec![
            json!("app_notifications"),
            json!(serde_json::to_string(notifications).map_err(err)?),
        ],
    )?;
    app.emit("kanvibe:notifications-changed", ()).map_err(err)
}
pub fn notification(app: &AppHandle, method: &str, args: Vec<Value>) -> Result<Value, String> {
    let state = app.state::<NativeState>();
    if method == "consumePendingNotificationActivation" {
        return Ok(state
            .pending_activation
            .lock()
            .map_err(err)?
            .remove(optional(&args, 0).unwrap_or("main"))
            .unwrap_or(Value::Null));
    }
    let _guard = state.notification_lock.lock().map_err(err)?;
    let mut entries = read_notifications(app)?;
    match method {
        "listNotifications" => Ok(json!(entries)),
        "showNotification" => {
            let payload = args.first().ok_or("알림이 필요합니다.")?;
            let locale = payload["locale"].as_str().unwrap_or("ko");
            let title = payload["title"].as_str().unwrap_or("");
            let body = payload["body"].as_str().unwrap_or("");
            let task = payload["taskId"].as_str();
            let dedupe = payload["dedupeKey"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| format!("{title}::{body}::{}::{locale}", task.unwrap_or("")));
            let now = chrono::Utc::now();
            if entries.iter().any(|v| {
                v["dedupeKey"] == dedupe
                    && v["createdAt"]
                        .as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .is_some_and(|t| {
                            (now - t.with_timezone(&chrono::Utc)).num_milliseconds() < 4000
                        })
            }) {
                return Ok(json!(true));
            }
            let route = task
                .map(|id| format!("/{locale}/task/{id}"))
                .unwrap_or_else(|| {
                    payload["relativePath"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| format!("/{locale}"))
                });
            let entry = json!({"id":uuid::Uuid::new_v4().to_string(),"title":title,"body":body,"taskId":task,"relativePath":route,"locale":locale,"isRead":false,"createdAt":now.to_rfc3339_opts(chrono::SecondsFormat::Millis,true),"dedupeKey":dedupe,"action":payload["action"]});
            entries.insert(0, entry);
            entries.truncate(100);
            save_notifications(app, &entries)?;
            let native_id = (uuid::Uuid::new_v4().as_u128() & 0x7fffffff) as i32;
            {
                let mut ids = state.notification_ids.lock().map_err(err)?;
                ids.retain(|_, id| entries.iter().any(|entry| entry["id"] == *id));
                ids.insert(
                    native_id,
                    entries[0]["id"].as_str().unwrap_or("").to_owned(),
                );
            }
            app.notification()
                .builder()
                .id(native_id)
                .title(title)
                .body(body)
                .show()
                .map_err(err)?;
            Ok(json!(true))
        }
        "markNotificationRead" | "markTaskNotificationsRead" | "markAllNotificationsRead" => {
            let id = optional(&args, 0).unwrap_or("");
            let mut count = 0;
            let mut selected = Value::Null;
            for entry in &mut entries {
                let matches = method == "markAllNotificationsRead"
                    || if method == "markNotificationRead" {
                        entry["id"] == id
                    } else {
                        entry["taskId"] == id
                    };
                if matches {
                    if entry["isRead"] != true {
                        entry["isRead"] = json!(true);
                        count += 1;
                    }
                    selected = entry.clone();
                }
            }
            if count > 0 {
                save_notifications(app, &entries)?;
            }
            Ok(if method == "markNotificationRead" {
                selected
            } else if method == "markTaskNotificationsRead" {
                json!(count)
            } else {
                Value::Null
            })
        }
        "activateNotification" => {
            let id = string(&args, 0)?;
            let Some(entry) = entries.iter_mut().find(|v| v["id"] == id) else {
                return Ok(json!(false));
            };
            entry["isRead"] = json!(true);
            let entry = entry.clone();
            save_notifications(app, &entries)?;
            let (target, queue) = if entry["action"]["type"] == "background-sync-review" {
                let preferred = optional(&args, 1).and_then(|label| app.get_webview_window(label));
                let window = preferred
                    .or_else(|| {
                        app.webview_windows()
                            .into_values()
                            .find(|w| w.is_focused().unwrap_or(false))
                    })
                    .or_else(|| app.get_webview_window("main"))
                    .ok_or("알림을 표시할 창이 없습니다.")?;
                window.set_focus().map_err(err)?;
                (window.label().to_owned(), false)
            } else {
                let route = entry["relativePath"]
                    .as_str()
                    .ok_or("알림 경로가 없습니다.")?;
                let exists = app.webview_windows().values().any(|w| {
                    w.url()
                        .ok()
                        .and_then(|u| u.fragment().map(String::from))
                        .as_deref()
                        == Some(route)
                });
                (open_route(app, route)?, !exists)
            };
            if queue {
                state
                    .pending_activation
                    .lock()
                    .map_err(err)?
                    .insert(target.clone(), entry.clone());
            }
            app.emit_to(&target, "kanvibe:notification-activated", entry)
                .map_err(err)?;
            Ok(json!(true))
        }
        _ => Err(format!("알 수 없는 알림 메서드: {method}")),
    }
}
pub fn dependency_dispatch(
    namespace: &str,
    method: &str,
    args: Vec<Value>,
) -> Result<Value, String> {
    crate::dependencies::dispatch(namespace, method, args)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_terminal_session_is_not_a_renderer_error() {
        let missing = Err(crate::terminal::SESSION_NOT_FOUND.to_string());
        assert_eq!(ignore_missing_session(missing), Ok(Value::Null));
        assert_eq!(ignore_missing_session(Ok(())), Ok(Value::Null));
        assert_eq!(
            ignore_missing_session(Err("write failed".into())),
            Err("write failed".to_string())
        );
    }
    #[test]
    fn quotes_do_not_expand_shell_metacharacters() {
        assert_eq!(quote("a'$(echo b)"), "'a'\\''$(echo b)'");
    }
    #[test]
    fn runtime_variables_never_enter_shells() {
        let vars = shell_environment();
        assert!(!vars.contains_key("PORT"));
        assert!(!vars.contains_key("HOST"));
        assert!(!vars.contains_key("NODE_ENV"));
        assert!(!vars.keys().any(|k| k.starts_with("KANVIBE_")));
        assert!(vars.contains_key("HOME"));
        assert!(vars.contains_key("PATH"));
    }
    #[test]
    fn internal_routes_reject_external_targets() {
        assert!(normalize_route("https://example.com").is_err());
        assert!(normalize_route("//example.com").is_err());
        assert!(normalize_route("/ko/task/a\r\n").is_err());
        assert_eq!(normalize_route("#/ko/task/a").unwrap(), "/ko/task/a");
    }
    #[test]
    fn new_window_urls_use_tauri_app_resolution_for_dev_and_bundled_frontends() {
        for route in ["/", "/ko/settings", "/ko/task/a?tab=terminal"] {
            let WebviewUrl::App(path) = route_webview_url(route, None).unwrap() else {
                panic!("새 창은 Tauri App URL을 사용해야 합니다.");
            };
            for base in ["http://127.0.0.1:5173/", "tauri://localhost/"] {
                let base = url::Url::parse(base).unwrap();
                let resolved = base.join(&path.to_string_lossy()).unwrap();
                assert_eq!(resolved.scheme(), base.scheme());
                assert_eq!(resolved.host_str(), base.host_str());
                assert_eq!(resolved.port(), base.port());
                assert_eq!(resolved.fragment(), Some(route));
                assert_eq!(resolved.path(), "/index.html");
            }
        }
    }
    #[test]
    fn renderer_override_preserves_entry_path_and_query_when_opening_routes() {
        let WebviewUrl::External(url) = route_webview_url(
            "/ko/task/a",
            Some("http://localhost:4321/preview/index.html?mode=qa#/old"),
        )
        .unwrap() else {
            panic!("명시한 renderer URL을 사용해야 합니다.");
        };
        assert_eq!(
            url.as_str(),
            "http://localhost:4321/preview/index.html?mode=qa#/ko/task/a"
        );
        assert!(route_webview_url("//example.com", None).is_err());
        assert!(route_webview_url("/", Some("invalid URL")).is_err());
    }
    #[test]
    fn process_stdin_and_deadline_share_one_executor() {
        assert_eq!(
            run_shell_input(
                "cat",
                None,
                Some("한글\n".as_bytes()),
                Duration::from_secs(2)
            )
            .unwrap(),
            "한글\n"
        );
        let start = Instant::now();
        assert!(run_shell_timeout("sleep 5", None, Duration::from_millis(40)).is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod lifecycle_regressions {
    use super::*;
    #[test]
    fn closing_last_window_prevents_one_automatic_exit_but_not_native_or_programmatic_quit() {
        let state = NativeState::default();
        assert!(!state.take_last_window_close(None));
        state
            .last_window_closing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(state.take_last_window_close(None));
        assert!(!state.take_last_window_close(None));
        state
            .last_window_closing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!state.take_last_window_close(Some(0)));
        assert!(!state.take_last_window_close(None));
    }
}
