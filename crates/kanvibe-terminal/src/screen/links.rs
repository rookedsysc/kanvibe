//! 보이는 칸에 걸린 링크. 프로그램이 OSC 8로 건 링크를 먼저 보고, 없으면 글자 속 http/https 주소를 찾는다
//! (TS `WebLinksAddon`의 주소 규칙). 자동 줄바꿈으로 여러 줄에 걸친 주소도 한 줄로 이어 찾는다.

use std::sync::LazyLock;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use regex::Regex;

use super::TerminalScreen;

/// `@xterm/addon-web-links`의 `strictUrlRegex`
static WEB_LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(https?|HTTPS?)://[^\s"'!*(){}|\\^<>`]*[^\s"':,.!?{}|\\^~\[\]`()<>]"#)
        .expect("고정된 주소 규칙")
});

impl TerminalScreen {
    /// 보이는 칸에 걸린 링크 주소. 링크가 아니면 `None`
    pub fn link_at(&self, row: usize, column: usize) -> Option<String> {
        let grid = self.term.grid();
        if row >= grid.screen_lines() || column >= grid.columns() {
            return None;
        }
        let line = Line(row as i32 - grid.display_offset() as i32);
        if let Some(hyperlink) = grid[line][Column(column)].hyperlink() {
            return Some(hyperlink.uri().to_string());
        }
        self.web_link_at(row, column)
    }

    fn web_link_at(&self, row: usize, column: usize) -> Option<String> {
        let (first_row, last_row) = self.wrapped_row_span(row);
        let mut text = String::new();
        let mut clicked_byte = None;
        for span_row in first_row..=last_row {
            for (cell_column, cell) in self.visible_cells(span_row)?.iter().enumerate() {
                if span_row == row && cell_column == column {
                    clicked_byte = Some(text.len());
                }
                match (cell.width, cell.text.is_empty()) {
                    (0, _) => {}
                    (_, true) => text.push(' '),
                    (_, false) => text.push_str(&cell.text),
                }
            }
        }
        let clicked_byte = clicked_byte?;
        WEB_LINK
            .find_iter(&text)
            .find(|found| found.range().contains(&clicked_byte))
            .map(|found| found.as_str().to_string())
    }

    /// 이 줄과 자동 줄바꿈으로 이어진 앞뒤 보이는 줄의 범위
    fn wrapped_row_span(&self, row: usize) -> (usize, usize) {
        let mut first_row = row;
        while first_row > 0 && self.is_visible_row_wrapped(first_row - 1) {
            first_row -= 1;
        }
        let mut last_row = row;
        while self.is_visible_row_wrapped(last_row) {
            last_row += 1;
        }
        (first_row, last_row)
    }
}
