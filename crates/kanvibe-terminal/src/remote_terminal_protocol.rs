//! 원격 터미널 직접 연결과 모바일 pane 미러가 함께 쓰는 규약(TS `src/lib/remoteTerminalProtocol.ts`).
//! grant 모양, 크기·payload 상한, 스크롤 요청 검증, 휠 보고 인코딩, Herdr 프레임 레코드 해석, 화면 다시 그리기 출력을 담는다.
//! 모든 함수는 부수효과가 없다.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use kanvibe_ai::javascript::utf16_length;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

/// 데스크탑 페어링과 함께 저장하면 안 되는 일회성 라우팅 권한
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalGrant {
    /// 언제나 [`REMOTE_TERMINAL_GRANT_VERSION`]이다
    pub version: u8,
    pub grant_id: String,
    pub direct: RemoteTerminalDirectRoute,
    pub expires_at: i64,
}

/// 모바일이 원격 helper에 바로 붙을 주소와 그 연결을 증명할 값
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalDirectRoute {
    pub host: String,
    pub port: u16,
    pub path: String,
    pub token: String,
    pub certificate_fingerprint: String,
}

pub const REMOTE_TERMINAL_GRANT_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalGrantInput {
    pub ssh_host: String,
    pub device_id: String,
    pub task_id: String,
    pub session_name: String,
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cols: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u32>,
}

/// 원격 helper에 넘기는 grant. 실행할 명령까지 담는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTerminalWireGrant {
    pub grant_id: String,
    pub token: String,
    pub device_id: String,
    pub task_id: String,
    pub pane_id: String,
    pub target_key: String,
    pub control_command: String,
    pub snapshot_command: String,
    pub geometry_command: String,
    pub expires_at: i64,
    pub cols: u32,
    pub rows: u32,
}

pub const REMOTE_TERMINAL_MAX_DIMENSION: u32 = 500;
pub const REMOTE_TERMINAL_INPUT_BYTES: usize = 1024 * 1024;
/// native controller가 첫 프레임을 준비하는 동안 몰려드는 붙여넣기 조각의 상한
pub const REMOTE_TERMINAL_INPUT_QUEUE_BYTES: usize = 8 * REMOTE_TERMINAL_INPUT_BYTES;
pub const REMOTE_TERMINAL_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const REMOTE_TERMINAL_ENVELOPE_BYTES: usize = 16 * 1024 * 1024;
pub const REMOTE_TERMINAL_ADMISSION_MS: u64 = 60_000;
/// 직접 소켓은 죽은 SSH 연결보다 오래 살 수 있어, 데스크탑 쪽 제어 heartbeat만 소유권을 갱신한다
pub const REMOTE_TERMINAL_OWNER_HEARTBEAT_MS: u64 = 10_000;
pub const REMOTE_TERMINAL_OWNER_LEASE_MS: u64 = 30_000;
pub const REMOTE_TERMINAL_COMMAND_TIMEOUT_MS: u64 = 5_000;

/// JS `Number.isInteger(value)`. 정수인 숫자면 그 값을 돌려준다
pub(crate) fn js_integer(value: Option<&Value>) -> Option<f64> {
    let number = value?.as_f64()?;
    (number.fract() == 0.0).then_some(number)
}

/// base64 문자열 payload를 바이트로 푼다. 문자열이 아니거나 길이가 맞지 않으면 디코딩 전에 거절하고,
/// 정규형 base64가 아니거나 풀린 바이트가 상한을 넘으면 거절한다
pub fn decode_remote_terminal_bytes(
    value: Option<&Value>,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let Some(encoded) = value.and_then(Value::as_str) else {
        return Err("Invalid terminal payload".to_string());
    };
    let encoded_length = utf16_length(encoded);
    if encoded_length > limit.div_ceil(3) * 4 || !encoded_length.is_multiple_of(4) {
        return Err("Invalid terminal payload".to_string());
    }
    // Node는 base64를 느슨하게 풀고 다시 인코딩해 원문과 비교한다. 엄격한 디코더의 실패가 그 불일치와 같은 경우다
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| "Terminal payload too large".to_string())?;
    if bytes.len() > limit {
        return Err("Terminal payload too large".to_string());
    }
    Ok(bytes)
}

pub fn is_remote_terminal_size(cols: Option<&Value>, rows: Option<&Value>) -> bool {
    [cols, rows].into_iter().all(|value| {
        js_integer(value)
            .is_some_and(|size| size >= 1.0 && size <= f64::from(REMOTE_TERMINAL_MAX_DIMENSION))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollDirection {
    Up,
    Down,
}

impl ScrollDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            ScrollDirection::Up => "up",
            ScrollDirection::Down => "down",
        }
    }
}

/// 모바일 손가락 스크롤 한 줄. 칸 좌표는 0부터 센다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalScrollRequest {
    pub direction: ScrollDirection,
    pub column: u32,
    pub row: u32,
}

pub fn parse_terminal_scroll_request(record: &Value) -> Option<TerminalScrollRequest> {
    if record.get("type").and_then(Value::as_str) != Some("scroll") {
        return None;
    }
    let direction = match record.get("direction").and_then(Value::as_str) {
        Some("up") => ScrollDirection::Up,
        Some("down") => ScrollDirection::Down,
        _ => return None,
    };
    let column = parse_scroll_cell(record.get("column"))?;
    let row = parse_scroll_cell(record.get("row"))?;
    Some(TerminalScrollRequest {
        direction,
        column,
        row,
    })
}

fn parse_scroll_cell(value: Option<&Value>) -> Option<u32> {
    let cell = js_integer(value)?;
    let is_in_grid = cell >= 0.0 && cell < f64::from(REMOTE_TERMINAL_MAX_DIMENSION);
    is_in_grid.then_some(cell as u32)
}

/// 휠 한 칸을 Herdr 제어 명령으로 바꾼다. 원시 휠 보고를 pane 입력으로 넣으면 마우스 보고를 켜지 않은 프로그램에 글자로 찍히므로,
/// Herdr 서버가 pane의 실제 모드를 보고 휠 보고·화살표 키·자체 스크롤백 중 하나로 고르게 한다.
pub fn build_herdr_scroll_control_record(request: &TerminalScrollRequest) -> Value {
    json!({
        "type": "terminal.scroll",
        "direction": request.direction.as_str(),
        "lines": 1,
        "column": request.column,
        "row": request.row,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseReportEncoding {
    Sgr,
    Utf8,
    Normal,
}

const WHEEL_UP_BUTTON: u32 = 64;
const WHEEL_DOWN_BUTTON: u32 = 65;
/// SGR이 아닌 보고는 값에 32를 더한 글자 하나로 싣는다. 기본 인코딩은 1바이트라 이 값을 넘는 칸은 실을 수 없다
const LEGACY_MOUSE_OFFSET: u32 = 32;
const NORMAL_MOUSE_MAX_VALUE: u32 = 255;

/// 휠 한 칸을 xterm 마우스 보고로 만든다. 칸 좌표는 0부터 받고, 보고에는 1부터 싣는다.
/// 기본 인코딩이 담지 못하는 먼 칸이면 `None`을 돌려준다.
/// <https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-Wheel-mice>
pub fn build_mouse_wheel_report(
    direction: ScrollDirection,
    column: u32,
    row: u32,
    encoding: MouseReportEncoding,
) -> Option<String> {
    let button = match direction {
        ScrollDirection::Up => WHEEL_UP_BUTTON,
        ScrollDirection::Down => WHEEL_DOWN_BUTTON,
    };
    let x = column + 1;
    let y = row + 1;
    if encoding == MouseReportEncoding::Sgr {
        return Some(format!("\x1b[<{button};{x};{y}M"));
    }
    let values = [button, x, y].map(|value| value + LEGACY_MOUSE_OFFSET);
    if encoding == MouseReportEncoding::Normal
        && values.iter().any(|value| *value > NORMAL_MOUSE_MAX_VALUE)
    {
        return None;
    }
    // JS `String.fromCharCode`와 같다. 대리 쌍 영역에 닿는 칸은 화면 크기 상한보다 훨씬 멀어 실제로 오지 않는다
    let characters: Option<String> = values.into_iter().map(char::from_u32).collect();
    Some(format!("\x1b[M{}", characters?))
}

/// observe 프레임이 알려 준 관찰자 격자 크기
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HerdrFrameSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HerdrObserveRecord {
    Frame {
        is_full_screen: bool,
        output: String,
        size: Option<HerdrFrameSize>,
    },
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HerdrTerminalRecord {
    Frame {
        is_full_screen: bool,
        bytes: Vec<u8>,
        size: Option<HerdrFrameSize>,
    },
    Closed,
    Invalid,
}

/// Herdr `terminal session` 출력 한 줄을 해석한다. 모르는 레코드는 `None`, 깨진 줄이나 payload는 `Invalid`다
pub fn parse_herdr_terminal_record(line: &str) -> Option<HerdrTerminalRecord> {
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return Some(HerdrTerminalRecord::Invalid);
    };
    // JS에서 `null.type`은 예외라 깨진 줄로 본다
    if record.is_null() {
        return Some(HerdrTerminalRecord::Invalid);
    }
    match record.get("type").and_then(Value::as_str) {
        Some("terminal.closed") => return Some(HerdrTerminalRecord::Closed),
        Some("terminal.frame") => {}
        _ => return None,
    }
    let Ok(bytes) = decode_remote_terminal_bytes(record.get("bytes"), REMOTE_TERMINAL_FRAME_BYTES)
    else {
        return Some(HerdrTerminalRecord::Invalid);
    };
    Some(HerdrTerminalRecord::Frame {
        is_full_screen: record.get("full") == Some(&Value::Bool(true)),
        bytes,
        size: parse_frame_size(&record),
    })
}

fn parse_frame_size(record: &Value) -> Option<HerdrFrameSize> {
    let width = record.get("width")?.as_f64()?;
    let height = record.get("height")?.as_f64()?;
    Some(HerdrFrameSize { width, height })
}

/// 예전 소비자는 텍스트를 받고, 지속 스트림은 프레임 경계를 넘어 원시 바이트를 푼다
pub fn parse_herdr_observe_record(line: &str) -> Option<HerdrObserveRecord> {
    match parse_herdr_terminal_record(line)? {
        HerdrTerminalRecord::Invalid => None,
        HerdrTerminalRecord::Closed => Some(HerdrObserveRecord::Closed),
        HerdrTerminalRecord::Frame {
            is_full_screen,
            bytes,
            size,
        } => Some(HerdrObserveRecord::Frame {
            is_full_screen,
            output: String::from_utf8_lossy(&bytes).into_owned(),
            size,
        }),
    }
}

/// pane 보이는 영역 안의 칸 좌표(0부터)와 그 영역의 높이
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCursorPosition {
    pub x: i64,
    pub y: i64,
    pub pane_height: i64,
}

pub const MAX_MIRROR_LINES: usize = 2000;

/// 화면, 스크롤백을 지우고 커서를 맨 위로 보낸다. 덤프를 받을 때마다 이어 붙이지 않고 다시 그리기 위해서다
const CLEAR_SCREEN_AND_SCROLLBACK: &str = "\x1b[H\x1b[2J\x1b[3J";

const WHEEL_DELEGATED_SCREEN_MODES: &str =
    "\x1b[?1049h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l";

/// 떠 온 화면 텍스트를 터미널이 처음부터 다시 그리는 출력으로 바꾼다.
///
/// `capture-pane`과 `dump-screen`은 줄 끝에 LF만 붙인다. 터미널은 LF를 "아래로 한 칸"으로만 읽으므로 그대로 쓰면
/// 다음 줄이 앞 줄이 끝난 열에서 시작해 화면이 계단처럼 밀린다. 줄마다 CR을 붙여 줄 처음으로 돌려보낸다.
pub fn build_screen_redraw(screen: &str, cursor: Option<&TerminalCursorPosition>) -> String {
    let lines = split_screen_lines(screen);
    let kept_lines = &lines[lines.len().saturating_sub(MAX_MIRROR_LINES)..];
    let cursor_move = cursor
        .map(build_cursor_move_from_last_line)
        .unwrap_or_default();
    format!(
        "{CLEAR_SCREEN_AND_SCROLLBACK}{}{cursor_move}",
        kept_lines.join("\r\n")
    )
}

/// JS `screen.split(/\r?\n/)`. LF 바로 앞의 CR만 줄 끝으로 보고 지운다
fn split_screen_lines(screen: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = screen.split('\n').collect();
    let last_index = lines.len() - 1;
    for line in &mut lines[..last_index] {
        *line = line.strip_suffix('\r').unwrap_or(line);
    }
    lines
}

/// 휠을 멀티플렉서가 처리하는 pane의 화면을 모바일 터미널이 대체 화면에서 다시 그리는 출력으로 바꾼다.
///
/// Herdr 프레임과 Zellij 덤프에는 화면·마우스 모드 전환이 없다. 대체 화면에 두어야 모바일이 손가락 스크롤을
/// 자기 스크롤백 대신 휠 이벤트로 내보내고, 그 휠은 멀티플렉서가 pane 모드에 맞춰 휠 보고·화살표 키·자체 스크롤백 중 하나로 처리한다.
/// 이 출력은 구독 중에도 다시 나간다. 모드 전환이 맨 앞에 오면 모바일이 다시 그리기로 알아보지 못해 선택 중인 화면을 지우므로,
/// 지우기를 모드 전환 앞에도 한 번 둔다.
pub fn build_wheel_delegated_screen_redraw(screen: &str) -> String {
    format!(
        "{CLEAR_SCREEN_AND_SCROLLBACK}{WHEEL_DELEGATED_SCREEN_MODES}{}",
        build_screen_redraw(screen, None)
    )
}

/// 덤프 앞에는 스크롤백이 붙고 모바일 화면 높이도 pane과 다를 수 있어, 절대 좌표로는 커서가 놓일 줄을 알 수 없다.
/// 덤프의 마지막 줄은 언제나 pane 보이는 영역의 맨 아랫줄이므로, 다 쓴 뒤 그 줄에서 위로 올라가 커서 줄을 찾는다.
fn build_cursor_move_from_last_line(cursor: &TerminalCursorPosition) -> String {
    let rows_above_last_line = cursor.pane_height - 1 - cursor.y;
    let move_up = if rows_above_last_line > 0 {
        format!("\x1b[{rows_above_last_line}A")
    } else {
        String::new()
    };
    format!("{move_up}\x1b[{}G", cursor.x + 1)
}
