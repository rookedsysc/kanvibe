//! cytoscape-fcose 2.2(cose-base 2.2·layout-base 2.0) 배치를 Mermaid architecture가 부르는 경로만 옮겼다.
//!
//! Mermaid는 `quality: "proof"`, `randomize: false`, `animate: false`에 정렬·상대 배치 제약을 넘긴다. 무작위
//! 스펙트럼 배치 없이 노드의 지금 자리에서 출발하는 증분(incremental) 배치라 결과가 결정적이다. 제약이 있으면
//! 타일링·나무 줄이기·성분 꾸리기도 꺼진다. 흐름은 제약 강제 → 힘 반복(최대 2500회, 100회마다 냉각) → 처음 묶음
//! 중심으로 되돌리기다. 부동소수 연산 순서까지 원본을 따라야 같은 좌표가 나온다.

mod constraint;
mod forces;
mod geometry;
mod graph;

use super::placement::RelativePlacement;
use constraint::{ModelConstraints, Relaxation};
use graph::CoseModel;

/// layout-base `DEFAULT_GRAPH_MARGIN`: 루트 그래프 경계 여백
const ROOT_GRAPH_MARGIN: f64 = 15.0;
/// layout-base `SIMPLE_NODE_SIZE`(= `EMPTY_COMPOUND_NODE_SIZE`)
const SIMPLE_NODE_SIZE: f64 = 40.0;
/// fcose `nestingFactor`
const PER_LEVEL_IDEAL_EDGE_LENGTH_FACTOR: f64 = 0.1;
const CONVERGENCE_CHECK_PERIOD: u32 = 100;
/// fcose `initialEnergyOnIncremental`
const INITIAL_COOLING_FACTOR: f64 = 0.3;
const FINAL_TEMPERATURE: f64 = 0.04;

/// cytoscape 노드 하나. 순서는 cytoscape에 넣은 순서다
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutNode {
    pub parent: Option<usize>,
    /// 지금 중심(묶음 노드는 쓰지 않는다)
    pub center: (f64, f64),
    pub width: f64,
    pub height: f64,
    /// 묶음 노드의 CSS `padding`(자식 그래프 여백)
    pub padding: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutEdge {
    pub source: usize,
    pub target: usize,
    pub ideal_length: f64,
    pub elasticity: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LayoutInput {
    pub nodes: Vec<LayoutNode>,
    pub edges: Vec<LayoutEdge>,
    /// 입력 노드 번호로 적은 정렬 묶음
    pub horizontal: Vec<Vec<usize>>,
    pub vertical: Vec<Vec<usize>>,
    pub relative: Vec<(RelativePlacement, f64)>,
    /// fcose `numIter`(노드 수 × 5보다 작으면 그쪽을 쓴다)
    pub max_iterations: f64,
}

/// 배치를 한 번 돌린 결과: 입력 노드마다 사각형 중심과 크기
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutBox {
    pub center: (f64, f64),
    pub width: f64,
    pub height: f64,
}

/// coseLayout + relocateComponent. 결과 전체 사각형의 가운데를 `original_center`로 옮긴다
pub(crate) fn run(input: &LayoutInput, original_center: (f64, f64)) -> Vec<LayoutBox> {
    let mut model = CoseModel::build(input);
    if model.nodes.is_empty() {
        return Vec::new();
    }
    let constraints = model_constraints(&model, input);
    model.prepare();
    let gravity_nodes = gravity_nodes(&model);
    constraint::enforce(&mut model, &constraints);
    let relaxation = Relaxation::new(&model, &constraints);
    let mut simulation = Simulation::new(model, gravity_nodes, relaxation, input.max_iterations);
    simulation.run_spring_embedder();
    relocate(&mut simulation.model, original_center);
    input_boxes(&simulation.model, input.nodes.len())
}

fn model_constraints(model: &CoseModel, input: &LayoutInput) -> ModelConstraints {
    let to_model = |nodes: &Vec<usize>| -> Vec<usize> {
        nodes
            .iter()
            .map(|&node| model.input_to_node[node])
            .collect()
    };
    let relative = input
        .relative
        .iter()
        .map(|&(placement, gap)| {
            let placement = match placement {
                RelativePlacement::Horizontal { left, right } => RelativePlacement::Horizontal {
                    left: model.input_to_node[left],
                    right: model.input_to_node[right],
                },
                RelativePlacement::Vertical { top, bottom } => RelativePlacement::Vertical {
                    top: model.input_to_node[top],
                    bottom: model.input_to_node[bottom],
                },
            };
            (placement, gap)
        })
        .collect();
    ModelConstraints {
        horizontal: input.horizontal.iter().map(to_model).collect(),
        vertical: input.vertical.iter().map(to_model).collect(),
        relative,
    }
}

/// calculateNodesToApplyGravitationTo: 이어지지 않은 그래프의 노드만 중력을 받는다
fn gravity_nodes(model: &CoseModel) -> Vec<usize> {
    (0..model.graphs.len())
        .filter(|&graph| !model.is_connected(graph))
        .flat_map(|graph| model.graphs[graph].nodes.iter().copied())
        .collect()
}

fn relocate(model: &mut CoseModel, original_center: (f64, f64)) {
    let mut bounds = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for node in &model.nodes {
        bounds.0 = bounds.0.min(node.rect.x);
        bounds.1 = bounds.1.max(node.rect.x + node.rect.width);
        bounds.2 = bounds.2.min(node.rect.y);
        bounds.3 = bounds.3.max(node.rect.y + node.rect.height);
    }
    let shift_x = original_center.0 - (bounds.1 + bounds.0) / 2.0;
    let shift_y = original_center.1 - (bounds.3 + bounds.2) / 2.0;
    for node in &mut model.nodes {
        let (center_x, center_y) = (node.rect.center_x(), node.rect.center_y());
        node.rect.set_center(center_x + shift_x, center_y + shift_y);
    }
}

fn input_boxes(model: &CoseModel, count: usize) -> Vec<LayoutBox> {
    (0..count)
        .map(|input| {
            let rect = model.nodes[model.input_to_node[input]].rect;
            LayoutBox {
                center: (rect.center_x(), rect.center_y()),
                width: rect.width,
                height: rect.height,
            }
        })
        .collect()
}

/// 힘 반복 상태(FDLayout·CoSELayout 필드)
pub(super) struct Simulation {
    model: CoseModel,
    gravity_nodes: Vec<usize>,
    relaxation: Relaxation,
    max_iterations: f64,
    default_edge_length: f64,
    min_repulsion_distance: f64,
    repulsion_range: f64,
    total_displacement_threshold: f64,
    total_iterations: u32,
    total_displacement: f64,
    old_total_displacement: f64,
    cooling_factor: f64,
    cooling_cycle: u32,
    max_cooling_cycle: f64,
    grid: Vec<Vec<Vec<usize>>>,
}

impl Simulation {
    fn new(
        model: CoseModel,
        gravity_nodes: Vec<usize>,
        relaxation: Relaxation,
        requested_iterations: f64,
    ) -> Self {
        let default_edge_length = model.default_edge_length;
        let ideal_edge_length = default_edge_length.max(10.0);
        let node_count = model.all_nodes.len() as f64;
        let max_iterations = (node_count * 5.0).max(requested_iterations);
        Self {
            gravity_nodes,
            relaxation,
            max_iterations,
            default_edge_length,
            min_repulsion_distance: default_edge_length / 10.0,
            // calcRepulsionRange: 2 × (level + 1) × idealEdgeLength, level은 0
            repulsion_range: 2.0 * ideal_edge_length,
            total_displacement_threshold: (3.0 * default_edge_length) / 100.0 * node_count,
            total_iterations: 0,
            total_displacement: 0.0,
            old_total_displacement: 0.0,
            cooling_factor: INITIAL_COOLING_FACTOR,
            cooling_cycle: 0,
            max_cooling_cycle: max_iterations / f64::from(CONVERGENCE_CHECK_PERIOD),
            grid: Vec::new(),
            model,
        }
    }

    fn run_spring_embedder(&mut self) {
        while !self.tick() {}
        self.model.update_bounds(0);
    }

    /// CoSELayout.tick. 끝났으면 참
    fn tick(&mut self) -> bool {
        self.total_iterations += 1;
        if f64::from(self.total_iterations) == self.max_iterations {
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
        self.model.update_bounds(0);
        self.calc_spring_forces();
        self.calc_repulsion_forces();
        self.calc_gravitational_forces();
        self.move_nodes();
        false
    }

    fn is_converged(&mut self) -> bool {
        let is_oscillating = f64::from(self.total_iterations) > self.max_iterations / 3.0
            && (self.total_displacement - self.old_total_displacement).abs() < 2.0;
        let is_converged = self.total_displacement < self.total_displacement_threshold;
        self.old_total_displacement = self.total_displacement;
        is_converged || is_oscillating
    }

    /// btluke 냉각 일정 3(quality "proof"라 coolingAdjuster는 1)
    fn cool_down(&mut self) {
        self.cooling_cycle += 1;
        let exponent = (100.0 * (INITIAL_COOLING_FACTOR - FINAL_TEMPERATURE)).ln()
            / self.max_cooling_cycle.ln();
        let cooled = INITIAL_COOLING_FACTOR - f64::from(self.cooling_cycle).powf(exponent) / 100.0;
        self.cooling_factor = cooled.max(FINAL_TEMPERATURE);
    }
}
