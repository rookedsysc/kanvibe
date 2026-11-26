//! c4Renderer.js의 배치(`Bounds`, `drawInsideBoundary`, `drawC4ShapeArray`). 모양 크기는 글자 크기에서 나오고,
//! 모양은 경계 안에서 줄 단위로 쌓인다.

use crate::text::{FontBook, FontWeight};

use super::parser::{C4Boundary, C4Diagram, C4Element, GLOBAL_BOUNDARY, ShapeStyle};

pub(crate) const DIAGRAM_MARGIN_X: f64 = 50.0;
pub(crate) const DIAGRAM_MARGIN_Y: f64 = 10.0;
const SHAPE_MARGIN: f64 = 50.0;
const SHAPE_PADDING: f64 = 20.0;
const SHAPE_WIDTH: f64 = 216.0;
const SHAPE_HEIGHT: f64 = 60.0;
const IMAGE_SIZE: f64 = 48.0;
/// `screen.availWidth`로 정하는 줄 폭 한계. 기준 SVG(모바일 워커의 화면 밖 창)는 모양마다 새 줄을 시작하므로
/// 한계가 두 번째 모양 끝(682px)보다 좁다. 그 동작을 그대로 따르도록 0으로 둔다
const SCREEN_AVAILABLE_WIDTH: f64 = 0.0;

pub(crate) const LABEL_FONT_SIZE: f64 = 16.0;
pub(crate) const TEXT_FONT_SIZE: f64 = 14.0;
pub(crate) const SMALL_FONT_SIZE: f64 = 12.0;

/// calcC4ShapeTextWH가 잰 글자 묶음. `offset_y`는 모양 위 끝에서 글자 기준점까지의 거리다
#[derive(Clone, Debug, Default)]
pub(crate) struct TextBlock {
    pub text: String,
    pub width: f64,
    pub height: f64,
    pub line_count: usize,
    pub offset_y: f64,
}

impl TextBlock {
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ShapeBox {
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// `«종류»` 글자 폭(그릴 때 `textLength`)
    pub type_width: f64,
    /// 사람 그림 자리. 스프라이트가 있으면 그 아래로 내려간다(스프라이트 그림은 그리지 않는다)
    pub image_y: Option<f64>,
    pub style: ShapeStyle,
    pub label: TextBlock,
    pub techn: TextBlock,
    pub descr: TextBlock,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BoundaryBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub label: TextBlock,
    pub kind: TextBlock,
    pub descr: TextBlock,
    pub is_deployment_node: bool,
    pub style: ShapeStyle,
}

/// 그리는 순서
pub(crate) enum Drawable {
    Shape(usize),
    Boundary(Box<BoundaryBox>),
}

pub(crate) struct C4Layout {
    /// `diagram.elements`와 같은 순서. 어느 경계에도 속하지 않아 그리지 않은 요소는 `None`
    pub shapes: Vec<Option<ShapeBox>>,
    pub drawables: Vec<Drawable>,
    pub start_x: f64,
    pub start_y: f64,
    pub stop_x: f64,
    pub stop_y: f64,
}

/// 글자 폭·높이: calculateTextDimensions(getBBox를 정수로 반올림, 줄마다 높이를 더한다)
pub(crate) fn measure_block(
    fonts: &FontBook,
    text: &str,
    font_size: f64,
    weight: FontWeight,
) -> TextBlock {
    if text.is_empty() {
        return TextBlock::default();
    }
    let lines = split_lines(text);
    let metrics = fonts.primary_metrics(font_size);
    let width = lines
        .iter()
        .map(|line| {
            fonts
                .measure_svg_runs(&[line], font_size, weight)
                .width()
                .round()
        })
        .fold(0.0, f64::max);
    TextBlock {
        text: text.to_string(),
        width,
        height: (metrics.ascent + metrics.descent) * lines.len() as f64,
        line_count: lines.len(),
        offset_y: 0.0,
    }
}

/// common.lineBreakRegex(`<br>`, `<br/>`, `<br />`)로 줄을 나눈다
pub(crate) fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.to_ascii_lowercase().find("<br") {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        lines.push(rest[..start].to_string());
        rest = &rest[start + end + 1..];
    }
    lines.push(rest.to_string());
    lines
}

#[derive(Clone, Copy, Debug, Default)]
struct Extent {
    start_x: f64,
    stop_x: f64,
    start_y: f64,
    stop_y: f64,
}

impl Extent {
    fn include(&mut self, start_x: f64, start_y: f64, stop_x: f64, stop_y: f64) {
        self.start_x = self.start_x.min(start_x);
        self.start_y = self.start_y.min(start_y);
        self.stop_x = self.stop_x.max(stop_x);
        self.stop_y = self.stop_y.max(stop_y);
    }
}

/// c4Renderer `Bounds`: 현재 경계 범위(원본 data)와 지금 줄 범위(nextData)
#[derive(Clone, Debug, Default)]
struct Bounds {
    extent: Extent,
    next: Extent,
    next_count: usize,
    width_limit: f64,
    /// `UpdateLayoutConfig`의 줄당 모양 수
    shapes_in_row: usize,
}

impl Bounds {
    fn reset_origin(&mut self, x: f64, y: f64) {
        self.extent = Extent {
            start_x: x,
            stop_x: x,
            start_y: y,
            stop_y: y,
        };
        self.next = self.extent;
    }

    /// 지금 줄 오른쪽에 붙이고, 한계를 넘거나 줄이 차면 다음 줄 왼쪽으로 내린다
    fn insert(&mut self, shape: &mut ShapeBox) {
        self.next_count += 1;
        let mut start_x = if self.next.start_x == self.next.stop_x {
            self.next.stop_x + SHAPE_MARGIN
        } else {
            self.next.stop_x + SHAPE_MARGIN * 2.0
        };
        let mut stop_x = start_x + shape.width;
        let mut start_y = self.next.start_y + SHAPE_MARGIN * 2.0;
        let mut stop_y = start_y + shape.height;
        let is_overflow = start_x >= self.width_limit || stop_x >= self.width_limit;
        if is_overflow || self.next_count > self.shapes_in_row {
            start_x = self.next.start_x + SHAPE_MARGIN;
            start_y = self.next.stop_y + SHAPE_MARGIN * 2.0;
            stop_x = start_x + shape.width;
            self.next.stop_x = stop_x;
            self.next.start_y = self.next.stop_y;
            stop_y = start_y + shape.height;
            self.next.stop_y = stop_y;
            self.next_count = 1;
        }
        shape.x = start_x;
        shape.y = start_y;
        self.extent.include(start_x, start_y, stop_x, stop_y);
        self.next.include(start_x, start_y, stop_x, stop_y);
    }
}

struct LayoutBuilder<'a> {
    fonts: &'a FontBook,
    diagram: &'a C4Diagram,
    layout: C4Layout,
    global_max_x: f64,
    global_max_y: f64,
}

pub(crate) fn layout(fonts: &FontBook, diagram: &C4Diagram) -> C4Layout {
    let mut builder = LayoutBuilder {
        fonts,
        diagram,
        layout: C4Layout {
            shapes: vec![None; diagram.elements.len()],
            drawables: Vec::new(),
            start_x: DIAGRAM_MARGIN_X,
            start_y: DIAGRAM_MARGIN_Y,
            stop_x: 0.0,
            stop_y: 0.0,
        },
        global_max_x: DIAGRAM_MARGIN_X,
        global_max_y: DIAGRAM_MARGIN_Y,
    };
    let mut screen = Bounds {
        width_limit: SCREEN_AVAILABLE_WIDTH,
        shapes_in_row: diagram.shapes_in_row,
        ..Bounds::default()
    };
    screen.reset_origin(DIAGRAM_MARGIN_X, DIAGRAM_MARGIN_Y);
    let top_level = builder.child_boundaries("");
    builder.layout_inside(&mut screen, &top_level);
    builder.layout.stop_x = builder.global_max_x;
    builder.layout.stop_y = builder.global_max_y;
    builder.layout
}

impl<'a> LayoutBuilder<'a> {
    fn child_boundaries(&self, parent: &str) -> Vec<&'a C4Boundary> {
        self.diagram
            .boundaries
            .iter()
            .filter(|boundary| boundary.parent == parent)
            .collect()
    }

    /// drawInsideBoundary: 경계들을 두 개씩 한 줄에 놓고, 각 경계 안의 모양과 하위 경계를 배치한다
    fn layout_inside(&mut self, parent: &mut Bounds, boundaries: &[&'a C4Boundary]) {
        let boundaries_in_row = self.diagram.boundaries_in_row;
        let mut current = Bounds {
            width_limit: parent.width_limit / boundaries_in_row.min(boundaries.len()).max(1) as f64,
            shapes_in_row: self.diagram.shapes_in_row,
            ..Bounds::default()
        };
        for (index, boundary) in boundaries.iter().enumerate() {
            let (mut boundary_box, text_height) = self.measure_boundary(boundary);
            if index.is_multiple_of(boundaries_in_row) {
                let x = parent.extent.start_x + DIAGRAM_MARGIN_X;
                let y = parent.extent.stop_y + DIAGRAM_MARGIN_Y + text_height;
                current.reset_origin(x, y);
            } else {
                let x = if current.extent.stop_x != current.extent.start_x {
                    current.extent.stop_x + DIAGRAM_MARGIN_X
                } else {
                    current.extent.start_x
                };
                let y = current.extent.start_y;
                current.reset_origin(x, y);
            }
            self.layout_shapes(&mut current, &boundary.alias);
            let children = self.child_boundaries(&boundary.alias);
            if !children.is_empty() {
                self.layout_inside(&mut current, &children);
            }
            if boundary.alias != GLOBAL_BOUNDARY {
                boundary_box.x = current.extent.start_x;
                boundary_box.y = current.extent.start_y;
                boundary_box.width = current.extent.stop_x - current.extent.start_x;
                boundary_box.height = current.extent.stop_y - current.extent.start_y;
                self.layout
                    .drawables
                    .push(Drawable::Boundary(Box::new(boundary_box)));
            }
            parent.extent.stop_y = (current.extent.stop_y + SHAPE_MARGIN).max(parent.extent.stop_y);
            parent.extent.stop_x = (current.extent.stop_x + SHAPE_MARGIN).max(parent.extent.stop_x);
            self.global_max_x = self.global_max_x.max(parent.extent.stop_x);
            self.global_max_y = self.global_max_y.max(parent.extent.stop_y);
        }
    }

    /// 경계 이름(굵게 16px)·종류(`[종류]`, 14px)·설명(12px)을 잰다. 스프라이트가 있으면 그림 자리만큼 내린다.
    /// 둘째 값은 글자가 차지한 높이다
    fn measure_boundary(&self, boundary: &C4Boundary) -> (BoundaryBox, f64) {
        let mut label = measure_block(
            self.fonts,
            &boundary.label,
            LABEL_FONT_SIZE,
            FontWeight::Bold,
        );
        let image_height = if boundary.has_sprite { IMAGE_SIZE } else { 0.0 };
        label.offset_y = image_height + 8.0;
        let mut y = label.offset_y + label.height;
        let mut kind = TextBlock::default();
        if !boundary.kind.is_empty() {
            kind = measure_block(
                self.fonts,
                &format!("[{}]", boundary.kind),
                TEXT_FONT_SIZE,
                FontWeight::Regular,
            );
            kind.offset_y = y + 5.0;
            y = kind.offset_y + kind.height;
        }
        let mut descr = TextBlock::default();
        if !boundary.descr.is_empty() {
            descr = measure_block(
                self.fonts,
                &boundary.descr,
                SMALL_FONT_SIZE,
                FontWeight::Regular,
            );
            descr.offset_y = y + 20.0;
            y = descr.offset_y + descr.height;
        }
        let boundary_box = BoundaryBox {
            label,
            kind,
            descr,
            is_deployment_node: boundary.is_deployment_node,
            style: boundary.style.clone(),
            ..BoundaryBox::default()
        };
        (boundary_box, y)
    }

    /// drawC4ShapeArray: 경계에 속한 모양을 재고 차례로 끼운 뒤 마지막 여백을 더한다
    fn layout_shapes(&mut self, bounds: &mut Bounds, parent: &str) {
        let members: Vec<usize> = (0..self.diagram.elements.len())
            .filter(|&index| self.diagram.elements[index].parent == parent)
            .collect();
        if members.is_empty() {
            return;
        }
        for index in members {
            let mut shape = measure_shape(self.fonts, &self.diagram.elements[index]);
            bounds.insert(&mut shape);
            self.layout.shapes[index] = Some(shape);
            self.layout.drawables.push(Drawable::Shape(index));
        }
        bounds.extent.stop_x += SHAPE_MARGIN;
        bounds.extent.stop_y += SHAPE_MARGIN;
    }
}

/// 사람 그림과 스프라이트 자리(각 48px)를 `top`부터 쌓는다. 사람 그림은 마지막 자리에 그리고, 스프라이트 그림은
/// 그리지 않는다. 둘째 값은 그 아래 y다
fn image_slots(element: &C4Element, top: f64) -> (Option<f64>, f64) {
    let mut image_y = None;
    let mut y = top;
    if element.kind.ends_with("person") {
        image_y = Some(y);
        y += IMAGE_SIZE;
    }
    if element.has_sprite {
        image_y = image_y.map(|_| y);
        y += IMAGE_SIZE;
    }
    (image_y, y)
}

/// 모양 안 글자를 위에서부터 쌓아 크기를 정한다(`«종류»` → 사람 그림 → 스프라이트 자리 → 이름 → 기술 → 설명)
fn measure_shape(fonts: &FontBook, element: &C4Element) -> ShapeBox {
    let type_text = format!("\u{ab}{}\u{bb}", element.kind);
    let type_width = measure_block(fonts, &type_text, SMALL_FONT_SIZE, FontWeight::Regular).width;
    let (image_y, mut y) = image_slots(element, SHAPE_PADDING + (SMALL_FONT_SIZE + 2.0) - 4.0);
    let mut label = measure_block(fonts, &element.label, LABEL_FONT_SIZE, FontWeight::Bold);
    label.offset_y = y + 8.0;
    y = label.offset_y + label.height;
    let mut techn = TextBlock::default();
    if !element.techn.is_empty() {
        // 원본은 기술 글자 크기를 정의되지 않은 설정으로 재서 기본 12px로 잰다
        techn = measure_block(
            fonts,
            &format!("[{}]", element.techn),
            SMALL_FONT_SIZE,
            FontWeight::Regular,
        );
        techn.offset_y = y + 5.0;
        y = techn.offset_y + techn.height;
    }
    let mut rect_height = y;
    let mut rect_width = label.width;
    let mut descr = TextBlock::default();
    if !element.descr.is_empty() {
        descr = measure_block(fonts, &element.descr, TEXT_FONT_SIZE, FontWeight::Regular);
        descr.offset_y = y + 20.0;
        y = descr.offset_y + descr.height;
        rect_width = label.width.max(descr.width);
        rect_height = y - descr.line_count as f64 * 5.0;
    }
    ShapeBox {
        kind: element.kind.clone(),
        width: SHAPE_WIDTH.max(rect_width + SHAPE_PADDING),
        height: SHAPE_HEIGHT.max(rect_height),
        type_width,
        image_y,
        style: element.style.clone(),
        label,
        techn,
        descr,
        ..ShapeBox::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c4::parser::parse;
    use crate::text::tests::bundled_fonts;

    #[test]
    fn 기준_svg와_같은_자리에_모양을_놓는다() {
        let fonts = bundled_fonts();
        let diagram = parse(
            "C4Context\n  title System Context\n  Person(user, \"User\", \"A KanVibe user\")\n  System(app, \"KanVibe\", \"Task board with terminals\")\n  System_Ext(git, \"GitHub\", \"Hosts repositories\")\n",
        )
        .unwrap();
        let layout = layout(&fonts, &diagram);
        let boxes: Vec<(f64, f64, f64, f64)> = layout
            .shapes
            .iter()
            .flatten()
            .map(|shape| (shape.x, shape.y, shape.width, shape.height))
            .collect();
        assert_eq!(
            boxes,
            vec![
                (150.0, 174.0, 216.0, 142.0),
                (150.0, 416.0, 216.0, 94.0),
                (150.0, 610.0, 216.0, 94.0)
            ]
        );
        assert_eq!((layout.stop_x, layout.stop_y), (466.0, 804.0));
        assert_eq!(layout.shapes[0].as_ref().unwrap().type_width, 52.0);
        assert_eq!(layout.shapes[2].as_ref().unwrap().type_width, 104.0);
    }

    /// 기준 사례 정의를 읽어 배치한다
    fn baseline_layout(fonts: &FontBook, id: &str) -> C4Layout {
        let path = format!(
            "{}/../../parity/diagrams/baseline/{id}.mmd",
            env!("CARGO_MANIFEST_DIR")
        );
        let source = std::fs::read_to_string(path).unwrap();
        layout(fonts, &parse(&source).unwrap())
    }

    #[test]
    fn 스프라이트가_있으면_그림_자리만큼_글자를_내린다() {
        let fonts = bundled_fonts();
        let placed = baseline_layout(&fonts, "c4-sprites-links");
        let boxes: Vec<(f64, f64, f64, f64)> = placed
            .shapes
            .iter()
            .flatten()
            .map(|shape| (shape.x, shape.y, shape.width, shape.height))
            .collect();
        assert_eq!(boxes[0], (150.0, 174.0, 216.0, 190.0));
        assert_eq!(placed.shapes[0].as_ref().unwrap().image_y, Some(78.0));
        assert_eq!(boxes[1], (200.0, 578.0, 216.0, 164.0));
        assert_eq!(boxes[3], (200.0, 1036.0, 216.0, 60.0));
        let phone = placed.drawables.iter().find_map(|drawable| match drawable {
            Drawable::Boundary(boundary) if boundary.label.text == "Phone" => {
                Some((boundary.x, boundary.y, boundary.label.offset_y))
            }
            _ => None,
        });
        assert_eq!(phone, Some((516.0, 478.0, 56.0)));
    }

    #[test]
    fn 줄당_경계_수를_바꾸면_경계를_세로로_쌓는다() {
        let fonts = bundled_fonts();
        let placed = baseline_layout(&fonts, "c4-styles");
        let boundaries: Vec<(f64, f64, f64, f64)> = placed
            .drawables
            .iter()
            .filter_map(|drawable| match drawable {
                Drawable::Boundary(boundary) => {
                    Some((boundary.x, boundary.y, boundary.width, boundary.height))
                }
                Drawable::Shape(_) => None,
            })
            .collect();
        assert_eq!(
            boundaries,
            vec![(150.0, 624.0, 316.0, 244.0), (150.0, 982.0, 316.0, 292.0)]
        );
    }
}
