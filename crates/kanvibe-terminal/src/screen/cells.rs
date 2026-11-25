//! alacritty 셀을 xterm 버퍼 셀처럼 읽는 규칙. 직렬화와 pane chat 판정이 같은 규칙을 쓴다.
//!
//! xterm은 한 번도 쓰지 않은 칸(빈 문자열)과 공백을 쓴 칸을 구분하지만 alacritty는 둘 다 공백이다.
//! 그래서 공백 칸은 보이는 꾸밈(반전·밑줄·취소선)이 없으면 빈 칸으로 본다.

use alacritty_terminal::grid::Row;
use alacritty_terminal::index::Column;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::vte::ansi::{Color, NamedColor};
use kanvibe_ai::pane_chat::codex_display::TerminalCellRow;

/// 공백 위에서도 눈에 보이는 꾸밈. 이것이 있는 공백은 내용이 있는 칸이다
const VISIBLE_ON_BLANK: Flags = Flags::INVERSE
    .union(Flags::ALL_UNDERLINES)
    .union(Flags::STRIKEOUT);

/// xterm 색 모드와 값. 팔레트 0~15를 `31`로 썼든 `38;5;1`로 썼든 직렬화 결과가 같아 하나로 본다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CellColor {
    Default,
    Palette(u8),
    Rgb(u8, u8, u8),
}

impl CellColor {
    pub(super) fn from_alacritty(color: Color) -> Self {
        match color {
            Color::Spec(rgb) => Self::Rgb(rgb.r, rgb.g, rgb.b),
            Color::Indexed(index) => Self::Palette(index),
            Color::Named(named) => named_palette_index(named).map_or(Self::Default, Self::Palette),
        }
    }
}

fn named_palette_index(named: NamedColor) -> Option<u8> {
    let index = named as usize;
    match named {
        _ if index < 16 => Some(index as u8),
        NamedColor::DimBlack
        | NamedColor::DimRed
        | NamedColor::DimGreen
        | NamedColor::DimYellow
        | NamedColor::DimBlue
        | NamedColor::DimMagenta
        | NamedColor::DimCyan
        | NamedColor::DimWhite => Some((index - NamedColor::DimBlack as usize) as u8),
        _ => None,
    }
}

/// xterm 셀 속성 가운데 직렬화가 비교하는 부분
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CellStyle {
    pub fg: CellColor,
    pub bg: CellColor,
    pub is_inverse: bool,
    pub is_bold: bool,
    pub is_underline: bool,
    pub is_invisible: bool,
    pub is_italic: bool,
    pub is_dim: bool,
    pub is_strikethrough: bool,
}

impl CellStyle {
    pub(super) const DEFAULT: Self = Self {
        fg: CellColor::Default,
        bg: CellColor::Default,
        is_inverse: false,
        is_bold: false,
        is_underline: false,
        is_invisible: false,
        is_italic: false,
        is_dim: false,
        is_strikethrough: false,
    };

    pub(super) fn of(cell: &Cell) -> Self {
        let flags = cell.flags;
        Self {
            fg: CellColor::from_alacritty(cell.fg),
            bg: CellColor::from_alacritty(cell.bg),
            is_inverse: flags.contains(Flags::INVERSE),
            is_bold: flags.contains(Flags::BOLD),
            is_underline: flags.intersects(Flags::ALL_UNDERLINES),
            is_invisible: flags.contains(Flags::HIDDEN),
            is_italic: flags.contains(Flags::ITALIC),
            is_dim: flags.contains(Flags::DIM),
            is_strikethrough: flags.contains(Flags::STRIKEOUT),
        }
    }

    pub(super) fn is_default(&self) -> bool {
        *self == Self::DEFAULT
    }
}

/// xterm `getWidth()`. 넓은 글자의 뒤 칸은 0이다
pub(super) fn cell_width(cell: &Cell) -> usize {
    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
        return 0;
    }
    match cell.flags.contains(Flags::WIDE_CHAR) {
        true => 2,
        false => 1,
    }
}

/// 내용 없는 칸인지. 탭으로 건너뛴 칸과 줄 끝에서 넓은 글자를 다음 줄로 넘긴 자리도 빈 칸이다
pub(super) fn is_blank_cell(cell: &Cell) -> bool {
    if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
        return true;
    }
    matches!(cell.c, ' ' | '\t')
        && cell.zerowidth().is_none()
        && !cell.flags.intersects(VISIBLE_ON_BLANK)
}

/// xterm `getChars()`. 빈 칸과 넓은 글자의 뒤 칸은 빈 문자열이다
pub(super) fn cell_chars(cell: &Cell) -> String {
    if cell_width(cell) == 0 || is_blank_cell(cell) {
        return String::new();
    }
    let mut chars = String::from(cell.c);
    chars.extend(cell.zerowidth().unwrap_or_default());
    chars
}

/// 화면의 한 줄을 xterm `IBufferLine`처럼 읽는다
pub struct ScreenRow<'grid> {
    row: &'grid Row<Cell>,
    /// 끝 공백을 떼도 남겨 둘 칸 수. 커서 줄은 커서 앞까지가 프로그램이 직접 쓴 칸이다
    written_columns: usize,
}

impl<'grid> ScreenRow<'grid> {
    pub(super) fn new(row: &'grid Row<Cell>, written_columns: usize) -> Self {
        Self {
            row,
            written_columns,
        }
    }

    fn cell(&self, column: usize) -> Option<&Cell> {
        (column < self.row.len()).then(|| &self.row[Column(column)])
    }

    /// pane chat 판정용 글자. 꾸밈이 전혀 없는 공백만 빈 칸으로 보고, dim 공백처럼 꾸밈이 있으면 쓴 칸으로 본다
    fn written_chars(cell: &Cell) -> String {
        let is_blank_character = matches!(cell.c, ' ' | '\t') && cell.zerowidth().is_none();
        if cell_width(cell) == 0 || (is_blank_character && CellStyle::of(cell).is_default()) {
            return String::new();
        }
        match is_blank_character {
            true => " ".to_string(),
            false => cell_chars(cell),
        }
    }
}

impl TerminalCellRow for ScreenRow<'_> {
    fn column_count(&self) -> usize {
        self.row.len()
    }

    fn cell_chars(&self, column: usize) -> Option<String> {
        self.cell(column).map(Self::written_chars)
    }

    fn is_cell_dim(&self, column: usize) -> bool {
        self.cell(column)
            .is_some_and(|cell| cell.flags.contains(Flags::DIM))
    }

    /// xterm은 직접 쓴 끝 공백(입력란 표시 `› ` 등)을 남긴다. alacritty는 그 공백과 빈 칸을 가르지 못하므로
    /// 커서 줄에서는 커서 앞까지를 쓴 칸으로 보고, 그 뒤의 빈 칸만 뗀다
    fn trimmed_text(&self) -> String {
        let mut pieces: Vec<(usize, String)> = Vec::new();
        for column in 0..self.row.len() {
            let cell = &self.row[Column(column)];
            if cell_width(cell) == 0 {
                continue;
            }
            let piece = match is_blank_cell(cell) {
                true => " ".to_string(),
                false => cell_chars(cell),
            };
            pieces.push((column, piece));
        }
        let kept_count = pieces
            .iter()
            .rposition(|(column, piece)| piece != " " || *column < self.written_columns)
            .map_or(0, |last| last + 1);
        pieces
            .into_iter()
            .take(kept_count)
            .map(|(_, piece)| piece)
            .collect()
    }
}
