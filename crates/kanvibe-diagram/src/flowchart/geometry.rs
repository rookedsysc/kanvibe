//! 간선 경로 계산: 노드 교점, 클러스터 경계 자르기, 모서리 보정, d3 curveBasis, 화살표 자리 보정.

use crate::scene::{PathCommand, Point};

/// rendering-elements/intersect/intersect-rect.js
pub fn intersect_rect(center: Point, width: f64, height: f64, point: Point) -> Point {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    let mut w = width / 2.0;
    let mut h = height / 2.0;
    let (sx, sy) = if dy.abs() * w > dx.abs() * h {
        if dy < 0.0 {
            h = -h;
        }
        (if dy == 0.0 { 0.0 } else { h * dx / dy }, h)
    } else {
        if dx < 0.0 {
            w = -w;
        }
        (w, if dx == 0.0 { 0.0 } else { w * dy / dx })
    };
    Point::new(center.x + sx, center.y + sy)
}

pub fn intersect_ellipse(center: Point, radius_x: f64, radius_y: f64, point: Point) -> Point {
    let px = center.x - point.x;
    let py = center.y - point.y;
    let det = (radius_x * radius_x * py * py + radius_y * radius_y * px * px).sqrt();
    let mut dx = (radius_x * radius_y * px / det).abs();
    if point.x < center.x {
        dx = -dx;
    }
    let mut dy = (radius_x * radius_y * py / det).abs();
    if point.y < center.y {
        dy = -dy;
    }
    Point::new(center.x + dx, center.y + dy)
}

fn intersect_line(p1: Point, p2: Point, q1: Point, q2: Point) -> Option<Point> {
    let a1 = p2.y - p1.y;
    let b1 = p1.x - p2.x;
    let c1 = p2.x * p1.y - p1.x * p2.y;
    let r3 = a1 * q1.x + b1 * q1.y + c1;
    let r4 = a1 * q2.x + b1 * q2.y + c1;
    if r3 != 0.0 && r4 != 0.0 && r3 * r4 > 0.0 {
        return None;
    }
    let a2 = q2.y - q1.y;
    let b2 = q1.x - q2.x;
    let c2 = q2.x * q1.y - q1.x * q2.y;
    let r1 = a2 * p1.x + b2 * p1.y + c2;
    let r2 = a2 * p2.x + b2 * p2.y + c2;
    let epsilon = 1e-6;
    if r1.abs() < epsilon && r2.abs() < epsilon && r1 * r2 > 0.0 {
        return None;
    }
    let denom = a1 * b2 - a2 * b1;
    if denom == 0.0 {
        return None;
    }
    let offset = (denom / 2.0).abs();
    let mut num = b1 * c2 - b2 * c1;
    let x = if num < 0.0 {
        (num - offset) / denom
    } else {
        (num + offset) / denom
    };
    num = a2 * c1 - a1 * c2;
    let y = if num < 0.0 {
        (num - offset) / denom
    } else {
        (num + offset) / denom
    };
    Some(Point::new(x, y))
}

/// rendering-elements/intersect/intersect-polygon.js
pub fn intersect_polygon(
    center: Point,
    width: f64,
    height: f64,
    points: &[Point],
    point: Point,
) -> Point {
    let min_x = points
        .iter()
        .map(|entry| entry.x)
        .fold(f64::INFINITY, f64::min);
    let min_y = points
        .iter()
        .map(|entry| entry.y)
        .fold(f64::INFINITY, f64::min);
    let left = center.x - width / 2.0 - min_x;
    let top = center.y - height / 2.0 - min_y;
    let mut hits: Vec<Point> = Vec::new();
    for (index, first) in points.iter().enumerate() {
        let second = points[if index + 1 < points.len() {
            index + 1
        } else {
            0
        }];
        let edge_start = Point::new(left + first.x, top + first.y);
        let edge_end = Point::new(left + second.x, top + second.y);
        if let Some(hit) = intersect_line(center, point, edge_start, edge_end) {
            hits.push(hit);
        }
    }
    let distance = |hit: &Point| ((hit.x - point.x).powi(2) + (hit.y - point.y).powi(2)).sqrt();
    hits.sort_by(|left, right| {
        distance(left)
            .partial_cmp(&distance(right))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hits.first().copied().unwrap_or(center)
}

/// 상자 크기로만 판정하는 노드(클러스터) 경계
#[derive(Clone, Copy, Debug)]
pub struct BoundaryBox {
    pub center: Point,
    pub width: f64,
    pub height: f64,
}

fn is_outside(boundary: &BoundaryBox, point: Point) -> bool {
    let dx = (point.x - boundary.center.x).abs();
    let dy = (point.y - boundary.center.y).abs();
    dx >= boundary.width / 2.0 || dy >= boundary.height / 2.0
}

/// edges.js intersection(): 상자 밖 점과 안 점을 잇는 선분이 상자 경계와 만나는 점
fn boundary_intersection(boundary: &BoundaryBox, outside: Point, inside: Point) -> Point {
    let (x, y) = (boundary.center.x, boundary.center.y);
    let (w, h) = (boundary.width / 2.0, boundary.height / 2.0);
    let q_total = (outside.y - inside.y).abs();
    let r_total = (outside.x - inside.x).abs();
    let is_top_or_bottom = (y - outside.y).abs() * w > (x - outside.x).abs() * h;
    let (r, mut result) = if is_top_or_bottom {
        horizontal_edge_hit(boundary, outside, inside)
    } else {
        vertical_edge_hit(boundary, outside, inside)
    };
    if r == 0.0 {
        result = outside;
    }
    if r_total == 0.0 {
        result.x = outside.x;
    }
    if q_total == 0.0 {
        result.y = outside.y;
    }
    result
}

/// 위·아래 변과 만날 때
fn horizontal_edge_hit(boundary: &BoundaryBox, outside: Point, inside: Point) -> (f64, Point) {
    let (y, h) = (boundary.center.y, boundary.height / 2.0);
    let q_total = (outside.y - inside.y).abs();
    let r_total = (outside.x - inside.x).abs();
    let q = if inside.y < outside.y {
        outside.y - h - y
    } else {
        y - h - outside.y
    };
    let r = r_total * q / q_total;
    let hit = Point::new(
        if inside.x < outside.x {
            inside.x + r
        } else {
            inside.x - r_total + r
        },
        if inside.y < outside.y {
            inside.y + q_total - q
        } else {
            inside.y - q_total + q
        },
    );
    (r, hit)
}

/// 왼쪽·오른쪽 변과 만날 때
fn vertical_edge_hit(boundary: &BoundaryBox, outside: Point, inside: Point) -> (f64, Point) {
    let (x, w) = (boundary.center.x, boundary.width / 2.0);
    let q_total = (outside.y - inside.y).abs();
    let r_total = (outside.x - inside.x).abs();
    let r = if inside.x < outside.x {
        outside.x - w - x
    } else {
        x - w - outside.x
    };
    let q = q_total * r / r_total;
    let hit = Point::new(
        if inside.x < outside.x {
            inside.x + r_total - r
        } else {
            inside.x - r_total + r
        },
        if inside.y < outside.y {
            inside.y + q
        } else {
            inside.y - q
        },
    );
    (r, hit)
}

/// cutPathAtIntersect: 클러스터 안으로 들어가는 첫 점에서 경로를 자른다
pub fn cut_path_at_intersect(points: &[Point], boundary: &BoundaryBox) -> Vec<Point> {
    let mut result: Vec<Point> = Vec::new();
    let Some(first) = points.first() else {
        return result;
    };
    let mut last_outside = *first;
    let mut is_inside = false;
    for point in points {
        if !is_outside(boundary, *point) && !is_inside {
            let hit = boundary_intersection(boundary, last_outside, *point);
            if !result
                .iter()
                .any(|existing| existing.x == hit.x && existing.y == hit.y)
            {
                result.push(hit);
            }
            is_inside = true;
        } else {
            last_outside = *point;
            if !is_inside {
                result.push(*point);
            }
        }
    }
    result
}

fn find_adjacent_point(point_a: Point, point_b: Point, distance: f64) -> Point {
    let x_diff = point_b.x - point_a.x;
    let y_diff = point_b.y - point_a.y;
    let length = (x_diff * x_diff + y_diff * y_diff).sqrt();
    let ratio = distance / length;
    Point::new(point_b.x - ratio * x_diff, point_b.y - ratio * y_diff)
}

fn is_corner(previous: Point, current: Point, next: Point) -> bool {
    let vertical_then_horizontal = previous.x == current.x
        && current.y == next.y
        && (current.x - next.x).abs() > 5.0
        && (current.y - previous.y).abs() > 5.0;
    let horizontal_then_vertical = previous.y == current.y
        && current.x == next.x
        && (current.x - previous.x).abs() > 5.0
        && (current.y - next.y).abs() > 5.0;
    vertical_then_horizontal || horizontal_then_vertical
}

/// fixCorners: 직각으로 꺾이는 점을 반지름 5의 모서리로 바꾼다
pub fn fix_corners(points: &[Point]) -> Vec<Point> {
    let mut result = Vec::with_capacity(points.len());
    for (index, current) in points.iter().enumerate() {
        let is_inner = index > 0 && index + 1 < points.len();
        if !is_inner || !is_corner(points[index - 1], *current, points[index + 1]) {
            result.push(*current);
            continue;
        }
        let previous = points[index - 1];
        let next = points[index + 1];
        let new_previous = find_adjacent_point(previous, *current, 5.0);
        let new_next = find_adjacent_point(next, *current, 5.0);
        result.push(new_previous);
        result.push(rounded_corner(
            previous,
            *current,
            next,
            new_previous,
            new_next,
        ));
        result.push(new_next);
    }
    result
}

fn rounded_corner(
    previous: Point,
    corner: Point,
    next: Point,
    new_previous: Point,
    new_next: Point,
) -> Point {
    let x_diff = new_next.x - new_previous.x;
    let y_diff = new_next.y - new_previous.y;
    let a = 2.0_f64.sqrt() * 2.0;
    let radius = 5.0;
    if (next.x - previous.x).abs() <= 10.0 || (next.y - previous.y).abs() < 10.0 {
        return corner;
    }
    if corner.x == new_previous.x {
        Point::new(
            if x_diff < 0.0 {
                new_previous.x - radius + a
            } else {
                new_previous.x + radius - a
            },
            if y_diff < 0.0 {
                new_previous.y - a
            } else {
                new_previous.y + a
            },
        )
    } else {
        Point::new(
            if x_diff < 0.0 {
                new_previous.x - a
            } else {
                new_previous.x + a
            },
            if y_diff < 0.0 {
                new_previous.y - radius + a
            } else {
                new_previous.y + radius - a
            },
        )
    }
}

/// getLineFunctionsWithOffset: 화살표 자리만큼 양 끝점을 안으로 당긴다
pub fn apply_line_offsets(
    points: &[Point],
    start_offset: Option<f64>,
    end_offset: Option<f64>,
) -> Vec<Point> {
    let count = points.len();
    if count == 0 {
        return Vec::new();
    }
    let first = points[0];
    let last = points[count - 1];
    let direction_right = first.x >= last.x;
    let direction_up = first.y >= last.y;
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let x = offset_axis(
                points,
                index,
                *point,
                start_offset,
                end_offset,
                true,
                direction_right,
            );
            let y = offset_axis(
                points,
                index,
                *point,
                start_offset,
                end_offset,
                false,
                direction_up,
            );
            Point::new(x, y)
        })
        .collect()
}

/// JS `Math.atan(dy / dx)`와 같은 각(dx가 0이면 ±π/2, 0/0은 NaN)
fn atan_ratio(dx: f64, dy: f64) -> f64 {
    (dy / dx).atan()
}

fn offset_axis(
    points: &[Point],
    index: usize,
    point: Point,
    start_offset: Option<f64>,
    end_offset: Option<f64>,
    is_x: bool,
    is_reversed: bool,
) -> f64 {
    let count = points.len();
    let axis = |p: Point| if is_x { p.x } else { p.y };
    let cross = |p: Point| if is_x { p.y } else { p.x };
    let mut offset = 0.0;
    if index == 0
        && count > 1
        && let Some(start) = start_offset
    {
        offset = endpoint_offset(points[0], points[1], start, is_x);
    } else if index == count - 1
        && count > 1
        && let Some(end) = end_offset
    {
        offset = endpoint_offset(points[count - 1], points[count - 2], end, is_x);
    }
    let last = points[count - 1];
    let first = points[0];
    let to_end = (axis(point) - axis(last)).abs();
    let cross_end = (cross(point) - cross(last)).abs();
    let to_start = (axis(point) - axis(first)).abs();
    let cross_start = (cross(point) - cross(first)).abs();
    let sign = if is_reversed { -1.0 } else { 1.0 };
    if let Some(end) = end_offset
        && to_end < end
        && to_end > 0.0
        && cross_end < end
    {
        offset -= (end + 1.0 - to_end) * sign;
    }
    if let Some(start) = start_offset
        && to_start < start
        && to_start > 0.0
        && cross_start < start
    {
        offset += (start + 1.0 - to_start) * sign;
    }
    axis(point) + offset
}

fn endpoint_offset(from: Point, toward: Point, amount: f64, is_x: bool) -> f64 {
    let delta_x = toward.x - from.x;
    let delta_y = toward.y - from.y;
    let angle = atan_ratio(delta_x, delta_y);
    if is_x {
        amount * angle.cos() * if delta_x >= 0.0 { 1.0 } else { -1.0 }
    } else {
        amount * angle.sin().abs() * if delta_y >= 0.0 { 1.0 } else { -1.0 }
    }
}

/// d3 curveBasis
pub fn curve_basis(points: &[Point]) -> Vec<PathCommand> {
    let mut commands = Vec::new();
    let Some(first) = points.first() else {
        return commands;
    };
    commands.push(PathCommand::MoveTo(*first));
    if points.len() == 1 {
        return commands;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::NAN, f64::NAN, first.x, first.y);
    for (index, point) in points.iter().enumerate().skip(1) {
        if index == 2 {
            commands.push(PathCommand::LineTo(Point::new(
                (5.0 * x0 + x1) / 6.0,
                (5.0 * y0 + y1) / 6.0,
            )));
        }
        if index >= 2 {
            commands.push(basis_segment(x0, y0, x1, y1, point.x, point.y));
        }
        (x0, y0, x1, y1) = (x1, y1, point.x, point.y);
    }
    if points.len() >= 3 {
        commands.push(basis_segment(x0, y0, x1, y1, x1, y1));
    }
    commands.push(PathCommand::LineTo(Point::new(x1, y1)));
    commands
}

fn basis_segment(x0: f64, y0: f64, x1: f64, y1: f64, x: f64, y: f64) -> PathCommand {
    PathCommand::CubicTo(
        Point::new((2.0 * x0 + x1) / 3.0, (2.0 * y0 + y1) / 3.0),
        Point::new((x0 + 2.0 * x1) / 3.0, (y0 + 2.0 * y1) / 3.0),
        Point::new((x0 + 4.0 * x1 + x) / 6.0, (y0 + 4.0 * y1 + y) / 6.0),
    )
}

/// utils.calcLabelPosition: 꺾은선 길이의 가운데 점
pub fn polyline_midpoint(points: &[Point]) -> Point {
    if points.len() == 1 {
        return points[0];
    }
    let distance = |a: Point, b: Point| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    let total: f64 = points
        .windows(2)
        .map(|pair| distance(pair[0], pair[1]))
        .sum();
    let mut remaining = total / 2.0;
    for pair in points.windows(2) {
        let segment = distance(pair[0], pair[1]);
        if segment == 0.0 {
            return pair[0];
        }
        if segment < remaining {
            remaining -= segment;
            continue;
        }
        let ratio = remaining / segment;
        if ratio <= 0.0 {
            return pair[0];
        }
        if ratio >= 1.0 {
            return pair[1];
        }
        let round = |value: f64| (value * 100_000.0).round() / 100_000.0;
        return Point::new(
            round((1.0 - ratio) * pair[0].x + ratio * pair[1].x),
            round((1.0 - ratio) * pair[0].y + ratio * pair[1].y),
        );
    }
    points[points.len() - 1]
}

/// 경로의 접선 방향(라디안). 끝점 쪽이면 마지막 두 점, 시작이면 처음 두 점을 쓴다
pub fn end_tangent(commands: &[PathCommand], at_end: bool) -> Option<(Point, f64)> {
    let mut points: Vec<Point> = Vec::new();
    for command in commands {
        match command {
            PathCommand::MoveTo(point) | PathCommand::LineTo(point) => points.push(*point),
            PathCommand::QuadTo(control, point) => points.extend([*control, *point]),
            PathCommand::CubicTo(first, second, point) => points.extend([*first, *second, *point]),
            PathCommand::Close => {}
        }
    }
    points.dedup_by(|a, b| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
    if points.len() < 2 {
        return None;
    }
    let (from, to, anchor) = if at_end {
        (
            points[points.len() - 2],
            points[points.len() - 1],
            points[points.len() - 1],
        )
    } else {
        (points[0], points[1], points[0])
    };
    Some((anchor, (to.y - from.y).atan2(to.x - from.x)))
}

/// 경로의 기하 bbox(SVG getBBox처럼 베지어 극값까지)
pub fn path_bounds(commands: &[PathCommand]) -> Option<(f64, f64, f64, f64)> {
    let mut points: Vec<Point> = Vec::new();
    let mut current = Point::new(0.0, 0.0);
    for command in commands {
        match command {
            PathCommand::MoveTo(point) | PathCommand::LineTo(point) => points.push(*point),
            PathCommand::QuadTo(control, point) => {
                points.extend(quad_samples(current, *control, *point))
            }
            PathCommand::CubicTo(first, second, point) => {
                let extrema = cubic_extrema(current, *first, *second, *point);
                points.extend(
                    extrema
                        .into_iter()
                        .map(|t| cubic_at(current, *first, *second, *point, t)),
                );
                points.push(*point);
            }
            PathCommand::Close => continue,
        }
        current = points.last().copied().unwrap_or(current);
    }
    let first = points.first()?;
    let initial = (first.x, first.y, first.x, first.y);
    Some(
        points
            .iter()
            .fold(initial, |(min_x, min_y, max_x, max_y), point| {
                (
                    min_x.min(point.x),
                    min_y.min(point.y),
                    max_x.max(point.x),
                    max_y.max(point.y),
                )
            }),
    )
}

/// 2차 베지어를 17점으로 훑는다(끝점 포함)
fn quad_samples(start: Point, control: Point, end: Point) -> Vec<Point> {
    (0..=16)
        .map(|step| {
            let t = f64::from(step) / 16.0;
            let u = 1.0 - t;
            Point::new(
                u * u * start.x + 2.0 * u * t * control.x + t * t * end.x,
                u * u * start.y + 2.0 * u * t * control.y + t * t * end.y,
            )
        })
        .collect()
}

fn cubic_at(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let blend = |a: f64, b: f64, c: f64, d: f64| {
        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
    };
    Point::new(blend(p0.x, p1.x, p2.x, p3.x), blend(p0.y, p1.y, p2.y, p3.y))
}

/// 3차 베지어 도함수가 0이 되는 t(축별)
fn cubic_extrema(p0: Point, p1: Point, p2: Point, p3: Point) -> Vec<f64> {
    let mut result = Vec::new();
    for (a0, a1, a2, a3) in [(p0.x, p1.x, p2.x, p3.x), (p0.y, p1.y, p2.y, p3.y)] {
        let a = -a0 + 3.0 * a1 - 3.0 * a2 + a3;
        let b = 2.0 * (a0 - 2.0 * a1 + a2);
        let c = a1 - a0;
        if a.abs() < 1e-12 {
            if b.abs() > 1e-12 {
                result.push(-c / b);
            }
            continue;
        }
        let discriminant = b * b - 4.0 * a * c;
        if discriminant < 0.0 {
            continue;
        }
        let root = discriminant.sqrt();
        result.push((-b + root) / (2.0 * a));
        result.push((-b - root) / (2.0 * a));
    }
    result.retain(|t| *t > 0.0 && *t < 1.0);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기준_svg와_같은_곡선을_만든다() {
        let points = [
            Point::new(43.203, 62.0),
            Point::new(43.203, 87.0),
            Point::new(43.203, 112.0),
        ];
        let offset = apply_line_offsets(&points, None, Some(4.0));
        assert!((offset[2].y - 108.0).abs() < 1e-9);
        let commands = curve_basis(&offset);
        assert_eq!(commands.len(), 5);
        let PathCommand::LineTo(second) = commands[1] else {
            panic!("lineTo")
        };
        assert!((second.y - 66.1666).abs() < 1e-3);
    }
}
