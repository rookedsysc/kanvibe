//! 데스크탑 터미널 뷰가 화면을 그릴 때 읽는 값. 읽기 전용이고 직렬화·pane chat 판정에는 영향이 없다.
//!
//! 뷰는 레지스트리가 가진 화면 하나만 읽는다. 클라이언트 쪽에 VT 해석기를 하나 더 두지 않으려는 것이다.

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::CursorShape;

use super::TerminalScreen;
use super::cells::{CellColor, cell_chars, cell_width};

/// 셀 하나의 글자와 꾸밈. `text`는 빈 칸과 넓은 글자의 뒤 칸에서 빈 문자열이다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCell {
    pub text: String,
    /// 0(넓은 글자 뒤 칸), 1, 2(넓은 글자)
    pub width: usize,
    pub fg: CellColor,
    pub bg: CellColor,
    pub is_bold: bool,
    pub is_dim: bool,
    pub is_italic: bool,
    pub is_underline: bool,
    pub is_inverse: bool,
    pub is_strikethrough: bool,
    pub is_hidden: bool,
}

impl ScreenCell {
    fn of(cell: &Cell) -> Self {
        let flags = cell.flags;
        Self {
            text: cell_chars(cell),
            width: cell_width(cell),
            fg: CellColor::from_alacritty(cell.fg),
            bg: CellColor::from_alacritty(cell.bg),
            is_bold: flags.contains(Flags::BOLD),
            is_dim: flags.contains(Flags::DIM),
            is_italic: flags.contains(Flags::ITALIC),
            is_underline: flags.intersects(Flags::ALL_UNDERLINES),
            is_inverse: flags.contains(Flags::INVERSE),
            is_strikethrough: flags.contains(Flags::STRIKEOUT),
            is_hidden: flags.contains(Flags::HIDDEN),
        }
    }
}

/// 커서 모양
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenCursorShape {
    Block,
    Underline,
    Bar,
}

/// 보이는 영역 기준 커서. 스크롤백을 보는 중이거나 프로그램이 커서를 숨기면 `is_visible`이 거짓이다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenCursor {
    pub column: usize,
    pub row: usize,
    pub shape: ScreenCursorShape,
    pub is_visible: bool,
}

/// 키·마우스 입력을 PTY 바이트로 바꿀 때 보는 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenInputModes {
    /// DECCKM. 방향키를 `ESC O A`로 보낸다
    pub is_application_cursor: bool,
    pub is_bracketed_paste: bool,
    /// 프로그램이 마우스 보고를 켰는지(클릭·드래그·이동 중 하나)
    pub is_mouse_reporting: bool,
    pub is_mouse_motion_reporting: bool,
    pub is_mouse_drag_reporting: bool,
    pub is_sgr_mouse: bool,
    pub is_alternate_screen: bool,
    /// 대체 화면에서 휠을 방향키로 보내는 모드(`?1007`)
    pub is_alternate_scroll: bool,
    pub is_focus_reporting: bool,
}

impl TerminalScreen {
    /// 보이는 영역의 셀 한 줄. 스크롤백을 보는 중이면 그만큼 위 줄이다
    pub fn visible_cells(&self, screen_row: usize) -> Option<Vec<ScreenCell>> {
        let grid = self.term.grid();
        if screen_row >= grid.screen_lines() {
            return None;
        }
        let line = Line(screen_row as i32 - grid.display_offset() as i32);
        let row = &grid[line];
        Some(
            (0..grid.columns())
                .map(|column| ScreenCell::of(&row[Column(column)]))
                .collect(),
        )
    }

    pub fn cursor(&self) -> ScreenCursor {
        let grid = self.term.grid();
        let point = grid.cursor.point;
        let style = self.term.cursor_style();
        let shape = match style.shape {
            CursorShape::Underline => ScreenCursorShape::Underline,
            CursorShape::Beam => ScreenCursorShape::Bar,
            _ => ScreenCursorShape::Block,
        };
        let is_shown =
            self.term.mode().contains(TermMode::SHOW_CURSOR) && style.shape != CursorShape::Hidden;
        ScreenCursor {
            column: point.column.0,
            row: point.line.0.max(0) as usize,
            shape,
            is_visible: is_shown && grid.display_offset() == 0,
        }
    }

    pub fn input_modes(&self) -> ScreenInputModes {
        let mode = *self.term.mode();
        ScreenInputModes {
            is_application_cursor: mode.contains(TermMode::APP_CURSOR),
            is_bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
            is_mouse_reporting: mode.intersects(TermMode::MOUSE_MODE),
            is_mouse_motion_reporting: mode.contains(TermMode::MOUSE_MOTION),
            is_mouse_drag_reporting: mode.contains(TermMode::MOUSE_DRAG),
            is_sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
            is_alternate_screen: mode.contains(TermMode::ALT_SCREEN),
            is_alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            is_focus_reporting: mode.contains(TermMode::FOCUS_IN_OUT),
        }
    }

    /// 스크롤백을 보는 위치. 0이면 맨 아래(지금 화면)다
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn history_size(&self) -> usize {
        let grid = self.term.grid();
        grid.total_lines() - grid.screen_lines()
    }

    /// 스크롤백 보기 위치를 옮긴다. 양수가 위(과거)다. 화면 내용과 직렬화는 바뀌지 않는다
    pub fn scroll_display(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_display_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// 뷰가 쓰는 셀 픽셀 크기. `CSI 14t` 같은 크기 질의 응답에만 쓴다
    pub fn set_cell_pixel_size(&mut self, width: u16, height: u16) {
        self.events.set_cell_pixel_size(width, height);
    }

    /// 해석 중에 쌓인 질의 응답을 꺼낸다. 레지스트리가 데스크탑 뷰가 붙어 있을 때만 PTY에 쓴다
    pub fn take_query_replies(&mut self) -> Vec<String> {
        self.events.take_replies()
    }
}
