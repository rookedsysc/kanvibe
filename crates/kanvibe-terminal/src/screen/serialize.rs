//! xterm `SerializeAddon`의 문자열 직렬화를 alacritty 화면 위에서 그대로 옮겼다.
//! 스크롤백까지 모든 줄을 차례로 훑으며 속성이 바뀐 자리에서만 SGR을 내고, 빈 칸은 커서 이동(`C`)이나 지우기(`X`)로 줄인다.
//! 이어진 줄(자동 줄바꿈)은 줄바꿈 없이 붙여 받는 쪽이 같은 자리에서 줄을 넘기게 하고, 끝에서 커서를 실제 위치로 옮긴다.

use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};

use super::cells::{CellColor, CellStyle, cell_chars, cell_width, is_blank_cell};
use super::grid_cell;

/// 스크롤백과 화면 전체를 직렬화한다. `template`은 다음 입력에 쓰일 현재 속성(xterm `_curAttrData`)이다
pub(super) fn serialize_grid(grid: &Grid<Cell>, template: &Cell) -> String {
    let mut serializer = StringSerializer::new(grid);
    for row in 0..grid.total_lines() {
        serializer.serialize_row(row);
    }
    serializer.finish(template)
}

/// xterm `_serializeModes`. alacritty에 없는 모드(역방향 줄바꿈, X10 마우스)는 켜질 수 없어 내지 않는다
pub(super) fn serialize_modes(mode: TermMode) -> String {
    let flagged_sequences = [
        (TermMode::APP_CURSOR, "\u{1b}[?1h"),
        (TermMode::APP_KEYPAD, "\u{1b}[?66h"),
        (TermMode::BRACKETED_PASTE, "\u{1b}[?2004h"),
        (TermMode::INSERT, "\u{1b}[4h"),
        (TermMode::ORIGIN, "\u{1b}[?6h"),
        (TermMode::FOCUS_IN_OUT, "\u{1b}[?1004h"),
    ];
    let mut content: String = flagged_sequences
        .iter()
        .filter(|(flag, _)| mode.contains(*flag))
        .map(|(_, sequence)| *sequence)
        .collect();
    if !mode.contains(TermMode::LINE_WRAP) {
        content.push_str("\u{1b}[?7l");
    }
    content.push_str(mouse_tracking_sequence(mode));
    content
}

fn mouse_tracking_sequence(mode: TermMode) -> &'static str {
    if mode.contains(TermMode::MOUSE_MOTION) {
        return "\u{1b}[?1003h";
    }
    if mode.contains(TermMode::MOUSE_DRAG) {
        return "\u{1b}[?1002h";
    }
    match mode.contains(TermMode::MOUSE_REPORT_CLICK) {
        true => "\u{1b}[?1000h",
        false => "",
    }
}

/// xterm `_diffStyle`. 바뀐 속성만 SGR 인자로 낸다
fn diff_style(style: &CellStyle, old: &CellStyle) -> Vec<u32> {
    let is_fg_changed = style.fg != old.fg;
    let is_bg_changed = style.bg != old.bg;
    let are_flags_changed = flag_values(style) != flag_values(old);
    if !(is_fg_changed || is_bg_changed || are_flags_changed) {
        return Vec::new();
    }
    if style.is_default() {
        return match old.is_default() {
            true => Vec::new(),
            false => vec![0],
        };
    }
    let mut sequence = Vec::new();
    if is_fg_changed {
        push_color(&mut sequence, style.fg, ColorLayer::Foreground);
    }
    if is_bg_changed {
        push_color(&mut sequence, style.bg, ColorLayer::Background);
    }
    if are_flags_changed {
        push_flag_changes(&mut sequence, style, old);
    }
    sequence
}

fn flag_values(style: &CellStyle) -> [bool; 7] {
    [
        style.is_inverse,
        style.is_bold,
        style.is_underline,
        style.is_invisible,
        style.is_italic,
        style.is_dim,
        style.is_strikethrough,
    ]
}

#[derive(Clone, Copy)]
enum ColorLayer {
    Foreground,
    Background,
}

fn push_color(sequence: &mut Vec<u32>, color: CellColor, layer: ColorLayer) {
    let (extended, normal, bright, default) = match layer {
        ColorLayer::Foreground => (38, 30, 90, 39),
        ColorLayer::Background => (48, 40, 100, 49),
    };
    match color {
        CellColor::Rgb(red, green, blue) => {
            sequence.extend([extended, 2, red.into(), green.into(), blue.into()])
        }
        CellColor::Palette(index) if index >= 16 => sequence.extend([extended, 5, index.into()]),
        CellColor::Palette(index) if index & 8 != 0 => sequence.push(bright + u32::from(index & 7)),
        CellColor::Palette(index) => sequence.push(normal + u32::from(index & 7)),
        CellColor::Default => sequence.push(default),
    }
}

/// xterm과 같은 순서로 켜고 끈다. 굵게와 흐리게는 끄는 코드(22)가 같다
fn push_flag_changes(sequence: &mut Vec<u32>, style: &CellStyle, old: &CellStyle) {
    let toggles = [
        (style.is_inverse, old.is_inverse, 7, 27),
        (style.is_bold, old.is_bold, 1, 22),
        (style.is_underline, old.is_underline, 4, 24),
        (style.is_invisible, old.is_invisible, 8, 28),
        (style.is_italic, old.is_italic, 3, 23),
        (style.is_dim, old.is_dim, 2, 22),
        (style.is_strikethrough, old.is_strikethrough, 9, 29),
    ];
    for (is_on, was_on, on_code, off_code) in toggles {
        if is_on != was_on {
            sequence.push(if is_on { on_code } else { off_code });
        }
    }
}

fn sgr(sequence: &[u32]) -> String {
    let parameters: Vec<String> = sequence.iter().map(u32::to_string).collect();
    format!("\u{1b}[{}m", parameters.join(";"))
}

struct StringSerializer<'grid> {
    grid: &'grid Grid<Cell>,
    columns: usize,
    screen_rows: usize,
    rows: Vec<String>,
    row_separators: Vec<String>,
    current_row: String,
    null_cell_count: usize,
    cursor_style: CellStyle,
    background_style: CellStyle,
    last_cursor: (usize, usize),
    last_content_cursor: (usize, usize),
}

impl<'grid> StringSerializer<'grid> {
    fn new(grid: &'grid Grid<Cell>) -> Self {
        Self {
            grid,
            columns: grid.columns(),
            screen_rows: grid.screen_lines(),
            rows: Vec::new(),
            row_separators: Vec::new(),
            current_row: String::new(),
            null_cell_count: 0,
            cursor_style: CellStyle::DEFAULT,
            background_style: CellStyle::DEFAULT,
            last_cursor: (0, 0),
            last_content_cursor: (0, 0),
        }
    }

    fn serialize_row(&mut self, row: usize) {
        for column in 0..self.columns {
            self.next_cell(grid_cell(self.grid, row, column), row, column);
        }
        self.row_end(row, row + 1 == self.grid.total_lines());
    }

    /// xterm `_nextCell`
    fn next_cell(&mut self, cell: &Cell, row: usize, column: usize) {
        let width = cell_width(cell);
        if width == 0 {
            return;
        }
        let is_empty_cell = is_blank_cell(cell);
        let style = CellStyle::of(cell);
        let sequence = diff_style(&style, &self.cursor_style);
        let is_style_changed = match is_empty_cell {
            true => self.cursor_style.bg != style.bg,
            false => !sequence.is_empty(),
        };
        if is_style_changed {
            self.flush_null_cells();
            self.last_cursor = (row, column);
            self.last_content_cursor = (row, column);
            self.current_row.push_str(&sgr(&sequence));
            self.cursor_style = style;
        }
        if is_empty_cell {
            self.null_cell_count += width;
            return;
        }
        self.flush_null_cells();
        self.current_row.push_str(&cell_chars(cell));
        self.last_cursor = (row, column + width);
        self.last_content_cursor = (row, column + width);
    }

    /// 모아 둔 빈 칸을 커서 이동으로 내보낸다. 배경이 기준과 다르면 그 칸을 현재 배경으로 먼저 지운다
    fn flush_null_cells(&mut self) {
        if self.null_cell_count == 0 {
            return;
        }
        if self.cursor_style.bg != self.background_style.bg {
            self.current_row
                .push_str(&format!("\u{1b}[{}X", self.null_cell_count));
        }
        self.current_row
            .push_str(&format!("\u{1b}[{}C", self.null_cell_count));
        self.null_cell_count = 0;
    }

    /// xterm `_rowEnd`
    fn row_end(&mut self, row: usize, is_last_row: bool) {
        if self.null_cell_count > 0 && self.cursor_style.bg != self.background_style.bg {
            self.current_row
                .push_str(&format!("\u{1b}[{}X", self.null_cell_count));
        }
        let mut separator = String::new();
        if !is_last_row {
            if row >= self.screen_rows {
                self.background_style = self.cursor_style;
            }
            separator = self.row_separator(row);
        }
        self.rows.push(std::mem::take(&mut self.current_row));
        self.row_separators.push(separator);
        self.null_cell_count = 0;
    }

    fn row_separator(&mut self, row: usize) -> String {
        if !is_wrapped_into_next(self.grid, row, self.columns) {
            self.last_cursor = (row + 1, 0);
            return "\r\n".to_string();
        }
        if self.can_continue_wrapped_row(row) {
            return String::new();
        }
        self.last_content_cursor = (row + 1, 0);
        self.last_cursor = (row + 1, 0);
        self.forced_wrap_separator()
    }

    /// 다음 줄이 이 줄에서 넘어온 줄이면, 받는 쪽도 같은 자리에서 저절로 넘어가는지 확인한다
    fn can_continue_wrapped_row(&self, row: usize) -> bool {
        let last = grid_cell(self.grid, row, self.columns - 1);
        let second_last = grid_cell(self.grid, row, self.columns.saturating_sub(2));
        let next_first = grid_cell(self.grid, row + 1, 0);
        let is_next_first_wide = cell_width(next_first) > 1;
        // JS `a && b ? x : y`는 `(a && b) ? x : y`로 묶인다
        let allowed_null_cells = match !cell_chars(next_first).is_empty() && is_next_first_wide {
            true => 1,
            false => 0,
        };
        if self.null_cell_count > allowed_null_cells {
            return false;
        }
        let same_bg = |cell: &Cell| CellStyle::of(cell).bg == CellStyle::of(next_first).bg;
        let is_filled = |cell: &Cell| !cell_chars(cell).is_empty() || cell_width(cell) == 0;
        if is_filled(last) && same_bg(last) {
            return true;
        }
        is_next_first_wide && is_filled(second_last) && same_bg(last) && same_bg(second_last)
    }

    /// 줄 끝을 채운 뒤 지워 받는 쪽이 강제로 줄을 넘기게 한다(xterm과 같은 바이트)
    fn forced_wrap_separator(&self) -> String {
        let mut separator = "-".repeat(self.null_cell_count + 1);
        separator.push_str("\u{1b}[1D\u{1b}[1X");
        if self.null_cell_count > 0 {
            let content_columns = self.columns - self.null_cell_count;
            separator.push_str("\u{1b}[A");
            separator.push_str(&format!("\u{1b}[{content_columns}C"));
            separator.push_str(&format!("\u{1b}[{}X", self.null_cell_count));
            separator.push_str(&format!("\u{1b}[{content_columns}D"));
            separator.push_str("\u{1b}[B");
        }
        separator
    }

    /// xterm `_serializeString`
    fn finish(mut self, template: &Cell) -> String {
        let mut row_end = self.rows.len();
        if self.grid.total_lines() <= self.screen_rows {
            row_end = self.last_content_cursor.0 + 1;
            self.last_cursor = self.last_content_cursor;
        }
        let mut content = String::new();
        for index in 0..row_end.min(self.rows.len()) {
            content.push_str(&self.rows[index]);
            if index + 1 < row_end {
                content.push_str(&self.row_separators[index]);
            }
        }
        content.push_str(&self.cursor_movement());
        let pending_style = diff_style(&CellStyle::of(template), &self.cursor_style);
        if !pending_style.is_empty() {
            content.push_str(&sgr(&pending_style));
        }
        content
    }

    fn cursor_movement(&self) -> String {
        let history = self.grid.total_lines() - self.screen_rows;
        let cursor = &self.grid.cursor;
        let real_row = history as i64 + i64::from(cursor.point.line.0);
        let real_column = cursor.point.column.0 as i64 + i64::from(cursor.input_needs_wrap);
        let row_offset = real_row - self.last_cursor.0 as i64;
        let column_offset = real_column - self.last_cursor.1 as i64;
        let mut movement = String::new();
        if row_offset > 0 {
            movement.push_str(&format!("\u{1b}[{row_offset}B"));
        } else if row_offset < 0 {
            movement.push_str(&format!("\u{1b}[{}A", -row_offset));
        }
        if column_offset > 0 {
            movement.push_str(&format!("\u{1b}[{column_offset}C"));
        } else if column_offset < 0 {
            movement.push_str(&format!("\u{1b}[{}D", -column_offset));
        }
        movement
    }
}

/// xterm의 다음 줄 `isWrapped`. alacritty는 넘어간 줄의 마지막 칸에 표시를 남긴다
fn is_wrapped_into_next(grid: &Grid<Cell>, row: usize, columns: usize) -> bool {
    grid_cell(grid, row, columns - 1)
        .flags
        .contains(Flags::WRAPLINE)
}
