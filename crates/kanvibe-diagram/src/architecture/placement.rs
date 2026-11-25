//! architectureDb.getDataStructures와 architectureRenderer의 getAlignments·getRelativeConstraints.
//!
//! 간선의 두 변(`L`·`R`·`T`·`B`)으로 노드를 격자 칸에 놓고(spatial map), 같은 줄·같은 칸끼리 정렬 제약과 이웃 칸
//! 사이 상대 배치 제약(간격 1.5 × iconSize)을 만든다. 원본은 JS 객체를 키 순서대로 돌므로, 정수처럼 생긴 키(0, 1, …)가
//! 다른 키보다 먼저 오름차순으로 나오는 JS 규칙까지 따라야 제약 순서와 부동소수 합 순서가 같아진다.

use std::collections::{HashMap, HashSet, VecDeque};

use indexmap::IndexMap;

use super::parser::{Architecture, Side};

/// 이웃 칸 사이 상대 배치 제약. 노드 번호는 [`Architecture::nodes`] 순번이다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RelativePlacement {
    Horizontal { left: usize, right: usize },
    Vertical { top: usize, bottom: usize },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PlacementConstraints {
    /// 같은 가로줄(y)에 놓일 노드 묶음(alignmentConstraint.horizontal)
    pub horizontal: Vec<Vec<usize>>,
    /// 같은 세로줄(x)에 놓일 노드 묶음
    pub vertical: Vec<Vec<usize>>,
    pub relative: Vec<RelativePlacement>,
}

type Cell = (i32, i32);

/// JS가 객체 키를 도는 순서: 배열 첨자처럼 생긴 키를 오름차순으로 먼저, 나머지는 넣은 순서대로
pub(crate) fn js_key_order<K: AsRef<str>>(keys: &[K]) -> Vec<usize> {
    let index_value = |key: &str| -> Option<u64> {
        let is_canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
        let value = key.parse::<u64>().ok()?;
        (is_canonical
            && key.bytes().all(|byte| byte.is_ascii_digit())
            && value < u64::from(u32::MAX))
        .then_some(value)
    };
    let mut integer_keys: Vec<(u64, usize)> = Vec::new();
    let mut other_keys: Vec<usize> = Vec::new();
    for (position, key) in keys.iter().enumerate() {
        match index_value(key.as_ref()) {
            Some(value) => integer_keys.push((value, position)),
            None => other_keys.push(position),
        }
    }
    integer_keys.sort();
    integer_keys
        .into_iter()
        .map(|(_, position)| position)
        .chain(other_keys)
        .collect()
}

/// `this.nodes`를 `Object.entries`로 돈 순서의 노드 번호
pub(crate) fn node_order(architecture: &Architecture) -> Vec<usize> {
    let ids: Vec<&str> = architecture
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect();
    js_key_order(&ids)
}

pub(crate) fn placement_constraints(architecture: &Architecture) -> PlacementConstraints {
    let order = node_order(architecture);
    let (adjacency, group_alignments) = adjacency_lists(architecture, &order);
    let spatial_maps = spatial_maps(&order, &adjacency);
    let mut constraints = PlacementConstraints::default();
    for spatial_map in &spatial_maps {
        let (horizontal, vertical) = alignments(architecture, spatial_map, &group_alignments);
        constraints.horizontal.extend(horizontal);
        constraints.vertical.extend(vertical);
    }
    for spatial_map in &spatial_maps {
        constraints
            .relative
            .extend(relative_constraints(spatial_map));
    }
    constraints
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GroupAlignment {
    Horizontal,
    Vertical,
}

type GroupAlignments = HashMap<(String, String), GroupAlignment>;

/// 노드마다 `방향쌍 → 이웃` 목록(adjList)과 서로 다른 그룹을 잇는 간선이 정한 그룹 정렬
fn adjacency_lists(
    architecture: &Architecture,
    order: &[usize],
) -> (Vec<IndexMap<String, usize>>, GroupAlignments) {
    let mut adjacency = vec![IndexMap::new(); architecture.nodes.len()];
    let mut group_alignments = GroupAlignments::new();
    for &index in order {
        let id = &architecture.nodes[index].id;
        for edge in architecture
            .edges
            .iter()
            .filter(|edge| edge.source == *id || edge.target == *id)
        {
            record_group_alignment(architecture, edge, &mut group_alignments);
            let (near_side, far_side, other) = if edge.source == *id {
                (edge.source_side, edge.target_side, &edge.target)
            } else {
                (edge.target_side, edge.source_side, &edge.source)
            };
            if near_side != far_side
                && let Some(other_index) = architecture.node_index(other)
            {
                let pair = format!("{}{}", near_side.letter(), far_side.letter());
                adjacency[index].insert(pair, other_index);
            }
        }
    }
    (adjacency, group_alignments)
}

fn record_group_alignment(
    architecture: &Architecture,
    edge: &super::parser::ArchitectureEdge,
    group_alignments: &mut GroupAlignments,
) {
    let source_group = architecture
        .node(&edge.source)
        .and_then(|node| node.parent.clone());
    let target_group = architecture
        .node(&edge.target)
        .and_then(|node| node.parent.clone());
    let (Some(source_group), Some(target_group)) = (source_group, target_group) else {
        return;
    };
    if source_group == target_group {
        return;
    }
    let alignment = match (
        edge.source_side.is_horizontal(),
        edge.target_side.is_horizontal(),
    ) {
        (true, true) => GroupAlignment::Horizontal,
        (false, false) => GroupAlignment::Vertical,
        _ => return,
    };
    group_alignments.insert((source_group.clone(), target_group.clone()), alignment);
    group_alignments.insert((target_group, source_group), alignment);
}

/// shiftPositionByArchitectureDirectionPair: y가 1 늘면 위쪽 칸이다
fn shift_cell((x, y): Cell, pair: &str) -> Cell {
    let mut letters = pair.chars();
    let near = letters.next().unwrap_or('L');
    let far = letters.next().unwrap_or('R');
    let is_x = |letter: char| letter == 'L' || letter == 'R';
    let step_x = |letter: char| if letter == 'L' { -1 } else { 1 };
    let step_y = |letter: char| if letter == 'T' { 1 } else { -1 };
    match (is_x(near), is_x(far)) {
        (true, false) => (x + step_x(near), y + step_y(far)),
        (true, true) => (x + step_x(near), y),
        (false, true) => (x - step_x(far), y + step_y(near)),
        (false, false) => (x, y + step_y(near)),
    }
}

/// getDataStructures의 BFS. 방문하지 않은 이웃은 큐에 이미 있어도 다시 넣고 칸을 덮어쓴다
fn spatial_maps(
    order: &[usize],
    adjacency: &[IndexMap<String, usize>],
) -> Vec<IndexMap<usize, Cell>> {
    let Some(&first) = order.first() else {
        return Vec::new();
    };
    let mut visited: HashSet<usize> = HashSet::from([first]);
    let mut maps = Vec::new();
    let mut start = Some(first);
    while let Some(starting) = start {
        maps.push(breadth_first_cells(starting, adjacency, &mut visited));
        start = order.iter().copied().find(|index| !visited.contains(index));
    }
    maps
}

fn breadth_first_cells(
    starting: usize,
    adjacency: &[IndexMap<String, usize>],
    visited: &mut HashSet<usize>,
) -> IndexMap<usize, Cell> {
    let mut cells: IndexMap<usize, Cell> = IndexMap::from([(starting, (0, 0))]);
    let mut queue = VecDeque::from([starting]);
    while let Some(current) = queue.pop_front() {
        visited.insert(current);
        let cell = cells[&current];
        for (pair, &neighbor) in &adjacency[current] {
            if !visited.contains(&neighbor) {
                cells.insert(neighbor, shift_cell(cell, pair));
                queue.push_back(neighbor);
            }
        }
    }
    cells
}

/// 줄(또는 칸) 값 → 그룹 → 노드 목록. 두 단계 모두 JS 키 순서로 돈다
type LineGroups = Vec<(String, Vec<(String, Vec<usize>)>)>;

fn group_lines(
    architecture: &Architecture,
    spatial_map: &IndexMap<usize, Cell>,
    coordinate: impl Fn(Cell) -> i32,
) -> LineGroups {
    let mut lines: IndexMap<String, IndexMap<String, Vec<usize>>> = IndexMap::new();
    for (&node, &cell) in spatial_map {
        let group = architecture.nodes[node]
            .parent
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let line = lines.entry(coordinate(cell).to_string()).or_default();
        line.entry(group).or_default().push(node);
    }
    let keys: Vec<String> = lines.keys().cloned().collect();
    js_key_order(&keys)
        .into_iter()
        .map(|position| {
            let (key, groups) = lines.get_index(position).expect("줄 키");
            (key.clone(), in_js_order(groups))
        })
        .collect()
}

fn in_js_order(groups: &IndexMap<String, Vec<usize>>) -> Vec<(String, Vec<usize>)> {
    let keys: Vec<&String> = groups.keys().collect();
    js_key_order(&keys)
        .into_iter()
        .map(|position| {
            let (group, nodes) = groups.get_index(position).expect("그룹 키");
            (group.clone(), nodes.clone())
        })
        .collect()
}

/// flattenAlignments: 한 줄에 그룹이 여럿이면 그룹 정렬이 같거나 한쪽이 그룹 밖일 때만 한 묶음으로 합친다
fn flatten_alignments(
    lines: &LineGroups,
    direction: GroupAlignment,
    group_alignments: &GroupAlignments,
) -> Vec<Vec<usize>> {
    let mut flattened: IndexMap<String, Vec<usize>> = IndexMap::new();
    for (line, groups) in lines {
        if let [(_, nodes)] = groups.as_slice() {
            flattened.insert(line.clone(), nodes.clone());
        } else {
            flatten_line(line, groups, (direction, group_alignments), &mut flattened);
        }
    }
    let keys: Vec<String> = flattened.keys().cloned().collect();
    js_key_order(&keys)
        .into_iter()
        .map(|position| flattened[position].clone())
        .filter(|nodes| nodes.len() > 1)
        .collect()
}

/// 한 줄의 그룹 쌍마다 합치거나(같은 키에 이어 붙임) `줄-번호` 키로 따로 둔다
fn flatten_line(
    line: &str,
    groups: &[(String, Vec<usize>)],
    (direction, group_alignments): (GroupAlignment, &GroupAlignments),
    flattened: &mut IndexMap<String, Vec<usize>>,
) {
    let mut split_count = 0;
    for (first_index, (first_group, first_nodes)) in groups.iter().enumerate() {
        for (second_group, second_nodes) in &groups[first_index + 1..] {
            let alignment = group_alignments.get(&(first_group.clone(), second_group.clone()));
            let is_merged = alignment == Some(&direction)
                || first_group == "default"
                || second_group == "default";
            if is_merged {
                let merged = flattened.entry(line.to_string()).or_default();
                merged.extend(first_nodes);
                merged.extend(second_nodes);
            } else {
                flattened.insert(format!("{line}-{split_count}"), first_nodes.clone());
                flattened.insert(format!("{line}-{}", split_count + 1), second_nodes.clone());
                split_count += 2;
            }
        }
    }
}

fn alignments(
    architecture: &Architecture,
    spatial_map: &IndexMap<usize, Cell>,
    group_alignments: &GroupAlignments,
) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let rows = group_lines(architecture, spatial_map, |(_, y)| y);
    let columns = group_lines(architecture, spatial_map, |(x, _)| x);
    (
        flatten_alignments(&rows, GroupAlignment::Horizontal, group_alignments),
        flatten_alignments(&columns, GroupAlignment::Vertical, group_alignments),
    )
}

/// getRelativeConstraints: (0,0) 칸부터 BFS로 네 방향 이웃 칸을 이어 제약을 만든다. 칸은 꺼낼 때 방문 표시하므로
/// 큐에 두 번 들어간 칸이 같은 제약을 두 번 만들 수 있다
fn relative_constraints(spatial_map: &IndexMap<usize, Cell>) -> Vec<RelativePlacement> {
    let mut cell_owner: IndexMap<Cell, usize> = IndexMap::new();
    for (&node, &cell) in spatial_map {
        cell_owner.insert(cell, node);
    }
    let mut constraints = Vec::new();
    let mut visited: HashSet<Cell> = HashSet::new();
    let mut queue = VecDeque::from([(0, 0)]);
    while let Some(current) = queue.pop_front() {
        visited.insert(current);
        let Some(&current_node) = cell_owner.get(&current) else {
            continue;
        };
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            let next = neighbor_cell(current, side);
            if let Some(&next_node) = cell_owner.get(&next)
                && !visited.contains(&next)
            {
                queue.push_back(next);
                constraints.push(placement_between(side, next_node, current_node));
            }
        }
    }
    constraints
}

fn neighbor_cell((x, y): Cell, side: Side) -> Cell {
    match side {
        Side::Left => (x - 1, y),
        Side::Right => (x + 1, y),
        Side::Top => (x, y + 1),
        Side::Bottom => (x, y - 1),
    }
}

/// `side` 쪽 이웃이 `next`, 지금 칸이 `current`
fn placement_between(side: Side, next: usize, current: usize) -> RelativePlacement {
    match side {
        Side::Left => RelativePlacement::Horizontal {
            left: next,
            right: current,
        },
        Side::Right => RelativePlacement::Horizontal {
            left: current,
            right: next,
        },
        Side::Top => RelativePlacement::Vertical {
            top: next,
            bottom: current,
        },
        Side::Bottom => RelativePlacement::Vertical {
            top: current,
            bottom: next,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture::parser::parse;

    #[test]
    fn 정수_키를_먼저_오름차순으로_돈다() {
        assert_eq!(
            js_key_order(&["-1", "0", "b", "2", "1"]),
            vec![1, 4, 3, 0, 2]
        );
    }

    #[test]
    fn 기본_예제의_칸과_제약이_원본과_같다() {
        let architecture = parse(
            "architecture-beta\n group api(cloud)[API]\n service db(database)[Database] in api\n service disk1(disk)[Storage] in api\n service disk2(disk)[Storage] in api\n service server(server)[Server] in api\n db:L -- R:server\n disk1:T -- B:server\n disk2:T -- B:db\n",
        )
        .unwrap();
        let constraints = placement_constraints(&architecture);
        // 기준 브라우저(headless cytoscape)에서 찍은 값: db=0, disk1=1, disk2=2, server=3
        assert_eq!(constraints.horizontal, vec![vec![0, 3], vec![2, 1]]);
        assert_eq!(constraints.vertical, vec![vec![0, 2], vec![3, 1]]);
        assert_eq!(
            constraints.relative,
            vec![
                RelativePlacement::Horizontal { left: 3, right: 0 },
                RelativePlacement::Vertical { top: 0, bottom: 2 },
                RelativePlacement::Vertical { top: 3, bottom: 1 },
                RelativePlacement::Horizontal { left: 1, right: 2 },
            ]
        );
    }
}
