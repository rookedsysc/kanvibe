//! treemap 다이어그램(treemap/renderer.ts). `nodeWidth·nodeHeight × 10` 칸(기본 1000×400)을 d3 squarify로 나눠
//! 구역(머리에 굵은 이름과 기울인 합계)과 잎(가운데 큰 이름과 값)을 그린다. 잎 글자는 칸에 들어갈 때까지 38px
//! (클래스 `font-size`가 있으면 그 크기)에서 줄이고, 들어가지 않으면 숨긴다. 글자는 SVG `<text>`라 라벨 방식과
//! 상관없이 같다. viewBox는 그린 것 전체의 getBBox에 `diagramPadding`(기본 8) 여백이다.

mod colors;
mod layout;
mod parser;
mod style;
mod value_format;

use std::collections::HashMap;

use crate::directive::{diagram_body, read_config};
use crate::scene::{ClipItem, Color, Point, Scene, SceneItem, StrokeStyle, TextItem, polygon};
use crate::text::{FontBook, FontWeight};
use crate::{DiagramError, RenderContext};
use colors::{OrdinalScale, TreemapColors};
use layout::{SECTION_HEADER_HEIGHT, Tree, TreeNode};
use style::{ClassStyle, TreemapConfig, read_class_styles};

const TITLE_HEIGHT: f64 = 30.0;
const TITLE_FONT_SIZE: f64 = 14.0;
const SECTION_LABEL_SIZE: f64 = 12.0;
const SECTION_VALUE_SIZE: f64 = 10.0;
const SECTION_FILL_OPACITY: f32 = 0.6;
const SECTION_STROKE_OPACITY: f32 = 0.4;
const SECTION_STROKE_WIDTH: f64 = 2.0;
const LEAF_FILL_OPACITY: f32 = 0.3;
const LEAF_STROKE_WIDTH: f64 = 3.0;
const LEAF_LABEL_MAX_SIZE: f64 = 38.0;
const LEAF_LABEL_MIN_SIZE: f64 = 8.0;
const LEAF_PADDING: f64 = 4.0;
/// Noto Sans x-height(1em 기준). `dominant-baseline: middle`은 x-height의 절반만큼 내린다
const X_HEIGHT_RATIO: f64 = 0.536;
/// Noto Sans ascender(1em 기준)와 Blink가 `dominant-baseline: hanging`에 쓰는 그 80%
const ASCENT_EM: f64 = 1.069;
const HANGING_ASCENT_RATIO: f64 = 0.8;

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

/// 그리는 동안 함께 쓰는 값. 색 척도는 Mermaid가 처음 물어본 순서대로 이름에 번호를 준다
struct TreemapCanvas<'a> {
    fonts: &'a FontBook,
    config: TreemapConfig,
    colors: TreemapColors,
    styles: HashMap<String, ClassStyle>,
    fill_scale: OrdinalScale,
    label_scale: OrdinalScale,
    scene: Scene,
    bounds: Bounds,
    /// 제목이 있으면 30만큼 내린 컨테이너의 y
    top: f64,
}

/// 글자 한 줄과 세로 기준
struct TextLine<'a> {
    text: &'a str,
    x: f64,
    baseline: f64,
    font_size: f64,
    weight: FontWeight,
    color: Color,
}

impl TreemapCanvas<'_> {
    fn style_for(&self, node: &TreeNode) -> ClassStyle {
        node.class_name
            .as_ref()
            .and_then(|name| self.styles.get(name))
            .cloned()
            .unwrap_or_default()
    }

    /// 글자를 장면에 넣고 getBBox 범위를 넓힌 뒤 돌려준다(잎 글자는 잘라 낼 묶음에 다시 담는다)
    fn text_item(&mut self, line: TextLine) -> TextItem {
        let extent = self
            .fonts
            .measure_svg_runs(&[line.text], line.font_size, line.weight);
        let metrics = self.fonts.primary_metrics(line.font_size);
        self.bounds.include(
            line.x + extent.left,
            line.baseline - metrics.ascent,
            line.x + extent.right,
            line.baseline + metrics.descent,
        );
        TextItem {
            text: line.text.to_string(),
            x: line.x,
            baseline: line.baseline,
            font_size: line.font_size,
            weight: line.weight,
            color: line.color,
            rotation: None,
        }
    }

    fn advance(&self, text: &str, font_size: f64, weight: FontWeight) -> f64 {
        self.fonts.measure_line(text, font_size, weight)
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let document = parser::parse(&diagram_body(definition))?;
    let diagram_config = read_config(definition);
    let config = TreemapConfig::from_config(&diagram_config);
    let title = document.title.clone().or(diagram_config.title);
    let mut tree = layout::build_tree(&document.rows);
    layout::place(&mut tree, config.width, config.height, config.inner_padding);
    let has_title = title.as_deref().is_some_and(|text| !text.is_empty());
    let mut canvas = TreemapCanvas {
        fonts: context.fonts,
        config,
        colors: TreemapColors::for_theme(context.palette.theme)
            .with_theme_variables(&context.theme_variables),
        styles: read_class_styles(&document.classes),
        fill_scale: OrdinalScale::default(),
        label_scale: OrdinalScale::default(),
        scene: Scene::default(),
        bounds: Bounds {
            left: f64::INFINITY,
            top: f64::INFINITY,
            right: f64::NEG_INFINITY,
            bottom: f64::NEG_INFINITY,
        },
        top: if has_title { TITLE_HEIGHT } else { 0.0 },
    };
    if let Some(title) = title.filter(|_| has_title) {
        draw_title(&mut canvas, &title);
    }
    let sections: Vec<usize> = tree
        .breadth_first()
        .into_iter()
        .filter(|&index| !tree.nodes[index].children.is_empty())
        .collect();
    for &index in &sections {
        draw_section(&mut canvas, &tree.nodes[index]);
    }
    for index in tree.leaves() {
        draw_leaf(&mut canvas, &tree, index);
    }
    Ok(finish_scene(canvas))
}

fn finish_scene(canvas: TreemapCanvas) -> Scene {
    let mut scene = canvas.scene;
    let bounds = canvas.bounds;
    if bounds.left > bounds.right {
        return scene;
    }
    let padding = canvas.config.diagram_padding;
    let (left, top) = (bounds.left - padding, bounds.top - padding);
    scene.translate(-left, -top);
    scene.width = bounds.right - bounds.left + 2.0 * padding;
    scene.height = bounds.bottom - bounds.top + 2.0 * padding;
    scene
}

fn draw_title(canvas: &mut TreemapCanvas, title: &str) {
    let width = canvas.advance(title, TITLE_FONT_SIZE, FontWeight::Regular);
    let item = canvas.text_item(TextLine {
        text: title,
        x: canvas.config.width / 2.0 - width / 2.0,
        baseline: TITLE_HEIGHT / 2.0 + X_HEIGHT_RATIO * TITLE_FONT_SIZE / 2.0,
        font_size: TITLE_FONT_SIZE,
        weight: FontWeight::Regular,
        color: canvas.colors.title,
    });
    canvas.scene.push_text(item);
}

fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<crate::scene::PathCommand> {
    polygon(&[
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ])
}

fn with_opacity(color: Color, opacity: f32) -> Color {
    Color {
        alpha: color.alpha * opacity,
        ..color
    }
}

/// 구역 상자와 머리 글자. 뿌리 구역은 Mermaid가 `display: none`으로 숨긴다
fn draw_section(canvas: &mut TreemapCanvas, node: &TreeNode) {
    let fill_index = canvas.fill_scale.index(&node.name);
    if node.depth == 0 {
        return;
    }
    let style = canvas.style_for(node);
    let (x, y) = (node.x0, node.y0 + canvas.top);
    let (width, height) = (node.x1 - node.x0, node.y1 - node.y0);
    let fill = style.fill.unwrap_or(canvas.colors.fill(fill_index));
    let stroke = StrokeStyle {
        color: with_opacity(
            style.stroke.unwrap_or(canvas.colors.peer(fill_index)),
            style.stroke_alpha(SECTION_STROKE_OPACITY),
        ),
        width: style.stroke_width.unwrap_or(SECTION_STROKE_WIDTH),
        dash: style.dash.clone(),
    };
    canvas.scene.push_path(
        rectangle(x, y, width, height),
        Some(with_opacity(fill, style.fill_alpha(SECTION_FILL_OPACITY))),
        Some(stroke),
    );
    canvas.bounds.include(x, y, x + width, y + height);
    let label_index = canvas.label_scale.index(&node.name);
    let color = style
        .label_color
        .unwrap_or(canvas.colors.label(label_index));
    draw_section_header(canvas, node, &style, Point::new(x, y), color);
}

/// 머리 왼쪽의 굵은 이름(자리가 모자라면 `...`로 줄인다)과 오른쪽의 기울인 합계. 클래스 라벨 스타일이
/// 크기·굵기·기울임을 바꾼다
fn draw_section_header(
    canvas: &mut TreemapCanvas,
    node: &TreeNode,
    style: &ClassStyle,
    origin: Point,
    color: Color,
) {
    let width = node.x1 - node.x0;
    let middle = origin.y + SECTION_HEADER_HEIGHT / 2.0;
    let has_value = canvas.config.should_show_values && node.value != 0.0;
    let label_font = (
        style.font_size.unwrap_or(SECTION_LABEL_SIZE),
        style.font_weight(true, false),
    );
    let label = fit_section_label(canvas, &node.name, width, has_value, label_font);
    let item = canvas.text_item(TextLine {
        text: &label,
        x: origin.x + 6.0,
        baseline: middle + X_HEIGHT_RATIO * label_font.0 / 2.0,
        font_size: label_font.0,
        weight: label_font.1,
        color,
    });
    canvas.scene.push_text(item);
    if !has_value {
        return;
    }
    let value = canvas.config.value_format.format(node.value);
    let (size, weight) = (
        style.font_size.unwrap_or(SECTION_VALUE_SIZE),
        style.font_weight(false, true),
    );
    let value_width = canvas.advance(&value, size, weight);
    let item = canvas.text_item(TextLine {
        text: &value,
        x: origin.x + width - 10.0 - value_width,
        baseline: middle + X_HEIGHT_RATIO * size / 2.0,
        font_size: size,
        weight,
        color,
    });
    canvas.scene.push_text(item);
}

/// 구역 이름이 머리에 들어갈 때까지 끝 글자를 하나씩 지우고 `...`를 붙인다
fn fit_section_label(
    canvas: &TreemapCanvas,
    name: &str,
    width: f64,
    has_value: bool,
    (font_size, weight): (f64, FontWeight),
) -> String {
    // 값이 있으면 값 자리(오른쪽 여백 10 + 값 폭 어림 30 + 간격 10)를 비운다
    let space = if has_value {
        width - 10.0 - 30.0 - 10.0 - 6.0
    } else {
        width - 6.0 - 6.0
    };
    let available = space.max(15.0);
    let measure = |text: &str| canvas.advance(text, font_size, weight);
    if measure(name) <= available {
        return name.to_string();
    }
    let characters: Vec<char> = name.chars().collect();
    for length in (1..characters.len()).rev() {
        let candidate: String = characters[..length].iter().collect::<String>() + "...";
        if measure(&candidate) <= available {
            return candidate;
        }
    }
    if measure("...") > available {
        return String::new();
    }
    "...".to_string()
}

/// 잎 상자와, 칸에 맞춘 가운데 이름·값. 글자는 칸에서 4px 줄인 사각형으로 잘라 낸다
fn draw_leaf(canvas: &mut TreemapCanvas, tree: &Tree, index: usize) {
    let node = &tree.nodes[index];
    let parent_name = node.parent.map_or(node.name.as_str(), |parent| {
        tree.nodes[parent].name.as_str()
    });
    let base = canvas.colors.fill(canvas.fill_scale.index(parent_name));
    let style = canvas.style_for(node);
    let (x, y) = (node.x0, node.y0 + canvas.top);
    let (width, height) = (node.x1 - node.x0, node.y1 - node.y0);
    let stroke = StrokeStyle {
        color: with_opacity(style.stroke.unwrap_or(base), style.stroke_alpha(1.0)),
        width: style.stroke_width.unwrap_or(LEAF_STROKE_WIDTH),
        dash: style.dash.clone(),
    };
    let fill = with_opacity(
        style.fill.unwrap_or(base),
        style.fill_alpha(LEAF_FILL_OPACITY),
    );
    canvas
        .scene
        .push_path(rectangle(x, y, width, height), Some(fill), Some(stroke));
    canvas.bounds.include(x, y, x + width, y + height);
    let label_index = canvas.label_scale.index(&node.name);
    let color = style
        .label_color
        .unwrap_or(canvas.colors.label(label_index));
    let items = leaf_texts(canvas, node, &style, Point::new(x, y), color);
    if !items.is_empty() {
        canvas.scene.items.push(SceneItem::Clip(ClipItem {
            x,
            y,
            width: (width - LEAF_PADDING).max(0.0),
            height: (height - LEAF_PADDING).max(0.0),
            items: items.into_iter().map(SceneItem::Text).collect(),
        }));
    }
}

/// 잎 이름 글자 크기. 폭에 맞을 때까지, 값 줄과 함께 높이에 맞을 때까지 1px씩 줄이고 그래도 넘치면 숨긴다.
/// 시작 크기는 `parseInt(font-size)`다
fn fit_leaf_label_size(
    canvas: &TreemapCanvas,
    name: &str,
    (width, height): (f64, f64),
    (start_size, weight): (f64, FontWeight),
) -> Option<f64> {
    let (available_width, available_height) =
        (width - 2.0 * LEAF_PADDING, height - 2.0 * LEAF_PADDING);
    if available_width < 10.0 || available_height < 10.0 {
        return None;
    }
    let measure = |size: f64| canvas.advance(name, size, weight);
    let mut size = start_size.trunc();
    while measure(size) > available_width && size > LEAF_LABEL_MIN_SIZE {
        size -= 1.0;
    }
    while size + 2.0 + value_font_size(size) > available_height && size > LEAF_LABEL_MIN_SIZE {
        size -= 1.0;
    }
    let is_hidden =
        measure(size) > available_width || size < LEAF_LABEL_MIN_SIZE || available_height < size;
    (!is_hidden).then_some(size)
}

/// 값 글자 크기: 이름 크기의 60%(6..=28px)
fn value_font_size(label_size: f64) -> f64 {
    (label_size * 0.6 + 0.5).floor().clamp(6.0, 28.0)
}

fn leaf_texts(
    canvas: &mut TreemapCanvas,
    node: &TreeNode,
    style: &ClassStyle,
    origin: Point,
    color: Color,
) -> Vec<TextItem> {
    let (width, height) = (node.x1 - node.x0, node.y1 - node.y0);
    let weight = style.font_weight(false, false);
    let start_size = style.font_size.unwrap_or(LEAF_LABEL_MAX_SIZE);
    let Some(label_size) =
        fit_leaf_label_size(canvas, &node.name, (width, height), (start_size, weight))
    else {
        return Vec::new();
    };
    let center = Point::new(origin.x + width / 2.0, origin.y + height / 2.0);
    let label_width = canvas.advance(&node.name, label_size, weight);
    let mut items = vec![canvas.text_item(TextLine {
        text: &node.name,
        x: center.x - label_width / 2.0,
        baseline: center.y + X_HEIGHT_RATIO * label_size / 2.0,
        font_size: label_size,
        weight,
        color,
    })];
    if !canvas.config.should_show_values || node.value == 0.0 {
        return items;
    }
    let value = canvas.config.value_format.format(node.value);
    let value_size = value_font_size(label_size);
    let value_top = height / 2.0 + label_size / 2.0 + 2.0;
    let value_width = canvas.advance(&value, value_size, weight);
    let is_visible = value_width <= width - 2.0 * LEAF_PADDING
        && value_top + value_size <= height - LEAF_PADDING;
    if is_visible {
        items.push(canvas.text_item(TextLine {
            text: &value,
            x: center.x - value_width / 2.0,
            baseline: origin.y + value_top + ASCENT_EM * value_size * HANGING_ASCENT_RATIO,
            font_size: value_size,
            weight,
            color,
        }));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 값_글자는_이름_크기의_60퍼센트다() {
        assert_eq!(value_font_size(38.0), 23.0);
        assert_eq!(value_font_size(24.0), 14.0);
        assert_eq!(value_font_size(8.0), 6.0);
    }
}
