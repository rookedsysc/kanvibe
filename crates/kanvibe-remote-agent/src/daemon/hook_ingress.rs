//! `hooks-v1.sock`에서 원격 hook·task CLI의 HTTP 요청을 받아 붙어 있는 데스크탑에 넘긴다(TS `serveHook`).
//!
//! 브라우저 요청(`Origin`)은 헤더를 보기 전에 403 JSON으로 막고, 허용한 8개 경로만 받는다. client id로 데스크탑을
//! 찾아 `hook.request`로 넘기고 `hook.response`를 그대로 돌려준다. 데스크탑이 없거나 끊기면 503으로 답하고
//! 요청을 저장했다가 다시 보내지 않는다. 본문은 512 KiB, 데스크탑 응답은 120초가 상한이다.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use serde_json::json;

use super::ServerShared;
use super::peer::{HookReply, MAX_ACTIVE_REQUESTS, MAX_HOOK_BYTES, Peer};
use crate::protocol::{
    encode_host_bytes, filter_hook_headers, is_valid_host_identifier, record_from,
};

const HOOK_PATHS: [&str; 6] = [
    "/api/hooks/status",
    "/api/cli/tasks",
    "/api/cli/tasks/delete",
    "/api/cli/tasks/register",
    "/api/cli/tasks/list",
    "/api/cli/tasks/session",
];
const HOOK_READ_PATHS: [&str; 2] = ["/api/hooks/health", "/api/install/hooks.sh"];
const HOOK_RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);
const HEADERS_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_HEADER_COUNT: usize = 32;

struct HookRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    /// 헤더 뒤에 함께 읽힌 본문 앞부분
    body_prefix: Vec<u8>,
}

struct HookResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HookResponse {
    fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }
}

/// 연결 하나를 처리하고 닫는다
pub(super) fn serve_hook_connection(shared: Arc<ServerShared>, mut stream: UnixStream) {
    let started_at = Instant::now();
    let _ = stream.set_read_timeout(Some(HEADERS_TIMEOUT));
    let Some(request) = read_head(&mut stream) else {
        return;
    };
    let response = route(&shared, &mut stream, request, started_at);
    let _ = write_response(&mut stream, &response);
}

fn route(
    shared: &ServerShared,
    stream: &mut UnixStream,
    request: HookRequest,
    started_at: Instant,
) -> HookResponse {
    if header(&request, "origin").is_some() {
        let body =
            json!({ "success": false, "error": "브라우저 요청은 허용하지 않습니다" }).to_string();
        return HookResponse {
            status: 403,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.into_bytes(),
        };
    }
    let is_allowed = (request.method == "POST" && HOOK_PATHS.contains(&request.path.as_str()))
        || (request.method == "GET" && HOOK_READ_PATHS.contains(&request.path.as_str()));
    if !is_allowed {
        return HookResponse::text(403, "Hook role cannot perform this operation");
    }
    let client_id = header(&request, "x-kanvibe-client-id")
        .unwrap_or_default()
        .to_string();
    if !is_valid_host_identifier(&client_id) {
        return HookResponse::text(400, "Invalid hook client");
    }
    let peer = shared.peers.lock().unwrap().get(&client_id).cloned();
    let Some(peer) = peer.filter(|peer| !peer.outbox.is_closed()) else {
        return HookResponse::text(503, "Desktop offline; request not replayed");
    };
    if peer.hooks.lock().unwrap().len() >= MAX_ACTIVE_REQUESTS {
        return HookResponse::text(503, "Hook capacity exceeded");
    }
    let _ = stream.set_read_timeout(Some(
        REQUEST_TIMEOUT
            .saturating_sub(started_at.elapsed())
            .max(Duration::from_millis(1)),
    ));
    let body = match read_body(stream, &request) {
        Ok(body) => body,
        Err(response) => return response,
    };
    forward_to_desktop(&peer, &request, body)
}

fn forward_to_desktop(peer: &Arc<Peer>, request: &HookRequest, body: Vec<u8>) -> HookResponse {
    if peer.outbox.is_closed() {
        return HookResponse::text(503, "Desktop disconnected; request not replayed");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (reply_sender, replies) = channel();
    peer.hooks.lock().unwrap().insert(id.clone(), reply_sender);
    let headers = filter_hook_headers(
        request
            .headers
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    let record = json!({
        "type": "hook.request", "id": id, "method": request.method, "path": request.path,
        "headers": headers, "body": encode_host_bytes(&body),
    });
    if !peer_send(peer, record) {
        peer.hooks.lock().unwrap().remove(&id);
        return HookResponse::text(503, "Desktop disconnected; request not replayed");
    }
    let reply = replies.recv_timeout(HOOK_RESPONSE_TIMEOUT);
    peer.hooks.lock().unwrap().remove(&id);
    match reply {
        Ok(HookReply::Response {
            status,
            headers,
            body,
        }) => HookResponse {
            status,
            headers,
            body,
        },
        Err(RecvTimeoutError::Timeout) => {
            HookResponse::text(504, "Desktop response timed out; request not replayed")
        }
        Ok(HookReply::Disconnected) | Err(RecvTimeoutError::Disconnected) => {
            HookResponse::text(503, "Desktop disconnected; request not replayed")
        }
    }
}

fn peer_send(peer: &Arc<Peer>, record: serde_json::Value) -> bool {
    peer.outbox.send(&record_from(record)).is_ok()
}

fn header<'request>(request: &'request HookRequest, name: &str) -> Option<&'request str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// 요청 줄과 헤더를 읽는다. 형식이 틀리거나 시간이 다 되면 답하지 않고 닫는다
fn read_head(stream: &mut UnixStream) -> Option<HookRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = stream.read(&mut chunk).ok().filter(|count| *count > 0)?;
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(request) = parse_head(&buffer) {
            return Some(request);
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return None;
        }
    }
}

fn parse_head(buffer: &[u8]) -> Option<HookRequest> {
    let mut slots = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Request::new(&mut slots);
    let httparse::Status::Complete(head_length) = parsed.parse(buffer).ok()? else {
        return None;
    };
    let headers = parsed
        .headers
        .iter()
        .take(MAX_HEADER_COUNT)
        .filter_map(|entry| {
            Some((
                entry.name.to_ascii_lowercase(),
                std::str::from_utf8(entry.value).ok()?.to_string(),
            ))
        })
        .collect();
    Some(HookRequest {
        method: parsed.method?.to_string(),
        path: parsed.path?.to_string(),
        headers,
        body_prefix: buffer[head_length..].to_vec(),
    })
}

/// 본문을 읽는다. 512 KiB를 넘으면 413으로 답한다
fn read_body(stream: &mut UnixStream, request: &HookRequest) -> Result<Vec<u8>, HookResponse> {
    let too_large = || HookResponse::text(413, "Hook body size exceeded");
    let is_chunked = header(request, "transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
    if is_chunked {
        return read_chunked_body(stream, request.body_prefix.clone()).ok_or_else(too_large);
    }
    let length: usize = header(request, "content-length")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);
    if length > MAX_HOOK_BYTES {
        return Err(too_large());
    }
    let mut body = request.body_prefix.clone();
    body.truncate(length);
    while body.len() < length {
        let mut chunk = vec![0; length - body.len()];
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(HookResponse::text(400, "Hook body incomplete")),
            Ok(count) => body.extend_from_slice(&chunk[..count]),
        }
    }
    Ok(body)
}

/// `Transfer-Encoding: chunked` 본문. 상한을 넘거나 형식이 틀리면 `None`이다
fn read_chunked_body(stream: &mut UnixStream, mut raw: Vec<u8>) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    let mut offset = 0;
    loop {
        let line_end = loop {
            if let Some(position) = raw[offset..].windows(2).position(|pair| pair == b"\r\n") {
                break offset + position;
            }
            read_more(stream, &mut raw)?;
        };
        let size_text = std::str::from_utf8(&raw[offset..line_end]).ok()?;
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        if body.len() + size > MAX_HOOK_BYTES {
            return None;
        }
        while raw.len() < line_end + 2 + size + 2 {
            read_more(stream, &mut raw)?;
        }
        if size == 0 {
            return Some(body);
        }
        body.extend_from_slice(&raw[line_end + 2..line_end + 2 + size]);
        offset = line_end + 2 + size + 2;
    }
}

fn read_more(stream: &mut UnixStream, raw: &mut Vec<u8>) -> Option<()> {
    let mut chunk = [0u8; 8192];
    let count = stream.read(&mut chunk).ok().filter(|count| *count > 0)?;
    raw.extend_from_slice(&chunk[..count]);
    (raw.len() <= MAX_HOOK_BYTES * 2).then_some(())
}

fn write_response(stream: &mut UnixStream, response: &HookResponse) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason_phrase(response.status)
    );
    for (key, value) in &response.headers {
        head.push_str(&format!("{key}: {value}\r\n"));
    }
    head.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        response.body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}
