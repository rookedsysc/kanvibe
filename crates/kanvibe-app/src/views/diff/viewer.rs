//! 비교 모드 side-by-side 보기(TS `DiffMonacoViewer`: Monaco `DiffEditor` readOnly, renderSideBySide, minimap·개요 눈금자 끔).
//! 두 편집기는 세로·가로 스크롤을 하나로 함께 쓰고, 한쪽에만 있는 줄 자리는 빗금 칸으로 채워 줄 높이를 맞춘다.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    Bounds, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Render, ScrollEvent,
    Styled, Task, Window, canvas, div, point, px, size,
};

use super::line_diff::{LineChange, TextSpan, compute_line_changes};
use super::monaco::{
    CHARACTER_WIDTH, GutterLayout, GutterMarker, LINE_HEIGHT, LineDecoration, MonacoPane,
    PaneDocument, PaneRow, TextFill, paint_pane, palette,
};
use super::syntax::{detect_language, tokenize_document};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};

/// 비교 결과 한 벌. 두 편집기의 칸 수는 언제나 같다
pub struct SideBySideModel {
    pub original: Rc<PaneDocument>,
    pub modified: Rc<PaneDocument>,
    pub original_rows: Rc<Vec<PaneRow>>,
    pub modified_rows: Rc<Vec<PaneRow>>,
}

/// Monaco 모델처럼 `\n`으로 나누고 줄 끝 `\r`은 줄바꿈의 일부로 본다
pub fn split_document_lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

impl SideBySideModel {
    pub fn new(original_text: &str, modified_text: &str, file_path: &str) -> Self {
        let language = detect_language(file_path);
        let original_lines = split_document_lines(original_text);
        let modified_lines = split_document_lines(modified_text);
        let changes = compute_line_changes(&original_lines, &modified_lines);
        let (original_rows, modified_rows) =
            build_rows(original_lines.len(), modified_lines.len(), &changes);
        let original_tokens = tokenize_document(&original_lines, language);
        let modified_tokens = tokenize_document(&modified_lines, language);
        Self {
            original: Rc::new(PaneDocument::new(original_lines, original_tokens)),
            modified: Rc::new(PaneDocument::new(modified_lines, modified_tokens)),
            original_rows: Rc::new(original_rows),
            modified_rows: Rc::new(modified_rows),
        }
    }
}

fn unchanged_row(line: usize) -> PaneRow {
    PaneRow::Text {
        line,
        decoration: LineDecoration::default(),
    }
}

fn build_rows(
    original_len: usize,
    modified_len: usize,
    changes: &[LineChange],
) -> (Vec<PaneRow>, Vec<PaneRow>) {
    let (mut original_rows, mut modified_rows) = (Vec::new(), Vec::new());
    let (mut original_line, mut modified_line) = (0, 0);
    for change in changes {
        while original_line < change.original.start {
            original_rows.push(unchanged_row(original_line));
            modified_rows.push(unchanged_row(modified_line));
            original_line += 1;
            modified_line += 1;
        }
        push_change_rows(change, &mut original_rows, &mut modified_rows);
        (original_line, modified_line) = (change.original.end, change.modified.end);
    }
    while original_line < original_len || modified_line < modified_len {
        original_rows.push(unchanged_row(
            original_line.min(original_len.saturating_sub(1)),
        ));
        modified_rows.push(unchanged_row(
            modified_line.min(modified_len.saturating_sub(1)),
        ));
        original_line += 1;
        modified_line += 1;
    }
    (original_rows, modified_rows)
}

/// 바뀐 덩어리의 양쪽 줄을 넣고, 짧은 쪽 아래를 빗금 칸으로 채운다
fn push_change_rows(
    change: &LineChange,
    original_rows: &mut Vec<PaneRow>,
    modified_rows: &mut Vec<PaneRow>,
) {
    let is_pure_insertion = change.original.is_empty();
    let is_pure_deletion = change.modified.is_empty();
    let original_spans: Vec<TextSpan> = change
        .inner_changes
        .iter()
        .map(|inner| inner.original)
        .collect();
    let modified_spans: Vec<TextSpan> = change
        .inner_changes
        .iter()
        .map(|inner| inner.modified)
        .collect();
    for line in change.original.clone() {
        let decoration = change_decoration(
            line,
            &original_spans,
            is_pure_deletion,
            GutterMarker::Removed,
        );
        original_rows.push(PaneRow::Text { line, decoration });
    }
    for line in change.modified.clone() {
        let decoration = change_decoration(
            line,
            &modified_spans,
            is_pure_insertion,
            GutterMarker::Inserted,
        );
        modified_rows.push(PaneRow::Text { line, decoration });
    }
    let row_count = change.original.len().max(change.modified.len());
    for zone_offset in 0..row_count - change.original.len() {
        original_rows.push(PaneRow::Filler { zone_offset });
    }
    for zone_offset in 0..row_count - change.modified.len() {
        modified_rows.push(PaneRow::Filler { zone_offset });
    }
}

/// 줄 하나의 diff 배경. 한쪽이 통째로 빈 덩어리는 줄 전체를 글자 변경색으로 먼저 칠한다(Monaco `diffWholeLine*Decoration`)
fn change_decoration(
    line: usize,
    spans: &[TextSpan],
    is_whole_block: bool,
    marker: GutterMarker,
) -> LineDecoration {
    let (line_color, text_color) = match marker {
        GutterMarker::Inserted => (palette::INSERTED_LINE, palette::INSERTED_TEXT),
        GutterMarker::Removed => (palette::REMOVED_LINE, palette::REMOVED_TEXT),
    };
    LineDecoration {
        whole_line_fill: is_whole_block.then_some(text_color),
        line_background: Some(line_color),
        text_fills: spans
            .iter()
            .filter_map(|span| span_fill_on_line(span, line, text_color))
            .collect(),
        marker: Some(marker),
    }
}

fn span_fill_on_line(span: &TextSpan, line: usize, color: gpui_kit::Rgba) -> Option<TextFill> {
    if line < span.start.line || line > span.end.line {
        return None;
    }
    let start = if line == span.start.line {
        span.start.column
    } else {
        0
    };
    if line < span.end.line {
        return Some(TextFill {
            start,
            end: None,
            color,
        });
    }
    (span.end.column > start).then_some(TextFill {
        start,
        end: Some(span.end.column),
        color,
    })
}

pub struct DiffViewer {
    model: SideBySideModel,
    scroll_x: f32,
    scroll_y: f32,
    /// 마지막으로 그린 보기 전체 영역. 스크롤 한도와 줄 위치 기록에 쓴다
    last_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    is_scrollbar_visible: bool,
    _scrollbar_fade: Option<Task<()>>,
}

/// 스크롤이 멈춘 뒤 막대를 숨기기까지(Monaco 스크롤바 fade 시간)
pub const SCROLLBAR_FADE_AFTER: Duration = Duration::from_millis(800);

/// Monaco는 원본 편집기에 폭의 절반을 내림해 주고 나머지를 수정본에 준다
fn split_bounds(bounds: Bounds<Pixels>) -> (Bounds<Pixels>, Bounds<Pixels>) {
    let original_width = (f32::from(bounds.size.width) / 2.0).floor();
    let original = Bounds::new(bounds.origin, size(px(original_width), bounds.size.height));
    let modified_origin = point(bounds.origin.x + px(original_width), bounds.origin.y);
    let modified = Bounds::new(
        modified_origin,
        size(bounds.size.width - px(original_width), bounds.size.height),
    );
    (original, modified)
}

impl DiffViewer {
    pub fn new(model: SideBySideModel) -> Self {
        Self {
            model,
            scroll_x: 0.0,
            scroll_y: 0.0,
            last_bounds: Rc::new(Cell::new(None)),
            is_scrollbar_visible: false,
            _scrollbar_fade: None,
        }
    }

    fn handle_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(px(LINE_HEIGHT));
        let Some(bounds) = self.last_bounds.get() else {
            return;
        };
        let (original_bounds, modified_bounds) = split_bounds(bounds);
        let (original_max_x, max_y) = self.pane(true).scroll_limits(original_bounds);
        let (modified_max_x, _) = self.pane(false).scroll_limits(modified_bounds);
        let max_x = original_max_x.max(modified_max_x);
        self.scroll_y = (self.scroll_y - f32::from(delta.y)).clamp(0.0, max_y);
        self.scroll_x = (self.scroll_x - f32::from(delta.x)).clamp(0.0, max_x);
        self.is_scrollbar_visible = true;
        self._scrollbar_fade = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SCROLLBAR_FADE_AFTER).await;
            let _ = this.update(cx, |this, cx| {
                this.is_scrollbar_visible = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn pane(&self, is_original: bool) -> MonacoPane {
        let (document, rows, gutter) = if is_original {
            (
                &self.model.original,
                &self.model.original_rows,
                GutterLayout::ORIGINAL,
            )
        } else {
            (
                &self.model.modified,
                &self.model.modified_rows,
                GutterLayout::MODIFIED,
            )
        };
        MonacoPane {
            document: Rc::clone(document),
            rows: Rc::clone(rows),
            gutter,
            current_line: Some(0),
            caret: None,
            selection: None,
            bracket_boxes: Vec::new(),
            active_guide: None,
            has_right_shadow: is_original,
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            is_scrollbar_visible: self.is_scrollbar_visible,
        }
    }

    /// 재생 드라이버가 코드 줄을 글자로 찾을 수 있게 보이는 줄마다 위치를 남긴다. 위치는 지난 프레임의 영역으로 잰다
    fn line_targets(
        &self,
        pane: &MonacoPane,
        pane_left: f32,
        viewport_height: f32,
    ) -> Vec<gpui_kit::Div> {
        let first_row = (pane.scroll_y / LINE_HEIGHT).floor() as usize;
        let row_count = (viewport_height / LINE_HEIGHT).ceil() as usize + 1;
        let content_left = pane_left + pane.content_left();
        let mut targets = Vec::new();
        for (row_index, row) in pane.rows.iter().enumerate().skip(first_row).take(row_count) {
            let PaneRow::Text { line, .. } = row else {
                continue;
            };
            let text = pane.document.lines[*line].clone();
            let width = text.chars().count() as f32 * CHARACTER_WIDTH;
            targets.push(
                div()
                    .absolute()
                    .top(px(row_index as f32 * LINE_HEIGHT - pane.scroll_y))
                    .left(px(content_left - pane.scroll_x))
                    .w(px(width))
                    .h(px(LINE_HEIGHT))
                    .child(qa_target_layer(QaTargetDescription {
                        role: "code-line",
                        name: text.clone().into(),
                        text: text.into(),
                    })),
            );
        }
        targets
    }
}

impl Render for DiffViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (original, modified) = (self.pane(true), self.pane(false));
        let mut targets = Vec::new();
        if let Some(bounds) = self.last_bounds.get() {
            let (original_bounds, modified_bounds) = split_bounds(bounds);
            let height = f32::from(bounds.size.height);
            targets.extend(self.line_targets(&original, 0.0, height));
            targets.extend(self.line_targets(
                &modified,
                f32::from(modified_bounds.origin.x - original_bounds.origin.x),
                height,
            ));
        }
        let bounds_cell = Rc::clone(&self.last_bounds);
        let entity_id = cx.entity_id();
        let surface = canvas(
            // 영역이 바뀌면 줄 위치 기록과 스크롤 한도를 새 영역으로 다시 그린다
            move |bounds, window, _| {
                if bounds_cell.replace(Some(bounds)) != Some(bounds) {
                    window.on_next_frame(move |_, cx| cx.notify(entity_id));
                    window.refresh();
                }
            },
            move |bounds, _, window, cx| {
                let (original_bounds, modified_bounds) = split_bounds(bounds);
                paint_pane(original_bounds, &original, window, cx);
                paint_pane(modified_bounds, &modified, window, cx);
            },
        )
        .size_full();
        div()
            .id("diff-viewer")
            .relative()
            .size_full()
            .overflow_hidden()
            .on_scroll(
                cx.listener(|this, event: &ScrollEvent, _, cx| this.handle_scroll(event, cx)),
            )
            .child(surface)
            .children(targets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_pad_the_shorter_side_with_hatched_cells() {
        let model = SideBySideModel::new("a\nb\nc", "a\nB\nx\ny\nc\n", "f.ts");

        assert_eq!(model.original_rows.len(), model.modified_rows.len());
        let fillers = model
            .original_rows
            .iter()
            .filter(|row| matches!(row, PaneRow::Filler { .. }))
            .count();
        assert_eq!(
            fillers,
            model.modified.lines.len() - model.original.lines.len()
        );
    }

    #[test]
    fn pure_insertions_fill_the_whole_line_before_the_line_background() {
        let model = SideBySideModel::new("a", "a\nb", "f.txt");

        let PaneRow::Text { decoration, .. } = &model.modified_rows[1] else {
            panic!("inserted line must be text");
        };
        assert_eq!(decoration.whole_line_fill, Some(palette::INSERTED_TEXT));
        assert_eq!(decoration.line_background, Some(palette::INSERTED_LINE));
        assert_eq!(decoration.marker, Some(GutterMarker::Inserted));
        assert!(matches!(
            model.original_rows[1],
            PaneRow::Filler { zone_offset: 0 }
        ));
    }
}
