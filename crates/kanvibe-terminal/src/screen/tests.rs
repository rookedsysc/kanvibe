use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use kanvibe_ai::pane_chat::codex_display::TerminalCellRow;

use super::cells::{CellStyle, cell_chars, is_blank_cell};
use super::{TerminalScreen, grid_cell};

/// 화면의 모든 줄(스크롤백 포함)을 보이는 글자와 속성으로 읽는다.
/// 빈 칸은 xterm 직렬화처럼 배경만 옮겨지므로 배경만 비교한다
fn cell_dump(screen: &TerminalScreen) -> Vec<Vec<(String, CellStyle)>> {
    let grid = screen.term.grid();
    (0..grid.total_lines())
        .map(|row| {
            (0..grid.columns())
                .map(|column| {
                    let cell = grid_cell(grid, row, column);
                    let style = CellStyle::of(cell);
                    match is_blank_cell(cell) {
                        true => (
                            String::new(),
                            CellStyle {
                                bg: style.bg,
                                ..CellStyle::DEFAULT
                            },
                        ),
                        false => (cell_chars(cell), style),
                    }
                })
                .collect()
        })
        .collect()
}

fn replay(screen: &TerminalScreen) -> TerminalScreen {
    let mut restored = TerminalScreen::new(screen.columns() as u16, screen.rows() as u16);
    restored.write(screen.serialize().as_bytes());
    restored
}

fn assert_round_trip(screen: &TerminalScreen) {
    let restored = replay(screen);
    assert_eq!(cell_dump(&restored), cell_dump(screen));
    assert_eq!(restored.cursor_row(), screen.cursor_row());
    assert_eq!(restored.cursor_column(), screen.cursor_column());
}

#[test]
fn plain_text_serializes_to_the_same_bytes_xterm_would_write() {
    let mut screen = TerminalScreen::new(20, 4);
    screen.write(b"hello\r\nworld");

    assert_eq!(screen.serialize(), "hello\r\nworld");
}

#[test]
fn colors_and_attributes_survive_a_replay() {
    let mut screen = TerminalScreen::new(30, 5);
    screen.write(b"\x1b[1;31mbold red\x1b[0m \x1b[38;5;200mpalette\x1b[0m\r\n");
    screen.write(b"\x1b[38;2;1;2;3m\x1b[48;5;17mrgb on blue\x1b[0m\r\n\x1b[2mdim\x1b[22m \x1b[4;7munder\x1b[0m");

    assert_round_trip(&screen);
}

#[test]
fn wrapped_rows_are_joined_so_the_receiver_wraps_at_the_same_place() {
    let mut screen = TerminalScreen::new(10, 4);
    screen.write(b"abcdefghijKLMNO");

    let serialized = screen.serialize();

    assert!(!serialized.contains("\r\n"), "{serialized:?}");
    assert_round_trip(&screen);
}

#[test]
fn scrollback_rows_are_replayed_before_the_visible_screen() {
    let mut screen = TerminalScreen::new(12, 3);
    for line in 0..8 {
        screen.write(format!("line {line}\r\n").as_bytes());
    }
    screen.write(b"prompt$ ");

    assert_round_trip(&screen);
    assert_eq!(replay(&screen).viewport_lines(), screen.viewport_lines());
}

#[test]
fn background_colored_blank_cells_are_erased_with_their_color() {
    let mut screen = TerminalScreen::new(16, 3);
    screen.write(b"\x1b[44m    \x1b[0mtext\r\n\x1b[41m\x1b[K\x1b[0m");

    assert_round_trip(&screen);
}

#[test]
fn wide_characters_take_two_cells_and_replay_in_place() {
    let mut screen = TerminalScreen::new(9, 3);
    screen.write("한글 입력창".as_bytes());

    assert_eq!(screen.viewport_lines()[0], "한글 입력");
    assert_round_trip(&screen);
}

#[test]
fn alternate_screen_keeps_the_primary_buffer_behind_it() {
    let mut screen = TerminalScreen::new(20, 4);
    screen.write(b"shell history\r\n$ vim");
    screen.write(b"\x1b[?1049h\x1b[Heditor body");

    let serialized = screen.serialize();
    let mut restored = TerminalScreen::new(20, 4);
    restored.write(serialized.as_bytes());

    // 빈 칸은 xterm과 같이 커서 이동으로 줄여 쓴다
    assert!(serialized.starts_with("shell\u{1b}[1Chistory\r\n$\u{1b}[1Cvim\u{1b}[?1049h"));
    assert_eq!(restored.viewport_lines()[0], "editor body");
    restored.write(b"\x1b[?1049l");
    assert_eq!(restored.viewport_lines()[..2], ["shell history", "$ vim"]);
}

#[test]
fn terminal_modes_are_restored_after_the_screen() {
    let mut screen = TerminalScreen::new(20, 4);
    screen.write(b"\x1b[?2004h\x1b[?1h\x1b[?1000h\x1b[?7lx");

    let serialized = screen.serialize();

    assert!(
        serialized.ends_with("\x1b[?1h\x1b[?2004h\x1b[?7l\x1b[?1000h"),
        "{serialized:?}"
    );
}

#[test]
fn pending_input_attributes_are_kept_for_the_next_output() {
    let mut screen = TerminalScreen::new(20, 4);
    screen.write(b"plain \x1b[32m");

    let mut restored = replay(&screen);
    restored.write(b"green");

    let cell = &restored.term.grid()[Line(0)][Column(6)];
    assert_eq!(cell.c, 'g');
    assert_eq!(CellStyle::of(cell).fg, super::cells::CellColor::Palette(2));
}

#[test]
fn cursor_after_the_last_column_waits_for_wrap_like_xterm() {
    let mut screen = TerminalScreen::new(5, 3);
    screen.write(b"12345");

    assert_eq!(screen.cursor_column(), 5);
    assert_round_trip(&screen);
}

#[test]
fn reading_returns_drawn_text_without_color_or_cursor_bytes() {
    let mut screen = TerminalScreen::new(30, 4);
    screen.write(b"\x1b[31mred\x1b[0m line\r\n\x1b[2K> typed\x1b[3D");

    let reading = screen.read();

    assert_eq!(reading.viewport_lines[0], "red line");
    assert_eq!(reading.input_line, "> typed");
    assert_eq!(reading.screen, "red line\n> typed");
    assert_eq!((reading.cursor_column, reading.cursor_row), (4, 1));
    assert_eq!(reading.viewport_columns, 30);
    assert!(!reading.is_input_placeholder);
}

#[test]
fn dim_codex_composer_hint_is_detected_from_cell_attributes() {
    let mut dim_hint = TerminalScreen::new(40, 3);
    dim_hint.write("› \x1b[2mAsk Codex to do anything\x1b[22m\x1b[24D".as_bytes());
    let mut typed_hint = TerminalScreen::new(40, 3);
    typed_hint.write("› Ask Codex to do anything\x1b[24D".as_bytes());

    assert!(dim_hint.read().is_input_placeholder);
    assert!(!typed_hint.read().is_input_placeholder);
}

#[test]
fn screen_row_reports_dim_cells_and_blank_cells_like_xterm() {
    let mut screen = TerminalScreen::new(10, 2);
    screen.write(b"a\x1b[2mb\x1b[22m");
    let row = screen.row(0).expect("첫 줄");

    assert_eq!(row.column_count(), 10);
    assert_eq!(row.cell_chars(0).as_deref(), Some("a"));
    assert!(row.is_cell_dim(1));
    assert_eq!(row.cell_chars(5).as_deref(), Some(""));
    assert_eq!(row.cell_chars(10), None);
    assert_eq!(row.trimmed_text(), "ab");
}

#[test]
fn resize_reflows_and_keeps_serializing() {
    let mut screen = TerminalScreen::new(20, 4);
    screen.write(b"first\r\nsecond line\r\n$ ");
    screen.resize(8, 4);

    assert_round_trip(&screen);
}
