//! block 다이어그램 그리기: 크기 재기(calculateBlockSize), 배치된 블록(insertBlockPositioned), 간선과 표식,
//! 간선 라벨(insertEdges).
//!
//! HTML 라벨 글자는 `.cluster span, p` 규칙이 마지막에 이겨 titleColor로 칠해지고, SVG 글자 라벨은
//! `.label text`의 글자색이다. classDef의 `color`(`tspan` 규칙)와 `style`의 `color`(`text` 인라인 채움)는 SVG
//! 글자에만 닿는다. classDef의 `stroke`는 라벨 묶음에 상속되지만 기준 이미지의 글자에는 윤곽선이 없다.

use std::collections::HashMap;

use super::layout::PADDING;
use super::shapes::{ShapeInput, shape_geometry};
use super::{
    Block, BlockDiagram, BlockEdge, BlockSize, BlockType, EdgeMarker, measure_block_label,
    resolve_style,
};
use crate::flowchart::geometry::{apply_line_offsets, curve_basis, end_tangent, path_bounds};
use crate::flowchart::shapes::NodeGeometry;
use crate::label::LabelBox;
use crate::scene::{
    Color, PathCommand, PathItem, Point, Scene, SceneItem, StrokeStyle, TextItem, ellipse, polygon,
    rounded_rect,
};
use crate::text::FontWeight;
use crate::{LabelMode, RenderContext};

/// lineWithOffset markerOffsets: 점 화살표만 선 끝을 당긴다
const POINT_MARKER_OFFSET: f64 = 4.0;
const THICK_EDGE_WIDTH: f64 = 3.5;
/// `.edge-pattern-dotted { stroke-dasharray: 2 }`
const DOTTED_DASH: f64 = 2.0;

fn fade(color: Color, alpha: f32) -> Color {
    Color { alpha, ..color }
}

pub(crate) struct BlockPainter<'a> {
    context: &'a RenderContext<'a>,
}

impl<'a> BlockPainter<'a> {
    pub(crate) fn new(context: &'a RenderContext<'a>) -> Self {
        Self { context }
    }

    fn label_of(&self, block: &Block) -> LabelBox {
        measure_block_label(self.context.fonts, &block.label, self.context.label_mode)
    }

    fn geometry_of(
        &self,
        block: &Block,
        label: &LabelBox,
        layout: Option<BlockSize>,
    ) -> NodeGeometry {
        let padding = if block.kind == BlockType::Composite {
            0.0
        } else {
            PADDING
        };
        shape_geometry(&ShapeInput {
            kind: block.kind,
            label_width: label.width,
            label_height: label.height,
            padding,
            directions: &block.directions,
            width_in_columns: block.width_in_columns,
            layout_size: layout.map(|size| (size.width, size.height)),
        })
    }

    /// calculateBlockSizes: 모양과 라벨을 합친 getBBox 크기를 자식부터 차례로 남긴다(뿌리는 빼고)
    pub(crate) fn measure_blocks(&self, blocks: &mut [Block], index: usize) {
        for child in blocks[index].children.clone() {
            let label = self.label_of(&blocks[child]);
            let geometry = self.geometry_of(&blocks[child], &label, None);
            let (width, height) = self.natural_bounds(&geometry, &label);
            blocks[child].size = Some(BlockSize {
                width,
                height,
                x: 0.0,
                y: 0.0,
            });
            self.measure_blocks(blocks, child);
        }
    }

    /// 윤곽선 상자와 노드 중심에 놓인 라벨 상자의 합집합 크기
    fn natural_bounds(&self, geometry: &NodeGeometry, label: &LabelBox) -> (f64, f64) {
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut include = |min_x: f64, min_y: f64, max_x: f64, max_y: f64| {
            bounds = [
                bounds[0].min(min_x),
                bounds[1].min(min_y),
                bounds[2].max(max_x),
                bounds[3].max(max_y),
            ];
        };
        for outline in &geometry.outlines {
            if let Some((min_x, min_y, max_x, max_y)) = path_bounds(outline) {
                include(min_x, min_y, max_x, max_y);
            }
        }
        if !label.is_empty() {
            let top = label.box_top(self.context.fonts, -label.height / 2.0);
            include(
                -label.width / 2.0,
                top,
                label.width / 2.0,
                top + label.height,
            );
        }
        if bounds[0] > bounds[2] {
            return (0.0, 0.0);
        }
        (bounds[2] - bounds[0], bounds[3] - bounds[1])
    }

    /// insertBlocks 다음 insertEdges: 블록은 계층 순서(부모 먼저), 간선은 정의 순서로 그린다
    pub(crate) fn paint(&self, diagram: &BlockDiagram) -> Scene {
        let mut scene = Scene::default();
        let mut geometries: HashMap<usize, NodeGeometry> = HashMap::new();
        self.paint_children(&mut scene, diagram, 0, &mut geometries);
        for edge in &diagram.edges {
            self.paint_edge(&mut scene, diagram, edge, &geometries);
        }
        scene
    }

    fn paint_children(
        &self,
        scene: &mut Scene,
        diagram: &BlockDiagram,
        index: usize,
        geometries: &mut HashMap<usize, NodeGeometry>,
    ) {
        for &child in &diagram.blocks[index].children {
            let block = &diagram.blocks[child];
            if block.kind != BlockType::Space
                && let Some(size) = block.size
            {
                let geometry = self.paint_block(scene, diagram, block, size);
                geometries.insert(child, geometry);
            }
            self.paint_children(scene, diagram, child, geometries);
        }
    }

    fn paint_block(
        &self,
        scene: &mut Scene,
        diagram: &BlockDiagram,
        block: &Block,
        size: BlockSize,
    ) -> NodeGeometry {
        let palette = self.context.palette;
        let label = self.label_of(block);
        let geometry = self.geometry_of(block, &label, Some(size));
        let style = resolve_style(diagram, block);
        let is_composite = block.kind == BlockType::Composite;
        let (default_fill, default_stroke) = if is_composite {
            (
                fade(palette.cluster_fill, 0.5),
                fade(palette.cluster_stroke, 0.2),
            )
        } else {
            (palette.node_fill, palette.node_stroke)
        };
        let stroke = StrokeStyle {
            color: style.stroke.unwrap_or(default_stroke),
            width: style.stroke_width.unwrap_or(1.0),
            dash: style.dash.clone(),
        };
        let mut shape = Scene::default();
        for outline in &geometry.outlines {
            shape.push_path(
                outline.clone(),
                Some(style.fill.unwrap_or(default_fill)),
                Some(stroke.clone()),
            );
        }
        let color = match self.context.label_mode {
            LabelMode::Html => palette.cluster_text,
            LabelMode::SvgText => style.text_color.unwrap_or(palette.text),
        };
        let (left, top) = (-label.width / 2.0, -label.height / 2.0);
        for text in label.text_items(self.context.fonts, left, top, color) {
            shape.push_text(text);
        }
        shape.translate(size.x, size.y);
        scene.items.extend(shape.items);
        geometry
    }

    fn paint_edge(
        &self,
        scene: &mut Scene,
        diagram: &BlockDiagram,
        edge: &BlockEdge,
        geometries: &HashMap<usize, NodeGeometry>,
    ) {
        let Some(points) = edge_points(diagram, edge, geometries) else {
            return;
        };
        let middle = points[1];
        let offset =
            |marker: EdgeMarker| (marker == EdgeMarker::Point).then_some(POINT_MARKER_OFFSET);
        let shifted =
            apply_line_offsets(&points, offset(edge.marker_start), offset(edge.marker_end));
        let commands = curve_basis(&shifted);
        let color = self.context.palette.line;
        let stroke = StrokeStyle {
            color,
            width: if edge.is_thick { THICK_EDGE_WIDTH } else { 1.0 },
            dash: if edge.is_dotted {
                vec![DOTTED_DASH, DOTTED_DASH]
            } else {
                Vec::new()
            },
        };
        scene.push_path(commands.clone(), None, Some(stroke));
        if let Some((anchor, angle)) = end_tangent(&commands, true) {
            push_marker(scene, edge.marker_end, true, anchor, angle, color);
        }
        if let Some((anchor, angle)) = end_tangent(&commands, false) {
            push_marker(scene, edge.marker_start, false, anchor, angle, color);
        }
        if !edge.label.is_empty() {
            self.paint_edge_label(scene, &edge.label, middle);
        }
    }

    /// insertEdgeLabel: 라벨 상자 가운데를 두 블록 중심의 가운데 점에 둔다
    fn paint_edge_label(&self, scene: &mut Scene, text: &str, center: Point) {
        let fonts = self.context.fonts;
        let palette = self.context.palette;
        let label = measure_block_label(fonts, text, self.context.label_mode);
        let metrics = fonts.primary_metrics(label.font_size);
        if self.context.label_mode == LabelMode::SvgText {
            for item in svg_edge_label_items(self.context, &label, center) {
                scene.push_text(item);
            }
            return;
        }
        // span.edgeLabel 바탕은 줄 상자가 아니라 글꼴 내용 영역(ascent + descent)만 칠한다
        let content_height = metrics.ascent + metrics.descent;
        let left = center.x - label.width / 2.0;
        let top = center.y - label.height / 2.0;
        let half_leading =
            ((label.height / label.lines.len().max(1) as f64 - content_height) / 2.0).floor();
        scene.push_path(
            rounded_rect(left, top + half_leading, label.width, content_height, 0.0),
            Some(palette.edge_label_inner),
            None,
        );
        for item in label.text_items(fonts, left, top, palette.cluster_text) {
            scene.push_text(item);
        }
    }
}

/// insertEdges·insertEdge: 두 블록 중심과 그 가운데 점. 두 블록이 모두 그려졌으면(빈칸이 아니면) 양 끝을 블록
/// 경계와 가운데 점을 잇는 선의 교점으로 바꾼다. 크기가 없는 블록에 닿은 간선은 그리지 않는다
fn edge_points(
    diagram: &BlockDiagram,
    edge: &BlockEdge,
    geometries: &HashMap<usize, NodeGeometry>,
) -> Option<[Point; 3]> {
    let start = *diagram.index.get(&edge.start)?;
    let end = *diagram.index.get(&edge.end)?;
    let from = diagram.blocks[start].size?;
    let to = diagram.blocks[end].size?;
    let from_center = Point::new(from.x, from.y);
    let to_center = Point::new(to.x, to.y);
    let middle = Point::new(
        from.x + (to.x - from.x) / 2.0,
        from.y + (to.y - from.y) / 2.0,
    );
    let points = match (geometries.get(&start), geometries.get(&end)) {
        (Some(tail), Some(head)) => [
            tail.intersect(from_center, middle),
            middle,
            head.intersect(to_center, middle),
        ],
        _ => [from_center, middle, to_center],
    };
    Some(points)
}

/// SVG 글자 간선 라벨: computeLabelTransform이 글자 getBBox 가운데를 점에 맞춘다
fn svg_edge_label_items(context: &RenderContext, label: &LabelBox, center: Point) -> Vec<TextItem> {
    let fonts = context.fonts;
    let ascent = fonts.primary_metrics(label.font_size).ascent;
    let top = center.y - label.height / 2.0;
    label
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.text.is_empty())
        .map(|(index, line)| {
            let runs: Vec<String> = line
                .text
                .split(' ')
                .filter(|word| !word.is_empty())
                .enumerate()
                .map(|(position, word)| {
                    if position == 0 {
                        word.to_string()
                    } else {
                        format!(" {word}")
                    }
                })
                .collect();
            let runs: Vec<&str> = runs.iter().map(String::as_str).collect();
            let extent = fonts.measure_svg_runs(&runs, label.font_size, FontWeight::Regular);
            TextItem {
                text: line.text.clone(),
                x: center.x - (extent.left + extent.right) / 2.0,
                baseline: top + ascent + label.font_size * 1.1 * index as f64,
                font_size: label.font_size,
                weight: FontWeight::Regular,
                color: context.palette.text,
                rotation: None,
            }
        })
        .collect()
}

/// 표식 좌표(viewBox 단위)를 경로 끝점·방향에 맞춰 옮긴다
fn place(anchor: Point, angle: f64, reference: Point, scale: f64, point: Point) -> Point {
    let (sin, cos) = angle.sin_cos();
    let x = (point.x - reference.x) * scale;
    let y = (point.y - reference.y) * scale;
    Point::new(anchor.x + x * cos - y * sin, anchor.y + x * sin + y * cos)
}

fn solid(color: Color, width: f64) -> StrokeStyle {
    StrokeStyle {
        color,
        width,
        dash: Vec::new(),
    }
}

/// dagre-wrapper markers.js: point(viewBox 10, 크기 12, refX 6·4.5), circle(크기 11, refX 11·-1),
/// cross(viewBox 11, 크기 11, refX 12·-1, 선 굵기 2). 모두 userSpaceOnUse라 간선 굵기와 상관없다
fn push_marker(
    scene: &mut Scene,
    marker: EdgeMarker,
    is_end: bool,
    anchor: Point,
    angle: f64,
    color: Color,
) {
    let item = match marker {
        EdgeMarker::None => return,
        EdgeMarker::Point => point_marker(is_end, anchor, angle, color),
        EdgeMarker::Circle => {
            let reference = Point::new(if is_end { 11.0 } else { -1.0 }, 5.0);
            let center = place(anchor, angle, reference, 1.1, Point::new(5.0, 5.0));
            PathItem {
                commands: ellipse(center, 5.5, 5.5),
                fill: Some(color),
                stroke: Some(solid(color, 1.1)),
            }
        }
        EdgeMarker::Cross => {
            let reference = Point::new(if is_end { 12.0 } else { -1.0 }, 5.2);
            let at = |x: f64, y: f64| place(anchor, angle, reference, 1.0, Point::new(x, y));
            PathItem {
                commands: vec![
                    PathCommand::MoveTo(at(1.0, 1.0)),
                    PathCommand::LineTo(at(10.0, 10.0)),
                    PathCommand::MoveTo(at(10.0, 1.0)),
                    PathCommand::LineTo(at(1.0, 10.0)),
                ],
                fill: None,
                stroke: Some(solid(color, 2.0)),
            }
        }
    };
    scene.items.push(SceneItem::Path(item));
}

fn point_marker(is_end: bool, anchor: Point, angle: f64, color: Color) -> PathItem {
    let (shape, reference) = if is_end {
        ([(0.0, 0.0), (10.0, 5.0), (0.0, 10.0)], Point::new(6.0, 5.0))
    } else {
        (
            [(0.0, 5.0), (10.0, 10.0), (10.0, 0.0)],
            Point::new(4.5, 5.0),
        )
    };
    let points: Vec<Point> = shape
        .iter()
        .map(|(x, y)| place(anchor, angle, reference, 1.2, Point::new(*x, *y)))
        .collect();
    PathItem {
        commands: polygon(&points),
        fill: Some(color),
        stroke: Some(solid(color, 1.2)),
    }
}
