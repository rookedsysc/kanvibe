//! dagre `rank/`(network simplex) 이식. 순회 순서는 graphlib과 같게 맞춘다.

use std::collections::HashMap;

use crate::graph::{EdgeKey, Graph};

/// 순위 계산용 간선: 가중치와 최소 길이
#[derive(Clone, Copy, Debug, Default)]
pub struct RankEdge {
    pub weight: f64,
    pub minlen: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct TreeNode {
    low: i32,
    lim: i32,
    parent_set: bool,
}

#[derive(Clone, Debug, Default)]
struct TreeState {
    parent: HashMap<String, String>,
    cut_values: HashMap<String, f64>,
}

type RankGraph = Graph<i32, RankEdge>;
type TreeGraph = Graph<TreeNode, ()>;

fn slack(graph: &RankGraph, edge: &EdgeKey) -> i32 {
    graph.node(&edge.w) - graph.node(&edge.v) - graph.edge_by_key(edge).minlen
}

/// 다중 간선을 하나로 합친다(가중치는 더하고 최소 길이는 큰 쪽)
pub fn simplify(graph: &Graph<i32, RankEdge>) -> RankGraph {
    let mut simplified = RankGraph::new(true, false);
    for v in graph.nodes() {
        simplified.set_node(&v, *graph.node(&v));
    }
    for edge in graph.edges() {
        let label = *graph.edge_by_key(&edge);
        let previous = simplified
            .edge(&edge.v, &edge.w, None)
            .copied()
            .unwrap_or(RankEdge {
                weight: 0.0,
                minlen: 1,
            });
        let merged = RankEdge {
            weight: previous.weight + label.weight,
            minlen: previous.minlen.max(label.minlen),
        };
        simplified.set_edge(&edge.v, &edge.w, merged, None);
    }
    simplified
}

pub fn longest_path(graph: &mut RankGraph) {
    let mut visited: HashMap<String, bool> = HashMap::new();
    for source in graph.sources() {
        longest_path_dfs(graph, &mut visited, &source);
    }
}

fn longest_path_dfs(graph: &mut RankGraph, visited: &mut HashMap<String, bool>, v: &str) -> i32 {
    if visited.contains_key(v) {
        return *graph.node(v);
    }
    visited.insert(v.to_string(), true);
    let mut rank: Option<i32> = None;
    for edge in graph.out_edges(v) {
        let candidate = longest_path_dfs(graph, visited, &edge.w) - graph.edge_by_key(&edge).minlen;
        rank = Some(rank.map_or(candidate, |current| current.min(candidate)));
    }
    let rank = rank.unwrap_or(0);
    *graph.node_mut(v) = rank;
    rank
}

fn feasible_tree(graph: &mut RankGraph) -> TreeGraph {
    let mut tree = TreeGraph::new(false, false);
    let start = graph.nodes()[0].clone();
    let size = graph.node_count();
    tree.set_node(&start, TreeNode::default());
    while tight_tree(&mut tree, graph) < size {
        let edge = find_min_slack_edge(&tree, graph);
        let delta = if tree.has_node(&edge.v) {
            slack(graph, &edge)
        } else {
            -slack(graph, &edge)
        };
        for v in tree.nodes() {
            *graph.node_mut(&v) += delta;
        }
    }
    tree
}

fn tight_tree(tree: &mut TreeGraph, graph: &RankGraph) -> usize {
    for v in tree.nodes() {
        tight_tree_dfs(tree, graph, &v);
    }
    tree.node_count()
}

fn tight_tree_dfs(tree: &mut TreeGraph, graph: &RankGraph, v: &str) {
    for edge in graph.node_edges(v) {
        let w = if v == edge.v {
            edge.w.clone()
        } else {
            edge.v.clone()
        };
        if !tree.has_node(&w) && slack(graph, &edge) == 0 {
            tree.set_node(&w, TreeNode::default());
            tree.set_edge(v, &w, (), None);
            tight_tree_dfs(tree, graph, &w);
        }
    }
}

fn find_min_slack_edge(tree: &TreeGraph, graph: &RankGraph) -> EdgeKey {
    let mut best: Option<(i32, EdgeKey)> = None;
    for edge in graph.edges() {
        if tree.has_node(&edge.v) == tree.has_node(&edge.w) {
            continue;
        }
        let value = slack(graph, &edge);
        if best.as_ref().is_none_or(|(current, _)| value < *current) {
            best = Some((value, edge));
        }
    }
    best.expect("feasible tree needs a crossing edge").1
}

/// network simplex로 순위를 매긴다. 결과는 원래 그래프 노드 이름별 순위다
pub fn network_simplex(input: &Graph<i32, RankEdge>) -> HashMap<String, i32> {
    let mut graph = simplify(input);
    longest_path(&mut graph);
    let mut tree = feasible_tree(&mut graph);
    let mut state = TreeState::default();
    init_low_lim_values(&mut tree, &mut state);
    init_cut_values(&tree, &graph, &mut state);
    while let Some(leaving) = leave_edge(&tree, &state) {
        let entering = enter_edge(&tree, &graph, &leaving);
        tree.remove_edge(&leaving);
        tree.set_edge(&entering.v, &entering.w, (), None);
        init_low_lim_values(&mut tree, &mut state);
        init_cut_values(&tree, &graph, &mut state);
        update_ranks(&tree, &mut graph, &state);
    }
    graph
        .nodes()
        .into_iter()
        .map(|v| {
            let rank = *graph.node(&v);
            (v, rank)
        })
        .collect()
}

fn tree_edge_id(v: &str, w: &str) -> String {
    if v <= w {
        format!("{v}\u{1}{w}")
    } else {
        format!("{w}\u{1}{v}")
    }
}

fn init_low_lim_values(tree: &mut TreeGraph, state: &mut TreeState) {
    let root = tree.nodes()[0].clone();
    state.parent.clear();
    let mut visited: HashMap<String, bool> = HashMap::new();
    assign_low_lim(tree, state, &mut visited, 1, &root, None);
}

fn assign_low_lim(
    tree: &mut TreeGraph,
    state: &mut TreeState,
    visited: &mut HashMap<String, bool>,
    mut next_lim: i32,
    v: &str,
    parent: Option<&str>,
) -> i32 {
    let low = next_lim;
    visited.insert(v.to_string(), true);
    for w in tree.neighbors(v) {
        if !visited.contains_key(&w) {
            next_lim = assign_low_lim(tree, state, visited, next_lim, &w, Some(v));
        }
    }
    let label = tree.node_mut(v);
    label.low = low;
    label.lim = next_lim;
    label.parent_set = parent.is_some();
    next_lim += 1;
    if let Some(parent) = parent {
        state.parent.insert(v.to_string(), parent.to_string());
    }
    next_lim
}

fn init_cut_values(tree: &TreeGraph, graph: &RankGraph, state: &mut TreeState) {
    let mut order = postorder(tree);
    order.pop();
    for child in order {
        let parent = state.parent[&child].clone();
        let value = calc_cut_value(tree, graph, state, &child);
        state
            .cut_values
            .insert(tree_edge_id(&child, &parent), value);
    }
}

/// graphlib `alg.postorder(t, t.nodes())`
fn postorder(tree: &TreeGraph) -> Vec<String> {
    let mut visited: HashMap<String, bool> = HashMap::new();
    let mut order = Vec::new();
    for root in tree.nodes() {
        postorder_dfs(tree, &root, &mut visited, &mut order);
    }
    order
}

fn postorder_dfs(
    tree: &TreeGraph,
    v: &str,
    visited: &mut HashMap<String, bool>,
    order: &mut Vec<String>,
) {
    if visited.contains_key(v) {
        return;
    }
    visited.insert(v.to_string(), true);
    for w in tree.neighbors(v) {
        postorder_dfs(tree, &w, visited, order);
    }
    order.push(v.to_string());
}

fn preorder(tree: &TreeGraph, root: &str) -> Vec<String> {
    let mut visited: HashMap<String, bool> = HashMap::new();
    let mut order = Vec::new();
    preorder_dfs(tree, root, &mut visited, &mut order);
    order
}

fn preorder_dfs(
    tree: &TreeGraph,
    v: &str,
    visited: &mut HashMap<String, bool>,
    order: &mut Vec<String>,
) {
    if visited.contains_key(v) {
        return;
    }
    visited.insert(v.to_string(), true);
    order.push(v.to_string());
    for w in tree.neighbors(v) {
        preorder_dfs(tree, &w, visited, order);
    }
}

fn calc_cut_value(tree: &TreeGraph, graph: &RankGraph, state: &TreeState, child: &str) -> f64 {
    let parent = &state.parent[child];
    let (child_is_tail, graph_edge) = match graph.edge(child, parent, None) {
        Some(edge) => (true, *edge),
        None => (
            false,
            *graph
                .edge(parent, child, None)
                .expect("tree edge exists in graph"),
        ),
    };
    let mut cut_value = graph_edge.weight;
    for edge in graph.node_edges(child) {
        let is_out_edge = edge.v == child;
        let other = if is_out_edge { &edge.w } else { &edge.v };
        if other == parent {
            continue;
        }
        let points_to_head = is_out_edge == child_is_tail;
        let other_weight = graph.edge_by_key(&edge).weight;
        cut_value += if points_to_head {
            other_weight
        } else {
            -other_weight
        };
        if tree.has_edge(child, other, None) {
            let other_cut_value = state.cut_values[&tree_edge_id(child, other)];
            cut_value += if points_to_head {
                -other_cut_value
            } else {
                other_cut_value
            };
        }
    }
    cut_value
}

fn leave_edge(tree: &TreeGraph, state: &TreeState) -> Option<EdgeKey> {
    tree.edges().into_iter().find(|edge| {
        state
            .cut_values
            .get(&tree_edge_id(&edge.v, &edge.w))
            .copied()
            .unwrap_or(0.0)
            < 0.0
    })
}

fn enter_edge(tree: &TreeGraph, graph: &RankGraph, edge: &EdgeKey) -> EdgeKey {
    let (v, w) = if graph.has_edge(&edge.v, &edge.w, None) {
        (&edge.v, &edge.w)
    } else {
        (&edge.w, &edge.v)
    };
    let v_label = *tree.node(v);
    let w_label = *tree.node(w);
    let (tail_label, flip) = if v_label.lim > w_label.lim {
        (w_label, true)
    } else {
        (v_label, false)
    };
    let is_descendant =
        |label: &TreeNode| tail_label.low <= label.lim && label.lim <= tail_label.lim;
    let mut best: Option<(i32, EdgeKey)> = None;
    for candidate in graph.edges() {
        let qualifies = flip == is_descendant(tree.node(&candidate.v))
            && flip != is_descendant(tree.node(&candidate.w));
        if !qualifies {
            continue;
        }
        let value = slack(graph, &candidate);
        if best.as_ref().is_none_or(|(current, _)| value < *current) {
            best = Some((value, candidate));
        }
    }
    best.expect("network simplex needs an entering edge").1
}

fn update_ranks(tree: &TreeGraph, graph: &mut RankGraph, state: &TreeState) {
    // dagre는 그래프 라벨에 parent가 없는 첫 트리 노드를 뿌리로 쓴다. 순위 라벨에는 parent가 없으므로 첫 노드다
    let root = tree.nodes()[0].clone();
    for v in preorder(tree, &root).into_iter().skip(1) {
        let parent = state.parent[&v].clone();
        let (edge, is_flipped) = match graph.edge(&v, &parent, None) {
            Some(edge) => (*edge, false),
            None => (*graph.edge(&parent, &v, None).expect("tree edge"), true),
        };
        let parent_rank = *graph.node(&parent);
        *graph.node_mut(&v) = parent_rank
            + if is_flipped {
                edge.minlen
            } else {
                -edge.minlen
            };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 사슬은_한_칸씩_순위가_내려간다() {
        let mut graph: Graph<i32, RankEdge> = Graph::new(true, false);
        graph.set_edge(
            "a",
            "b",
            RankEdge {
                weight: 1.0,
                minlen: 1,
            },
            None,
        );
        graph.set_edge(
            "b",
            "c",
            RankEdge {
                weight: 1.0,
                minlen: 1,
            },
            None,
        );
        let ranks = network_simplex(&graph);
        assert_eq!(ranks["b"] - ranks["a"], 1);
        assert_eq!(ranks["c"] - ranks["b"], 1);
    }

    #[test]
    fn 짧은_간선이_긴_간선을_당긴다() {
        let mut graph: Graph<i32, RankEdge> = Graph::new(true, false);
        let unit = RankEdge {
            weight: 1.0,
            minlen: 1,
        };
        graph.set_edge("a", "b", unit, None);
        graph.set_edge("b", "c", unit, None);
        graph.set_edge("a", "d", unit, None);
        graph.set_edge("d", "c", unit, None);
        graph.set_edge("e", "c", unit, None);
        let ranks = network_simplex(&graph);
        assert_eq!(ranks["c"] - ranks["e"], 1);
    }
}
