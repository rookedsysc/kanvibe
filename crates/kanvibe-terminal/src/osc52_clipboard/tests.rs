use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::*;
use crate::screen::TerminalScreen;

fn encode_base64_utf8(value: &str) -> String {
    STANDARD.encode(value.as_bytes())
}

/// 받은 글을 기록하는 클립보드. `failure`가 있으면 쓰기를 거절한다
#[derive(Clone, Default)]
struct RecordingClipboard {
    written: Arc<Mutex<Vec<String>>>,
    failure: Option<String>,
}

impl SystemClipboardWriter for RecordingClipboard {
    fn write_text(&self, text: &str) -> Result<(), String> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        self.written.lock().unwrap().push(text.to_string());
        Ok(())
    }
}

impl RecordingClipboard {
    fn written(&self) -> Vec<String> {
        self.written.lock().unwrap().clone()
    }
}

fn osc52_sequence(payload: &str, terminator: &str) -> Vec<u8> {
    format!("\u{1b}]52;{payload}{terminator}").into_bytes()
}

#[test]
fn decodes_a_base64_clipboard_payload() {
    let data = format!("c;{}", encode_base64_utf8("kanvibe"));

    assert_eq!(read_osc52_clipboard_text(&data).as_deref(), Some("kanvibe"));
}

#[test]
fn decodes_multibyte_text_without_corruption() {
    let data = format!("c;{}", encode_base64_utf8("한글 복사 ✅"));

    assert_eq!(
        read_osc52_clipboard_text(&data).as_deref(),
        Some("한글 복사 ✅")
    );
}

#[test]
fn ignores_a_clipboard_read_request_so_terminal_content_is_not_sent_back() {
    assert_eq!(read_osc52_clipboard_text("c;?"), None);
}

#[test]
fn ignores_a_payload_without_a_selection_separator() {
    assert_eq!(read_osc52_clipboard_text("c"), None);
}

#[test]
fn ignores_an_empty_payload() {
    assert_eq!(read_osc52_clipboard_text("c;"), None);
}

#[test]
fn ignores_an_undecodable_payload() {
    assert_eq!(read_osc52_clipboard_text("c;!!!not-base64!!!"), None);
}

#[test]
fn ignores_a_payload_above_the_size_limit() {
    let data = format!("c;{}", "A".repeat(1_000_001));

    assert_eq!(read_osc52_clipboard_text(&data), None);
}

#[test]
fn copies_the_decoded_payload_to_the_system_clipboard() {
    let clipboard = RecordingClipboard::default();
    let mut forwarder = Osc52ClipboardForwarder::new(clipboard.clone());

    forwarder.observe(&osc52_sequence(
        &format!("c;{}", encode_base64_utf8("복사 대상")),
        "\u{7}",
    ));

    assert_eq!(clipboard.written(), vec!["복사 대상".to_string()]);
}

#[test]
fn consumes_a_read_request_without_touching_the_clipboard() {
    let clipboard = RecordingClipboard::default();
    let mut forwarder = Osc52ClipboardForwarder::new(clipboard.clone());
    let mut screen = TerminalScreen::new(20, 2);
    let output = osc52_sequence("c;?", "\u{7}");

    forwarder.observe(&output);
    screen.write(&output);

    assert!(clipboard.written().is_empty());
    assert_eq!(screen.viewport_lines()[0], "");
}

#[test]
fn keeps_the_terminal_usable_when_the_clipboard_write_is_rejected() {
    let clipboard = RecordingClipboard {
        failure: Some("clipboard blocked".to_string()),
        ..RecordingClipboard::default()
    };
    let mut forwarder = Osc52ClipboardForwarder::new(clipboard.clone());

    forwarder.observe(&osc52_sequence(
        &format!("c;{}", encode_base64_utf8("kanvibe")),
        "\u{7}",
    ));
    let recovered = RecordingClipboard::default();
    let mut next_forwarder = Osc52ClipboardForwarder::new(recovered.clone());
    next_forwarder.observe(&osc52_sequence(
        &format!("c;{}", encode_base64_utf8("다음")),
        "\u{7}",
    ));

    assert!(clipboard.written().is_empty());
    assert_eq!(recovered.written(), vec!["다음".to_string()]);
}

#[test]
fn copies_a_bell_terminated_sequence_which_is_what_a_plain_printf_sends() {
    let clipboard = RecordingClipboard::default();
    let mut forwarder = Osc52ClipboardForwarder::new(clipboard.clone());

    forwarder.observe(&osc52_sequence(
        &format!("c;{}", encode_base64_utf8("OSC52-OK")),
        "\u{7}",
    ));

    assert_eq!(clipboard.written(), vec!["OSC52-OK".to_string()]);
}

#[test]
fn copies_a_string_terminated_sequence_which_is_what_tmux_forwards() {
    let clipboard = RecordingClipboard::default();
    let mut forwarder = Osc52ClipboardForwarder::new(clipboard.clone());
    let output = osc52_sequence(
        &format!("c;{}", encode_base64_utf8("tmux 복사")),
        "\u{1b}\\",
    );
    let mut screen = TerminalScreen::new(20, 2);

    // tmux는 긴 시퀀스를 여러 조각으로 나눠 보낼 수 있다
    let (head, tail) = output.split_at(7);
    forwarder.observe(head);
    forwarder.observe(tail);
    screen.write(&output);

    assert_eq!(clipboard.written(), vec!["tmux 복사".to_string()]);
    assert_eq!(screen.viewport_lines()[0], "");
}

#[test]
fn oversized_sequences_are_dropped_without_buffering_the_whole_payload() {
    let mut scanner = Osc52SequenceScanner::default();
    let mut output = b"\x1b]52;c;".to_vec();
    output.extend(std::iter::repeat_n(b'A', 1_200_000));
    output.push(0x07);

    assert_eq!(scanner.scan(&output), vec![Osc52Body::Oversized]);
}

#[test]
fn other_osc_sequences_and_cancelled_sequences_are_ignored() {
    let mut scanner = Osc52SequenceScanner::default();

    let found = scanner.scan(b"\x1b]0;title\x07\x1b]52;c;YQ\x18==\x07plain");

    assert!(found.is_empty());
}

#[test]
fn unpadded_base64_is_accepted_like_atob() {
    assert_eq!(read_osc52_clipboard_text("c;YQ").as_deref(), Some("a"));
}
