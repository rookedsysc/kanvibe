//! daemon PTY의 headless 화면(TS `@xterm/headless` + `SerializeAddon`, 스크롤백 200줄).
//!
//! 다시 붙는 데스크탑에 지금 화면을 그려 주는 ANSI 문자열만 만든다. 받는 쪽 터미널이 같은 크기라면
//! 스크롤백과 화면 줄을 차례로 찍은 뒤 커서를 옮겨 같은 화면이 된다. xterm 직렬화와 바이트 단위로
//! 같지는 않다(데스크탑 `kanvibe_terminal::screen`은 kanvibe-db를 끌고 와 정적 바이너리에 넣지 않는다).

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Color, Processor};

const SCROLLBACK_LINES: usize = 200;

struct ScreenSize {
    columns: usize,
    lines: usize,
}

impl Dimensions for ScreenSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

pub struct DaemonScreen {
    term: Term<VoidListener>,
    parser: Processor,
}

impl DaemonScreen {
    pub fn new(columns: u16, rows: u16) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size(columns, rows), VoidListener),
            parser: Processor::new(),
        }
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.term.resize(size(columns, rows));
    }

    /// 스크롤백, 화면, 커서 위치, 켜진 입력 모드를 다시 그리는 ANSI 문자열
    pub fn serialize(&self) -> String {
        let grid = self.term.grid();
        let mode = *self.term.mode();
        let mut content = String::new();
        if mode.contains(TermMode::ALT_SCREEN) {
            content.push_str("\u{1b}[?1049h\u{1b}[H");
        }
        let history = grid.total_lines() - grid.screen_lines();
        let first_line = -(history as i32);
        let last_line = grid.screen_lines() as i32 - 1;
        for line in first_line..=last_line {
            push_line(&mut content, grid, Line(line));
            let is_wrapped = grid[Line(line)][Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE);
            if line < last_line && !is_wrapped {
                content.push_str("\r\n");
            }
        }
        let cursor = grid.cursor.point;
        content.push_str(&format!(
            "\u{1b}[0m\u{1b}[{};{}H",
            cursor.line.0 + 1,
            cursor.column.0 + 1
        ));
        content.push_str(&mode_sequences(mode));
        content
    }
}

fn size(columns: u16, rows: u16) -> ScreenSize {
    ScreenSize {
        columns: usize::from(columns.max(1)),
        lines: usize::from(rows.max(1)),
    }
}

/// 줄 하나를 SGR이 바뀐 자리에서만 속성을 내며 찍는다. 끝의 기본 빈칸은 내지 않는다
fn push_line(content: &mut String, grid: &Grid<Cell>, line: Line) {
    let row = &grid[line];
    let last_visible = (0..grid.columns())
        .rev()
        .find(|column| !is_default_blank(&row[Column(*column)]));
    let Some(last_visible) = last_visible else {
        return;
    };
    let mut previous_style = String::from("0");
    for column in 0..=last_visible {
        let cell = &row[Column(column)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let style = cell_style(cell);
        if style != previous_style {
            content.push_str(&format!("\u{1b}[{style}m"));
            previous_style = style;
        }
        content.push(cell.c);
        for zero_width in cell.zerowidth().unwrap_or_default() {
            content.push(*zero_width);
        }
    }
    if previous_style != "0" {
        content.push_str("\u{1b}[0m");
    }
}

fn is_default_blank(cell: &Cell) -> bool {
    cell.c == ' ' && cell_style(cell) == "0"
}

/// 셀 속성을 SGR 매개변수로 만든다. 기본 속성이면 `0`이다
fn cell_style(cell: &Cell) -> String {
    let flag_codes = [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::UNDERLINE, "4"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ];
    let mut parameters = vec!["0".to_string()];
    for (flag, code) in flag_codes {
        if cell.flags.contains(flag) {
            parameters.push(code.to_string());
        }
    }
    parameters.extend(color_parameters(cell.fg, 30));
    parameters.extend(color_parameters(cell.bg, 40));
    parameters.join(";")
}

/// 전경색은 30번대, 배경색은 40번대 기준으로 색을 낸다. 기본색은 내지 않는다
fn color_parameters(color: Color, base: u8) -> Option<String> {
    match color {
        Color::Named(named) => {
            let index = named as usize;
            match index {
                0..8 => Some((base as usize + index).to_string()),
                8..16 => Some((base as usize + 60 + index - 8).to_string()),
                _ => None,
            }
        }
        Color::Indexed(index) => Some(format!("{};5;{index}", base + 8)),
        Color::Spec(rgb) => Some(format!("{};2;{};{};{}", base + 8, rgb.r, rgb.g, rgb.b)),
    }
}

/// 다시 붙은 터미널이 같은 입력 방식을 쓰도록 켜진 모드를 다시 켠다
fn mode_sequences(mode: TermMode) -> String {
    let flagged = [
        (TermMode::APP_CURSOR, "\u{1b}[?1h"),
        (TermMode::APP_KEYPAD, "\u{1b}="),
        (TermMode::BRACKETED_PASTE, "\u{1b}[?2004h"),
        (TermMode::MOUSE_REPORT_CLICK, "\u{1b}[?1000h"),
        (TermMode::MOUSE_DRAG, "\u{1b}[?1002h"),
        (TermMode::MOUSE_MOTION, "\u{1b}[?1003h"),
        (TermMode::SGR_MOUSE, "\u{1b}[?1006h"),
        (TermMode::FOCUS_IN_OUT, "\u{1b}[?1004h"),
    ];
    let mut sequences: String = flagged
        .iter()
        .filter(|(flag, _)| mode.contains(*flag))
        .map(|(_, sequence)| *sequence)
        .collect();
    if !mode.contains(TermMode::SHOW_CURSOR) {
        sequences.push_str("\u{1b}[?25l");
    }
    sequences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaying_the_snapshot_reproduces_the_visible_screen() {
        let mut screen = DaemonScreen::new(20, 4);
        screen.write(b"one\r\n\x1b[1;31mred\x1b[0m\r\nthree\r\nfour\r\nfive");

        let mut replay = DaemonScreen::new(20, 4);
        replay.write(screen.serialize().as_bytes());

        assert_eq!(visible_text(&replay), visible_text(&screen));
        assert_eq!(
            replay.term.grid().cursor.point,
            screen.term.grid().cursor.point
        );
        assert!(screen.serialize().contains("\u{1b}[0;1;31mred"));
    }

    fn visible_text(screen: &DaemonScreen) -> Vec<String> {
        let grid = screen.term.grid();
        (0..grid.screen_lines() as i32)
            .map(|line| {
                let row = &grid[Line(line)];
                (0..grid.columns())
                    .map(|column| row[Column(column)].c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }
}
