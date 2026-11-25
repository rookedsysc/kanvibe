//! daemon이 넘긴 hook 요청을 터널의 로컬 소켓에 HTTP/1.1로 다시 보낸다(TS `forwardHostHookRequest`).
//!
//! 터널 소켓을 듣는 서버가 평문 포트와 같은 처리기로 답하므로 요청을 그대로 옮기고 응답을 그대로 돌려준다.
//! 상태 hook은 금방 끝나므로 12초, task CLI는 원격 worktree 준비까지 기다리므로 110초 동안 응답이 없으면 포기한다.
//! 110초는 원격 daemon이 데스크탑을 기다리는 상한(120초)보다 짧아 원격이 먼저 끊어 결과를 잃지 않는다.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use kanvibe_remote_agent::protocol::HOST_RECORD_BYTES;

use super::boundaries::{HostHookRequest, HostHookResponse};

const HOOK_FORWARD_IDLE_TIMEOUT: Duration = Duration::from_millis(12_000);
const TASK_CLI_FORWARD_IDLE_TIMEOUT: Duration = Duration::from_millis(110_000);
/// 요청 본문과 응답 본문의 상한. 레코드 하나에 base64로 실어야 한다
const MAX_REPLAY_BYTES: usize = HOST_RECORD_BYTES / 2;
const MAX_HEADER_BYTES: usize = 64 * 1024;

pub fn replay_hook_request(
    socket_path: &Path,
    request: &HostHookRequest,
) -> Result<HostHookResponse, String> {
    if request.body.len() > MAX_REPLAY_BYTES {
        return Err("Invalid host bytes size".into());
    }
    let idle_timeout = match request.path.starts_with("/api/cli/") {
        true => TASK_CLI_FORWARD_IDLE_TIMEOUT,
        false => HOOK_FORWARD_IDLE_TIMEOUT,
    };
    let mut stream = UnixStream::connect(socket_path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(idle_timeout))
        .and_then(|()| stream.set_write_timeout(Some(idle_timeout)))
        .map_err(|error| error.to_string())?;
    write_request(&mut stream, request).map_err(|error| timeout_message(&error))?;
    read_response(&mut stream)
}

fn timeout_message(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            "Hook request timed out".into()
        }
        _ => error.to_string(),
    }
}

/// 연결이 바꾸는 헤더(길이, 연결 방식)는 다시 쓴다
fn write_request(stream: &mut UnixStream, request: &HostHookRequest) -> std::io::Result<()> {
    let mut head = format!(
        "{} {} HTTP/1.1\r\nhost: localhost\r\n",
        request.method, request.path
    );
    let rewritten = ["host", "content-length", "connection", "transfer-encoding"];
    for (key, value) in &request.headers {
        if !rewritten.contains(&key.to_ascii_lowercase().as_str()) {
            head.push_str(&format!("{key}: {value}\r\n"));
        }
    }
    head.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        request.body.len()
    ));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&request.body)?;
    stream.flush()
}

struct ResponseHead {
    status: u16,
    headers: Vec<(String, String)>,
    body_prefix: Vec<u8>,
}

fn read_response(stream: &mut UnixStream) -> Result<HostHookResponse, String> {
    let head = read_head(stream)?;
    let header = |name: &str| {
        head.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let is_chunked =
        header("transfer-encoding").is_some_and(|value| value.eq_ignore_ascii_case("chunked"));
    let content_length = header("content-length").and_then(|value| value.trim().parse().ok());
    let raw = read_until_end(stream, head.body_prefix.clone(), content_length)?;
    let body = match is_chunked {
        true => decode_chunked(&raw)?,
        false => raw,
    };
    if body.len() > MAX_REPLAY_BYTES {
        return Err("Hook response too large".into());
    }
    Ok(HostHookResponse {
        status: head.status,
        headers: head.headers,
        body,
    })
}

fn read_head(stream: &mut UnixStream) -> Result<ResponseHead, String> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = stream
            .read(&mut chunk)
            .map_err(|error| timeout_message(&error))?;
        if count == 0 {
            return Err("Hook response ended before headers".into());
        }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(head) = parse_head(&buffer)? {
            return Ok(head);
        }
        if buffer.len() > MAX_HEADER_BYTES {
            return Err("Hook response headers too large".into());
        }
    }
}

fn parse_head(buffer: &[u8]) -> Result<Option<ResponseHead>, String> {
    let mut slots = [httparse::EMPTY_HEADER; 64];
    let mut parsed = httparse::Response::new(&mut slots);
    let status = parsed.parse(buffer).map_err(|error| error.to_string())?;
    let httparse::Status::Complete(head_length) = status else {
        return Ok(None);
    };
    let headers = parsed
        .headers
        .iter()
        .filter_map(|entry| {
            let value = std::str::from_utf8(entry.value).ok()?;
            Some((entry.name.to_ascii_lowercase(), value.to_string()))
        })
        .collect();
    Ok(Some(ResponseHead {
        status: parsed.code.unwrap_or(502),
        headers,
        body_prefix: buffer[head_length..].to_vec(),
    }))
}

/// 길이를 알면 그만큼, 모르면 연결이 닫힐 때까지 읽는다. 상한을 넘으면 그만 읽는다
fn read_until_end(
    stream: &mut UnixStream,
    mut raw: Vec<u8>,
    content_length: Option<usize>,
) -> Result<Vec<u8>, String> {
    let limit = MAX_REPLAY_BYTES * 2;
    let mut chunk = [0u8; 16 * 1024];
    while content_length.is_none_or(|length| raw.len() < length) {
        if raw.len() > limit {
            return Err("Hook response too large".into());
        }
        let count = stream
            .read(&mut chunk)
            .map_err(|error| timeout_message(&error))?;
        if count == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..count]);
    }
    if let Some(length) = content_length {
        raw.truncate(length);
    }
    Ok(raw)
}

fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>, String> {
    let invalid = || "Invalid chunked hook response".to_string();
    let mut body = Vec::new();
    let mut offset = 0;
    loop {
        let line_length = raw[offset..]
            .windows(2)
            .position(|pair| pair == b"\r\n")
            .ok_or_else(invalid)?;
        let size_text =
            std::str::from_utf8(&raw[offset..offset + line_length]).map_err(|_| invalid())?;
        let size_text = size_text.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| invalid())?;
        let start = offset + line_length + 2;
        if size == 0 {
            return Ok(body);
        }
        let end = start + size;
        body.extend_from_slice(raw.get(start..end).ok_or_else(invalid)?);
        offset = end + 2;
    }
}
