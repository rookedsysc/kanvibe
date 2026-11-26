//! layout-base `FDLayout`의 용수철·반발·중력 힘과 cose-base `CoSENode`의 변위·이동.
//!
//! 반발력은 FR 격자 변형이다. 10회마다(`totalIterations % 10 == 1`) 루트 그래프 경계로 격자를 다시 짜고, 노드마다
//! 둘레 칸에서 같은 그래프에 속한 이웃(`surrounding`)을 다시 모은다. 그 사이 반복은 이전 이웃 목록을 그대로 쓴다.

use super::Simulation;
use super::geometry::{clip_points, separation_amount, sign};

/// 노드 반발 세기(fcose `nodeRepulsion` 기본값)
const NODE_REPULSION: f64 = 4500.0;
const GRAVITY_CONSTANT: f64 = 0.25;
const COMPOUND_GRAVITY_CONSTANT: f64 = 1.0;
const GRAVITY_RANGE_FACTOR: f64 = 3.8;
const COMPOUND_GRAVITY_RANGE_FACTOR: f64 = 1.5;
const GRID_CALCULATION_CHECK_PERIOD: u32 = 10;
/// MAX_NODE_DISPLACEMENT_INCREMENTAL
const MAX_NODE_DISPLACEMENT: f64 = 100.0;

impl Simulation {
    pub(super) fn calc_spring_forces(&mut self) {
        for position in 0..self.model.all_edges.len() {
            let edge = self.model.all_edges[position];
            self.model.update_edge_length(edge);
            let edge = &self.model.edges[edge];
            if edge.is_overlapping || edge.length == 0.0 {
                continue;
            }
            let force = edge.elasticity * (edge.length - edge.ideal_length);
            let force_x = force * (edge.length_x / edge.length);
            let force_y = force * (edge.length_y / edge.length);
            let (source, target) = (edge.source, edge.target);
            let source = &mut self.model.nodes[source].spring;
            source.0 += force_x;
            source.1 += force_y;
            let target = &mut self.model.nodes[target].spring;
            target.0 -= force_x;
            target.1 -= force_y;
        }
    }

    pub(super) fn calc_repulsion_forces(&mut self) {
        let is_grid_iteration = self.total_iterations % GRID_CALCULATION_CHECK_PERIOD == 1;
        if is_grid_iteration {
            self.update_grid();
        }
        let mut processed = vec![false; self.model.nodes.len()];
        for position in 0..self.model.all_nodes.len() {
            let node = self.model.all_nodes[position];
            if is_grid_iteration {
                self.model.nodes[node].surrounding = self.surrounding_of(node, &processed);
            }
            for index in 0..self.model.nodes[node].surrounding.len() {
                let other = self.model.nodes[node].surrounding[index];
                self.calc_repulsion_force(node, other);
            }
            processed[node] = true;
        }
    }

    /// calcGrid + addNodeToGrid
    fn update_grid(&mut self) {
        let root = &self.model.graphs[0];
        let (left, top) = (root.left, root.top);
        let range = self.repulsion_range;
        let size_x = ((root.right - root.left) / range).ceil().max(0.0) as usize;
        let size_y = ((root.bottom - root.top) / range).ceil().max(0.0) as usize;
        self.grid = vec![vec![Vec::new(); size_y]; size_x];
        for position in 0..self.model.all_nodes.len() {
            let node = self.model.all_nodes[position];
            let rect = self.model.nodes[node].rect;
            let start_x = ((rect.x - left) / range).floor() as i64;
            let finish_x = ((rect.width + rect.x - left) / range).floor() as i64;
            let start_y = ((rect.y - top) / range).floor() as i64;
            let finish_y = ((rect.height + rect.y - top) / range).floor() as i64;
            self.place_in_grid(node, [start_x, finish_x, start_y, finish_y]);
        }
    }

    fn place_in_grid(&mut self, node: usize, span: [i64; 4]) {
        let [start_x, finish_x, start_y, finish_y] = span;
        for column in start_x..=finish_x {
            for row in start_y..=finish_y {
                if let Some(cell) = self.grid_cell_mut(column, row) {
                    cell.push(node);
                }
            }
        }
        self.model.nodes[node].grid_span = span;
    }

    fn grid_cell_mut(&mut self, column: i64, row: i64) -> Option<&mut Vec<usize>> {
        let column = usize::try_from(column).ok()?;
        let row = usize::try_from(row).ok()?;
        self.grid.get_mut(column)?.get_mut(row)
    }

    fn grid_cell(&self, column: i64, row: i64) -> &[usize] {
        let (Ok(column), Ok(row)) = (usize::try_from(column), usize::try_from(row)) else {
            return &[];
        };
        self.grid
            .get(column)
            .and_then(|cells| cells.get(row))
            .map_or(&[], Vec::as_slice)
    }

    /// calculateRepulsionForceOfANode의 이웃 모으기: 둘레 한 칸까지 보고 간격이 반발 범위 안인 노드
    fn surrounding_of(&self, node: usize, processed: &[bool]) -> Vec<usize> {
        let [start_x, finish_x, start_y, finish_y] = self.model.nodes[node].grid_span;
        let cells = (start_x - 1..finish_x + 2)
            .flat_map(|column| (start_y - 1..finish_y + 2).map(move |row| (column, row)));
        let mut surrounding: Vec<usize> = Vec::new();
        for (column, row) in cells {
            for &other in self.grid_cell(column, row) {
                if self.is_repulsion_neighbor(node, other, processed, &surrounding) {
                    surrounding.push(other);
                }
            }
        }
        surrounding
    }

    fn is_repulsion_neighbor(
        &self,
        node: usize,
        other: usize,
        processed: &[bool],
        surrounding: &[usize],
    ) -> bool {
        let (first, second) = (&self.model.nodes[node], &self.model.nodes[other]);
        if first.owner != second.owner || node == other {
            return false;
        }
        if processed[other] || surrounding.contains(&other) {
            return false;
        }
        let (a, b) = (first.rect, second.rect);
        let distance_x = (a.center_x() - b.center_x()).abs() - (a.width / 2.0 + b.width / 2.0);
        let distance_y = (a.center_y() - b.center_y()).abs() - (a.height / 2.0 + b.height / 2.0);
        distance_x <= self.repulsion_range && distance_y <= self.repulsion_range
    }

    fn calc_repulsion_force(&mut self, node: usize, other: usize) {
        let (a, b) = (self.model.nodes[node].rect, self.model.nodes[other].rect);
        let children_a = self.model.nodes[node].no_of_children;
        let children_b = self.model.nodes[other].no_of_children;
        let (force_x, force_y) = if a.intersects(&b) {
            let (overlap_x, overlap_y) = separation_amount(&a, &b, self.default_edge_length / 2.0);
            let children_constant = children_a * children_b / (children_a + children_b);
            (
                children_constant * (2.0 * overlap_x),
                children_constant * (2.0 * overlap_y),
            )
        } else {
            let clip = clip_points(&a, &b).unwrap_or_default();
            let mut distance_x = clip[2] - clip[0];
            let mut distance_y = clip[3] - clip[1];
            if distance_x.abs() < self.min_repulsion_distance {
                distance_x = sign(distance_x) * self.min_repulsion_distance;
            }
            if distance_y.abs() < self.min_repulsion_distance {
                distance_y = sign(distance_y) * self.min_repulsion_distance;
            }
            let distance_squared = distance_x * distance_x + distance_y * distance_y;
            let distance = distance_squared.sqrt();
            let force = (NODE_REPULSION / 2.0 + NODE_REPULSION / 2.0) * children_a * children_b
                / distance_squared;
            (force * distance_x / distance, force * distance_y / distance)
        };
        let first = &mut self.model.nodes[node].repulsion;
        first.0 -= force_x;
        first.1 -= force_y;
        let second = &mut self.model.nodes[other].repulsion;
        second.0 += force_x;
        second.1 += force_y;
    }

    pub(super) fn calc_gravitational_forces(&mut self) {
        for position in 0..self.gravity_nodes.len() {
            let node = self.gravity_nodes[position];
            let owner = self.model.nodes[node].owner;
            let graph = &self.model.graphs[owner];
            let rect = self.model.nodes[node].rect;
            let distance_x = rect.center_x() - (graph.right + graph.left) / 2.0;
            let distance_y = rect.center_y() - (graph.top + graph.bottom) / 2.0;
            let reach_x = distance_x.abs() + rect.width / 2.0;
            let reach_y = distance_y.abs() + rect.height / 2.0;
            let (range_factor, strength) = if owner == 0 {
                (GRAVITY_RANGE_FACTOR, 1.0)
            } else {
                (COMPOUND_GRAVITY_RANGE_FACTOR, COMPOUND_GRAVITY_CONSTANT)
            };
            let estimated = graph.estimated_size * range_factor;
            if reach_x > estimated || reach_y > estimated {
                self.model.nodes[node].gravitation = (
                    -GRAVITY_CONSTANT * distance_x * strength,
                    -GRAVITY_CONSTANT * distance_y * strength,
                );
            }
        }
    }

    /// CoSELayout.moveNodes: 변위 계산 → 제약 보정 → 이동
    pub(super) fn move_nodes(&mut self) {
        for position in 0..self.model.all_nodes.len() {
            let node = self.model.all_nodes[position];
            self.calculate_displacement(node);
        }
        self.relaxation.adjust(&mut self.model);
        for position in 0..self.model.all_nodes.len() {
            let node = self.model.all_nodes[position];
            self.move_node(node);
        }
    }

    fn calculate_displacement(&mut self, node: usize) {
        let limit = self.cooling_factor * MAX_NODE_DISPLACEMENT;
        let cooling = self.cooling_factor;
        let entry = &mut self.model.nodes[node];
        let children = entry.no_of_children;
        entry.displacement.0 +=
            cooling * (entry.spring.0 + entry.repulsion.0 + entry.gravitation.0) / children;
        entry.displacement.1 +=
            cooling * (entry.spring.1 + entry.repulsion.1 + entry.gravitation.1) / children;
        if entry.displacement.0.abs() > limit {
            entry.displacement.0 = limit * sign(entry.displacement.0);
        }
        if entry.displacement.1.abs() > limit {
            entry.displacement.1 = limit * sign(entry.displacement.1);
        }
        let (shift, child) = (entry.displacement, entry.child);
        if let Some(child) = child.filter(|&child| !self.model.graphs[child].nodes.is_empty()) {
            self.propagate_displacement(child, shift);
        }
    }

    fn propagate_displacement(&mut self, graph: usize, shift: (f64, f64)) {
        for position in 0..self.model.graphs[graph].nodes.len() {
            let node = self.model.graphs[graph].nodes[position];
            match self.model.nodes[node].child {
                None => {
                    let entry = &mut self.model.nodes[node].displacement;
                    entry.0 += shift.0;
                    entry.1 += shift.1;
                }
                Some(child) => self.propagate_displacement(child, shift),
            }
        }
    }

    fn move_node(&mut self, node: usize) {
        let is_movable = self.model.nodes[node]
            .child
            .is_none_or(|child| self.model.graphs[child].nodes.is_empty());
        let entry = &mut self.model.nodes[node];
        if is_movable {
            entry.rect.x += entry.displacement.0;
            entry.rect.y += entry.displacement.1;
            self.total_displacement += entry.displacement.0.abs() + entry.displacement.1.abs();
        }
        entry.spring = (0.0, 0.0);
        entry.repulsion = (0.0, 0.0);
        entry.gravitation = (0.0, 0.0);
        entry.displacement = (0.0, 0.0);
    }
}
