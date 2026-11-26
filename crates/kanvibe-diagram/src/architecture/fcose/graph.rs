//! cose-base `CoSEGraphManager`·`CoSEGraph`·`CoSENode`·`CoSEEdge`(layout-base LGraph 계열)의 자료 구조.
//!
//! 루트 그래프가 0번이고, 묶음(compound) 노드마다 자식 그래프를 하나씩 만든다. 노드 전체 순서(`getAllNodes`)는
//! 그래프를 만든 순서대로 각 그래프의 노드를 이어 붙인 것이고, 힘 계산의 덧셈 순서가 이 순서를 따른다.

use super::geometry::{Rect, sign};
use super::{LayoutInput, ROOT_GRAPH_MARGIN, SIMPLE_NODE_SIZE};

#[derive(Clone, Debug, Default)]
pub(super) struct CoseNode {
    pub rect: Rect,
    pub owner: usize,
    pub child: Option<usize>,
    pub edges: Vec<usize>,
    pub no_of_children: f64,
    pub estimated_size: f64,
    pub depth: i32,
    pub spring: (f64, f64),
    pub repulsion: (f64, f64),
    pub gravitation: (f64, f64),
    pub displacement: (f64, f64),
    /// 격자 칸 범위(startX, finishX, startY, finishY)
    pub grid_span: [i64; 4],
    pub surrounding: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CoseGraph {
    /// 이 그래프를 품은 묶음 노드. 루트는 `None`
    pub parent: Option<usize>,
    pub margin: f64,
    pub nodes: Vec<usize>,
    pub edges: Vec<usize>,
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub estimated_size: f64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CoseEdge {
    pub source: usize,
    pub target: usize,
    pub ideal_length: f64,
    pub elasticity: f64,
    pub is_inter_graph: bool,
    pub length: f64,
    pub length_x: f64,
    pub length_y: f64,
    pub is_overlapping: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct CoseModel {
    pub nodes: Vec<CoseNode>,
    pub graphs: Vec<CoseGraph>,
    pub edges: Vec<CoseEdge>,
    /// getAllNodes 순서
    pub all_nodes: Vec<usize>,
    /// getAllEdges 순서: 그래프 순서대로 그래프 안 간선, 그다음 그래프 사이 간선
    pub all_edges: Vec<usize>,
    /// 입력 노드 번호 → 모델 노드 번호
    pub input_to_node: Vec<usize>,
    /// processEdges가 실제로 넣은 간선들의 이상 길이 평균(DEFAULT_EDGE_LENGTH)
    pub default_edge_length: f64,
}

impl CoseModel {
    /// coseLayout의 processChildrenList·processEdges
    pub fn build(input: &LayoutInput) -> Self {
        let mut model = Self {
            graphs: vec![CoseGraph {
                margin: ROOT_GRAPH_MARGIN,
                ..CoseGraph::default()
            }],
            input_to_node: vec![usize::MAX; input.nodes.len()],
            ..Self::default()
        };
        let top_most: Vec<usize> = (0..input.nodes.len())
            .filter(|&index| input.nodes[index].parent.is_none())
            .collect();
        model.add_children(input, 0, &top_most);
        model.all_nodes = model
            .graphs
            .iter()
            .flat_map(|graph| graph.nodes.iter().copied())
            .collect();
        model.add_edges(input);
        model
    }

    fn add_children(&mut self, input: &LayoutInput, graph: usize, children: &[usize]) {
        for &child in children {
            let spec = &input.nodes[child];
            let node = self.nodes.len();
            self.nodes.push(CoseNode {
                rect: Rect {
                    x: spec.center.0 - spec.width / 2.0,
                    y: spec.center.1 - spec.height / 2.0,
                    width: spec.width,
                    height: spec.height,
                },
                owner: graph,
                ..CoseNode::default()
            });
            self.input_to_node[child] = node;
            self.graphs[graph].nodes.push(node);
            let grandchildren: Vec<usize> = (0..input.nodes.len())
                .filter(|&index| input.nodes[index].parent == Some(child))
                .collect();
            if !grandchildren.is_empty() {
                let child_graph = self.graphs.len();
                self.graphs.push(CoseGraph {
                    parent: Some(node),
                    margin: spec.padding,
                    ..CoseGraph::default()
                });
                self.nodes[node].child = Some(child_graph);
                self.add_children(input, child_graph, &grandchildren);
            }
        }
    }

    fn add_edges(&mut self, input: &LayoutInput) {
        let mut inter_graph_edges = Vec::new();
        let mut ideal_total = 0.0;
        for spec in &input.edges {
            let source = self.input_to_node[spec.source];
            let target = self.input_to_node[spec.target];
            if source == target || self.has_edge_between(source, target) {
                continue;
            }
            let edge = self.edges.len();
            let is_inter_graph = self.nodes[source].owner != self.nodes[target].owner;
            self.edges.push(CoseEdge {
                source,
                target,
                ideal_length: spec.ideal_length,
                elasticity: spec.elasticity,
                is_inter_graph,
                ..CoseEdge::default()
            });
            self.nodes[source].edges.push(edge);
            self.nodes[target].edges.push(edge);
            if is_inter_graph {
                inter_graph_edges.push(edge);
            } else {
                let owner = self.nodes[source].owner;
                self.graphs[owner].edges.push(edge);
            }
            ideal_total += spec.ideal_length;
        }
        let count = self.edges.len();
        // 이상 길이가 함수라서 간선이 없으면 50을 쓴다
        self.default_edge_length = if count > 0 {
            ideal_total / count as f64
        } else {
            50.0
        };
        self.all_edges = self
            .graphs
            .iter()
            .flat_map(|graph| graph.edges.iter().copied())
            .chain(inter_graph_edges)
            .collect();
    }

    fn has_edge_between(&self, first: usize, second: usize) -> bool {
        self.nodes[first].edges.iter().any(|&edge| {
            let edge = &self.edges[edge];
            edge.source == second || edge.target == second
        })
    }

    fn graph_of_parent(&self, graph: usize) -> Option<usize> {
        self.graphs[graph].parent
    }

    /// LGraph.getInclusionTreeDepth: 루트는 1, 나머지는 품은 노드의 깊이
    fn graph_depth(&self, graph: usize) -> i32 {
        self.graph_of_parent(graph)
            .map_or(1, |parent| self.nodes[parent].depth)
    }

    /// calcNoOfChildrenForAllNodes·calcInclusionTreeDepths·calcEstimatedSize·calcIdealEdgeLengths
    pub fn prepare(&mut self) {
        for index in 0..self.nodes.len() {
            self.nodes[index].no_of_children = self.leaf_count(index);
        }
        self.assign_depths(0, 1);
        self.estimate_graph_size(0);
        for position in 0..self.all_edges.len() {
            let edge = self.all_edges[position];
            if self.edges[edge].is_inter_graph {
                self.adjust_inter_graph_length(edge);
            }
        }
    }

    fn leaf_count(&self, node: usize) -> f64 {
        let Some(child) = self.nodes[node].child else {
            return 1.0;
        };
        let count: f64 = self.graphs[child]
            .nodes
            .iter()
            .map(|&grandchild| self.leaf_count(grandchild))
            .sum();
        if count == 0.0 { 1.0 } else { count }
    }

    fn assign_depths(&mut self, graph: usize, depth: i32) {
        for position in 0..self.graphs[graph].nodes.len() {
            let node = self.graphs[graph].nodes[position];
            self.nodes[node].depth = depth;
            if let Some(child) = self.nodes[node].child {
                self.assign_depths(child, depth + 1);
            }
        }
    }

    fn estimate_graph_size(&mut self, graph: usize) -> f64 {
        let mut size = 0.0;
        for position in 0..self.graphs[graph].nodes.len() {
            let node = self.graphs[graph].nodes[position];
            size += self.estimate_node_size(node);
        }
        let estimated = if size == 0.0 {
            SIMPLE_NODE_SIZE
        } else {
            size / (self.graphs[graph].nodes.len() as f64).sqrt()
        };
        self.graphs[graph].estimated_size = estimated;
        estimated
    }

    fn estimate_node_size(&mut self, node: usize) -> f64 {
        let estimated = match self.nodes[node].child {
            None => (self.nodes[node].rect.width + self.nodes[node].rect.height) / 2.0,
            Some(child) => {
                let estimated = self.estimate_graph_size(child);
                self.nodes[node].rect.width = estimated;
                self.nodes[node].rect.height = estimated;
                estimated
            }
        };
        self.nodes[node].estimated_size = estimated;
        estimated
    }

    /// calcLowestCommonAncestors가 정한 공통 조상 그래프와 그 안의 두 대표 노드로 이상 길이를 늘린다
    fn adjust_inter_graph_length(&mut self, edge: usize) {
        let (source, target) = (self.edges[edge].source, self.edges[edge].target);
        let (lca, source_in_lca, target_in_lca) = self.lowest_common_ancestor(source, target);
        let original = self.edges[edge].ideal_length;
        let smart = self.nodes[source_in_lca].estimated_size
            + self.nodes[target_in_lca].estimated_size
            - 2.0 * SIMPLE_NODE_SIZE;
        let level_span =
            self.nodes[source].depth + self.nodes[target].depth - 2 * self.graph_depth(lca);
        let edge = &mut self.edges[edge];
        edge.ideal_length += smart;
        edge.ideal_length +=
            original * super::PER_LEVEL_IDEAL_EDGE_LENGTH_FACTOR * f64::from(level_span);
    }

    fn lowest_common_ancestor(&self, source: usize, target: usize) -> (usize, usize, usize) {
        let mut source_in_lca = source;
        let mut source_graph = self.nodes[source].owner;
        loop {
            let mut target_in_lca = target;
            let mut target_graph = self.nodes[target].owner;
            loop {
                if target_graph == source_graph {
                    return (target_graph, source_in_lca, target_in_lca);
                }
                let Some(parent) = self.graph_of_parent(target_graph) else {
                    break;
                };
                target_in_lca = parent;
                target_graph = self.nodes[parent].owner;
            }
            let Some(parent) = self.graph_of_parent(source_graph) else {
                return (0, source_in_lca, target);
            };
            source_in_lca = parent;
            source_graph = self.nodes[parent].owner;
        }
    }

    /// LGraph.updateBounds(true): 묶음 노드 사각형을 자식 그래프 경계로 다시 잡는다
    pub fn update_bounds(&mut self, graph: usize) {
        let mut bounds = (f64::MAX, -f64::MAX, f64::MAX, -f64::MAX);
        for position in 0..self.graphs[graph].nodes.len() {
            let node = self.graphs[graph].nodes[position];
            if let Some(child) = self.nodes[node].child {
                self.update_bounds(child);
                let child_graph = &self.graphs[child];
                self.nodes[node].rect = Rect {
                    x: child_graph.left,
                    y: child_graph.top,
                    width: child_graph.right - child_graph.left,
                    height: child_graph.bottom - child_graph.top,
                };
            }
            let rect = self.nodes[node].rect;
            bounds.0 = bounds.0.min(rect.x);
            bounds.1 = bounds.1.max(rect.right());
            bounds.2 = bounds.2.min(rect.y);
            bounds.3 = bounds.3.max(rect.bottom());
        }
        let margin = self.graphs[graph].margin;
        let (left, top) = (bounds.0, bounds.2);
        let (width, height) = (bounds.1 - left, bounds.3 - top);
        let graph = &mut self.graphs[graph];
        graph.left = left - margin;
        graph.right = left + width + margin;
        graph.top = top - margin;
        graph.bottom = top + height + margin;
    }

    /// LGraph.updateConnected: 첫 노드에서 간선(자식 포함)으로 그래프 안 노드를 모두 닿을 수 있는지
    pub fn is_connected(&self, graph: usize) -> bool {
        let nodes = &self.graphs[graph].nodes;
        let Some(&first) = nodes.first() else {
            return true;
        };
        let mut visited: Vec<usize> = self.with_children(first);
        let mut cursor = 0;
        while cursor < visited.len() {
            let current = visited[cursor];
            cursor += 1;
            for &edge in &self.nodes[current].edges {
                let Some(neighbor) = self.other_end_in_graph(edge, current, graph) else {
                    continue;
                };
                if !visited.contains(&neighbor) {
                    let reached = self.with_children(neighbor);
                    absorb_unvisited(&mut visited, reached);
                }
            }
        }
        let owned = visited
            .iter()
            .filter(|&&node| self.nodes[node].owner == graph)
            .count();
        visited.len() >= nodes.len() && owned == nodes.len()
    }

    /// 노드와 그 아래 모든 자손(LNode.withChildren)
    fn with_children(&self, node: usize) -> Vec<usize> {
        let mut collected = vec![node];
        if let Some(child) = self.nodes[node].child {
            for &grandchild in &self.graphs[child].nodes {
                collected.extend(self.with_children(grandchild));
            }
        }
        collected
    }

    fn other_end_in_graph(&self, edge: usize, node: usize, graph: usize) -> Option<usize> {
        let edge = &self.edges[edge];
        let mut other = if edge.source == node {
            edge.target
        } else {
            edge.source
        };
        loop {
            let owner = self.nodes[other].owner;
            if owner == graph {
                return Some(other);
            }
            other = self.graph_of_parent(owner)?;
        }
    }

    /// LEdge.updateLength: 두 사각형 테두리 사이 길이. 겹치면 길이를 갱신하지 않는다
    pub fn update_edge_length(&mut self, edge: usize) {
        let (source, target) = (self.edges[edge].source, self.edges[edge].target);
        let clip = super::geometry::clip_points(&self.nodes[target].rect, &self.nodes[source].rect);
        let edge = &mut self.edges[edge];
        edge.is_overlapping = clip.is_none();
        let Some(clip) = clip else {
            return;
        };
        edge.length_x = clip[0] - clip[2];
        edge.length_y = clip[1] - clip[3];
        if edge.length_x.abs() < 1.0 {
            edge.length_x = sign(edge.length_x);
        }
        if edge.length_y.abs() < 1.0 {
            edge.length_y = sign(edge.length_y);
        }
        edge.length = (edge.length_x * edge.length_x + edge.length_y * edge.length_y).sqrt();
    }
}

/// 방문 목록(JS `Set`)에 아직 없는 노드만 넣는 순서대로 더한다
fn absorb_unvisited(visited: &mut Vec<usize>, reached: Vec<usize>) {
    for node in reached {
        if !visited.contains(&node) {
            visited.push(node);
        }
    }
}
