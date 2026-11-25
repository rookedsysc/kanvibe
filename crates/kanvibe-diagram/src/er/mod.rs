//! erDiagram 다이어그램(Mermaid 11 erRenderer-unified: dagre 레이아웃, erBox 모양, 차수 표식).
//!
//! 엔티티 사이 간격은 er.nodeSpacing 140·er.rankSpacing 80이고, 간선 라벨은 `.edgeLabel .label` 규칙대로 14px다.
//! 비식별 관계는 `.edge-pattern-dashed`(8,8) 점선으로 그린다.

mod entity_box;
mod markers;
mod parser;
pub(crate) mod unified;

use self::entity_box::{EntityColors, EntityStyle, entity_node};
use self::markers::cardinality_arrow;
use self::parser::ErDiagram;
use self::unified::{UnifiedGraph, render_unified};
use crate::flowchart::parser::Stroke;
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender};
use crate::label::{LabelBox, measure_label, measure_label_with_font_size};
use crate::scene::{Color, Scene};
use crate::theme::{Palette, Theme, hsl};
use crate::{DiagramError, LabelMode, RenderContext};

const NODE_SPACING: f64 = 140.0;
const RANK_SPACING: f64 = 80.0;
const EDGE_LABEL_FONT_SIZE: f64 = 14.0;
const NON_IDENTIFYING_DASH: [f64; 2] = [8.0, 8.0];

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let diagram = parser::parse(definition)?;
    if diagram.relationships.len() > crate::MAX_EDGES {
        return Err(DiagramError::Syntax(format!(
            "간선이 {}개를 넘습니다",
            crate::MAX_EDGES
        )));
    }
    let palette = er_palette(context.palette, context.label_mode);
    let render = FlowRender {
        fonts: context.fonts,
        palette: &palette,
    };
    let graph = UnifiedGraph {
        direction: diagram.direction,
        nodes: entity_nodes(&diagram, context),
        edges: relationship_edges(&diagram, context),
        node_spacing: NODE_SPACING,
        rank_spacing: RANK_SPACING,
    };
    Ok(render_unified(&render, graph))
}

/// erDiagram 스타일에서 flowchart 기본과 다른 색: 간선 라벨 바탕(labelBkg, SVG 글자 라벨 사각형)
fn er_palette(base: &Palette, mode: LabelMode) -> Palette {
    let mut palette = base.clone();
    palette.edge_label_outer = match base.theme {
        Theme::Light => Color::rgba(249, 255, 236, 0.5),
        Theme::Dark => Color::rgba(32, 31, 31, 0.5),
    };
    palette.edge_label_inner = unified::edge_label_background(base.edge_label_inner, mode);
    palette
}

fn entity_colors(palette: &Palette) -> EntityColors {
    let (row_odd, row_even) = match palette.theme {
        Theme::Light => (
            hsl(240.0, 100.0, 100.0),
            hsl(240.0, 100.0, 97.274_509_803_9),
        ),
        Theme::Dark => (
            hsl(180.0, 1.587_301_587_3, 17.352_941_176_5),
            hsl(180.0, 1.587_301_587_3, 2.352_941_176_5),
        ),
    };
    EntityColors {
        fill: palette.node_fill,
        border: palette.node_stroke,
        row_odd,
        row_even,
        text: palette.text,
    }
}

/// ErDB.addEntity의 id: `entity-<이름>-<순번>`
fn entity_id(diagram: &ErDiagram, name: &str) -> String {
    let index = diagram
        .entities
        .iter()
        .position(|entity| entity.name == name)
        .unwrap_or_default();
    format!("entity-{name}-{index}")
}

fn entity_nodes(diagram: &ErDiagram, context: &RenderContext) -> Vec<FlowNode> {
    let colors = entity_colors(context.palette);
    let style = EntityStyle {
        fonts: context.fonts,
        mode: context.label_mode,
        colors: &colors,
    };
    diagram
        .entities
        .iter()
        .map(|entity| entity_node(entity_id(diagram, &entity.name), entity, &style))
        .collect()
}

/// 간선 라벨은 14px로 그린다. SVG 글자 방식은 원본이 CSS가 붙기 전 16px로 잰 크기로 자리를 잡으므로 크기만 16px로 잰다
fn role_label(context: &RenderContext, role: &str) -> LabelBox {
    match context.label_mode {
        LabelMode::Html => {
            measure_label_with_font_size(context.fonts, role, LabelMode::Html, EDGE_LABEL_FONT_SIZE)
        }
        LabelMode::SvgText => LabelBox {
            font_size: EDGE_LABEL_FONT_SIZE,
            ..measure_label(context.fonts, role, LabelMode::SvgText)
        },
    }
}

/// ErDB.getData: 시작 표식은 왼쪽 차수(cardB), 끝 표식은 오른쪽 차수(cardA)다
fn relationship_edges(diagram: &ErDiagram, context: &RenderContext) -> Vec<FlowEdge> {
    diagram
        .relationships
        .iter()
        .enumerate()
        .map(|(counter, relationship)| {
            let start = entity_id(diagram, &relationship.start);
            let end = entity_id(diagram, &relationship.end);
            let dash = if relationship.is_identifying {
                Vec::new()
            } else {
                NON_IDENTIFYING_DASH.to_vec()
            };
            FlowEdge {
                id: format!("id_{start}_{end}_{counter}"),
                label: role_label(context, &relationship.role),
                arrow_start: cardinality_arrow(relationship.start_cardinality, true),
                arrow_end: cardinality_arrow(relationship.end_cardinality, false),
                stroke: Stroke::Normal,
                minlen: 1,
                style: ElementStyle {
                    dash,
                    ..ElementStyle::default()
                },
                start,
                end,
            }
        })
        .collect()
}
