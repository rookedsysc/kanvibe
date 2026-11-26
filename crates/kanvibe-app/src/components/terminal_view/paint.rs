//! 레지스트리 화면을 그대로 그린다(xterm.js DOM 렌더러 자리). 셀 너비를 고정해 글자를 칸에 맞추고,
//! 같은 꾸밈이 이어진 칸은 한 번에 그린다. 넓은 글자(한글 등)는 두 칸 자리에 따로 그린다.
//! 화면은 출력 반영 잠금 안에서 `ScreenPaintFrame`으로 복사만 하고, 글자 배치와 그리기는 잠금 밖에서 한다.

use gpui_kit::{
    App, Bounds, Font, FontWeight, Hsla, Pixels, Point, Rgba, SharedString, Size,
    StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, Window, fill, point, px, size,
};
use kanvibe_terminal::screen::{
    CellColor, ScreenCell, ScreenCursor, ScreenCursorShape, TERMINAL_BACKGROUND,
    TERMINAL_FOREGROUND, TerminalScreen, default_color,
};

use std::ops::Range;

use super::mouse::{GridPoint, GridSelection};
use crate::fonts::mono_font;

/// TS `createTerminalOptions`의 `fontSize: 14`
pub const TERMINAL_FONT_SIZE: f32 = 14.0;
/// TS 테마 `selectionBackground: #3b82f680`
pub(crate) const SELECTION_BACKGROUND: u32 = 0x3b82f680;

/// 셀 크기와 그릴 자리. 그리기 전에 글꼴 측정으로 정한다
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLayout {
    pub origin: Point<Pixels>,
    pub cell_size: Size<Pixels>,
    pub columns: u16,
    pub rows: u16,
}

impl GridLayout {
    /// 창 좌표를 칸으로 바꾼다. 화면 밖은 가장자리 칸으로 붙인다
    pub fn grid_point(&self, position: Point<Pixels>) -> GridPoint {
        let column = f32::from(position.x - self.origin.x) / f32::from(self.cell_size.width);
        let row = f32::from(position.y - self.origin.y) / f32::from(self.cell_size.height);
        GridPoint {
            row: (row.max(0.0) as usize).min(usize::from(self.rows.max(1)) - 1),
            column: (column.max(0.0) as usize).min(usize::from(self.columns.max(1)) - 1),
        }
    }

    fn cell_origin(&self, row: usize, column: usize) -> Point<Pixels> {
        point(
            self.origin.x + self.cell_size.width * column as f32,
            self.origin.y + self.cell_size.height * row as f32,
        )
    }
}

/// 글꼴로 셀 크기를 잰다. xterm처럼 글자 높이를 올림한 값을 줄 높이로 쓴다
pub fn measure_cell_size(window: &Window) -> Size<Pixels> {
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&mono_font(400));
    let font_size = px(TERMINAL_FONT_SIZE);
    let width = text_system
        .advance(font_id, font_size, 'm')
        .map(|advance| advance.width)
        .unwrap_or(px(8.0));
    // GPUI는 descent를 기준선 아래 음수로 준다
    let ascent = f32::from(text_system.ascent(font_id, font_size));
    let descent = f32::from(text_system.descent(font_id, font_size)).abs();
    size(width, px((ascent + descent).ceil()))
}

/// 화면을 그릴 때 함께 보는 뷰 상태
pub struct GridPaintState<'a> {
    pub layout: GridLayout,
    pub selection: Option<GridSelection>,
    pub is_focused: bool,
    /// 깜박임에서 커서를 그리는 차례인지. 포커스가 없으면 깜박이지 않는다
    pub is_cursor_blink_on: bool,
    /// IME 조합 중인 글자. 커서 자리에 밑줄로 그린다
    pub marked_text: Option<&'a str>,
}

/// 그릴 줄의 셀과 커서를 화면에서 복사해 둔 것. 뷰가 프레임마다 같은 버퍼를 다시 써서 칸마다 할당하지 않는다
#[derive(Default)]
pub struct ScreenPaintFrame {
    rows: Vec<Vec<ScreenCell>>,
    row_count: usize,
    cursor: Option<ScreenCursor>,
}

impl ScreenPaintFrame {
    /// 출력 반영 잠금 안에서 부른다. 보이는 줄 중 `visible_rows`까지만 셀 값을 복사한다
    pub fn copy_from(&mut self, screen: &TerminalScreen, visible_rows: usize) {
        self.row_count = visible_rows.min(screen.rows());
        if self.rows.len() < self.row_count {
            self.rows.resize_with(self.row_count, Vec::new);
        }
        for (row, cells) in self.rows.iter_mut().take(self.row_count).enumerate() {
            screen.copy_visible_cells_into(row, cells);
        }
        self.cursor = Some(screen.cursor());
    }

    fn visible_rows(&self) -> &[Vec<ScreenCell>] {
        &self.rows[..self.row_count]
    }
}

pub fn paint_screen(
    frame: &ScreenPaintFrame,
    state: &GridPaintState<'_>,
    window: &mut Window,
    cx: &mut App,
) {
    let fonts = TerminalFonts::new();
    for (row, cells) in frame.visible_rows().iter().enumerate() {
        paint_row_backgrounds(cells, row, state, window);
        paint_row_text(cells, row, state, &fonts, window, cx);
    }
    let Some(cursor) = frame.cursor else {
        return;
    };
    let cursor_cell = frame
        .visible_rows()
        .get(cursor.row)
        .and_then(|cells| cells.get(cursor.column));
    paint_cursor(cursor, cursor_cell, state, &fonts, window, cx);
}

/// 셀의 실제 글자색과 배경색. 반전·dim·숨김과 xterm `drawBoldTextInBrightColors`를 반영한다
pub fn resolve_cell_colors(cell: &ScreenCell) -> (Rgba, Option<Rgba>) {
    let foreground_color = match (cell.fg, cell.is_bold) {
        (CellColor::Palette(index @ 0..=7), true) => CellColor::Palette(index + 8),
        (color, _) => color,
    };
    let mut foreground = resolve_color(foreground_color, TERMINAL_FOREGROUND);
    let mut background = match cell.bg {
        CellColor::Default => None,
        color => Some(resolve_color(color, TERMINAL_BACKGROUND)),
    };
    if cell.is_inverse {
        let swapped_background = foreground;
        foreground = background.unwrap_or_else(|| rgb(TERMINAL_BACKGROUND));
        background = Some(swapped_background);
    }
    if cell.is_dim {
        foreground.a *= 0.5;
    }
    if cell.is_hidden {
        foreground.a = 0.0;
    }
    (foreground, background)
}

fn resolve_color(color: CellColor, default: u32) -> Rgba {
    match color {
        CellColor::Default => rgb(default),
        CellColor::Palette(index) => {
            let color = default_color(usize::from(index));
            rgb((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b))
        }
        CellColor::Rgb(red, green, blue) => {
            rgb((u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue))
        }
    }
}

fn rgb(value: u32) -> Rgba {
    gpui_kit::rgb(value)
}

/// 같은 값이 이어지는 칸들을 `(칸 범위, 값)` 하나로 묶는다. 값이 없는 칸은 건너뛴다
fn merge_column_runs<T: PartialEq + Copy>(
    values: impl IntoIterator<Item = Option<T>>,
) -> Vec<(Range<usize>, T)> {
    let mut runs: Vec<(Range<usize>, T)> = Vec::new();
    for (column, value) in values.into_iter().enumerate() {
        let Some(value) = value else {
            continue;
        };
        match runs.last_mut() {
            Some((columns, run_value)) if columns.end == column && *run_value == value => {
                columns.end = column + 1;
            }
            _ => runs.push((column..column + 1, value)),
        }
    }
    runs
}

fn paint_row_backgrounds(
    cells: &[ScreenCell],
    row: usize,
    state: &GridPaintState<'_>,
    window: &mut Window,
) {
    let backgrounds = merge_column_runs(cells.iter().map(|cell| resolve_cell_colors(cell).1));
    for (columns, background) in backgrounds {
        window.paint_quad(fill(column_run_bounds(row, &columns, state), background));
    }
    let Some(selection) = state.selection else {
        return;
    };
    let selected_columns = merge_column_runs(
        (0..cells.len()).map(|column| selection.contains(GridPoint { row, column }).then_some(())),
    );
    for (columns, ()) in selected_columns {
        window.paint_quad(fill(
            column_run_bounds(row, &columns, state),
            gpui_kit::rgba(SELECTION_BACKGROUND),
        ));
    }
}

fn column_run_bounds(
    row: usize,
    columns: &Range<usize>,
    state: &GridPaintState<'_>,
) -> Bounds<Pixels> {
    let cell_size = state.layout.cell_size;
    let origin = state.layout.cell_origin(row, columns.start);
    let width = cell_size.width * columns.len() as f32;
    Bounds::new(origin, size(width, cell_size.height))
}

/// 같은 꾸밈이 이어진 한 칸짜리 글자 묶음
struct TextBatch {
    start_column: usize,
    text: String,
    run: TextRun,
}

/// 굵게·기울임 조합 네 가지 글꼴. 프레임마다 한 번 만들고 칸에서는 복제(참조 카운트)만 한다
struct TerminalFonts {
    regular: Font,
    bold: Font,
    italic: Font,
    bold_italic: Font,
}

impl TerminalFonts {
    fn new() -> Self {
        Self {
            regular: styled_mono_font(false, false),
            bold: styled_mono_font(true, false),
            italic: styled_mono_font(false, true),
            bold_italic: styled_mono_font(true, true),
        }
    }

    fn for_cell(&self, cell: &ScreenCell) -> &Font {
        match (cell.is_bold, cell.is_italic) {
            (false, false) => &self.regular,
            (true, false) => &self.bold,
            (false, true) => &self.italic,
            (true, true) => &self.bold_italic,
        }
    }
}

fn styled_mono_font(is_bold: bool, is_italic: bool) -> Font {
    let mut font = mono_font(if is_bold { 700 } else { 400 });
    if is_italic {
        font.style = gpui_kit::FontStyle::Italic;
    }
    if is_bold {
        font.weight = FontWeight::BOLD;
    }
    font
}

fn cell_run(cell: &ScreenCell, length: usize, fonts: &TerminalFonts) -> TextRun {
    let (foreground, _) = resolve_cell_colors(cell);
    let color: Hsla = foreground.into();
    TextRun {
        len: length,
        font: fonts.for_cell(cell).clone(),
        color,
        background_color: None,
        underline: cell.is_underline.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: cell.is_strikethrough.then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        }),
    }
}

fn is_same_style(left: &TextRun, right: &TextRun) -> bool {
    left.font == right.font
        && left.color == right.color
        && left.underline == right.underline
        && left.strikethrough == right.strikethrough
}

fn paint_row_text(
    cells: &[ScreenCell],
    row: usize,
    state: &GridPaintState<'_>,
    fonts: &TerminalFonts,
    window: &mut Window,
    cx: &mut App,
) {
    let mut batch: Option<TextBatch> = None;
    for (column, cell) in cells.iter().enumerate() {
        if cell.width == 0 {
            continue;
        }
        let text = if cell.text.is_empty() {
            " "
        } else {
            cell.text.as_str()
        };
        if cell.width == 2 {
            flush_batch(batch.take(), row, state, window, cx);
            let wide = TextBatch {
                start_column: column,
                text: text.to_string(),
                run: cell_run(cell, text.len(), fonts),
            };
            paint_batch(wide, row, state, None, window, cx);
            continue;
        }
        let run = cell_run(cell, text.len(), fonts);
        match batch.as_mut() {
            Some(current) if is_same_style(&current.run, &run) => {
                current.text.push_str(text);
                current.run.len += text.len();
            }
            _ => {
                flush_batch(batch.take(), row, state, window, cx);
                batch = Some(TextBatch {
                    start_column: column,
                    text: text.to_string(),
                    run,
                });
            }
        }
    }
    flush_batch(batch, row, state, window, cx);
}

fn flush_batch(
    batch: Option<TextBatch>,
    row: usize,
    state: &GridPaintState<'_>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(batch) = batch.filter(|batch| !batch.text.trim().is_empty()) else {
        return;
    };
    let cell_width = state.layout.cell_size.width;
    paint_batch(batch, row, state, Some(cell_width), window, cx);
}

fn paint_batch(
    batch: TextBatch,
    row: usize,
    state: &GridPaintState<'_>,
    force_width: Option<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let origin = state.layout.cell_origin(row, batch.start_column);
    let line = window.text_system().shape_line(
        SharedString::from(batch.text),
        px(TERMINAL_FONT_SIZE),
        &[batch.run],
        force_width,
    );
    let _ = line.paint(
        origin,
        state.layout.cell_size.height,
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

/// 커서. 포커스가 있으면 채운 블록 위에 글자를 배경색으로 다시 그리고, 없으면 테두리만 그린다(xterm.js)
fn paint_cursor(
    cursor: ScreenCursor,
    cursor_cell: Option<&ScreenCell>,
    state: &GridPaintState<'_>,
    fonts: &TerminalFonts,
    window: &mut Window,
    cx: &mut App,
) {
    if !cursor.is_visible || usize::from(state.layout.rows) <= cursor.row {
        return;
    }
    let origin = state.layout.cell_origin(cursor.row, cursor.column);
    let cell_size = state.layout.cell_size;
    if let Some(marked_text) = state.marked_text.filter(|text| !text.is_empty()) {
        paint_marked_text(marked_text, origin, state, window, cx);
        return;
    }
    if state.is_focused && !state.is_cursor_blink_on {
        return;
    }
    let color = rgb(TERMINAL_FOREGROUND);
    let is_wide_cell = cursor_cell.is_some_and(|cell| cell.width == 2);
    let block_width = match is_wide_cell {
        true => cell_size.width * 2.0,
        false => cell_size.width,
    };
    let bounds = match cursor.shape {
        ScreenCursorShape::Bar => Bounds::new(origin, size(px(1.0), cell_size.height)),
        ScreenCursorShape::Underline => Bounds::new(
            point(origin.x, origin.y + cell_size.height - px(1.0)),
            size(cell_size.width, px(1.0)),
        ),
        ScreenCursorShape::Block => Bounds::new(origin, size(block_width, cell_size.height)),
    };
    match (cursor.shape, state.is_focused) {
        (ScreenCursorShape::Block, false) => {
            window.paint_quad(gpui_kit::outline(
                bounds,
                color,
                gpui_kit::BorderStyle::Solid,
            ));
        }
        (ScreenCursorShape::Block, true) => {
            window.paint_quad(fill(bounds, color));
            let glyph = cursor_cell.and_then(|cell| cursor_glyph_batch(cell, cursor.column, fonts));
            if let Some(glyph) = glyph {
                let force_width = (!is_wide_cell).then_some(cell_size.width);
                paint_batch(glyph, cursor.row, state, force_width, window, cx);
            }
        }
        _ => window.paint_quad(fill(bounds, color)),
    }
}

/// 채운 블록 커서 위에 다시 그릴 커서 칸 글자. 블록에 가려지지 않게 배경색으로 그린다. 빈 칸·숨긴 글자면 `None`이다
fn cursor_glyph_batch(
    cell: &ScreenCell,
    column: usize,
    fonts: &TerminalFonts,
) -> Option<TextBatch> {
    if cell.text.trim().is_empty() || cell.is_hidden {
        return None;
    }
    let mut run = cell_run(cell, cell.text.len(), fonts);
    let background: Hsla = rgb(TERMINAL_BACKGROUND).into();
    run.color = background;
    if let Some(underline) = run.underline.as_mut() {
        underline.color = Some(background);
    }
    if let Some(strikethrough) = run.strikethrough.as_mut() {
        strikethrough.color = Some(background);
    }
    Some(TextBatch {
        start_column: column,
        text: cell.text.clone(),
        run,
    })
}

fn paint_marked_text(
    marked_text: &str,
    origin: Point<Pixels>,
    state: &GridPaintState<'_>,
    window: &mut Window,
    cx: &mut App,
) {
    let color: Hsla = rgb(TERMINAL_FOREGROUND).into();
    let run = TextRun {
        len: marked_text.len(),
        font: mono_font(400),
        color,
        background_color: Some(rgb(TERMINAL_BACKGROUND).into()),
        underline: Some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    };
    let line = window.text_system().shape_line(
        SharedString::from(marked_text.to_string()),
        px(TERMINAL_FONT_SIZE),
        &[run],
        None,
    );
    let _ = line.paint(
        origin,
        state.layout.cell_size.height,
        TextAlign::Left,
        None,
        window,
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(fg: CellColor, bg: CellColor) -> ScreenCell {
        ScreenCell {
            text: "x".into(),
            width: 1,
            fg,
            bg,
            is_bold: false,
            is_dim: false,
            is_italic: false,
            is_underline: false,
            is_inverse: false,
            is_strikethrough: false,
            is_hidden: false,
        }
    }

    #[test]
    fn bold_low_palette_colors_draw_in_bright_colors() {
        let mut bold_red = cell(CellColor::Palette(1), CellColor::Default);
        bold_red.is_bold = true;
        let (foreground, background) = resolve_cell_colors(&bold_red);
        assert_eq!(foreground, rgb(0xef2929));
        assert_eq!(background, None);
    }

    #[test]
    fn inverse_swaps_foreground_and_background_and_dim_halves_alpha() {
        let mut inverse = cell(CellColor::Default, CellColor::Default);
        inverse.is_inverse = true;
        inverse.is_dim = true;
        let (foreground, background) = resolve_cell_colors(&inverse);
        assert_eq!(background, Some(rgb(TERMINAL_FOREGROUND)));
        assert_eq!(foreground.a, 0.5);
        let rgb_cell = cell(CellColor::Rgb(1, 2, 3), CellColor::Palette(4));
        assert_eq!(resolve_cell_colors(&rgb_cell).0, rgb(0x010203));
    }

    #[test]
    fn the_focused_block_cursor_redraws_its_glyph_in_the_background_color() {
        let mut underlined = cell(CellColor::Palette(2), CellColor::Default);
        underlined.is_underline = true;
        let fonts = TerminalFonts::new();
        let glyph = cursor_glyph_batch(&underlined, 5, &fonts).expect("글자가 있는 칸");
        let background: Hsla = rgb(TERMINAL_BACKGROUND).into();
        assert_eq!((glyph.start_column, glyph.text.as_str()), (5, "x"));
        assert_eq!(glyph.run.color, background);
        assert_eq!(
            glyph.run.underline.and_then(|style| style.color),
            Some(background)
        );

        let mut blank = cell(CellColor::Default, CellColor::Default);
        blank.text = String::new();
        assert!(cursor_glyph_batch(&blank, 0, &fonts).is_none());
    }

    #[test]
    fn neighbouring_cells_with_the_same_background_share_one_quad() {
        let red = Some(rgb(0xff0000));
        let blue = Some(rgb(0x0000ff));
        let runs = merge_column_runs([red, red, None, blue, blue, blue, red]);
        assert_eq!(
            runs,
            [
                (0..2, rgb(0xff0000)),
                (3..6, rgb(0x0000ff)),
                (6..7, rgb(0xff0000)),
            ]
        );
        assert!(merge_column_runs::<Rgba>([None, None]).is_empty());
    }

    #[test]
    fn each_style_combination_keeps_its_weight_and_slant() {
        let fonts = TerminalFonts::new();
        let mut cell = cell(CellColor::Default, CellColor::Default);
        cell.is_bold = true;
        cell.is_italic = true;
        let font = fonts.for_cell(&cell);
        assert_eq!(font.weight, FontWeight::BOLD);
        assert_eq!(font.style, gpui_kit::FontStyle::Italic);
        assert_eq!(fonts.regular.weight, FontWeight::NORMAL);
    }

    #[test]
    fn pointer_positions_clamp_to_the_grid() {
        let layout = GridLayout {
            origin: point(px(10.0), px(20.0)),
            cell_size: size(px(8.0), px(17.0)),
            columns: 80,
            rows: 24,
        };
        assert_eq!(
            layout.grid_point(point(px(27.0), px(40.0))),
            GridPoint { row: 1, column: 2 }
        );
        assert_eq!(
            layout.grid_point(point(px(0.0), px(9999.0))),
            GridPoint { row: 23, column: 0 }
        );
    }
}
