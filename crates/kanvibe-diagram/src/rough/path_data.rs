//! path-data-parser의 normalize(H·V·A를 L·C로)와 points-on-path·points-on-curve(곡선을 점으로 펴고 줄이기).
//! 입력은 이미 절대 좌표인 구간이다(Mermaid가 넘기는 경로는 상대 명령을 쓰지 않는다).

use std::f64::consts::PI;

use crate::scene::Point;

/// 절대 좌표 경로 구간
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    MoveTo(Point),
    LineTo(Point),
    Horizontal(f64),
    Vertical(f64),
    CubicTo(Point, Point, Point),
    Arc {
        radius: Point,
        rotation: f64,
        is_large: bool,
        is_sweep: bool,
        end: Point,
    },
    Close,
}

/// normalize: M·L·C·Z만 남긴다
pub fn normalize(segments: &[Segment]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut current = Point::new(0.0, 0.0);
    let mut subpath_start = current;
    for segment in segments {
        match *segment {
            Segment::MoveTo(point) => {
                out.push(Segment::MoveTo(point));
                current = point;
                subpath_start = point;
            }
            Segment::LineTo(point) => {
                out.push(Segment::LineTo(point));
                current = point;
            }
            Segment::Horizontal(x) => {
                current = Point::new(x, current.y);
                out.push(Segment::LineTo(current));
            }
            Segment::Vertical(y) => {
                current = Point::new(current.x, y);
                out.push(Segment::LineTo(current));
            }
            Segment::CubicTo(first, second, end) => {
                out.push(Segment::CubicTo(first, second, end));
                current = end;
            }
            Segment::Arc { end, .. } => {
                out.extend(arc_segments(current, segment));
                current = end;
            }
            Segment::Close => {
                out.push(Segment::Close);
                current = subpath_start;
            }
        }
    }
    out
}

struct ArcSpec {
    radius: Point,
    rotation: f64,
    is_large: bool,
    is_sweep: bool,
}

/// 원호 하나: 반지름이 0이면 직선 같은 곡선, 시작과 끝이 같으면 아무것도 없다
fn arc_segments(start: Point, segment: &Segment) -> Vec<Segment> {
    let Segment::Arc {
        radius,
        rotation,
        is_large,
        is_sweep,
        end,
    } = *segment
    else {
        return Vec::new();
    };
    let radius = Point::new(radius.x.abs(), radius.y.abs());
    let arc = ArcSpec {
        radius,
        rotation,
        is_large,
        is_sweep,
    };
    if arc.radius.x == 0.0 || arc.radius.y == 0.0 {
        return vec![Segment::CubicTo(start, end, end)];
    }
    if start == end {
        return Vec::new();
    }
    let curves = arc_to_cubic_curves(start, end, &arc);
    curves
        .chunks(3)
        .map(|chunk| Segment::CubicTo(chunk[0], chunk[1], chunk[2]))
        .collect()
}

fn rotate(point: Point, angle: f64) -> Point {
    Point::new(
        point.x * angle.cos() - point.y * angle.sin(),
        point.x * angle.sin() + point.y * angle.cos(),
    )
}

/// JavaScript `parseFloat(value.toFixed(9))`
fn round_to_nine(value: f64) -> f64 {
    format!("{value:.9}").parse().unwrap_or(value)
}

/// 원호의 중심과 시작·끝 각(arcToCubicCurves의 처음 부분)
fn arc_center(start: Point, end: Point, arc: &ArcSpec) -> (Point, Point, f64, f64) {
    let (mut r1, mut r2) = (arc.radius.x, arc.radius.y);
    let x = (start.x - end.x) / 2.0;
    let y = (start.y - end.y) / 2.0;
    let mut h = (x * x) / (r1 * r1) + (y * y) / (r2 * r2);
    if h > 1.0 {
        h = h.sqrt();
        r1 *= h;
        r2 *= h;
    }
    let sign = if arc.is_large == arc.is_sweep {
        -1.0
    } else {
        1.0
    };
    let (r1_pow, r2_pow) = (r1 * r1, r2 * r2);
    let left = r1_pow * r2_pow - r1_pow * y * y - r2_pow * x * x;
    let right = r1_pow * y * y + r2_pow * x * x;
    let k = sign * (left / right).abs().sqrt();
    let center = Point::new(
        k * r1 * y / r2 + (start.x + end.x) / 2.0,
        k * -r2 * x / r1 + (start.y + end.y) / 2.0,
    );
    let angle_of = |point: Point| {
        let mut angle = round_to_nine((point.y - center.y) / r2).asin();
        if point.x < center.x {
            angle = PI - angle;
        }
        if angle < 0.0 {
            angle += PI * 2.0;
        }
        angle
    };
    (center, Point::new(r1, r2), angle_of(start), angle_of(end))
}

/// arcToCubicCurves: 120°를 넘는 원호는 나눠 3차 곡선 제어점 목록(점 세 개씩)으로 돌려준다
fn arc_to_cubic_curves(start: Point, end: Point, arc: &ArcSpec) -> Vec<Point> {
    let angle = PI * arc.rotation / 180.0;
    let start = rotate(start, -angle);
    let end = rotate(end, -angle);
    let (center, radius, mut f1, mut f2) = arc_center(start, end, arc);
    if arc.is_sweep && f1 > f2 {
        f1 -= PI * 2.0;
    }
    if !arc.is_sweep && f2 > f1 {
        f2 -= PI * 2.0;
    }
    let pieces = arc_pieces(start, end, (center, radius), (f1, f2), arc.is_sweep);
    pieces.iter().map(|point| rotate(*point, angle)).collect()
}

/// 나누어진 원호 조각들의 제어점(회전 전 좌표). 재귀 호출 부분
fn arc_pieces(
    start: Point,
    end: Point,
    (center, radius): (Point, Point),
    (f1, f2): (f64, f64),
    is_sweep: bool,
) -> Vec<Point> {
    let limit = PI * 120.0 / 180.0;
    let (mut end, mut f2, mut rest) = (end, f2, Vec::new());
    if (f2 - f1).abs() > limit {
        let (old_f2, old_end) = (f2, end);
        f2 = if is_sweep && f2 > f1 {
            f1 + limit
        } else {
            f1 - limit
        };
        end = Point::new(
            center.x + radius.x * f2.cos(),
            center.y + radius.y * f2.sin(),
        );
        rest = arc_pieces(end, old_end, (center, radius), (f2, old_f2), is_sweep);
    }
    let t = ((f2 - f1) / 4.0).tan();
    let hx = 4.0 / 3.0 * radius.x * t;
    let hy = 4.0 / 3.0 * radius.y * t;
    let near = Point::new(start.x + hx * f1.sin(), start.y - hy * f1.cos());
    let first = Point::new(2.0 * start.x - near.x, 2.0 * start.y - near.y);
    let second = Point::new(end.x + hx * f2.sin(), end.y - hy * f2.cos());
    let mut points = vec![first, second, end];
    points.extend(rest);
    points
}

/// pointsOnPath: 하위 경로마다 점 목록. 곡선은 허용 오차로 펴고, `distance`로 줄인다
pub fn points_on_path(segments: &[Segment], tolerance: f64, distance: f64) -> Vec<Vec<Point>> {
    let mut collector = PointCollector {
        sets: Vec::new(),
        current: Vec::new(),
        start: Point::new(0.0, 0.0),
        pending_curve: Vec::new(),
    };
    for segment in segments {
        match *segment {
            Segment::MoveTo(point) => {
                collector.flush_points(tolerance);
                collector.start = point;
                collector.current.push(point);
            }
            Segment::LineTo(point) => {
                collector.flush_curve(tolerance);
                collector.current.push(point);
            }
            Segment::CubicTo(first, second, end) => collector.push_curve(first, second, end),
            Segment::Close => {
                collector.flush_curve(tolerance);
                let start = collector.start;
                collector.current.push(start);
            }
            _ => {}
        }
    }
    collector.flush_points(tolerance);
    collector
        .sets
        .into_iter()
        .map(|set| simplify(&set, 0, set.len(), distance))
        .filter(|set| !set.is_empty())
        .collect()
}

struct PointCollector {
    sets: Vec<Vec<Point>>,
    current: Vec<Point>,
    start: Point,
    pending_curve: Vec<Point>,
}

impl PointCollector {
    fn push_curve(&mut self, first: Point, second: Point, end: Point) {
        if self.pending_curve.is_empty() {
            let last = self.current.last().copied().unwrap_or(self.start);
            self.pending_curve.push(last);
        }
        self.pending_curve.extend([first, second, end]);
    }

    fn flush_curve(&mut self, tolerance: f64) {
        if self.pending_curve.len() >= 4 {
            let points = points_on_bezier_curves(&self.pending_curve, tolerance);
            self.current.extend(points);
        }
        self.pending_curve.clear();
    }

    fn flush_points(&mut self, tolerance: f64) {
        self.flush_curve(tolerance);
        if !self.current.is_empty() {
            self.sets.push(std::mem::take(&mut self.current));
        }
    }
}

fn distance_sq(first: Point, second: Point) -> f64 {
    (first.x - second.x).powi(2) + (first.y - second.y).powi(2)
}

fn lerp(from: Point, to: Point, t: f64) -> Point {
    Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
}

fn distance_to_segment_sq(point: Point, from: Point, to: Point) -> f64 {
    let length_sq = distance_sq(from, to);
    if length_sq == 0.0 {
        return distance_sq(point, from);
    }
    let t =
        ((point.x - from.x) * (to.x - from.x) + (point.y - from.y) * (to.y - from.y)) / length_sq;
    distance_sq(point, lerp(from, to, t.clamp(0.0, 1.0)))
}

fn flatness(points: &[Point]) -> f64 {
    let [p1, p2, p3, p4] = [points[0], points[1], points[2], points[3]];
    let ux = (3.0 * p2.x - 2.0 * p1.x - p4.x).powi(2);
    let uy = (3.0 * p2.y - 2.0 * p1.y - p4.y).powi(2);
    let vx = (3.0 * p3.x - 2.0 * p4.x - p1.x).powi(2);
    let vy = (3.0 * p3.y - 2.0 * p4.y - p1.y).powi(2);
    ux.max(vx) + uy.max(vy)
}

/// getPointsOnBezierCurveWithSplitting: 충분히 평평할 때까지 반으로 나눈다
fn split_bezier(points: &[Point], tolerance: f64, out: &mut Vec<Point>) {
    if flatness(points) < tolerance {
        let start = points[0];
        let is_far = out
            .last()
            .is_none_or(|last| distance_sq(*last, start).sqrt() > 1.0);
        if is_far {
            out.push(start);
        }
        out.push(points[3]);
        return;
    }
    let [p1, p2, p3, p4] = [points[0], points[1], points[2], points[3]];
    let q1 = lerp(p1, p2, 0.5);
    let q2 = lerp(p2, p3, 0.5);
    let q3 = lerp(p3, p4, 0.5);
    let r1 = lerp(q1, q2, 0.5);
    let r2 = lerp(q2, q3, 0.5);
    let middle = lerp(r1, r2, 0.5);
    split_bezier(&[p1, q1, r1, middle], tolerance, out);
    split_bezier(&[middle, r2, q3, p4], tolerance, out);
}

/// pointsOnBezierCurves(거리 줄이기 없이)
pub fn points_on_bezier_curves(points: &[Point], tolerance: f64) -> Vec<Point> {
    let mut out = Vec::new();
    let segment_count = (points.len() - 1) / 3;
    for index in 0..segment_count {
        split_bezier(&points[index * 3..index * 3 + 4], tolerance, &mut out);
    }
    out
}

/// Ramer–Douglas–Peucker(simplifyPoints)
fn simplify(points: &[Point], start: usize, end: usize, epsilon: f64) -> Vec<Point> {
    let mut out = Vec::new();
    simplify_into(points, start, end, epsilon, &mut out);
    out
}

fn simplify_into(points: &[Point], start: usize, end: usize, epsilon: f64, out: &mut Vec<Point>) {
    let (first, last) = (points[start], points[end - 1]);
    let mut max_distance_sq = 0.0;
    let mut max_index = 1;
    for (index, point) in points.iter().enumerate().take(end - 1).skip(start + 1) {
        let distance = distance_to_segment_sq(*point, first, last);
        if distance > max_distance_sq {
            max_distance_sq = distance;
            max_index = index;
        }
    }
    if max_distance_sq.sqrt() > epsilon {
        simplify_into(points, start, max_index + 1, epsilon, out);
        simplify_into(points, max_index, end, epsilon, out);
    } else {
        if out.is_empty() {
            out.push(first);
        }
        out.push(last);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 수평_수직_원호를_직선과_3차_곡선으로_바꾼다() {
        let arc = Segment::Arc {
            radius: Point::new(5.0, 5.0),
            rotation: 0.0,
            is_large: false,
            is_sweep: true,
            end: Point::new(10.0, 5.0),
        };
        let normalized = normalize(&[
            Segment::MoveTo(Point::new(0.0, 0.0)),
            Segment::Horizontal(5.0),
            arc,
            Segment::Vertical(20.0),
        ]);
        assert_eq!(normalized[1], Segment::LineTo(Point::new(5.0, 0.0)));
        let Segment::CubicTo(_, _, end) = normalized[2] else {
            panic!("곡선");
        };
        assert!((end.x - 10.0).abs() < 1e-9 && (end.y - 5.0).abs() < 1e-9);
        assert_eq!(normalized[3], Segment::LineTo(Point::new(10.0, 20.0)));
    }
}
