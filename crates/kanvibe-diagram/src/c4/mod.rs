//! C4 다이어그램(c4Renderer.js·svgDraw.js). C4Context·C4Container·C4Component·C4Dynamic·C4Deployment를 같은
//! 규칙으로 그린다. 라벨은 늘 SVG 글자라 모바일·데스크톱 결과가 같다.

mod draw;
mod fields;
mod layout;
mod parser;

use crate::scene::{PathCommand, Point, Scene, StrokeStyle, TextItem};
use crate::text::FontWeight;
use crate::{DiagramError, RenderContext};

use draw::{RELATION_COLOR, draw_boundary, draw_shape, middle_text, push_marker, style_color};
use layout::{
    C4Layout, DIAGRAM_MARGIN_X, DIAGRAM_MARGIN_Y, Drawable, SMALL_FONT_SIZE, ShapeBox,
    measure_block,
};
use parser::{C4Diagram, C4Relation};

/// 제목이 있으면 viewBox 위쪽을 이만큼 더 연다
const TITLE_SPACE: f64 = 60.0;
const TITLE_FONT_SIZE: f64 = 16.0;
const ARROW_HEAD: [(f64, f64); 3] = [(0.0, 0.0), (10.0, 5.0), (0.0, 10.0)];
const ARROW_END: [(f64, f64); 3] = [(10.0, 0.0), (0.0, 5.0), (10.0, 10.0)];

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let diagram = parser::parse(definition)?;
    let layout = layout::layout(context.fonts, &diagram);
    let mut scene = Scene::default();
    for drawable in &layout.drawables {
        match drawable {
            Drawable::Shape(index) => {
                if let Some(shape) = &layout.shapes[*index] {
                    draw_shape(&mut scene, context.fonts, shape);
                }
            }
            Drawable::Boundary(boundary) => draw_boundary(&mut scene, context.fonts, boundary),
        }
    }
    draw_relations(&mut scene, &diagram, &layout, context);
    fit_viewport(&mut scene, &diagram, &layout, context);
    Ok(scene)
}

/// 제목을 그리고 viewBox(`startx − 50, −(10 + 제목 여백), 폭, 높이`)에 맞춰 옮긴다
fn fit_viewport(
    scene: &mut Scene,
    diagram: &C4Diagram,
    layout: &C4Layout,
    context: &RenderContext,
) {
    let box_width = layout.stop_x - layout.start_x;
    let box_height = layout.stop_y - layout.start_y;
    let has_title = !diagram.title.is_empty();
    if has_title {
        scene.push_text(TextItem {
            text: diagram.title.clone(),
            x: box_width / 2.0 - 4.0 * DIAGRAM_MARGIN_X,
            baseline: layout.start_y + DIAGRAM_MARGIN_Y,
            font_size: TITLE_FONT_SIZE,
            weight: FontWeight::Regular,
            color: context.palette.text,
            rotation: None,
        });
    }
    let title_space = if has_title { TITLE_SPACE } else { 0.0 };
    scene.width = box_width + 2.0 * DIAGRAM_MARGIN_X;
    scene.height = box_height + 2.0 * DIAGRAM_MARGIN_Y + title_space;
    scene.translate(
        -(layout.start_x - DIAGRAM_MARGIN_X),
        DIAGRAM_MARGIN_Y + title_space,
    );
}

/// drawRels: 첫 관계는 곧은 선, 나머지는 2차 곡선이다. 글자는 두 끝점 가운데에서 라벨 폭 절반만큼 오른쪽에 두고
/// `UpdateRelStyle`의 `offsetX`·`offsetY`만큼 옮긴다. 선 색을 바꿔도 화살촉 표식 색은 그대로다
fn draw_relations(
    scene: &mut Scene,
    diagram: &C4Diagram,
    layout: &C4Layout,
    context: &RenderContext,
) {
    let find = |alias: &str| {
        diagram
            .elements
            .iter()
            .position(|element| element.alias == alias)
            .and_then(|index| layout.shapes[index].as_ref())
    };
    for (index, relation) in diagram.relations.iter().enumerate() {
        let (Some(from), Some(to)) = (find(&relation.from), find(&relation.to)) else {
            continue;
        };
        let (start, end) = intersect_points(from, to);
        let label = if diagram.diagram_type == "C4Dynamic" {
            format!("{}: {}", index + 1, relation.label)
        } else {
            relation.label.clone()
        };
        draw_relation_line(scene, relation, index == 0, (start, end), context);
        draw_relation_text(scene, relation, &label, (start, end), context);
    }
}

fn draw_relation_line(
    scene: &mut Scene,
    relation: &C4Relation,
    is_straight: bool,
    (start, end): (Point, Point),
    context: &RenderContext,
) {
    let control = Point::new(
        start.x + (end.x - start.x) / 2.0 - (end.x - start.x) / 4.0,
        start.y + (end.y - start.y) / 2.0,
    );
    let (commands, end_direction, start_direction) = if is_straight {
        let line = vec![PathCommand::MoveTo(start), PathCommand::LineTo(end)];
        (line, (start, end), (start, end))
    } else {
        let curve = vec![
            PathCommand::MoveTo(start),
            PathCommand::QuadTo(control, end),
        ];
        (curve, (control, end), (start, control))
    };
    let stroke = StrokeStyle {
        color: style_color(relation.line_color.as_deref(), RELATION_COLOR),
        width: 1.0,
        dash: Vec::new(),
    };
    scene.push_path(commands, None, Some(stroke));
    let marker_color = context.palette.text;
    if relation.kind != "rel_b" {
        let angle = direction_angle(end_direction);
        push_marker(scene, &ARROW_HEAD, (9.0, 5.0), end, angle, marker_color);
    }
    if relation.kind == "birel" || relation.kind == "rel_b" {
        let angle = direction_angle(start_direction);
        push_marker(scene, &ARROW_END, (1.0, 5.0), start, angle, marker_color);
    }
}

fn direction_angle((from, to): (Point, Point)) -> f64 {
    (to.y - from.y).atan2(to.x - from.x)
}

fn draw_relation_text(
    scene: &mut Scene,
    relation: &C4Relation,
    label: &str,
    (start, end): (Point, Point),
    context: &RenderContext,
) {
    let fonts = context.fonts;
    let color = style_color(relation.text_color.as_deref(), RELATION_COLOR);
    let middle_x = start.x.min(end.x) + (end.x - start.x).abs() / 2.0 + relation.offset_x;
    let middle_y = start.y.min(end.y) + (end.y - start.y).abs() / 2.0 + relation.offset_y;
    let label_block = measure_block(fonts, label, SMALL_FONT_SIZE, FontWeight::Regular);
    let mut texts = middle_text(
        fonts,
        label,
        middle_x + label_block.width / 2.0,
        middle_y,
        SMALL_FONT_SIZE,
        FontWeight::Regular,
        color,
    );
    if !relation.techn.is_empty() {
        let techn_block =
            measure_block(fonts, &relation.techn, SMALL_FONT_SIZE, FontWeight::Regular);
        let width = label_block.width.max(techn_block.width);
        texts.extend(middle_text(
            fonts,
            &format!("[{}]", relation.techn),
            middle_x + width / 2.0,
            middle_y + SMALL_FONT_SIZE + 5.0,
            SMALL_FONT_SIZE,
            FontWeight::Italic,
            color,
        ));
    }
    texts.into_iter().for_each(|text| scene.push_text(text));
}

/// getIntersectPoints: 두 모양의 중심을 잇는 선이 각 모양 테두리와 만나는 점
fn intersect_points(from: &ShapeBox, to: &ShapeBox) -> (Point, Point) {
    let to_center = Point::new(to.x + to.width / 2.0, to.y + to.height / 2.0);
    let from_center = Point::new(from.x + from.width / 2.0, from.y + from.height / 2.0);
    (
        intersect_point(from, to_center),
        intersect_point(to, from_center),
    )
}

/// getIntersectPoint. 원본처럼 모양 중심이 아닌 왼쪽 위 꼭짓점과 상대 중심을 비교해 방향을 고른다
fn intersect_point(node: &ShapeBox, target: Point) -> Point {
    let (x1, y1) = (node.x, node.y);
    let (x2, y2) = (target.x, target.y);
    let center_x = x1 + node.width / 2.0;
    let center_y = y1 + node.height / 2.0;
    let (dx, dy) = ((x1 - x2).abs(), (y1 - y2).abs());
    let tangent = dy / dx;
    let node_ratio = node.height / node.width;
    let is_shallow = node_ratio >= tangent;
    match (x1.partial_cmp(&x2), y1.partial_cmp(&y2)) {
        (Some(std::cmp::Ordering::Greater), Some(std::cmp::Ordering::Less)) if is_shallow => {
            Point::new(x1, center_y + tangent * node.width / 2.0)
        }
        (Some(std::cmp::Ordering::Greater), Some(std::cmp::Ordering::Less)) => {
            Point::new(center_x - dx / dy * node.height / 2.0, y1 + node.height)
        }
        (Some(std::cmp::Ordering::Less), Some(std::cmp::Ordering::Less)) if is_shallow => {
            Point::new(x1 + node.width, center_y + tangent * node.width / 2.0)
        }
        (Some(std::cmp::Ordering::Less), Some(std::cmp::Ordering::Less)) => {
            Point::new(center_x + dx / dy * node.height / 2.0, y1 + node.height)
        }
        (Some(std::cmp::Ordering::Less), Some(std::cmp::Ordering::Greater)) if is_shallow => {
            Point::new(x1 + node.width, center_y - tangent * node.width / 2.0)
        }
        (Some(std::cmp::Ordering::Less), Some(std::cmp::Ordering::Greater)) => {
            Point::new(center_x + node.height / 2.0 * dx / dy, y1)
        }
        (Some(std::cmp::Ordering::Greater), Some(std::cmp::Ordering::Greater)) if is_shallow => {
            Point::new(x1, center_y - node.width / 2.0 * tangent)
        }
        (Some(std::cmp::Ordering::Greater), Some(std::cmp::Ordering::Greater)) => {
            Point::new(center_x - node.height / 2.0 * dx / dy, y1)
        }
        _ => axis_intersect_point(node, target),
    }
}

/// 꼭짓점과 상대 중심이 한 축 위에 있을 때(원본의 앞쪽 네 갈래)
fn axis_intersect_point(node: &ShapeBox, target: Point) -> Point {
    let center = Point::new(node.x + node.width / 2.0, node.y + node.height / 2.0);
    if node.y == target.y {
        let x = if node.x < target.x {
            node.x + node.width
        } else {
            node.x
        };
        Point::new(x, center.y)
    } else if node.y < target.y {
        Point::new(center.x, node.y + node.height)
    } else {
        Point::new(center.x, node.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LabelMode;
    use crate::text::tests::bundled_fonts;
    use crate::theme::{Palette, Theme};

    #[test]
    fn 관계_끝점과_viewbox가_기준_svg와_같다() {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::Html,
            theme_variables: Default::default(),
        };
        let definition = "C4Context\n  title System Context\n  Person(user, \"User\", \"A KanVibe user\")\n  System(app, \"KanVibe\", \"Task board with terminals\")\n  Rel(user, app, \"Uses\")\n";
        let diagram = parser::parse(definition).unwrap();
        let layout = layout::layout(&fonts, &diagram);
        let shapes: Vec<&ShapeBox> = layout.shapes.iter().flatten().collect();
        let (start, end) = intersect_points(shapes[0], shapes[1]);
        assert!((start.x - 284.532_871_972_318_33).abs() < 1e-9 && start.y == 316.0);
        assert!((end.x - 287.684_210_526_315_8).abs() < 1e-9 && end.y == 416.0);
        assert_eq!(
            measure_block(&fonts, "Uses", 12.0, FontWeight::Regular).width,
            27.0
        );
        assert_eq!(
            measure_block(&fonts, "Fetches PRs", 12.0, FontWeight::Regular).width,
            67.0
        );
        let scene = render(definition, &context).unwrap();
        assert_eq!(scene.width, 516.0);
    }
}
