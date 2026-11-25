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

/// 한 줄 글자. `x`는 줄 왼쪽, `baseline`은 기준선 y다
#[derive(Clone, Debug, PartialEq)]
pub struct TextItem {
    pub text: String,
    pub x: f64,
    pub baseline: f64,
    pub font_size: f64,
    pub weight: FontWeight,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SceneItem {
    Path(PathItem),
    Text(TextItem),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    /// viewBox 너비·높이(px)
    pub width: f64,
    pub height: f64,
    pub items: Vec<SceneItem>,
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
        for item in &mut self.items {
            match item {
                SceneItem::Path(path) => path
                    .commands
                    .iter_mut()
                    .for_each(|command| translate_command(command, dx, dy)),
                SceneItem::Text(text) => {
                    text.x += dx;
                    text.baseline += dy;
                }
            }
        }
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
