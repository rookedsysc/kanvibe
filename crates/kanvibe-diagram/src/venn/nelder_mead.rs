//! fmin `nelderMead`(venn.js 번들 판): 내리막 단체법으로 함수의 최솟점을 찾는다.
//!
//! 원 배치 결과가 반복 순서와 비교 기준의 작은 차이에도 달라지므로, 정렬(안정 정렬), 반사·확장·수축·축소
//! 순서와 멈춤 조건을 원본과 같게 둔다.

/// 탐색 설정. fmin은 `minErrorDelta`를 주면 `minTolerance`도 같은 값을 쓰므로 부르는 쪽이 둘을 맞춘다
pub struct SearchLimits {
    pub max_iterations: usize,
    pub min_error_delta: f64,
    pub min_tolerance: f64,
}

#[derive(Clone)]
struct Vertex {
    point: Vec<f64>,
    value: f64,
}

const NON_ZERO_DELTA: f64 = 1.05;
const ZERO_DELTA: f64 = 0.001;
const RHO: f64 = 1.0;
const CHI: f64 = 2.0;
const PSI: f64 = -0.5;
const SIGMA: f64 = 0.5;

fn weighted_sum(first_weight: f64, first: &[f64], second_weight: f64, second: &[f64]) -> Vec<f64> {
    first
        .iter()
        .zip(second)
        .map(|(a, b)| first_weight * a + second_weight * b)
        .collect()
}

fn evaluate(function: &impl Fn(&[f64]) -> f64, point: Vec<f64>) -> Vertex {
    let value = function(&point);
    Vertex { point, value }
}

/// 첫 꼭짓점은 시작점, 나머지는 좌표 하나씩을 5% 늘린(0이면 0.001로 둔) 점이다
fn initial_simplex(function: &impl Fn(&[f64]) -> f64, start: &[f64]) -> Vec<Vertex> {
    let mut simplex = vec![evaluate(function, start.to_vec())];
    for index in 0..start.len() {
        let mut point = start.to_vec();
        point[index] = if point[index] != 0.0 {
            point[index] * NON_ZERO_DELTA
        } else {
            ZERO_DELTA
        };
        simplex.push(evaluate(function, point));
    }
    simplex
}

fn has_converged(simplex: &[Vertex], limits: &SearchLimits) -> bool {
    let size = simplex.len() - 1;
    let max_difference = (0..size)
        .map(|index| (simplex[0].point[index] - simplex[1].point[index]).abs())
        .fold(0.0, f64::max);
    (simplex[0].value - simplex[size].value).abs() < limits.min_error_delta
        && max_difference < limits.min_tolerance
}

fn centroid(simplex: &[Vertex]) -> Vec<f64> {
    let size = simplex.len() - 1;
    (0..size)
        .map(|index| {
            let sum = simplex[..size]
                .iter()
                .fold(0.0, |total, vertex| total + vertex.point[index]);
            sum / size as f64
        })
        .collect()
}

pub fn minimize(
    function: impl Fn(&[f64]) -> f64,
    start: &[f64],
    limits: &SearchLimits,
) -> Vec<f64> {
    let mut simplex = initial_simplex(&function, start);
    let size = start.len();
    for _ in 0..limits.max_iterations {
        simplex.sort_by(|a, b| a.value.total_cmp(&b.value));
        if has_converged(&simplex, limits) {
            break;
        }
        let center = centroid(&simplex);
        step(&function, &mut simplex, &center, size);
    }
    simplex.sort_by(|a, b| a.value.total_cmp(&b.value));
    simplex.swap_remove(0).point
}

/// 한 번의 반사·확장·수축. 수축으로도 나아지지 않으면 가장 좋은 꼭짓점 쪽으로 단체를 줄인다
fn step(function: &impl Fn(&[f64]) -> f64, simplex: &mut [Vertex], center: &[f64], size: usize) {
    let worst = simplex[size].clone();
    let reflected = evaluate(
        function,
        weighted_sum(1.0 + RHO, center, -RHO, &worst.point),
    );
    if reflected.value < simplex[0].value {
        let expanded = evaluate(
            function,
            weighted_sum(1.0 + CHI, center, -CHI, &worst.point),
        );
        simplex[size] = if expanded.value < reflected.value {
            expanded
        } else {
            reflected
        };
        return;
    }
    if reflected.value < simplex[size - 1].value {
        simplex[size] = reflected;
        return;
    }
    match contract(function, center, &worst, &reflected) {
        Some(vertex) => simplex[size] = vertex,
        None => shrink(function, simplex),
    }
}

/// 반사점이 가장 나쁜 점보다도 나쁘면 안쪽, 아니면 바깥쪽으로 수축한다. 나아지지 않으면 `None`
fn contract(
    function: &impl Fn(&[f64]) -> f64,
    center: &[f64],
    worst: &Vertex,
    reflected: &Vertex,
) -> Option<Vertex> {
    if reflected.value > worst.value {
        let inside = evaluate(
            function,
            weighted_sum(1.0 + PSI, center, -PSI, &worst.point),
        );
        return (inside.value < worst.value).then_some(inside);
    }
    let outside = evaluate(
        function,
        weighted_sum(1.0 - PSI * RHO, center, PSI * RHO, &worst.point),
    );
    (outside.value < reflected.value).then_some(outside)
}

fn shrink(function: &impl Fn(&[f64]) -> f64, simplex: &mut [Vertex]) {
    let best = simplex[0].point.clone();
    for vertex in simplex.iter_mut().skip(1) {
        let point = weighted_sum(1.0 - SIGMA, &best, SIGMA, &vertex.point);
        *vertex = evaluate(function, point);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 이차함수의_최솟점을_찾는다() {
        let limits = SearchLimits {
            max_iterations: 500,
            min_error_delta: 1e-10,
            min_tolerance: 1e-10,
        };
        let found = minimize(
            |point| (point[0] - 3.0).powi(2) + (point[1] + 1.0).powi(2),
            &[0.0, 0.0],
            &limits,
        );
        assert!((found[0] - 3.0).abs() < 1e-4);
        assert!((found[1] + 1.0).abs() < 1e-4);
    }
}
