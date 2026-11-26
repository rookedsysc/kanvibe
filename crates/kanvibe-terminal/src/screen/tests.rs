use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use kanvibe_ai::pane_chat::codex_display::TerminalCellRow;

use super::cells::{CellStyle, cell_chars, is_blank_cell};
use super::{
    LONGEST_ALTERNATE_ENTRY, SCROLLBACK_LINES, TerminalScreen, VisibleSelection, grid_cell,
};

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
fn an_alternate_entry_split_across_output_chunks_still_keeps_the_primary_buffer() {
    let entry = b"\x1b[?1049h";
    for split in 1..entry.len() {
        let mut screen = TerminalScreen::new(20, 4);
        screen.write(b"shell history\r\n$ vim");
        screen.write(&entry[..split]);
        screen.write(&[&entry[split..], &b"\x1b[Heditor body"[..]].concat());

        assert!(
            screen.primary_grid_while_alternate.is_some(),
            "split at {split}"
        );
    }
}

#[test]
fn a_large_output_chunk_keeps_only_a_short_tail_for_the_next_chunk() {
    let mut screen = TerminalScreen::new(20, 4);
    screen.write(&vec![b'x'; 64 * 1024]);

    assert_eq!(screen.recent_tail.len(), LONGEST_ALTERNATE_ENTRY - 1);
    assert!(screen.recent_tail.capacity() < 64);
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

#[test]
fn view_cells_expose_colors_and_attributes_without_changing_the_snapshot() {
    let mut screen = TerminalScreen::new(20, 4);
    screen.write(b"\x1b[2;4;7;38;2;1;2;3;48;5;200mA\x1b[0m\xed\x95\x9c");
    let before = screen.serialize();

    let cells = screen.visible_cells(0).unwrap();
    assert_eq!(cells[0].text, "A");
    assert!(cells[0].is_dim && cells[0].is_underline && cells[0].is_inverse);
    assert_eq!(cells[0].fg, super::CellColor::Rgb(1, 2, 3));
    assert_eq!(cells[0].bg, super::CellColor::Palette(200));
    assert_eq!((cells[1].text.as_str(), cells[1].width), ("한", 2));
    assert_eq!(cells[2].width, 0);
    assert_eq!(screen.cursor().column, 3);
    assert_eq!(screen.serialize(), before);
}

#[test]
fn copying_into_a_reused_cell_buffer_overwrites_the_previous_frame() {
    let mut screen = TerminalScreen::new(4, 2);
    screen.write(b"\x1b[1;41mAB\x1b[0m\xed\x95\x9c");
    let mut cells = Vec::new();
    assert!(screen.copy_visible_cells_into(0, &mut cells));
    assert_eq!(cells, screen.visible_cells(0).unwrap());

    screen.write(b"\x1b[H\x1b[2Kx");
    assert!(screen.copy_visible_cells_into(0, &mut cells));
    assert_eq!(cells, screen.visible_cells(0).unwrap());
    assert_eq!(cells.len(), 4);
    assert_eq!((cells[0].text.as_str(), cells[0].is_bold), ("x", false));
    assert_eq!((cells[2].text.as_str(), cells[2].width), ("", 1));

    assert!(!screen.copy_visible_cells_into(2, &mut cells));
    assert!(cells.is_empty());
}

#[test]
fn query_replies_are_collected_once_and_size_queries_use_the_screen_size() {
    let mut screen = TerminalScreen::new(80, 24);
    screen.write(b"\x1b[5n\x1b[18t");

    assert_eq!(
        screen.take_query_replies(),
        ["\x1b[0n".to_string(), "\x1b[8;24;80t".to_string()]
    );
    assert!(screen.take_query_replies().is_empty());
}

#[test]
fn input_modes_follow_the_program_requests() {
    let mut screen = TerminalScreen::new(80, 24);
    screen.write(b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1049h");

    let modes = screen.input_modes();
    assert!(modes.is_application_cursor && modes.is_bracketed_paste);
    assert!(modes.is_mouse_reporting && modes.is_sgr_mouse && modes.is_alternate_screen);
}

#[test]
fn scrolling_the_view_shows_history_and_hides_the_cursor() {
    let mut screen = TerminalScreen::new(10, 2);
    screen.write(b"one\r\ntwo\r\nthree");
    screen.scroll_display(1);

    assert_eq!(screen.display_offset(), 1);
    assert_eq!(screen.visible_cells(0).unwrap()[0].text, "o");
    assert!(!screen.cursor().is_visible);
    screen.scroll_display_to_bottom();
    assert_eq!(screen.visible_cells(1).unwrap()[0].text, "t");
}

#[test]
fn an_unfinished_synchronized_update_is_shown_once_its_time_limit_passes() {
    let mut screen = TerminalScreen::new(10, 2);
    screen.write(b"\x1b[?2026hprompt");
    let deadline = screen
        .pending_sync_deadline()
        .expect("동기화 출력을 모으는 중");

    screen.finish_expired_sync(deadline - std::time::Duration::from_millis(1));
    assert_eq!(screen.viewport_lines()[0], "");

    screen.finish_expired_sync(deadline);
    assert_eq!(screen.viewport_lines()[0], "prompt");
    assert_eq!(screen.pending_sync_deadline(), None);
}

#[test]
fn a_selection_keeps_its_text_after_output_pushes_the_screen_up() {
    let mut screen = TerminalScreen::new(10, 3);
    screen.write(b"alpha\r\nbravo\r\ncharlie");
    screen.select_visible_range((1, 0), (1, 4));

    screen.write(b"\r\ndelta\r\necho");

    assert_eq!(screen.selected_text().as_deref(), Some("bravo"));
    let shown = screen.visible_selection();
    assert_eq!(shown, None, "bravo는 이제 보기 위 스크롤백에 있다");
    screen.scroll_display(2);
    assert_eq!(
        screen.visible_selection(),
        Some(VisibleSelection {
            start_row: 1,
            start_column: 0,
            end_row: 1,
            end_column: 4,
        })
    );
}

#[test]
fn a_selection_made_while_scrolled_back_points_at_the_history_line() {
    let mut screen = TerminalScreen::new(10, 2);
    screen.write(b"one\r\ntwo\r\nthree");
    screen.scroll_display(1);
    screen.select_visible_range((0, 0), (0, 2));

    screen.scroll_display_to_bottom();

    assert_eq!(screen.selected_text().as_deref(), Some("one"));
}

#[test]
fn a_press_without_a_drag_selects_nothing() {
    let mut screen = TerminalScreen::new(10, 2);
    screen.write(b"one");
    screen.start_selection(0, 1);

    assert_eq!(screen.selected_text(), None);
    assert_eq!(screen.visible_selection(), None);
}

#[test]
fn a_selection_pushed_out_of_the_scrollback_is_cleared() {
    let mut screen = TerminalScreen::new(10, 2);
    screen.write(b"first");
    screen.select_visible_range((0, 0), (0, 4));

    for _ in 0..SCROLLBACK_LINES + 2 {
        screen.write(b"\r\nmore");
    }

    assert_eq!(screen.selected_text(), None);
}

#[test]
fn a_web_address_is_found_under_the_clicked_cell_without_trailing_punctuation() {
    let mut screen = TerminalScreen::new(40, 2);
    screen.write(b"see https://example.com/a?b=1. ok");

    assert_eq!(
        screen.link_at(0, 10).as_deref(),
        Some("https://example.com/a?b=1")
    );
    assert_eq!(screen.link_at(0, 30), None);
    assert_eq!(screen.link_at(0, 1), None);
}

#[test]
fn a_web_address_wrapped_onto_the_next_row_is_found_from_either_row() {
    let mut screen = TerminalScreen::new(16, 3);
    screen.write(b"go https://example.com/long/path");

    let expected = Some("https://example.com/long/path");
    assert_eq!(screen.link_at(0, 5).as_deref(), expected);
    assert_eq!(screen.link_at(1, 3).as_deref(), expected);
}

#[test]
fn an_osc8_hyperlink_opens_its_target_rather_than_its_label() {
    let mut screen = TerminalScreen::new(40, 2);
    screen.write(b"\x1b]8;;https://example.com/pr/1\x1b\\PR #1\x1b]8;;\x1b\\ done");

    assert_eq!(
        screen.link_at(0, 2).as_deref(),
        Some("https://example.com/pr/1")
    );
    assert_eq!(screen.link_at(0, 8), None);
}
