//! treemap 나무 만들기(`buildHierarchy`·d3 `hierarchy().sum().sort()`)와 d3-hierarchy `treemap()` 배치.
//! 타일은 squarify(황금비), 구역 안쪽 여백은 위 35·나머지 10, 형제 사이는 `treemap.padding`이고 끝에 좌표를
//! 반올림한다.

use super::parser::TreemapRow;

/// 구역 머리(이름 줄) 높이
pub const SECTION_HEADER_HEIGHT: f64 = 25.0;
const SECTION_INNER_PADDING: f64 = 10.0;
/// squarify가 노리는 가로세로 비(황금비)
const SQUARIFY_RATIO: f64 = std::f64::consts::GOLDEN_RATIO;

#[derive(Clone, Debug, Default)]
pub struct TreeNode {
    pub name: String,
    pub class_name: Option<String>,
    pub value: f64,
    pub depth: usize,
    pub parent: Option<usize>,
    /// 비어 있으면 d3는 잎으로 본다(값 없는 빈 구역 포함)
    pub children: Vec<usize>,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// 0번이 이름 없는 뿌리인 노드 묶음
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<TreeNode>,
}

impl Tree {
    /// 넓이 우선 순서(d3 `descendants()`)
    pub fn breadth_first(&self) -> Vec<usize> {
        let mut order = vec![0];
        let mut position = 0;
        while position < order.len() {
            order.extend(self.nodes[order[position]].children.iter().copied());
            position += 1;
        }
        order
    }

    /// 앞 순서 깊이 우선으로 자식 없는 노드(d3 `leaves()`)
    pub fn leaves(&self) -> Vec<usize> {
        let mut leaves = Vec::new();
        let mut stack = vec![0];
        while let Some(index) = stack.pop() {
            let node = &self.nodes[index];
            if node.children.is_empty() {
                leaves.push(index);
            }
            stack.extend(node.children.iter().rev().copied());
        }
        leaves
    }
}

/// 들여쓰기로 부모를 찾아 나무를 만들고 값을 더한 뒤 값이 큰 순서로 형제를 정렬한다
pub fn build_tree(rows: &[TreemapRow]) -> Tree {
    let mut tree = Tree {
        nodes: vec![TreeNode::default()],
    };
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for row in rows {
        while stack.last().is_some_and(|(_, level)| *level >= row.level) {
            stack.pop();
        }
        let parent = stack.last().map_or(0, |(index, _)| *index);
        let index = tree.nodes.len();
        tree.nodes.push(TreeNode {
            name: row.name.clone(),
            class_name: row.class_name.clone(),
            value: row.value.unwrap_or(0.0),
            depth: tree.nodes[parent].depth + 1,
            parent: Some(parent),
            ..TreeNode::default()
        });
        tree.nodes[parent].children.push(index);
        if row.value.is_none() {
            stack.push((index, row.level));
        }
    }
    sum_values(&mut tree, 0);
    sort_children(&mut tree);
    tree
}

fn sum_values(tree: &mut Tree, index: usize) -> f64 {
    let children = tree.nodes[index].children.clone();
    let total: f64 = children.iter().map(|&child| sum_values(tree, child)).sum();
    tree.nodes[index].value += total;
    tree.nodes[index].value
}

fn sort_children(tree: &mut Tree) {
    for index in 0..tree.nodes.len() {
        let mut children = std::mem::take(&mut tree.nodes[index].children);
        children.sort_by(|&a, &b| {
            tree.nodes[b]
                .value
                .partial_cmp(&tree.nodes[a].value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        tree.nodes[index].children = children;
    }
}

/// 뿌리를 `width × height`에 놓고 앞 순서로 자식을 타일링한 뒤 반올림한다. 형제 사이는 `inner_padding`이다
pub fn place(tree: &mut Tree, width: f64, height: f64, inner_padding: f64) {
    let root = &mut tree.nodes[0];
    (root.x0, root.y0, root.x1, root.y1) = (0.0, 0.0, width, height);
    let mut stack = vec![0];
    while let Some(index) = stack.pop() {
        position_node(tree, index, inner_padding / 2.0);
        stack.extend(tree.nodes[index].children.iter().rev().copied());
    }
    for node in &mut tree.nodes {
        for coordinate in [&mut node.x0, &mut node.y0, &mut node.x1, &mut node.y1] {
            *coordinate = js_round(*coordinate);
        }
    }
}

/// JS `Math.round`(반은 +∞ 쪽)
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// 부모가 남긴 형제 간격의 절반만큼 줄이고, 자식이 있으면 안쪽 여백을 뺀 칸을 타일링한다(`positionNode`)
fn position_node(tree: &mut Tree, index: usize, half: f64) {
    let inset = if index == 0 { 0.0 } else { half };
    let node = &mut tree.nodes[index];
    let (x0, x1) = shrink(node.x0 + inset, node.x1 - inset);
    let (y0, y1) = shrink(node.y0 + inset, node.y1 - inset);
    (node.x0, node.y0, node.x1, node.y1) = (x0, y0, x1, y1);
    if node.children.is_empty() {
        return;
    }
    let (x0, x1) = shrink(
        x0 + SECTION_INNER_PADDING - half,
        x1 - (SECTION_INNER_PADDING - half),
    );
    let (y0, y1) = shrink(
        y0 + SECTION_HEADER_HEIGHT + SECTION_INNER_PADDING - half,
        y1 - (SECTION_INNER_PADDING - half),
    );
    squarify(tree, index, Region { x0, y0, x1, y1 });
}

/// 끝이 시작보다 앞서면 둘 다 가운데로 모은다
fn shrink(start: f64, end: f64) -> (f64, f64) {
    if end < start {
        let middle = (start + end) / 2.0;
        return (middle, middle);
    }
    (start, end)
}

#[derive(Clone, Copy, Debug)]
struct Region {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

/// d3 `squarifyRatio`: 가로세로 비가 나빠지기 전까지 한 줄에 자식을 모으고, 짧은 변을 따라 줄을 놓는다
fn squarify(tree: &mut Tree, parent: usize, mut region: Region) {
    let children = tree.nodes[parent].children.clone();
    let mut remaining = tree.nodes[parent].value;
    let (mut start, mut end) = (0, 0);
    while start < children.len() {
        let (dx, dy) = (region.x1 - region.x0, region.y1 - region.y0);
        let mut row_value = loop {
            let value = tree.nodes[children[end]].value;
            end += 1;
            if value != 0.0 || end >= children.len() {
                break value;
            }
        };
        let alpha = (dy / dx).max(dx / dy) / (remaining * SQUARIFY_RATIO);
        let (mut min_value, mut max_value) = (row_value, row_value);
        let mut min_ratio = aspect_ratio(row_value, min_value, max_value, alpha);
        while end < children.len() {
            let value = tree.nodes[children[end]].value;
            let ratio = aspect_ratio(
                row_value + value,
                min_value.min(value),
                max_value.max(value),
                alpha,
            );
            if ratio > min_ratio {
                break;
            }
            row_value += value;
            (min_value, max_value) = (min_value.min(value), max_value.max(value));
            min_ratio = ratio;
            end += 1;
        }
        place_row(
            tree,
            &children[start..end],
            row_value,
            remaining,
            &mut region,
        );
        remaining -= row_value;
        start = end;
    }
}

fn aspect_ratio(sum: f64, min_value: f64, max_value: f64, alpha: f64) -> f64 {
    let beta = sum * sum * alpha;
    (max_value / beta).max(beta / min_value)
}

/// 넓은 칸이면 세로 띠(`slice`), 높은 칸이면 가로 띠(`dice`)로 한 줄을 놓고 남은 칸을 줄인다
fn place_row(tree: &mut Tree, row: &[usize], row_value: f64, remaining: f64, region: &mut Region) {
    let (dx, dy) = (region.x1 - region.x0, region.y1 - region.y0);
    let is_dice = dx < dy;
    let mut band = *region;
    if is_dice {
        if remaining != 0.0 {
            region.y0 += dy * row_value / remaining;
        }
        band.y1 = if remaining != 0.0 {
            region.y0
        } else {
            region.y1
        };
    } else {
        if remaining != 0.0 {
            region.x0 += dx * row_value / remaining;
        }
        band.x1 = if remaining != 0.0 {
            region.x0
        } else {
            region.x1
        };
    }
    let span = if is_dice {
        band.x1 - band.x0
    } else {
        band.y1 - band.y0
    };
    let scale = if row_value != 0.0 {
        span / row_value
    } else {
        0.0
    };
    let mut cursor = if is_dice { band.x0 } else { band.y0 };
    for &index in row {
        let node = &mut tree.nodes[index];
        let next = cursor + node.value * scale;
        if is_dice {
            (node.x0, node.x1, node.y0, node.y1) = (cursor, next, band.y0, band.y1);
        } else {
            (node.x0, node.x1, node.y0, node.y1) = (band.x0, band.x1, cursor, next);
        }
        cursor = next;
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::*;

    #[test]
    fn 기준_svg와_같은_구역과_잎_좌표를_낸다() {
        let document = parse(
            "treemap-beta\n\"Section 1\"\n    \"Leaf 1.1\": 12\n    \"Section 1.2\"\n      \"Leaf 1.2.1\": 12\n\"Section 2\"\n    \"Leaf 2.1\": 20\n    \"Leaf 2.2\": 25",
        )
        .unwrap();
        let mut tree = build_tree(&document.rows);
        place(&mut tree, 1000.0, 400.0, 10.0);
        let rectangle = |name: &str| {
            let node = tree.nodes.iter().find(|node| node.name == name).unwrap();
            (node.x0, node.y0, node.x1 - node.x0, node.y1 - node.y0)
        };
        // treemap-basic--light.svg
        assert_eq!(rectangle("Section 2"), (10.0, 35.0, 636.0, 355.0));
        assert_eq!(rectangle("Section 1.2"), (666.0, 230.0, 314.0, 150.0));
        assert_eq!(rectangle("Leaf 2.2").2, 338.0);
        assert_eq!(rectangle("Leaf 1.2.1").3, 105.0);
    }
}
