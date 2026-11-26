//! 마인드맵 노드 모양(rendering-elements/shapes의 defaultMindmapNode·mindmapCircle·squareRect·roundedRect·
//! hexagon·cloud·bang). 좌표는 노드 중심 기준이다.

use crate::LabelMode;
use crate::flowchart::geometry::path_bounds;
use crate::label::LabelBox;
use crate::scene::{PathCommand, Point, ellipse, polygon, rounded_rect};

use super::parser::NodeShape;
use super::skia_arc::{RelativeArc, arc_path_bounds};

/// 마인드맵 기본 여백(`mindmap.padding`)
const PADDING: f64 = 10.0;
/// defaultMindmapNode 모서리 반지름
const CORNER_RADIUS: f64 = 5.0;

pub(crate) struct NodeGeometry {
    pub outline: Vec<PathCommand>,
    /// 기본 노드 아래 밑줄(왼쪽 끝, 오른쪽 끝)
    pub underline: Option<(Point, Point)>,
    /// 라벨 상자 왼쪽 위
    pub label_origin: Point,
    /// getBBox(): 모양과 라벨을 합친 범위 (min_x, min_y, max_x, max_y)
    pub bounds: (f64, f64, f64, f64),
}

/// `label_box_shift`는 라벨 묶음 원점에서 라벨 getBBox 상자 왼쪽 위까지의 거리다(아이콘 배경 상자 등)
pub(crate) fn node_geometry(
    shape: NodeShape,
    label: &LabelBox,
    label_box_shift: Point,
) -> NodeGeometry {
    let label_origin = label_origin(shape, label);
    let label_box = Point::new(
        label_origin.x + label_box_shift.x,
        label_origin.y + label_box_shift.y,
    );
    let (width, height) = (label.width, label.height);
    let (outline, underline) = match shape {
        NodeShape::Default => default_outline(width + 8.0 * (PADDING / 2.0), height + PADDING),
        NodeShape::Rect => (
            centered_rect(width + PADDING * 4.0, height + PADDING * 2.0, 0.0),
            None,
        ),
        NodeShape::RoundedRect => (
            centered_rect(width + 30.0, height + 30.0, CORNER_RADIUS),
            None,
        ),
        NodeShape::Circle => {
            let radius = width / 2.0 + PADDING;
            (ellipse(Point::new(0.0, 0.0), radius, radius), None)
        }
        NodeShape::Hexagon => (hexagon_outline(width, height), None),
        NodeShape::Cloud => {
            let (w, h) = (width + PADDING, height + PADDING);
            return arc_geometry(cloud_arcs(w, h), (w, h), (label_origin, label_box), label);
        }
        NodeShape::Bang => {
            let (w, h) = (width + PADDING * 5.0, height + PADDING * 4.0);
            return arc_geometry(bang_arcs(w, h), (w, h), (label_origin, label_box), label);
        }
    };
    let shape_bounds = path_bounds(&outline).unwrap_or((0.0, 0.0, 0.0, 0.0));
    let bounds = (
        shape_bounds.0.min(label_box.x),
        shape_bounds.1.min(label_box.y),
        shape_bounds.2.max(label_box.x + width),
        shape_bounds.3.max(label_box.y + height),
    );
    NodeGeometry {
        outline,
        underline,
        label_origin,
        bounds,
    }
}

/// labelHelper는 SVG 글자 라벨을 가로로 옮기지 않는다(translate(0, −h/2)). 기본·구름·폭발 모양만
/// 라벨을 다시 가운데로 옮기므로 원·사각형·육각형 SVG 라벨은 노드 중심에서 시작한다
fn label_origin(shape: NodeShape, label: &LabelBox) -> Point {
    let is_recentered = matches!(
        shape,
        NodeShape::Default | NodeShape::Cloud | NodeShape::Bang
    );
    let left = if label.mode == LabelMode::Html || is_recentered {
        -label.width / 2.0
    } else {
        0.0
    };
    Point::new(left, -label.height / 2.0)
}

fn centered_rect(width: f64, height: f64, radius: f64) -> Vec<PathCommand> {
    rounded_rect(-width / 2.0, -height / 2.0, width, height, radius)
}

/// defaultMindmapNode: 네 모서리를 2차 곡선으로 둥글린 사각형과 아래 밑줄
fn default_outline(width: f64, height: f64) -> (Vec<PathCommand>, Option<(Point, Point)>) {
    let (half_width, half_height, r) = (width / 2.0, height / 2.0, CORNER_RADIUS);
    let commands = vec![
        PathCommand::MoveTo(Point::new(-half_width, half_height - r)),
        PathCommand::LineTo(Point::new(-half_width, -half_height + r)),
        PathCommand::QuadTo(
            Point::new(-half_width, -half_height),
            Point::new(-half_width + r, -half_height),
        ),
        PathCommand::LineTo(Point::new(half_width - r, -half_height)),
        PathCommand::QuadTo(
            Point::new(half_width, -half_height),
            Point::new(half_width, -half_height + r),
        ),
        PathCommand::LineTo(Point::new(half_width, half_height - r)),
        PathCommand::QuadTo(
            Point::new(half_width, half_height),
            Point::new(half_width - r, half_height),
        ),
        PathCommand::LineTo(Point::new(-half_width + r, half_height)),
        PathCommand::QuadTo(
            Point::new(-half_width, half_height),
            Point::new(-half_width, half_height - r),
        ),
        PathCommand::Close,
    ];
    let underline = (
        Point::new(-half_width, half_height),
        Point::new(half_width, half_height),
    );
    (commands, Some(underline))
}

/// hexagon: 여백은 `padding × 2`(20), 옆 꼭짓점 폭은 높이의 1/4
fn hexagon_outline(label_width: f64, label_height: f64) -> Vec<PathCommand> {
    let padding = PADDING * 2.0;
    let height = label_height + padding;
    let inset = height / 4.0;
    let width = label_width + 2.0 * inset + padding;
    let (dx, dy) = (-width / 2.0, height / 2.0);
    let points = [
        (inset, 0.0),
        (width - inset, 0.0),
        (width, -height / 2.0),
        (width - inset, -height),
        (inset, -height),
        (0.0, -height / 2.0),
    ];
    let shifted: Vec<Point> = points
        .iter()
        .map(|&(x, y)| Point::new(x + dx, y + dy))
        .collect();
    polygon(&shifted)
}

/// 구름·폭발: `M0 0 a… H0 V0 Z` 경로를 모양 왼쪽 위가 (−w/2, −h/2)에 오도록 옮긴다.
/// `(label_origin, label_box)`는 라벨 묶음 원점과 라벨 getBBox 상자 왼쪽 위다
fn arc_geometry(
    arcs: Vec<RelativeArc>,
    (width, height): (f64, f64),
    (label_origin, label_box): (Point, Point),
    label: &LabelBox,
) -> NodeGeometry {
    let offset = Point::new(-width / 2.0, -height / 2.0);
    NodeGeometry {
        outline: arc_outline(&arcs, offset),
        underline: None,
        label_origin,
        bounds: chromium_arc_node_bounds(&arcs, offset, label_box, label),
    }
}

/// 노드 `<g>`의 getBBox: 원호 경로 범위(translate로 옮김)와 라벨 상자를 f32로 합친다(RectF::UnionEvenIfEmpty)
fn chromium_arc_node_bounds(
    arcs: &[RelativeArc],
    offset: Point,
    label_box: Point,
    label: &LabelBox,
) -> (f64, f64, f64, f64) {
    let (min_x, min_y, max_x, max_y) = arc_path_bounds(arcs);
    let path_x = (f64::from(min_x) + offset.x) as f32;
    let path_y = (f64::from(min_y) + offset.y) as f32;
    let (path_width, path_height) = (max_x - min_x, max_y - min_y);
    let (label_x, label_y) = (label_box.x as f32, label_box.y as f32);
    let (label_width, label_height) = (label.width as f32, label.height as f32);
    let left = path_x.min(label_x);
    let top = path_y.min(label_y);
    let right = (path_x + path_width).max(label_x + label_width);
    let bottom = (path_y + path_height).max(label_y + label_height);
    let (width, height) = (right - left, bottom - top);
    (
        f64::from(left),
        f64::from(top),
        f64::from(left) + f64::from(width),
        f64::from(top) + f64::from(height),
    )
}

fn cloud_arcs(width: f64, height: f64) -> Vec<RelativeArc> {
    let (r1, r2, r3, r4) = (0.15 * width, 0.25 * width, 0.35 * width, 0.2 * width);
    let (w, h) = (width, height);
    vec![
        (r1, r1, 0.0, false, true, w * 0.25, -w * 0.1),
        (r3, r3, 1.0, false, true, w * 0.4, -w * 0.1),
        (r2, r2, 1.0, false, true, w * 0.35, w * 0.2),
        (r1, r1, 1.0, false, true, w * 0.15, h * 0.35),
        (r4, r4, 1.0, false, true, -w * 0.15, h * 0.65),
        (r2, r1, 1.0, false, true, -w * 0.25, w * 0.15),
        (r3, r3, 1.0, false, true, -w * 0.5, 0.0),
        (r1, r1, 1.0, false, true, -w * 0.25, -w * 0.15),
        (r1, r1, 1.0, false, true, -w * 0.1, -h * 0.35),
        (r4, r4, 1.0, false, true, w * 0.1, -h * 0.65),
    ]
}

fn bang_arcs(width: f64, height: f64) -> Vec<RelativeArc> {
    let r = 0.15 * width;
    let (w, h) = (width, height);
    let arc = |radius: f64, dx: f64, dy: f64| -> RelativeArc {
        (radius, radius, 1.0, false, false, dx, dy)
    };
    vec![
        arc(r, w * 0.25, -h * 0.1),
        arc(r, w * 0.25, 0.0),
        arc(r, w * 0.25, 0.0),
        arc(r, w * 0.25, h * 0.1),
        arc(r, w * 0.15, h * 0.33),
        arc(r * 0.8, 0.0, h * 0.34),
        arc(r, -w * 0.15, h * 0.33),
        arc(r, -w * 0.25, h * 0.15),
        arc(r, -w * 0.25, 0.0),
        arc(r, -w * 0.25, 0.0),
        arc(r, -w * 0.25, -h * 0.15),
        arc(r, -w * 0.1, -h * 0.33),
        arc(r * 0.8, 0.0, -h * 0.34),
        arc(r, w * 0.1, -h * 0.33),
    ]
}

/// `M0 0 a… H0 V0 Z`를 offset만큼 옮겨 3차 곡선으로 바꾼다
fn arc_outline(arcs: &[RelativeArc], offset: Point) -> Vec<PathCommand> {
    let mut current = offset;
    let mut commands = vec![PathCommand::MoveTo(current)];
    for &(radius_x, radius_y, rotation, is_large, is_sweep, dx, dy) in arcs {
        let end = Point::new(current.x + dx, current.y + dy);
        let arc = SvgArc {
            radius_x,
            radius_y,
            rotation_degrees: rotation,
            is_large,
            is_sweep,
        };
        commands.extend(arc.to_cubics(current, end));
        current = end;
    }
    commands.push(PathCommand::LineTo(Point::new(offset.x, current.y)));
    commands.push(PathCommand::LineTo(offset));
    commands.push(PathCommand::Close);
    commands
}

/// SVG 원호 명령(끝점 표기)
struct SvgArc {
    radius_x: f64,
    radius_y: f64,
    rotation_degrees: f64,
    is_large: bool,
    is_sweep: bool,
}

impl SvgArc {
    /// SVG 1.1 부록 F.6의 중심 표기 변환 뒤 90° 이하 조각마다 3차 곡선으로 근사한다
    fn to_cubics(&self, start: Point, end: Point) -> Vec<PathCommand> {
        let (mut rx, mut ry) = (self.radius_x.abs(), self.radius_y.abs());
        if rx == 0.0 || ry == 0.0 || (start.x == end.x && start.y == end.y) {
            return vec![PathCommand::LineTo(end)];
        }
        let phi = self.rotation_degrees.to_radians();
        let (sin_phi, cos_phi) = phi.sin_cos();
        let half_dx = (start.x - end.x) / 2.0;
        let half_dy = (start.y - end.y) / 2.0;
        let x1 = cos_phi * half_dx + sin_phi * half_dy;
        let y1 = -sin_phi * half_dx + cos_phi * half_dy;
        let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if lambda > 1.0 {
            rx *= lambda.sqrt();
            ry *= lambda.sqrt();
        }
        let numerator = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
        let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
        let sign = if self.is_large == self.is_sweep {
            -1.0
        } else {
            1.0
        };
        let factor = sign * (numerator / denominator).sqrt();
        let (cx1, cy1) = (factor * rx * y1 / ry, -factor * ry * x1 / rx);
        let center = Point::new(
            cos_phi * cx1 - sin_phi * cy1 + (start.x + end.x) / 2.0,
            sin_phi * cx1 + cos_phi * cy1 + (start.y + end.y) / 2.0,
        );
        let start_angle = vector_angle(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
        let mut sweep = vector_angle(
            (x1 - cx1) / rx,
            (y1 - cy1) / ry,
            (-x1 - cx1) / rx,
            (-y1 - cy1) / ry,
        );
        if !self.is_sweep && sweep > 0.0 {
            sweep -= std::f64::consts::TAU;
        } else if self.is_sweep && sweep < 0.0 {
            sweep += std::f64::consts::TAU;
        }
        let ellipse = ArcEllipse {
            center,
            rx,
            ry,
            sin_phi,
            cos_phi,
        };
        ellipse.cubics(start_angle, sweep, end)
    }
}

struct ArcEllipse {
    center: Point,
    rx: f64,
    ry: f64,
    sin_phi: f64,
    cos_phi: f64,
}

impl ArcEllipse {
    fn point(&self, angle: f64) -> Point {
        let (x, y) = (self.rx * angle.cos(), self.ry * angle.sin());
        Point::new(
            self.center.x + self.cos_phi * x - self.sin_phi * y,
            self.center.y + self.sin_phi * x + self.cos_phi * y,
        )
    }

    fn derivative(&self, angle: f64) -> Point {
        let (x, y) = (-self.rx * angle.sin(), self.ry * angle.cos());
        Point::new(
            self.cos_phi * x - self.sin_phi * y,
            self.sin_phi * x + self.cos_phi * y,
        )
    }

    fn cubics(&self, start_angle: f64, sweep: f64, end: Point) -> Vec<PathCommand> {
        let segments = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
        let step = sweep / segments as f64;
        let handle = 4.0 / 3.0 * (step / 4.0).tan();
        let mut commands = Vec::with_capacity(segments);
        for index in 0..segments {
            let from = start_angle + step * index as f64;
            let to = from + step;
            let (p0, p3) = (
                self.point(from),
                if index + 1 == segments {
                    end
                } else {
                    self.point(to)
                },
            );
            let (d0, d3) = (self.derivative(from), self.derivative(to));
            commands.push(PathCommand::CubicTo(
                Point::new(p0.x + handle * d0.x, p0.y + handle * d0.y),
                Point::new(p3.x - handle * d3.x, p3.y - handle * d3.y),
                p3,
            ));
        }
        commands
    }
}

fn vector_angle(ux: f64, uy: f64, vx: f64, vy: f64) -> f64 {
    let sign = if ux * vy - uy * vx < 0.0 { -1.0 } else { 1.0 };
    let dot = (ux * vx + uy * vy) / ((ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt());
    sign * dot.clamp(-1.0, 1.0).acos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 반원_호는_지름_끝에서_끝난다() {
        let arc = SvgArc {
            radius_x: 10.0,
            radius_y: 10.0,
            rotation_degrees: 0.0,
            is_large: false,
            is_sweep: true,
        };
        let commands = arc.to_cubics(Point::new(0.0, 0.0), Point::new(20.0, 0.0));
        let (_, min_y, _, _) =
            path_bounds(&[vec![PathCommand::MoveTo(Point::new(0.0, 0.0))], commands].concat())
                .unwrap();
        // 쓸기 방향 1은 화면 좌표에서 시계 방향이라 위로 볼록하다
        assert!((min_y + 10.0).abs() < 0.05, "{min_y}");
    }
}
