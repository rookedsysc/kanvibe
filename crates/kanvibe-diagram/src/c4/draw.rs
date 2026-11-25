//! c4 svgDraw.js: 모양·경계·관계·사람 그림을 장면에 옮긴다.
//!
//! 글자는 모두 `"Open Sans", sans-serif`라 기준 환경에서 Noto Sans로 풀린다. `«종류»`·기술(`[기술]`)·관계 기술은
//! `font-style: italic`이라 번들의 Noto Sans Italic으로 그린다. 폭은 원본처럼 바로 선 글자로 잰다.

use crate::scene::{
    Color, PathCommand, Point, Scene, StrokeStyle, TextItem, ellipse, polygon, rounded_rect,
};
use crate::text::{FontBook, FontWeight};

use super::layout::{
    BoundaryBox, LABEL_FONT_SIZE, SMALL_FONT_SIZE, ShapeBox, TEXT_FONT_SIZE, TextBlock,
};
use super::parser::ShapeStyle;

/// 관계선·관계 글자·경계 기본색(`#444444`)
pub(crate) const RELATION_COLOR: Color = Color::rgb(0x44, 0x44, 0x44);
const SHAPE_TEXT_COLOR: Color = Color::rgb(0xFF, 0xFF, 0xFF);
/// `dominant-baseline: middle` 글자에 tspan의 `alignment-baseline: mathematical`이 겹친 결과 기준선이 `y`보다
/// 이만큼(em) 아래에 온다. 기준 PNG의 12·14·16px 글자 위치에 맞춰 잰 값이다
const MIDDLE_BASELINE_EM: f64 = 0.55;

/// c4 설정 `<종류>_bg_color`·`<종류>_border_color`
fn shape_colors(kind: &str) -> (Color, Color) {
    let base = kind.trim_end_matches("_db").trim_end_matches("_queue");
    let (fill, border) = match base {
        "person" => (0x08427B, 0x073B6F),
        "external_person" => (0x686868, 0x8A8A8A),
        "system" => (0x1168BD, 0x3C7FC0),
        "external_system" => (0x999999, 0x8A8A8A),
        "container" => (0x438DD5, 0x3C7FC0),
        "external_container" => (0xB3B3B3, 0xA6A6A6),
        "component" => (0x85BBF0, 0x78A8D8),
        _ => (0xCCCCCC, 0xBFBFBF),
    };
    (hex_color(fill), hex_color(border))
}

/// `UpdateElementStyle`·`UpdateRelStyle`로 바꾼 색. 없거나 읽지 못하면 기본색이다
pub(crate) fn style_color(value: Option<&str>, default: Color) -> Color {
    value.and_then(Color::parse_css).unwrap_or(default)
}

/// 모양 칠·테두리·글자색. 사람 그림(PNG)은 바꾼 칠과 상관없이 종류의 기본 칠을 바탕으로 쓴다
fn styled_shape_colors(kind: &str, style: &ShapeStyle) -> (Color, Color, Color) {
    let (fill, border) = shape_colors(kind);
    (
        style_color(style.background.as_deref(), fill),
        style_color(style.border.as_deref(), border),
        style_color(style.font.as_deref(), SHAPE_TEXT_COLOR),
    )
}

fn hex_color(value: u32) -> Color {
    let [_, red, green, blue] = value.to_be_bytes();
    Color::rgb(red, green, blue)
}

/// byTspan: `<br>`로 나눈 줄마다 가운데 맞춘 `<text>`. 줄 사이는 글자 크기만큼이고 묶음 가운데가 `y`다
pub(crate) fn middle_text(
    fonts: &FontBook,
    text: &str,
    center_x: f64,
    y: f64,
    font_size: f64,
    weight: FontWeight,
    color: Color,
) -> Vec<TextItem> {
    let lines = super::layout::split_lines(text);
    let count = lines.len() as f64;
    lines
        .into_iter()
        .enumerate()
        .filter(|(_, line)| !line.is_empty())
        .map(|(index, line)| {
            let dy = index as f64 * font_size - font_size * (count - 1.0) / 2.0;
            let advance = fonts.measure_svg_runs(&[&line], font_size, weight).advance;
            TextItem {
                x: center_x - advance / 2.0,
                baseline: y + dy + MIDDLE_BASELINE_EM * font_size,
                text: line,
                font_size,
                weight,
                color,
                rotation: None,
            }
        })
        .collect()
}

/// 상대 좌표 경로를 절대 좌표 명령으로 쌓는다
struct PathPen {
    current: Point,
    commands: Vec<PathCommand>,
}

impl PathPen {
    fn new(start: Point) -> Self {
        Self {
            current: start,
            commands: vec![PathCommand::MoveTo(start)],
        }
    }

    fn line(&mut self, dx: f64, dy: f64) {
        self.current = Point::new(self.current.x + dx, self.current.y + dy);
        self.commands.push(PathCommand::LineTo(self.current));
    }

    fn curve(&mut self, first: (f64, f64), second: (f64, f64), end: (f64, f64)) {
        let origin = self.current;
        let at = |(dx, dy): (f64, f64)| Point::new(origin.x + dx, origin.y + dy);
        self.current = at(end);
        self.commands
            .push(PathCommand::CubicTo(at(first), at(second), self.current));
    }
}

pub(crate) fn draw_shape(scene: &mut Scene, fonts: &FontBook, shape: &ShapeBox) {
    let (fill, border, text_color) = styled_shape_colors(&shape.kind, &shape.style);
    let stroke = Some(StrokeStyle {
        color: border,
        width: 0.5,
        dash: Vec::new(),
    });
    for (commands, is_filled) in shape_body(shape) {
        scene.push_path(commands, is_filled.then_some(fill), stroke.clone());
    }
    let center_x = shape.x + shape.width / 2.0;
    let type_text = format!("<<{}>>", shape.kind);
    let type_left = center_x - shape.type_width / 2.0;
    let type_baseline = shape.y + 20.0;
    let type_line = SpacedLine {
        left: type_left,
        baseline: type_baseline,
        text_length: shape.type_width,
        color: text_color,
    };
    spaced_text(fonts, &type_text, &type_line)
        .into_iter()
        .for_each(|text| scene.push_text(text));
    if let Some(image_y) = shape.image_y {
        let (icon_background, _) = shape_colors(&shape.kind);
        let origin = Point::new(center_x - 24.0, shape.y + image_y);
        draw_person_icon(scene, origin, icon_background);
    }
    let blocks = [
        (&shape.label, LABEL_FONT_SIZE, FontWeight::Bold),
        (&shape.techn, TEXT_FONT_SIZE, FontWeight::Italic),
        (&shape.descr, TEXT_FONT_SIZE, FontWeight::Regular),
    ];
    for (block, font_size, weight) in blocks {
        draw_block(
            scene,
            fonts,
            block,
            (center_x, shape.y),
            font_size,
            weight,
            text_color,
        );
    }
}

fn draw_block(
    scene: &mut Scene,
    fonts: &FontBook,
    block: &TextBlock,
    (center_x, top): (f64, f64),
    font_size: f64,
    weight: FontWeight,
    color: Color,
) {
    if block.is_empty() {
        return;
    }
    let y = top + block.offset_y;
    middle_text(fonts, &block.text, center_x, y, font_size, weight, color)
        .into_iter()
        .for_each(|text| scene.push_text(text));
}

/// 사각형·원통(db)·관(queue) 몸통. 둘째 값이 false면 채우지 않는 덧선이다
fn shape_body(shape: &ShapeBox) -> Vec<(Vec<PathCommand>, bool)> {
    let (x, y, width, height) = (shape.x, shape.y, shape.width, shape.height);
    let half = width / 2.0;
    if shape.kind.ends_with("_db") {
        let mut body = PathPen::new(Point::new(x, y));
        body.curve((0.0, -10.0), (half, -10.0), (half, -10.0));
        body.curve((0.0, 0.0), (half, 0.0), (half, 10.0));
        body.line(0.0, height);
        body.curve((0.0, 10.0), (-half, 10.0), (-half, 10.0));
        body.curve((0.0, 0.0), (-half, 0.0), (-half, -10.0));
        body.line(0.0, -height);
        let mut rim = PathPen::new(Point::new(x, y));
        rim.curve((0.0, 10.0), (half, 10.0), (half, 10.0));
        rim.curve((0.0, 0.0), (half, 0.0), (half, -10.0));
        return vec![(body.commands, true), (rim.commands, false)];
    }
    if shape.kind.ends_with("_queue") {
        let half = height / 2.0;
        let mut body = PathPen::new(Point::new(x, y));
        body.line(width, 0.0);
        body.curve((5.0, 0.0), (5.0, half), (5.0, half));
        body.curve((0.0, 0.0), (0.0, half), (-5.0, half));
        body.line(-width, 0.0);
        body.curve((-5.0, 0.0), (-5.0, -half), (-5.0, -half));
        body.curve((0.0, 0.0), (0.0, -half), (5.0, -half));
        let mut rim = PathPen::new(Point::new(x + width, y));
        rim.curve((-5.0, 0.0), (-5.0, half), (-5.0, half));
        rim.curve((0.0, half), (5.0, half), (5.0, half));
        return vec![(body.commands, true), (rim.commands, false)];
    }
    vec![(rounded_rect(x, y, width, height, 2.5), true)]
}

/// `textLength`로 폭을 맞추는 기울인 `«종류»` 한 줄
struct SpacedLine {
    left: f64,
    baseline: f64,
    text_length: f64,
    color: Color,
}

/// `lengthAdjust="spacing"`·`textLength`: 글자 사이를 고르게 줄이거나 늘려 첫 글자부터 마지막 글자 끝까지를
/// `text_length`에 맞춘다(SVG 2 글자 배치, 간격은 글자 수 − 1로 나눈다)
fn spaced_text(fonts: &FontBook, text: &str, spaced: &SpacedLine) -> Vec<TextItem> {
    let line = fonts.shape_line(text, SMALL_FONT_SIZE, FontWeight::Italic);
    let characters: Vec<char> = text.chars().collect();
    let item = |text: String, x: f64| TextItem {
        text,
        x,
        baseline: spaced.baseline,
        font_size: SMALL_FONT_SIZE,
        weight: FontWeight::Italic,
        color: spaced.color,
        rotation: None,
    };
    if characters.len() < 2 || line.glyphs.len() != characters.len() {
        return vec![item(text.to_string(), spaced.left)];
    }
    let shift = (spaced.text_length - line.width) / (characters.len() - 1) as f64;
    characters
        .iter()
        .zip(&line.glyphs)
        .enumerate()
        .map(|(index, (character, glyph))| {
            let x = spaced.left + glyph.x + shift * index as f64;
            item(character.to_string(), x)
        })
        .collect()
}

/// 48×48 사람 그림(PNG)을 머리 원과 어깨 모양 몸통으로 그린다
fn draw_person_icon(scene: &mut Scene, origin: Point, background: Color) {
    let white = Some(Color::rgb(255, 255, 255));
    let at = |x: f64, y: f64| Point::new(origin.x + x, origin.y + y);
    scene.push_path(
        rounded_rect(origin.x, origin.y, 48.0, 48.0, 0.0),
        Some(background),
        None,
    );
    scene.push_path(ellipse(at(23.5, 12.5), 10.5, 10.5), white, None);
    let (left, right, top, bottom) = (5.0, 42.0, 27.0, 45.0);
    let (upper, lower) = (11.0, 3.0);
    let k = 0.552_284_749_8;
    let commands = vec![
        PathCommand::MoveTo(at(left, top + upper)),
        PathCommand::CubicTo(
            at(left, top + upper * (1.0 - k)),
            at(left + upper * (1.0 - k), top),
            at(left + upper, top),
        ),
        PathCommand::LineTo(at(right - upper, top)),
        PathCommand::CubicTo(
            at(right - upper * (1.0 - k), top),
            at(right, top + upper * (1.0 - k)),
            at(right, top + upper),
        ),
        PathCommand::LineTo(at(right, bottom - lower)),
        PathCommand::CubicTo(
            at(right, bottom - lower * (1.0 - k)),
            at(right - lower * (1.0 - k), bottom),
            at(right - lower, bottom),
        ),
        PathCommand::LineTo(at(left + lower, bottom)),
        PathCommand::CubicTo(
            at(left + lower * (1.0 - k), bottom),
            at(left, bottom - lower * (1.0 - k)),
            at(left, bottom - lower),
        ),
        PathCommand::Close,
    ];
    scene.push_path(commands, white, None);
}

/// drawBoundary: 점선(배포 노드는 실선) 테두리와 이름·종류·설명. 칠한 경계는 먼저 그린 안쪽 모양을 덮는다
pub(crate) fn draw_boundary(scene: &mut Scene, fonts: &FontBook, boundary: &BoundaryBox) {
    let dash = if boundary.is_deployment_node {
        Vec::new()
    } else {
        vec![7.0, 7.0]
    };
    let style = &boundary.style;
    let stroke = StrokeStyle {
        color: style_color(style.border.as_deref(), RELATION_COLOR),
        width: 1.0,
        dash,
    };
    let fill = style.background.as_deref().and_then(Color::parse_css);
    let outline = rounded_rect(boundary.x, boundary.y, boundary.width, boundary.height, 2.5);
    scene.push_path(outline, fill, Some(stroke));
    let anchor = (boundary.x + boundary.width / 2.0, boundary.y);
    let blocks = [
        (&boundary.label, LABEL_FONT_SIZE, FontWeight::Bold),
        (&boundary.kind, TEXT_FONT_SIZE, FontWeight::Regular),
        (&boundary.descr, SMALL_FONT_SIZE, FontWeight::Regular),
    ];
    for (block, font_size, weight) in blocks {
        draw_block(
            scene,
            fonts,
            block,
            anchor,
            font_size,
            weight,
            RELATION_COLOR,
        );
    }
}

/// 화살촉 표식(markerUnits userSpaceOnUse, orient auto): 다각형을 `reference`가 `point`에 오도록 `angle`만큼 돌린다
pub(crate) fn push_marker(
    scene: &mut Scene,
    shape: &[(f64, f64)],
    reference: (f64, f64),
    point: Point,
    angle: f64,
    color: Color,
) {
    let (sin, cos) = angle.sin_cos();
    let corners: Vec<Point> = shape
        .iter()
        .map(|&(u, v)| {
            let (dx, dy) = (u - reference.0, v - reference.1);
            Point::new(point.x + dx * cos - dy * sin, point.y + dx * sin + dy * cos)
        })
        .collect();
    scene.push_path(polygon(&corners), Some(color), None);
}
