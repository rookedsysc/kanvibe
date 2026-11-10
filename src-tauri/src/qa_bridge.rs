//! 로컬 QA 실행에서만 사용하는 인증된 WebView 검증 통로.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Mutex, OnceLock},
    time::Duration,
};
use tauri::{AppHandle, Manager};
type Evaluation = Result<Value, String>;
static PENDING: OnceLock<Mutex<HashMap<String, mpsc::Sender<Evaluation>>>> = OnceLock::new();
fn pending() -> &'static Mutex<HashMap<String, mpsc::Sender<Evaluation>>> {
    PENDING.get_or_init(Mutex::default)
}
#[tauri::command]
pub fn qa_result(id: String, value: Option<Value>, error: Option<String>) {
    if let Ok(mut map) = pending().lock() {
        if let Some(tx) = map.remove(&id) {
            let _ = tx.send(match error {
                Some(e) => Err(e),
                None => Ok(value.unwrap_or(Value::Null)),
            });
        }
    }
}
pub fn start(app: AppHandle) -> Result<(), String> {
    if std::env::var("KANVIBE_QA_MODE").as_deref() != Ok("1") {
        return Ok(());
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    let token = uuid::Uuid::new_v4().to_string();
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let path = super::runtime::database_path(&app)?
        .parent()
        .ok_or("QA data directory missing")?
        .join("qa-bridge.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    write!(
        file,
        "{}",
        json!({"port":port,"token":token,"pid":std::process::id()})
    )
    .map_err(|e| e.to_string())?;
    eprintln!("[kanvibe qa] bridge metadata: {}", path.display());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                break;
            };
            let app = app.clone();
            let token = token.clone();
            std::thread::spawn(move || {
                let _ = handle(stream, &app, &token);
            });
        }
    });
    Ok(())
}
fn reply(stream: &mut TcpStream, status: &str, value: Value) -> Result<(), String> {
    let body = value.to_string();
    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).map_err(|e|e.to_string())
}
fn handle(mut stream: TcpStream, app: &AppHandle, token: &str) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("HTTP disconnected".into());
        }
        bytes.extend_from_slice(&buffer[..n]);
        if let Some(pos) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
        if bytes.len() > 16384 {
            return reply(
                &mut stream,
                "431 Request Header Fields Too Large",
                json!({"error":"headers too large"}),
            );
        }
    }
    let headers = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
    let auth = headers.lines().skip(1).find_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(_, v)| v.trim())
    });
    if auth != Some(format!("Bearer {token}").as_str()) {
        return reply(
            &mut stream,
            "401 Unauthorized",
            json!({"error":"unauthorized"}),
        );
    }
    if headers.starts_with("GET /health ") {
        return reply(&mut stream, "200 OK", json!({"ok":true}));
    }
    if !headers.starts_with("POST /eval ") {
        return reply(&mut stream, "404 Not Found", json!({"error":"not found"}));
    }
    let length = headers
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        })
        .ok_or("Content-Length required")?;
    if length > 1024 * 1024 {
        return reply(
            &mut stream,
            "413 Content Too Large",
            json!({"error":"body too large"}),
        );
    }
    while bytes.len() < header_end + length {
        let n = stream.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("HTTP body disconnected".into());
        }
        bytes.extend_from_slice(&buffer[..n]);
    }
    let request: Value = serde_json::from_slice(&bytes[header_end..header_end + length])
        .map_err(|e| e.to_string())?;
    let script = request["script"].as_str().ok_or("script required")?;
    let label = request["window"].as_str().unwrap_or("main");
    let Some(window) = app.get_webview_window(label) else {
        return reply(
            &mut stream,
            "404 Not Found",
            json!({"error":"window not found"}),
        );
    };
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel();
    pending()
        .lock()
        .map_err(|e| e.to_string())?
        .insert(id.clone(), tx);
    let js=format!("(async()=>{{try{{const value=await(async()=>{{{script}\n}})();await window.__TAURI_INTERNALS__.invoke('qa_result',{{id:{id},value:value===undefined?null:JSON.parse(JSON.stringify(value)),error:null}});}}catch(e){{await window.__TAURI_INTERNALS__.invoke('qa_result',{{id:{id},value:null,error:String(e?.stack||e)}});}}}})();",id=json!(id));
    if let Err(e) = window.eval(&js) {
        pending().lock().map_err(|e| e.to_string())?.remove(&id);
        return reply(
            &mut stream,
            "500 Internal Server Error",
            json!({"error":e.to_string()}),
        );
    }
    let result = rx.recv_timeout(Duration::from_secs(45));
    pending().lock().map_err(|e| e.to_string())?.remove(&id);
    match result {
        Ok(Ok(value)) => reply(&mut stream, "200 OK", json!({"value":value})),
        Ok(Err(error)) => reply(
            &mut stream,
            "422 Unprocessable Content",
            json!({"error":error}),
        ),
        Err(error) => reply(
            &mut stream,
            "504 Gateway Timeout",
            json!({"error":error.to_string()}),
        ),
    }
}

#[tauri::command]
pub async fn qa_resize(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if !(100.0..=4096.0).contains(&width) || !(100.0..=4096.0).contains(&height) {
        return Err("invalid QA viewport size".into());
    }
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn qa_snapshot(window: tauri::WebviewWindow) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        use base64::Engine;
        use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
        use objc2_foundation::{NSDictionary, NSError};
        use objc2_web_kit::WKWebView;
        let (sender, receiver) = mpsc::channel::<Result<String, String>>();
        window
            .with_webview(move |webview| {
                // Tauri는 이 콜백을 WebView 소유 스레드에서 실행한다. WKWebView의 비동기 스냅샷만 읽는다.
                let view = unsafe { &*(webview.inner() as *const WKWebView) };
                let callback =
                    block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                        let result = (|| -> Result<String, String> {
                            if image.is_null() {
                                return Err(if error.is_null() {
                                    "WKWebView returned no snapshot".into()
                                } else {
                                    unsafe { &*error }.to_string()
                                });
                            }
                            let image = unsafe { &*image };
                            let tiff = image
                                .TIFFRepresentation()
                                .ok_or("snapshot TIFF conversion failed")?;
                            let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)
                                .ok_or("snapshot bitmap conversion failed")?;
                            let png = unsafe {
                                bitmap.representationUsingType_properties(
                                    NSBitmapImageFileType::PNG,
                                    &NSDictionary::new(),
                                )
                            }
                            .ok_or("snapshot PNG conversion failed")?;
                            Ok(base64::engine::general_purpose::STANDARD.encode(png.to_vec()))
                        })();
                        let _ = sender.send(result);
                    });
                unsafe {
                    view.takeSnapshotWithConfiguration_completionHandler(None, &callback);
                }
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || {
            receiver
                .recv_timeout(Duration::from_secs(30))
                .map_err(|e| e.to_string())?
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("native snapshot is implemented for macOS QA".into())
    }
}
