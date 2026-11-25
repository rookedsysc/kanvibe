//! cytoscape-cose-bilkent 4.1(cose-base 1.0·layout-base 1.0) 배치를 옮겼다.
//!
//! Mermaid 마인드맵은 `quality: "proof"`, `randomize: true`, `animate: false`로 이 배치를 부른다. 마인드맵은 늘
//! 나무 하나라서 cose-base는 무작위 흩뿌리기 대신 방사형 초기 배치(`positionNodesRadially`)를 쓰고, 연결 그래프라
//! 중력도 걸지 않는다. 그래서 결과가 결정적이다. 부동소수 연산 순서까지 원본을 따라야 같은 좌표가 나온다.

use crate::scene::Point;

/// layout-base `DEFAULT_GRAPH_MARGIN`. 그래프 경계는 노드 바깥으로 이만큼 넓다
const GRAPH_MARGIN: f64 = 15.0;
const IDEAL_EDGE_LENGTH: f64 = 50.0;
const SPRING_CONSTANT: f64 = 0.45;
const REPULSION_CONSTANT: f64 = 4500.0;
/// 초기 `maxIterations`. 냉각 주기 수와 최종 온도는 이 값으로 정한다
const DEFAULT_MAX_ITERATIONS: u32 = 2500;
const CONVERGENCE_CHECK_PERIOD: u32 = 100;
const GRID_CALCULATION_CHECK_PERIOD: u32 = 10;
/// `DEFAULT_EDGE_LENGTH / 10`
const MIN_REPULSION_DISTANCE: f64 = 5.0;
/// 겹친 두 노드를 떼어 놓을 때 더하는 여유(`DEFAULT_EDGE_LENGTH / 2`)
const SEPARATION_BUFFER: f64 = 25.0;
const MAX_NODE_DISPLACEMENT: f64 = 300.0;
/// `3 × DEFAULT_EDGE_LENGTH / 100`
const DISPLACEMENT_THRESHOLD_PER_NODE: f64 = 1.5;
/// `2 × (level + 1) × idealEdgeLength`, level은 늘 0
const REPULSION_RANGE: f64 = 100.0;
const RADIAL_SEPARATION: f64 = 50.0;
const WORLD_CENTER_X: f64 = 1200.0;
const WORLD_CENTER_Y: f64 = 900.0;
const WORLD_BOUNDARY: f64 = 1_000_000.0;

#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    fn intersects(&self, other: &Rect) -> bool {
        !(self.right() < other.x
            || self.bottom() < other.y
            || other.right() < self.x
            || other.bottom() < self.y)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct GridSpan {
    start_x: i64,
    finish_x: i64,
    start_y: i64,
    finish_y: i64,
}

#[derive(Clone, Debug, Default)]
struct CoseNode {
    rect: Rect,
    edges: Vec<usize>,
    spring_force: (f64, f64),
    repulsion_force: (f64, f64),
    grid_span: GridSpan,
    surrounding: Vec<usize>,
}

/// 방사형 배치에서 한 가지가 맡은 각도 범위(도)와 중심에서의 거리
#[derive(Clone, Copy, Debug)]
struct RadialBranch {
    start_angle: f64,
    end_angle: f64,
    distance: f64,
    separation: f64,
}

/// 그래프 경계(노드 바깥 + 여백)
#[derive(Clone, Copy, Debug, Default)]
struct GraphBounds {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

struct CoseLayout {
    nodes: Vec<CoseNode>,
    /// (source, target)
    edges: Vec<(usize, usize)>,
    bounds: GraphBounds,
    grid: Vec<Vec<Vec<usize>>>,
    total_iterations: u32,
    max_iterations: u32,
    cooling_cycle: u32,
    cooling_factor: f64,
    total_displacement: f64,
    old_total_displacement: f64,
}

/// 노드 크기(너비, 높이)와 부모→자식 간선을 받아 노드 중심 좌표를 돌려준다. 첫 노드에서 모든 노드에 닿아야 한다
pub(crate) fn layout(sizes: &[(f64, f64)], edges: &[(usize, usize)]) -> Vec<Point> {
    let mut state = CoseLayout::new(sizes, edges);
    if state.nodes.is_empty() {
        return Vec::new();
    }
    state.position_nodes_radially();
    state.run_spring_embedder();
    state.transform_left_top(0.0, 0.0);
    state
        .nodes
        .iter()
        .map(|node| Point::new(node.rect.center_x(), node.rect.center_y()))
        .collect()
}

impl CoseLayout {
    /// cytoscape 위치는 모두 (0, 0)이라 사각형 왼쪽 위가 (−w/2, −h/2)다. 같은 두 노드 사이 간선은 하나만 둔다
    fn new(sizes: &[(f64, f64)], edges: &[(usize, usize)]) -> Self {
        let mut nodes: Vec<CoseNode> = sizes
            .iter()
            .map(|&(width, height)| CoseNode {
                rect: Rect {
                    x: -width / 2.0,
                    y: -height / 2.0,
                    width,
                    height,
                },
                ..CoseNode::default()
            })
            .collect();
        let mut kept: Vec<(usize, usize)> = Vec::new();
        for &(source, target) in edges {
            let is_duplicate = kept
                .iter()
                .any(|&(a, b)| (a == source && b == target) || (a == target && b == source));
            if source == target || is_duplicate {
                continue;
            }
            nodes[source].edges.push(kept.len());
            nodes[target].edges.push(kept.len());
            kept.push((source, target));
        }
        Self {
            nodes,
            edges: kept,
            bounds: GraphBounds::default(),
            grid: Vec::new(),
            total_iterations: 0,
            max_iterations: DEFAULT_MAX_ITERATIONS,
            cooling_cycle: 0,
            cooling_factor: 1.0,
            total_displacement: 0.0,
            old_total_displacement: 0.0,
        }
    }

    fn other_end(&self, edge: usize, node: usize) -> usize {
        let (source, target) = self.edges[edge];
        if source == node { target } else { source }
    }

    /// 간선 순서대로 모은 이웃(중복 없음)
    fn neighbors(&self, node: usize) -> Vec<usize> {
        let mut result: Vec<usize> = Vec::new();
        for &edge in &self.nodes[node].edges {
            let other = self.other_end(edge, node);
            if !result.contains(&other) {
                result.push(other);
            }
        }
        result
    }

    /// getFlatForest의 BFS 방문 순서
    fn breadth_first_order(&self) -> Vec<usize> {
        let mut visited: Vec<usize> = Vec::new();
        let mut queue = std::collections::VecDeque::from([0usize]);
        let mut parents: Vec<Option<usize>> = vec![None; self.nodes.len()];
        while let Some(current) = queue.pop_front() {
            visited.push(current);
            for &edge in &self.nodes[current].edges {
                let neighbor = self.other_end(edge, current);
                if parents[current] != Some(neighbor) && !visited.contains(&neighbor) {
                    queue.push_back(neighbor);
                    parents[neighbor] = Some(current);
                }
            }
        }
        visited
    }

    /// Layout.findCenterOfTree. 원본은 돌던 목록에서 자기 자신을 지우며 건너뛰는데, 그 순서도 그대로 따른다
    fn find_center_of_tree(&self, tree: &[usize]) -> usize {
        let mut list: Vec<usize> = tree.to_vec();
        if list.len() <= 2 {
            return list[0];
        }
        let mut remaining_degrees: Vec<usize> = vec![0; self.nodes.len()];
        let mut removed: Vec<usize> = Vec::new();
        for &node in &list {
            remaining_degrees[node] = self.neighbors(node).len();
            if remaining_degrees[node] == 1 {
                removed.push(node);
            }
        }
        loop {
            let mut newly_removed: Vec<usize> = Vec::new();
            let mut index = 0;
            while index < list.len() {
                let node = list.remove(index);
                self.lower_neighbor_degrees(
                    node,
                    &removed,
                    &mut remaining_degrees,
                    &mut newly_removed,
                );
                index += 1;
            }
            removed.extend(newly_removed);
            if list.len() == 1 || list.len() == 2 {
                return list[0];
            }
        }
    }

    fn lower_neighbor_degrees(
        &self,
        node: usize,
        removed: &[usize],
        remaining_degrees: &mut [usize],
        newly_removed: &mut Vec<usize>,
    ) {
        for neighbor in self.neighbors(node) {
            if removed.contains(&neighbor) {
                continue;
            }
            remaining_degrees[neighbor] = remaining_degrees[neighbor].saturating_sub(1);
            if remaining_degrees[neighbor] == 1 {
                newly_removed.push(neighbor);
            }
        }
    }

    /// positionNodesRadially: 나무 하나를 (0, 0)에서 시작하는 방사형으로 놓고 세계 중심으로 옮긴다
    fn position_nodes_radially(&mut self) {
        let tree = self.breadth_first_order();
        let center = self.find_center_of_tree(&tree);
        let max_diagonal = tree
            .iter()
            .map(|&node| {
                let rect = self.nodes[node].rect;
                (rect.width * rect.width + rect.height * rect.height).sqrt()
            })
            .fold(f64::MIN, f64::max);
        let separation = max_diagonal.max(RADIAL_SEPARATION);
        let branch = RadialBranch {
            start_angle: 0.0,
            end_angle: 359.0,
            distance: 0.0,
            separation,
        };
        self.branch_radial_layout(center, None, branch);
        let (min_x, min_y, max_x, max_y) = self.node_extent(&tree);
        for &node in &tree {
            let rect = &mut self.nodes[node].rect;
            rect.x = clamp_world(rect.x) - min_x;
            rect.y = clamp_world(rect.y) - min_y;
        }
        let (extent_x, extent_y) = (max_x - min_x, max_y - min_y);
        self.transform_left_top(
            WORLD_CENTER_X - extent_x / 2.0,
            WORLD_CENTER_Y - extent_y / 2.0,
        );
    }

    fn node_extent(&self, tree: &[usize]) -> (f64, f64, f64, f64) {
        let mut extent = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for &node in tree {
            let rect = self.nodes[node].rect;
            extent.0 = extent.0.min(rect.x);
            extent.1 = extent.1.min(rect.y);
            extent.2 = extent.2.max(rect.right());
            extent.3 = extent.3.max(rect.bottom());
        }
        extent
    }

    /// CoSELayout.branchRadialLayout: 자기 각도 범위 가운데에 놓고, 자식들에게 범위를 고르게 나눠 준다
    fn branch_radial_layout(&mut self, node: usize, parent: Option<usize>, branch: RadialBranch) {
        let mut half_interval = ((branch.end_angle - branch.start_angle) + 1.0) / 2.0;
        if half_interval < 0.0 {
            half_interval += 180.0;
        }
        let node_angle = (half_interval + branch.start_angle) % 360.0;
        let theta = (node_angle * (2.0 * std::f64::consts::PI)) / 360.0;
        let rect = &mut self.nodes[node].rect;
        rect.x = branch.distance * theta.cos() - rect.width / 2.0;
        rect.y = branch.distance * theta.sin() - rect.height / 2.0;
        let children = self.children_after_parent(node, parent);
        let step_angle = (branch.end_angle - branch.start_angle).abs() / children.len() as f64;
        for (branch_count, child) in children.into_iter().enumerate() {
            let child_start = (branch.start_angle + branch_count as f64 * step_angle) % 360.0;
            let child_branch = RadialBranch {
                start_angle: child_start,
                end_angle: (child_start + step_angle) % 360.0,
                distance: branch.distance + branch.separation,
                separation: branch.separation,
            };
            self.branch_radial_layout(child, Some(node), child_branch);
        }
    }

    /// 부모로 가는 간선 다음 간선부터 한 바퀴 돌며 만난 이웃(부모 제외)
    fn children_after_parent(&self, node: usize, parent: Option<usize>) -> Vec<usize> {
        let edges = &self.nodes[node].edges;
        let start_index = parent
            .and_then(|parent| {
                edges
                    .iter()
                    .position(|&edge| self.other_end(edge, node) == parent)
            })
            .map_or(0, |index| index + 1);
        (0..edges.len())
            .map(|offset| self.other_end(edges[(start_index + offset) % edges.len()], node))
            .filter(|&neighbor| Some(neighbor) != parent)
            .collect()
    }

    /// Layout.transform(newLeftTop): 그래프 왼쪽 위(여백 포함)를 주어진 점으로 옮긴다
    fn transform_left_top(&mut self, left: f64, top: f64) {
        let indices: Vec<usize> = (0..self.nodes.len()).collect();
        let (min_x, min_y, _, _) = self.node_extent(&indices);
        let (device_left, device_top) = (min_x - GRAPH_MARGIN, min_y - GRAPH_MARGIN);
        for node in &mut self.nodes {
            node.rect.x = left + (clamp_world(node.rect.x) - device_left);
            node.rect.y = top + (clamp_world(node.rect.y) - device_top);
        }
    }

    fn run_spring_embedder(&mut self) {
        let node_count = self.nodes.len() as u32;
        self.max_iterations = (node_count * 5).max(DEFAULT_MAX_ITERATIONS);
        while !self.tick() {}
    }

    /// CoSELayout.tick. 끝났으면 true
    fn tick(&mut self) -> bool {
        self.total_iterations += 1;
        if self.total_iterations == self.max_iterations {
            return true;
        }
        if self
            .total_iterations
            .is_multiple_of(CONVERGENCE_CHECK_PERIOD)
        {
            if self.is_converged() {
                return true;
            }
            self.cool_down();
        }
        self.total_displacement = 0.0;
        self.update_bounds();
        self.calc_spring_forces();
        self.calc_repulsion_forces();
        self.move_nodes();
        false
    }

    fn is_converged(&mut self) -> bool {
        let is_oscillating = f64::from(self.total_iterations)
            > f64::from(self.max_iterations) / 3.0
            && (self.total_displacement - self.old_total_displacement).abs() < 2.0;
        let threshold = DISPLACEMENT_THRESHOLD_PER_NODE * self.nodes.len() as f64;
        let is_converged = self.total_displacement < threshold;
        self.old_total_displacement = self.total_displacement;
        is_converged || is_oscillating
    }

    /// 냉각 일정(proof 품질이라 coolingAdjuster는 1로 남는다)
    fn cool_down(&mut self) {
        self.cooling_cycle += 1;
        let initial = 1.0;
        let max_cooling_cycle = f64::from(DEFAULT_MAX_ITERATIONS / CONVERGENCE_CHECK_PERIOD);
        let final_temperature =
            f64::from(CONVERGENCE_CHECK_PERIOD) / f64::from(DEFAULT_MAX_ITERATIONS);
        let exponent = (100.0 * (initial - final_temperature)).ln() / max_cooling_cycle.ln();
        let drop = f64::from(self.cooling_cycle).powf(exponent) / 100.0 * 1.0;
        self.cooling_factor = (initial - drop).max(final_temperature);
    }

    fn update_bounds(&mut self) {
        let mut bounds = GraphBounds {
            left: f64::MAX,
            right: f64::MIN,
            top: f64::MAX,
            bottom: f64::MIN,
        };
        for node in &self.nodes {
            bounds.left = bounds.left.min(node.rect.x);
            bounds.right = bounds.right.max(node.rect.right());
            bounds.top = bounds.top.min(node.rect.y);
            bounds.bottom = bounds.bottom.max(node.rect.bottom());
        }
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        self.bounds = GraphBounds {
            left: bounds.left - GRAPH_MARGIN,
            right: bounds.left + width + GRAPH_MARGIN,
            top: bounds.top - GRAPH_MARGIN,
            bottom: bounds.top + height + GRAPH_MARGIN,
        };
    }

    fn calc_spring_forces(&mut self) {
        for index in 0..self.edges.len() {
            let (source, target) = self.edges[index];
            let (clip, is_overlapping) =
                clip_points(&self.nodes[target].rect, &self.nodes[source].rect);
            if is_overlapping {
                continue;
            }
            let mut length_x = clip[0] - clip[2];
            let mut length_y = clip[1] - clip[3];
            if length_x.abs() < 1.0 {
                length_x = sign(length_x);
            }
            if length_y.abs() < 1.0 {
                length_y = sign(length_y);
            }
            let length = (length_x * length_x + length_y * length_y).sqrt();
            if length == 0.0 {
                continue;
            }
            let spring_force = SPRING_CONSTANT * (length - IDEAL_EDGE_LENGTH);
            let force_x = spring_force * (length_x / length);
            let force_y = spring_force * (length_y / length);
            self.nodes[source].spring_force.0 += force_x;
            self.nodes[source].spring_force.1 += force_y;
            self.nodes[target].spring_force.0 -= force_x;
            self.nodes[target].spring_force.1 -= force_y;
        }
    }

    /// FR 격자 방식 반발력. 격자와 이웃 목록은 10회마다 새로 만든다
    fn calc_repulsion_forces(&mut self) {
        let should_refresh = self.total_iterations % GRID_CALCULATION_CHECK_PERIOD == 1;
        if should_refresh {
            self.update_grid();
        }
        let mut is_processed = vec![false; self.nodes.len()];
        for node in 0..self.nodes.len() {
            if should_refresh {
                self.nodes[node].surrounding = self.find_surrounding(node, &is_processed);
            }
            for other in self.nodes[node].surrounding.clone() {
                self.calc_repulsion_force(node, other);
            }
            is_processed[node] = true;
        }
    }

    fn update_grid(&mut self) {
        let bounds = self.bounds;
        let size_x = ((bounds.right - bounds.left) / REPULSION_RANGE).ceil() as usize;
        let size_y = ((bounds.bottom - bounds.top) / REPULSION_RANGE).ceil() as usize;
        self.grid = vec![vec![Vec::new(); size_y]; size_x];
        for node in 0..self.nodes.len() {
            let rect = self.nodes[node].rect;
            let cell =
                |offset: f64, origin: f64| ((offset - origin) / REPULSION_RANGE).floor() as i64;
            let span = GridSpan {
                start_x: cell(rect.x, bounds.left),
                finish_x: cell(rect.width + rect.x, bounds.left),
                start_y: cell(rect.y, bounds.top),
                finish_y: cell(rect.height + rect.y, bounds.top),
            };
            for i in span.start_x..=span.finish_x {
                for j in span.start_y..=span.finish_y {
                    self.grid[i as usize][j as usize].push(node);
                }
            }
            self.nodes[node].grid_span = span;
        }
    }

    fn find_surrounding(&self, node: usize, is_processed: &[bool]) -> Vec<usize> {
        let span = self.nodes[node].grid_span;
        let columns = (span.start_x - 1)..(span.finish_x + 2);
        let cells: Vec<(i64, i64)> = columns
            .flat_map(|i| ((span.start_y - 1)..(span.finish_y + 2)).map(move |j| (i, j)))
            .collect();
        let mut surrounding: Vec<usize> = Vec::new();
        for other in cells.into_iter().flat_map(|(i, j)| self.grid_cell(i, j)) {
            let is_candidate =
                other != node && !is_processed[other] && !surrounding.contains(&other);
            if is_candidate && self.is_within_repulsion_range(node, other) {
                surrounding.push(other);
            }
        }
        surrounding
    }

    fn grid_cell(&self, i: i64, j: i64) -> Vec<usize> {
        let is_outside = i < 0
            || j < 0
            || i as usize >= self.grid.len()
            || j as usize >= self.grid.first().map_or(0, Vec::len);
        if is_outside {
            return Vec::new();
        }
        self.grid[i as usize][j as usize].clone()
    }

    fn is_within_repulsion_range(&self, a: usize, b: usize) -> bool {
        let (first, second) = (self.nodes[a].rect, self.nodes[b].rect);
        let distance_x =
            (first.center_x() - second.center_x()).abs() - (first.width / 2.0 + second.width / 2.0);
        let distance_y = (first.center_y() - second.center_y()).abs()
            - (first.height / 2.0 + second.height / 2.0);
        distance_x <= REPULSION_RANGE && distance_y <= REPULSION_RANGE
    }

    /// FDLayout.calcRepulsionForce(잎 노드끼리라 자식 수는 모두 1)
    fn calc_repulsion_force(&mut self, a: usize, b: usize) {
        let (rect_a, rect_b) = (self.nodes[a].rect, self.nodes[b].rect);
        let (force_x, force_y) = if rect_a.intersects(&rect_b) {
            let (overlap_x, overlap_y) = separation_amount(&rect_a, &rect_b);
            let children_constant = 0.5;
            (
                children_constant * (2.0 * overlap_x),
                children_constant * (2.0 * overlap_y),
            )
        } else {
            let (clip, _) = clip_points(&rect_a, &rect_b);
            distant_repulsion(clip[2] - clip[0], clip[3] - clip[1])
        };
        self.nodes[a].repulsion_force.0 -= force_x;
        self.nodes[a].repulsion_force.1 -= force_y;
        self.nodes[b].repulsion_force.0 += force_x;
        self.nodes[b].repulsion_force.1 += force_y;
    }

    /// CoSENode.move
    fn move_nodes(&mut self) {
        let limit = self.cooling_factor * MAX_NODE_DISPLACEMENT;
        for node in &mut self.nodes {
            let mut displacement_x =
                self.cooling_factor * (node.spring_force.0 + node.repulsion_force.0);
            let mut displacement_y =
                self.cooling_factor * (node.spring_force.1 + node.repulsion_force.1);
            if displacement_x.abs() > limit {
                displacement_x = limit * sign(displacement_x);
            }
            if displacement_y.abs() > limit {
                displacement_y = limit * sign(displacement_y);
            }
            node.rect.x += displacement_x;
            node.rect.y += displacement_y;
            self.total_displacement += displacement_x.abs() + displacement_y.abs();
            node.spring_force = (0.0, 0.0);
            node.repulsion_force = (0.0, 0.0);
        }
    }
}

fn clamp_world(value: f64) -> f64 {
    value.clamp(-WORLD_BOUNDARY, WORLD_BOUNDARY)
}

/// IMath.sign
fn sign(value: f64) -> f64 {
    if value > 0.0 {
        1.0
    } else if value < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// 떨어진 두 노드 사이 반발력(거리가 너무 짧으면 최소 거리로 본다)
fn distant_repulsion(mut distance_x: f64, mut distance_y: f64) -> (f64, f64) {
    if distance_x.abs() < MIN_REPULSION_DISTANCE {
        distance_x = sign(distance_x) * MIN_REPULSION_DISTANCE;
    }
    if distance_y.abs() < MIN_REPULSION_DISTANCE {
        distance_y = sign(distance_y) * MIN_REPULSION_DISTANCE;
    }
    let distance_squared = distance_x * distance_x + distance_y * distance_y;
    let distance = distance_squared.sqrt();
    let repulsion = REPULSION_CONSTANT * 1.0 * 1.0 / distance_squared;
    (
        repulsion * distance_x / distance,
        repulsion * distance_y / distance,
    )
}

/// IGeometry.calcSeparationAmount: 겹친 두 사각형을 떼어 놓을 양
fn separation_amount(a: &Rect, b: &Rect) -> (f64, f64) {
    let direction_x = if a.center_x() < b.center_x() {
        -1.0
    } else {
        1.0
    };
    let direction_y = if a.center_y() < b.center_y() {
        -1.0
    } else {
        1.0
    };
    let mut overlap_x = a.right().min(b.right()) - a.x.max(b.x);
    let mut overlap_y = a.bottom().min(b.bottom()) - a.y.max(b.y);
    if a.x <= b.x && a.right() >= b.right() {
        overlap_x += (b.x - a.x).min(a.right() - b.right());
    } else if b.x <= a.x && b.right() >= a.right() {
        overlap_x += (a.x - b.x).min(b.right() - a.right());
    }
    if a.y <= b.y && a.bottom() >= b.bottom() {
        overlap_y += (b.y - a.y).min(a.bottom() - b.bottom());
    } else if b.y <= a.y && b.bottom() >= a.bottom() {
        overlap_y += (a.y - b.y).min(b.bottom() - a.bottom());
    }
    let mut slope = ((b.center_y() - a.center_y()) / (b.center_x() - a.center_x())).abs();
    if b.center_y() == a.center_y() && b.center_x() == a.center_x() {
        slope = 1.0;
    }
    let mut move_by_y = slope * overlap_x;
    let mut move_by_x = overlap_y / slope;
    if overlap_x < move_by_x {
        move_by_x = overlap_x;
    } else {
        move_by_y = overlap_y;
    }
    (
        -direction_x * ((move_by_x / 2.0) + SEPARATION_BUFFER),
        -direction_y * ((move_by_y / 2.0) + SEPARATION_BUFFER),
    )
}

/// IGeometry.getIntersection2: 두 중심을 잇는 선이 각 사각형 테두리와 만나는 점 [ax, ay, bx, by].
/// 두 사각형이 겹치면 중심을 돌려주고 true다
fn clip_points(a: &Rect, b: &Rect) -> ([f64; 4], bool) {
    let (p1x, p1y, p2x, p2y) = (a.center_x(), a.center_y(), b.center_x(), b.center_y());
    if a.intersects(b) {
        return ([p1x, p1y, p2x, p2y], true);
    }
    if p1x == p2x {
        let result = if p1y > p2y {
            [p1x, a.y, p2x, b.bottom()]
        } else {
            [p1x, a.bottom(), p2x, b.y]
        };
        return (result, false);
    }
    if p1y == p2y {
        let result = if p1x > p2x {
            [a.x, p1y, b.right(), p2y]
        } else {
            [a.right(), p1y, b.x, p2y]
        };
        return (result, false);
    }
    (diagonal_clip_points(a, b), false)
}

fn diagonal_clip_points(a: &Rect, b: &Rect) -> [f64; 4] {
    let (p1x, p1y, p2x, p2y) = (a.center_x(), a.center_y(), b.center_x(), b.center_y());
    let slope_a = a.height / a.width;
    let slope_b = b.height / b.width;
    let slope_prime = (p2y - p1y) / (p2x - p1x);
    let corner_a = corner_clip(a, slope_a, slope_prime, p1x > p2x);
    let corner_b = corner_clip(b, slope_b, slope_prime, p2x > p1x);
    let (line_a, line_b) = match (p1x > p2x, p1y > p2y) {
        (true, true) => ((slope_a, 4), (slope_b, 2)),
        (true, false) => ((-slope_a, 3), (-slope_b, 1)),
        (false, true) => ((-slope_a, 1), (-slope_b, 3)),
        (false, false) => ((slope_a, 2), (slope_b, 4)),
    };
    let point_a = corner_a.unwrap_or_else(|| {
        side_clip(
            a,
            p1x,
            p1y,
            slope_prime,
            cardinal_direction(line_a.0, slope_prime, line_a.1),
        )
    });
    let point_b = corner_b.unwrap_or_else(|| {
        side_clip(
            b,
            p2x,
            p2y,
            slope_prime,
            cardinal_direction(line_b.0, slope_prime, line_b.1),
        )
    });
    [point_a.0, point_a.1, point_b.0, point_b.1]
}

/// 중심선 기울기가 사각형 대각선과 정확히 같으면 모서리가 교점이다
fn corner_clip(
    rect: &Rect,
    slope: f64,
    slope_prime: f64,
    is_toward_left: bool,
) -> Option<(f64, f64)> {
    if -slope == slope_prime {
        return Some(if is_toward_left {
            (rect.x, rect.bottom())
        } else {
            (rect.right(), rect.y)
        });
    }
    if slope == slope_prime {
        return Some(if is_toward_left {
            (rect.x, rect.y)
        } else {
            (rect.right(), rect.bottom())
        });
    }
    None
}

fn cardinal_direction(slope: f64, slope_prime: f64, line: u8) -> u8 {
    if slope > slope_prime {
        line
    } else {
        1 + line % 4
    }
}

fn side_clip(
    rect: &Rect,
    center_x: f64,
    center_y: f64,
    slope_prime: f64,
    direction: u8,
) -> (f64, f64) {
    let (half_width, half_height) = (rect.width / 2.0, rect.height / 2.0);
    match direction {
        1 => (center_x + (-half_height) / slope_prime, rect.y),
        2 => (rect.right(), center_y + half_width * slope_prime),
        3 => (center_x + half_height / slope_prime, rect.bottom()),
        _ => (rect.x, center_y + (-half_width) * slope_prime),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기준_svg의_마인드맵_좌표를_재현한다() {
        // mindmap-basic--light.svg(HTML 라벨) 노드 크기와 translate 좌표
        let sizes = [
            (81.375, 81.375),
            (102.21875, 34.0),
            (78.453125, 34.0),
            (105.5, 34.0),
            (91.3125, 34.0),
            (74.765625, 34.0),
            (112.515625, 34.0),
            (88.1875, 34.0),
        ];
        let edges = [(0, 1), (1, 2), (1, 3), (0, 4), (4, 5), (4, 6), (0, 7)];
        let expected = [
            (155.98525943608888, 214.04642088471883),
            (155.89744134967964, 325.7822834715107),
            (261.7364459964349, 386.2595238332452),
            (67.75, 391.75378269559076),
            (131.38293977404214, 104.02185902817507),
            (59.96599082202124, 32.0),
            (255.5197896993118, 47.78768762644904),
            (294.35003279788407, 194.6896137269381),
        ];
        let centers = layout(&sizes, &edges);
        for (center, (x, y)) in centers.iter().zip(expected) {
            assert!(
                (center.x - x).abs() < 1e-6 && (center.y - y).abs() < 1e-6,
                "{center:?} != ({x}, {y})"
            );
        }
    }
}
