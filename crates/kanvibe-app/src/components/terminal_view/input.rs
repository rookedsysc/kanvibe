//! 키 입력을 PTY 바이트로 바꾼다. xterm.js 기본 동작(`Terminal.onData`)과 같은 바이트를 만든다.
//!
//! 글자 입력(IME 포함)은 GPUI 입력기 경로로 따로 들어오므로, 여기서는 글자가 아닌 키와 Ctrl·Alt 조합만 다룬다.
//! `None`이면 터미널이 이 키를 쓰지 않는다는 뜻이고, 글자 키라면 입력기 경로가 이어서 처리한다.

use gpui_kit::Keystroke;
use kanvibe_domain::platform::ShortcutPlatform;

/// 방향키·Home·End가 `ESC O x`(DECCKM)로 나가는지
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyEncodingModes {
    pub is_application_cursor: bool,
}

/// xterm 수정자 번호: 1 + Shift + Alt*2 + Ctrl*4
fn modifier_parameter(keystroke: &Keystroke) -> u8 {
    let modifiers = &keystroke.modifiers;
    1 + u8::from(modifiers.shift) + 2 * u8::from(modifiers.alt) + 4 * u8::from(modifiers.control)
}

fn has_modifier(keystroke: &Keystroke) -> bool {
    modifier_parameter(keystroke) > 1
}

/// 키 하나를 PTY에 보낼 바이트로 바꾼다. macOS Command 조합은 앱 명령이라 보내지 않는다
pub fn encode_keystroke(keystroke: &Keystroke, modes: KeyEncodingModes) -> Option<String> {
    encode_keystroke_on_platform(keystroke, modes, ShortcutPlatform::current())
}

fn encode_keystroke_on_platform(
    keystroke: &Keystroke,
    modes: KeyEncodingModes,
    platform: ShortcutPlatform,
) -> Option<String> {
    if keystroke.modifiers.platform {
        return None;
    }
    if let Some(sequence) = encode_named_key(keystroke, modes) {
        return Some(sequence);
    }
    encode_modified_character(keystroke, platform)
}

fn encode_named_key(keystroke: &Keystroke, modes: KeyEncodingModes) -> Option<String> {
    let modifiers = &keystroke.modifiers;
    let alt_prefix = if modifiers.alt { "\x1b" } else { "" };
    let sequence = match keystroke.key.as_str() {
        "enter" => format!("{alt_prefix}\r"),
        "escape" => format!("{alt_prefix}\x1b"),
        "tab" if modifiers.shift => "\x1b[Z".to_string(),
        "tab" => format!("{alt_prefix}\t"),
        "backspace" if modifiers.control => format!("{alt_prefix}\x08"),
        "backspace" => format!("{alt_prefix}\x7f"),
        "space" if modifiers.control => format!("{alt_prefix}\x00"),
        "up" => cursor_key('A', keystroke, modes),
        "down" => cursor_key('B', keystroke, modes),
        "right" => cursor_key('C', keystroke, modes),
        "left" => cursor_key('D', keystroke, modes),
        "home" => cursor_key('H', keystroke, modes),
        "end" => cursor_key('F', keystroke, modes),
        "insert" => tilde_key(2, keystroke),
        "delete" => tilde_key(3, keystroke),
        "pageup" => tilde_key(5, keystroke),
        "pagedown" => tilde_key(6, keystroke),
        key => return function_key(key, keystroke),
    };
    Some(sequence)
}

/// 방향키·Home·End. 수정자가 있으면 `ESC [1;m x`, DECCKM이면 `ESC O x`, 아니면 `ESC [ x`
fn cursor_key(final_byte: char, keystroke: &Keystroke, modes: KeyEncodingModes) -> String {
    if has_modifier(keystroke) {
        return format!("\x1b[1;{}{final_byte}", modifier_parameter(keystroke));
    }
    match modes.is_application_cursor {
        true => format!("\x1bO{final_byte}"),
        false => format!("\x1b[{final_byte}"),
    }
}

fn tilde_key(code: u8, keystroke: &Keystroke) -> String {
    match has_modifier(keystroke) {
        true => format!("\x1b[{code};{}~", modifier_parameter(keystroke)),
        false => format!("\x1b[{code}~"),
    }
}

fn function_key(key: &str, keystroke: &Keystroke) -> Option<String> {
    let number: u8 = key.strip_prefix('f')?.parse().ok()?;
    let ss3_final = match number {
        1 => Some('P'),
        2 => Some('Q'),
        3 => Some('R'),
        4 => Some('S'),
        _ => None,
    };
    if let Some(final_byte) = ss3_final {
        return Some(match has_modifier(keystroke) {
            true => format!("\x1b[1;{}{final_byte}", modifier_parameter(keystroke)),
            false => format!("\x1bO{final_byte}"),
        });
    }
    let code = match number {
        5 => 15,
        6..=10 => number + 11,
        11 | 12 => number + 12,
        _ => return None,
    };
    Some(tilde_key(code, keystroke))
}

/// Ctrl·Alt와 함께 누른 글자 키. 수정자가 없는 글자는 입력기 경로로 넘긴다.
/// macOS Option은 xterm.js 기본값(`macOptionIsMeta: false`)처럼 메타가 아니라 `@`·`[` 같은 글자를 만드는 키라서,
/// Ctrl 없이 Option만 누른 글자도 입력기 경로가 조합된 글자를 그대로 보내게 넘긴다
fn encode_modified_character(keystroke: &Keystroke, platform: ShortcutPlatform) -> Option<String> {
    let modifiers = &keystroke.modifiers;
    if !modifiers.control && !modifiers.alt {
        return None;
    }
    let is_mac_option_composition = platform == ShortcutPlatform::Mac && !modifiers.control;
    if is_mac_option_composition {
        return None;
    }
    let mut characters = keystroke.key.chars();
    let key = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    let base = match modifiers.control {
        true => control_character(key)?,
        false => keystroke
            .key_char
            .clone()
            .unwrap_or_else(|| shifted_character(key, modifiers.shift)),
    };
    Some(match modifiers.alt {
        true => format!("\x1b{base}"),
        false => base,
    })
}

/// Ctrl+글자가 만드는 C0 제어 문자
fn control_character(key: char) -> Option<String> {
    let code = match key.to_ascii_lowercase() {
        letter @ 'a'..='z' => letter as u8 - b'a' + 1,
        '@' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '7' | '/' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    };
    Some(char::from(code).to_string())
}

fn shifted_character(key: char, is_shift: bool) -> String {
    match is_shift {
        true => key.to_uppercase().to_string(),
        false => key.to_string(),
    }
}

/// Shift+PageUp·PageDown은 PTY로 보내지 않고 스크롤백 보기를 한 화면(`rows - 1`줄) 넘긴다(xterm.js `PAGE_UP`·`PAGE_DOWN`).
/// 양수가 위(지난 출력) 쪽이다
pub fn page_scroll_lines(keystroke: &Keystroke, rows: u16) -> Option<i32> {
    let modifiers = &keystroke.modifiers;
    if !modifiers.shift || modifiers.platform {
        return None;
    }
    let page = i32::from(rows.saturating_sub(1));
    match keystroke.key.as_str() {
        "pageup" => Some(page),
        "pagedown" => Some(-page),
        _ => None,
    }
}

/// 붙여넣기. bracketed paste 모드면 표시를 두르고, 줄바꿈은 xterm처럼 CR로 바꾼다
pub fn encode_paste(text: &str, is_bracketed_paste: bool) -> String {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    match is_bracketed_paste {
        true => format!("\x1b[200~{}\x1b[201~", normalized.replace('\x1b', "")),
        false => normalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(text: &'static str) -> Option<String> {
        encode_keystroke_on_platform(
            &Keystroke::parse(text).unwrap(),
            KeyEncodingModes::default(),
            ShortcutPlatform::Linux,
        )
    }

    fn encode_on_mac(text: &'static str) -> Option<String> {
        encode_keystroke_on_platform(
            &Keystroke::parse(text).unwrap(),
            KeyEncodingModes::default(),
            ShortcutPlatform::Mac,
        )
    }

    #[test]
    fn mac_option_characters_are_left_to_the_text_input_path() {
        assert_eq!(encode_on_mac("alt-l"), None);
        assert_eq!(encode_on_mac("alt-shift-7"), None);
    }

    #[test]
    fn mac_option_keeps_named_key_and_control_sequences() {
        assert_eq!(encode_on_mac("alt-left").as_deref(), Some("\x1b[1;3D"));
        assert_eq!(encode_on_mac("alt-enter").as_deref(), Some("\x1b\r"));
        assert_eq!(encode_on_mac("ctrl-alt-c").as_deref(), Some("\x1b\x03"));
    }

    #[test]
    fn linux_alt_characters_get_the_meta_prefix() {
        assert_eq!(encode("alt-l").as_deref(), Some("\x1bl"));
    }

    #[test]
    fn control_keys_become_c0_characters() {
        assert_eq!(encode("ctrl-c").as_deref(), Some("\x03"));
        assert_eq!(encode("ctrl-d").as_deref(), Some("\x04"));
        assert_eq!(encode("ctrl-[").as_deref(), Some("\x1b"));
        assert_eq!(encode("ctrl-space").as_deref(), Some("\x00"));
    }

    #[test]
    fn plain_characters_are_left_to_the_text_input_path() {
        assert_eq!(encode("a"), None);
        assert_eq!(encode("shift-a"), None);
        assert_eq!(encode("cmd-c"), None);
    }

    #[test]
    fn navigation_keys_follow_xterm_sequences() {
        assert_eq!(encode("enter").as_deref(), Some("\r"));
        assert_eq!(encode("backspace").as_deref(), Some("\x7f"));
        assert_eq!(encode("shift-tab").as_deref(), Some("\x1b[Z"));
        assert_eq!(encode("up").as_deref(), Some("\x1b[A"));
        assert_eq!(encode("ctrl-right").as_deref(), Some("\x1b[1;5C"));
        assert_eq!(encode("pageup").as_deref(), Some("\x1b[5~"));
        assert_eq!(encode("f1").as_deref(), Some("\x1bOP"));
        assert_eq!(encode("f5").as_deref(), Some("\x1b[15~"));
        assert_eq!(encode("f12").as_deref(), Some("\x1b[24~"));
        assert_eq!(encode("alt-b").as_deref(), Some("\x1bb"));
        let application = KeyEncodingModes {
            is_application_cursor: true,
        };
        assert_eq!(
            encode_keystroke(&Keystroke::parse("up").unwrap(), application).as_deref(),
            Some("\x1bOA")
        );
    }

    #[test]
    fn shift_page_keys_scroll_the_view_by_a_screen_instead_of_reaching_the_pty() {
        let page_scroll =
            |text: &'static str| page_scroll_lines(&Keystroke::parse(text).unwrap(), 24);
        assert_eq!(page_scroll("shift-pageup"), Some(23));
        assert_eq!(page_scroll("shift-pagedown"), Some(-23));
        assert_eq!(page_scroll("pageup"), None);
        assert_eq!(page_scroll("ctrl-pageup"), None);
        assert_eq!(page_scroll("shift-up"), None);
    }

    #[test]
    fn paste_wraps_bracketed_markers_and_normalizes_newlines() {
        assert_eq!(encode_paste("a\nb", false), "a\rb");
        assert_eq!(encode_paste("a\r\nb", true), "\x1b[200~a\rb\x1b[201~");
    }
}
