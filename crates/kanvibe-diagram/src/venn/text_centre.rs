//! venn.js `computeTextCentres`: 영역마다 라벨을 둘 점.
//!
//! 영역에 속한 원(안쪽) 안에 들면서 나머지 원(바깥쪽)에서는 가장 멀리 떨어진 점을 찾는다. 원 둘레 몇 점에서
//! 여유가 가장 큰 점을 고른 뒤 Nelder–Mead로 여유를 키운다. 그 점이 조건을 어기면 영역 모양에 따라 대신할
//! 점을 고른다.

use super::geometry::{Circle, center_of_points, distance, intersection_area};
use super::layout::Solution;
use super::nelder_mead::{SearchLimits, minimize};

fn center(circle: &Circle) -> (f64, f64) {
    (circle.x, circle.y)
}

/// 안쪽 원 경계까지 남은 거리와 바깥쪽 원까지 거리 가운데 가장 작은 값
fn circle_margin(point: (f64, f64), interior: &[Circle], exterior: &[Circle]) -> f64 {
    let mut margin = interior[0].radius - distance(center(&interior[0]), point);
    for circle in &interior[1..] {
        let candidate = circle.radius - distance(center(circle), point);
        if candidate <= margin {
            margin = candidate;
        }
    }
    for circle in exterior {
        let candidate = distance(center(circle), point) - circle.radius;
        if candidate <= margin {
            margin = candidate;
        }
    }
    margin
}

/// 라벨 점. 화면에 나타낼 수 없는 영역은 venn.js처럼 화면 밖 (0, -1000)이다
pub fn compute_text_centre(interior: &[Circle], exterior: &[Circle]) -> (f64, f64) {
    let mut samples = Vec::new();
    for circle in interior {
        let half = circle.radius / 2.0;
        samples.push((circle.x, circle.y));
        samples.push((circle.x + half, circle.y));
        samples.push((circle.x - half, circle.y));
        samples.push((circle.x, circle.y + half));
        samples.push((circle.x, circle.y - half));
    }
    let mut initial = samples[0];
    let mut margin = circle_margin(samples[0], interior, exterior);
    for sample in &samples[1..] {
        let candidate = circle_margin(*sample, interior, exterior);
        if candidate >= margin {
            initial = *sample;
            margin = candidate;
        }
    }
    let limits = SearchLimits {
        max_iterations: 500,
        min_error_delta: 1e-10,
        min_tolerance: 1e-10,
    };
    let solution = minimize(
        |point| -circle_margin((point[0], point[1]), interior, exterior),
        &[initial.0, initial.1],
        &limits,
    );
    let found = (solution[0], solution[1]);
    let is_valid = interior
        .iter()
        .all(|circle| distance(found, center(circle)) <= circle.radius)
        && exterior
            .iter()
            .all(|circle| distance(found, center(circle)) >= circle.radius);
    if is_valid {
        return found;
    }
    fallback_centre(interior, exterior)
}

/// 찾은 점이 조건을 어길 때(완전히 겹친 경우 등) 대신 쓰는 점
fn fallback_centre(interior: &[Circle], exterior: &[Circle]) -> (f64, f64) {
    if interior.len() == 1 {
        return center(&interior[0]);
    }
    let (_, arcs) = intersection_area(interior);
    match arcs.as_slice() {
        [] => (0.0, -1000.0),
        [only] => center(&only.circle),
        _ if !exterior.is_empty() => compute_text_centre(interior, &[]),
        _ => {
            let points: Vec<(f64, f64)> = arcs.iter().map(|arc| arc.p1).collect();
            center_of_points(&points)
        }
    }
}

/// 집합별로 그 집합을 완전히 덮는 다른 집합 목록(getOverlappingCircles)
fn overlapping_circles(circles: &Solution) -> Vec<Vec<String>> {
    let mut covered_by = vec![Vec::new(); circles.len()];
    for first in 0..circles.len() {
        for second in first + 1..circles.len() {
            let (a, b) = (&circles[first].1, &circles[second].1);
            let gap = distance(center(a), center(b));
            if gap + b.radius <= a.radius + 1e-10 {
                covered_by[second].push(circles[first].0.clone());
            } else if gap + a.radius <= b.radius + 1e-10 {
                covered_by[first].push(circles[second].0.clone());
            }
        }
    }
    covered_by
}

/// 영역(집합 이름 목록)마다 라벨 점
pub fn compute_text_centres(circles: &Solution, areas: &[Vec<String>]) -> Vec<(f64, f64)> {
    let covered_by = overlapping_circles(circles);
    areas
        .iter()
        .map(|sets| {
            let mut excluded: Vec<&String> = Vec::new();
            for (index, (name, _)) in circles.iter().enumerate() {
                if sets.contains(name) {
                    excluded.extend(covered_by[index].iter());
                }
            }
            let mut interior = Vec::new();
            let mut exterior = Vec::new();
            for (name, circle) in circles {
                if sets.contains(name) {
                    interior.push(*circle);
                } else if !excluded.contains(&name) {
                    exterior.push(*circle);
                }
            }
            compute_text_centre(&interior, &exterior)
        })
        .collect()
}
