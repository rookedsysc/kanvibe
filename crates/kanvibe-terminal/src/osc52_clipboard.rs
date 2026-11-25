//! OSC 52 클립보드 쓰기(TS `terminalClipboard.ts`). tmux·zellij 너머 프로세스가 보낸 복사 요청을 시스템 클립보드로 옮긴다.
//!
//! 터미널 뷰는 PTY 출력을 화면에 넣기 전에 [`Osc52ClipboardForwarder::observe`]에 같은 바이트를 보여 준다.
//! 읽기 요청에는 답하지 않아 터미널 내용이 원격으로 새지 않고, base64 본문에는 상한을 둔다.
//! 화면 쪽 alacritty는 OSC 52를 끈 채로 두어 같은 시퀀스를 두 번 처리하지 않는다.

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// 터미널이 시스템 클립보드에 값을 넣을 때 쓰는 OSC 번호
const OSC_CLIPBOARD_PREFIX: &[u8] = b"52;";
/// 클립보드에 넣을 base64 본문 상한. 임의 프로세스가 보내는 값이라 무한정 받지 않는다
const MAX_CLIPBOARD_PAYLOAD_LENGTH: usize = 1_000_000;
/// OSC 52의 읽기 요청 표기
const CLIPBOARD_READ_REQUEST: &str = "?";

const ESCAPE: u8 = 0x1b;
const BELL: u8 = 0x07;
const CANCEL: u8 = 0x18;
const SUBSTITUTE: u8 = 0x1a;

/// OSC 52 본문(`<선택 영역>;<base64>`)에서 클립보드에 넣을 문자열을 뽑는다. 읽기 요청이거나 해석할 수 없으면 `None`이다
pub fn read_osc52_clipboard_text(data: &str) -> Option<String> {
    let (_, encoded_text) = data.split_once(';')?;
    if encoded_text.is_empty() || encoded_text == CLIPBOARD_READ_REQUEST {
        return None;
    }
    if encoded_text.encode_utf16().count() > MAX_CLIPBOARD_PAYLOAD_LENGTH {
        return None;
    }
    decode_base64_utf8(encoded_text)
}

/// `atob`처럼 패딩이 없거나 끝 비트가 남아도 받고 ASCII 공백은 건너뛴다
const ATOB_ENGINE: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);
const UTF8_BYTE_ORDER_MARK: &[u8] = b"\xef\xbb\xbf";

/// 한글처럼 멀티바이트 문자가 깨지지 않도록 바이트로 푼 뒤 UTF-8로 읽는다.
/// `TextDecoder`처럼 잘못된 바이트는 대체 문자로 바꾸고 맨 앞 BOM은 뗀다
fn decode_base64_utf8(encoded_text: &str) -> Option<String> {
    let compact: String = encoded_text
        .chars()
        .filter(|character| !matches!(character, '\t' | '\n' | '\x0c' | '\r' | ' '))
        .collect();
    let bytes = ATOB_ENGINE.decode(compact).ok()?;
    let text_bytes = bytes.strip_prefix(UTF8_BYTE_ORDER_MARK).unwrap_or(&bytes);
    Some(String::from_utf8_lossy(text_bytes).into_owned())
}

/// 시스템 클립보드 쓰기. 데스크탑 앱은 창 포커스와 무관한 네이티브 클립보드로 구현한다
pub trait SystemClipboardWriter: Send + Sync {
    fn write_text(&self, text: &str) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Ground,
    Escape,
    OscBody,
    OscEscape,
}

/// 끝난 OSC 52 시퀀스 하나의 본문(`52;` 뒤)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Osc52Body {
    Text(String),
    /// 상한을 넘겨 본문을 모으지 않았다. 해석하지 않고 버린다
    Oversized,
}

/// 출력 조각 경계를 넘어 OSC 시퀀스를 찾아 OSC 52 본문만 돌려준다
#[derive(Debug)]
pub struct Osc52SequenceScanner {
    state: ScanState,
    body: Vec<u8>,
    has_overflowed: bool,
}

impl Default for Osc52SequenceScanner {
    fn default() -> Self {
        Self {
            state: ScanState::Ground,
            body: Vec::new(),
            has_overflowed: false,
        }
    }
}

impl Osc52SequenceScanner {
    /// 이 조각에서 끝난 OSC 52 본문들
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<Osc52Body> {
        let mut payloads = Vec::new();
        for &byte in bytes {
            if let Some(payload) = self.advance(byte) {
                payloads.push(payload);
            }
        }
        payloads
    }

    fn advance(&mut self, byte: u8) -> Option<Osc52Body> {
        match (self.state, byte) {
            (ScanState::Ground, ESCAPE) => self.state = ScanState::Escape,
            (ScanState::Ground, _) => {}
            (ScanState::Escape, b']') => self.start_body(),
            (ScanState::Escape, ESCAPE) => {}
            (ScanState::Escape, _) => self.state = ScanState::Ground,
            (ScanState::OscBody | ScanState::OscEscape, CANCEL | SUBSTITUTE) => {
                self.state = ScanState::Ground
            }
            (ScanState::OscBody, BELL) => return self.finish_body(ScanState::Ground),
            (ScanState::OscBody, ESCAPE) => self.state = ScanState::OscEscape,
            (ScanState::OscBody, _) => self.push_body(byte),
            // xterm은 OSC 안의 ESC를 문자열 끝으로 보고 본문을 넘긴 뒤 그 ESC부터 다시 해석한다
            (ScanState::OscEscape, b'\\') => return self.finish_body(ScanState::Ground),
            (ScanState::OscEscape, b']') => {
                let payload = self.finish_body(ScanState::Ground);
                self.start_body();
                return payload;
            }
            (ScanState::OscEscape, _) => return self.finish_body(ScanState::Ground),
        }
        None
    }

    fn start_body(&mut self) {
        self.state = ScanState::OscBody;
        self.body.clear();
        self.has_overflowed = false;
    }

    fn push_body(&mut self, byte: u8) {
        if self.body.len() > MAX_CLIPBOARD_PAYLOAD_LENGTH + OSC_CLIPBOARD_PREFIX.len() + 1 {
            self.has_overflowed = true;
            return;
        }
        self.body.push(byte);
    }

    fn finish_body(&mut self, next_state: ScanState) -> Option<Osc52Body> {
        self.state = next_state;
        let body = std::mem::take(&mut self.body);
        let clipboard_body = body.strip_prefix(OSC_CLIPBOARD_PREFIX)?;
        match self.has_overflowed {
            true => Some(Osc52Body::Oversized),
            false => Some(Osc52Body::Text(
                String::from_utf8_lossy(clipboard_body).into_owned(),
            )),
        }
    }
}

/// 터미널 뷰 하나에 붙는 OSC 52 전달기. 읽기 요청이나 잘못된 본문도 소비만 하고 화면에는 내보내지 않는다
pub struct Osc52ClipboardForwarder<W: SystemClipboardWriter> {
    scanner: Osc52SequenceScanner,
    writer: W,
}

impl<W: SystemClipboardWriter> Osc52ClipboardForwarder<W> {
    pub fn new(writer: W) -> Self {
        Self {
            scanner: Osc52SequenceScanner::default(),
            writer,
        }
    }

    /// PTY 출력 한 조각을 보고 끝난 복사 요청을 클립보드로 보낸다. 쓰기가 실패해도 터미널은 계속 동작한다
    pub fn observe(&mut self, bytes: &[u8]) {
        for body in self.scanner.scan(bytes) {
            let Osc52Body::Text(payload) = body else {
                continue;
            };
            let Some(clipboard_text) = read_osc52_clipboard_text(&payload) else {
                continue;
            };
            if let Err(error) = self.writer.write_text(&clipboard_text) {
                eprintln!("터미널 클립보드 복사 실패: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests;
