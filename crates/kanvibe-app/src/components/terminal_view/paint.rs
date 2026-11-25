//! 레지스트리 화면을 그대로 그린다(xterm.js DOM 렌더러 자리). 셀 너비를 고정해 글자를 칸에 맞추고,
//! 같은 꾸밈이 이어진 칸은 한 번에 그린다. 넓은 글자(한글 등)는 두 칸 자리에 따로 그린다.

use gpui_kit::{
    App, Bounds, Font, FontWeight, Hsla, Pixels, Point, Rgba, SharedString, Size,
    StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, Window, fill, point, px, size,
};
use kanvibe_terminal::screen::{
    CellColor, ScreenCell, ScreenCursor, ScreenCursorShape, TERMINAL_BACKGROUND,
    TERMINAL_FOREGROUND, TerminalScreen, default_color,
};

use super::mouse::{GridPoint, GridSelection};
use crate::fonts::mono_font;

/// TS `createTerminalOptions`의 `fontSize: 14`
pub const TERMINAL_FONT_SIZE: f32 = 14.0;
/// TS 테마 `selectionBackground: #3b82f680`
const SELECTION_BACKGROUND: u32 = 0x3b82f680;

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
    /// IME 조합 중인 글자. 커서 자리에 밑줄로 그린다
    pub marked_text: Option<&'a str>,
}

pub fn paint_screen(
    screen: &TerminalScreen,
    state: &GridPaintState<'_>,
    window: &mut Window,
    cx: &mut App,
) {
    let rows = usize::from(state.layout.rows).min(screen.rows());
    for row in 0..rows {
        let Some(cells) = screen.visible_cells(row) else {
            continue;
        };
        paint_row_backgrounds(&cells, row, state, window);
        paint_row_text(&cells, row, state, window, cx);
    }
    paint_cursor(screen.cursor(), state, window, cx);
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

fn paint_row_backgrounds(
    cells: &[ScreenCell],
    row: usize,
    state: &GridPaintState<'_>,
    window: &mut Window,
) {
    let cell_size = state.layout.cell_size;
    for (column, cell) in cells.iter().enumerate() {
        let is_selected = state
            .selection
            .is_some_and(|selection| selection.contains(GridPoint { row, column }));
        let (_, background) = resolve_cell_colors(cell);
        let origin = state.layout.cell_origin(row, column);
        let bounds = Bounds::new(origin, cell_size);
        if let Some(background) = background {
            window.paint_quad(fill(bounds, background));
        }
        if is_selected {
            window.paint_quad(fill(bounds, gpui_kit::rgba(SELECTION_BACKGROUND)));
        }
    }
}

/// 같은 꾸밈이 이어진 한 칸짜리 글자 묶음
struct TextBatch {
    start_column: usize,
    text: String,
    run: TextRun,
}

fn cell_font(cell: &ScreenCell) -> Font {
    let mut font = mono_font(if cell.is_bold { 700 } else { 400 });
    if cell.is_italic {
        font.style = gpui_kit::FontStyle::Italic;
    }
    if cell.is_bold {
        font.weight = FontWeight::BOLD;
    }
    font
}

fn cell_run(cell: &ScreenCell, length: usize) -> TextRun {
    let (foreground, _) = resolve_cell_colors(cell);
    let color: Hsla = foreground.into();
    TextRun {
        len: length,
        font: cell_font(cell),
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
                run: cell_run(cell, text.len()),
            };
            paint_batch(wide, row, state, None, window, cx);
            continue;
        }
        let run = cell_run(cell, text.len());
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
    state: &GridPaintState<'_>,
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
    let color = rgb(TERMINAL_FOREGROUND);
    let bounds = match cursor.shape {
        ScreenCursorShape::Bar => Bounds::new(origin, size(px(1.0), cell_size.height)),
        ScreenCursorShape::Underline => Bounds::new(
            point(origin.x, origin.y + cell_size.height - px(1.0)),
            size(cell_size.width, px(1.0)),
        ),
        ScreenCursorShape::Block => Bounds::new(origin, cell_size),
    };
    match (cursor.shape, state.is_focused) {
        (ScreenCursorShape::Block, false) => {
            window.paint_quad(gpui_kit::outline(
                bounds,
                color,
                gpui_kit::BorderStyle::Solid,
            ));
        }
        _ => window.paint_quad(fill(bounds, color)),
    }
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
