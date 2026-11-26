//! 화면 없이 PTY 출력을 해석해 두는 headless 터미널(TS `@xterm/headless` + `SerializeAddon` 자리).
//!
//! 멀티플렉서 없는 terminal 세션은 나중에 붙는 클라이언트에게 지금 화면을 다시 그려 줄 곳이 없어,
//! PTY 출력을 이 화면에도 흘려 두고 붙는 순간 ANSI로 직렬화해 건넨다. pane chat은 같은 화면에서
//! 입력란 줄과 커서, 셀의 dim 속성을 읽는다(Codex `/statusline` 예외 판정이 dim 정보에 기댄다).

mod cells;
mod events;
mod links;
mod selection;
mod serialize;
#[cfg(test)]
mod tests;
mod view;

use std::time::Instant;

use alacritty_terminal::Term;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::Processor;
use kanvibe_ai::pane_chat::codex_display::{TerminalCellRow, is_dim_hint_empty_composer};

pub use cells::{CellColor, ScreenRow};
pub use events::{TERMINAL_BACKGROUND, TERMINAL_CURSOR, TERMINAL_FOREGROUND, default_color};
pub use selection::VisibleSelection;
pub use view::{ScreenCell, ScreenCursor, ScreenCursorShape, ScreenInputModes};

use events::ScreenEvents;

/// xterm.js 기본 스크롤백 줄 수. headless xterm을 옵션 없이 만들던 TS와 같은 양을 기억한다
pub const SCROLLBACK_LINES: usize = 1000;

/// 대체 화면 진입 시퀀스. 진입 직전의 기본 화면을 붙잡아 두는 데 쓴다
const ALTERNATE_SCREEN_ENTRIES: [&[u8]; 3] = [b"\x1b[?1049h", b"\x1b[?1047h", b"\x1b[?47h"];
const LONGEST_ALTERNATE_ENTRY: usize = 8;

#[derive(Debug, Clone, Copy)]
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

/// 화면에서 읽은 지금 상태. TS `readLocalTerminalTabSnapshot`과 pane chat surface가 돌려주던 모양이다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenReading {
    /// 보이는 줄을 이어 붙이고 끝 공백을 뗀 화면 글
    pub screen: String,
    /// 커서가 있는 줄의 글
    pub input_line: String,
    pub cursor_column: usize,
    pub cursor_row: usize,
    pub viewport_lines: Vec<String>,
    pub viewport_columns: usize,
    /// 커서 줄이 Codex의 dim 처리된 빈 입력란 안내인지
    pub is_input_placeholder: bool,
}

pub struct TerminalScreen {
    term: Term<ScreenEvents>,
    /// 질의 응답이 쌓이는 자리. `term`이 같은 값을 복제해 쥐고 있다
    events: ScreenEvents,
    parser: Processor,
    /// 대체 화면에 있는 동안 가려진 기본 화면. alacritty는 비활성 화면을 내주지 않아 진입 직전에 복사해 둔다
    primary_grid_while_alternate: Option<Grid<Cell>>,
    /// 진입 시퀀스가 출력 조각 경계에 걸려도 찾도록 앞 조각의 끝을 남긴다
    recent_tail: Vec<u8>,
}

impl TerminalScreen {
    pub fn new(columns: u16, rows: u16) -> Self {
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            ..Config::default()
        };
        let events = ScreenEvents::new(columns, rows);
        Self {
            term: Term::new(config, &screen_size(columns, rows), events.clone()),
            events,
            parser: Processor::new(),
            primary_grid_while_alternate: None,
            recent_tail: Vec::new(),
        }
    }

    /// PTY 출력 한 조각을 해석한다. 돌아오면 그 조각은 화면에 모두 반영돼 있다
    pub fn write(&mut self, bytes: &[u8]) {
        self.finish_expired_sync(Instant::now());
        let was_alternate = self.is_alternate_screen();
        if !was_alternate && self.contains_alternate_entry(bytes) {
            self.primary_grid_while_alternate = Some(self.term.grid().clone());
        }
        self.parser.advance(&mut self.term, bytes);
        if !self.is_alternate_screen() {
            self.primary_grid_while_alternate = None;
        }
        self.remember_tail(bytes);
    }

    /// 동기화 출력(`CSI ? 2026 h`)이 끝나는 신호 없이 시간 제한을 넘겼으면 쌓아 둔 출력을 지금 화면에 반영한다.
    /// 프레임 중간에 죽은 TUI 뒤의 출력이 버퍼에만 쌓여 화면이 멈춘 것처럼 보이지 않게 한다(xterm.js의 동기화 출력 시간 제한)
    pub fn finish_expired_sync(&mut self, now: Instant) {
        if self
            .pending_sync_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            self.parser.stop_sync(&mut self.term);
        }
    }

    /// 동기화 출력을 모으는 중이면 시간 제한이 끝나는 시각
    pub fn pending_sync_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.term.resize(screen_size(columns, rows));
        self.events.set_size(columns, rows);
        self.primary_grid_while_alternate = None;
    }

    pub fn columns(&self) -> usize {
        self.term.grid().columns()
    }

    pub fn rows(&self) -> usize {
        self.term.grid().screen_lines()
    }

    /// xterm `buffer.cursorX`. 마지막 칸을 쓰고 줄바꿈을 기다리는 동안은 열 수와 같다
    pub fn cursor_column(&self) -> usize {
        let cursor = &self.term.grid().cursor;
        cursor.point.column.0 + usize::from(cursor.input_needs_wrap)
    }

    /// xterm `buffer.cursorY`. 화면 맨 위가 0이다
    pub fn cursor_row(&self) -> usize {
        self.term.grid().cursor.point.line.0.max(0) as usize
    }

    /// 화면의 한 줄. 보이는 영역 밖이면 `None`이다
    pub fn row(&self, screen_row: usize) -> Option<ScreenRow<'_>> {
        if screen_row >= self.rows() {
            return None;
        }
        let written_columns = match screen_row == self.cursor_row() {
            true => self.cursor_column(),
            false => 0,
        };
        Some(ScreenRow::new(
            &self.term.grid()[Line(screen_row as i32)],
            written_columns,
        ))
    }

    /// 보이는 줄마다 끝 공백을 뗀 글(xterm `translateToString(true)`)
    pub fn viewport_lines(&self) -> Vec<String> {
        (0..self.rows())
            .filter_map(|screen_row| self.row(screen_row))
            .map(|row| row.trimmed_text())
            .collect()
    }

    pub fn read(&self) -> ScreenReading {
        let viewport_lines = self.viewport_lines();
        let cursor_row = self.cursor_row();
        let cursor_column = self.cursor_column();
        let cursor_line = self.row(cursor_row);
        let input_line = cursor_line
            .as_ref()
            .map(TerminalCellRow::trimmed_text)
            .unwrap_or_default();
        let is_input_placeholder = is_dim_hint_empty_composer(
            cursor_line.as_ref().map(|row| row as &dyn TerminalCellRow),
            cursor_column,
        );
        ScreenReading {
            screen: viewport_lines.join("\n").trim_end().to_string(),
            input_line,
            cursor_column,
            cursor_row,
            viewport_lines,
            viewport_columns: self.columns(),
            is_input_placeholder,
        }
    }

    /// 지금 화면을 새 터미널에 그대로 다시 그리는 ANSI 문자열(xterm `SerializeAddon.serialize()`)
    pub fn serialize(&self) -> String {
        let template = &self.term.grid().cursor.template;
        let mut content = match self.is_alternate_screen() {
            true => self.serialize_with_alternate(template),
            false => serialize::serialize_grid(self.term.grid(), template),
        };
        content.push_str(&serialize::serialize_modes(*self.term.mode()));
        content
    }

    fn serialize_with_alternate(&self, template: &Cell) -> String {
        let primary = self
            .primary_grid_while_alternate
            .as_ref()
            .map(|grid| serialize::serialize_grid(grid, template))
            .unwrap_or_default();
        let alternate = serialize::serialize_grid(self.term.grid(), template);
        format!("{primary}\u{1b}[?1049h\u{1b}[H{alternate}")
    }

    fn is_alternate_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// 조각 경계에 걸친 시퀀스는 앞 조각의 끝과 새 조각의 앞부분만 이은 작은 버퍼에서 찾는다.
    /// 앞 조각의 끝에서 시작한 시퀀스는 새 조각으로 최대 `LONGEST_ALTERNATE_ENTRY - 1`바이트만 넘어오기 때문이다
    fn contains_alternate_entry(&self, bytes: &[u8]) -> bool {
        let head = &bytes[..bytes.len().min(LONGEST_ALTERNATE_ENTRY - 1)];
        let mut seam = [0u8; 2 * (LONGEST_ALTERNATE_ENTRY - 1)];
        let tail_length = self.recent_tail.len();
        let seam_length = tail_length + head.len();
        seam[..tail_length].copy_from_slice(&self.recent_tail);
        seam[tail_length..seam_length].copy_from_slice(head);
        has_alternate_entry(&seam[..seam_length]) || has_alternate_entry(bytes)
    }

    fn remember_tail(&mut self, bytes: &[u8]) {
        let kept_length = LONGEST_ALTERNATE_ENTRY - 1;
        if bytes.len() >= kept_length {
            self.recent_tail.clear();
            self.recent_tail
                .extend_from_slice(&bytes[bytes.len() - kept_length..]);
            return;
        }
        self.recent_tail.extend_from_slice(bytes);
        let excess = self.recent_tail.len().saturating_sub(kept_length);
        self.recent_tail.drain(..excess);
    }
}

/// ESC 자리에서만 진입 시퀀스를 비교한다
fn has_alternate_entry(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == 0x1b)
        .any(|(start, _)| {
            ALTERNATE_SCREEN_ENTRIES
                .iter()
                .any(|entry| bytes[start..].starts_with(entry))
        })
}

fn screen_size(columns: u16, rows: u16) -> ScreenSize {
    ScreenSize {
        columns: usize::from(columns.max(1)),
        lines: usize::from(rows.max(1)),
    }
}

/// 화면 맨 위 줄부터 센 행 번호를 alacritty 줄 번호로 바꾼다. 스크롤백이 음수 줄이다
fn grid_line(grid: &Grid<Cell>, absolute_row: usize) -> Line {
    let history = grid.total_lines() - grid.screen_lines();
    Line(absolute_row as i32 - history as i32)
}

fn grid_cell(grid: &Grid<Cell>, absolute_row: usize, column: usize) -> &Cell {
    &grid[grid_line(grid, absolute_row)][Column(column)]
}
