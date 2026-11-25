//! timeline 다이어그램(가로, timelineRenderer.ts·svgDraw.js). 기간 상자를 200 간격으로 늘어놓고, 각 기간
//! 아래 점선 화살표를 따라 사건 상자를 쌓는다. 구역이 있으면 기간들 위에 구역 상자를 둔다.
//! 상자 글자는 SVG `<text>`(가운데 정렬, `dominant-baseline: middle`)라 라벨 방식과 상관없이 같다.
//! `timeline TD`는 세로 렌더러([`vertical`])가 그린다.

mod colors;
mod parser;
mod vertical;

use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, TextItem, polygon};
use crate::text::{FontBook, FontWeight};
use crate::{DiagramError, RenderContext};
use colors::{SECTION_COLOR_COUNT, SectionColors, TimelineColors};
use parser::Period;

const LEFT_MARGIN: f64 = 150.0;
const NODE_TEXT_WIDTH: f64 = 150.0;
const NODE_PADDING: f64 = 20.0;
/// 기간·사건 상자 폭(글자 폭 150 + 안쪽 여백 2 × 20)
const NODE_WIDTH: f64 = NODE_TEXT_WIDTH + 2.0 * NODE_PADDING;
const COLUMN_STEP: f64 = 200.0;
const TOP: f64 = 50.0;
const EVENT_MIN_HEIGHT: f64 = 50.0;
const EVENT_GAP: f64 = 10.0;
const FONT_SIZE: f64 = 16.0;
/// SVG 글자 줄 간격(`tspan dy="1.1em"`)
const LINE_HEIGHT: f64 = 1.1 * FONT_SIZE;
/// Noto Sans OS/2 sxHeight(글꼴 단위 1000 기준). `dominant-baseline: middle`과 제목 `4ex`의 기준이다
const X_HEIGHT_RATIO: f64 = 0.536;
const TITLE_FONT_SIZE: f64 = 4.0 * X_HEIGHT_RATIO * FONT_SIZE;
const CORNER_RADIUS: f64 = 5.0;
const VIEW_PADDING: f64 = 50.0;
/// TS가 기간 높이를 미리 잴 때 기간 이름 대신 작업 객체를 글자로 넣어 생기는 문자열
const MEASURED_PERIOD_TEXT: &str = "[object Object]";

/// SVG getBBox와 같은 축 정렬 경계
#[derive(Clone, Copy, Debug)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Bounds {
    const EMPTY: Self = Self {
        left: f64::INFINITY,
        top: f64::INFINITY,
        right: f64::NEG_INFINITY,
        bottom: f64::NEG_INFINITY,
    };

    fn include(&mut self, left: f64, top: f64, right: f64, bottom: f64) {
        self.left = self.left.min(left);
        self.top = self.top.min(top);
        self.right = self.right.max(right);
        self.bottom = self.bottom.max(bottom);
    }

    fn width(&self) -> f64 {
        (self.right - self.left).max(0.0)
    }
}

/// 상자 왼쪽 위와 글자 줄바꿈 폭·안쪽 여백. 상자 폭은 글자 폭 + 안쪽 여백 2배다
#[derive(Clone, Copy, Debug)]
struct NodeFrame {
    origin: Point,
    text_width: f64,
    padding: f64,
}

/// 그리는 동안 함께 쓰는 값
struct TimelineCanvas<'a> {
    fonts: &'a FontBook,
    colors: TimelineColors,
    scene: Scene,
    bounds: Bounds,
}

/// 미리 잰 높이들(timelineRenderer `draw`의 첫 두 반복)
#[derive(Clone, Copy, Debug)]
struct RowHeights {
    section: f64,
    period: f64,
    event_column: f64,
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let timeline = parser::parse(definition)?;
    if timeline.is_vertical {
        return Ok(vertical::render(&timeline, context));
    }
    let fonts = context.fonts;
    let heights = measure_rows(fonts, &timeline.sections, &timeline.periods);
    let mut canvas = TimelineCanvas {
        fonts,
        colors: TimelineColors::for_theme(context.palette.theme),
        scene: Scene::default(),
        bounds: Bounds::EMPTY,
    };
    let has_sections = !timeline.sections.is_empty();
    if has_sections {
        draw_sections(&mut canvas, &timeline.sections, &timeline.periods, heights);
    } else {
        let origin = Point::new(LEFT_MARGIN + 50.0, TOP);
        draw_periods(&mut canvas, &timeline.periods, (0, true), origin, heights);
    }
    let content_width = canvas.bounds.width();
    if !timeline.title.is_empty() {
        draw_title(
            &mut canvas,
            &timeline.title,
            content_width / 2.0 - LEFT_MARGIN,
        );
    }
    let depth_y = if has_sections {
        heights.section + heights.period + 150.0
    } else {
        heights.period + 100.0
    };
    let axis_end = Point::new(content_width + 3.0 * LEFT_MARGIN, depth_y);
    draw_arrow(&mut canvas, Point::new(LEFT_MARGIN, depth_y), axis_end, 4.0);
    Ok(finish_scene(canvas))
}

/// setupGraphViewbox: 그린 것의 경계에 여백 50을 둘러 viewBox로 삼는다
fn finish_scene(canvas: TimelineCanvas) -> Scene {
    let bounds = canvas.bounds;
    let mut scene = canvas.scene;
    scene.width = bounds.right - bounds.left + 2.0 * VIEW_PADDING;
    scene.height = bounds.bottom - bounds.top + 2.0 * VIEW_PADDING;
    scene.translate(VIEW_PADDING - bounds.left, VIEW_PADDING - bounds.top);
    scene
}

fn measure_rows(fonts: &FontBook, sections: &[String], periods: &[Period]) -> RowHeights {
    let section = sections
        .iter()
        .map(|name| node_content_height(fonts, name, NODE_TEXT_WIDTH, NODE_PADDING) + 20.0)
        .fold(0.0, f64::max);
    let period = if periods.is_empty() {
        0.0
    } else {
        node_content_height(fonts, MEASURED_PERIOD_TEXT, NODE_TEXT_WIDTH, NODE_PADDING) + 20.0
    };
    let event_column = periods
        .iter()
        .map(|period| {
            let heights: f64 = period
                .events
                .iter()
                .map(|event| node_content_height(fonts, event, NODE_TEXT_WIDTH, NODE_PADDING))
                .sum();
            heights + period.events.len().saturating_sub(1) as f64 * EVENT_GAP
        })
        .fold(0.0, f64::max);
    RowHeights {
        section,
        period,
        event_column,
    }
}

/// svgDraw `wrap`: 공백·`<br>`로 나눈 조각(구분자 포함)을 이어 붙이다 폭을 넘으면 새 줄을 연다
fn wrap_lines(fonts: &FontBook, text: &str, width: f64) -> Vec<String> {
    let measure = |line: &str| {
        fonts
            .measure_svg_runs(&[line], FONT_SIZE, FontWeight::Regular)
            .advance
    };
    let mut lines = vec![String::new()];
    let mut words: Vec<String> = Vec::new();
    for word in split_keeping_separators(text) {
        words.push(word.clone());
        let joined = collapse_whitespace(&words.join(" "));
        let is_overflowing = measure(&joined) > width || word == "<br>";
        let current_line = if is_overflowing {
            words.pop();
            collapse_whitespace(&words.join(" "))
        } else {
            joined
        };
        if let Some(last) = lines.last_mut() {
            *last = current_line;
        }
        if !is_overflowing {
            continue;
        }
        words = vec![if word == "<br>" {
            String::new()
        } else {
            word.clone()
        }];
        lines.push(collapse_whitespace(&word));
    }
    lines
}

/// JS `text.split(/(\s+|<br>)/)`
fn split_keeping_separators(text: &str) -> Vec<String> {
    let mut pieces = vec![String::new()];
    let mut rest = text;
    while let Some(character) = rest.chars().next() {
        let separator_length = if rest.starts_with("<br>") {
            4
        } else if character.is_whitespace() {
            rest.len() - rest.trim_start().len()
        } else {
            0
        };
        if separator_length == 0 {
            if let Some(piece) = pieces.last_mut() {
                piece.push(character);
            }
            rest = &rest[character.len_utf8()..];
            continue;
        }
        pieces.push(rest[..separator_length].to_string());
        pieces.push(String::new());
        rest = &rest[separator_length..];
    }
    pieces
}

/// SVG 글자는 이어진 공백을 하나로 줄이고 앞뒤 공백을 지운다
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 글자 묶음 getBBox 높이: 글자가 있는 첫 줄 위부터 마지막 줄 아래까지
fn text_block_height(fonts: &FontBook, lines: &[String]) -> f64 {
    let filled: Vec<usize> = (0..lines.len())
        .filter(|index| !lines[*index].is_empty())
        .collect();
    let (Some(first), Some(last)) = (filled.first(), filled.last()) else {
        return 0.0;
    };
    let metrics = fonts.primary_metrics(FONT_SIZE);
    metrics.ascent + metrics.descent + (last - first) as f64 * LINE_HEIGHT
}

/// 상자 높이(최소 높이 적용 전, getVirtualNodeHeight): 글자 높이 + 반 줄 + 안쪽 여백
fn node_content_height(fonts: &FontBook, text: &str, text_width: f64, padding: f64) -> f64 {
    let lines = wrap_lines(fonts, text, text_width);
    text_block_height(fonts, &lines) + FONT_SIZE * 1.1 * 0.5 + padding
}

/// 상자 하나: 위 모서리만 둥근 바탕, 밑줄, 가운데 글자. 그린 높이를 돌려준다
fn draw_node(
    canvas: &mut TimelineCanvas,
    text: &str,
    frame: NodeFrame,
    min_height: f64,
    colors: SectionColors,
) -> f64 {
    let NodeFrame {
        origin,
        text_width,
        padding,
    } = frame;
    let width = text_width + 2.0 * padding;
    let lines = wrap_lines(canvas.fonts, text, text_width);
    let content = text_block_height(canvas.fonts, &lines) + FONT_SIZE * 1.1 * 0.5 + padding;
    let height = content.max(min_height);
    let background = node_background(origin, width, height);
    canvas.scene.push_path(background, Some(colors.fill), None);
    let bottom = origin.y + height;
    let underline = vec![
        PathCommand::MoveTo(Point::new(origin.x, bottom)),
        PathCommand::LineTo(Point::new(origin.x + width, bottom)),
    ];
    let line_style = stroke(colors.line, 3.0, Vec::new());
    canvas.scene.push_path(underline, None, line_style);
    let center_x = origin.x + width / 2.0;
    let middle_shift = X_HEIGHT_RATIO * FONT_SIZE / 2.0;
    for (index, line) in lines.into_iter().enumerate() {
        let baseline = origin.y + padding / 2.0 + FONT_SIZE + index as f64 * LINE_HEIGHT;
        let item = centered_text(
            canvas.fonts,
            line,
            center_x,
            baseline + middle_shift,
            colors.text,
        );
        canvas.scene.push_text(item);
    }
    canvas
        .bounds
        .include(origin.x, origin.y, origin.x + width, bottom);
    height
}

/// defaultBkg: `M0 h-5 v-(h-10) q0,-5,5,-5 h(w-10) q5,0,5,5 v(h-5) H0 Z`
fn node_background(origin: Point, width: f64, height: f64) -> Vec<PathCommand> {
    let at = |x: f64, y: f64| Point::new(origin.x + x, origin.y + y);
    vec![
        PathCommand::MoveTo(at(0.0, height - CORNER_RADIUS)),
        PathCommand::LineTo(at(0.0, CORNER_RADIUS)),
        PathCommand::QuadTo(at(0.0, 0.0), at(CORNER_RADIUS, 0.0)),
        PathCommand::LineTo(at(width - CORNER_RADIUS, 0.0)),
        PathCommand::QuadTo(at(width, 0.0), at(width, CORNER_RADIUS)),
        PathCommand::LineTo(at(width, height)),
        PathCommand::LineTo(at(0.0, height)),
        PathCommand::Close,
    ]
}

fn stroke(color: Color, width: f64, dash: Vec<f64>) -> Option<StrokeStyle> {
    Some(StrokeStyle { color, width, dash })
}

fn centered_text(
    fonts: &FontBook,
    text: String,
    center_x: f64,
    baseline: f64,
    color: Color,
) -> TextItem {
    let width = fonts.measure_line(&text, FONT_SIZE, FontWeight::Regular);
    TextItem {
        text,
        x: center_x - width / 2.0,
        baseline,
        font_size: FONT_SIZE,
        weight: FontWeight::Regular,
        color,
        rotation: None,
    }
}

/// drawNode의 구역 번호: `번호 % 12 − 1`
fn section_number(color_index: usize) -> i64 {
    (color_index % SECTION_COLOR_COUNT) as i64 - 1
}

fn draw_sections(
    canvas: &mut TimelineCanvas,
    sections: &[String],
    periods: &[Period],
    heights: RowHeights,
) {
    let mut column_x = LEFT_MARGIN + 50.0;
    for (number, section) in sections.iter().enumerate() {
        let section_periods: Vec<Period> = periods
            .iter()
            .filter(|period| &period.section == section)
            .cloned()
            .collect();
        let columns = section_periods.len().max(1) as f64;
        let colors = canvas.colors.section(section_number(number));
        let frame = NodeFrame {
            origin: Point::new(column_x, TOP),
            text_width: COLUMN_STEP * columns - 50.0,
            padding: NODE_PADDING,
        };
        draw_node(canvas, section, frame, heights.section, colors);
        let period_origin = Point::new(column_x, TOP + heights.section + 50.0);
        draw_periods(
            canvas,
            &section_periods,
            (number, false),
            period_origin,
            heights,
        );
        column_x += COLUMN_STEP * columns;
    }
}

/// 기간 상자, 점선 화살표, 사건 상자 순서로 그린다. 구역이 없으면 기간마다 색을 바꾼다
fn draw_periods(
    canvas: &mut TimelineCanvas,
    periods: &[Period],
    (mut color_index, is_without_sections): (usize, bool),
    origin: Point,
    heights: RowHeights,
) {
    let mut period_height = heights.period;
    let mut x = origin.x;
    for period in periods {
        let colors = canvas.colors.section(section_number(color_index));
        let frame = NodeFrame {
            origin: Point::new(x, origin.y),
            text_width: NODE_TEXT_WIDTH,
            padding: NODE_PADDING,
        };
        let height = draw_node(canvas, &period.name, frame, period_height, colors);
        period_height = period_height.max(height);
        let line_top = origin.y + period_height;
        let line_bottom = line_top + 200.0 + heights.event_column;
        let center_x = x + NODE_WIDTH / 2.0;
        draw_dashed_arrow(
            canvas,
            Point::new(center_x, line_top),
            Point::new(center_x, line_bottom),
        );
        let mut event_y = origin.y + 200.0;
        for event in &period.events {
            let frame = NodeFrame {
                origin: Point::new(x, event_y),
                text_width: NODE_TEXT_WIDTH,
                padding: NODE_PADDING,
            };
            let height = draw_node(canvas, event, frame, EVENT_MIN_HEIGHT, colors.brightened());
            event_y += height + EVENT_GAP;
        }
        x += COLUMN_STEP;
        if is_without_sections {
            color_index += 1;
        }
    }
}

fn draw_dashed_arrow(canvas: &mut TimelineCanvas, from: Point, to: Point) {
    let color = canvas.colors.connector;
    let commands = vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)];
    canvas
        .scene
        .push_path(commands, None, stroke(color, 2.0, vec![5.0, 5.0]));
    push_arrow_head(canvas, from, to, 2.0);
    canvas.bounds.include(from.x, from.y, to.x, to.y);
}

fn draw_arrow(canvas: &mut TimelineCanvas, from: Point, to: Point, stroke_width: f64) {
    let color = canvas.colors.connector;
    let commands = vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)];
    canvas
        .scene
        .push_path(commands, None, stroke(color, stroke_width, Vec::new()));
    push_arrow_head(canvas, from, to, stroke_width);
    canvas.bounds.include(from.x, from.y, to.x, to.y);
}

/// 화살촉 표식(`M0,0 V4 L6,2 Z`, 기준점 5,2, 선 방향으로 돌림)을 선 굵기 배율로 키워 끝에 붙인다
fn push_arrow_head(canvas: &mut TimelineCanvas, from: Point, to: Point, scale: f64) {
    let length = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2))
        .sqrt()
        .max(f64::EPSILON);
    let (along_x, along_y) = ((to.x - from.x) / length, (to.y - from.y) / length);
    let at = |along: f64, across: f64| {
        Point::new(
            to.x + scale * (along * along_x - across * along_y),
            to.y + scale * (along * along_y + across * along_x),
        )
    };
    let head = [at(-5.0, -2.0), at(-5.0, 2.0), at(1.0, 0.0)];
    canvas
        .scene
        .push_path(polygon(&head), Some(canvas.colors.title), None);
}

fn draw_title(canvas: &mut TimelineCanvas, title: &str, x: f64) {
    let baseline = 20.0;
    let extent = canvas
        .fonts
        .measure_svg_runs(&[title], TITLE_FONT_SIZE, FontWeight::Bold);
    let metrics = canvas.fonts.primary_metrics(TITLE_FONT_SIZE);
    canvas.bounds.include(
        x + extent.left,
        baseline - metrics.ascent,
        x + extent.right,
        baseline + metrics.descent,
    );
    canvas.scene.push_text(TextItem {
        text: title.to_string(),
        x,
        baseline,
        font_size: TITLE_FONT_SIZE,
        weight: FontWeight::Bold,
        color: canvas.colors.title,
        rotation: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    #[test]
    fn 긴_글자는_폭_150에서_줄을_바꾼다() {
        let fonts = bundled_fonts();
        assert_eq!(
            wrap_lines(&fonts, "2004 ", NODE_TEXT_WIDTH),
            vec!["2004".to_string()]
        );
        let lines = wrap_lines(
            &fonts,
            "A very long event description that wraps",
            NODE_TEXT_WIDTH,
        );
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|line| !line.is_empty()));
    }

    #[test]
    fn 한_줄_상자_높이는_50_8이다() {
        let fonts = bundled_fonts();
        let height = node_content_height(&fonts, "LinkedIn", NODE_TEXT_WIDTH, NODE_PADDING);
        assert!((height - 50.8).abs() < 1e-9);
    }
}
