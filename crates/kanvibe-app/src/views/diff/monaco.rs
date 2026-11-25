//! Monaco 편집기 면(라이트 `vs` 테마)을 직접 그린다. TS 앱은 앱 테마와 상관없이 Monaco를 항상 라이트로 띄웠고, 기준 이미지의
//! 픽셀(줄 높이 18, 글자 13px DejaVu Sans Mono, 줄 번호 칸, 현재 줄 테두리, 들여쓰기 안내선, diff 배경, 빗금, 개요 눈금자)을 그대로 옮겼다.
//! 비교 화면의 두 편집기와 편집 모드 편집기가 이 화가를 함께 쓴다. 화면 상태(스크롤, 커서)는 호출하는 쪽이 갖는다.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::{
    App, Bounds, ContentMask, Hsla, Pixels, Rgba, SharedString, TextAlign, TextRun, Window, fill,
    point, px, size,
};

use super::line_diff::TextPoint;
use super::search::FindMatch;
use super::syntax::{Token, TokenKind, token_color};
use crate::fonts::{mono_font, sans_font};

pub const LINE_HEIGHT: f32 = 18.0;
pub const FONT_SIZE: f32 = 13.0;
/// DejaVu Sans Mono 13px 글자 폭(advance 1233/2048 em). 빈 줄의 안내선 위치에 쓴다
pub const CHARACTER_WIDTH: f32 = 1233.0 / 2048.0 * FONT_SIZE;
/// 세로 스크롤바·개요 눈금자 폭
pub const SCROLLBAR_WIDTH: f32 = 14.0;
/// GPUI는 줄 높이 18 안의 13px 글자 기준선을 반 픽셀 아래(13.5 → 14)로 반올림하고 Chromium은 반 행간을 내림해 13에 둔다.
/// 기준선이 정수 위치에 오도록 반 픽셀 올려 Chromium과 같은 행에 그린다
const TEXT_BASELINE_SHIFT: f32 = -0.5;
/// 줄 번호와 본문 사이 접기 단추 칸 폭
pub const FOLDING_CONTROL_WIDTH: f32 = 16.0;
/// Monaco `lineNumbersMinChars`
const MIN_LINE_NUMBER_DIGITS: usize = 5;

/// Monaco `vs` 테마 색. 반투명 값은 브라우저가 흰 배경 위에 섞은 결과와 같아지도록 CSS 값 그대로 둔다
pub mod palette {
    use gpui_kit::{Rgba, rgb};

    const fn rgba(red: u8, green: u8, blue: u8, alpha: f32) -> Rgba {
        Rgba {
            r: red as f32 / 255.0,
            g: green as f32 / 255.0,
            b: blue as f32 / 255.0,
            a: alpha,
        }
    }

    pub fn editor_background() -> Rgba {
        rgb(0xFFFFFE)
    }
    pub fn line_number() -> Rgba {
        rgb(0x237893)
    }
    pub fn active_line_number() -> Rgba {
        rgb(0x0B216F)
    }
    pub fn line_highlight_border() -> Rgba {
        rgb(0xEEEEEE)
    }
    pub fn indent_guide() -> Rgba {
        rgb(0xD3D3D3)
    }
    pub fn active_indent_guide() -> Rgba {
        rgb(0x939393)
    }
    pub fn diagonal_fill() -> Rgba {
        rgb(0xD3D3D2)
    }
    pub fn selection() -> Rgba {
        rgb(0xADD6FF)
    }
    pub fn bracket_match_border() -> Rgba {
        rgb(0xB9B9B9)
    }
    pub fn cursor() -> Rgba {
        rgb(0x000000)
    }
    /// `editor.findMatchBackground`
    pub fn find_match() -> Rgba {
        rgb(0xA8AC94)
    }
    pub fn fold_placeholder() -> Rgba {
        rgb(0x808080)
    }
    /// `editorGutter.foldingControlForeground`(icon.foreground)
    pub fn fold_control() -> Rgba {
        rgb(0x424242)
    }
    /// `editor.findMatchHighlightBackground`(#EA5C0055)
    pub const FIND_MATCH_HIGHLIGHT: Rgba = rgba(234, 92, 0, 85.0 / 255.0);
    /// `editor.foldBackground`(선택색 30%)
    pub const FOLD_BACKGROUND: Rgba = rgba(173, 214, 255, 0.3);
    /// `diffEditor.insertedLineBackground`
    pub const INSERTED_LINE: Rgba = rgba(155, 185, 85, 0.2);
    /// `diffEditor.removedLineBackground`
    pub const REMOVED_LINE: Rgba = rgba(255, 0, 0, 0.2);
    /// `diffEditor.insertedTextBackground`(#9ccc2c40)
    pub const INSERTED_TEXT: Rgba = rgba(156, 204, 44, 0.25);
    /// `diffEditor.removedTextBackground`(#ff000033)
    pub const REMOVED_TEXT: Rgba = rgba(255, 0, 0, 0.2);
    /// `editorBracketMatch.background`
    pub const BRACKET_MATCH: Rgba = rgba(0, 100, 0, 0.1);
    /// 개요 눈금자 왼쪽 테두리(흰 바탕에서 #ECECEB가 되는 값)
    pub const RULER_BORDER: Rgba = rgba(93, 93, 93, 0.12);
    /// 줄 번호 칸의 `+`·`−` 아이콘 색
    pub const GUTTER_MARKER: Rgba = rgba(0, 0, 0, 0.5);
    /// 개요 눈금자의 커서 표시(2px, 흰 바탕에서 #4C4C4C·#474747)
    pub const RULER_CURSOR: [Rgba; 2] = [rgba(76, 76, 76, 1.0), rgba(71, 71, 71, 1.0)];
    /// 원본 편집기 오른쪽 그림자(`scrollbar.shadow` #dddddd를 5px 흐림). 오른쪽 끝 6열의 검정 불투명도
    pub const RIGHT_SHADOW_ALPHAS: [f32; 6] = [
        1.0 / 255.0,
        3.0 / 255.0,
        6.0 / 255.0,
        10.0 / 255.0,
        15.0 / 255.0,
        20.0 / 255.0,
    ];
}

/// 줄 번호와 본문 위치. 값은 편집기 왼쪽 끝에서 잰 거리다
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GutterLayout {
    /// 줄 번호 칸 왼쪽(Monaco `glyphMargin` 폭만큼 밀린다)
    pub number_left: f32,
    /// 줄 번호 칸과 본문 사이 장식 칸 폭(diff 표시 10, 접기 칸 포함 26)
    pub decoration_width: f32,
}

impl GutterLayout {
    /// 비교 화면 원본 편집기: glyph 여백 18 + 줄 번호 + 장식 10
    pub const ORIGINAL: GutterLayout = GutterLayout {
        number_left: 18.0,
        decoration_width: 10.0,
    };
    pub const MODIFIED: GutterLayout = GutterLayout {
        number_left: 0.0,
        decoration_width: 10.0,
    };
    /// 편집 모드: 줄 번호 + 장식 10 + 접기 칸 16
    pub const EDITOR: GutterLayout = GutterLayout {
        number_left: 0.0,
        decoration_width: 26.0,
    };

    pub fn number_right(&self, line_count: usize) -> f32 {
        self.number_left + line_number_width(line_count)
    }

    pub fn content_left(&self, line_count: usize) -> f32 {
        self.number_right(line_count) + self.decoration_width
    }
}

/// Monaco `lineNumbersWidth`: 최소 5자리 숫자 폭을 반올림한다
fn line_number_width(line_count: usize) -> f32 {
    let digits = line_count.to_string().len().max(MIN_LINE_NUMBER_DIGITS);
    (digits as f32 * CHARACTER_WIDTH).round()
}

/// 문서 한 벌. 줄 번호 칸과 안내선은 이 값으로 정한다
#[derive(Debug, Clone)]
pub struct PaneDocument {
    pub lines: Vec<String>,
    pub tokens: Vec<Vec<Token>>,
    /// 줄마다 그릴 들여쓰기 안내선 개수
    pub guide_levels: Vec<usize>,
    pub indent_size: usize,
    /// 마지막 줄이 비어 있으면 그 줄 번호를 흐리게 그린다(Monaco `renderFinalNewline: dimmed`)
    pub is_final_line_dimmed: bool,
}

impl PaneDocument {
    pub fn new(lines: Vec<String>, tokens: Vec<Vec<Token>>) -> Self {
        let indent_size = detect_indent_size(&lines);
        let guide_levels = compute_guide_levels(&lines, indent_size);
        let is_final_line_dimmed =
            lines.len() > 1 && lines.last().is_some_and(|line| line.is_empty());
        Self {
            lines,
            tokens,
            guide_levels,
            indent_size,
            is_final_line_dimmed,
        }
    }
}

fn indentation_columns(line: &str, tab_size: usize) -> usize {
    line.chars()
        .take_while(|character| *character == ' ' || *character == '\t')
        .map(|character| if character == '\t' { tab_size } else { 1 })
        .sum()
}

fn detect_indent_size(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| indentation_columns(line, 4))
        .filter(|indent| *indent > 0)
        .min()
        .unwrap_or(4)
}

/// 빈 줄은 위아래 내용 줄로 정한다. 위가 더 얕으면 블록이 열린 자리라 위 + 1, 아니면 둘 중 깊은 쪽(Monaco 안내선 규칙의 근사)
fn compute_guide_levels(lines: &[String], indent_size: usize) -> Vec<usize> {
    let content_levels: Vec<Option<usize>> = lines
        .iter()
        .map(|line| {
            (!line.trim().is_empty())
                .then(|| indentation_columns(line, indent_size).div_ceil(indent_size))
        })
        .collect();
    (0..lines.len())
        .map(|index| {
            content_levels[index].unwrap_or_else(|| {
                let above = content_levels[..index]
                    .iter()
                    .rev()
                    .find_map(|level| *level)
                    .unwrap_or(0);
                let below = content_levels[index + 1..]
                    .iter()
                    .find_map(|level| *level)
                    .unwrap_or(0);
                if above < below {
                    above + 1
                } else {
                    above.max(below)
                }
            })
        })
        .collect()
}

/// 커서가 있는 블록의 안내선(Monaco 활성 안내선). 커서 줄이 블록을 여는 줄이면 그 아래 블록을 가리킨다
pub fn active_indent_guide(document: &PaneDocument, cursor_line: usize) -> Option<ActiveGuide> {
    let levels = &document.guide_levels;
    let here = *levels.get(cursor_line)?;
    let next = levels.get(cursor_line + 1).copied().unwrap_or(0);
    let (level, anchor) = if next > here {
        (here, cursor_line + 1)
    } else if here > 0 {
        (here - 1, cursor_line)
    } else {
        return None;
    };
    let start = (0..anchor)
        .rev()
        .take_while(|index| levels[*index] > level)
        .last()
        .unwrap_or(anchor);
    let end = (anchor..levels.len())
        .take_while(|index| levels[*index] > level)
        .last()
        .map_or(anchor, |last| last + 1);
    Some(ActiveGuide {
        lines: start..end,
        level,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveGuide {
    pub lines: Range<usize>,
    pub level: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterMarker {
    Inserted,
    Removed,
}

/// 줄 안 배경 칠. `end`가 없으면 줄바꿈까지 덮은 변경이라 편집기 오른쪽 끝까지 채운다(Monaco `shouldFillLineOnLineBreak`)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextFill {
    pub start: usize,
    pub end: Option<usize>,
    pub color: Rgba,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineDecoration {
    /// 줄 전체가 삽입·삭제된 덩어리. 줄 배경보다 먼저 칠한다
    pub whole_line_fill: Option<Rgba>,
    pub line_background: Option<Rgba>,
    pub text_fills: Vec<TextFill>,
    pub marker: Option<GutterMarker>,
}

/// 화면의 한 줄 칸. 반대편에만 있는 줄 자리는 빗금 칸으로 채운다
#[derive(Debug, Clone, PartialEq)]
pub enum PaneRow {
    Text {
        line: usize,
        decoration: LineDecoration,
    },
    /// `zone_offset`: 빗금 구역 안에서 몇 번째 칸인지. 무늬는 구역 위쪽에 맞춘다
    Filler { zone_offset: usize },
}

/// 한 번 그릴 편집기 면
pub struct MonacoPane {
    pub document: Rc<PaneDocument>,
    pub rows: Rc<Vec<PaneRow>>,
    pub gutter: GutterLayout,
    /// 현재 줄(테두리와 진한 줄 번호, 눈금자 표시)
    pub current_line: Option<usize>,
    pub caret: Option<TextPoint>,
    pub selection: Option<(TextPoint, TextPoint)>,
    pub bracket_boxes: Vec<TextPoint>,
    pub active_guide: Option<ActiveGuide>,
    pub has_right_shadow: bool,
    pub scroll_x: f32,
    pub scroll_y: f32,
    /// 스크롤한 직후에만 막대를 보인다(Monaco 스크롤바는 움직임이 멈추면 사라진다)
    pub is_scrollbar_visible: bool,
    /// 찾기 결과(`editor.findMatchHighlightBackground`)와 지금 결과(`editor.findMatchBackground`)
    pub find_highlights: Rc<Vec<FindMatch>>,
    pub current_find_match: Option<FindMatch>,
    /// 접을 수 있는 줄과 접혔는지. 비교 모드 편집기는 비어 있다(diff 편집기는 접기를 끈다)
    pub fold_markers: Rc<Vec<(usize, bool)>>,
    pub are_fold_controls_visible: bool,
}

impl MonacoPane {
    /// 이 영역에서 가로·세로로 스크롤할 수 있는 최대 거리(Monaco `scrollBeyondLastLine: false`)
    pub fn scroll_limits(&self, bounds: Bounds<Pixels>) -> (f32, f32) {
        let viewport_width = f32::from(bounds.size.width) - self.content_left() - SCROLLBAR_WIDTH;
        let max_x = (widest_line_width(&self.document) + CHARACTER_WIDTH - viewport_width).max(0.0);
        let max_y = (self.content_height() - f32::from(bounds.size.height)).max(0.0);
        (max_x, max_y)
    }

    pub fn content_left(&self) -> f32 {
        self.gutter.content_left(self.document.lines.len())
    }

    /// 스크롤 없이 모든 칸을 그렸을 때의 높이
    pub fn content_height(&self) -> f32 {
        self.rows.len() as f32 * LINE_HEIGHT
    }
}

/// 줄 텍스트를 토큰 색으로 모양 잡는다
pub fn shape_code_line(
    document: &PaneDocument,
    line: usize,
    window: &mut Window,
) -> gpui_kit::ShapedLine {
    let text = document.lines[line].clone();
    let runs = token_runs(
        &text,
        document
            .tokens
            .get(line)
            .map(Vec::as_slice)
            .unwrap_or_default(),
    );
    window
        .text_system()
        .shape_line(SharedString::from(text), px(FONT_SIZE), &runs, None)
}

fn token_runs(text: &str, tokens: &[Token]) -> Vec<TextRun> {
    let font = mono_font(400);
    let run = |len: usize, kind: TokenKind| TextRun {
        len,
        font: font.clone(),
        color: Hsla::from(token_color(kind)),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut runs = Vec::with_capacity(tokens.len() + 1);
    let mut offset = 0;
    for token in tokens {
        if token.range.start > offset {
            runs.push(run(token.range.start - offset, TokenKind::Plain));
        }
        runs.push(run(token.range.len(), token.kind));
        offset = token.range.end;
    }
    if offset < text.len() || runs.is_empty() {
        runs.push(run(text.len() - offset, TokenKind::Plain));
    }
    runs
}

/// 편집기 면 전체를 그린다
pub fn paint_pane(bounds: Bounds<Pixels>, pane: &MonacoPane, window: &mut Window, cx: &mut App) {
    window.paint_quad(fill(bounds, palette::editor_background()));
    let first_row = (pane.scroll_y / LINE_HEIGHT).floor().max(0.0) as usize;
    let visible_rows = (f32::from(bounds.size.height) / LINE_HEIGHT).ceil() as usize + 1;
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        for row_index in first_row..(first_row + visible_rows).min(pane.rows.len()) {
            let top = f32::from(bounds.origin.y) + row_index as f32 * LINE_HEIGHT - pane.scroll_y;
            let painter = RowPainter { bounds, pane, top };
            match &pane.rows[row_index] {
                PaneRow::Text { line, decoration } => {
                    painter.paint_text_row(*line, decoration, window, cx)
                }
                PaneRow::Filler { zone_offset } => painter.paint_filler(*zone_offset, window),
            }
        }
        paint_overlays(bounds, pane, window);
        if pane.is_scrollbar_visible {
            paint_scrollbar_sliders(bounds, pane, window);
        }
    });
}

/// 세로(14px)·가로(12px) 스크롤 막대(`scrollbarSlider.background`)
fn paint_scrollbar_sliders(bounds: Bounds<Pixels>, pane: &MonacoPane, window: &mut Window) {
    const HORIZONTAL_SCROLLBAR_HEIGHT: f32 = 12.0;
    const MIN_SLIDER_LENGTH: f32 = 20.0;
    let slider_color = Rgba {
        r: 100.0 / 255.0,
        g: 100.0 / 255.0,
        b: 100.0 / 255.0,
        a: 0.4,
    };
    let (max_x, max_y) = pane.scroll_limits(bounds);
    let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let slider = |viewport: f32, content: f32, offset: f32, max: f32| {
        let length = (viewport * viewport / content)
            .max(MIN_SLIDER_LENGTH)
            .min(viewport);
        (offset / max * (viewport - length), length)
    };
    if max_y > 0.0 {
        let (offset, length) = slider(height, height + max_y, pane.scroll_y, max_y);
        let bar = Bounds::new(
            point(px(left + width - SCROLLBAR_WIDTH), px(top + offset)),
            size(px(SCROLLBAR_WIDTH), px(length)),
        );
        window.paint_quad(fill(bar, slider_color));
    }
    if max_x > 0.0 {
        let track_left = left + pane.content_left();
        let track_width = width - pane.content_left() - SCROLLBAR_WIDTH;
        let (offset, length) = slider(track_width, track_width + max_x, pane.scroll_x, max_x);
        let bar_top = top + height - HORIZONTAL_SCROLLBAR_HEIGHT;
        let bar = Bounds::new(
            point(px(track_left + offset), px(bar_top)),
            size(px(length), px(HORIZONTAL_SCROLLBAR_HEIGHT)),
        );
        window.paint_quad(fill(bar, slider_color));
    }
}

/// 한 칸을 그리는 데 필요한 위치
struct RowPainter<'a> {
    bounds: Bounds<Pixels>,
    pane: &'a MonacoPane,
    top: f32,
}

impl RowPainter<'_> {
    fn left(&self) -> f32 {
        f32::from(self.bounds.origin.x)
    }

    fn right(&self) -> f32 {
        f32::from(self.bounds.origin.x + self.bounds.size.width)
    }

    /// 가로 스크롤을 반영한 본문 시작 x
    fn text_origin_x(&self) -> f32 {
        self.left() + self.pane.content_left() - self.pane.scroll_x
    }

    fn content_mask(&self) -> ContentMask<Pixels> {
        let content_left = self.left() + self.pane.content_left();
        ContentMask {
            bounds: Bounds::new(
                point(px(content_left), self.bounds.origin.y),
                size(px(self.right() - content_left), self.bounds.size.height),
            ),
        }
    }

    fn paint_rect(
        &self,
        window: &mut Window,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Rgba,
    ) {
        if width > 0.0 && height > 0.0 {
            window.paint_quad(fill(
                Bounds::new(point(px(x), px(y)), size(px(width), px(height))),
                color,
            ));
        }
    }

    fn paint_text_row(
        &self,
        line: usize,
        decoration: &LineDecoration,
        window: &mut Window,
        cx: &mut App,
    ) {
        let shaped = shape_code_line(&self.pane.document, line, window);
        window.with_content_mask(Some(self.content_mask()), |window| {
            if self.pane.current_line == Some(line) {
                self.paint_line_highlight(window);
            }
            self.paint_indent_guides(line, window);
            self.paint_decoration_backgrounds(decoration, &shaped, window);
            if self.is_collapsed_fold(line) {
                self.paint_rect(
                    window,
                    self.left() + self.pane.content_left(),
                    self.top,
                    self.right(),
                    LINE_HEIGHT,
                    palette::FOLD_BACKGROUND,
                );
            }
            self.paint_find_highlights(line, &shaped, window);
            self.paint_selection(line, &shaped, window);
            self.paint_bracket_boxes(line, &shaped, window);
            let origin = point(px(self.text_origin_x()), px(self.top + TEXT_BASELINE_SHIFT));
            let _ = shaped.paint(origin, px(LINE_HEIGHT), TextAlign::Left, None, window, cx);
            if self.is_collapsed_fold(line) {
                self.paint_fold_placeholder(&shaped, window, cx);
            }
            self.paint_caret(line, &shaped, window);
        });
        self.paint_line_number(line, window, cx);
        self.paint_fold_control(line, window, cx);
        if let Some(marker) = decoration.marker {
            self.paint_gutter_marker(marker, window);
        }
    }

    fn is_collapsed_fold(&self, line: usize) -> bool {
        self.pane.fold_markers.contains(&(line, true))
    }

    /// 다른 결과는 반투명 주황, 지금 결과는 회녹색으로 글자 뒤를 칠한다
    fn paint_find_highlights(
        &self,
        line: usize,
        shaped: &gpui_kit::ShapedLine,
        window: &mut Window,
    ) {
        let current = self
            .pane
            .current_find_match
            .as_ref()
            .filter(|found| found.line == line);
        for found in self
            .pane
            .find_highlights
            .iter()
            .filter(|found| found.line == line)
        {
            let color = if Some(found) == current {
                palette::find_match()
            } else {
                palette::FIND_MATCH_HIGHLIGHT
            };
            let start =
                (self.text_origin_x() + f32::from(shaped.x_for_index(found.range.start))).round();
            let end =
                (self.text_origin_x() + f32::from(shaped.x_for_index(found.range.end))).round();
            self.paint_rect(window, start, self.top, end - start, LINE_HEIGHT, color);
        }
    }

    /// 접힌 줄 끝의 `⋯`(`editor.foldPlaceholderForeground`, 앞 여백 0.2em)
    fn paint_fold_placeholder(
        &self,
        shaped: &gpui_kit::ShapedLine,
        window: &mut Window,
        cx: &mut App,
    ) {
        let x = self.text_origin_x() + f32::from(shaped.width) + FONT_SIZE * 0.2;
        let label = paint_label(
            "⋯",
            mono_font(400),
            FONT_SIZE,
            palette::fold_placeholder(),
            window,
        );
        let _ = label.paint(
            point(px(x), px(self.top + TEXT_BASELINE_SHIFT)),
            px(LINE_HEIGHT),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    /// 접기 단추: 접힌 범위는 늘 `›`, 펼친 범위는 줄 번호 칸에 포인터가 있을 때만 `⌄`
    fn paint_fold_control(&self, line: usize, window: &mut Window, cx: &mut App) {
        let Some((_, is_collapsed)) = self
            .pane
            .fold_markers
            .iter()
            .find(|(start, _)| *start == line)
        else {
            return;
        };
        if !is_collapsed && !self.pane.are_fold_controls_visible {
            return;
        }
        let glyph = if *is_collapsed { "›" } else { "⌄" };
        let label = paint_label(glyph, sans_font(400), 14.0, palette::fold_control(), window);
        let control_left = self.left() + self.pane.content_left() - FOLDING_CONTROL_WIDTH;
        let x = control_left + (FOLDING_CONTROL_WIDTH - f32::from(label.width)) / 2.0;
        let _ = label.paint(
            point(px(x), px(self.top)),
            px(LINE_HEIGHT),
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    /// 현재 줄 테두리(위·아래·왼쪽 2px). 오른쪽 변은 편집기 밖으로 밀려 보이지 않는다
    fn paint_line_highlight(&self, window: &mut Window) {
        let left = self.left() + self.pane.content_left();
        let width = self.right() - left;
        let color = palette::line_highlight_border();
        self.paint_rect(window, left, self.top, width, 2.0, color);
        self.paint_rect(
            window,
            left,
            self.top + LINE_HEIGHT - 2.0,
            width,
            2.0,
            color,
        );
        self.paint_rect(window, left, self.top, 2.0, LINE_HEIGHT, color);
    }

    fn paint_indent_guides(&self, line: usize, window: &mut Window) {
        let document = &self.pane.document;
        let guide_step = document.indent_size as f32 * CHARACTER_WIDTH;
        for level in 0..document.guide_levels[line] {
            let is_active = self
                .pane
                .active_guide
                .as_ref()
                .is_some_and(|guide| guide.level == level && guide.lines.contains(&line));
            let color = if is_active {
                palette::active_indent_guide()
            } else {
                palette::indent_guide()
            };
            let x = (self.text_origin_x() + level as f32 * guide_step).round();
            self.paint_rect(window, x, self.top, 1.0, LINE_HEIGHT, color);
        }
    }

    fn paint_decoration_backgrounds(
        &self,
        decoration: &LineDecoration,
        shaped: &gpui_kit::ShapedLine,
        window: &mut Window,
    ) {
        let left = self.left() + self.pane.content_left();
        let full_width = self.right() - left;
        if let Some(color) = decoration.whole_line_fill {
            self.paint_rect(window, left, self.top, full_width, LINE_HEIGHT, color);
        }
        if let Some(color) = decoration.line_background {
            self.paint_rect(window, left, self.top, full_width, LINE_HEIGHT, color);
        }
        for text_fill in &decoration.text_fills {
            let start = self.text_origin_x() + f32::from(shaped.x_for_index(text_fill.start));
            let end = match text_fill.end {
                Some(end) => self.text_origin_x() + f32::from(shaped.x_for_index(end)),
                None => self.right(),
            };
            self.paint_rect(
                window,
                start.round(),
                self.top,
                end.round() - start.round(),
                LINE_HEIGHT,
                text_fill.color,
            );
        }
    }

    fn paint_selection(&self, line: usize, shaped: &gpui_kit::ShapedLine, window: &mut Window) {
        let Some((start, end)) = self.pane.selection else {
            return;
        };
        if line < start.line || line > end.line {
            return;
        }
        let from = if line == start.line { start.column } else { 0 };
        let start_x = self.text_origin_x() + f32::from(shaped.x_for_index(from));
        let end_x = if line == end.line {
            self.text_origin_x() + f32::from(shaped.x_for_index(end.column))
        } else {
            // 줄바꿈까지 고른 줄은 줄 끝 뒤로 한 칸 더 칠한다
            self.text_origin_x() + f32::from(shaped.width) + CHARACTER_WIDTH
        };
        self.paint_rect(
            window,
            start_x.round(),
            self.top,
            end_x.round() - start_x.round(),
            LINE_HEIGHT,
            palette::selection(),
        );
    }

    fn paint_bracket_boxes(&self, line: usize, shaped: &gpui_kit::ShapedLine, window: &mut Window) {
        for bracket in self
            .pane
            .bracket_boxes
            .iter()
            .filter(|bracket| bracket.line == line)
        {
            let next = self.pane.document.lines[line][bracket.column..]
                .chars()
                .next()
                .map_or(0, char::len_utf8);
            let start =
                (self.text_origin_x() + f32::from(shaped.x_for_index(bracket.column))).round();
            let end = (self.text_origin_x() + f32::from(shaped.x_for_index(bracket.column + next)))
                .round();
            let border = palette::bracket_match_border();
            self.paint_rect(
                window,
                start,
                self.top,
                end - start,
                LINE_HEIGHT,
                palette::BRACKET_MATCH,
            );
            self.paint_rect(window, start, self.top, end - start, 1.0, border);
            self.paint_rect(
                window,
                start,
                self.top + LINE_HEIGHT - 1.0,
                end - start,
                1.0,
                border,
            );
            self.paint_rect(window, start, self.top, 1.0, LINE_HEIGHT, border);
            self.paint_rect(window, end - 1.0, self.top, 1.0, LINE_HEIGHT, border);
        }
    }

    fn paint_caret(&self, line: usize, shaped: &gpui_kit::ShapedLine, window: &mut Window) {
        let Some(caret) = self.pane.caret.filter(|caret| caret.line == line) else {
            return;
        };
        let x = (self.text_origin_x() + f32::from(shaped.x_for_index(caret.column))).round();
        self.paint_rect(window, x, self.top, 2.0, LINE_HEIGHT, palette::cursor());
    }

    fn paint_line_number(&self, line: usize, window: &mut Window, cx: &mut App) {
        let document = &self.pane.document;
        let is_last = line + 1 == document.lines.len();
        let color = if self.pane.current_line == Some(line) {
            palette::active_line_number()
        } else if is_last && document.is_final_line_dimmed {
            Rgba {
                a: 0.4,
                ..palette::line_number()
            }
        } else {
            palette::line_number()
        };
        let label = SharedString::from((line + 1).to_string());
        let run = TextRun {
            len: label.len(),
            font: mono_font(400),
            color: color.into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(label, px(FONT_SIZE), &[run], None);
        let number_right = self.left() + self.pane.gutter.number_right(document.lines.len());
        let origin = point(
            px(number_right) - shaped.width,
            px(self.top + TEXT_BASELINE_SHIFT),
        );
        let _ = shaped.paint(origin, px(LINE_HEIGHT), TextAlign::Left, None, window, cx);
    }

    /// Monaco codicon `diff-insert`·`diff-remove`를 픽셀 단위로 옮긴 십자·가로선
    fn paint_gutter_marker(&self, marker: GutterMarker, window: &mut Window) {
        let x = self.left()
            + self
                .pane
                .gutter
                .number_right(self.pane.document.lines.len());
        let color = palette::GUTTER_MARKER;
        match marker {
            GutterMarker::Removed => {
                self.paint_rect(window, x + 1.0, self.top + 9.0, 8.0, 1.0, color)
            }
            GutterMarker::Inserted => {
                self.paint_rect(window, x + 1.0, self.top + 9.0, 8.0, 1.0, color);
                self.paint_rect(window, x + 5.0, self.top + 5.0, 1.0, 9.0, color);
            }
        }
    }

    /// 반대편에만 있는 줄 자리: 8px 주기의 2px 사선(`diffEditor.diagonalFill`). 무늬는 본문 왼쪽과 구역 위쪽에 맞춘다
    fn paint_filler(&self, zone_offset: usize, window: &mut Window) {
        window.with_content_mask(Some(self.content_mask()), |window| {
            let color = palette::diagonal_fill();
            let pattern_left = self.text_origin_x();
            let first_tile = pattern_left
                + ((self.left() + self.pane.content_left() - pattern_left) / 8.0).floor() * 8.0
                - 8.0;
            for row in 0..LINE_HEIGHT as usize {
                let phase = (zone_offset * LINE_HEIGHT as usize + row) % 8;
                let stripe_offset = (6 - phase as isize).rem_euclid(8) as f32;
                let mut tile_x = first_tile;
                while tile_x < self.right() {
                    self.paint_rect(
                        window,
                        tile_x + stripe_offset,
                        self.top + row as f32,
                        2.0,
                        1.0,
                        color,
                    );
                    tile_x += 8.0;
                }
            }
        });
    }
}

/// 개요 눈금자 테두리와 커서 표시, 원본 편집기 오른쪽 그림자
fn paint_overlays(bounds: Bounds<Pixels>, pane: &MonacoPane, window: &mut Window) {
    let right = f32::from(bounds.origin.x + bounds.size.width);
    let top = f32::from(bounds.origin.y);
    let height = f32::from(bounds.size.height);
    let ruler_left = right - SCROLLBAR_WIDTH;
    let rect = |x: f32, y: f32, width: f32, row_height: f32| {
        Bounds::new(point(px(x), px(y)), size(px(width), px(row_height)))
    };
    window.paint_quad(fill(
        rect(ruler_left, top, 1.0, height),
        palette::RULER_BORDER,
    ));
    if let Some(row_index) = pane
        .current_line
        .and_then(|line| current_row_index(pane, line))
    {
        let content_height = pane.content_height().max(height);
        let marker_top = (top + row_index as f32 * LINE_HEIGHT * height / content_height - 1.0)
            .max(top)
            .round();
        for (offset, color) in palette::RULER_CURSOR.iter().enumerate() {
            window.paint_quad(fill(
                rect(
                    ruler_left + 1.0,
                    marker_top + offset as f32,
                    SCROLLBAR_WIDTH - 1.0,
                    1.0,
                ),
                *color,
            ));
        }
    }
    if pane.has_right_shadow {
        for (offset, alpha) in palette::RIGHT_SHADOW_ALPHAS.iter().enumerate() {
            let x = right - palette::RIGHT_SHADOW_ALPHAS.len() as f32 + offset as f32;
            window.paint_quad(fill(
                rect(x, top, 1.0, height),
                Rgba {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: *alpha,
                },
            ));
        }
    }
}

fn current_row_index(pane: &MonacoPane, line: usize) -> Option<usize> {
    pane.rows
        .iter()
        .position(|row| matches!(row, PaneRow::Text { line: row_line, .. } if *row_line == line))
}

/// 한 색 한 글꼴 글자 한 줄
fn paint_label(
    text: &'static str,
    font: gpui_kit::Font,
    font_size: f32,
    color: Rgba,
    window: &mut Window,
) -> gpui_kit::ShapedLine {
    let run = TextRun {
        len: text.len(),
        font,
        color: color.into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text), px(font_size), &[run], None)
}

/// 클릭 위치를 문서 위치로 바꾼다. 빗금 칸이나 문서 밖은 가장 가까운 줄로 붙인다
pub fn hit_test(
    pane: &MonacoPane,
    bounds: Bounds<Pixels>,
    position: gpui_kit::Point<Pixels>,
    window: &mut Window,
) -> TextPoint {
    let relative_y = f32::from(position.y - bounds.origin.y) + pane.scroll_y;
    let row_index = ((relative_y / LINE_HEIGHT).floor().max(0.0) as usize)
        .min(pane.rows.len().saturating_sub(1));
    let line = pane.rows[..=row_index]
        .iter()
        .rev()
        .find_map(|row| match row {
            PaneRow::Text { line, .. } => Some(*line),
            PaneRow::Filler { .. } => None,
        })
        .unwrap_or(0);
    let shaped = shape_code_line(&pane.document, line, window);
    let text_x = f32::from(position.x - bounds.origin.x) - pane.content_left() + pane.scroll_x;
    TextPoint {
        line,
        column: shaped.closest_index_for_x(px(text_x.max(0.0))),
    }
}

/// 문서에서 가장 긴 줄의 폭(가로 스크롤 한도)
pub fn widest_line_width(document: &PaneDocument) -> f32 {
    document
        .lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as f32
        * CHARACTER_WIDTH
}
