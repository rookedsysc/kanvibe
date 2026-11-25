//! block-beta 다이어그램(blockDB.ts·layout.ts·renderHelpers.ts, dagre-wrapper 모양).
//!
//! 블록을 칸 격자에 늘어놓고(`columns`, `:칸수`, `space`), `block … end`로 묶음을 만든다. 크기는 라벨 상자에서
//! 모양별로 재고, [`layout`]이 칸 크기를 맞춘 뒤 위치를 정한다. 간선은 두 블록 중심과 그 가운데 점을 잇는
//! curveBasis이고 양 끝은 블록 경계에서 자른다. viewBox는 블록 상자 전체에 5px 여백을 두른 것이다.

mod draw;
mod layout;
mod parser;
mod shapes;

use indexmap::IndexMap;

use self::draw::BlockPainter;
use self::parser::{NodeSpec, Statement};
use crate::directive::diagram_body;
use crate::flowchart::ElementStyle;
use crate::label::{LINE_HEIGHT, LabelBox, LabelLine, label_lines, measure_svg_label};
use crate::scene::Scene;
use crate::text::{FontBook, FontWeight};
use crate::theme::Palette;
use crate::theme_variables::{ThemeVariables, parse_theme_color};
use crate::{DiagramError, LabelMode, RenderContext};

const FONT_SIZE: f64 = 16.0;
/// draw()가 viewBox에 두르는 여백
const VIEW_MARGIN: f64 = 5.0;
const ROOT: usize = 0;

/// typeStr2Type 결과
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BlockType {
    Square,
    Round,
    Circle,
    LeftInvArrow,
    Diamond,
    Hexagon,
    Stadium,
    Subroutine,
    Cylinder,
    DoubleCircle,
    LeanRight,
    LeanLeft,
    Trapezoid,
    InvTrapezoid,
    BlockArrow,
    Composite,
    Space,
    /// 이름표 기호가 없는 블록(사각형으로 그린다)
    Na,
}

impl BlockType {
    fn from_type_str(type_str: Option<&str>) -> Self {
        match type_str.unwrap_or_default() {
            "[]" => Self::Square,
            "()" => Self::Round,
            "(())" => Self::Circle,
            ">]" => Self::LeftInvArrow,
            "{}" => Self::Diamond,
            "{{}}" => Self::Hexagon,
            "([])" => Self::Stadium,
            "[[]]" => Self::Subroutine,
            "[()]" => Self::Cylinder,
            "((()))" => Self::DoubleCircle,
            "[//]" => Self::LeanRight,
            "[\\\\]" => Self::LeanLeft,
            "[/\\]" => Self::Trapezoid,
            "[\\/]" => Self::InvTrapezoid,
            "<[]>" => Self::BlockArrow,
            _ => Self::Na,
        }
    }
}

/// 블록 중심 좌표와 크기
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct BlockSize {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub id: String,
    pub label: String,
    pub kind: BlockType,
    pub children: Vec<usize>,
    pub columns: Option<i64>,
    pub width_in_columns: u32,
    pub directions: Vec<String>,
    pub classes: Vec<String>,
    /// `style` 문장의 선언(쉼표로 나눈 것)
    pub styles: Vec<String>,
    pub size: Option<BlockSize>,
}

impl Block {
    fn new(id: &str, kind: BlockType) -> Self {
        Self {
            id: id.to_string(),
            label: String::new(),
            kind,
            children: Vec::new(),
            columns: None,
            width_in_columns: 1,
            directions: Vec::new(),
            classes: Vec::new(),
            styles: Vec::new(),
            size: None,
        }
    }
}

/// 간선 끝 표식
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EdgeMarker {
    None,
    Point,
    Circle,
    Cross,
}

#[derive(Clone, Debug)]
pub(crate) struct BlockEdge {
    pub start: String,
    pub end: String,
    pub label: String,
    pub marker_start: EdgeMarker,
    pub marker_end: EdgeMarker,
    pub is_thick: bool,
    pub is_dotted: bool,
}

impl BlockEdge {
    /// edgeStrToEdgeData·edgeStrToEdgeStartData·edgeStrToThickness·edgeStrToPattern
    fn new(start: &str, end: &str, link: &str, label: String) -> Self {
        let link = link.trim();
        let marker = |character: Option<char>, point: char| match character {
            Some('x') => EdgeMarker::Cross,
            Some('o') => EdgeMarker::Circle,
            Some(found) if found == point => EdgeMarker::Point,
            _ => EdgeMarker::None,
        };
        Self {
            start: start.to_string(),
            end: end.to_string(),
            label,
            marker_start: marker(link.chars().next(), '<'),
            marker_end: marker(link.chars().last(), '>'),
            is_thick: link.contains("=="),
            is_dotted: link.contains(".-"),
        }
    }
}

/// classDef 하나의 선언 목록. `color`는 SVG 글자에만, 나머지는 모양에 쓴다
#[derive(Clone, Debug, Default)]
pub(crate) struct StyleClass {
    pub styles: Vec<String>,
}

/// blockDB: id로 찾는 블록 목록(0번이 뿌리), 간선, classDef
pub(crate) struct BlockDiagram {
    pub blocks: Vec<Block>,
    pub index: IndexMap<String, usize>,
    pub edges: Vec<BlockEdge>,
    pub classes: IndexMap<String, StyleClass>,
    generated: usize,
}

impl BlockDiagram {
    fn new() -> Self {
        let mut index = IndexMap::new();
        index.insert("root".to_string(), ROOT);
        Self {
            blocks: vec![Block::new("root", BlockType::Composite)],
            index,
            edges: Vec::new(),
            classes: IndexMap::new(),
            generated: 0,
        }
    }

    fn generate_id(&mut self) -> String {
        self.generated += 1;
        format!("id-block-{}", self.generated)
    }

    fn insert(&mut self, block: Block) -> usize {
        let position = self.blocks.len();
        self.index.insert(block.id.clone(), position);
        self.blocks.push(block);
        position
    }
}

/// 문장 목록을 펼친 한 항목(간선 문장은 앞 노드·간선·뒤 노드 셋이 된다)
enum Entry {
    Block(Block, Option<Vec<Statement>>),
    /// 이어 쓴 간선이 남기는 id `undefined` 빈 블록
    Placeholder,
    Space(u32),
    Edge(BlockEdge),
    Columns(i64),
    ClassDef(String, String),
    ApplyClass(String, String),
    ApplyStyles(String, String),
}

fn node_block(node: &NodeSpec, width_in_columns: u32) -> Block {
    Block {
        label: node.label.clone().unwrap_or_default(),
        width_in_columns,
        directions: node.directions.clone(),
        ..Block::new(&node.id, BlockType::from_type_str(node.type_str.as_deref()))
    }
}

fn flatten(diagram: &mut BlockDiagram, statements: Vec<Statement>) -> Vec<Entry> {
    let mut entries = Vec::new();
    for statement in statements {
        match statement {
            Statement::Node {
                node,
                width_in_columns,
            } => entries.push(Entry::Block(node_block(&node, width_in_columns), None)),
            Statement::Edge {
                from,
                to,
                link,
                label,
            } => {
                let edge = BlockEdge::new(&from.id, &to.id, &link, label);
                entries.push(Entry::Block(node_block(&from, 1), None));
                entries.push(Entry::Edge(edge));
                entries.push(Entry::Block(node_block(&to, 1), None));
            }
            Statement::Composite { head, children } => {
                let mut block = match head {
                    Some((node, span)) => node_block(&node, span),
                    None => Block::new(&diagram.generate_id(), BlockType::Composite),
                };
                block.kind = BlockType::Composite;
                entries.push(Entry::Block(block, Some(children)));
            }
            Statement::BrokenChain { last } => {
                entries.push(Entry::Placeholder);
                entries.push(Entry::Block(node_block(&last, 1), None));
            }
            Statement::Space(count) => entries.push(Entry::Space(count)),
            Statement::Columns(columns) => entries.push(Entry::Columns(columns)),
            Statement::ClassDef { name, css } => entries.push(Entry::ClassDef(name, css)),
            Statement::ApplyClass { ids, class_name } => {
                entries.push(Entry::ApplyClass(ids, class_name))
            }
            Statement::ApplyStyles { ids, styles } => entries.push(Entry::ApplyStyles(ids, styles)),
        }
    }
    entries
}

/// populateBlockDatabase: 처음 나온 블록만 부모의 자식이 되고, 다시 나오면 모양·이름만 고친다
fn populate(
    diagram: &mut BlockDiagram,
    statements: Vec<Statement>,
    parent: usize,
) -> Result<(), DiagramError> {
    let mut children = Vec::new();
    for entry in flatten(diagram, statements) {
        match entry {
            Entry::Block(block, nested) => {
                if let Some(child) = add_block(diagram, block, nested)? {
                    children.push(child);
                }
            }
            Entry::Placeholder => children.extend(add_placeholder(diagram)),
            Entry::Space(count) => children.extend(add_spaces(diagram, count)),
            Entry::Edge(edge) => diagram.edges.push(edge),
            Entry::Columns(0) => return Err(DiagramError::Syntax("columns 0".to_string())),
            Entry::Columns(columns) => diagram.blocks[parent].columns = Some(columns),
            Entry::ClassDef(name, css) => add_style_class(diagram, &name, &css),
            Entry::ApplyClass(ids, class_name) => apply_class(diagram, &ids, &class_name),
            Entry::ApplyStyles(id, styles) => apply_styles(diagram, &id, &styles)?,
        }
    }
    diagram.blocks[parent].children = children;
    Ok(())
}

/// 새 블록이면 넣고 그 번호를 돌려준다. 이미 있으면 모양·이름만 고치고 자식 목록에는 넣지 않는다
fn add_block(
    diagram: &mut BlockDiagram,
    mut block: Block,
    nested: Option<Vec<Statement>>,
) -> Result<Option<usize>, DiagramError> {
    if block.label.is_empty() && block.kind != BlockType::Composite {
        block.label = block.id.clone();
    }
    if let Some(&existing) = diagram.index.get(&block.id) {
        let target = &mut diagram.blocks[existing];
        if block.kind != BlockType::Na {
            target.kind = block.kind;
        }
        if block.label != block.id {
            target.label = block.label;
        }
        return Ok(None);
    }
    let position = diagram.insert(block);
    if let Some(statements) = nested {
        populate(diagram, statements, position)?;
    }
    Ok(Some(position))
}

/// 이어 쓴 간선의 앞 노드 자리: 이름이 비어 있는 사각형이고, 이미 있으면 다시 넣지 않는다
fn add_placeholder(diagram: &mut BlockDiagram) -> Option<usize> {
    const PLACEHOLDER_ID: &str = "undefined";
    if diagram.index.contains_key(PLACEHOLDER_ID) {
        return None;
    }
    Some(diagram.insert(Block::new(PLACEHOLDER_ID, BlockType::Na)))
}

/// space:N은 빈칸 N개를 따로 넣는다
fn add_spaces(diagram: &mut BlockDiagram, count: u32) -> Vec<usize> {
    let id = diagram.generate_id();
    (0..count)
        .map(|index| {
            let mut block = Block::new(&format!("{id}-{index}"), BlockType::Space);
            block.label = block.id.clone();
            diagram.insert(block)
        })
        .collect()
}

/// addStyleClass: 쉼표로 나눈 선언에서 첫 `;`를 지운다
fn add_style_class(diagram: &mut BlockDiagram, name: &str, css: &str) {
    let class = diagram.classes.entry(name.to_string()).or_default();
    for attribute in css.split(',') {
        let fixed = attribute.replacen(';', "", 1).trim().to_string();
        class.styles.push(fixed);
    }
}

/// setCssClass: 없는 id면 모양 없는(`na`) 블록을 새로 만든다(자식 목록에는 넣지 않는다)
fn apply_class(diagram: &mut BlockDiagram, ids: &str, class_name: &str) {
    for id in ids.split(',') {
        let id = id.trim();
        let position = match diagram.index.get(id) {
            Some(position) => *position,
            None => diagram.insert(Block::new(id, BlockType::Na)),
        };
        diagram.blocks[position]
            .classes
            .push(class_name.to_string());
    }
}

/// addStyle2Node: 원본은 없는 블록이면 그리기 전에 실패한다
fn apply_styles(diagram: &mut BlockDiagram, id: &str, styles: &str) -> Result<(), DiagramError> {
    let position = *diagram
        .index
        .get(id)
        .ok_or_else(|| DiagramError::Syntax(format!("style 대상 블록이 없습니다: {id}")))?;
    diagram.blocks[position].styles = styles.split(',').map(str::to_string).collect();
    Ok(())
}

fn parse_diagram(definition: &str) -> Result<BlockDiagram, DiagramError> {
    let statements = parser::parse(&diagram_body(definition))?;
    let mut diagram = BlockDiagram::new();
    populate(&mut diagram, statements, ROOT)?;
    Ok(diagram)
}

/// createLabel(width: Infinity): 줄을 바꾸지 않는다. HTML은 div 상자, SVG 글자는 getBBox 크기다
pub(crate) fn measure_block_label(fonts: &FontBook, label: &str, mode: LabelMode) -> LabelBox {
    if mode == LabelMode::SvgText {
        return measure_svg_label(fonts, label, FONT_SIZE, f64::INFINITY);
    }
    let lines: Vec<LabelLine> = label_lines(label)
        .into_iter()
        .map(|text| LabelLine {
            width: fonts.measure_line(&text, FONT_SIZE, FontWeight::Regular),
            text,
        })
        .collect();
    let width = lines.iter().map(|line| line.width).fold(0.0, f64::max);
    let height = if label.is_empty() {
        0.0
    } else {
        lines.len() as f64 * LINE_HEIGHT
    };
    LabelBox {
        lines,
        width,
        height,
        mode,
        font_size: FONT_SIZE,
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let mut diagram = parse_diagram(definition)?;
    let palette = block_palette(context.palette, &context.theme_variables);
    let context = &RenderContext {
        fonts: context.fonts,
        palette: &palette,
        label_mode: context.label_mode,
        theme_variables: context.theme_variables.clone(),
    };
    let painter = BlockPainter::new(context);
    painter.measure_blocks(&mut diagram.blocks, ROOT);
    layout::set_block_sizes(&mut diagram.blocks, ROOT, 0.0, 0.0);
    layout::layout_blocks(&mut diagram.blocks, ROOT);
    let mut bounds = [0.0; 4];
    layout::find_bounds(&diagram.blocks, ROOT, &mut bounds);
    let mut scene = painter.paint(&diagram);
    let [min_x, min_y, max_x, max_y] = bounds;
    scene.width = max_x - min_x + VIEW_MARGIN * 2.0;
    scene.height = max_y - min_y + VIEW_MARGIN * 2.0;
    scene.translate(VIEW_MARGIN - min_x, VIEW_MARGIN - min_y);
    Ok(scene)
}

/// block styles.ts의 색. 블록은 mainBkg·nodeBorder, 묶음 블록은 clusterBkg·clusterBorder를 흐리게, SVG 글자는
/// `.label text`(nodeTextColor || textColor), 선은 lineColor, 간선 라벨 바탕은 edgeLabelBackground다. HTML 라벨의 `p`는
/// 뒤에 오는 `.cluster span,p` 규칙 때문에 titleColor가 된다
fn block_palette(base: &Palette, variables: &ThemeVariables) -> Palette {
    let mut palette = base.clone();
    variables.read_color("mainBkg", &mut palette.node_fill);
    variables.read_color("nodeBorder", &mut palette.node_stroke);
    variables.read_color("clusterBkg", &mut palette.cluster_fill);
    variables.read_color("clusterBorder", &mut palette.cluster_stroke);
    variables.read_color("titleColor", &mut palette.cluster_text);
    variables.read_color("lineColor", &mut palette.line);
    variables.read_color("edgeLabelBackground", &mut palette.edge_label_inner);
    if let Some(text) = ["nodeTextColor", "textColor"]
        .iter()
        .filter_map(|key| variables.text(key))
        .find(|text| !text.is_empty())
        .and_then(|text| parse_theme_color(&text))
    {
        palette.text = text;
    }
    palette
}

/// 블록의 `style` 선언과 classDef 선언을 합친다. classDef는 `!important`라서 `style`보다 이긴다
pub(crate) fn resolve_style(diagram: &BlockDiagram, block: &Block) -> ElementStyle {
    let inline = ElementStyle::from_declarations(block.styles.iter());
    let class_declarations: Vec<String> = block
        .classes
        .iter()
        .filter_map(|name| diagram.classes.get(name))
        .flat_map(|class| class.styles.iter().cloned())
        .collect();
    let classed = ElementStyle::from_declarations(class_declarations.iter());
    ElementStyle {
        fill: classed.fill.or(inline.fill),
        stroke: classed.stroke.or(inline.stroke),
        stroke_width: classed.stroke_width.or(inline.stroke_width),
        dash: if classed.dash.is_empty() {
            inline.dash
        } else {
            classed.dash
        },
        text_color: classed.text_color.or(inline.text_color),
        ..ElementStyle::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 처음_나온_블록만_자식이_되고_빈칸은_칸수만큼_생긴다() {
        let diagram =
            parse_diagram("block-beta\n  columns 3\n  a b:2\n  space:2\n  a --> c\n  b[\"B\"]")
                .unwrap();
        let root = &diagram.blocks[ROOT];
        assert_eq!(root.columns, Some(3));
        let ids: Vec<&str> = root
            .children
            .iter()
            .map(|child| diagram.blocks[*child].id.as_str())
            .collect();
        assert_eq!(ids.len(), 5);
        assert_eq!(&ids[..2], &["a", "b"]);
        assert_eq!(ids[4], "c");
        let b = &diagram.blocks[diagram.index["b"]];
        assert_eq!((b.label.as_str(), b.kind), ("B", BlockType::Square));
        assert_eq!(b.width_in_columns, 2);
        assert_eq!(diagram.edges[0].marker_end, EdgeMarker::Point);
    }

    #[test]
    fn 간선_기호로_표식과_굵기와_점선을_정한다() {
        let edge = BlockEdge::new("a", "b", " x==o ", String::new());
        assert_eq!(
            (edge.marker_start, edge.marker_end, edge.is_thick),
            (EdgeMarker::Cross, EdgeMarker::Circle, true)
        );
        let dotted = BlockEdge::new("a", "b", "-.->", String::new());
        assert!(dotted.is_dotted);
        assert_eq!(dotted.marker_start, EdgeMarker::None);
    }

    #[test]
    fn class_def는_style보다_이긴다() {
        let diagram = parse_diagram(
            "block\n  a\n  classDef warm fill:#f96,stroke:#333,color:#000\n  class a warm\n  style a fill:#bbf,stroke-width:2px",
        )
        .unwrap();
        let style = resolve_style(&diagram, &diagram.blocks[diagram.index["a"]]);
        assert_eq!(style.fill, crate::scene::Color::parse_css("#f96"));
        assert_eq!(style.stroke_width, Some(2.0));
        assert!(style.text_color.is_some());
    }
}
