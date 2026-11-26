//! roughjs 4.6.6의 손그림 생성기(`look: handDrawn`). Mermaid가 쓰는 path·circle·rectangle·line과
//! hachure·solid 채움만 옮겼다.
//!
//! 같은 시드면 같은 선이 나오도록 roughjs의 의사 난수(`Math.imul(48271, seed)`)와 난수를 꺼내는 순서를 그대로
//! 따른다. 모양 하나마다 옵션 객체가 새로 만들어지므로 난수기도 모양마다 시드에서 다시 시작한다.

mod hachure;
mod path_data;
mod sketch;

pub use self::path_data::Segment;

use crate::scene::{Color, PathCommand, PathItem, Point, SceneItem, StrokeStyle};

/// roughjs 옵션 가운데 Mermaid가 바꾸는 값(나머지는 roughjs 기본값)
#[derive(Clone, Debug)]
pub struct RoughOptions {
    pub roughness: f64,
    pub seed: i32,
    pub stroke: Option<Color>,
    pub stroke_width: f64,
    pub fill: Option<Color>,
    pub fill_style: FillStyle,
    pub fill_weight: f64,
    pub hachure_angle: f64,
    pub hachure_gap: f64,
    /// strokeLineDash(테두리 경로의 stroke-dasharray)
    pub stroke_dash: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillStyle {
    Hachure,
    Solid,
}

impl RoughOptions {
    /// roughjs 기본 옵션(거칠기 1, 검은 1px 선, 채움 없음)
    pub fn new(seed: i32) -> Self {
        Self {
            roughness: 1.0,
            seed,
            stroke: Some(Color::rgb(0, 0, 0)),
            stroke_width: 1.0,
            fill: None,
            fill_style: FillStyle::Hachure,
            fill_weight: -1.0,
            hachure_angle: -41.0,
            hachure_gap: -1.0,
            stroke_dash: Vec::new(),
        }
    }
}

/// 그릴 경로 묶음 하나(roughjs drawable.sets의 한 칸)
#[derive(Clone, Debug, PartialEq)]
pub struct RoughSet {
    pub commands: Vec<PathCommand>,
    pub fill: Option<Color>,
    pub stroke: Option<StrokeStyle>,
}

impl RoughSet {
    pub fn into_item(self) -> SceneItem {
        SceneItem::Path(PathItem {
            commands: self.commands,
            fill: self.fill,
            stroke: self.stroke,
        })
    }
}

/// RoughGenerator.path: 채움(hachure 또는 solid)을 먼저, 테두리를 나중에 돌려준다
pub fn path(segments: &[Segment], options: &RoughOptions) -> Vec<RoughSet> {
    let mut sketcher = sketch::Sketcher::new(options);
    let normalized = path_data::normalize(segments);
    let distance = (1.0 + options.roughness) / 2.0;
    let point_sets = path_data::points_on_path(&normalized, 1.0, distance);
    let outline = sketcher.svg_path(&normalized);
    let mut sets = Vec::new();
    if let Some(fill) = options.fill {
        let ops = match options.fill_style {
            FillStyle::Hachure => sketcher.hachure_fill(&point_sets),
            FillStyle::Solid if point_sets.len() == 1 => sketcher.solid_path_fill(&normalized),
            FillStyle::Solid => sketcher.solid_polygon_fill(&point_sets),
        };
        sets.push(fill_set(ops, fill, options));
    }
    sets.extend(stroke_set(outline, options));
    sets
}

/// RoughGenerator.circle(ellipse): 지름 `diameter`의 원
pub fn circle(center: Point, diameter: f64, options: &RoughOptions) -> Vec<RoughSet> {
    let mut sketcher = sketch::Sketcher::new(options);
    let params = sketcher.ellipse_params(diameter, diameter);
    let (outline, estimated) = sketcher.ellipse_with_params(center, &params);
    let mut sets = Vec::new();
    if let Some(fill) = options.fill {
        let ops = match options.fill_style {
            FillStyle::Hachure => sketcher.hachure_fill(&[estimated]),
            FillStyle::Solid => sketcher.ellipse_with_params(center, &params).0,
        };
        sets.push(fill_set(ops, fill, options));
    }
    sets.extend(stroke_set(outline, options));
    sets
}

/// RoughGenerator.rectangle: 네 변을 닫힌 linearPath로 긋고, 채움은 그 다각형을 칠한다
pub fn rectangle(origin: Point, width: f64, height: f64, options: &RoughOptions) -> Vec<RoughSet> {
    let mut sketcher = sketch::Sketcher::new(options);
    let corners = vec![
        origin,
        Point::new(origin.x + width, origin.y),
        Point::new(origin.x + width, origin.y + height),
        Point::new(origin.x, origin.y + height),
    ];
    let outline = sketcher.closed_linear_path(&corners);
    let mut sets = Vec::new();
    if let Some(fill) = options.fill {
        let polygons = [corners];
        let ops = match options.fill_style {
            FillStyle::Hachure => sketcher.hachure_fill(&polygons),
            FillStyle::Solid => sketcher.solid_polygon_fill(&polygons),
        };
        sets.push(fill_set(ops, fill, options));
    }
    sets.extend(stroke_set(outline, options));
    sets
}

/// RoughGenerator.line: 흔들린 직선 두 번
pub fn line(from: Point, to: Point, options: &RoughOptions) -> Vec<RoughSet> {
    let mut sketcher = sketch::Sketcher::new(options);
    let ops = sketcher.closed_linear_path(&[from, to]);
    stroke_set(ops, options).into_iter().collect()
}

/// 채움 묶음: solid는 채운 경로, hachure는 채움색 선(fillWeight, 음수면 선 굵기 절반)
fn fill_set(commands: Vec<PathCommand>, fill: Color, options: &RoughOptions) -> RoughSet {
    match options.fill_style {
        FillStyle::Solid => RoughSet {
            commands,
            fill: Some(fill),
            stroke: None,
        },
        FillStyle::Hachure => {
            let width = if options.fill_weight < 0.0 {
                options.stroke_width / 2.0
            } else {
                options.fill_weight
            };
            RoughSet {
                commands,
                fill: None,
                stroke: Some(StrokeStyle {
                    color: fill,
                    width,
                    dash: Vec::new(),
                }),
            }
        }
    }
}

fn stroke_set(commands: Vec<PathCommand>, options: &RoughOptions) -> Option<RoughSet> {
    let color = options.stroke?;
    Some(RoughSet {
        commands,
        fill: None,
        stroke: Some(StrokeStyle {
            color,
            width: options.stroke_width,
            dash: options.stroke_dash.clone(),
        }),
    })
}

/// 경로 명령을 M·L·C 구간으로 바꾼다(평평하게 펴지 않은 간선 경로를 손그림에 넘길 때)
pub fn segments_from_commands(commands: &[PathCommand]) -> Vec<Segment> {
    commands
        .iter()
        .filter_map(|command| match command {
            PathCommand::MoveTo(point) => Some(Segment::MoveTo(*point)),
            PathCommand::LineTo(point) => Some(Segment::LineTo(*point)),
            PathCommand::CubicTo(first, second, point) => {
                Some(Segment::CubicTo(*first, *second, *point))
            }
            PathCommand::QuadTo(..) => None,
            PathCommand::Close => Some(Segment::Close),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 시드_7_시작_원이_기준_svg_경로와_같다() {
        let options = RoughOptions {
            roughness: 0.7,
            fill: Some(Color::rgb(0x33, 0x33, 0x33)),
            stroke: Some(Color::rgb(0x33, 0x33, 0x33)),
            fill_weight: 2.0,
            hachure_angle: 120.0,
            hachure_gap: 4.0,
            ..RoughOptions::new(7)
        };
        let sets = circle(Point::new(0.0, 0.0), 14.0, &options);
        for set in &sets {
            eprintln!("{:?}", &set.commands[..2]);
        }
        let PathCommand::MoveTo(start) = sets[1].commands[0] else {
            panic!("move");
        };
        assert!(
            (start.x - -1.441_458_276_512_849_6).abs() < 1e-9,
            "{start:?}"
        );
        let PathCommand::MoveTo(fill_start) = sets[0].commands[0] else {
            panic!("move");
        };
        assert!(
            (fill_start.x - 3.174_688_420_123_480_6).abs() < 1e-9,
            "{fill_start:?}"
        );
    }
}
