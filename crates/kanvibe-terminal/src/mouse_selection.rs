//! 터미널 뷰의 macOS Shift 선택 규칙(TS `terminalMouseSelection.ts`).
//!
//! 마우스 보고를 켠 TUI 안에서는 클릭이 TUI로 가므로, 글을 선택하려면 강제 선택 키가 필요하다.
//! macOS 사용자는 다른 플랫폼처럼 Shift 왼쪽 클릭으로도 선택할 수 있어야 해서, 그 클릭을 Option 클릭으로 바꿔 강제 선택 경로로 보낸다.

/// 선택 판정에 쓰는 마우스 누름. 좌표와 클릭 수는 그대로 전달한다
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalMousePress {
    /// 0이 왼쪽 버튼이다
    pub button: u8,
    pub is_shift_pressed: bool,
    pub is_alt_pressed: bool,
    pub is_control_pressed: bool,
    pub is_meta_pressed: bool,
    pub column: f32,
    pub row: f32,
    pub click_count: u8,
}

/// macOS Shift 왼쪽 클릭(Option 없이)인지
pub fn should_promote_mac_shift_selection(
    press: &TerminalMousePress,
    is_mac_platform: bool,
) -> bool {
    is_mac_platform && press.button == 0 && press.is_shift_pressed && !press.is_alt_pressed
}

/// 강제 선택 여부. macOS Shift 클릭이면 기본 규칙을 묻지 않고 선택한다
pub fn should_force_selection(
    press: &TerminalMousePress,
    is_mac_platform: bool,
    default_rule: impl Fn(&TerminalMousePress) -> bool,
) -> bool {
    should_promote_mac_shift_selection(press, is_mac_platform) || default_rule(press)
}

/// 선택 시작 경로로 넘길 누름. macOS Shift 클릭은 Option 클릭으로 바꾸고, 그 밖의 누름은 그대로 둔다
pub fn normalize_selection_press(
    press: &TerminalMousePress,
    is_mac_platform: bool,
) -> TerminalMousePress {
    if !should_promote_mac_shift_selection(press, is_mac_platform) {
        return *press;
    }
    TerminalMousePress {
        is_alt_pressed: true,
        is_shift_pressed: false,
        ..*press
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift_left_click() -> TerminalMousePress {
        TerminalMousePress {
            button: 0,
            is_shift_pressed: true,
            is_alt_pressed: false,
            is_control_pressed: false,
            is_meta_pressed: false,
            column: 10.0,
            row: 20.0,
            click_count: 1,
        }
    }

    #[test]
    fn mac_shift_left_click_is_forced_selection_without_asking_the_default_rule() {
        let asked = std::cell::Cell::new(false);

        let is_forced = should_force_selection(&shift_left_click(), true, |press| {
            asked.set(true);
            press.is_alt_pressed
        });

        assert!(is_forced);
        assert!(!asked.get());
    }

    #[test]
    fn shift_click_is_not_promoted_outside_macos() {
        let is_forced =
            should_force_selection(&shift_left_click(), false, |press| press.is_alt_pressed);

        assert!(!is_forced);
    }

    #[test]
    fn mac_shift_click_is_normalized_to_the_option_click_selection_path() {
        let normalized = normalize_selection_press(&shift_left_click(), true);

        assert!(normalized.is_alt_pressed);
        assert!(!normalized.is_shift_pressed);
        assert_eq!((normalized.column, normalized.row), (10.0, 20.0));
        assert_eq!(normalized.click_count, 1);
    }
}
