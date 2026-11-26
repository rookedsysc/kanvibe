//! 데스크탑 뷰가 드래그로 고른 글. 선택은 alacritty `Term`이 버퍼 줄에 묶어 쥐므로, 출력이 화면을 밀거나
//! 스크롤백 보기 위치가 바뀌어도 처음 고른 글을 가리킨다. 고른 줄이 스크롤백 밖으로 밀려나면 alacritty가 선택을 지운다
//! (xterm.js `SelectionService`가 버퍼 줄에 묶고 `onTrim`으로 옮기던 동작).

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};

use super::TerminalScreen;

/// 보이는 칸 좌표(0부터)로 옮긴 선택 범위. 시작이 끝보다 앞이고 두 칸을 모두 포함한다.
/// 일부가 보기 밖이면 보이는 가장자리까지로 자른다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibleSelection {
    pub start_row: usize,
    pub start_column: usize,
    pub end_row: usize,
    pub end_column: usize,
}

impl TerminalScreen {
    /// 보이는 칸에서 드래그 선택을 시작한다. 끝을 옮기기 전까지는 빈 선택이다
    pub fn start_selection(&mut self, row: usize, column: usize) {
        let point = self.visible_point(row, column);
        self.term.selection = Some(Selection::new(SelectionType::Simple, point, Side::Left));
    }

    /// 드래그 선택의 움직이는 끝을 보이는 칸으로 옮긴다. 시작 칸과 끝 칸을 모두 포함한다
    pub fn extend_selection(&mut self, row: usize, column: usize) {
        let point = self.visible_point(row, column);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, Side::Left);
            selection.include_all();
        }
    }

    /// 보이는 두 칸 사이를 한 번에 고른다(두 번 누른 단어, 세 번 누른 줄)
    pub fn select_visible_range(&mut self, start: (usize, usize), end: (usize, usize)) {
        self.start_selection(start.0, start.1);
        self.extend_selection(end.0, end.1);
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// 지금 보기 위치에서 선택이 덮는 칸. 선택이 없거나 비었거나 보기 밖에 있으면 `None`
    pub fn visible_selection(&self) -> Option<VisibleSelection> {
        let range = self.term.selection.as_ref()?.to_range(&self.term)?;
        let grid = self.term.grid();
        let display_offset = grid.display_offset() as i32;
        let last_row = grid.screen_lines() as i32 - 1;
        let start_row = range.start.line.0 + display_offset;
        let end_row = range.end.line.0 + display_offset;
        if end_row < 0 || start_row > last_row {
            return None;
        }
        let (start_row, start_column) = match start_row < 0 {
            true => (0, 0),
            false => (start_row as usize, range.start.column.0),
        };
        let (end_row, end_column) = match end_row > last_row {
            true => (last_row as usize, grid.columns() - 1),
            false => (end_row as usize, range.end.column.0),
        };
        Some(VisibleSelection {
            start_row,
            start_column,
            end_row,
            end_column,
        })
    }

    /// 고른 글 전체(보기 밖으로 밀린 부분 포함). 자동 줄바꿈으로 이어진 줄은 개행 없이 잇는다
    pub fn selected_text(&self) -> Option<String> {
        self.term
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    fn visible_point(&self, row: usize, column: usize) -> Point {
        let grid = self.term.grid();
        let line = Line(row as i32 - grid.display_offset() as i32);
        Point::new(line, Column(column.min(grid.columns() - 1)))
    }
}
