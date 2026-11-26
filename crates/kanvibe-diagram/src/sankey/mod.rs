//! sankey 다이어그램(sankeyRenderer.ts). d3-sankey로 600×400 안에 노드 열을 배치하고, 노드 막대·이름표·링크를
//! 그 순서로 그린다. 링크는 반투명(0.5) 굵은 곡선이고 `mix-blend-mode: multiply`로 아래 그림과 곱해진다.
//! 글자는 SVG `<text>`라 라벨 방식과 상관없이 같다. viewBox는 그린 것 전체의 getBBox(여백 0)다.

mod layout;
mod parser;

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::directive::read_config;
use crate::scene::{
    Color, MultiplyStrokeItem, PathCommand, Point, Scene, SceneItem, TextItem, TextStrokeItem,
};
use crate::text::FontWeight;
use crate::{DiagramError, RenderContext};
use layout::{LayoutLink, LayoutOptions, NodeAlignment, SankeyLayout};

const LABEL_FONT_SIZE: f64 = 14.0;
/// 노드 막대와 이름표 사이
const LABEL_GAP: f64 = 6.0;
/// 값을 함께 보일 때 `nodePadding`에 더하는 간격
const VALUE_LINE_PADDING: f64 = 15.0;
const LINK_OPACITY: f32 = 0.5;
/// `outlined` 이름표 배경 윤곽 굵기
const OUTLINE_WIDTH: f64 = 4.0;
/// d3 `schemeTableau10`
const TABLEAU10: [Color; 10] = [
    Color::rgb(0x4e, 0x79, 0xa7),
    Color::rgb(0xf2, 0x8e, 0x2c),
    Color::rgb(0xe1, 0x57, 0x59),
    Color::rgb(0x76, 0xb7, 0xb2),
    Color::rgb(0x59, 0xa1, 0x4f),
    Color::rgb(0xed, 0xc9, 0x49),
    Color::rgb(0xaf, 0x7a, 0xa1),
    Color::rgb(0xff, 0x9d, 0xa7),
    Color::rgb(0x9c, 0x75, 0x5f),
    Color::rgb(0xba, 0xb0, 0xab),
];

/// `linkColor` 설정
#[derive(Clone, Copy, Debug, PartialEq)]
enum LinkColoring {
    Gradient,
    Source,
    Target,
    Fixed(Color),
}

/// Mermaid `sankey` 설정(기본값은 config.schema.yaml)
#[derive(Clone, Debug)]
struct SankeyConfig {
    width: f64,
    height: f64,
    link_coloring: LinkColoring,
    alignment: NodeAlignment,
    should_show_values: bool,
    prefix: String,
    suffix: String,
    node_width: f64,
    node_padding: f64,
    /// `labelStyle: outlined`
    is_outlined: bool,
    /// `nodeColors`: 노드 이름 → 막대 색
    node_colors: HashMap<String, Color>,
}

impl SankeyConfig {
    fn from_section(section: Option<&Map<String, Value>>) -> Self {
        let empty = Map::new();
        let section = section.unwrap_or(&empty);
        let number =
            |key: &str, default: f64| section.get(key).and_then(Value::as_f64).unwrap_or(default);
        let text = |key: &str| section.get(key).and_then(Value::as_str);
        Self {
            width: number("width", 600.0),
            height: number("height", 400.0),
            link_coloring: match text("linkColor").unwrap_or("gradient") {
                "gradient" => LinkColoring::Gradient,
                "source" => LinkColoring::Source,
                "target" => LinkColoring::Target,
                other => {
                    LinkColoring::Fixed(Color::parse_css(other).unwrap_or(Color::rgb(0, 0, 0)))
                }
            },
            alignment: match text("nodeAlignment").unwrap_or("justify") {
                "left" => NodeAlignment::Left,
                "right" => NodeAlignment::Right,
                "center" => NodeAlignment::Center,
                _ => NodeAlignment::Justify,
            },
            should_show_values: section
                .get("showValues")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            prefix: text("prefix").unwrap_or("").to_string(),
            suffix: text("suffix").unwrap_or("").to_string(),
            node_width: number("nodeWidth", 10.0),
            node_padding: number("nodePadding", 12.0),
            is_outlined: text("labelStyle") == Some("outlined"),
            node_colors: read_node_colors(section),
        }
    }
}

/// 색으로 읽지 못하는 값은 SVG `fill` 기본값(검정)이 된다
fn read_node_colors(section: &Map<String, Value>) -> HashMap<String, Color> {
    let Some(colors) = section.get("nodeColors").and_then(Value::as_object) else {
        return HashMap::new();
    };
    colors
        .iter()
        .map(|(name, value)| {
            let color = value.as_str().and_then(Color::parse_css);
            (name.clone(), color.unwrap_or(Color::rgb(0, 0, 0)))
        })
        .collect()
}

/// SVG getBBox와 같은 축 정렬 경계
#[derive(Clone, Copy, Debug)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Bounds {
    fn include(&mut self, left: f64, top: f64, right: f64, bottom: f64) {
        self.left = self.left.min(left);
        self.top = self.top.min(top);
        self.right = self.right.max(right);
        self.bottom = self.bottom.max(bottom);
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let config = SankeyConfig::from_section(read_config(definition).section("sankey"));
    let graph = parser::parse(&crate::directive::diagram_body(definition))?;
    let padding_extra = if config.should_show_values {
        VALUE_LINE_PADDING
    } else {
        0.0
    };
    let layout = layout::compute(
        &graph,
        LayoutOptions {
            width: config.width,
            height: config.height,
            node_width: config.node_width,
            node_padding: config.node_padding + padding_extra,
            alignment: config.alignment,
        },
    )?;
    let mut scene = Scene::default();
    let mut bounds = Bounds {
        left: f64::INFINITY,
        top: f64::INFINITY,
        right: f64::NEG_INFINITY,
        bottom: f64::NEG_INFINITY,
    };
    let colors = node_colors(&config, &graph.nodes);
    draw_nodes(&mut scene, &mut bounds, &layout, &colors);
    let spots = label_spots(context, &config, &graph.nodes, &layout);
    draw_labels(&mut scene, &mut bounds, context, &spots, config.is_outlined);
    draw_links(&mut scene, &mut bounds, &config, &layout, &colors);
    if bounds.left > bounds.right {
        return Ok(scene);
    }
    scene.translate(-bounds.left, -bounds.top);
    scene.width = bounds.right - bounds.left;
    scene.height = bounds.bottom - bounds.top;
    Ok(scene)
}

/// `nodeColors`에 없는 노드만 d3 `scaleOrdinal(schemeTableau10)`에 차례로 물어 색을 받는다
fn node_colors(config: &SankeyConfig, names: &[String]) -> Vec<Color> {
    let mut ordinal = 0;
    names
        .iter()
        .map(|name| {
            config.node_colors.get(name).copied().unwrap_or_else(|| {
                ordinal += 1;
                TABLEAU10[(ordinal - 1) % TABLEAU10.len()]
            })
        })
        .collect()
}

fn draw_nodes(scene: &mut Scene, bounds: &mut Bounds, layout: &SankeyLayout, colors: &[Color]) {
    for (index, node) in layout.nodes.iter().enumerate() {
        let corners = [
            Point::new(node.x0, node.y0),
            Point::new(node.x1, node.y0),
            Point::new(node.x1, node.y1),
            Point::new(node.x0, node.y1),
        ];
        scene.push_path(crate::scene::polygon(&corners), Some(colors[index]), None);
        bounds.include(node.x0, node.y0, node.x1, node.y1);
    }
}

/// 이름표 글자. 값을 보이면 `이름\n값`인데 SVG 글자는 줄바꿈을 공백 하나로 접는다
fn label_text(config: &SankeyConfig, name: &str, value: f64) -> String {
    if !config.should_show_values {
        return name.to_string();
    }
    let rounded = (value * 100.0).round() / 100.0;
    format!("{name} {}{rounded}{}", config.prefix, config.suffix)
}

/// 이름표 한 줄의 자리. `text-anchor: end`면 `x`가 글자 끝이다
struct LabelSpot {
    text: String,
    x: f64,
    baseline: f64,
}

/// `findCentralNodeLayer`: 값이 가장 큰(처음 나온) 노드의 열
fn central_layer(layout: &SankeyLayout) -> usize {
    let mut best = (0.0, 0);
    for node in &layout.nodes {
        if node.value > best.0 {
            best = (node.value, node.layer);
        }
    }
    best.1
}

/// 기본(`legacy`)은 왼쪽 절반 노드의 이름표를 막대 오른쪽에, 나머지를 왼쪽에 둔다. `outlined`는 가운데 열보다
/// 앞 열이면 왼쪽, 아니면 오른쪽에 둔다. 값을 보이면 dy 0, 아니면 dy 0.35em으로 막대 가운데에 놓는다
fn label_spots(
    context: &RenderContext,
    config: &SankeyConfig,
    names: &[String],
    layout: &SankeyLayout,
) -> Vec<LabelSpot> {
    let central = central_layer(layout);
    let shift = if config.should_show_values {
        0.0
    } else {
        0.35 * LABEL_FONT_SIZE
    };
    let mut spots = Vec::with_capacity(names.len());
    for (node, name) in layout.nodes.iter().zip(names) {
        let text = label_text(config, name, node.value);
        let is_before = if config.is_outlined {
            node.layer < central
        } else {
            node.x0 >= config.width / 2.0
        };
        let x = if is_before {
            let advance = context.fonts.measure_svg_runs(
                &[text.as_str()],
                LABEL_FONT_SIZE,
                FontWeight::Regular,
            );
            node.x0 - LABEL_GAP - advance.advance
        } else {
            node.x1 + LABEL_GAP
        };
        let baseline = (node.y0 + node.y1) / 2.0 + shift;
        spots.push(LabelSpot { text, x, baseline });
    }
    spots
}

/// sankey styles.js: 글자는 `textColor`(기본 이름표는 루트 `fill`), 윤곽은 `mainBkg || background || #fff`
fn label_colors(context: &RenderContext) -> (Color, Color) {
    let variables = &context.theme_variables;
    let mut text = context.palette.text;
    variables.read_color("textColor", &mut text);
    let mut outline = context.palette.node_fill;
    let outline_key = ["mainBkg", "background"]
        .into_iter()
        .find(|key| variables.text(key).is_some_and(|value| !value.is_empty()));
    match outline_key {
        Some(key) => variables.read_color(key, &mut outline),
        None if variables.text("textColor").is_some() => outline = Color::rgb(255, 255, 255),
        None => {}
    }
    (text, outline)
}

/// 이름표를 그린다. `outlined`면 배경색 4px 둥근 윤곽(`paint-order: stroke`)을 깐 글자를 먼저 모두 두고 그 위에
/// 앞 글자를 다시 둔다
fn draw_labels(
    scene: &mut Scene,
    bounds: &mut Bounds,
    context: &RenderContext,
    spots: &[LabelSpot],
    is_outlined: bool,
) {
    let fonts = context.fonts;
    let metrics = fonts.primary_metrics(LABEL_FONT_SIZE);
    let (text_color, outline_color) = label_colors(context);
    let label = |spot: &LabelSpot| TextItem {
        text: spot.text.clone(),
        x: spot.x,
        baseline: spot.baseline,
        font_size: LABEL_FONT_SIZE,
        weight: FontWeight::Regular,
        color: text_color,
        rotation: None,
    };
    for spot in spots {
        let extent =
            fonts.measure_svg_runs(&[spot.text.as_str()], LABEL_FONT_SIZE, FontWeight::Regular);
        bounds.include(
            spot.x + extent.left,
            spot.baseline - metrics.ascent,
            spot.x + extent.right,
            spot.baseline + metrics.descent,
        );
        if is_outlined {
            scene.items.push(SceneItem::TextStroke(TextStrokeItem {
                text: label(spot),
                color: outline_color,
                width: OUTLINE_WIDTH,
                is_round_join: true,
            }));
        }
        scene.push_text(label(spot));
    }
    if is_outlined {
        spots.iter().for_each(|spot| scene.push_text(label(spot)));
    }
}

fn draw_links(
    scene: &mut Scene,
    bounds: &mut Bounds,
    config: &SankeyConfig,
    layout: &SankeyLayout,
    colors: &[Color],
) {
    for link in &layout.links {
        let start_x = layout.nodes[link.source].x1;
        let end_x = layout.nodes[link.target].x0;
        let (start_color, end_color) = link_colors(config.link_coloring, link, colors);
        bounds.include(start_x, link.y0.min(link.y1), end_x, link.y0.max(link.y1));
        scene
            .items
            .push(SceneItem::MultiplyStroke(MultiplyStrokeItem {
                commands: link_path(start_x, end_x, link),
                width: link.width.max(1.0),
                start_x,
                end_x,
                start_color: with_link_opacity(start_color),
                end_color: with_link_opacity(end_color),
            }));
    }
}

fn link_colors(coloring: LinkColoring, link: &LayoutLink, colors: &[Color]) -> (Color, Color) {
    let (source, target) = (colors[link.source], colors[link.target]);
    match coloring {
        LinkColoring::Gradient => (source, target),
        LinkColoring::Source => (source, source),
        LinkColoring::Target => (target, target),
        LinkColoring::Fixed(color) => (color, color),
    }
}

fn with_link_opacity(color: Color) -> Color {
    Color {
        alpha: color.alpha * LINK_OPACITY,
        ..color
    }
}

/// `sankeyLinkHorizontal`(d3 `linkHorizontal`): 가운데 x에 두 조절점을 둔 3차 곡선
fn link_path(start_x: f64, end_x: f64, link: &LayoutLink) -> Vec<PathCommand> {
    let middle = (start_x + end_x) / 2.0;
    vec![
        PathCommand::MoveTo(Point::new(start_x, link.y0)),
        PathCommand::CubicTo(
            Point::new(middle, link.y0),
            Point::new(middle, link.y1),
            Point::new(end_x, link.y1),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_for(definition: &str, config: &SankeyConfig) -> SankeyLayout {
        let graph = parser::parse(definition).unwrap();
        layout::compute(
            &graph,
            LayoutOptions {
                width: config.width,
                height: config.height,
                node_width: config.node_width,
                node_padding: config.node_padding + VALUE_LINE_PADDING,
                alignment: config.alignment,
            },
        )
        .unwrap()
    }

    #[test]
    fn 기준_svg와_같은_노드_위치를_낸다() {
        let config = SankeyConfig::from_section(None);
        let definition = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../parity/diagrams/baseline/sankey-quoted.mmd"
        ))
        .unwrap();
        let layout = layout_for(&definition, &config);
        // sankey-quoted--light.svg의 node-1·node-2·node-7
        assert!((layout.nodes[0].y0 - 41.426_467_041_438_92).abs() < 1e-6);
        assert!((layout.nodes[1].y0 - 27.480_736_676_898_868).abs() < 1e-6);
        assert!((layout.nodes[6].y0 - 304.300_000_000_000_07).abs() < 1e-6);
        assert_eq!(layout.nodes[3].x0, 590.0);
    }

    #[test]
    fn 값은_소수_둘째_자리까지_이름_뒤에_붙인다() {
        let config = SankeyConfig::from_section(None);
        assert_eq!(label_text(&config, "Losses", 208.2461), "Losses 208.25");
        assert_eq!(label_text(&config, "Grid", 300.0), "Grid 300");
    }
}
