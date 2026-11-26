//! venn.js `layout.js`: 집합 크기와 겹친 크기에 맞춰 원을 놓는다.
//!
//! 빠진 두 집합 겹침을 0으로 채운 뒤, 가장 많이 겹친 집합부터 탐욕적으로 원을 놓고(greedyLayout), 두 집합 이상
//! 겹침 넓이 오차의 제곱합을 Nelder–Mead로 줄인다. 그 뒤 가장 큰 원을 기준으로 돌리고 뒤집어(normalize)
//! 그림 상자에 맞춰 키운다(scaleSolution). 원 순서는 JS 객체 키 순서를 따른다.
//! 영역이 8개 이상이면 venn.js는 무작위 재시작 MDS 배치와 비교하지만, 무작위라 같은 결과를 낼 수 없어
//! 탐욕 배치만 쓴다.

use std::f64::consts::PI;

use super::geometry::{
    Circle, bisect, circle_circle_intersection, circle_overlap, distance, intersection_area,
};
use super::nelder_mead::{SearchLimits, minimize};

const SMALL: f64 = 1e-10;
/// 아직 놓지 않은 원의 자리
const UNPLACED: f64 = 1e10;

/// 크기가 주어진 영역(집합 하나 또는 여러 집합의 겹침)
#[derive(Clone, Debug, PartialEq)]
pub struct AreaSize {
    pub sets: Vec<String>,
    pub size: f64,
}

/// 집합 이름과 원. 순서가 JS 객체 키 순서다
pub type Solution = Vec<(String, Circle)>;

fn circle_of<'a>(solution: &'a Solution, set: &str) -> &'a Circle {
    solution
        .iter()
        .find(|(name, _)| name == set)
        .map(|(_, circle)| circle)
        .expect("영역의 집합은 모두 원이 있다")
}

fn center(circle: &Circle) -> (f64, f64) {
    (circle.x, circle.y)
}

/// venn(): 탐욕 배치 뒤 손실 함수 최소화
pub fn venn(areas: &[AreaSize]) -> Solution {
    let areas = add_missing_areas(areas);
    let mut circles = greedy_layout(&areas);
    let start: Vec<f64> = circles
        .iter()
        .flat_map(|(_, circle)| [circle.x, circle.y])
        .collect();
    let radii: Vec<(String, f64)> = circles
        .iter()
        .map(|(name, circle)| (name.clone(), circle.radius))
        .collect();
    let limits = SearchLimits {
        max_iterations: 500,
        min_error_delta: 1e-6,
        min_tolerance: 1e-5,
    };
    let positions = minimize(
        |values| {
            let current: Solution = radii
                .iter()
                .enumerate()
                .map(|(index, (name, radius))| {
                    let circle = Circle {
                        x: values[2 * index],
                        y: values[2 * index + 1],
                        radius: *radius,
                    };
                    (name.clone(), circle)
                })
                .collect();
            loss(&current, &areas)
        },
        &start,
        &limits,
    );
    for (index, (_, circle)) in circles.iter_mut().enumerate() {
        circle.x = positions[2 * index];
        circle.y = positions[2 * index + 1];
    }
    circles
}

/// 두 원(반지름 r1, r2)의 겹친 넓이가 `overlap`이 되는 중심 거리
fn distance_from_intersect_area(first: f64, second: f64, overlap: f64) -> f64 {
    let smaller = first.min(second);
    if smaller * smaller * PI <= overlap + SMALL {
        return (first - second).abs();
    }
    bisect(
        |gap| circle_overlap(first, second, gap) - overlap,
        0.0,
        first + second,
    )
}

fn pair_key(first: &str, second: &str) -> String {
    format!("{first};{second}")
}

/// 정의에 없는 두 집합 겹침을 크기 0으로 덧붙인다
fn add_missing_areas(areas: &[AreaSize]) -> Vec<AreaSize> {
    let mut result = areas.to_vec();
    let mut ids: Vec<String> = Vec::new();
    let mut pairs = std::collections::HashSet::new();
    for area in areas {
        match area.sets.as_slice() {
            [single] => ids.push(single.clone()),
            [first, second] => {
                pairs.insert(pair_key(first, second));
                pairs.insert(pair_key(second, first));
            }
            _ => {}
        }
    }
    ids.sort();
    for (index, first) in ids.iter().enumerate() {
        for second in &ids[index + 1..] {
            if !pairs.contains(&pair_key(first, second)) {
                result.push(AreaSize {
                    sets: vec![first.clone(), second.clone()],
                    size: 0.0,
                });
            }
        }
    }
    result
}

/// 이웃 집합과 겹친 크기·가중치
#[derive(Clone)]
struct Overlap {
    set: String,
    size: f64,
    weight: f64,
}

/// 집합별 이웃 겹침 목록. 다른 쪽을 완전히 품는 겹침은 가중치 0이다
fn set_overlaps(pairs: &[&AreaSize], circles: &[(String, Circle, f64)]) -> Vec<Vec<Overlap>> {
    let index_of = |set: &str| {
        circles
            .iter()
            .position(|(name, _, _)| name == set)
            .expect("겹침의 집합은 모두 원이 있다")
    };
    let mut overlaps = vec![Vec::new(); circles.len()];
    for area in pairs {
        let (left, right) = (index_of(&area.sets[0]), index_of(&area.sets[1]));
        let smaller = circles[left].2.min(circles[right].2);
        let weight = if area.size + SMALL >= smaller {
            0.0
        } else {
            1.0
        };
        overlaps[left].push(Overlap {
            set: area.sets[1].clone(),
            size: area.size,
            weight,
        });
        overlaps[right].push(Overlap {
            set: area.sets[0].clone(),
            size: area.size,
            weight,
        });
    }
    overlaps
}

/// greedyLayout: 겹침이 큰 집합부터, 이미 놓인 원과의 겹침이 맞는 후보 자리 가운데 손실이 가장 작은 곳에 둔다
fn greedy_layout(areas: &[AreaSize]) -> Solution {
    let sized: Vec<(String, Circle, f64)> = areas
        .iter()
        .filter(|area| area.sets.len() == 1)
        .map(|area| {
            let circle = Circle {
                x: UNPLACED,
                y: UNPLACED,
                radius: (area.size / PI).sqrt(),
            };
            (area.sets[0].clone(), circle, area.size)
        })
        .collect();
    let pairs: Vec<&AreaSize> = areas.iter().filter(|area| area.sets.len() == 2).collect();
    let pair_areas: Vec<AreaSize> = pairs.iter().map(|area| (*area).clone()).collect();
    let overlaps = set_overlaps(&pairs, &sized);
    let mut order: Vec<(usize, f64)> = overlaps
        .iter()
        .enumerate()
        .map(|(index, list)| {
            let total = list
                .iter()
                .fold(0.0, |sum, overlap| sum + overlap.size * overlap.weight);
            (index, total)
        })
        .collect();
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut circles: Solution = sized
        .into_iter()
        .map(|(name, circle, _)| (name, circle))
        .collect();
    let mut placed = vec![false; circles.len()];
    let first = order[0].0;
    circles[first].1.x = 0.0;
    circles[first].1.y = 0.0;
    placed[first] = true;
    for &(index, _) in &order[1..] {
        let candidates = candidate_points(&circles, &placed, &overlaps[index], index);
        let best = best_candidate(&mut circles, index, &candidates, &pair_areas);
        circles[index].1.x = best.0;
        circles[index].1.y = best.1;
        placed[index] = true;
    }
    circles
}

/// 이미 놓인 이웃마다 상하좌우 네 자리와, 이웃 둘과 동시에 맞는 교점 자리
fn candidate_points(
    circles: &Solution,
    placed: &[bool],
    overlaps: &[Overlap],
    index: usize,
) -> Vec<(f64, f64)> {
    let is_placed = |set: &str| {
        circles
            .iter()
            .position(|(name, _)| name == set)
            .is_some_and(|position| placed[position])
    };
    let mut neighbours: Vec<&Overlap> = overlaps
        .iter()
        .filter(|overlap| is_placed(&overlap.set))
        .collect();
    neighbours.sort_by(|a, b| b.size.total_cmp(&a.size));
    let radius = circles[index].1.radius;
    let mut points = Vec::new();
    for (position, neighbour) in neighbours.iter().enumerate() {
        let first = circle_of(circles, &neighbour.set);
        let first_gap = distance_from_intersect_area(radius, first.radius, neighbour.size);
        points.push((first.x + first_gap, first.y));
        points.push((first.x - first_gap, first.y));
        points.push((first.x, first.y + first_gap));
        points.push((first.x, first.y - first_gap));
        for other in &neighbours[position + 1..] {
            let second = circle_of(circles, &other.set);
            let second_gap = distance_from_intersect_area(radius, second.radius, other.size);
            let first_ring = Circle {
                radius: first_gap,
                ..*first
            };
            let second_ring = Circle {
                radius: second_gap,
                ..*second
            };
            points.extend(circle_circle_intersection(&first_ring, &second_ring));
        }
    }
    points
}

fn best_candidate(
    circles: &mut Solution,
    index: usize,
    candidates: &[(f64, f64)],
    pair_areas: &[AreaSize],
) -> (f64, f64) {
    let mut best_loss = 1e50;
    let mut best_point = candidates[0];
    for point in candidates {
        circles[index].1.x = point.0;
        circles[index].1.y = point.1;
        let local = loss(circles, pair_areas);
        if local < best_loss {
            best_loss = local;
            best_point = *point;
        }
    }
    best_point
}

/// lossFunction: 두 집합 이상 영역마다 실제 겹친 넓이와 원하는 크기 차이의 제곱합
pub fn loss(circles: &Solution, areas: &[AreaSize]) -> f64 {
    let mut output = 0.0;
    for area in areas {
        let overlap = match area.sets.as_slice() {
            [_] => continue,
            [left, right] => {
                let (left, right) = (circle_of(circles, left), circle_of(circles, right));
                circle_overlap(
                    left.radius,
                    right.radius,
                    distance(center(left), center(right)),
                )
            }
            sets => {
                let members: Vec<Circle> =
                    sets.iter().map(|set| *circle_of(circles, set)).collect();
                intersection_area(&members).0
            }
        };
        output += (overlap - area.size) * (overlap - area.size);
    }
    output
}

/// 가장 큰 원을 원점에 두고, 두 번째 원이 `orientation`(π/2) 방향에 오도록 돌리고, 세 번째 원이 그 선 위쪽이면
/// 뒤집는다
fn orientate_circles(cluster: &mut [(String, Circle)], orientation: f64) {
    cluster.sort_by(|a, b| b.1.radius.total_cmp(&a.1.radius));
    let Some((_, largest)) = cluster.first().cloned() else {
        return;
    };
    for (_, circle) in cluster.iter_mut() {
        circle.x -= largest.x;
        circle.y -= largest.y;
    }
    if let [(_, first), (_, second)] = cluster
        && distance(center(first), center(second)) < (second.radius - first.radius).abs()
    {
        second.x = first.x + first.radius - second.radius - 1e-10;
        second.y = first.y;
    }
    if cluster.len() > 1 {
        let rotation = cluster[1].1.x.atan2(cluster[1].1.y) - orientation;
        let (sine, cosine) = rotation.sin_cos();
        for (_, circle) in cluster.iter_mut() {
            let (x, y) = (circle.x, circle.y);
            circle.x = cosine * x - sine * y;
            circle.y = sine * x + cosine * y;
        }
    }
    if cluster.len() > 2 {
        mirror_if_third_above(cluster, orientation);
    }
}

fn mirror_if_third_above(cluster: &mut [(String, Circle)], orientation: f64) {
    let mut angle = cluster[2].1.x.atan2(cluster[2].1.y) - orientation;
    while angle < 0.0 {
        angle += 2.0 * PI;
    }
    while angle > 2.0 * PI {
        angle -= 2.0 * PI;
    }
    if angle <= PI {
        return;
    }
    let slope = cluster[1].1.y / (1e-10 + cluster[1].1.x);
    for (_, circle) in cluster.iter_mut() {
        let projected = (circle.x + slope * circle.y) / (1.0 + slope * slope);
        circle.x = 2.0 * projected - circle.x;
        circle.y = 2.0 * projected * slope - circle.y;
    }
}

/// 원들의 가로·세로 범위 (min_x, max_x, min_y, max_y)
fn bounding_box(circles: &[(String, Circle)]) -> (f64, f64, f64, f64) {
    circles.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), (_, circle)| {
            (
                min_x.min(circle.x - circle.radius),
                max_x.max(circle.x + circle.radius),
                min_y.min(circle.y - circle.radius),
                max_y.max(circle.y + circle.radius),
            )
        },
    )
}

/// disjointCluster: 서로 겹치는 원끼리 묶는다(union-find). 묶음 순서는 대표 원이 처음 나온 순서다
fn disjoint_clusters(circles: &Solution) -> Vec<Vec<(String, Circle)>> {
    let mut parents: Vec<usize> = (0..circles.len()).collect();
    fn find(parents: &mut [usize], index: usize) -> usize {
        if parents[index] != index {
            let root = find(parents, parents[index]);
            parents[index] = root;
        }
        parents[index]
    }
    for first in 0..circles.len() {
        for second in first + 1..circles.len() {
            let (a, b) = (&circles[first].1, &circles[second].1);
            if distance(center(a), center(b)) + 1e-10 < a.radius + b.radius {
                let second_root = find(&mut parents, second);
                let first_root = find(&mut parents, first);
                parents[second_root] = first_root;
            }
        }
    }
    let mut roots: Vec<usize> = Vec::new();
    let mut clusters: Vec<Vec<(String, Circle)>> = Vec::new();
    for (index, circle) in circles.iter().enumerate() {
        let root = find(&mut parents, index);
        let position = roots.iter().position(|known| *known == root);
        let position = position.unwrap_or_else(|| {
            roots.push(root);
            clusters.push(Vec::new());
            clusters.len() - 1
        });
        clusters[position].push(circle.clone());
    }
    clusters
}

/// normalizeSolution: 묶음마다 방향을 맞추고, 가장 큰 묶음 둘레에 나머지를 격자로 붙인다
pub fn normalize_solution(solution: &Solution) -> Solution {
    let orientation = PI / 2.0;
    let mut clusters = disjoint_clusters(solution);
    for cluster in &mut clusters {
        orientate_circles(cluster, orientation);
    }
    let area_of = |cluster: &Vec<(String, Circle)>| {
        let (min_x, max_x, min_y, max_y) = bounding_box(cluster);
        (max_x - min_x) * (max_y - min_y)
    };
    clusters.sort_by(|a, b| area_of(b).total_cmp(&area_of(a)));
    let mut clusters = clusters.into_iter();
    let mut circles = clusters.next().unwrap_or_default();
    let mut bounds = bounding_box(&circles);
    let spacing = (bounds.1 - bounds.0) / 50.0;
    let rest: Vec<_> = clusters.collect();
    for group in rest.chunks(3) {
        let placements = [(true, false), (false, true), (true, true)];
        for (cluster, (is_right, is_bottom)) in group.iter().zip(placements) {
            let offset =
                cluster_offset(bounds, bounding_box(cluster), spacing, is_right, is_bottom);
            circles.extend(cluster.iter().map(|(name, circle)| {
                let moved = Circle {
                    x: circle.x + offset.0,
                    y: circle.y + offset.1,
                    radius: circle.radius,
                };
                (name.clone(), moved)
            }));
        }
        bounds = bounding_box(&circles);
    }
    circles
}

/// addCluster: 오른쪽(또는 아래)에 붙이거나 끝을 맞추고, 더 작으면 가운데로 민다
fn cluster_offset(
    target: (f64, f64, f64, f64),
    cluster: (f64, f64, f64, f64),
    spacing: f64,
    is_right: bool,
    is_bottom: bool,
) -> (f64, f64) {
    let along = |target_min: f64, target_max: f64, min: f64, max: f64, is_after: bool| {
        if is_after {
            return target_max - min + spacing;
        }
        let centring = (max - min) / 2.0 - (target_max - target_min) / 2.0;
        target_max - max + centring.min(0.0)
    };
    (
        along(target.0, target.1, cluster.0, cluster.1, is_right),
        along(target.2, target.3, cluster.2, cluster.3, is_bottom),
    )
}

/// scaleSolution: 여백을 뺀 상자에 가로세로 비율을 지켜 맞추고 가운데 둔다
pub fn scale_solution(solution: &Solution, width: f64, height: f64, padding: f64) -> Solution {
    let width = width - 2.0 * padding;
    let height = height - 2.0 * padding;
    let (min_x, max_x, min_y, max_y) = bounding_box(solution);
    if max_x == min_x || max_y == min_y {
        return solution.clone();
    }
    let scaling = f64::min(height / (max_y - min_y), width / (max_x - min_x));
    let x_offset = (width - (max_x - min_x) * scaling) / 2.0;
    let y_offset = (height - (max_y - min_y) * scaling) / 2.0;
    solution
        .iter()
        .map(|(name, circle)| {
            let scaled = Circle {
                radius: scaling * circle.radius,
                x: padding + x_offset + (circle.x - min_x) * scaling,
                y: padding + y_offset + (circle.y - min_y) * scaling,
            };
            (name.clone(), scaled)
        })
        .collect()
}
