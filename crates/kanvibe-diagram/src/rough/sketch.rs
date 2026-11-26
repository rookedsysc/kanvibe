//! roughjs renderer.js: 흔들린 직선(_doubleLine)·곡선(_curve)·원(_computeEllipsePoints)·경로(svgPath)와
//! hachure 채움 선. 난수는 옵션 하나에 묶인 Random을 꺼내 쓴 순서 그대로 쓴다.

use super::RoughOptions;
use super::hachure::hachure_lines;
use super::path_data::Segment;
use crate::scene::{PathCommand, Point};
use std::f64::consts::PI;

/// roughjs 기본값(Mermaid가 바꾸지 않는 옵션)
const MAX_RANDOMNESS_OFFSET: f64 = 2.0;
const BOWING: f64 = 1.0;
const CURVE_TIGHTNESS: f64 = 0.0;
const CURVE_FITTING: f64 = 0.95;
const CURVE_STEP_COUNT: f64 = 9.0;
const FILL_SHAPE_ROUGHNESS_GAIN: f64 = 0.8;

/// roughjs math.js Random: `seed = Math.imul(48271, seed)`, `(2³¹−1 & seed) / 2³¹`
#[derive(Clone, Copy, Debug)]
struct Random {
    seed: i32,
}

impl Random {
    fn next(&mut self) -> f64 {
        if self.seed == 0 {
            // 시드 0은 원본이 Math.random을 써서 다시 그릴 수 없다. 가운데 값으로 고정한다
            return 0.5;
        }
        self.seed = self.seed.wrapping_mul(48271);
        f64::from(self.seed & i32::MAX) / 2_147_483_648.0
    }
}

/// 원의 단계 각과 흔들린 반지름(generateEllipseParams)
pub struct EllipseParams {
    increment: f64,
    radius: Point,
}

pub struct Sketcher {
    random: Random,
    roughness: f64,
    is_single_stroke: bool,
    hachure_angle: f64,
    hachure_gap: f64,
    stroke_width: f64,
}

impl Sketcher {
    pub fn new(options: &RoughOptions) -> Self {
        Self {
            random: Random { seed: options.seed },
            roughness: options.roughness,
            is_single_stroke: false,
            hachure_angle: options.hachure_angle,
            hachure_gap: options.hachure_gap,
            stroke_width: options.stroke_width,
        }
    }

    fn offset(&mut self, min: f64, max: f64, gain: f64) -> f64 {
        self.roughness * gain * (self.random.next() * (max - min) + min)
    }

    fn offset_opt(&mut self, value: f64, gain: f64) -> f64 {
        self.offset(-value, value, gain)
    }

    /// _doubleLine: 같은 직선을 두 번 흔들어 긋는다(채움 선도 두 번)
    fn double_line(&mut self, from: Point, to: Point) -> Vec<PathCommand> {
        let mut ops = self.line(from, to, false);
        if !self.is_single_stroke {
            ops.extend(self.line(from, to, true));
        }
        ops
    }

    /// _line: 길이에 따라 흔들림을 줄이고, 휘어진 중간점 두 개로 3차 곡선을 만든다
    fn line(&mut self, from: Point, to: Point, is_overlay: bool) -> Vec<PathCommand> {
        let length_sq = (from.x - to.x).powi(2) + (from.y - to.y).powi(2);
        let length = length_sq.sqrt();
        let gain = if length < 200.0 {
            1.0
        } else if length > 500.0 {
            0.4
        } else {
            -0.001_666_8 * length + 1.233_334
        };
        let mut offset = MAX_RANDOMNESS_OFFSET;
        if offset * offset * 100.0 > length_sq {
            offset = length / 10.0;
        }
        let jitter = if is_overlay { offset / 2.0 } else { offset };
        let diverge = 0.2 + self.random.next() * 0.2;
        let mid_x = BOWING * MAX_RANDOMNESS_OFFSET * (to.y - from.y) / 200.0;
        let mid_y = BOWING * MAX_RANDOMNESS_OFFSET * (from.x - to.x) / 200.0;
        let mid_x = self.offset_opt(mid_x, gain);
        let mid_y = self.offset_opt(mid_y, gain);
        let start = Point::new(
            from.x + self.offset_opt(jitter, gain),
            from.y + self.offset_opt(jitter, gain),
        );
        let first = Point::new(
            mid_x + from.x + (to.x - from.x) * diverge + self.offset_opt(jitter, gain),
            mid_y + from.y + (to.y - from.y) * diverge + self.offset_opt(jitter, gain),
        );
        let second = Point::new(
            mid_x + from.x + 2.0 * (to.x - from.x) * diverge + self.offset_opt(jitter, gain),
            mid_y + from.y + 2.0 * (to.y - from.y) * diverge + self.offset_opt(jitter, gain),
        );
        let end = Point::new(
            to.x + self.offset_opt(jitter, gain),
            to.y + self.offset_opt(jitter, gain),
        );
        vec![
            PathCommand::MoveTo(start),
            PathCommand::CubicTo(first, second, end),
        ]
    }

    /// linearPath(닫힘): 점이 셋 이상이면 이웃 점마다, 끝에서 처음 점까지 _doubleLine. 둘이면 직선 하나
    pub fn closed_linear_path(&mut self, points: &[Point]) -> Vec<PathCommand> {
        match points.len() {
            0 | 1 => Vec::new(),
            2 => self.double_line(points[0], points[1]),
            length => {
                let mut ops = Vec::new();
                for pair in points.windows(2) {
                    ops.extend(self.double_line(pair[0], pair[1]));
                }
                ops.extend(self.double_line(points[length - 1], points[0]));
                ops
            }
        }
    }

    /// _curve: 점들을 지나는 Catmull-Rom 곡선(curveTightness 0)
    fn curve(&mut self, points: &[Point]) -> Vec<PathCommand> {
        let tension = 1.0 - CURVE_TIGHTNESS;
        match points.len() {
            length if length > 3 => {
                let mut ops = vec![PathCommand::MoveTo(points[1])];
                for index in 1..length - 2 {
                    let (previous, current) = (points[index - 1], points[index]);
                    let (next, after) = (points[index + 1], points[index + 2]);
                    let first = Point::new(
                        current.x + (tension * next.x - tension * previous.x) / 6.0,
                        current.y + (tension * next.y - tension * previous.y) / 6.0,
                    );
                    let second = Point::new(
                        next.x + (tension * current.x - tension * after.x) / 6.0,
                        next.y + (tension * current.y - tension * after.y) / 6.0,
                    );
                    ops.push(PathCommand::CubicTo(first, second, next));
                }
                ops
            }
            3 => vec![
                PathCommand::MoveTo(points[1]),
                PathCommand::CubicTo(points[1], points[2], points[2]),
            ],
            2 => self.line(points[0], points[1], true),
            _ => Vec::new(),
        }
    }

    /// generateEllipseParams
    pub fn ellipse_params(&mut self, width: f64, height: f64) -> EllipseParams {
        let half = ((width / 2.0).powi(2) + (height / 2.0).powi(2)) / 2.0;
        let psq = (PI * 2.0 * half.sqrt()).sqrt();
        let step_count = CURVE_STEP_COUNT
            .max(CURVE_STEP_COUNT / 200f64.sqrt() * psq)
            .ceil();
        let increment = PI * 2.0 / step_count;
        let fit_randomness = 1.0 - CURVE_FITTING;
        let (mut rx, mut ry) = ((width / 2.0).abs(), (height / 2.0).abs());
        rx += self.offset_opt(rx * fit_randomness, 1.0);
        ry += self.offset_opt(ry * fit_randomness, 1.0);
        EllipseParams {
            increment,
            radius: Point::new(rx, ry),
        }
    }

    /// ellipseWithParams: 흔들린 원 두 바퀴 경로와 채움용 핵심 점들
    pub fn ellipse_with_params(
        &mut self,
        center: Point,
        params: &EllipseParams,
    ) -> (Vec<PathCommand>, Vec<Point>) {
        let inner = self.offset(0.4, 1.0, 1.0);
        let overlap = params.increment * self.offset(0.1, inner, 1.0);
        let (all, core) = self.ellipse_points(center, params, 1.0, overlap);
        let mut ops = self.curve(&all);
        if !self.is_single_stroke && self.roughness != 0.0 {
            let (second, _) = self.ellipse_points(center, params, 1.5, 0.0);
            ops.extend(self.curve(&second));
        }
        (ops, core)
    }

    /// _computeEllipsePoints(거칠기가 0이 아닌 경우)
    fn ellipse_points(
        &mut self,
        center: Point,
        params: &EllipseParams,
        offset: f64,
        overlap: f64,
    ) -> (Vec<Point>, Vec<Point>) {
        let (increment, radius) = (params.increment, params.radius);
        let rad_offset = self.offset_opt(0.5, 1.0) - PI / 2.0;
        let point_at = |sketcher: &mut Self, angle: f64, scale: f64| {
            let x = sketcher.offset_opt(offset, 1.0) + center.x + scale * radius.x * angle.cos();
            let y = sketcher.offset_opt(offset, 1.0) + center.y + scale * radius.y * angle.sin();
            Point::new(x, y)
        };
        let mut all = vec![point_at(self, rad_offset - increment, 0.9)];
        let mut core = Vec::new();
        let end_angle = PI * 2.0 + rad_offset - 0.01;
        let mut angle = rad_offset;
        while angle < end_angle {
            let point = point_at(self, angle, 1.0);
            core.push(point);
            all.push(point);
            angle += increment;
        }
        all.push(point_at(self, rad_offset + PI * 2.0 + overlap * 0.5, 1.0));
        all.push(point_at(self, rad_offset + overlap, 0.98));
        all.push(point_at(self, rad_offset + overlap * 0.5, 0.9));
        (all, core)
    }

    /// svgPath: 직선은 _doubleLine, 3차 곡선은 _bezierTo, 닫기는 시작점까지 직선
    pub fn svg_path(&mut self, segments: &[Segment]) -> Vec<PathCommand> {
        let mut ops = Vec::new();
        let mut first = Point::new(0.0, 0.0);
        let mut current = first;
        for segment in segments {
            match *segment {
                Segment::MoveTo(point) => {
                    current = point;
                    first = point;
                }
                Segment::LineTo(point) => {
                    ops.extend(self.double_line(current, point));
                    current = point;
                }
                Segment::CubicTo(control1, control2, end) => {
                    ops.extend(self.bezier_to([control1, control2, end], current));
                    current = end;
                }
                Segment::Close => {
                    ops.extend(self.double_line(current, first));
                    current = first;
                }
                _ => {}
            }
        }
        ops
    }

    /// _bezierTo: 제어점과 끝점을 흔든 곡선 두 번
    fn bezier_to(
        &mut self,
        [control1, control2, end]: [Point; 3],
        current: Point,
    ) -> Vec<PathCommand> {
        let offsets = [MAX_RANDOMNESS_OFFSET, MAX_RANDOMNESS_OFFSET + 0.3];
        let iterations = if self.is_single_stroke { 1 } else { 2 };
        let mut ops = Vec::new();
        for (index, jitter) in offsets.iter().take(iterations).enumerate() {
            let start = if index == 0 {
                current
            } else {
                let x = current.x + self.offset_opt(offsets[0], 1.0);
                let y = current.y + self.offset_opt(offsets[0], 1.0);
                Point::new(x, y)
            };
            ops.push(PathCommand::MoveTo(start));
            let target = Point::new(
                end.x + self.offset_opt(*jitter, 1.0),
                end.y + self.offset_opt(*jitter, 1.0),
            );
            let first = Point::new(
                control1.x + self.offset_opt(*jitter, 1.0),
                control1.y + self.offset_opt(*jitter, 1.0),
            );
            let second = Point::new(
                control2.x + self.offset_opt(*jitter, 1.0),
                control2.y + self.offset_opt(*jitter, 1.0),
            );
            ops.push(PathCommand::CubicTo(first, second, target));
        }
        ops
    }

    /// polygonHachureLines + HachureFiller.renderLines
    pub fn hachure_fill(&mut self, polygons: &[Vec<Point>]) -> Vec<PathCommand> {
        let angle = self.hachure_angle + 90.0;
        let mut gap = self.hachure_gap;
        if gap < 0.0 {
            gap = self.stroke_width * 4.0;
        }
        let gap = (gap.max(0.1) + 0.5).floor();
        let mut step = 1.0;
        if self.roughness >= 1.0 && self.random.next() > 0.7 {
            step = gap;
        }
        let lines = hachure_lines(polygons, gap, angle, step);
        lines
            .iter()
            .flat_map(|[from, to]| self.double_line(*from, *to))
            .collect()
    }

    /// path의 solid 채움(하위 경로가 하나일 때): 거칠기를 더해 한 번만 그은 경로에서 뒤따르는 move를 뺀다
    pub fn solid_path_fill(&mut self, segments: &[Segment]) -> Vec<PathCommand> {
        let saved = (self.roughness, self.is_single_stroke);
        if self.roughness != 0.0 {
            self.roughness += FILL_SHAPE_ROUGHNESS_GAIN;
        }
        self.is_single_stroke = true;
        let ops = self.svg_path(segments);
        (self.roughness, self.is_single_stroke) = saved;
        ops.into_iter()
            .enumerate()
            .filter(|(index, op)| *index == 0 || !matches!(op, PathCommand::MoveTo(_)))
            .map(|(_, op)| op)
            .collect()
    }

    /// solidFillPolygon: 꼭짓점마다 흔든 다각형(하위 경로가 여럿인 path의 solid 채움)
    pub fn solid_polygon_fill(&mut self, polygons: &[Vec<Point>]) -> Vec<PathCommand> {
        let mut ops = Vec::new();
        for points in polygons.iter().filter(|points| points.len() > 2) {
            for (index, point) in points.iter().enumerate() {
                let x = point.x + self.offset_opt(MAX_RANDOMNESS_OFFSET, 1.0);
                let y = point.y + self.offset_opt(MAX_RANDOMNESS_OFFSET, 1.0);
                let moved = Point::new(x, y);
                ops.push(match index {
                    0 => PathCommand::MoveTo(moved),
                    _ => PathCommand::LineTo(moved),
                });
            }
        }
        ops
    }
}
