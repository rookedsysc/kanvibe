//! agent가 쓰는 원격 터미널 와이어 규칙(TS `remoteTerminalProtocol.ts`의 일부).
//!
//! 데스크탑 쪽 `kanvibe_terminal::remote_terminal_protocol`과 바이트 단위로 같아야 한다. 정적 바이너리가
//! kanvibe-terminal(그리고 rusqlite)을 끌고 오지 않도록 agent가 쓰는 함수만 여기 따로 둔다.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Value, json};

pub const REMOTE_TERMINAL_MAX_DIMENSION: u32 = 500;
pub const REMOTE_TERMINAL_INPUT_BYTES: usize = 1024 * 1024;
/// 붙기 전에 쌓아 둘 수 있는 입력. 한 번에 보낼 수 있는 입력 여덟 개만큼이다
pub const REMOTE_TERMINAL_INPUT_QUEUE_BYTES: usize = 8 * REMOTE_TERMINAL_INPUT_BYTES;
pub const REMOTE_TERMINAL_FRAME_BYTES: usize = 8 * 1024 * 1024;
pub const REMOTE_TERMINAL_ENVELOPE_BYTES: usize = 16 * 1024 * 1024;
pub const REMOTE_TERMINAL_ADMISSION_MS: u64 = 60_000;
pub const REMOTE_TERMINAL_OWNER_LEASE_MS: u64 = 30_000;
pub const REMOTE_TERMINAL_COMMAND_TIMEOUT_MS: u64 = 5_000;

const CLEAR_SCREEN_AND_SCROLLBACK: &str = "\x1b[H\x1b[2J\x1b[3J";
const WHEEL_DELEGATED_SCREEN_MODES: &str =
    "\x1b[?1049h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l";
const MAX_MIRROR_LINES: usize = 2000;

/// JS `Number.isInteger(value)`. 정수인 숫자면 그 값을 돌려준다
fn js_integer(value: Option<&Value>) -> Option<f64> {
    let number = value?.as_f64()?;
    (number.fract() == 0.0).then_some(number)
}

/// base64 payload를 바이트로 푼다. 길이가 맞지 않거나 정규형이 아니거나 상한을 넘으면 거절한다
pub fn decode_remote_terminal_bytes(
    value: Option<&Value>,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let Some(encoded) = value.and_then(Value::as_str) else {
        return Err("Invalid terminal payload".to_string());
    };
    let encoded_length: usize = encoded.chars().map(char::len_utf16).sum();
    if encoded_length > limit.div_ceil(3) * 4 || !encoded_length.is_multiple_of(4) {
        return Err("Invalid terminal payload".to_string());
    }
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

/// 모바일 손가락 스크롤 한 줄을 Herdr 제어 레코드(`terminal.scroll`)로 바꾼다. 스크롤 요청이 아니면 `None`이다
pub fn parse_scroll_control_record(record: &Value) -> Option<Value> {
    if record.get("type").and_then(Value::as_str) != Some("scroll") {
        return None;
    }
    let direction = match record.get("direction").and_then(Value::as_str) {
        Some(direction @ ("up" | "down")) => direction,
        _ => return None,
    };
    let column = parse_scroll_cell(record.get("column"))?;
    let row = parse_scroll_cell(record.get("row"))?;
    Some(
        json!({ "type": "terminal.scroll", "direction": direction, "lines": 1, "column": column, "row": row }),
    )
}

fn parse_scroll_cell(value: Option<&Value>) -> Option<u32> {
    let cell = js_integer(value)?;
    let is_in_grid = cell >= 0.0 && cell < f64::from(REMOTE_TERMINAL_MAX_DIMENSION);
    is_in_grid.then_some(cell as u32)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HerdrTerminalRecord {
    Frame {
        is_full_screen: bool,
        bytes: Vec<u8>,
    },
    Closed,
    Invalid,
}

/// Herdr `terminal session` 출력 한 줄. 모르는 레코드는 `None`, 깨진 줄이나 payload는 `Invalid`다
pub fn parse_herdr_terminal_record(line: &str) -> Option<HerdrTerminalRecord> {
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        return Some(HerdrTerminalRecord::Invalid);
    };
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
    })
}

/// 떠 온 화면을 모바일 터미널이 대체 화면에서 처음부터 다시 그리는 출력으로 바꾼다
pub fn build_wheel_delegated_screen_redraw(screen: &str) -> String {
    let lines = split_screen_lines(screen);
    let kept_lines = &lines[lines.len().saturating_sub(MAX_MIRROR_LINES)..];
    format!(
        "{CLEAR_SCREEN_AND_SCROLLBACK}{WHEEL_DELEGATED_SCREEN_MODES}{CLEAR_SCREEN_AND_SCROLLBACK}{}",
        kept_lines.join("\r\n")
    )
}

/// JS `screen.split(/\r?\n/)`
fn split_screen_lines(screen: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = screen.split('\n').collect();
    let last_index = lines.len() - 1;
    for line in &mut lines[..last_index] {
        *line = line.strip_suffix('\r').unwrap_or(line);
    }
    lines
}

/// Node `StringDecoder("utf8")`. 조각 경계에 걸린 글자는 다음 조각까지 남기고, 깨진 바이트는 U+FFFD로 바꾼다
#[derive(Default)]
pub struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub fn write(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut text = String::new();
        let mut offset = 0;
        loop {
            match std::str::from_utf8(&self.pending[offset..]) {
                Ok(valid) => {
                    text.push_str(valid);
                    offset = self.pending.len();
                    break;
                }
                Err(error) => {
                    let valid_end = offset + error.valid_up_to();
                    text.push_str(
                        std::str::from_utf8(&self.pending[offset..valid_end]).unwrap_or_default(),
                    );
                    let Some(invalid_length) = error.error_len() else {
                        offset = valid_end;
                        break;
                    };
                    text.push('\u{fffd}');
                    offset = valid_end + invalid_length;
                }
            }
        }
        self.pending.drain(..offset);
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_decoder_keeps_split_characters_until_complete() {
        let mut decoder = Utf8StreamDecoder::default();
        let bytes = "한글".as_bytes();
        assert_eq!(decoder.write(&bytes[..1]), "");
        assert_eq!(decoder.write(&bytes[1..]), "한글");
        assert_eq!(decoder.write(&[0xff, b'a']), "\u{fffd}a");
    }

    #[test]
    fn wheel_redraw_matches_the_desktop_byte_layout() {
        assert_eq!(
            build_wheel_delegated_screen_redraw("a\r\nb\n"),
            "\x1b[H\x1b[2J\x1b[3J\x1b[?1049h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[H\x1b[2J\x1b[3Ja\r\nb\r\n"
        );
    }
}
