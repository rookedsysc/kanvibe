//! Mermaid가 만든 cytoscape 그래프와 cytoscape 3.34의 경계 상자 계산(boundingBoxImpl·updateCompoundBounds).
//!
//! fcose는 배치가 끝나면 결과 전체의 가운데를 "배치 전 `eles.boundingBox()`의 가운데"로 옮긴다. 이 상자는 노드 몸통
//! 뿐 아니라 노드 아래 라벨(`text-valign: bottom`), 간선 몸통(굵기 3)과 간선 라벨, 안티앨리어싱 여유 1px까지 담는다.
//! 그룹(묶음 노드)은 자식 상자(라벨 포함, `compound-sizing-wrt-labels: include`)에 `padding`과 테두리 1을 더한
//! 크기이고, Mermaid는 그룹 사각형을 이 상자로 그린다.
//!
//! 꺾인 간선(한쪽 L·R, 다른 쪽 T·B)은 `curve-style: segments`라 가운데 점이 꺾는 점 하나다. 처음에는
//! `segment-weights: 0`, `segment-distances: 0.5`(시작점에서 수직으로 0.5)이고, 첫 배치가 끝나면(layoutstop)
//! Mermaid가 두 중심이 한 축에 나란하지 않은 간선마다 꺾는 점이 모서리에 오도록 무게·거리를 다시 정한다.
//! 두 번째 배치가 노드를 조금 옮기면 꺾는 점도 그 무게·거리로 따라 움직인다.

use super::config::ArchitectureConfig;
use super::parser::{Architecture, NodeKind, Side};
use super::placement::{js_key_order, node_order};
use crate::text::{FontBook, FontWeight};
/// 기본 스타일 `:parent { border-width: 1 }`
const COMPOUND_BORDER_WIDTH: f64 = 1.0;
/// cytoscape 기본 노드 폭(빈 그룹은 자식이 없어 보통 노드로 그린다)
const DEFAULT_NODE_SIZE: f64 = 30.0;
/// 기본 스타일 `edge { width: 3 }`
const EDGE_WIDTH: f64 = 3.0;
/// 간선 라벨 글자 크기(cytoscape 기본 `font-size: 16px`, 줄 높이 1). 노드 라벨은 `architecture.fontSize`다
const EDGE_LABEL_FONT_SIZE: f64 = 16.0;
/// updateBoundsFromLabel의 `marginOfError`
const LABEL_MARGIN: f64 = 2.0;
/// 브라우저 오차를 덮으려고 상자를 사방으로 넓히는 양
const ANTIALIAS_EXPANSION: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Bounds {
    pub const EMPTY: Self = Self {
        x1: f64::INFINITY,
        y1: f64::INFINITY,
        x2: f64::NEG_INFINITY,
        y2: f64::NEG_INFINITY,
    };

    pub fn around(center: (f64, f64), width: f64, height: f64) -> Self {
        Self {
            x1: center.0 - width / 2.0,
            y1: center.1 - height / 2.0,
            x2: center.0 + width / 2.0,
            y2: center.1 + height / 2.0,
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
            x2: self.x2.max(other.x2),
            y2: self.y2.max(other.y2),
        }
    }

    pub fn expand(self, amount: f64) -> Self {
        Self {
            x1: self.x1 - amount,
            y1: self.y1 - amount,
            x2: self.x2 + amount,
            y2: self.y2 + amount,
        }
    }

    pub fn width(&self) -> f64 {
        self.x2 - self.x1
    }

    pub fn height(&self) -> f64 {
        self.y2 - self.y1
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x1 + self.width() / 2.0, self.y1 + self.height() / 2.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CyElement {
    Group(usize),
    Node(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CyNode {
    pub element: CyElement,
    pub parent: Option<usize>,
    /// 캔버스로 잰 노드 라벨 폭(`Math.ceil(measureText)`). 제목 없는 서비스·접점·그룹은 `None`
    pub label_width: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CyEdge {
    pub source: usize,
    pub target: usize,
    pub source_side: Side,
    pub target_side: Side,
    pub label_width: Option<f64>,
}

/// segments 간선의 꺾는 점(`segment-weights`·`segment-distances`)
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SegmentWeight {
    /// 시작점(0)에서 끝점(1)까지 비율
    pub weight: f64,
    /// 시작→끝 방향을 시계 방향으로 90° 돌린 쪽으로 떨어진 거리
    pub distance: f64,
}

impl SegmentWeight {
    /// 스타일 `segment-weights: "0"`, `segment-distances: [0.5]`
    pub const INITIAL: Self = Self {
        weight: 0.0,
        distance: 0.5,
    };
}

/// cytoscape에 넣은 순서(그룹 → 서비스 → 접점)의 노드와 간선
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CyGraph {
    pub nodes: Vec<CyNode>,
    pub edges: Vec<CyEdge>,
    /// [`Architecture::nodes`] 번호 → cytoscape 노드 번호
    pub node_to_cy: Vec<usize>,
    pub config: ArchitectureConfig,
    children: Vec<Vec<usize>>,
}

fn canvas_label_width(fonts: &FontBook, label: &str, font_size: f64) -> f64 {
    fonts
        .shape_line(label, font_size, FontWeight::Regular)
        .width
        .ceil()
}

/// addGroups → addServices → addJunctions 순서. 각 목록은 DB 객체를 JS 키 순서로 돈 것이다
fn element_order(architecture: &Architecture) -> Vec<CyElement> {
    let group_ids: Vec<&str> = architecture
        .groups
        .iter()
        .map(|group| group.id.as_str())
        .collect();
    let mut elements: Vec<CyElement> = js_key_order(&group_ids)
        .into_iter()
        .map(CyElement::Group)
        .collect();
    let order = node_order(architecture);
    for kind in [NodeKind::Service, NodeKind::Junction] {
        let of_kind = order
            .iter()
            .filter(|&&index| architecture.nodes[index].kind == kind);
        elements.extend(of_kind.map(|&index| CyElement::Node(index)));
    }
    elements
}

fn cy_edges(architecture: &Architecture, node_to_cy: &[usize], fonts: &FontBook) -> Vec<CyEdge> {
    architecture
        .edges
        .iter()
        .filter_map(|edge| {
            Some(CyEdge {
                source: node_to_cy[architecture.node_index(&edge.source)?],
                target: node_to_cy[architecture.node_index(&edge.target)?],
                source_side: edge.source_side,
                target_side: edge.target_side,
                label_width: edge
                    .title
                    .as_deref()
                    .map(|title| canvas_label_width(fonts, title, EDGE_LABEL_FONT_SIZE)),
            })
        })
        .collect()
}

impl CyGraph {
    pub fn new(architecture: &Architecture, fonts: &FontBook, config: ArchitectureConfig) -> Self {
        let elements = element_order(architecture);
        let parent_id = |element: CyElement| match element {
            CyElement::Group(index) => architecture.groups[index].parent.as_deref(),
            CyElement::Node(index) => architecture.nodes[index].parent.as_deref(),
        };
        let group_position = |id: &str| {
            elements.iter().position(|&element| {
                matches!(element, CyElement::Group(index) if architecture.groups[index].id == id)
            })
        };
        let nodes: Vec<CyNode> = elements
            .iter()
            .map(|&element| CyNode {
                element,
                parent: parent_id(element).and_then(group_position),
                label_width: node_label(architecture, element)
                    .map(|label| canvas_label_width(fonts, label, config.font_size)),
            })
            .collect();
        let mut node_to_cy = vec![0; architecture.nodes.len()];
        for (position, node) in nodes.iter().enumerate() {
            if let CyElement::Node(index) = node.element {
                node_to_cy[index] = position;
            }
        }
        let edges = cy_edges(architecture, &node_to_cy, fonts);
        let children = (0..nodes.len())
            .map(|parent| {
                (0..nodes.len())
                    .filter(|&child| nodes[child].parent == Some(parent))
                    .collect()
            })
            .collect();
        Self {
            nodes,
            edges,
            node_to_cy,
            config,
            children,
        }
    }

    pub fn is_compound(&self, node: usize) -> bool {
        !self.children[node].is_empty()
    }

    /// 묶음이 아닌 노드의 크기: 서비스·접점은 iconSize, 빈 그룹은 기본 30 + padding
    pub fn leaf_size(&self, node: usize) -> f64 {
        match self.nodes[node].element {
            CyElement::Node(_) => self.config.icon_size,
            CyElement::Group(_) => DEFAULT_NODE_SIZE + 2.0 * self.config.padding,
        }
    }

    /// 노드 아래 라벨 상자(updateBoundsFromLabel). 라벨 맨 위는 노드 아래 끝이다
    fn label_bounds(&self, node: usize, center: (f64, f64)) -> Option<Bounds> {
        let width = self.nodes[node].label_width?;
        let top = center.1 + self.leaf_size(node) / 2.0;
        Some(Bounds {
            x1: center.0 - width / 2.0 - LABEL_MARGIN,
            y1: top - LABEL_MARGIN,
            x2: center.0 + width / 2.0 + LABEL_MARGIN,
            y2: top + self.config.font_size + LABEL_MARGIN,
        })
    }

    /// 노드 바깥 사각형(outerWidth·outerHeight). `centers`는 묶음이 아닌 노드 중심이다
    pub fn outer_rects(&self, centers: &[(f64, f64)]) -> Vec<Bounds> {
        let mut rects = vec![Bounds::EMPTY; self.nodes.len()];
        for node in 0..self.nodes.len() {
            self.fill_outer_rect(node, centers, &mut rects);
        }
        rects
    }

    fn fill_outer_rect(&self, node: usize, centers: &[(f64, f64)], rects: &mut [Bounds]) {
        if rects[node] != Bounds::EMPTY {
            return;
        }
        if !self.is_compound(node) {
            let size = self.leaf_size(node);
            rects[node] = Bounds::around(centers[node], size, size);
            return;
        }
        let mut children = Bounds::EMPTY;
        for &child in &self.children[node] {
            self.fill_outer_rect(child, centers, rects);
            let body = rects[child].expand(ANTIALIAS_EXPANSION);
            let label = self.label_bounds(child, centers[child]);
            children = children.union(label.map_or(body, |label| body.union(label)));
        }
        let outer_extra = 2.0 * self.config.padding + COMPOUND_BORDER_WIDTH;
        rects[node] = Bounds::around(
            children.center(),
            children.width() + outer_extra,
            children.height() + outer_extra,
        );
    }

    /// 노드 하나의 `boundingBox()`(라벨 포함, 1px 넓힘). Mermaid 그룹 사각형도 이것이다
    pub fn node_bounding_box(
        &self,
        node: usize,
        rects: &[Bounds],
        centers: &[(f64, f64)],
    ) -> Bounds {
        let label = (!self.is_compound(node))
            .then(|| self.label_bounds(node, centers[node]))
            .flatten();
        let body = rects[node];
        label
            .map_or(body, |label| body.union(label))
            .expand(ANTIALIAS_EXPANSION)
    }

    /// cytoscape 간선 끝점(`source-endpoint`의 백분율 위치는 노드 중심에서 잰 변위다)
    pub fn endpoint(&self, center: (f64, f64), side: Side) -> (f64, f64) {
        let size = self.config.icon_size;
        match side {
            Side::Left => (center.0, center.1 + size / 2.0),
            Side::Right => (center.0 + size, center.1 + size / 2.0),
            Side::Top => (center.0 + size / 2.0, center.1),
            Side::Bottom => (center.0 + size / 2.0, center.1 + size),
        }
    }

    fn edge_endpoints(&self, edge: &CyEdge, centers: &[(f64, f64)]) -> ((f64, f64), (f64, f64)) {
        (
            self.endpoint(centers[edge.source], edge.source_side),
            self.endpoint(centers[edge.target], edge.target_side),
        )
    }

    /// `edge.midpoint()`: 곧은 간선은 두 끝점의 가운데, 꺾인 간선은 꺾는 점
    pub fn edge_midpoint(
        &self,
        edge: &CyEdge,
        centers: &[(f64, f64)],
        segment: SegmentWeight,
    ) -> (f64, f64) {
        let (start, end) = self.edge_endpoints(edge, centers);
        let is_bend = edge.source_side.is_horizontal() != edge.target_side.is_horizontal();
        if !is_bend {
            return ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
        }
        // findSegmentsPoints(`edge-distances: endpoints`)
        let (dx, dy) = (end.0 - start.0, end.1 - start.1);
        let length = (dx * dx + dy * dy).sqrt();
        let normal = (-dy / length, dx / length);
        let (stay, move_on) = (1.0 - segment.weight, segment.weight);
        (
            start.0 * stay + end.0 * move_on + normal.0 * segment.distance,
            start.1 * stay + end.1 * move_on + normal.1 * segment.distance,
        )
    }

    /// layoutstop 처리: 두 중심이 한 축에 나란하지 않은 간선은 꺾는 점이 모서리에 오도록 무게·거리를 다시 잡는다
    pub fn segment_weights(
        &self,
        centers: &[(f64, f64)],
        previous: &[SegmentWeight],
    ) -> Vec<SegmentWeight> {
        self.edges
            .iter()
            .zip(previous)
            .map(|(edge, &segment)| {
                let (source, target) = (centers[edge.source], centers[edge.target]);
                if source.0 == target.0 || source.1 == target.1 {
                    return segment;
                }
                let (start, end) = self.edge_endpoints(edge, centers);
                let corner = if edge.source_side.is_horizontal() {
                    (end.0, start.1)
                } else {
                    (start.0, end.1)
                };
                corner_segment_weight(start, end, corner)
            })
            .collect()
    }

    fn edge_bounding_box(
        &self,
        edge: &CyEdge,
        centers: &[(f64, f64)],
        segment: SegmentWeight,
    ) -> Bounds {
        let (start, end) = self.edge_endpoints(edge, centers);
        let middle = self.edge_midpoint(edge, centers, segment);
        // 길이 0인 꺾인 간선의 꺾는 점은 NaN이고, cytoscape의 상자 넓히기는 NaN 비교를 건너뛴다
        let points = [start, middle, end]
            .into_iter()
            .filter(|point| !point.0.is_nan() && !point.1.is_nan());
        let body = points
            .fold(Bounds::EMPTY, |bounds, point| {
                bounds.union(Bounds::around(point, 0.0, 0.0))
            })
            .expand(EDGE_WIDTH / 2.0);
        let label = edge
            .label_width
            .map(|width| Bounds::around(middle, width, EDGE_LABEL_FONT_SIZE).expand(LABEL_MARGIN));
        label
            .map_or(body, |label| body.union(label))
            .expand(ANTIALIAS_EXPANSION)
    }

    /// `eles.boundingBox()`의 가운데: fcose가 배치 결과를 되돌려 놓을 곳
    pub fn elements_center(
        &self,
        centers: &[(f64, f64)],
        segments: &[SegmentWeight],
    ) -> (f64, f64) {
        let rects = self.outer_rects(centers);
        let nodes = (0..self.nodes.len()).fold(Bounds::EMPTY, |bounds, node| {
            bounds.union(self.node_bounding_box(node, &rects, centers))
        });
        let all = self
            .edges
            .iter()
            .zip(segments)
            .fold(nodes, |bounds, (edge, &segment)| {
                bounds.union(self.edge_bounding_box(edge, centers, segment))
            });
        all.center()
    }
}

/// layoutstop의 getSegmentWeights: 시작·끝점을 잇는 선에서 `corner`까지의 부호 있는 거리(D)와 선 위 비율(W)
fn corner_segment_weight(
    (source_x, source_y): (f64, f64),
    (target_x, target_y): (f64, f64),
    (point_x, point_y): (f64, f64),
) -> SegmentWeight {
    let slope = (source_y - target_y) / (source_x - target_x);
    let offset = (source_x - point_x) * (source_y - target_y) / (source_x - target_x);
    let mut distance = (point_y - source_y + offset) / (1.0 + slope * slope).sqrt();
    let along_squared =
        (point_y - source_y).powi(2) + (point_x - source_x).powi(2) - distance * distance;
    let length = ((target_x - source_x).powi(2) + (target_y - source_y).powi(2)).sqrt();
    let mut weight = along_squared.sqrt() / length;
    let cross =
        (target_x - source_x) * (point_y - source_y) - (target_y - source_y) * (point_x - source_x);
    let dot =
        (target_x - source_x) * (point_x - source_x) + (target_y - source_y) * (point_y - source_y);
    distance = distance.abs() * if cross >= 0.0 { 1.0 } else { -1.0 };
    weight *= if dot >= 0.0 { 1.0 } else { -1.0 };
    SegmentWeight { weight, distance }
}

fn node_label(architecture: &Architecture, element: CyElement) -> Option<&str> {
    match element {
        CyElement::Node(index) => {
            let node = &architecture.nodes[index];
            (node.kind == NodeKind::Service)
                .then_some(node.title.as_deref())
                .flatten()
                .filter(|title| !title.is_empty())
        }
        CyElement::Group(_) => None,
    }
}
