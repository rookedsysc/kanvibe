//! cose-base 2.2 `ConstraintHandler.handleConstraints`(강제 단계)와 `CoSELayout.initConstraintVariables`·
//! `updateDisplacements`(반복마다 변위를 제약에 맞추는 단계).
//!
//! 정렬 묶음은 대표 가상 노드(dummy)로 바꿔 상대 배치 그래프를 만든다. Mermaid는 `randomize: false`라 처음 배치를
//! 돌리거나 뒤집는 변환 단계는 건너뛴다. 원본은 10회마다 상대 배치 노드 순서 뒤쪽 1/3을 `Math.random`으로 섞는데,
//! 제약이 만족된 동안에는 순서가 결과를 바꾸지 않고, 바꾸더라도 차이가 1e-4px 아래라 섞지 않는다. 그래서 TS도
//! 그래프에 따라 매번 미세하게 다른 좌표를 낼 수 있다(architecture-bends: 두 갈래, 약 2e-5px).

use std::collections::{HashMap, VecDeque};

use indexmap::IndexMap;

use super::graph::CoseModel;
use crate::architecture::placement::RelativePlacement;

/// 상대 배치 그래프의 꼭짓점: 정렬 묶음에 든 노드는 그 묶음의 가상 노드로 바뀐다
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Vertex {
    Node(usize),
    Dummy(usize),
}

/// 모델 노드 번호로 바꾼 제약
#[derive(Clone, Debug, Default)]
pub(super) struct ModelConstraints {
    pub horizontal: Vec<Vec<usize>>,
    pub vertical: Vec<Vec<usize>>,
    pub relative: Vec<(RelativePlacement, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

/// 한 축의 정렬 묶음 ↔ 가상 노드 대응
#[derive(Clone, Debug, Default)]
struct DummyMap {
    node_to_dummy: HashMap<usize, usize>,
    members: Vec<Vec<usize>>,
}

impl DummyMap {
    fn new(alignments: &[Vec<usize>]) -> Self {
        let mut map = Self::default();
        for (index, members) in alignments.iter().enumerate() {
            for &node in members {
                map.node_to_dummy.insert(node, index);
            }
            map.members.push(members.clone());
        }
        map
    }

    fn vertex(&self, node: usize) -> Vertex {
        self.node_to_dummy
            .get(&node)
            .map_or(Vertex::Node(node), |&dummy| Vertex::Dummy(dummy))
    }

    fn representative(&self, vertex: Vertex) -> usize {
        match vertex {
            Vertex::Node(node) => node,
            Vertex::Dummy(dummy) => self.members[dummy][0],
        }
    }

    fn nodes_of(&self, vertex: Vertex) -> Vec<usize> {
        match vertex {
            Vertex::Node(node) => vec![node],
            Vertex::Dummy(dummy) => self.members[dummy].clone(),
        }
    }
}

fn unique_in_order(nodes: &[usize]) -> Vec<usize> {
    let mut unique: Vec<usize> = Vec::new();
    for &node in nodes {
        if !unique.contains(&node) {
            unique.push(node);
        }
    }
    unique
}

/// 한 방향의 상대 배치 제약: (앞 노드, 뒤 노드, 간격)
fn axis_pairs(constraints: &ModelConstraints, axis: Axis) -> Vec<(usize, usize, f64)> {
    constraints
        .relative
        .iter()
        .filter_map(|&(placement, gap)| match (placement, axis) {
            (RelativePlacement::Horizontal { left, right }, Axis::X) => Some((left, right, gap)),
            (RelativePlacement::Vertical { top, bottom }, Axis::Y) => Some((top, bottom, gap)),
            _ => None,
        })
        .collect()
}

/// handleConstraints의 강제 단계: 정렬 묶음을 평균 좌표로 모으고 상대 배치를 위상 순서로 밀어낸다
pub(super) fn enforce(model: &mut CoseModel, constraints: &ModelConstraints) {
    let mut coordinates: HashMap<usize, (f64, f64)> = model
        .all_nodes
        .iter()
        .filter(|&&node| model.nodes[node].child.is_none())
        .map(|&node| {
            let rect = &model.nodes[node].rect;
            (node, (rect.center_x(), rect.center_y()))
        })
        .collect();
    align_to_average(&mut coordinates, &constraints.vertical, Axis::X);
    align_to_average(&mut coordinates, &constraints.horizontal, Axis::Y);
    if !constraints.relative.is_empty() {
        enforce_relative(&mut coordinates, constraints, Axis::X);
        enforce_relative(&mut coordinates, constraints, Axis::Y);
    }
    for (node, (x, y)) in coordinates {
        model.nodes[node].rect.set_center(x, y);
    }
}

fn coordinate(coordinates: &HashMap<usize, (f64, f64)>, node: usize, axis: Axis) -> f64 {
    let (x, y) = coordinates[&node];
    if axis == Axis::X { x } else { y }
}

fn set_coordinate(
    coordinates: &mut HashMap<usize, (f64, f64)>,
    node: usize,
    axis: Axis,
    value: f64,
) {
    if let Some(entry) = coordinates.get_mut(&node) {
        match axis {
            Axis::X => entry.0 = value,
            Axis::Y => entry.1 = value,
        }
    }
}

fn align_to_average(
    coordinates: &mut HashMap<usize, (f64, f64)>,
    alignments: &[Vec<usize>],
    axis: Axis,
) {
    for alignment in alignments {
        let members = unique_in_order(alignment);
        let sum: f64 = members
            .iter()
            .fold(0.0, |sum, &node| sum + coordinate(coordinates, node, axis));
        let average = sum / members.len() as f64;
        for &node in &members {
            set_coordinate(coordinates, node, axis, average);
        }
    }
}

/// 가상 노드로 바꾼 방향 그래프(dagOnHorizontal·dagOnVertical)
type Dag = IndexMap<Vertex, Vec<(Vertex, f64)>>;

fn axis_dag(constraints: &ModelConstraints, axis: Axis, dummies: &DummyMap) -> Dag {
    // 원래 dag는 제약 순서대로 앞 노드 키를 만들고, 그 키 순서대로 가상 노드 그래프를 다시 쌓는다
    let mut by_source: IndexMap<usize, Vec<(usize, f64)>> = IndexMap::new();
    for (placement, gap) in &constraints.relative {
        let (first, second) = match *placement {
            RelativePlacement::Horizontal { left, right } => (left, right),
            RelativePlacement::Vertical { top, bottom } => (top, bottom),
        };
        let is_axis = matches!(
            (placement, axis),
            (RelativePlacement::Horizontal { .. }, Axis::X)
                | (RelativePlacement::Vertical { .. }, Axis::Y)
        );
        by_source
            .entry(first)
            .or_default()
            .push((second, if is_axis { *gap } else { f64::NAN }));
        by_source.entry(second).or_default();
    }
    let mut dag = Dag::new();
    for (&source, targets) in &by_source {
        for &(target, gap) in targets.iter().filter(|(_, gap)| !gap.is_nan()) {
            let target_vertex = dummies.vertex(target);
            dag.entry(dummies.vertex(source))
                .or_default()
                .push((target_vertex, gap));
            dag.entry(target_vertex).or_default();
        }
    }
    dag
}

fn enforce_relative(
    coordinates: &mut HashMap<usize, (f64, f64)>,
    constraints: &ModelConstraints,
    axis: Axis,
) {
    let alignments = if axis == Axis::X {
        &constraints.vertical
    } else {
        &constraints.horizontal
    };
    let dummies = DummyMap::new(alignments);
    let dag = axis_dag(constraints, axis, &dummies);
    let before = |vertex: Vertex| coordinate(coordinates, dummies.representative(vertex), axis);
    let sources = component_sources(&dag);
    let positions = relative_positions(&dag, &before, &sources);
    for (vertex, position) in positions {
        for node in dummies.nodes_of(vertex) {
            set_coordinate(coordinates, node, axis, position);
        }
    }
}

/// 약하게 이어진 성분마다 들어오는 간선이 없는 꼭짓점들(findComponents + dagToReversed)
fn component_sources(dag: &Dag) -> Vec<Vec<Vertex>> {
    let mut undirected: IndexMap<Vertex, Vec<Vertex>> =
        dag.keys().map(|&key| (key, Vec::new())).collect();
    let mut in_degree: HashMap<Vertex, usize> = dag.keys().map(|&key| (key, 0)).collect();
    for (&source, targets) in dag {
        for &(target, _) in targets {
            undirected[&source].push(target);
            undirected[&target].push(source);
            *in_degree.entry(target).or_default() += 1;
        }
    }
    let mut visited: Vec<Vertex> = Vec::new();
    let mut sources = Vec::new();
    for &key in undirected.keys() {
        if visited.contains(&key) {
            continue;
        }
        let component = breadth_first_component(&undirected, key, &mut visited);
        sources.push(
            component
                .into_iter()
                .filter(|vertex| in_degree[vertex] == 0)
                .collect(),
        );
    }
    sources
}

fn breadth_first_component(
    undirected: &IndexMap<Vertex, Vec<Vertex>>,
    start: Vertex,
    visited: &mut Vec<Vertex>,
) -> Vec<Vertex> {
    let mut component = vec![start];
    let mut queue = VecDeque::from([start]);
    visited.push(start);
    while let Some(current) = queue.pop_front() {
        for &neighbor in &undirected[&current] {
            if !visited.contains(&neighbor) {
                visited.push(neighbor);
                queue.push_back(neighbor);
                component.push(neighbor);
            }
        }
    }
    component
}

/// findAppropriatePositionForRelativePlacement(고정 노드 없음)
fn relative_positions(
    dag: &Dag,
    before: &dyn Fn(Vertex) -> f64,
    sources: &[Vec<Vertex>],
) -> IndexMap<Vertex, f64> {
    let mut in_degree: IndexMap<Vertex, usize> = dag.keys().map(|&key| (key, 0)).collect();
    for (target, _) in dag.values().flatten() {
        in_degree[target] += 1;
    }
    let mut positions = source_positions(&in_degree, before, sources);
    let mut past: IndexMap<Vertex, Vec<Vertex>> = dag.keys().map(|&key| (key, vec![key])).collect();
    let mut queue: VecDeque<Vertex> = in_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(&key, _)| key)
        .collect();
    while let Some(current) = queue.pop_front() {
        for &(neighbor, gap) in &dag[&current] {
            let pushed = positions[&current] + gap;
            if positions[&neighbor] < pushed {
                positions[&neighbor] = pushed;
            }
            in_degree[&neighbor] -= 1;
            if in_degree[&neighbor] == 0 {
                queue.push_back(neighbor);
            }
            past[&neighbor] = union_in_order(&past[&current], &past[&neighbor]);
        }
    }
    recenter_sink_components(dag, &past, before, &mut positions);
    positions
}

/// 들어오는 간선이 없는 꼭짓점은 성분 안 출발점들의 평균 자리에 두고, 나머지는 −∞에서 출발해 앞 꼭짓점이
/// 밀어낸다
fn source_positions(
    in_degree: &IndexMap<Vertex, usize>,
    before: &dyn Fn(Vertex) -> f64,
    sources: &[Vec<Vertex>],
) -> IndexMap<Vertex, f64> {
    let mut positions: IndexMap<Vertex, f64> = in_degree
        .iter()
        .map(|(&key, &degree)| {
            let start = if degree == 0 {
                f64::NAN
            } else {
                f64::NEG_INFINITY
            };
            (key, start)
        })
        .collect();
    for component in sources {
        let sum = component
            .iter()
            .fold(0.0, |sum, &vertex| sum + before(vertex));
        let average = sum / component.len() as f64;
        component
            .iter()
            .for_each(|vertex| positions[vertex] = average);
    }
    positions
}

/// JS `new Set([...first, ...second])`
fn union_in_order(first: &[Vertex], second: &[Vertex]) -> Vec<Vertex> {
    let mut union = first.to_vec();
    for vertex in second {
        if !union.contains(vertex) {
            union.push(*vertex);
        }
    }
    union
}

/// 싱크마다 그 조상 묶음을 성분으로 모아, 밀어내기 전 범위의 가운데로 되돌린다
fn recenter_sink_components(
    dag: &Dag,
    past: &IndexMap<Vertex, Vec<Vertex>>,
    before: &dyn Fn(Vertex) -> f64,
    positions: &mut IndexMap<Vertex, f64>,
) {
    let mut components: Vec<Vec<Vertex>> = Vec::new();
    for (key, ancestors) in past {
        if !dag[key].is_empty() {
            continue;
        }
        let existing = components
            .iter()
            .rposition(|component| component.contains(&ancestors[0]));
        match existing {
            Some(index) => components[index] = union_in_order(&components[index], ancestors),
            None => components.push(ancestors.clone()),
        }
    }
    for component in components {
        let (mut min_before, mut max_before) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut min_after, mut max_after) = (f64::INFINITY, f64::NEG_INFINITY);
        for vertex in &component {
            let (position_before, position_after) = (before(*vertex), positions[vertex]);
            min_before = min_before.min(position_before);
            max_before = max_before.max(position_before);
            min_after = min_after.min(position_after);
            max_after = max_after.max(position_after);
        }
        let shift = (min_before + max_before) / 2.0 - (min_after + max_after) / 2.0;
        component
            .iter()
            .for_each(|vertex| positions[vertex] += shift);
    }
}

/// 반복 중 상대 배치 한 꼭짓점의 이웃 제약
#[derive(Clone, Copy, Debug)]
enum Neighbor {
    /// 이 꼭짓점이 앞(왼쪽·위)이고 상대가 뒤
    After(Vertex, f64),
    /// 이 꼭짓점이 뒤이고 상대가 앞
    Before(Vertex, f64),
}

#[derive(Clone, Debug, Default)]
struct AxisRelaxation {
    dummies: DummyMap,
    order: Vec<Vertex>,
    neighbors: HashMap<Vertex, Vec<Neighbor>>,
    positions: HashMap<Vertex, f64>,
}

/// initConstraintVariables가 만든 반복용 상태
#[derive(Clone, Debug, Default)]
pub(super) struct Relaxation {
    horizontal: Vec<Vec<usize>>,
    vertical: Vec<Vec<usize>>,
    x_axis: Option<AxisRelaxation>,
    y_axis: Option<AxisRelaxation>,
}

impl Relaxation {
    pub fn new(model: &CoseModel, constraints: &ModelConstraints) -> Self {
        let has_relative = !constraints.relative.is_empty();
        let axis = |axis: Axis| has_relative.then(|| AxisRelaxation::new(model, constraints, axis));
        Self {
            horizontal: constraints.horizontal.clone(),
            vertical: constraints.vertical.clone(),
            x_axis: axis(Axis::X),
            y_axis: axis(Axis::Y),
        }
    }

    /// updateDisplacements: 정렬 묶음은 평균 변위로 맞추고, 상대 배치는 간격을 지키도록 변위를 고친다
    pub fn adjust(&mut self, model: &mut CoseModel) {
        average_displacements(model, &self.vertical, Axis::X);
        average_displacements(model, &self.horizontal, Axis::Y);
        if let Some(relaxation) = &mut self.x_axis {
            relaxation.adjust(model, Axis::X);
        }
        if let Some(relaxation) = &mut self.y_axis {
            relaxation.adjust(model, Axis::Y);
        }
    }
}

fn displacement(model: &CoseModel, node: usize, axis: Axis) -> f64 {
    let (x, y) = model.nodes[node].displacement;
    if axis == Axis::X { x } else { y }
}

fn set_displacement(model: &mut CoseModel, node: usize, axis: Axis, value: f64) {
    let entry = &mut model.nodes[node].displacement;
    match axis {
        Axis::X => entry.0 = value,
        Axis::Y => entry.1 = value,
    }
}

fn average_displacements(model: &mut CoseModel, alignments: &[Vec<usize>], axis: Axis) {
    for alignment in alignments {
        let total = alignment
            .iter()
            .fold(0.0, |sum, &node| sum + displacement(model, node, axis));
        let average = total / alignment.len() as f64;
        for &node in alignment {
            set_displacement(model, node, axis, average);
        }
    }
}

impl AxisRelaxation {
    fn new(model: &CoseModel, constraints: &ModelConstraints, axis: Axis) -> Self {
        let alignments = if axis == Axis::X {
            &constraints.vertical
        } else {
            &constraints.horizontal
        };
        let mut relaxation = Self {
            dummies: DummyMap::new(alignments),
            ..Self::default()
        };
        for (first, second, gap) in axis_pairs(constraints, axis) {
            let first = relaxation.register(model, first, axis);
            let second = relaxation.register(model, second, axis);
            relaxation
                .neighbors
                .entry(first)
                .or_default()
                .push(Neighbor::After(second, gap));
            relaxation
                .neighbors
                .entry(second)
                .or_default()
                .push(Neighbor::Before(first, gap));
        }
        relaxation
    }

    fn register(&mut self, model: &CoseModel, node: usize, axis: Axis) -> Vertex {
        let vertex = self.dummies.vertex(node);
        if !self.order.contains(&vertex) {
            self.order.push(vertex);
            let rect = &model.nodes[self.dummies.representative(vertex)].rect;
            let center = if axis == Axis::X {
                rect.center_x()
            } else {
                rect.center_y()
            };
            self.positions.insert(vertex, center);
        }
        vertex
    }

    fn adjust(&mut self, model: &mut CoseModel, axis: Axis) {
        for position in 0..self.order.len() {
            let vertex = self.order[position];
            let mut shift = displacement(model, self.dummies.representative(vertex), axis);
            let current = self.positions[&vertex];
            for neighbor in &self.neighbors[&vertex] {
                shift = keep_gap(shift, current, *neighbor, &self.positions);
            }
            self.positions.insert(vertex, current + shift);
            for node in self.dummies.nodes_of(vertex) {
                set_displacement(model, node, axis, shift);
            }
        }
    }
}

fn keep_gap(shift: f64, current: f64, neighbor: Neighbor, positions: &HashMap<Vertex, f64>) -> f64 {
    match neighbor {
        Neighbor::After(other, gap) => {
            let difference = (positions[&other] - current) - shift;
            if difference < gap {
                shift - (gap - difference)
            } else {
                shift
            }
        }
        Neighbor::Before(other, gap) => {
            let difference = (current - positions[&other]) + shift;
            if difference < gap {
                shift + (gap - difference)
            } else {
                shift
            }
        }
    }
}
