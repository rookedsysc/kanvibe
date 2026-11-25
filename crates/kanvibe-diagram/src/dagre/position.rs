//! dagre `position/`: 층마다 y를 쌓고, Brandes-Köpf 네 방향 정렬의 중앙값으로 x를 정한다.

use std::collections::HashMap;

use super::{BorderSide, LabelPos, LayoutState};
use crate::graph::Graph;

type Conflicts = HashMap<String, HashMap<String, bool>>;
type Xs = HashMap<String, f64>;

pub(super) fn position(state: &mut LayoutState) {
    let layering = state.build_layer_matrix();
    position_y(state, &layering);
    for (v, x) in position_x(state, &layering) {
        state.graph.node_mut(&v).x = x;
    }
}

fn position_y(state: &mut LayoutState, layering: &[Vec<String>]) {
    let rank_sep = state.config.ranksep;
    let mut previous_y = 0.0;
    for layer in layering {
        let max_height = layer
            .iter()
            .map(|v| state.graph.node(v).height)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_height = if layer.is_empty() {
            f64::NAN
        } else {
            max_height
        };
        for v in layer {
            state.graph.node_mut(v).y = previous_y + max_height / 2.0;
        }
        previous_y += max_height + rank_sep;
    }
}

fn is_dummy(state: &LayoutState, v: &str) -> bool {
    state.graph.node(v).dummy.is_some()
}

fn order_of(state: &LayoutState, v: &str) -> usize {
    state.graph.node(v).order.unwrap_or(0)
}

fn add_conflict(conflicts: &mut Conflicts, v: &str, w: &str) {
    let (first, second) = if v > w { (w, v) } else { (v, w) };
    conflicts
        .entry(first.to_string())
        .or_default()
        .insert(second.to_string(), true);
}

fn has_conflict(conflicts: &Conflicts, v: &str, w: &str) -> bool {
    let (first, second) = if v > w { (w, v) } else { (v, w) };
    conflicts
        .get(first)
        .is_some_and(|inner| inner.contains_key(second))
}

fn find_other_inner_segment_node(state: &LayoutState, v: &str) -> Option<String> {
    if !is_dummy(state, v) {
        return None;
    }
    state
        .graph
        .predecessors(v)
        .into_iter()
        .find(|u| is_dummy(state, u))
}

fn find_type1_conflicts(state: &LayoutState, layering: &[Vec<String>], conflicts: &mut Conflicts) {
    for pair in layering.windows(2) {
        let (previous_layer, layer) = (&pair[0], &pair[1]);
        let mut k0 = 0;
        let mut scan_position = 0;
        let last_node = layer.last();
        for (index, v) in layer.iter().enumerate() {
            let w = find_other_inner_segment_node(state, v);
            let k1 = w
                .as_ref()
                .map_or(previous_layer.len(), |w| order_of(state, w));
            if w.is_none() && Some(v) != last_node {
                continue;
            }
            for scan_node in &layer[scan_position..=index] {
                add_type1_conflicts(state, conflicts, scan_node, (k0, k1));
            }
            scan_position = index + 1;
            k0 = k1;
        }
    }
}

fn scan_type2(
    state: &LayoutState,
    conflicts: &mut Conflicts,
    south: &[String],
    range: (usize, usize),
    north_borders: (Option<usize>, Option<usize>),
) {
    let (previous_north, next_north) = north_borders;
    for v in &south[range.0..range.1] {
        if !is_dummy(state, v) {
            continue;
        }
        for u in state.graph.predecessors(v) {
            if !is_dummy(state, &u) {
                continue;
            }
            let order = order_of(state, &u) as f64;
            // JS에서 undefined와의 비교는 항상 false다
            let is_before = previous_north.is_some_and(|value| order < value as f64);
            let is_after = next_north.is_some_and(|value| order > value as f64);
            if is_before || is_after {
                add_conflict(conflicts, &u, v);
            }
        }
    }
}

/// 안쪽 구간(k0..=k1) 밖의 위층 노드와 잇는 간선은 충돌이다. 더미끼리는 빼고
fn add_type1_conflicts(
    state: &LayoutState,
    conflicts: &mut Conflicts,
    scan_node: &str,
    window: (usize, usize),
) {
    let (k0, k1) = window;
    for u in state.graph.predecessors(scan_node) {
        let u_position = order_of(state, &u);
        let is_both_dummy = is_dummy(state, &u) && is_dummy(state, scan_node);
        if (u_position < k0 || k1 < u_position) && !is_both_dummy {
            add_conflict(conflicts, &u, scan_node);
        }
    }
}

fn find_type2_conflicts(state: &LayoutState, layering: &[Vec<String>], conflicts: &mut Conflicts) {
    for pair in layering.windows(2) {
        let (north, south) = (&pair[0], &pair[1]);
        // prevNorthPos는 -1에서 시작한다. -1보다 작은 order는 없으므로 None과 같다
        let mut previous_north: Option<usize> = None;
        let mut next_north: Option<usize> = None;
        let mut south_position = 0;
        for (lookahead, v) in south.iter().enumerate() {
            let is_border = state.graph.node(v).dummy == Some(super::DummyKind::Border);
            let border_predecessor = if is_border {
                state.graph.predecessors(v).first().cloned()
            } else {
                None
            };
            if let Some(first) = border_predecessor {
                next_north = Some(order_of(state, &first));
                scan_type2(
                    state,
                    conflicts,
                    south,
                    (south_position, lookahead),
                    (previous_north, next_north),
                );
                south_position = lookahead;
                previous_north = next_north;
            }
            // 원본(dagre-d3-es)은 이 호출이 반복문 안에 있다
            scan_type2(
                state,
                conflicts,
                south,
                (south_position, south.len()),
                (next_north, Some(north.len())),
            );
        }
    }
}

struct Alignment {
    root: HashMap<String, String>,
    align: HashMap<String, String>,
}

fn vertical_alignment(
    state: &LayoutState,
    layering: &[Vec<String>],
    conflicts: &Conflicts,
    is_up: bool,
) -> Alignment {
    let mut alignment = Alignment {
        root: HashMap::new(),
        align: HashMap::new(),
    };
    let mut position: HashMap<String, usize> = HashMap::new();
    for layer in layering {
        for (order, v) in layer.iter().enumerate() {
            alignment.root.insert(v.clone(), v.clone());
            alignment.align.insert(v.clone(), v.clone());
            position.insert(v.clone(), order);
        }
    }
    for layer in layering {
        let mut previous_index: i64 = -1;
        for v in layer {
            let mut neighbors = if is_up {
                state.graph.predecessors(v)
            } else {
                state.graph.successors(v)
            };
            neighbors.sort_by_key(|w| position[w]);
            let medians = median_neighbors(&neighbors);
            previous_index = align_to_medians(
                &mut alignment,
                conflicts,
                &position,
                v,
                medians,
                previous_index,
            );
        }
    }
    alignment
}

/// 정렬한 이웃의 가운데 하나 또는 둘
fn median_neighbors(neighbors: &[String]) -> &[String] {
    if neighbors.is_empty() {
        return neighbors;
    }
    let middle = (neighbors.len() as f64 - 1.0) / 2.0;
    &neighbors[middle.floor() as usize..=middle.ceil() as usize]
}

/// 충돌 없이 왼쪽부터 처음 가능한 가운데 이웃의 블록에 v를 붙이고, 새 previous_index를 돌려준다
fn align_to_medians(
    alignment: &mut Alignment,
    conflicts: &Conflicts,
    position: &HashMap<String, usize>,
    v: &str,
    medians: &[String],
    mut previous_index: i64,
) -> i64 {
    for w in medians {
        let w_position = position[w] as i64;
        let can_align = alignment.align[v] == v
            && previous_index < w_position
            && !has_conflict(conflicts, v, w);
        if can_align {
            join_block(&mut alignment.root, &mut alignment.align, v, w);
            previous_index = w_position;
        }
    }
    previous_index
}

/// w가 속한 블록에 v를 잇는다
fn join_block(
    root: &mut HashMap<String, String>,
    align: &mut HashMap<String, String>,
    v: &str,
    w: &str,
) {
    align.insert(w.to_string(), v.to_string());
    let w_root = root[w].clone();
    root.insert(v.to_string(), w_root.clone());
    align.insert(v.to_string(), w_root);
}

fn separation(state: &LayoutState, v: &str, w: &str, is_reverse: bool) -> f64 {
    let v_label = state.graph.node(v);
    let w_label = state.graph.node(w);
    let mut sum = v_label.width / 2.0;
    let side_delta = |label: &super::NodeLabel, sign: f64| match label.labelpos {
        Some(LabelPos::Left) => -sign * label.width / 2.0,
        Some(LabelPos::Right) => sign * label.width / 2.0,
        _ => 0.0,
    };
    let delta = side_delta(v_label, 1.0);
    if delta != 0.0 {
        sum += if is_reverse { delta } else { -delta };
    }
    sum += if v_label.dummy.is_some() {
        state.config.edgesep
    } else {
        state.config.nodesep
    } / 2.0;
    sum += if w_label.dummy.is_some() {
        state.config.edgesep
    } else {
        state.config.nodesep
    } / 2.0;
    sum += w_label.width / 2.0;
    let delta = side_delta(w_label, -1.0);
    if delta != 0.0 {
        sum += if is_reverse { delta } else { -delta };
    }
    sum
}

fn build_block_graph(
    state: &LayoutState,
    layering: &[Vec<String>],
    root: &HashMap<String, String>,
    is_reverse: bool,
) -> Graph<(), f64> {
    let mut block_graph: Graph<(), f64> = Graph::new(true, false);
    for layer in layering {
        let mut previous: Option<&String> = None;
        for v in layer {
            let v_root = &root[v];
            block_graph.ensure_node(v_root);
            if let Some(u) = previous {
                let u_root = &root[u];
                let previous_max = block_graph
                    .edge(u_root, v_root, None)
                    .copied()
                    .unwrap_or(0.0);
                let value = separation(state, v, u, is_reverse).max(previous_max);
                block_graph.set_edge(u_root, v_root, value, None);
            }
            previous = Some(v);
        }
    }
    block_graph
}

fn horizontal_compaction(
    state: &LayoutState,
    layering: &[Vec<String>],
    alignment: &Alignment,
    is_reverse: bool,
) -> Xs {
    let block_graph = build_block_graph(state, layering, &alignment.root, is_reverse);
    let border_type = if is_reverse {
        BorderSide::Left
    } else {
        BorderSide::Right
    };
    let mut xs: Xs = HashMap::new();
    let first_pass = iterate_blocks(&block_graph, true);
    for element in first_pass {
        let value = block_graph
            .in_edges(&element)
            .iter()
            .fold(0.0, |acc: f64, edge| {
                acc.max(xs[&edge.v] + block_graph.edge_by_key(edge))
            });
        xs.insert(element, value);
    }
    for element in iterate_blocks(&block_graph, false) {
        let min = block_graph
            .out_edges(&element)
            .iter()
            .fold(f64::INFINITY, |acc: f64, edge| {
                acc.min(xs[&edge.w] - block_graph.edge_by_key(edge))
            });
        let node = state.graph.node(&element);
        if min != f64::INFINITY && node.border_type != Some(border_type) {
            let current = xs[&element];
            xs.insert(element, current.max(min));
        }
    }
    for v in alignment.align.values() {
        let value = xs[&alignment.root[v]];
        xs.insert(v.clone(), value);
    }
    xs
}

/// 원본 iterate(): 스택으로 돌며 두 번째 방문 때 값을 정하는 순서
fn iterate_blocks(block_graph: &Graph<(), f64>, is_first_pass: bool) -> Vec<String> {
    let mut stack = block_graph.nodes();
    let mut visited: HashMap<String, bool> = HashMap::new();
    let mut order = Vec::new();
    while let Some(element) = stack.pop() {
        if visited.contains_key(&element) {
            order.push(element);
            continue;
        }
        visited.insert(element.clone(), true);
        stack.push(element.clone());
        let next = if is_first_pass {
            block_graph.predecessors(&element)
        } else {
            block_graph.successors(&element)
        };
        stack.extend(next);
    }
    order
}

fn alignment_width(state: &LayoutState, xs: &Xs) -> f64 {
    let mut max = f64::NEG_INFINITY;
    let mut min = f64::INFINITY;
    for (v, x) in xs {
        let half_width = state.graph.node(v).width / 2.0;
        max = max.max(x + half_width);
        min = min.min(x - half_width);
    }
    max - min
}

const ALIGNMENT_NAMES: [&str; 4] = ["ul", "ur", "dl", "dr"];

fn position_x(state: &LayoutState, layering: &[Vec<String>]) -> Xs {
    let mut conflicts: Conflicts = HashMap::new();
    find_type1_conflicts(state, layering, &mut conflicts);
    find_type2_conflicts(state, layering, &mut conflicts);
    let mut xss: Vec<Xs> = Vec::new();
    for is_up in [true, false] {
        let base: Vec<Vec<String>> = if is_up {
            layering.to_vec()
        } else {
            layering.iter().rev().cloned().collect()
        };
        for is_right in [false, true] {
            let adjusted: Vec<Vec<String>> = if is_right {
                base.iter()
                    .map(|layer| layer.iter().rev().cloned().collect())
                    .collect()
            } else {
                base.clone()
            };
            let alignment = vertical_alignment(state, &adjusted, &conflicts, is_up);
            let mut xs = horizontal_compaction(state, &adjusted, &alignment, is_right);
            if is_right {
                xs.values_mut().for_each(|x| *x = -*x);
            }
            xss.push(xs);
        }
    }
    let smallest = smallest_width_alignment(state, &xss);
    align_coordinates(&mut xss, smallest);
    balance(&xss)
}

fn smallest_width_alignment(state: &LayoutState, xss: &[Xs]) -> usize {
    let mut best = 0;
    let mut best_width = f64::INFINITY;
    for (index, xs) in xss.iter().enumerate() {
        let width = alignment_width(state, xs);
        if width < best_width {
            best_width = width;
            best = index;
        }
    }
    best
}

fn align_coordinates(xss: &mut [Xs], align_to: usize) {
    let target_min = xss[align_to]
        .values()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let target_max = xss[align_to]
        .values()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    for (index, name) in ALIGNMENT_NAMES.iter().enumerate() {
        if index == align_to {
            continue;
        }
        let xs = &mut xss[index];
        let delta = if name.ends_with('l') {
            target_min - xs.values().copied().fold(f64::INFINITY, f64::min)
        } else {
            target_max - xs.values().copied().fold(f64::NEG_INFINITY, f64::max)
        };
        if delta != 0.0 {
            xs.values_mut().for_each(|x| *x += delta);
        }
    }
}

fn balance(xss: &[Xs]) -> Xs {
    xss[0]
        .keys()
        .map(|v| {
            let mut values: Vec<f64> = xss.iter().map(|xs| xs[v]).collect();
            values.sort_by(|left, right| {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            });
            (v.clone(), (values[1] + values[2]) / 2.0)
        })
        .collect()
}
