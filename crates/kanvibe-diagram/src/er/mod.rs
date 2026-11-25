//! erDiagram 다이어그램(Mermaid 11 erRenderer-unified: dagre 레이아웃, erBox 모양, 차수 표식).
//!
//! 엔티티 사이 간격은 er.nodeSpacing 140·er.rankSpacing 80이고, 간선 라벨은 `.edgeLabel .label` 규칙대로 14px다.
//! 비식별 관계는 `.edge-pattern-dashed`(8,8) 점선으로 그린다. classDef·style 선언은 상자 채움·테두리·글자색을
//! 덮고, 앞머리 `title`은 그림 위 가운데에 16px(뿌리 글자색)로 놓는다.

mod entity_box;
mod markers;
mod parser;
pub(crate) mod unified;

use self::entity_box::{EntityColors, EntityStyle, entity_node};
use self::markers::cardinality_arrow;
use self::parser::ErDiagram;
use self::unified::{DiagramTitle, UnifiedGraph, render_unified, rough_pen};
use crate::flowchart::parser::Stroke;
use crate::flowchart::{ElementStyle, FlowEdge, FlowNode, FlowRender};
use crate::label::{FONT_SIZE, LabelBox, measure_label, measure_label_with_font_size};
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
        title: crate::directive::read_config(definition)
            .title
            .map(|text| DiagramTitle {
                text,
                font_size: FONT_SIZE,
                color: context.palette.text,
            }),
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

/// 테마 색 위에 엔티티의 classDef·style 선언을 덮는다. 짝수 줄 상자도 사용자 채움색을 받는다
fn entity_colors(palette: &Palette, style: &ElementStyle) -> EntityColors {
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
        fill: style.fill.unwrap_or(palette.node_fill),
        border: rough_pen(
            style.stroke.unwrap_or(palette.node_stroke),
            style.stroke_width,
        ),
        row_odd,
        row_even: style.fill.unwrap_or(row_even),
        text: style.text_color.unwrap_or(palette.text),
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
    diagram
        .entities
        .iter()
        .map(|entity| {
            let declarations = diagram.entity_declarations(entity);
            let user_style = ElementStyle::from_declarations(declarations.iter());
            let colors = entity_colors(context.palette, &user_style);
            let style = EntityStyle {
                fonts: context.fonts,
                mode: context.label_mode,
                colors: &colors,
            };
            let mut node = entity_node(entity_id(diagram, &entity.name), entity, &style);
            if entity.attributes.is_empty() {
                node.style = user_style;
            }
            node
        })
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

#[cfg(test)]
mod tests {
    use crate::scene::{Color, SceneItem};
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;
    use crate::{LabelMode, render_scene};

    #[test]
    fn 클래스_스타일과_앞머리_제목을_기준_svg대로_그린다() {
        let fonts = bundled_fonts();
        let source = include_str!("../../../../parity/diagrams/baseline/er-styles.mmd");
        let scene = render_scene(source, Theme::Light, LabelMode::Html, &fonts).unwrap();
        // er-styles 기준 SVG viewBox: 0 -50 152.765625 521.5
        assert!((scene.width - 152.765_625).abs() < 0.01, "{}", scene.width);
        assert!((scene.height - 521.5).abs() < 0.01, "{}", scene.height);
        let fills: Vec<Color> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SceneItem::Path(path) => path.fill,
                _ => None,
            })
            .collect();
        assert!(fills.contains(&Color::rgb(0xFF, 0x99, 0x66)));
        assert!(fills.contains(&Color::rgb(0xBB, 0xDD, 0xFF)));
        let item_color = scene.items.iter().find_map(|item| match item {
            SceneItem::Text(text) if text.text == "ITEM" => Some(text.color),
            _ => None,
        });
        assert_eq!(item_color, Some(Color::rgb(0x99, 0x00, 0x00)));
    }
}
