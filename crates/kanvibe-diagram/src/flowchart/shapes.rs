//! Mermaid 11 `rendering-elements/shapes`의 classic 모양 크기·윤곽선·간선 교점.
//!
//! 크기는 라벨 상자(bbox)와 노드 padding(flowchart 기본 15)에서 원본과 같은 식으로 구한다. 윤곽선 좌표는
//! 노드 중심 기준이다.

use super::parser::NodeShape;
use crate::scene::{PathCommand, Point, ellipse, polygon, rounded_rect};

/// 노드의 크기·그림·교점 규칙
#[derive(Clone, Debug)]
pub struct NodeGeometry {
    /// dagre에 넘기는 크기(원본의 getBBox 결과)
    pub width: f64,
    pub height: f64,
    /// 채우고 테두리를 그릴 경로들(그리는 순서대로)
    pub outlines: Vec<Vec<PathCommand>>,
    pub intersect: Intersect,
    /// 라벨 중심을 노드 중심에서 아래로 옮길 거리(원기둥)
    pub label_offset_y: f64,
}

#[derive(Clone, Debug)]
pub enum Intersect {
    Rect,
    Circle(f64),
    /// 원본 intersectPolygon에 넘기는 점(모양 자체 좌표)과 결과 보정값
    Polygon {
        points: Vec<Point>,
        correction: f64,
    },
    Cylinder {
        radius_x: f64,
        radius_y: f64,
    },
}

pub fn node_geometry(
    shape: NodeShape,
    label_width: f64,
    label_height: f64,
    padding: f64,
) -> NodeGeometry {
    match shape {
        NodeShape::Rect => rect_geometry(
            label_width + padding * 4.0,
            label_height + padding * 2.0,
            0.0,
        ),
        NodeShape::Rounded => rect_geometry(
            label_width + padding * 2.0,
            label_height + padding * 2.0,
            5.0,
        ),
        NodeShape::Stadium => stadium_geometry(label_width, label_height, padding),
        NodeShape::Subroutine => subroutine_geometry(label_width, label_height, padding),
        NodeShape::Cylinder => cylinder_geometry(label_width, label_height, padding),
        NodeShape::Circle => circle_geometry(label_width / 2.0 + padding / 2.0, None),
        NodeShape::DoubleCircle => circle_geometry(label_width / 2.0 + padding, Some(5.0)),
        NodeShape::Diamond => diamond_geometry(label_width, label_height, padding),
        NodeShape::Hexagon => hexagon_geometry(label_width, label_height, padding),
        NodeShape::LeanRight
        | NodeShape::LeanLeft
        | NodeShape::Trapezoid
        | NodeShape::InvTrapezoid => slanted_geometry(shape, label_width, label_height, padding),
        NodeShape::Odd => odd_geometry(label_width, label_height, padding),
    }
}

fn rect_geometry(width: f64, height: f64, radius: f64) -> NodeGeometry {
    NodeGeometry {
        width,
        height,
        outlines: vec![rounded_rect(
            -width / 2.0,
            -height / 2.0,
            width,
            height,
            radius,
        )],
        intersect: Intersect::Rect,
        label_offset_y: 0.0,
    }
}

/// insertPolygonShape: 점을 translate(-w/2, h/2)로 옮겨 그린다. 크기는 점들의 bbox다
fn polygon_geometry(points: Vec<Point>, translate: Point) -> NodeGeometry {
    let (min_x, max_x, min_y, max_y) = bounds(&points);
    let drawn: Vec<Point> = points
        .iter()
        .map(|point| Point::new(point.x + translate.x, point.y + translate.y))
        .collect();
    NodeGeometry {
        width: max_x - min_x,
        height: max_y - min_y,
        outlines: vec![polygon(&drawn)],
        intersect: Intersect::Polygon {
            points,
            correction: 0.0,
        },
        label_offset_y: 0.0,
    }
}

fn bounds(points: &[Point]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |acc, point| {
            (
                acc.0.min(point.x),
                acc.1.max(point.x),
                acc.2.min(point.y),
                acc.3.max(point.y),
            )
        },
    )
}

/// generateCirclePoints: 원본처럼 부호를 뒤집은 점을 만든다
fn circle_points(
    center_x: f64,
    center_y: f64,
    radius: f64,
    start_degree: f64,
    end_degree: f64,
) -> Vec<Point> {
    const POINT_COUNT: usize = 50;
    let start = start_degree.to_radians();
    let step = (end_degree.to_radians() - start) / (POINT_COUNT - 1) as f64;
    (0..POINT_COUNT)
        .map(|index| {
            let angle = start + index as f64 * step;
            Point::new(
                -(center_x + radius * angle.cos()),
                -(center_y + radius * angle.sin()),
            )
        })
        .collect()
}

fn stadium_geometry(label_width: f64, label_height: f64, padding: f64) -> NodeGeometry {
    let height = label_height + padding;
    let width = label_width + height / 4.0 + padding;
    let radius = height / 2.0;
    let mut points = vec![
        Point::new(-width / 2.0 + radius, -height / 2.0),
        Point::new(width / 2.0 - radius, -height / 2.0),
    ];
    points.extend(circle_points(
        -width / 2.0 + radius,
        0.0,
        radius,
        90.0,
        270.0,
    ));
    points.push(Point::new(width / 2.0 - radius, height / 2.0));
    points.extend(circle_points(
        width / 2.0 - radius,
        0.0,
        radius,
        270.0,
        450.0,
    ));
    NodeGeometry {
        width,
        height,
        outlines: vec![polygon(&points)],
        intersect: Intersect::Polygon {
            points,
            correction: 0.0,
        },
        label_offset_y: 0.0,
    }
}

const FRAME_WIDTH: f64 = 8.0;

fn subroutine_geometry(label_width: f64, label_height: f64, padding: f64) -> NodeGeometry {
    let width = label_width + padding;
    let height = label_height + padding;
    let corner = |x: f64, y: f64| Point::new(x, y);
    let points = vec![
        corner(0.0, 0.0),
        corner(width, 0.0),
        corner(width, -height),
        corner(0.0, -height),
        corner(0.0, 0.0),
        corner(-FRAME_WIDTH, 0.0),
        corner(width + FRAME_WIDTH, 0.0),
        corner(width + FRAME_WIDTH, -height),
        corner(-FRAME_WIDTH, -height),
        corner(-FRAME_WIDTH, 0.0),
    ];
    polygon_geometry(points, Point::new(-width / 2.0, height / 2.0))
}

fn cylinder_geometry(label_width: f64, label_height: f64, padding: f64) -> NodeGeometry {
    let width = label_width + padding;
    let radius_x = width / 2.0;
    let radius_y = radius_x / (2.5 + width / 50.0);
    let height = label_height + padding + radius_y;
    let (left, top) = (-width / 2.0, -(height / 2.0 + radius_y));
    let top_ellipse = ellipse(Point::new(0.0, top + radius_y), radius_x, radius_y);
    let body = vec![
        PathCommand::MoveTo(Point::new(left, top + radius_y)),
        PathCommand::LineTo(Point::new(left, top + radius_y + height)),
        bottom_arc(left, top + radius_y + height, radius_x, radius_y),
        PathCommand::LineTo(Point::new(-left, top + radius_y)),
    ];
    let mut outline = body;
    outline.extend(top_ellipse);
    NodeGeometry {
        width,
        height: height + 2.0 * radius_y,
        outlines: vec![outline],
        intersect: Intersect::Cylinder { radius_x, radius_y },
        label_offset_y: padding / 1.5,
    }
}

/// 왼쪽 점에서 오른쪽 점까지 아래로 볼록한 반타원(3차 베지어 두 개 대신 근사 상수 하나로)
fn bottom_arc(left: f64, y: f64, radius_x: f64, radius_y: f64) -> PathCommand {
    let k = 4.0 / 3.0;
    PathCommand::CubicTo(
        Point::new(left, y + radius_y * k),
        Point::new(left + 2.0 * radius_x, y + radius_y * k),
        Point::new(left + 2.0 * radius_x, y),
    )
}

fn circle_geometry(radius: f64, inner_gap: Option<f64>) -> NodeGeometry {
    let mut outlines = vec![ellipse(Point::new(0.0, 0.0), radius, radius)];
    if let Some(gap) = inner_gap {
        outlines.push(ellipse(Point::new(0.0, 0.0), radius - gap, radius - gap));
    }
    NodeGeometry {
        width: radius * 2.0,
        height: radius * 2.0,
        outlines,
        intersect: Intersect::Circle(radius),
        label_offset_y: 0.0,
    }
}

fn diamond_geometry(label_width: f64, label_height: f64, padding: f64) -> NodeGeometry {
    let size = label_width + padding + label_height + padding;
    let points = vec![
        Point::new(size / 2.0, 0.0),
        Point::new(size, -size / 2.0),
        Point::new(size / 2.0, -size),
        Point::new(0.0, -size / 2.0),
    ];
    let mut geometry = polygon_geometry(points.clone(), Point::new(-size / 2.0 + 0.5, size / 2.0));
    geometry.intersect = Intersect::Polygon {
        points,
        correction: -0.5,
    };
    geometry
}

fn hexagon_geometry(label_width: f64, label_height: f64, padding: f64) -> NodeGeometry {
    let height = label_height + padding;
    let notch = height / 4.0;
    let width = label_width + 2.0 * notch + padding;
    let points = vec![
        Point::new(notch, 0.0),
        Point::new(width - notch, 0.0),
        Point::new(width, -height / 2.0),
        Point::new(width - notch, -height),
        Point::new(notch, -height),
        Point::new(0.0, -height / 2.0),
    ];
    polygon_geometry(points, Point::new(-width / 2.0, height / 2.0))
}

fn slanted_geometry(
    shape: NodeShape,
    label_width: f64,
    label_height: f64,
    padding: f64,
) -> NodeGeometry {
    let height = label_height + padding;
    let width = label_width + padding;
    let slant = 3.0 * height / 6.0;
    let points = match shape {
        NodeShape::LeanRight => vec![
            Point::new(-slant, 0.0),
            Point::new(width, 0.0),
            Point::new(width + slant, -height),
            Point::new(0.0, -height),
        ],
        NodeShape::LeanLeft => vec![
            Point::new(0.0, 0.0),
            Point::new(width + slant, 0.0),
            Point::new(width, -height),
            Point::new(-slant, -height),
        ],
        NodeShape::Trapezoid => vec![
            Point::new(-slant, 0.0),
            Point::new(width + slant, 0.0),
            Point::new(width, -height),
            Point::new(0.0, -height),
        ],
        _ => vec![
            Point::new(0.0, 0.0),
            Point::new(width, 0.0),
            Point::new(width + slant, -height),
            Point::new(-slant, -height),
        ],
    };
    polygon_geometry(points, Point::new(-width / 2.0, height / 2.0))
}

fn odd_geometry(label_width: f64, label_height: f64, padding: f64) -> NodeGeometry {
    let width = label_width + padding;
    let height = label_height + padding;
    let (x, y) = (-width / 2.0, -height / 2.0);
    let notch = y / 2.0;
    let points = vec![
        Point::new(x + notch, y),
        Point::new(x, 0.0),
        Point::new(x + notch, -y),
        Point::new(-x, -y),
        Point::new(-x, y),
    ];
    let (min_x, max_x, min_y, max_y) = bounds(&points);
    NodeGeometry {
        width: max_x - min_x,
        height: max_y - min_y,
        outlines: vec![polygon(&points)],
        intersect: Intersect::Polygon {
            points,
            correction: 0.0,
        },
        label_offset_y: 0.0,
    }
}

impl NodeGeometry {
    /// 중심이 `center`인 노드 경계와 `point` 방향 선분의 교점
    pub fn intersect(&self, center: Point, point: Point) -> Point {
        match &self.intersect {
            Intersect::Rect => {
                crate::flowchart::geometry::intersect_rect(center, self.width, self.height, point)
            }
            Intersect::Circle(radius) => {
                crate::flowchart::geometry::intersect_ellipse(center, *radius, *radius, point)
            }
            Intersect::Polygon { points, correction } => {
                let hit = crate::flowchart::geometry::intersect_polygon(
                    center,
                    self.width,
                    self.height,
                    points,
                    point,
                );
                Point::new(hit.x + correction, hit.y + correction)
            }
            Intersect::Cylinder { radius_x, radius_y } => {
                self.intersect_cylinder(center, point, *radius_x, *radius_y)
            }
        }
    }

    fn intersect_cylinder(
        &self,
        center: Point,
        point: Point,
        radius_x: f64,
        radius_y: f64,
    ) -> Point {
        let mut hit =
            crate::flowchart::geometry::intersect_rect(center, self.width, self.height, point);
        let x = hit.x - center.x;
        let is_on_cap = x.abs() < self.width / 2.0
            || (x.abs() == self.width / 2.0
                && (hit.y - center.y).abs() > self.height / 2.0 - radius_y);
        if radius_x != 0.0 && is_on_cap {
            let mut y = radius_y * radius_y * (1.0 - x * x / (radius_x * radius_x));
            if y > 0.0 {
                y = y.sqrt();
            }
            y = radius_y - y;
            if point.y - center.y > 0.0 {
                y = -y;
            }
            hit.y += y;
        }
        hit
    }
}
