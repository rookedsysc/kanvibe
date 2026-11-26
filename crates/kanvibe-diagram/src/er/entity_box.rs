//! rendering-elements/shapes/erBox.ts: 속성이 있는 엔티티는 이름 줄과 속성 표(타입·이름·키·설명 열)를,
//! 속성이 없는 엔티티는 drawRect 사각형을 그린다.

use super::parser::{Attribute, Entity};
use super::unified::{
    decorated_node, empty_text_extent, label_items, measure_single_line, rough_line, rough_rect,
};
use crate::LabelMode;
use crate::flowchart::FlowNode;
use crate::flowchart::shapes::{Intersect, NodeGeometry};
use crate::label::{LabelBox, measure_label};
use crate::scene::{Color, Point, SceneItem, StrokeStyle, rounded_rect};
use crate::text::{FontBook, FontWeight};

/// er.diagramPadding(20)·er.entityPadding(15)에 1.25를 곱한 값(htmlLabels 설정이 최상위에 없어 두 방식 모두 곱한다)
const PADDING: f64 = 25.0;
const TEXT_PADDING: f64 = 18.75;
/// 속성 없는 엔티티의 drawRect 여백과 er.minEntityWidth
const RECT_PADDING: f64 = 20.0;
const MIN_ENTITY_WIDTH: f64 = 100.0;

/// 엔티티 상자 색(테마 기본에 classDef·style을 덮은 값)
pub struct EntityColors {
    pub fill: Color,
    /// 바깥 상자·줄 상자·구분선 테두리(사용자 stroke 선언은 모든 경로에 먹는다)
    pub border: StrokeStyle,
    pub row_odd: Color,
    pub row_even: Color,
    pub text: Color,
}

pub struct EntityStyle<'a> {
    pub fonts: &'a FontBook,
    pub mode: LabelMode,
    pub colors: &'a EntityColors,
}

/// 속성 한 줄의 네 칸 라벨
struct AttributeRow {
    cells: [LabelBox; 4],
    offset: f64,
    height: f64,
}

/// 타입·이름·키·설명 열 폭
struct Columns {
    widths: [f64; 4],
}

impl Columns {
    fn offset(&self, column: usize) -> f64 {
        self.widths[..column].iter().sum()
    }

    fn total(&self) -> f64 {
        self.widths.iter().sum()
    }
}

pub fn entity_node(id: String, entity: &Entity, style: &EntityStyle) -> FlowNode {
    let title = if entity.alias.is_empty() {
        &entity.name
    } else {
        &entity.alias
    };
    if entity.attributes.is_empty() {
        return plain_entity_node(id, title, style);
    }
    let name = measure_single_line(style.fonts, title, style.mode, FontWeight::Regular);
    let rows = attribute_rows(&entity.attributes, style);
    let columns = column_widths(&rows, &name);
    let widest_label = rows
        .iter()
        .flat_map(|row| row.cells.iter().map(|cell| cell.width))
        .fold(name.width, f64::max);
    let width = (widest_label + PADDING * 2.0).max(columns.total());
    let rows_height: f64 = rows.iter().map(|row| row.height).sum();
    let height = rows_height + name.height + TEXT_PADDING;
    let table = EntityTable {
        name,
        rows,
        columns,
        width,
        height,
    };
    decorated_node(id, width, height, table.decorations(style))
}

fn plain_entity_node(id: String, title: &str, style: &EntityStyle) -> FlowNode {
    let label = measure_label(style.fonts, title, style.mode);
    let natural_width =
        style
            .fonts
            .measure_line(title, crate::label::FONT_SIZE, FontWeight::Regular);
    let mut width = label.width + RECT_PADDING * 2.0;
    if natural_width + RECT_PADDING * 2.0 < MIN_ENTITY_WIDTH {
        width = width.max(MIN_ENTITY_WIDTH);
    }
    let height = label.height + RECT_PADDING * 3.0;
    let mut node = decorated_node(id, width, height, Vec::new());
    node.geometry = Some(NodeGeometry {
        width,
        height,
        outlines: vec![rounded_rect(
            -width / 2.0,
            -height / 2.0,
            width,
            height,
            0.0,
        )],
        intersect: Intersect::Rect,
        label_offset_y: 0.0,
    });
    node.label = label;
    node
}

/// parseGenericTypes: `~T~`를 `<T>`로 바꾼다
fn generic_type_text(text: &str) -> String {
    let mut is_open = false;
    text.chars()
        .map(|character| match character {
            '~' => {
                is_open = !is_open;
                if is_open { '<' } else { '>' }
            }
            other => other,
        })
        .collect()
}

fn attribute_rows(attributes: &[Attribute], style: &EntityStyle) -> Vec<AttributeRow> {
    let mut rows = Vec::new();
    let mut offset = 0.0;
    for attribute in attributes {
        let texts = [
            generic_type_text(&attribute.attribute_type),
            generic_type_text(&attribute.name),
            attribute.keys.join(","),
            attribute.comment.clone(),
        ];
        let cells = texts
            .map(|text| measure_single_line(style.fonts, &text, style.mode, FontWeight::Regular));
        let height = cells.iter().map(|cell| cell.height).fold(0.0, f64::max) + TEXT_PADDING;
        rows.push(AttributeRow {
            cells,
            offset,
            height,
        });
        offset += height;
    }
    rows
}

/// 열마다 가장 넓은 칸 + PADDING. 키·설명 열이 비면 0으로 접고, 이름 줄이 더 넓으면 남는 폭을 나눠 준다
fn column_widths(rows: &[AttributeRow], name: &LabelBox) -> Columns {
    let mut widths = [0.0; 4];
    for row in rows {
        for (column, cell) in row.cells.iter().enumerate() {
            widths[column] = f64::max(widths[column], cell.width + PADDING);
        }
    }
    for width in &mut widths[2..] {
        if *width <= PADDING {
            *width = 0.0;
        }
    }
    let present = 2 + widths[2..].iter().filter(|width| **width > 0.0).count();
    let difference = name.width + PADDING * 2.0 - widths.iter().sum::<f64>();
    if difference > 0.0 {
        for (column, width) in widths.iter_mut().enumerate() {
            if column < 2 || *width > 0.0 {
                *width += difference / present as f64;
            }
        }
    }
    Columns { widths }
}

/// 크기가 정해진 속성 표
struct EntityTable {
    name: LabelBox,
    rows: Vec<AttributeRow>,
    columns: Columns,
    width: f64,
    height: f64,
}

impl EntityTable {
    fn name_height(&self) -> f64 {
        self.name.height + TEXT_PADDING
    }

    /// 바깥 상자 → 줄 상자 → 글자 → 구분선 순서(원본 DOM 순서)
    fn decorations(&self, style: &EntityStyle) -> Vec<SceneItem> {
        let colors = style.colors;
        let (x, y) = (-self.width / 2.0, -self.height / 2.0);
        let mut items: Vec<SceneItem> =
            rough_rect(x, y, self.width, self.height, colors.fill, &colors.border).into();
        for (index, row) in self.rows.iter().enumerate() {
            let is_even = (index + 1) % 2 == 0 && row.offset != 0.0;
            let fill = if is_even {
                colors.row_even
            } else {
                colors.row_odd
            };
            let top = y + self.name_height() + row.offset;
            items.extend(rough_rect(
                x,
                top,
                self.width,
                row.height,
                fill,
                &colors.border,
            ));
        }
        items.extend(self.text_items(style));
        items.extend(self.dividers(&colors.border));
        items
    }

    fn text_items(&self, style: &EntityStyle) -> Vec<SceneItem> {
        let (x, y) = (-self.width / 2.0, -self.height / 2.0);
        let color = style.colors.text;
        let weight = FontWeight::Regular;
        let name_origin = Point::new(-self.name.width / 2.0, y + TEXT_PADDING / 2.0);
        let mut items = label_items(style.fonts, &self.name, name_origin, color, weight);
        for row in &self.rows {
            let top = row.offset + y + self.name_height() + TEXT_PADDING / 2.0;
            for (column, cell) in row.cells.iter().enumerate() {
                let left = x + PADDING / 2.0 + self.columns.offset(column);
                let origin = Point::new(left, top);
                if cell.height == 0.0 && style.mode == LabelMode::SvgText {
                    items.push(empty_text_extent(origin));
                }
                items.extend(label_items(style.fonts, cell, origin, color, weight));
            }
        }
        items
    }

    /// 이름 줄 아래 가로선(원본은 두 번 그린다)과 열 사이 세로선
    fn dividers(&self, border: &StrokeStyle) -> Vec<SceneItem> {
        let (x, y) = (-self.width / 2.0, -self.height / 2.0);
        let top = y + self.name_height();
        let bottom = y + self.height;
        let horizontal = rough_line(Point::new(x, top), Point::new(x + self.width, top), border);
        let mut items = vec![horizontal.clone()];
        for column in 1..4 {
            let is_present = column == 1 || self.columns.widths[column] > 0.0;
            if is_present {
                let divider_x = x + self.columns.offset(column);
                let line = rough_line(
                    Point::new(divider_x, top),
                    Point::new(divider_x, bottom),
                    border,
                );
                items.push(line);
            }
        }
        items.push(horizontal);
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 제네릭_타입은_꺾쇠로_바꾼다() {
        assert_eq!(generic_type_text("List~int~"), "List<int>");
    }
}
