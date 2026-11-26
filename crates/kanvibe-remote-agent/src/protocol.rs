//! 데스크탑과 원격 host daemon이 주고받는 프레이밍(TS `remoteHostProtocol.ts`).
//!
//! 레코드 하나는 4바이트 big-endian 길이 뒤에 JSON 객체 본문이 붙는다. 본문은 1 MiB를 넘지 못하고,
//! 바이트 데이터는 32 KiB 조각마다 정규 base64로 싣는다. 레코드 종류(`exec.*`, `pty.*`, `socket.*`,
//! `hook.request`/`hook.response`, `status`, `stop`)와 필드 이름은 Node daemon과 같다.
//!
//! 버전 핸드셰이크: Rust daemon은 `hello`/`ready`의 `version`을 [`HOST_PROTOCOL_VERSION`](2)으로 올렸다.
//! 프레이밍과 레코드는 그대로지만, 2는 "바이너리로 올린 Rust daemon"이라는 뜻이다. Node daemon(1)은
//! `version`과 `buildHash`가 다른 `hello`를 `error` 레코드로 거절하므로, 두 쪽이 기대가 어긋난 채 대화하지 않는다.
//! 이전 빌드 daemon을 내리는 일은 bridge가 한다(`daemon::bridge` 참조).

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value};

/// 프레이밍과 레코드는 Node daemon(1)과 같고, 바이너리 daemon이라 2로 올렸다
pub const HOST_PROTOCOL_VERSION: u64 = 2;
pub const HOST_RECORD_BYTES: usize = 1024 * 1024;
pub const HOST_CHUNK_BYTES: usize = 32 * 1024;
pub const HOST_BUFFER_BYTES: usize = 8 * 1024 * 1024;
/// 같은 client id의 기존 데스크탑이 ping에 답해 새 연결을 거절했다는 `error` 레코드의 `code`
pub const HOST_CLIENT_ALREADY_CONNECTED: &str = "client-already-connected";

/// JSON 객체 하나. `type` 필드는 언제나 문자열이다
pub type HostRecord = Map<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostWireError {
    #[error("Host record size exceeded")]
    RecordSizeExceeded,
    #[error("Invalid host record")]
    InvalidRecord,
    #[error("Invalid host bytes")]
    InvalidBytes,
    #[error("Invalid host bytes size")]
    InvalidBytesSize,
    #[error("Invalid host namespace or uid")]
    InvalidNamespace,
}

/// `dev`, `prod`, `qa-<소문자·숫자·하이픈 1~24자>`만 namespace로 받는다
pub fn is_valid_host_namespace(namespace: &str) -> bool {
    if namespace == "dev" || namespace == "prod" {
        return true;
    }
    let Some(suffix) = namespace.strip_prefix("qa-") else {
        return false;
    };
    (1..=24).contains(&suffix.len())
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// 요청 id와 hook client id 형식(`^[A-Za-z0-9-]{1,128}$`)
pub fn is_valid_host_identifier(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// namespace 하나가 쓰는 비공개 디렉터리와 두 소켓 경로
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSocketPaths {
    pub directory: String,
    pub rpc_socket_path: String,
    pub hook_socket_path: String,
}

pub fn build_host_socket_paths(
    namespace: &str,
    uid: u32,
) -> Result<HostSocketPaths, HostWireError> {
    if !is_valid_host_namespace(namespace) {
        return Err(HostWireError::InvalidNamespace);
    }
    Ok(host_socket_paths_in(&format!("/tmp/kv-{uid}"), namespace))
}

/// `parent/<namespace>` 아래 소켓 경로. 테스트는 /tmp/kv-<uid> 대신 임시 루트를 준다
pub fn host_socket_paths_in(parent: &str, namespace: &str) -> HostSocketPaths {
    let directory = format!("{}/{namespace}", parent.trim_end_matches('/'));
    HostSocketPaths {
        rpc_socket_path: format!("{directory}/rpc-v1.sock"),
        hook_socket_path: format!("{directory}/hooks-v1.sock"),
        directory,
    }
}

/// 레코드를 길이 접두어와 JSON 본문으로 만든다. 본문이 비거나 1 MiB를 넘으면 거절한다
pub fn encode_host_record(record: &HostRecord) -> Result<Vec<u8>, HostWireError> {
    let body = serde_json::to_vec(record).map_err(|_| HostWireError::InvalidRecord)?;
    if body.is_empty() || body.len() > HOST_RECORD_BYTES {
        return Err(HostWireError::RecordSizeExceeded);
    }
    let mut bytes = Vec::with_capacity(body.len() + 4);
    bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&body);
    Ok(bytes)
}

/// `serde_json::json!` 객체를 레코드로 바꾼다. 객체가 아니면 빈 레코드가 되어 인코딩에서 걸러진다
pub fn record_from(value: Value) -> HostRecord {
    match value {
        Value::Object(record) => record,
        _ => HostRecord::new(),
    }
}

/// 조각으로 들어오는 바이트를 레코드로 다시 묶는다(TS `createHostRecordDecoder`)
#[derive(Default)]
pub struct HostRecordDecoder {
    prefix: [u8; 4],
    prefix_bytes: usize,
    expected: usize,
    body: Vec<u8>,
}

impl HostRecordDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 완성된 레코드를 들어온 순서대로 돌려준다. 오류가 나면 스트림 전체가 신뢰할 수 없다
    pub fn push(&mut self, mut chunk: &[u8]) -> Result<Vec<HostRecord>, HostWireError> {
        let mut records = Vec::new();
        while !chunk.is_empty() {
            if self.prefix_bytes < 4 {
                let count = (4 - self.prefix_bytes).min(chunk.len());
                self.prefix[self.prefix_bytes..self.prefix_bytes + count]
                    .copy_from_slice(&chunk[..count]);
                self.prefix_bytes += count;
                chunk = &chunk[count..];
                if self.prefix_bytes < 4 {
                    continue;
                }
                self.expected = u32::from_be_bytes(self.prefix) as usize;
                if self.expected == 0 || self.expected > HOST_RECORD_BYTES {
                    return Err(HostWireError::RecordSizeExceeded);
                }
                self.body = Vec::with_capacity(self.expected);
            }
            let count = (self.expected - self.body.len()).min(chunk.len());
            self.body.extend_from_slice(&chunk[..count]);
            chunk = &chunk[count..];
            if self.body.len() == self.expected {
                records.push(parse_record_body(&self.body)?);
                self.prefix_bytes = 0;
                self.body = Vec::new();
            }
        }
        Ok(records)
    }
}

fn parse_record_body(body: &[u8]) -> Result<HostRecord, HostWireError> {
    let text = std::str::from_utf8(body).map_err(|_| HostWireError::InvalidRecord)?;
    let value: Value = serde_json::from_str(text).map_err(|_| HostWireError::InvalidRecord)?;
    match value {
        Value::Object(record) if record.get("type").is_some_and(Value::is_string) => Ok(record),
        _ => Err(HostWireError::InvalidRecord),
    }
}

/// 정규 base64 문자열만 받아 `maximum` 바이트 이하로 디코딩한다(TS `decodeHostBytes`)
pub fn decode_host_bytes(
    encoded: Option<&Value>,
    maximum: usize,
) -> Result<Vec<u8>, HostWireError> {
    let Some(Value::String(encoded)) = encoded else {
        return Err(HostWireError::InvalidBytes);
    };
    if encoded.len() > maximum.div_ceil(3) * 4 {
        return Err(HostWireError::InvalidBytes);
    }
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| HostWireError::InvalidBytes)?;
    if bytes.len() > maximum || BASE64.encode(&bytes) != *encoded {
        return Err(HostWireError::InvalidBytesSize);
    }
    Ok(bytes)
}

pub fn encode_host_bytes(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

/// 레코드의 문자열 필드
pub fn record_text<'record>(record: &'record HostRecord, field: &str) -> Option<&'record str> {
    record.get(field).and_then(Value::as_str)
}

/// JS `Number.isSafeInteger`를 통과하는 정수 필드
pub fn record_integer(record: &HostRecord, field: &str) -> Option<i64> {
    const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;
    let value = record.get(field)?;
    let integer = value.as_i64().or_else(|| {
        value
            .as_f64()
            .filter(|number| number.fract() == 0.0)
            .map(|number| number as i64)
    })?;
    (integer.abs() <= MAX_SAFE_INTEGER).then_some(integer)
}

/// 문자열 base64로 실을 수 있는 가장 긴 명령 텍스트(TS `HOST_RECORD_BYTES / 2`)
pub const MAX_COMMAND_TEXT_BYTES: usize = HOST_RECORD_BYTES / 2;

/// hook 요청·응답 헤더 가운데 넘기는 이름
pub const FORWARDED_HOOK_HEADERS: [&str; 4] = [
    "content-type",
    "authorization",
    "x-kanvibe-client-id",
    "x-kanvibe-install-token",
];

/// 넘기는 헤더만 남긴다. 이름은 소문자로 바꾸고, 8 KiB를 넘거나 줄바꿈이 든 값은 버린다
pub fn filter_hook_headers<'pair>(
    headers: impl IntoIterator<Item = (&'pair str, &'pair str)>,
) -> Map<String, Value> {
    let mut filtered = Map::new();
    for (key, value) in headers {
        let name = key.to_ascii_lowercase();
        let is_safe_value = value.len() <= 8192 && !value.contains(['\r', '\n']);
        if FORWARDED_HOOK_HEADERS.contains(&name.as_str()) && is_safe_value {
            filtered.insert(name, Value::String(value.to_string()));
        }
    }
    filtered
}

/// 현재 사용자 uid
pub fn current_uid() -> u32 {
    // SAFETY: getuid는 실패하지 않는 읽기 전용 호출이다
    unsafe { libc::getuid() }
}

#[cfg(test)]
mod tests;
