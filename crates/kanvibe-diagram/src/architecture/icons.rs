//! architectureIcons.ts의 내장 아이콘(80×80 viewBox)과 이름을 못 찾았을 때 쓰는 `unknownIcon`.
//!
//! 앱은 다른 아이콘 묶음을 등록하지 않으므로 `logos:aws` 같은 iconify 이름은 모두 물음표 아이콘이 된다. 아이콘은
//! 파란 바탕(#087ebf) 위에 흰 선(대부분 굵기 2, `stroke-miterlimit: 10`)으로 그린다.

use crate::scene::{
    Color, PathCommand, Point, Scene, StrokeStyle, TextItem, ellipse, polygon, rounded_rect,
};
use crate::text::FontWeight;

const VIEW_SIZE: f64 = 80.0;
const BACKGROUND: Color = Color::rgb(0x08, 0x7E, 0xBF);
const WHITE: Color = Color::rgb(0xFF, 0xFF, 0xFF);

/// 아이콘 안 도형 하나. 좌표는 80×80 viewBox 기준
#[derive(Clone, Copy, Debug)]
enum Shape {
    Path(&'static str),
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
    },
    Ellipse {
        cx: f64,
        cy: f64,
        rx: f64,
        ry: f64,
    },
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
}

/// 흰 선 굵기(`None`이면 선 없음)와 흰 채우기 여부
#[derive(Clone, Copy, Debug)]
struct Mark {
    shape: Shape,
    stroke_width: Option<f64>,
    is_filled: bool,
}

const fn outlined(shape: Shape) -> Mark {
    Mark {
        shape,
        stroke_width: Some(2.0),
        is_filled: false,
    }
}

const DATABASE: &[Mark] = &[
    outlined(Shape::Path(
        "m20,57.86c0,3.94,8.95,7.14,20,7.14s20-3.2,20-7.14",
    )),
    outlined(Shape::Path(
        "m20,45.95c0,3.94,8.95,7.14,20,7.14s20-3.2,20-7.14",
    )),
    outlined(Shape::Path(
        "m20,34.05c0,3.94,8.95,7.14,20,7.14s20-3.2,20-7.14",
    )),
    outlined(Shape::Ellipse {
        cx: 40.0,
        cy: 22.14,
        rx: 20.0,
        ry: 7.14,
    }),
    outlined(Shape::Line {
        x1: 20.0,
        y1: 57.86,
        x2: 20.0,
        y2: 22.14,
    }),
    outlined(Shape::Line {
        x1: 60.0,
        y1: 57.86,
        x2: 60.0,
        y2: 22.14,
    }),
];

const SERVER_BAR: &str =
    "m56.25,{y}c0,.27-.45.5-1,.5h-10.5c-.55,0-1-.23-1-.5s.45-.5,1-.5h10.5c.55,0,1,.23,1,.5Z";

const SERVER_FRAME: &[Mark] = &[
    outlined(Shape::Rect {
        x: 17.5,
        y: 17.5,
        width: 45.0,
        height: 45.0,
        radius: 2.0,
    }),
    outlined(Shape::Line {
        x1: 17.5,
        y1: 32.5,
        x2: 62.5,
        y2: 32.5,
    }),
    outlined(Shape::Line {
        x1: 17.5,
        y1: 47.5,
        x2: 62.5,
        y2: 47.5,
    }),
];

const DISK: &[Mark] = &[
    outlined(Shape::Rect {
        x: 20.0,
        y: 15.0,
        width: 40.0,
        height: 50.0,
        radius: 1.0,
    }),
    outlined(Shape::Ellipse {
        cx: 24.0,
        cy: 19.17,
        rx: 0.8,
        ry: 0.83,
    }),
    outlined(Shape::Ellipse {
        cx: 56.0,
        cy: 19.17,
        rx: 0.8,
        ry: 0.83,
    }),
    outlined(Shape::Ellipse {
        cx: 24.0,
        cy: 60.83,
        rx: 0.8,
        ry: 0.83,
    }),
    outlined(Shape::Ellipse {
        cx: 56.0,
        cy: 60.83,
        rx: 0.8,
        ry: 0.83,
    }),
    outlined(Shape::Ellipse {
        cx: 40.0,
        cy: 33.75,
        rx: 14.0,
        ry: 14.58,
    }),
    Mark {
        shape: Shape::Ellipse {
            cx: 40.0,
            cy: 33.75,
            rx: 4.0,
            ry: 4.17,
        },
        stroke_width: Some(2.0),
        is_filled: true,
    },
    Mark {
        shape: Shape::Path(
            "m37.51,42.52l-4.83,13.22c-.26.71-1.1,1.02-1.76.64l-4.18-2.42c-.66-.38-.81-1.26-.33-1.84l9.01-10.8c.88-1.05,2.56-.08,2.09,1.2Z",
        ),
        stroke_width: None,
        is_filled: true,
    },
];

const INTERNET: &[Mark] = &[
    outlined(Shape::Ellipse {
        cx: 40.0,
        cy: 40.0,
        rx: 22.5,
        ry: 22.5,
    }),
    outlined(Shape::Line {
        x1: 40.0,
        y1: 17.5,
        x2: 40.0,
        y2: 62.5,
    }),
    outlined(Shape::Line {
        x1: 17.5,
        y1: 40.0,
        x2: 62.5,
        y2: 40.0,
    }),
    outlined(Shape::Path("m39.99,17.51c-15.28,11.1-15.28,33.88,0,44.98")),
    outlined(Shape::Path("m40.01,17.51c15.28,11.1,15.28,33.88,0,44.98")),
    outlined(Shape::Line {
        x1: 19.75,
        y1: 30.1,
        x2: 60.25,
        y2: 30.1,
    }),
    outlined(Shape::Line {
        x1: 19.75,
        y1: 49.9,
        x2: 60.25,
        y2: 49.9,
    }),
];

const CLOUD: &[Mark] = &[outlined(Shape::Path(
    "m65,47.5c0,2.76-2.24,5-5,5H20c-2.76,0-5-2.24-5-5,0-1.87,1.03-3.51,2.56-4.36-.04-.21-.06-.42-.06-.64,0-2.6,2.48-4.74,5.65-4.97,1.65-4.51,6.34-7.76,11.85-7.76.86,0,1.69.08,2.5.23,2.09-1.57,4.69-2.5,7.5-2.5,6.1,0,11.19,4.38,12.28,10.17,2.14.56,3.72,2.51,3.72,4.83,0,.03,0,.07-.01.1,2.29.46,4.01,2.48,4.01,4.9Z",
))];

/// getIconSVG가 고르는 아이콘
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Database,
    Server,
    Disk,
    Internet,
    Cloud,
    /// 빈 파란 사각형(`iconText` 서비스 바탕)
    Blank,
    /// 이름을 못 찾았을 때의 물음표
    Unknown,
}

impl Icon {
    /// 접두어가 없거나 `mermaid-architecture:`인 이름만 내장 묶음에서 찾는다
    pub fn named(name: &str) -> Self {
        let local = name.strip_prefix("mermaid-architecture:").unwrap_or(name);
        match local {
            "database" => Self::Database,
            "server" => Self::Server,
            "disk" => Self::Disk,
            "internet" => Self::Internet,
            "cloud" => Self::Cloud,
            "blank" => Self::Blank,
            _ => Self::Unknown,
        }
    }

    fn marks(self) -> &'static [Mark] {
        match self {
            Self::Database => DATABASE,
            Self::Server => SERVER_FRAME,
            Self::Disk => DISK,
            Self::Internet => INTERNET,
            Self::Cloud => CLOUD,
            Self::Blank | Self::Unknown => &[],
        }
    }
}

/// 아이콘 하나를 (left, top)에 `size`×`size`로 그린다
pub(crate) fn draw_icon(scene: &mut Scene, icon: Icon, left: f64, top: f64, size: f64) {
    let scale = size / VIEW_SIZE;
    let place = |x: f64, y: f64| Point::new(left + x * scale, top + y * scale);
    scene.push_path(
        polygon(&[
            place(0.0, 0.0),
            place(80.0, 0.0),
            place(80.0, 80.0),
            place(0.0, 80.0),
        ]),
        Some(BACKGROUND),
        None,
    );
    for mark in icon.marks() {
        draw_mark(scene, mark, &place, scale);
    }
    if icon == Icon::Server {
        draw_server_details(scene, &place, scale);
    }
    if icon == Icon::Unknown {
        scene.push_text(TextItem {
            text: "?".to_string(),
            x: left + 21.16 * scale,
            baseline: top + 64.67 * scale,
            font_size: 67.75 * scale,
            weight: FontWeight::Regular,
            color: WHITE,
            rotation: None,
        });
    }
}

fn draw_mark(scene: &mut Scene, mark: &Mark, place: &dyn Fn(f64, f64) -> Point, scale: f64) {
    let commands = shape_commands(mark.shape, place, scale);
    let stroke = mark.stroke_width.map(|width| StrokeStyle {
        color: WHITE,
        width: width * scale,
        dash: Vec::new(),
    });
    scene.push_path(commands, mark.is_filled.then_some(WHITE), stroke);
}

/// 서버 아이콘의 세 칸 막대(채움 + 굵기 1 테두리)와 칸마다 점 세 개(반지름 .75, 굵기 1 테두리)
fn draw_server_details(scene: &mut Scene, place: &dyn Fn(f64, f64) -> Point, scale: f64) {
    let thin = StrokeStyle {
        color: WHITE,
        width: scale,
        dash: Vec::new(),
    };
    for row in ["25", "40", "55"] {
        let bar = SERVER_BAR.replace("{y}", row);
        let commands = path_commands(&bar, place);
        scene.push_path(commands.clone(), Some(WHITE), None);
        scene.push_path(commands, None, Some(thin.clone()));
    }
    for row in [25.0, 40.0, 55.0] {
        for column in [32.5, 27.5, 22.5] {
            let center = place(column, row);
            let radius = 0.75 * scale;
            scene.push_path(
                ellipse(center, radius, radius),
                Some(WHITE),
                Some(thin.clone()),
            );
        }
    }
}

fn shape_commands(shape: Shape, place: &dyn Fn(f64, f64) -> Point, scale: f64) -> Vec<PathCommand> {
    match shape {
        Shape::Path(data) => path_commands(data, place),
        Shape::Rect {
            x,
            y,
            width,
            height,
            radius,
        } => {
            let corner = place(x, y);
            rounded_rect(
                corner.x,
                corner.y,
                width * scale,
                height * scale,
                radius * scale,
            )
        }
        Shape::Ellipse { cx, cy, rx, ry } => ellipse(place(cx, cy), rx * scale, ry * scale),
        Shape::Line { x1, y1, x2, y2 } => {
            vec![
                PathCommand::MoveTo(place(x1, y1)),
                PathCommand::LineTo(place(x2, y2)),
            ]
        }
    }
}

/// SVG 경로 글자에서 숫자를 읽는다(`-`·`.`이 새 숫자를 시작하는 압축 표기 포함)
fn path_numbers(text: &str) -> Vec<f64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, numbers: &mut Vec<f64>| {
        if let Ok(value) = current.parse::<f64>() {
            numbers.push(value);
        }
        current.clear();
    };
    for character in text.chars() {
        let starts_new = character == '-' || (character == '.' && current.contains('.'));
        if character == ',' || character.is_whitespace() || starts_new {
            flush(&mut current, &mut numbers);
        }
        if character != ',' && !character.is_whitespace() {
            current.push(character);
        }
    }
    flush(&mut current, &mut numbers);
    numbers
}

/// 내장 아이콘이 쓰는 경로 명령(M·L·H·V·C·S·Z, 대소문자)만 읽는다
fn path_commands(data: &str, place: &dyn Fn(f64, f64) -> Point) -> Vec<PathCommand> {
    let mut tracer = PathTracer::new();
    let mut segments: Vec<(char, String)> = Vec::new();
    for character in data.chars() {
        if character.is_ascii_alphabetic() {
            segments.push((character, String::new()));
        } else if let Some((_, arguments)) = segments.last_mut() {
            arguments.push(character);
        }
    }
    for (command, arguments) in segments {
        tracer.apply(command, &path_numbers(&arguments));
    }
    tracer
        .commands
        .into_iter()
        .map(|command| place_command(command, place))
        .collect()
}

fn place_command(command: PathCommand, place: &dyn Fn(f64, f64) -> Point) -> PathCommand {
    let at = |point: Point| place(point.x, point.y);
    match command {
        PathCommand::MoveTo(point) => PathCommand::MoveTo(at(point)),
        PathCommand::LineTo(point) => PathCommand::LineTo(at(point)),
        PathCommand::QuadTo(control, point) => PathCommand::QuadTo(at(control), at(point)),
        PathCommand::CubicTo(first, second, point) => {
            PathCommand::CubicTo(at(first), at(second), at(point))
        }
        PathCommand::Close => PathCommand::Close,
    }
}

/// 상대 좌표를 절대 좌표로 풀며 경로를 따라간다
struct PathTracer {
    commands: Vec<PathCommand>,
    current: Point,
    start: Point,
    /// 직전 3차 곡선의 두 번째 조절점(S 명령의 반사 기준)
    last_control: Option<Point>,
}

impl PathTracer {
    fn new() -> Self {
        let origin = Point::new(0.0, 0.0);
        Self {
            commands: Vec::new(),
            current: origin,
            start: origin,
            last_control: None,
        }
    }

    fn apply(&mut self, command: char, numbers: &[f64]) {
        let is_relative = command.is_ascii_lowercase();
        match command.to_ascii_uppercase() {
            'M' | 'L' => self.lines(numbers, is_relative, command.eq_ignore_ascii_case(&'m')),
            'H' => numbers.iter().for_each(|&x| {
                let base = if is_relative { self.current.x } else { 0.0 };
                self.line_or_move(Point::new(base + x, self.current.y), false);
            }),
            'V' => numbers.iter().for_each(|&y| {
                let base = if is_relative { self.current.y } else { 0.0 };
                self.line_or_move(Point::new(self.current.x, base + y), false);
            }),
            'C' => self.cubics(numbers, is_relative),
            'S' => self.smooth_cubics(numbers, is_relative),
            _ => {
                self.commands.push(PathCommand::Close);
                self.current = self.start;
                self.last_control = None;
            }
        }
    }

    /// 상대 명령이면 지금 점, 절대 명령이면 원점을 기준으로 (x, y)를 옮긴다
    fn resolve(&self, is_relative: bool, x: f64, y: f64) -> Point {
        if is_relative {
            Point::new(self.current.x + x, self.current.y + y)
        } else {
            Point::new(x, y)
        }
    }

    /// M·L: `M` 뒤 첫 쌍만 옮기기이고 나머지 쌍은 선이다
    fn lines(&mut self, numbers: &[f64], is_relative: bool, is_move: bool) {
        let (pairs, _) = numbers.as_chunks::<2>();
        for (index, [x, y]) in pairs.iter().enumerate() {
            let point = self.resolve(is_relative, *x, *y);
            self.line_or_move(point, is_move && index == 0);
        }
    }

    fn cubics(&mut self, numbers: &[f64], is_relative: bool) {
        let (groups, _) = numbers.as_chunks::<6>();
        for [x1, y1, x2, y2, x, y] in groups {
            let first = self.resolve(is_relative, *x1, *y1);
            let second = self.resolve(is_relative, *x2, *y2);
            let end = self.resolve(is_relative, *x, *y);
            self.cubic(first, second, end);
        }
    }

    /// S: 첫 조절점은 직전 곡선의 두 번째 조절점을 지금 점에 대해 뒤집은 점이다
    fn smooth_cubics(&mut self, numbers: &[f64], is_relative: bool) {
        let (groups, _) = numbers.as_chunks::<4>();
        for [x2, y2, x, y] in groups {
            let current = self.current;
            let first = self.last_control.map_or(current, |control| {
                Point::new(2.0 * current.x - control.x, 2.0 * current.y - control.y)
            });
            let second = self.resolve(is_relative, *x2, *y2);
            let end = self.resolve(is_relative, *x, *y);
            self.cubic(first, second, end);
        }
    }

    fn line_or_move(&mut self, point: Point, is_move: bool) {
        if is_move {
            self.commands.push(PathCommand::MoveTo(point));
            self.start = point;
        } else {
            self.commands.push(PathCommand::LineTo(point));
        }
        self.current = point;
        self.last_control = None;
    }

    fn cubic(&mut self, first: Point, second: Point, end: Point) {
        self.commands.push(PathCommand::CubicTo(first, second, end));
        self.current = end;
        self.last_control = Some(second);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 압축된_경로_숫자를_나눠_읽는다() {
        assert_eq!(path_numbers("-.45.5-1,.5"), vec![-0.45, 0.5, -1.0, 0.5]);
        assert_eq!(path_numbers("15.28,11.1-15.28"), vec![15.28, 11.1, -15.28]);
    }

    #[test]
    fn 상대_곡선과_반사_곡선을_절대_좌표로_푼다() {
        let commands = path_commands(
            "m20,57.86c0,3.94,8.95,7.14,20,7.14s20-3.2,20-7.14",
            &|x, y| Point::new(x, y),
        );
        assert_eq!(commands[0], PathCommand::MoveTo(Point::new(20.0, 57.86)));
        let PathCommand::CubicTo(first, _, end) = commands[2] else {
            panic!("반사 곡선");
        };
        assert!((first.x - 51.05).abs() < 1e-9 && (first.y - 65.0).abs() < 1e-9);
        assert!((end.x - 60.0).abs() < 1e-9 && (end.y - 57.86).abs() < 1e-9);
    }

    #[test]
    fn 등록되지_않은_아이콘_묶음은_물음표다() {
        assert_eq!(Icon::named("logos:aws-lambda"), Icon::Unknown);
        assert_eq!(Icon::named("database"), Icon::Database);
    }
}
