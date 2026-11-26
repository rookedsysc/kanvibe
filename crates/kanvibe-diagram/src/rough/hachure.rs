//! hachure-fill의 hachureLines: 다각형을 돌려 가로 주사선으로 채울 선분을 찾고 다시 돌려놓는다.

use crate::scene::Point;

/// 원점 둘레로 `degrees`만큼 돌린다(roughjs는 다각형을 제자리에서 돌렸다 되돌린다)
fn rotate_points(points: &mut [Point], degrees: f64) {
    let angle = std::f64::consts::PI / 180.0 * degrees;
    let (cos, sin) = (angle.cos(), angle.sin());
    for point in points {
        let (x, y) = (point.x, point.y);
        point.x = x * cos - y * sin;
        point.y = x * sin + y * cos;
    }
}

/// 주사선이 지나는 다각형 변
#[derive(Clone, Copy, Debug)]
struct Edge {
    y_min: f64,
    y_max: f64,
    x: f64,
    inverse_slope: f64,
}

/// JavaScript 정렬 비교 함수의 부호를 Ordering으로
fn sign_order(value: f64) -> std::cmp::Ordering {
    if value < 0.0 {
        std::cmp::Ordering::Less
    } else if value > 0.0 {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// hachureLines(polygons, gap, angle, stepOffset): 채움 선분(양 끝점) 목록
pub fn hachure_lines(polygons: &[Vec<Point>], gap: f64, angle: f64, step: f64) -> Vec<[Point; 2]> {
    let mut rotated: Vec<Vec<Point>> = polygons.to_vec();
    if angle != 0.0 {
        for polygon in &mut rotated {
            rotate_points(polygon, angle);
        }
    }
    let mut lines = straight_hachure_lines(&rotated, gap.max(0.1), step);
    if angle != 0.0 {
        for line in &mut lines {
            rotate_points(line, -angle);
        }
    }
    lines
}

fn polygon_edges(polygons: &[Vec<Point>]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for polygon in polygons {
        let mut vertices = polygon.clone();
        let (first, last) = (vertices[0], vertices[vertices.len() - 1]);
        if first != last {
            vertices.push(first);
        }
        if vertices.len() <= 2 {
            continue;
        }
        for pair in vertices.windows(2) {
            let (p1, p2) = (pair[0], pair[1]);
            if p1.y == p2.y {
                continue;
            }
            let y_min = p1.y.min(p2.y);
            edges.push(Edge {
                y_min,
                y_max: p1.y.max(p2.y),
                x: if y_min == p1.y { p1.x } else { p2.x },
                inverse_slope: (p2.x - p1.x) / (p2.y - p1.y),
            });
        }
    }
    edges.sort_by(|first, second| {
        sign_order(first.y_min - second.y_min)
            .then(sign_order(first.x - second.x))
            .then(sign_order(first.y_max - second.y_max))
    });
    edges
}

/// 주사선마다 활성 변을 x 순으로 짝지어 선분을 만든다
fn straight_hachure_lines(polygons: &[Vec<Point>], gap: f64, step: f64) -> Vec<[Point; 2]> {
    let mut edges = polygon_edges(polygons);
    let mut lines = Vec::new();
    let Some(first) = edges.first() else {
        return lines;
    };
    let mut active: Vec<Edge> = Vec::new();
    let mut y = first.y_min;
    let mut iteration: u64 = 0;
    while !active.is_empty() || !edges.is_empty() {
        let entering = edges.iter().take_while(|edge| edge.y_min <= y).count();
        active.extend(edges.drain(..entering));
        active.retain(|edge| edge.y_max > y);
        active.sort_by(|first, second| sign_order(first.x - second.x));
        let is_drawn_row = step != 1.0 || (iteration as f64 % gap) == 0.0;
        if is_drawn_row {
            for [left, right] in active.as_chunks::<2>().0 {
                lines.push([
                    Point::new(js_round(left.x), y),
                    Point::new(js_round(right.x), y),
                ]);
            }
        }
        y += step;
        for edge in &mut active {
            edge.x += step * edge.inverse_slope;
        }
        iteration += 1;
    }
    lines
}

/// JavaScript `Math.round`(0.5는 늘 +∞ 쪽)
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}
