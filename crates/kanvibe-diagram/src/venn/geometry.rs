//! venn.js `circleintersection.js`: 원 여러 개의 겹친 넓이와 그 경계 호.
//!
//! 겹친 영역은 모든 원 안에 드는 원끼리의 교점을 중심 각도로 줄 세우고, 이웃 교점 사이마다 가장 좁은 호를
//! 고른다. 교점이 없으면 가장 작은 원이 다른 원 모두에 들어 있는지로 완전 포함과 서로 떨어짐을 가른다.

use std::f64::consts::PI;

const SMALL: f64 = 1e-10;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
}

/// 원 둘의 교점. `parents`는 그 교점을 만든 두 원의 번호다
#[derive(Clone, Copy, Debug)]
struct IntersectionPoint {
    x: f64,
    y: f64,
    parents: [usize; 2],
    angle: f64,
}

/// 겹친 영역 경계의 호 하나: `circle` 위에서 `p2`부터 `p1`까지
#[derive(Clone, Copy, Debug)]
pub struct Arc {
    pub circle: Circle,
    pub width: f64,
    pub p1: (f64, f64),
    pub p2: (f64, f64),
}

pub fn distance(first: (f64, f64), second: (f64, f64)) -> f64 {
    ((first.0 - second.0) * (first.0 - second.0) + (first.1 - second.1) * (first.1 - second.1))
        .sqrt()
}

fn center_of(circle: &Circle) -> (f64, f64) {
    (circle.x, circle.y)
}

/// 원의 활꼴 넓이(반지름 `radius`, 높이 `width`)
pub fn circle_area(radius: f64, width: f64) -> f64 {
    radius * radius * (1.0 - width / radius).acos()
        - (radius - width) * (width * (2.0 * radius - width)).sqrt()
}

/// 중심 거리가 `gap`인 두 원의 겹친 넓이
pub fn circle_overlap(first_radius: f64, second_radius: f64, gap: f64) -> f64 {
    if gap >= first_radius + second_radius {
        return 0.0;
    }
    if gap <= (first_radius - second_radius).abs() {
        let smaller = first_radius.min(second_radius);
        return PI * smaller * smaller;
    }
    let first_width = first_radius
        - (gap * gap - second_radius * second_radius + first_radius * first_radius) / (2.0 * gap);
    let second_width = second_radius
        - (gap * gap - first_radius * first_radius + second_radius * second_radius) / (2.0 * gap);
    circle_area(first_radius, first_width) + circle_area(second_radius, second_width)
}

/// 두 원의 교점(접하거나 한쪽이 다른 쪽을 품으면 없다)
pub fn circle_circle_intersection(first: &Circle, second: &Circle) -> Vec<(f64, f64)> {
    let gap = distance(center_of(first), center_of(second));
    let (r1, r2) = (first.radius, second.radius);
    if gap >= r1 + r2 || gap <= (r1 - r2).abs() {
        return Vec::new();
    }
    let along = (r1 * r1 - r2 * r2 + gap * gap) / (2.0 * gap);
    let height = (r1 * r1 - along * along).sqrt();
    let x0 = first.x + along * (second.x - first.x) / gap;
    let y0 = first.y + along * (second.y - first.y) / gap;
    let rx = -(second.y - first.y) * (height / gap);
    let ry = -(second.x - first.x) * (height / gap);
    vec![(x0 + rx, y0 - ry), (x0 - rx, y0 + ry)]
}

pub fn center_of_points(points: &[(f64, f64)]) -> (f64, f64) {
    let count = points.len() as f64;
    let (sum_x, sum_y) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), point| (x + point.0, y + point.1));
    (sum_x / count, sum_y / count)
}

fn intersection_points(circles: &[Circle]) -> Vec<IntersectionPoint> {
    let mut points = Vec::new();
    for first in 0..circles.len() {
        for second in first + 1..circles.len() {
            for (x, y) in circle_circle_intersection(&circles[first], &circles[second]) {
                points.push(IntersectionPoint {
                    x,
                    y,
                    parents: [first, second],
                    angle: 0.0,
                });
            }
        }
    }
    points
}

fn is_inside_all(point: (f64, f64), circles: &[Circle]) -> bool {
    circles
        .iter()
        .all(|circle| distance(point, center_of(circle)) < circle.radius + SMALL)
}

/// 겹친 넓이와 경계 호
pub fn intersection_area(circles: &[Circle]) -> (f64, Vec<Arc>) {
    let mut inner: Vec<IntersectionPoint> = intersection_points(circles)
        .into_iter()
        .filter(|point| is_inside_all((point.x, point.y), circles))
        .collect();
    if inner.len() <= 1 {
        return contained_area(circles);
    }
    let coordinates: Vec<(f64, f64)> = inner.iter().map(|point| (point.x, point.y)).collect();
    let center = center_of_points(&coordinates);
    for point in &mut inner {
        point.angle = (point.x - center.0).atan2(point.y - center.1);
    }
    inner.sort_by(|a, b| b.angle.total_cmp(&a.angle));
    let mut arcs = Vec::new();
    let mut arc_area = 0.0;
    let mut polygon_area = 0.0;
    let mut previous = inner[inner.len() - 1];
    for current in &inner {
        polygon_area += (previous.x + current.x) * (current.y - previous.y);
        if let Some(arc) = narrowest_arc(circles, current, &previous) {
            arc_area += circle_area(arc.circle.radius, arc.width);
            arcs.push(arc);
            previous = *current;
        }
    }
    (arc_area + polygon_area / 2.0, arcs)
}

/// 두 교점이 함께 속한 원 가운데 두 점 사이 호가 가장 좁은 원
fn narrowest_arc(
    circles: &[Circle],
    current: &IntersectionPoint,
    previous: &IntersectionPoint,
) -> Option<Arc> {
    let middle = (
        (current.x + previous.x) / 2.0,
        (current.y + previous.y) / 2.0,
    );
    let mut best: Option<Arc> = None;
    for parent in current.parents {
        if !previous.parents.contains(&parent) {
            continue;
        }
        let circle = circles[parent];
        let start = (current.x - circle.x).atan2(current.y - circle.y);
        let end = (previous.x - circle.x).atan2(previous.y - circle.y);
        let mut sweep = end - start;
        if sweep < 0.0 {
            sweep += 2.0 * PI;
        }
        let halfway = end - sweep / 2.0;
        let edge = (
            circle.x + circle.radius * halfway.sin(),
            circle.y + circle.radius * halfway.cos(),
        );
        let width = distance(middle, edge).min(circle.radius * 2.0);
        if best.is_none_or(|arc| arc.width > width) {
            best = Some(Arc {
                circle,
                width,
                p1: (current.x, current.y),
                p2: (previous.x, previous.y),
            });
        }
    }
    best
}

/// 교점이 없을 때: 가장 작은 원이 다른 원 모두에 들어 있으면 그 원 전체, 아니면 겹침 없음
fn contained_area(circles: &[Circle]) -> (f64, Vec<Arc>) {
    let mut smallest = circles[0];
    for circle in &circles[1..] {
        if circle.radius < smallest.radius {
            smallest = *circle;
        }
    }
    let is_disjoint = circles.iter().any(|circle| {
        distance(center_of(circle), center_of(&smallest)) > (smallest.radius - circle.radius).abs()
    });
    if is_disjoint {
        return (0.0, Vec::new());
    }
    let arc = Arc {
        circle: smallest,
        width: smallest.radius * 2.0,
        p1: (smallest.x, smallest.y + smallest.radius),
        p2: (smallest.x - SMALL, smallest.y + smallest.radius),
    };
    (smallest.radius * smallest.radius * PI, vec![arc])
}

/// fmin `bisect`: 부호가 다른 두 끝 사이에서 근을 반씩 좁혀 찾는다
pub fn bisect(function: impl Fn(f64) -> f64, mut low: f64, high: f64) -> f64 {
    let low_value = function(low);
    let high_value = function(high);
    if low_value == 0.0 {
        return low;
    }
    if high_value == 0.0 {
        return high;
    }
    let mut delta = high - low;
    for _ in 0..100 {
        delta /= 2.0;
        let middle = low + delta;
        let middle_value = function(middle);
        if middle_value * low_value >= 0.0 {
            low = middle;
        }
        if delta.abs() < 1e-10 || middle_value == 0.0 {
            return middle;
        }
    }
    low + delta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 반쯤_겹친_두_원의_넓이는_두_활꼴의_합이다() {
        let first = Circle {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
        };
        let second = Circle { x: 1.0, ..first };
        let (area, arcs) = intersection_area(&[first, second]);
        assert!((area - circle_overlap(1.0, 1.0, 1.0)).abs() < 1e-9);
        assert_eq!(arcs.len(), 2);
    }
}
