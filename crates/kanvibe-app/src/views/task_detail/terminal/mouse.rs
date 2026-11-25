//! 터미널 마우스 규칙(xterm.js 기본 동작). 프로그램이 마우스 보고를 켜면 클릭·휠을 보고로 보내고,
//! 강제 선택 키(Linux Shift, macOS Option 또는 Shift)를 누르면 보고 대신 글을 선택한다.

use kanvibe_terminal::mouse_selection::{
    TerminalMousePress, normalize_selection_press, should_force_selection,
};
use kanvibe_terminal::remote_terminal_protocol::{
    MouseReportEncoding, ScrollDirection, build_mouse_wheel_report,
};
use kanvibe_terminal::screen::ScreenInputModes;

/// 화면 칸 좌표(0부터)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GridPoint {
    pub row: usize,
    pub column: usize,
}

/// 드래그로 고른 영역. 시작과 끝 순서는 상관없다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSelection {
    pub anchor: GridPoint,
    pub head: GridPoint,
}

impl GridSelection {
    pub fn ordered(&self) -> (GridPoint, GridPoint) {
        match self.anchor <= self.head {
            true => (self.anchor, self.head),
            false => (self.head, self.anchor),
        }
    }

    /// 이 칸이 선택에 들어가는지(줄 단위 흐름 선택)
    pub fn contains(&self, point: GridPoint) -> bool {
        let (start, end) = self.ordered();
        start <= point && point <= end
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// 누른 마우스를 선택으로 다룰지 보고로 보낼지
pub fn is_selection_press(
    press: &TerminalMousePress,
    modes: &ScreenInputModes,
    is_mac_platform: bool,
) -> bool {
    if !modes.is_mouse_reporting {
        return true;
    }
    let normalized = normalize_selection_press(press, is_mac_platform);
    should_force_selection(
        &normalized,
        is_mac_platform,
        |press| match is_mac_platform {
            true => press.is_alt_pressed,
            false => press.is_shift_pressed,
        },
    )
}

/// 버튼 누름·뗌 보고. `button`은 0 왼쪽, 1 가운데, 2 오른쪽
pub fn build_mouse_button_report(
    button: u8,
    is_pressed: bool,
    point: GridPoint,
    modes: &ScreenInputModes,
) -> Option<String> {
    let x = point.column + 1;
    let y = point.row + 1;
    if modes.is_sgr_mouse {
        let suffix = if is_pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{button};{x};{y}{suffix}"));
    }
    // 옛 인코딩은 뗌을 버튼 3으로 알린다
    let code = if is_pressed { u32::from(button) } else { 3 };
    let values = [code, x as u32, y as u32].map(|value| value + 32);
    if values.iter().any(|value| *value > 255) {
        return None;
    }
    let characters: Option<String> = values.into_iter().map(char::from_u32).collect();
    Some(format!("\x1b[M{}", characters?))
}

/// 휠 한 칸. 보고 모드면 휠 보고, 대체 화면이면 방향키, 아니면 `None`(로컬 스크롤백을 움직인다)
pub fn build_wheel_input(
    is_up: bool,
    point: GridPoint,
    modes: &ScreenInputModes,
) -> Option<String> {
    let direction = match is_up {
        true => ScrollDirection::Up,
        false => ScrollDirection::Down,
    };
    if modes.is_mouse_reporting {
        let encoding = match modes.is_sgr_mouse {
            true => MouseReportEncoding::Sgr,
            false => MouseReportEncoding::Normal,
        };
        return build_mouse_wheel_report(
            direction,
            point.column as u32,
            point.row as u32,
            encoding,
        );
    }
    if !modes.is_alternate_screen {
        return None;
    }
    let final_byte = if is_up { 'A' } else { 'B' };
    Some(match modes.is_application_cursor {
        true => format!("\x1bO{final_byte}"),
        false => format!("\x1b[{final_byte}"),
    })
}

/// 줄 글에서 단어 경계를 찾는다(더블 클릭 선택). xterm.js 기본 구분자는 공백과 `()[]{}'",`이다
pub fn word_bounds(characters: &[String], column: usize) -> (usize, usize) {
    let is_separator = |text: &str| {
        text.is_empty()
            || text
                .chars()
                .all(|character| " ()[]{}'\",`".contains(character))
    };
    if characters.get(column).is_none_or(|text| is_separator(text)) {
        return (column, column);
    }
    let start = (0..column)
        .rev()
        .take_while(|index| !is_separator(&characters[*index]))
        .last()
        .unwrap_or(column);
    let end = (column..characters.len())
        .take_while(|index| !is_separator(&characters[*index]))
        .last()
        .unwrap_or(column);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(is_shift: bool, is_alt: bool) -> TerminalMousePress {
        TerminalMousePress {
            button: 0,
            is_shift_pressed: is_shift,
            is_alt_pressed: is_alt,
            is_control_pressed: false,
            is_meta_pressed: false,
            column: 1.0,
            row: 1.0,
            click_count: 1,
        }
    }

    fn reporting_modes() -> ScreenInputModes {
        ScreenInputModes {
            is_mouse_reporting: true,
            is_sgr_mouse: true,
            ..ScreenInputModes::default()
        }
    }

    #[test]
    fn clicks_select_text_unless_the_program_reports_the_mouse() {
        assert!(is_selection_press(
            &press(false, false),
            &ScreenInputModes::default(),
            false
        ));
        assert!(!is_selection_press(
            &press(false, false),
            &reporting_modes(),
            false
        ));
        assert!(is_selection_press(
            &press(true, false),
            &reporting_modes(),
            false
        ));
    }

    #[test]
    fn mac_shift_click_forces_selection_inside_mouse_reporting_programs() {
        assert!(is_selection_press(
            &press(true, false),
            &reporting_modes(),
            true
        ));
        assert!(is_selection_press(
            &press(false, true),
            &reporting_modes(),
            true
        ));
        assert!(!is_selection_press(
            &press(false, false),
            &reporting_modes(),
            true
        ));
    }

    #[test]
    fn button_and_wheel_reports_use_sgr_when_enabled() {
        let point = GridPoint { row: 4, column: 9 };
        assert_eq!(
            build_mouse_button_report(0, true, point, &reporting_modes()).as_deref(),
            Some("\x1b[<0;10;5M")
        );
        assert_eq!(
            build_wheel_input(true, point, &reporting_modes()).as_deref(),
            Some("\x1b[<64;10;5M")
        );
    }

    #[test]
    fn wheel_moves_the_cursor_in_alternate_screen_and_scrolls_history_otherwise() {
        let point = GridPoint { row: 0, column: 0 };
        let alternate = ScreenInputModes {
            is_alternate_screen: true,
            ..ScreenInputModes::default()
        };
        assert_eq!(
            build_wheel_input(false, point, &alternate).as_deref(),
            Some("\x1b[B")
        );
        assert_eq!(
            build_wheel_input(true, point, &ScreenInputModes::default()),
            None
        );
    }

    #[test]
    fn double_click_selects_the_word_under_the_pointer() {
        let characters: Vec<String> = "git  status --short"
            .chars()
            .map(|character| character.to_string())
            .collect();
        assert_eq!(word_bounds(&characters, 6), (5, 10));
        assert_eq!(word_bounds(&characters, 3), (3, 3));
    }
}
