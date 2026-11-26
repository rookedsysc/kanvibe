//! Markdown 표(TS `kv-chat-table` 틀 안의 `table w-max min-w-full border-collapse`).
//!
//! 브라우저 자동 표 배치처럼 열마다 가장 긴 칸의 한 줄 폭을 재고, 표가 틀보다 좁으면 남는 폭을 그 비율로 나눠
//! 틀을 채운다. 넓으면 표 틀 안에서만 옆으로 넘긴다. 칸은 `border border-border-default px-3 py-1.5 align-top`,
//! 머리 칸(`th`)은 `bg-bg-page text-left` 굵은 글자다. 여러 열을 차지하는 칸은 그 열 폭을 합친 자리에 놓고,
//! 여러 행을 차지하는 칸은 첫 행에만 그린다.

use gpui_kit::{
    App, Div, InteractiveElement, IntoElement, ParentElement, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_markdown::{Alignment, Block, Inline, Table, TableCaption, TableCell};

use super::markdown_inline::{InlinePiece, InlineStyle, inline_pieces};
use super::markdown_view::{MarkdownContext, render_blocks};
use crate::theme::spacing;

/// 칸 좌우 안쪽 여백 합(`px-3`)과 테두리
const CELL_HORIZONTAL_EXTRA: f32 = 24.0 + 1.0;

#[derive(IntoElement)]
pub struct MarkdownTable {
    table: Table,
    context: MarkdownContext,
    path: String,
}

/// 칸 안 블록의 글자 줄들(폭 재기용)
fn cell_inlines(blocks: &[Block]) -> Vec<&[Inline]> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(inlines) | Block::Inlines(inlines) | Block::Preformatted(inlines) => {
                Some(inlines.as_slice())
            }
            Block::Heading { content, .. } => Some(content.as_slice()),
            _ => None,
        })
        .collect()
}

impl MarkdownTable {
    pub fn new(table: Table, context: MarkdownContext, path: &str) -> Self {
        Self {
            table,
            context,
            path: path.to_string(),
        }
    }

    /// 칸 하나의 가장 긴 줄 폭
    fn cell_width(&self, cell: &TableCell, window: &Window) -> f32 {
        let style = InlineStyle {
            css_weight: if cell.is_header { 600 } else { 400 },
            ..InlineStyle::plain(self.context.theme.text.primary)
        };
        let mut widest: f32 = 0.0;
        for inlines in cell_inlines(&cell.blocks) {
            for piece in inline_pieces(inlines, &style, &self.context.theme) {
                let InlinePiece::Text(spans) = piece else {
                    continue;
                };
                widest = widest.max(widest_line(&spans, window));
            }
        }
        widest.ceil() + CELL_HORIZONTAL_EXTRA
    }

    /// 열마다 그 열 하나만 차지하는 칸 중 가장 넓은 폭
    pub fn column_widths(&self, window: &Window) -> Vec<f32> {
        let columns = self
            .table
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .map(|cell| cell.column_span.max(1) as usize)
                    .sum()
            })
            .max()
            .unwrap_or(0);
        let mut widths = vec![CELL_HORIZONTAL_EXTRA; columns];
        for row in &self.table.rows {
            let mut column = 0;
            for cell in &row.cells {
                let span = cell.column_span.max(1) as usize;
                if span == 1 && column < columns {
                    widths[column] = widths[column].max(self.cell_width(cell, window));
                }
                column += span;
            }
        }
        widths
    }

    fn render_row(&self, cells: &[TableCell], widths: &[f32], row_index: usize) -> Div {
        let theme = &self.context.theme;
        let total: f32 = widths.iter().sum();
        let mut row = div().flex().min_w(px(total)).w_full();
        let mut column = 0;
        for (cell_index, cell) in cells.iter().enumerate() {
            let span = cell.column_span.max(1) as usize;
            let width: f32 = widths.iter().skip(column).take(span).sum();
            column += span;
            let element = div()
                .flex()
                .flex_col()
                .flex_basis(px(0.0))
                .flex_grow(width)
                .min_w(px(width))
                .border_1()
                .border_color(theme.border.default)
                .px(spacing(3.0))
                .py(spacing(1.5));
            let element = match (cell.is_header, cell.alignment) {
                (true, _) => element.bg(theme.background.page),
                (false, Alignment::Center) => element.items_center(),
                (false, Alignment::Right) => element.items_end(),
                _ => element,
            };
            // 이웃 칸과 테두리 1px을 나눠 쓴다(`border-collapse`)
            let element = match cell_index {
                0 => element,
                _ => element.ml(px(-1.0)),
            };
            let path = format!("{}t{row_index}c{cell_index}", self.path);
            row = row.child(element.child(self.render_cell(cell, &path)));
        }
        row
    }

    /// 표 제목(`caption`). 표 너비 안에서 가운데 맞춘 보통 굵기 글자다
    fn render_caption(&self, caption: &TableCaption, index: usize) -> Div {
        let path = format!("{}cap{index}", self.path);
        div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .text_align(gpui_kit::TextAlign::Center)
            .child(render_blocks(&caption.blocks, &self.context, &path))
    }

    /// 머리 칸은 굵게(브라우저 `th` 기본 모양)
    fn render_cell(&self, cell: &TableCell, path: &str) -> Div {
        if !cell.is_header {
            return render_blocks(&cell.blocks, &self.context, path);
        }
        let bold: Vec<Block> = cell
            .blocks
            .iter()
            .map(|block| match block {
                Block::Inlines(inlines) => Block::Inlines(vec![Inline::Strong(inlines.clone())]),
                Block::Paragraph(inlines) => {
                    Block::Paragraph(vec![Inline::Strong(inlines.clone())])
                }
                other => other.clone(),
            })
            .collect();
        render_blocks(&bold, &self.context, path)
    }
}

/// 여러 줄 글자 중 가장 긴 줄의 폭
fn widest_line(spans: &super::selectable_text::RichTextSpans, window: &Window) -> f32 {
    let mut widest: f32 = 0.0;
    let mut offset = 0;
    for line in spans.text.split('\n') {
        let runs = runs_for_range(&spans.runs, offset, line.len());
        let shaped = window.text_system().shape_line(
            SharedString::from(line.to_string()),
            px(14.0),
            &runs,
            None,
        );
        widest = widest.max(f32::from(shaped.width));
        offset += line.len() + 1;
    }
    widest
}

/// 한 줄에 해당하는 글자 구간만 잘라 낸 모양 목록
fn runs_for_range(
    runs: &[gpui_kit::TextRun],
    start: usize,
    length: usize,
) -> Vec<gpui_kit::TextRun> {
    let end = start + length;
    let mut position = 0;
    let mut sliced = Vec::new();
    for run in runs {
        let (run_start, run_end) = (position, position + run.len);
        position = run_end;
        let overlap = run_end.min(end).saturating_sub(run_start.max(start));
        if overlap > 0 {
            sliced.push(gpui_kit::TextRun {
                len: overlap,
                ..run.clone()
            });
        }
    }
    sliced
}

impl RenderOnce for MarkdownTable {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let widths = self.column_widths(window);
        let mut table = div().flex().flex_col().min_w_full();
        let captions = self.table.captions.iter().enumerate();
        for (index, caption) in captions.clone().filter(|(_, caption)| !caption.is_below) {
            table = table.child(self.render_caption(caption, index));
        }
        for (index, row) in self.table.rows.iter().enumerate() {
            let element = self.render_row(&row.cells, &widths, index);
            // 겹친 테두리 1px(`border-collapse`)
            table = table.child(match index {
                0 => element,
                _ => element.mt(px(-1.0)),
            });
        }
        for (index, caption) in captions.filter(|(_, caption)| caption.is_below) {
            table = table.child(self.render_caption(caption, index));
        }
        let id = SharedString::from(format!("{}/{}table", self.context.scope, self.path));
        div().id(id).w_full().overflow_x_scroll().child(table)
    }
}
