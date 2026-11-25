//! 렌더러에 묶이지 않은 벡터 장면.
//!
//! 좌표는 Mermaid SVG의 viewBox 좌표(px)이며 원점은 viewBox 왼쪽 위다. GPUI는 이 장면을 `paint_path`와 글자
//! 그리기로 옮기고, 모바일 이미지는 [`crate::raster`]가 tiny-skia로 그린다. 그리는 순서는 `items` 순서다.

use crate::text::FontWeight;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    /// 0.0..=1.0
    pub alpha: f32,
}

impl Color {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 1.0,
        }
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// `#rgb`·`#rrggbb`·`rgb()`·`rgba()`와 몇 가지 이름 색을 읽는다
    pub fn parse_css(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        if let Some(hex) = value.strip_prefix('#') {
            return parse_hex(hex);
        }
        if let Some(arguments) = value
            .strip_prefix("rgba(")
            .or_else(|| value.strip_prefix("rgb("))
        {
            return parse_rgb_function(arguments.trim_end_matches(')'));
        }
        named_color(&value)
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    let digit =
        |index: usize, width: usize| u8::from_str_radix(hex.get(index..index + width)?, 16).ok();
    match hex.len() {
        3 => Some(Color::rgb(
            digit(0, 1)? * 17,
            digit(1, 1)? * 17,
            digit(2, 1)? * 17,
        )),
        6 => Some(Color::rgb(digit(0, 2)?, digit(2, 2)?, digit(4, 2)?)),
        8 => Some(Color::rgba(
            digit(0, 2)?,
            digit(2, 2)?,
            digit(4, 2)?,
            f32::from(digit(6, 2)?) / 255.0,
        )),
        _ => None,
    }
}

fn parse_rgb_function(arguments: &str) -> Option<Color> {
    let parts: Vec<f32> = arguments
        .split(',')
        .map(|part| part.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .ok()?;
    let channel = |value: f32| value.round().clamp(0.0, 255.0) as u8;
    match parts.as_slice() {
        [red, green, blue] => Some(Color::rgb(channel(*red), channel(*green), channel(*blue))),
        [red, green, blue, alpha] => Some(Color::rgba(
            channel(*red),
            channel(*green),
            channel(*blue),
            *alpha,
        )),
        _ => None,
    }
}

fn named_color(name: &str) -> Option<Color> {
    let color = match name {
        "black" => Color::rgb(0, 0, 0),
        "white" => Color::rgb(255, 255, 255),
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "blue" => Color::rgb(0, 0, 255),
        "yellow" => Color::rgb(255, 255, 0),
        "orange" => Color::rgb(255, 165, 0),
        "purple" => Color::rgb(128, 0, 128),
        "gray" | "grey" => Color::rgb(128, 128, 128),
        "lightgrey" | "lightgray" => Color::rgb(211, 211, 211),
        "transparent" | "none" => Color::rgba(0, 0, 0, 0.0),
        _ => return None,
    };
    Some(color)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    pub color: Color,
    pub width: f64,
    /// 비어 있으면 실선
    pub dash: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathItem {
    pub commands: Vec<PathCommand>,
    pub fill: Option<Color>,
    pub stroke: Option<StrokeStyle>,
}

/// 글자를 `center` 둘레로 `degrees`(시계 방향, SVG `rotate()`와 같은 부호)만큼 돌린다
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    pub degrees: f64,
    pub center: Point,
}

/// 한 줄 글자. `x`는 줄 왼쪽, `baseline`은 기준선 y다
#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    pub text: String,
    pub x: f64,
    pub baseline: f64,
    pub font_size: f64,
    pub weight: FontWeight,
    pub color: Color,
    /// 돌리지 않으면 `None`
    pub rotation: Option<Rotation>,
}

/// 사각형 밖을 잘라 내고 그리는 묶음(크기를 줄인 foreignObject의 `overflow: hidden` 등)
#[derive(Clone, Debug, PartialEq)]
pub struct ClipItem {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub items: Vec<SceneItem>,
}

/// 가로 선형 그라디언트로 칠한 굵은 선을 SVG `mix-blend-mode: multiply`처럼 아래 그림과 곱해 섞는다
/// (sankey 링크). 그라디언트는 `start_x`에서 `end_x`까지 이어지고 그 밖은 끝 색으로 채운다
#[derive(Clone, Debug, PartialEq)]
pub struct MultiplyStrokeItem {
    pub commands: Vec<PathCommand>,
    pub width: f64,
    pub start_x: f64,
    pub end_x: f64,
    /// 알파에는 `stroke-opacity`가 들어 있다
    pub start_color: Color,
    pub end_color: Color,
}

/// 글자 윤곽선을 따라 긋는 선(SVG `<text>`의 `stroke`). 칠은 같은 자리의 [`TextItem`]이 따로 맡으므로,
/// `paint-order: stroke`면 이 항목을 칠보다 먼저, 기본 순서면 나중에 넣는다
#[derive(Clone, Debug, PartialEq)]
pub struct TextStrokeItem {
    pub text: TextItem,
    pub color: Color,
    pub width: f64,
    /// `stroke-linejoin: round`이면 참(아니면 miter)
    pub is_round_join: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SceneItem {
    Path(PathItem),
    Text(TextItem),
    Clip(ClipItem),
    MultiplyStroke(MultiplyStrokeItem),
    TextStroke(TextStrokeItem),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// viewBox 너비·높이(px)
    pub width: f64,
    pub height: f64,
    pub items: Vec<SceneItem>,
    /// SVG `preserveAspectRatio`가 `xMinYMin`이면 참(journey). 아니면 `xMidYMid`로 가운데 둔다
    pub is_start_aligned: bool,
}

impl Scene {
    pub fn push_path(
        &mut self,
        commands: Vec<PathCommand>,
        fill: Option<Color>,
        stroke: Option<StrokeStyle>,
    ) {
        self.items.push(SceneItem::Path(PathItem {
            commands,
            fill,
            stroke,
        }));
    }

    pub fn push_text(&mut self, text: TextItem) {
        self.items.push(SceneItem::Text(text));
    }

    /// 모든 좌표를 옮긴다(viewBox 원점을 0,0으로 맞출 때)
    pub fn translate(&mut self, dx: f64, dy: f64) {
        translate_items(&mut self.items, dx, dy);
    }
}

fn translate_items(items: &mut [SceneItem], dx: f64, dy: f64) {
    for item in items {
        match item {
            SceneItem::Path(path) => path
                .commands
                .iter_mut()
                .for_each(|command| translate_command(command, dx, dy)),
            SceneItem::Text(text) => translate_text(text, dx, dy),
            SceneItem::TextStroke(stroke) => translate_text(&mut stroke.text, dx, dy),
            SceneItem::Clip(clip) => {
                clip.x += dx;
                clip.y += dy;
                translate_items(&mut clip.items, dx, dy);
            }
            SceneItem::MultiplyStroke(stroke) => {
                stroke
                    .commands
                    .iter_mut()
                    .for_each(|command| translate_command(command, dx, dy));
                stroke.start_x += dx;
                stroke.end_x += dx;
            }
        }
    }
}

fn translate_text(text: &mut TextItem, dx: f64, dy: f64) {
    text.x += dx;
    text.baseline += dy;
    if let Some(rotation) = &mut text.rotation {
        rotation.center.x += dx;
        rotation.center.y += dy;
    }
}

fn translate_command(command: &mut PathCommand, dx: f64, dy: f64) {
    let shift = |point: &mut Point| {
        point.x += dx;
        point.y += dy;
    };
    match command {
        PathCommand::MoveTo(point) | PathCommand::LineTo(point) => shift(point),
        PathCommand::QuadTo(control, point) => {
            shift(control);
            shift(point);
        }
        PathCommand::CubicTo(first, second, point) => {
            shift(first);
            shift(second);
            shift(point);
        }
        PathCommand::Close => {}
    }
}

/// 다각형 경로
pub fn polygon(points: &[Point]) -> Vec<PathCommand> {
    let mut commands = Vec::with_capacity(points.len() + 1);
    for (index, point) in points.iter().enumerate() {
        commands.push(if index == 0 {
            PathCommand::MoveTo(*point)
        } else {
            PathCommand::LineTo(*point)
        });
    }
    commands.push(PathCommand::Close);
    commands
}

/// 모서리를 둥글린 사각형(SVG rect의 rx·ry)
pub fn rounded_rect(x: f64, y: f64, width: f64, height: f64, radius: f64) -> Vec<PathCommand> {
    if radius <= 0.0 {
        let corners = [
            Point::new(x, y),
            Point::new(x + width, y),
            Point::new(x + width, y + height),
            Point::new(x, y + height),
        ];
        return polygon(&corners);
    }
    let radius = radius.min(width / 2.0).min(height / 2.0);
    // 원호를 3차 베지어로 근사하는 상수
    let k = radius * 0.552_284_749_8;
    let (right, bottom) = (x + width, y + height);
    vec![
        PathCommand::MoveTo(Point::new(x + radius, y)),
        PathCommand::LineTo(Point::new(right - radius, y)),
        PathCommand::CubicTo(
            Point::new(right - radius + k, y),
            Point::new(right, y + radius - k),
            Point::new(right, y + radius),
        ),
        PathCommand::LineTo(Point::new(right, bottom - radius)),
        PathCommand::CubicTo(
            Point::new(right, bottom - radius + k),
            Point::new(right - radius + k, bottom),
            Point::new(right - radius, bottom),
        ),
        PathCommand::LineTo(Point::new(x + radius, bottom)),
        PathCommand::CubicTo(
            Point::new(x + radius - k, bottom),
            Point::new(x, bottom - radius + k),
            Point::new(x, bottom - radius),
        ),
        PathCommand::LineTo(Point::new(x, y + radius)),
        PathCommand::CubicTo(
            Point::new(x, y + radius - k),
            Point::new(x + radius - k, y),
            Point::new(x + radius, y),
        ),
        PathCommand::Close,
    ]
}

/// 타원(원)
pub fn ellipse(center: Point, radius_x: f64, radius_y: f64) -> Vec<PathCommand> {
    let kx = radius_x * 0.552_284_749_8;
    let ky = radius_y * 0.552_284_749_8;
    let (cx, cy) = (center.x, center.y);
    vec![
        PathCommand::MoveTo(Point::new(cx + radius_x, cy)),
        PathCommand::CubicTo(
            Point::new(cx + radius_x, cy + ky),
            Point::new(cx + kx, cy + radius_y),
            Point::new(cx, cy + radius_y),
        ),
        PathCommand::CubicTo(
            Point::new(cx - kx, cy + radius_y),
            Point::new(cx - radius_x, cy + ky),
            Point::new(cx - radius_x, cy),
        ),
        PathCommand::CubicTo(
            Point::new(cx - radius_x, cy - ky),
            Point::new(cx - kx, cy - radius_y),
            Point::new(cx, cy - radius_y),
        ),
        PathCommand::CubicTo(
            Point::new(cx + kx, cy - radius_y),
            Point::new(cx + radius_x, cy - ky),
            Point::new(cx + radius_x, cy),
        ),
        PathCommand::Close,
    ]
}
