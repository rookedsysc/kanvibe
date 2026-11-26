//! d3-sankey 0.12.3 `sankey()`를 옮겼다. 노드 깊이로 열을 정하고, 열마다 값에 비례한 높이로 쌓은 뒤 6번
//! 오른쪽→왼쪽, 왼쪽→오른쪽으로 이웃 링크 쪽으로 끌어당기며 겹침을 푼다. 정렬은 JS `Array.sort`처럼 안정 정렬이다.

use std::cmp::Ordering;

use super::parser::SankeyGraph;
use crate::DiagramError;

const ITERATIONS: usize = 6;

/// `nodeAlign` 함수(`sankeyLeft`·`sankeyRight`·`sankeyCenter`·`sankeyJustify`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeAlignment {
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug)]
pub struct LayoutOptions {
    pub width: f64,
    pub height: f64,
    pub node_width: f64,
    pub node_padding: f64,
    pub alignment: NodeAlignment,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutNode {
    pub value: f64,
    pub depth: usize,
    pub height: usize,
    pub layer: usize,
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
    pub source_links: Vec<usize>,
    pub target_links: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutLink {
    pub source: usize,
    pub target: usize,
    pub value: f64,
    pub width: f64,
    /// 출발 노드 쪽 선 가운데 y
    pub y0: f64,
    /// 도착 노드 쪽 선 가운데 y
    pub y1: f64,
}

#[derive(Clone, Debug, Default)]
pub struct SankeyLayout {
    pub nodes: Vec<LayoutNode>,
    pub links: Vec<LayoutLink>,
}

/// 실제 노드 사이 간격(`py`)과 범위를 들고 다니는 계산기
struct Solver {
    options: LayoutOptions,
    padding: f64,
    layout: SankeyLayout,
}

pub fn compute(graph: &SankeyGraph, options: LayoutOptions) -> Result<SankeyLayout, DiagramError> {
    let mut solver = Solver {
        options,
        padding: options.node_padding,
        layout: link_nodes(graph),
    };
    solver.compute_node_values();
    solver.compute_depths(true)?;
    solver.compute_depths(false)?;
    solver.compute_node_breadths();
    solver.compute_link_breadths();
    Ok(solver.layout)
}

fn link_nodes(graph: &SankeyGraph) -> SankeyLayout {
    let mut nodes = vec![LayoutNode::default(); graph.nodes.len()];
    let mut links = Vec::with_capacity(graph.flows.len());
    for (index, flow) in graph.flows.iter().enumerate() {
        nodes[flow.source].source_links.push(index);
        nodes[flow.target].target_links.push(index);
        links.push(LayoutLink {
            source: flow.source,
            target: flow.target,
            value: flow.value,
            ..LayoutLink::default()
        });
    }
    SankeyLayout { nodes, links }
}

impl Solver {
    fn compute_node_values(&mut self) {
        let SankeyLayout { nodes, links } = &mut self.layout;
        for node in nodes.iter_mut() {
            let outgoing: f64 = node.source_links.iter().map(|&l| links[l].value).sum();
            let incoming: f64 = node.target_links.iter().map(|&l| links[l].value).sum();
            node.value = outgoing.max(incoming);
        }
    }

    /// `computeNodeDepths`(앞으로) 또는 `computeNodeHeights`(뒤로). 노드 수보다 깊어지면 순환이다
    fn compute_depths(&mut self, is_forward: bool) -> Result<(), DiagramError> {
        let count = self.layout.nodes.len();
        let mut current: Vec<usize> = (0..count).collect();
        let mut level = 0;
        while !current.is_empty() {
            let mut is_next = vec![false; count];
            for &index in &current {
                let node = &mut self.layout.nodes[index];
                let neighbours = if is_forward {
                    node.depth = level;
                    node.source_links.clone()
                } else {
                    node.height = level;
                    node.target_links.clone()
                };
                for link in neighbours {
                    let link = &self.layout.links[link];
                    is_next[if is_forward { link.target } else { link.source }] = true;
                }
            }
            level += 1;
            if level > count {
                return Err(DiagramError::Syntax("sankey 순환 링크".into()));
            }
            current = (0..count).filter(|&index| is_next[index]).collect();
        }
        Ok(())
    }

    fn align(&self, node: &LayoutNode, layer_count: usize) -> f64 {
        let last = layer_count as f64 - 1.0;
        match self.options.alignment {
            NodeAlignment::Left => node.depth as f64,
            NodeAlignment::Right => last - node.height as f64,
            NodeAlignment::Justify if node.source_links.is_empty() => last,
            NodeAlignment::Justify => node.depth as f64,
            NodeAlignment::Center if !node.target_links.is_empty() => node.depth as f64,
            NodeAlignment::Center if node.source_links.is_empty() => 0.0,
            NodeAlignment::Center => {
                let nearest_target = node
                    .source_links
                    .iter()
                    .map(|&link| self.layout.nodes[self.layout.links[link].target].depth)
                    .min()
                    .unwrap_or(0);
                nearest_target as f64 - 1.0
            }
        }
    }

    fn compute_node_layers(&mut self) -> Vec<Vec<usize>> {
        let layer_count = self
            .layout
            .nodes
            .iter()
            .map(|node| node.depth)
            .max()
            .unwrap_or(0)
            + 1;
        let step = (self.options.width - self.options.node_width) / (layer_count as f64 - 1.0);
        let mut columns: Vec<Vec<usize>> = vec![Vec::new(); layer_count];
        for index in 0..self.layout.nodes.len() {
            let aligned = self.align(&self.layout.nodes[index], layer_count).floor();
            let layer = aligned.min(layer_count as f64 - 1.0).max(0.0) as usize;
            let node = &mut self.layout.nodes[index];
            node.layer = layer;
            node.x0 = layer as f64 * step;
            node.x1 = node.x0 + self.options.node_width;
            columns[layer].push(index);
        }
        columns.retain(|column| !column.is_empty());
        columns
    }

    fn compute_node_breadths(&mut self) {
        let columns = &mut self.compute_node_layers();
        let tallest = columns.iter().map(Vec::len).max().unwrap_or(1);
        self.padding = self
            .options
            .node_padding
            .min(self.options.height / (tallest as f64 - 1.0));
        self.initialize_node_breadths(columns);
        for iteration in 0..ITERATIONS {
            let alpha = 0.99_f64.powi(iteration as i32);
            let beta = (1.0 - alpha).max((iteration + 1) as f64 / ITERATIONS as f64);
            self.relax(columns, alpha, beta, false);
            self.relax(columns, alpha, beta, true);
        }
    }

    fn initialize_node_breadths(&mut self, columns: &[Vec<usize>]) {
        let height = self.options.height;
        let scale = columns
            .iter()
            .map(|column| {
                let total: f64 = column.iter().map(|&n| self.layout.nodes[n].value).sum();
                (height - (column.len() as f64 - 1.0) * self.padding) / total
            })
            .fold(f64::INFINITY, f64::min);
        for column in columns {
            let mut y = 0.0;
            for &index in column {
                let node = &mut self.layout.nodes[index];
                node.y0 = y;
                node.y1 = y + node.value * scale;
                y = node.y1 + self.padding;
                for &link in &node.source_links.clone() {
                    let link = &mut self.layout.links[link];
                    link.width = link.value * scale;
                }
            }
            let spread = (height - y + self.padding) / (column.len() as f64 + 1.0);
            for (position, &index) in column.iter().enumerate() {
                let node = &mut self.layout.nodes[index];
                node.y0 += spread * (position + 1) as f64;
                node.y1 += spread * (position + 1) as f64;
            }
            for &index in column {
                self.sort_source_links(index);
                self.sort_target_links(index);
            }
        }
    }

    /// `relaxLeftToRight`(`is_forward`) 또는 `relaxRightToLeft`
    fn relax(&mut self, columns: &mut [Vec<usize>], alpha: f64, beta: f64, is_forward: bool) {
        let count = columns.len();
        let order: Vec<usize> = if is_forward {
            (1..count).collect()
        } else {
            (0..count.saturating_sub(1)).rev().collect()
        };
        for column_index in order {
            for &index in &columns[column_index] {
                self.pull_toward_neighbours(index, alpha, is_forward);
            }
            let nodes = &self.layout.nodes;
            columns[column_index].sort_by(|&a, &b| {
                nodes[a]
                    .y0
                    .partial_cmp(&nodes[b].y0)
                    .unwrap_or(Ordering::Equal)
            });
            self.resolve_collisions(&columns[column_index], beta);
        }
    }

    /// 들어오는(또는 나가는) 링크가 곧게 이어질 위치의 값 가중 평균 쪽으로 노드를 옮긴다
    fn pull_toward_neighbours(&mut self, index: usize, alpha: f64, is_forward: bool) {
        let node = &self.layout.nodes[index];
        let links = if is_forward {
            &node.target_links
        } else {
            &node.source_links
        };
        let (mut weighted, mut total) = (0.0, 0.0);
        for &link_index in links {
            let link = &self.layout.links[link_index];
            let (source, target) = (link.source, link.target);
            let weight = link.value
                * (self.layout.nodes[target].layer as f64 - self.layout.nodes[source].layer as f64);
            let ideal = if is_forward {
                self.target_top(source, target)
            } else {
                self.source_top(source, target)
            };
            weighted += ideal * weight;
            total += weight;
        }
        if total.partial_cmp(&0.0) != Some(Ordering::Greater) {
            return;
        }
        let node = &mut self.layout.nodes[index];
        let shift = (weighted / total - node.y0) * alpha;
        node.y0 += shift;
        node.y1 += shift;
        self.reorder_node_links(index);
    }

    fn resolve_collisions(&mut self, column: &[usize], alpha: f64) {
        let middle = column.len() >> 1;
        let subject = &self.layout.nodes[column[middle]];
        let (above, below) = (subject.y0 - self.padding, subject.y1 + self.padding);
        self.push_up(column, above, middle as isize - 1, alpha);
        self.push_down(column, below, middle + 1, alpha);
        self.push_up(
            column,
            self.options.height,
            column.len() as isize - 1,
            alpha,
        );
        self.push_down(column, 0.0, 0, alpha);
    }

    /// `resolveCollisionsTopToBottom`: 겹친 노드를 아래로 민다
    fn push_down(&mut self, column: &[usize], mut y: f64, start: usize, alpha: f64) {
        for &index in column.iter().skip(start) {
            let node = &mut self.layout.nodes[index];
            let shift = (y - node.y0) * alpha;
            if shift > 1e-6 {
                node.y0 += shift;
                node.y1 += shift;
            }
            y = node.y1 + self.padding;
        }
    }

    /// `resolveCollisionsBottomToTop`: 겹친 노드를 위로 민다
    fn push_up(&mut self, column: &[usize], mut y: f64, start: isize, alpha: f64) {
        let mut position = start;
        while position >= 0 {
            let node = &mut self.layout.nodes[column[position as usize]];
            let shift = (node.y1 - y) * alpha;
            if shift > 1e-6 {
                node.y0 -= shift;
                node.y1 -= shift;
            }
            y = node.y0 - self.padding;
            position -= 1;
        }
    }

    fn reorder_node_links(&mut self, index: usize) {
        let node = &self.layout.nodes[index];
        let sources: Vec<usize> = node
            .target_links
            .iter()
            .map(|&link| self.layout.links[link].source)
            .collect();
        let targets: Vec<usize> = node
            .source_links
            .iter()
            .map(|&link| self.layout.links[link].target)
            .collect();
        sources
            .into_iter()
            .for_each(|source| self.sort_source_links(source));
        targets
            .into_iter()
            .for_each(|target| self.sort_target_links(target));
    }

    /// 나가는 링크를 도착 노드 위쪽 순(`ascendingTargetBreadth`)으로
    fn sort_source_links(&mut self, index: usize) {
        let (nodes, links) = (&self.layout.nodes, &self.layout.links);
        let mut sorted = nodes[index].source_links.clone();
        sorted.sort_by(|&a, &b| {
            let (first, second) = (&nodes[links[a].target], &nodes[links[b].target]);
            breadth_order(first.y0 - second.y0, a, b)
        });
        self.layout.nodes[index].source_links = sorted;
    }

    /// 들어오는 링크를 출발 노드 위쪽 순(`ascendingSourceBreadth`)으로
    fn sort_target_links(&mut self, index: usize) {
        let (nodes, links) = (&self.layout.nodes, &self.layout.links);
        let mut sorted = nodes[index].target_links.clone();
        sorted.sort_by(|&a, &b| {
            let (first, second) = (&nodes[links[a].source], &nodes[links[b].source]);
            breadth_order(first.y0 - second.y0, a, b)
        });
        self.layout.nodes[index].target_links = sorted;
    }

    /// 출발 노드에서 도착 노드로 곧게 이어지려면 도착 노드 y0이 있어야 할 자리(`targetTop`)
    fn target_top(&self, source: usize, target: usize) -> f64 {
        let (nodes, links) = (&self.layout.nodes, &self.layout.links);
        let source_node = &nodes[source];
        let mut y =
            source_node.y0 - (source_node.source_links.len() as f64 - 1.0) * self.padding / 2.0;
        for &link in &source_node.source_links {
            if links[link].target == target {
                break;
            }
            y += links[link].width + self.padding;
        }
        for &link in &nodes[target].target_links {
            if links[link].source == source {
                break;
            }
            y -= links[link].width;
        }
        y
    }

    /// 도착 노드로 곧게 이어지려면 출발 노드 y0이 있어야 할 자리(`sourceTop`)
    fn source_top(&self, source: usize, target: usize) -> f64 {
        let (nodes, links) = (&self.layout.nodes, &self.layout.links);
        let target_node = &nodes[target];
        let mut y =
            target_node.y0 - (target_node.target_links.len() as f64 - 1.0) * self.padding / 2.0;
        for &link in &target_node.target_links {
            if links[link].source == source {
                break;
            }
            y += links[link].width + self.padding;
        }
        for &link in &nodes[source].source_links {
            if links[link].target == target {
                break;
            }
            y -= links[link].width;
        }
        y
    }

    fn compute_link_breadths(&mut self) {
        let SankeyLayout { nodes, links } = &mut self.layout;
        for node in nodes.iter() {
            let mut y = node.y0;
            for &link in &node.source_links {
                links[link].y0 = y + links[link].width / 2.0;
                y += links[link].width;
            }
            let mut y = node.y0;
            for &link in &node.target_links {
                links[link].y1 = y + links[link].width / 2.0;
                y += links[link].width;
            }
        }
    }
}

/// `a.y0 - b.y0 || a.index - b.index`
fn breadth_order(difference: f64, first: usize, second: usize) -> Ordering {
    if difference != 0.0 && !difference.is_nan() {
        return difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal);
    }
    first.cmp(&second)
}
