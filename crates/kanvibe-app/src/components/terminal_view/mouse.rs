//! 터미널 마우스 규칙(xterm.js 기본 동작). 프로그램이 마우스 보고를 켜면 클릭·휠을 보고로 보내고,
//! 강제 선택 키(Linux Shift, macOS Option 또는 Shift)를 누르면 보고 대신 글을 선택한다.

use kanvibe_terminal::mouse_selection::{
    TerminalMousePress, normalize_selection_press, should_force_selection,
};
use kanvibe_terminal::remote_terminal_protocol::{
    MouseReportEncoding, ScrollDirection, build_mouse_wheel_report,
};
use kanvibe_terminal::screen::{ScreenCell, ScreenInputModes};

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

/// 버튼이 없음을 뜻하는 보고 번호. 옛 인코딩의 뗌과 버튼 없이 움직인 보고에 쓴다
const NO_BUTTON_CODE: u32 = 3;
/// 이동 보고 표시(xterm `+32`)
const MOTION_FLAG: u32 = 32;

/// 보고와 함께 누른 수정자 키. 보고 번호에 Shift 4, Alt 8, Ctrl 16을 더한다(xterm.js `CoreMouseService`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MouseReportModifiers {
    pub is_shift_pressed: bool,
    pub is_alt_pressed: bool,
    pub is_control_pressed: bool,
}

impl MouseReportModifiers {
    fn report_bits(self) -> u32 {
        4 * u32::from(self.is_shift_pressed)
            + 8 * u32::from(self.is_alt_pressed)
            + 16 * u32::from(self.is_control_pressed)
    }
}

/// 버튼 누름·뗌 보고. `button`은 0 왼쪽, 1 가운데, 2 오른쪽
pub fn build_mouse_button_report(
    button: u8,
    is_pressed: bool,
    point: GridPoint,
    modifiers: MouseReportModifiers,
    modes: &ScreenInputModes,
) -> Option<String> {
    // 옛 인코딩은 뗌을 버튼 3으로 알린다
    let button_code = match is_pressed || modes.is_sgr_mouse {
        true => u32::from(button),
        false => NO_BUTTON_CODE,
    };
    encode_mouse_report(
        button_code + modifiers.report_bits(),
        is_pressed,
        point,
        modes,
    )
}

/// 마우스 이동 보고. 버튼을 누른 채 움직이면 버튼 이벤트 모드(1002)나 모든 이동 모드(1003)에서,
/// 버튼 없이 움직이면 모든 이동 모드에서만 보낸다
pub fn build_mouse_motion_report(
    pressed_button: Option<u8>,
    point: GridPoint,
    modifiers: MouseReportModifiers,
    modes: &ScreenInputModes,
) -> Option<String> {
    let is_motion_reported = match pressed_button {
        Some(_) => modes.is_mouse_drag_reporting || modes.is_mouse_motion_reporting,
        None => modes.is_mouse_motion_reporting,
    };
    if !is_motion_reported {
        return None;
    }
    let button_code = pressed_button.map_or(NO_BUTTON_CODE, u32::from);
    let code = MOTION_FLAG + button_code + modifiers.report_bits();
    encode_mouse_report(code, true, point, modes)
}

/// 보고 번호와 칸 좌표를 SGR(`ESC [ < b;x;y M/m`)이나 옛 인코딩(`ESC [ M` + 32를 더한 세 글자)으로 적는다
fn encode_mouse_report(
    code: u32,
    is_pressed: bool,
    point: GridPoint,
    modes: &ScreenInputModes,
) -> Option<String> {
    let x = point.column + 1;
    let y = point.row + 1;
    if modes.is_sgr_mouse {
        let suffix = if is_pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{x};{y}{suffix}"));
    }
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

/// 휠 양(줄 단위)을 쌓아 온전한 줄 수만 꺼낸다. 1줄 미만 나머지는 다음 이벤트로 넘기고(xterm.js `_wheelPartialScroll`),
/// 방향이 바뀌면 반대 방향에 쌓인 나머지를 버린다
pub fn take_whole_wheel_lines(remainder_lines: &mut f32, delta_lines: f32) -> i32 {
    let is_direction_reversed = *remainder_lines * delta_lines < 0.0;
    if is_direction_reversed {
        *remainder_lines = 0.0;
    }
    *remainder_lines += delta_lines;
    let whole_lines = remainder_lines.trunc();
    *remainder_lines -= whole_lines;
    whole_lines as i32
}

/// 줄 글에서 단어 경계를 찾는다(더블 클릭 선택). xterm.js 기본 구분자는 공백과 `()[]{}'",`이다.
/// 넓은 글자의 뒤 칸(폭 0)은 글이 비어 있어도 앞 글자에 붙은 칸이라 구분자가 아니다(xterm.js `_getWordAt`)
pub fn word_bounds(cells: &[ScreenCell], column: usize) -> (usize, usize) {
    let is_separator = |cell: &ScreenCell| {
        let is_wide_character_trailer = cell.width == 0;
        !is_wide_character_trailer
            && (cell.text.is_empty()
                || cell
                    .text
                    .chars()
                    .all(|character| " ()[]{}'\",`".contains(character)))
    };
    if cells.get(column).is_none_or(is_separator) {
        return (column, column);
    }
    let start = (0..column)
        .rev()
        .take_while(|index| !is_separator(&cells[*index]))
        .last()
        .unwrap_or(column);
    let end = (column..cells.len())
        .take_while(|index| !is_separator(&cells[*index]))
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
            build_mouse_button_report(
                0,
                true,
                point,
                MouseReportModifiers::default(),
                &reporting_modes()
            )
            .as_deref(),
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

    fn drag_reporting_modes() -> ScreenInputModes {
        ScreenInputModes {
            is_mouse_drag_reporting: true,
            ..reporting_modes()
        }
    }

    fn motion_reporting_modes() -> ScreenInputModes {
        ScreenInputModes {
            is_mouse_motion_reporting: true,
            ..reporting_modes()
        }
    }

    #[test]
    fn middle_and_right_buttons_report_press_and_release() {
        let point = GridPoint { row: 0, column: 0 };
        let no_modifiers = MouseReportModifiers::default();
        let modes = reporting_modes();

        let middle_press = build_mouse_button_report(1, true, point, no_modifiers, &modes);
        let right_release = build_mouse_button_report(2, false, point, no_modifiers, &modes);

        assert_eq!(middle_press.as_deref(), Some("\x1b[<1;1;1M"));
        assert_eq!(right_release.as_deref(), Some("\x1b[<2;1;1m"));
    }

    #[test]
    fn button_reports_carry_modifier_bits() {
        let point = GridPoint { row: 0, column: 0 };
        let control_alt = MouseReportModifiers {
            is_alt_pressed: true,
            is_control_pressed: true,
            ..MouseReportModifiers::default()
        };

        let report = build_mouse_button_report(0, true, point, control_alt, &reporting_modes());

        assert_eq!(report.as_deref(), Some("\x1b[<24;1;1M"));
    }

    #[test]
    fn legacy_encoding_reports_release_as_button_three() {
        let point = GridPoint { row: 0, column: 0 };
        let legacy_modes = ScreenInputModes {
            is_sgr_mouse: false,
            ..reporting_modes()
        };

        let report = build_mouse_button_report(
            2,
            false,
            point,
            MouseReportModifiers::default(),
            &legacy_modes,
        );

        assert_eq!(report.as_deref(), Some("\x1b[M#!!"));
    }

    #[test]
    fn dragging_reports_motion_in_button_event_mode() {
        let point = GridPoint { row: 2, column: 3 };
        let no_modifiers = MouseReportModifiers::default();

        let drag = build_mouse_motion_report(Some(0), point, no_modifiers, &drag_reporting_modes());
        let hover = build_mouse_motion_report(None, point, no_modifiers, &drag_reporting_modes());

        assert_eq!(drag.as_deref(), Some("\x1b[<32;4;3M"));
        assert_eq!(hover, None);
    }

    #[test]
    fn any_motion_mode_reports_moves_without_a_button() {
        let point = GridPoint { row: 0, column: 1 };

        let hover = build_mouse_motion_report(
            None,
            point,
            MouseReportModifiers::default(),
            &motion_reporting_modes(),
        );

        assert_eq!(hover.as_deref(), Some("\x1b[<35;2;1M"));
    }

    #[test]
    fn click_only_reporting_sends_no_motion() {
        let point = GridPoint { row: 0, column: 0 };

        let drag = build_mouse_motion_report(
            Some(0),
            point,
            MouseReportModifiers::default(),
            &reporting_modes(),
        );

        assert_eq!(drag, None);
    }

    #[test]
    fn partial_wheel_amounts_accumulate_into_whole_lines() {
        let mut remainder_lines = 0.0;

        let scrolled_lines: Vec<i32> = [0.3, 0.3, 0.3, 0.3]
            .into_iter()
            .map(|delta_lines| take_whole_wheel_lines(&mut remainder_lines, delta_lines))
            .collect();

        assert_eq!(scrolled_lines, vec![0, 0, 0, 1]);
        assert!((remainder_lines - 0.2).abs() < 1e-5);
    }

    #[test]
    fn reversing_the_wheel_direction_drops_the_previous_remainder() {
        let mut remainder_lines = 0.0;
        take_whole_wheel_lines(&mut remainder_lines, 0.8);

        let scrolled_lines = take_whole_wheel_lines(&mut remainder_lines, -0.5);

        assert_eq!(scrolled_lines, 0);
        assert!((remainder_lines + 0.5).abs() < 1e-5);
    }

    #[test]
    fn whole_line_wheel_steps_scroll_immediately() {
        let mut remainder_lines = 0.0;

        assert_eq!(take_whole_wheel_lines(&mut remainder_lines, -3.0), -3);
        assert_eq!(remainder_lines, 0.0);
    }

    fn screen_cell(text: &str, width: usize) -> ScreenCell {
        ScreenCell {
            text: text.to_string(),
            width,
            fg: kanvibe_terminal::screen::CellColor::Default,
            bg: kanvibe_terminal::screen::CellColor::Default,
            is_bold: false,
            is_dim: false,
            is_italic: false,
            is_underline: false,
            is_inverse: false,
            is_strikethrough: false,
            is_hidden: false,
        }
    }

    fn narrow_cells(line: &str) -> Vec<ScreenCell> {
        line.chars()
            .map(|character| screen_cell(&character.to_string(), 1))
            .collect()
    }

    #[test]
    fn double_click_selects_the_word_under_the_pointer() {
        let cells = narrow_cells("git  status --short");
        assert_eq!(word_bounds(&cells, 6), (5, 10));
        assert_eq!(word_bounds(&cells, 3), (3, 3));
    }

    #[test]
    fn double_click_selects_a_whole_word_of_wide_characters() {
        let cells = vec![
            screen_cell("한", 2),
            screen_cell("", 0),
            screen_cell("글", 2),
            screen_cell("", 0),
            screen_cell("", 1),
            screen_cell("a", 1),
        ];
        assert_eq!(word_bounds(&cells, 0), (0, 3));
        assert_eq!(word_bounds(&cells, 3), (0, 3));
        assert_eq!(word_bounds(&cells, 4), (4, 4));
    }
}
