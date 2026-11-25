//! eventmodeling 다이어그램(eventmodeling renderer.ts).
//!
//! 틀을 차례로 놓아 수영 레인 안에 상자를 비스듬히 이어 가고, 앞 틀(또는 원천 틀) 상자에서 화살표를 긋는다.
//! 상자 글자는 foreignObject 안 HTML(굵은 16px 검정 이름을 가운데, 데이터는 빈 줄 아래 16px 고정폭 `<code>`
//! 블록을 왼쪽에)이고 레인 이름은 SVG 굵은 글자다. 상자 크기는 [`content`]가 정한다. 그림 범위는 레인 전체에
//! 사방 30px 여백을 더한 viewBox다.

mod content;
mod layout;
mod parser;

use crate::scene::{
    Color, PathCommand, Point, Scene, StrokeStyle, TextItem, polygon, rounded_rect,
};
use crate::text::{FontBook, FontWeight, snap_layout_unit};
use crate::theme::{Theme, hsl};
use crate::{DiagramError, RenderContext, directive};

use content::{BoxContent, TEXT_MAX_WIDTH};
use layout::{ModelLayout, SWIMLANE_PADDING};
use parser::EntityType;

const FONT_SIZE: f64 = 16.0;
const VIEWBOX_PADDING: f64 = 30.0;
const CORNER_RADIUS: f64 = 3.0;
const BOX_PADDING: f64 = 10.0;
const BOX_TEXT_COLOR: Color = Color::rgb(0, 0, 0);
/// 16px DejaVu Sans Mono 줄 높이(ascent 15 + descent 4)와 ascent
const CODE_LINE_HEIGHT: f64 = 19.0;
const CODE_ASCENT: f64 = 15.0;

/// 레인·관계선·요소 종류별 상자 색(테마 변수 `em*`)
struct ModelColors {
    lane_fill: Color,
    lane_stroke: Color,
    relation: Color,
    lane_text: Color,
}

impl ModelColors {
    fn for_theme(theme: Theme, lane_text: Color) -> Self {
        match theme {
            Theme::Light => Self {
                lane_fill: Color::rgb(250, 250, 250),
                lane_stroke: Color::rgb(240, 240, 240),
                relation: Color::rgb(0x33, 0x33, 0x33),
                lane_text,
            },
            Theme::Dark => Self {
                lane_fill: hsl(0.0, 0.0, 25.0),
                lane_stroke: hsl(0.0, 0.0, 32.0),
                relation: Color::rgb(211, 211, 211),
                lane_text,
            },
        }
    }
}

/// (채움, 테두리)
fn entity_colors(theme: Theme, entity_type: EntityType) -> (Color, Color) {
    let hex = |value: u32| {
        let [_, red, green, blue] = value.to_be_bytes();
        Color::rgb(red, green, blue)
    };
    match (theme, entity_type) {
        (Theme::Light, EntityType::Ui) => (hex(0xFFFFFF), hex(0xDBDADA)),
        (Theme::Light, EntityType::Processor) => (hex(0xEDB3F6), hex(0xB88CBF)),
        (Theme::Light, EntityType::ReadModel) => (hex(0xD3F1A2), hex(0xA3B732)),
        (Theme::Light, EntityType::Command) => (hex(0xBCD6FE), hex(0x679AC3)),
        (Theme::Light, EntityType::Event) => (hex(0xFFB778), hex(0xC19A0F)),
        (Theme::Dark, EntityType::Ui) => (hex(0x2D2D2D), hex(0x555555)),
        (Theme::Dark, EntityType::Processor) => (
            hsl(296.129_032_258_1, 20.261_437_908_5, 40.0),
            hex(0x8A6D8C),
        ),
        (Theme::Dark, EntityType::ReadModel) => (
            hsl(98.666_666_666_7, 33.333_333_333_3, 36.470_588_235_3),
            hex(0x6D8C5C),
        ),
        (Theme::Dark, EntityType::Command) => (
            hsl(218.666_666_666_7, 33.333_333_333_3, 36.470_588_235_3),
            hex(0x5C6D8C),
        ),
        (Theme::Dark, EntityType::Event) => {
            (hsl(32.0, 33.333_333_333_3, 36.470_588_235_3), hex(0x8C755C))
        }
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let model = parser::parse(&directive::diagram_body(definition))?;
    let contents: Vec<BoxContent> = model
        .frames
        .iter()
        .map(|frame| content::box_content(context.fonts, frame, &model.data_blocks))
        .collect();
    let sizes: Vec<(f64, f64)> = contents
        .iter()
        .map(|content| (content.width, content.height))
        .collect();
    let layout = ModelLayout::new(&model.frames, &sizes);
    let theme = context.palette.theme;
    let colors = ModelColors::for_theme(theme, context.palette.text);
    let lane_width = layout.max_right + SWIMLANE_PADDING;
    let mut scene = Scene {
        width: lane_width + VIEWBOX_PADDING * 2.0,
        height: layout.bottom() + VIEWBOX_PADDING * 2.0,
        ..Scene::default()
    };
    draw_swimlanes(&mut scene, &layout, &colors, lane_width);
    for (frame_box, content) in layout.boxes.iter().zip(&contents) {
        draw_box(&mut scene, &layout, frame_box, theme);
        let top = layout.swimlanes[frame_box.swimlane].y + SWIMLANE_PADDING + BOX_PADDING;
        let area = ContentArea {
            left: frame_box.x + BOX_PADDING,
            top,
            width: frame_box.width - 2.0 * BOX_PADDING,
            height: frame_box.height - 2.0 * BOX_PADDING,
        };
        draw_box_text(&mut scene, context.fonts, &area, content);
    }
    for (source, target) in &layout.relations {
        draw_relation(&mut scene, &layout, *source, *target, colors.relation);
    }
    scene.translate(VIEWBOX_PADDING, VIEWBOX_PADDING);
    Ok(scene)
}

fn draw_swimlanes(scene: &mut Scene, layout: &ModelLayout, colors: &ModelColors, width: f64) {
    for lane in &layout.swimlanes {
        let stroke = StrokeStyle {
            color: colors.lane_stroke,
            width: 1.0,
            dash: Vec::new(),
        };
        let outline = rounded_rect(0.0, lane.y, width, lane.height, CORNER_RADIUS);
        scene.push_path(outline, Some(colors.lane_fill), Some(stroke));
        scene.push_text(TextItem {
            text: lane.label.clone(),
            x: 30.0,
            baseline: lane.y + 30.0,
            font_size: FONT_SIZE,
            weight: FontWeight::Bold,
            color: colors.lane_text,
            rotation: None,
        });
    }
}

fn draw_box(scene: &mut Scene, layout: &ModelLayout, frame_box: &layout::FrameBox, theme: Theme) {
    let top = layout.swimlanes[frame_box.swimlane].y + SWIMLANE_PADDING;
    let (fill, stroke) = entity_colors(theme, frame_box.entity_type);
    let outline = rounded_rect(
        frame_box.x,
        top,
        frame_box.width,
        frame_box.height,
        CORNER_RADIUS,
    );
    let stroke = StrokeStyle {
        color: stroke,
        width: 1.0,
        dash: Vec::new(),
    };
    scene.push_path(outline, Some(fill), Some(stroke));
}

/// foreignObject 안쪽 영역(상자에서 사방 10px 안)
struct ContentArea {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// 표 칸(`vertical-align: middle`) 가운데에 이름 줄(22px), 데이터가 있으면 빈 줄(22px)과 `<code>` 줄(19px)들
fn draw_box_text(scene: &mut Scene, fonts: &FontBook, area: &ContentArea, content: &BoxContent) {
    let metrics = fonts.primary_metrics(FONT_SIZE);
    let line_height = metrics.ascent + metrics.descent;
    let code_lines: Vec<String> = content
        .code_html
        .as_deref()
        .map(content::code_lines)
        .unwrap_or_default()
        .iter()
        .flat_map(|line| wrap_code_line(fonts, line, area.width.min(TEXT_MAX_WIDTH)))
        .collect();
    let code_height = match content.code_html {
        Some(_) => line_height + code_lines.len() as f64 * CODE_LINE_HEIGHT,
        None => 0.0,
    };
    let total_height = line_height + code_height;
    let line_top = area.top + snap_layout_unit(((area.height - total_height) / 2.0).max(0.0));
    let name_width = fonts.measure_line(&content.name, FONT_SIZE, FontWeight::Bold);
    scene.push_text(box_text(
        &content.name,
        area.left + snap_layout_unit((area.width - name_width) / 2.0),
        line_top + metrics.ascent,
        FontWeight::Bold,
    ));
    let code_top = line_top + 2.0 * line_height;
    for (index, line) in code_lines.into_iter().enumerate() {
        let baseline = code_top + index as f64 * CODE_LINE_HEIGHT + CODE_ASCENT;
        scene.push_text(box_text(&line, area.left, baseline, FontWeight::Monospace));
    }
}

fn box_text(text: &str, x: f64, baseline: f64, weight: FontWeight) -> TextItem {
    TextItem {
        text: text.to_string(),
        x,
        baseline,
        font_size: FONT_SIZE,
        weight,
        color: BOX_TEXT_COLOR,
        rotation: None,
    }
}

/// `<code>` 블록 폭 안에서 보통 공백(nbsp 제외)마다 줄을 바꿀 수 있다
fn wrap_code_line(fonts: &FontBook, line: &str, width: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in line.split(' ') {
        let candidate = match current.is_empty() {
            true => word.to_string(),
            false => format!("{current} {word}"),
        };
        let is_overflowing =
            fonts.measure_line(&candidate, FONT_SIZE, FontWeight::Monospace) > width;
        if is_overflowing && !current.is_empty() {
            lines.push(std::mem::replace(&mut current, word.to_string()));
        } else {
            current = candidate;
        }
    }
    lines.push(current);
    lines
}

/// 관계선: 출발 상자 2/3 지점에서 도착 상자 1/3 지점까지. 위로 가면 출발 위 끝에서 도착 아래 끝으로 긋는다
fn draw_relation(
    scene: &mut Scene,
    layout: &ModelLayout,
    source: usize,
    target: usize,
    color: Color,
) {
    let (from_box, to_box) = (&layout.boxes[source], &layout.boxes[target]);
    let from_top = layout.swimlanes[from_box.swimlane].y + SWIMLANE_PADDING;
    let to_top = layout.swimlanes[to_box.swimlane].y + SWIMLANE_PADDING;
    let is_upwards = from_top > to_top;
    let from = Point::new(
        from_box.x + from_box.width * 2.0 / 3.0,
        if is_upwards {
            from_top
        } else {
            from_top + from_box.height
        },
    );
    let to = Point::new(
        to_box.x + to_box.width / 3.0,
        if is_upwards {
            to_top + to_box.height
        } else {
            to_top
        },
    );
    let stroke = StrokeStyle {
        color,
        width: 1.0,
        dash: Vec::new(),
    };
    scene.push_path(
        vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)],
        None,
        Some(stroke),
    );
    draw_arrowhead(scene, from, to, color);
}

/// 관계선 끝 화살촉: markerWidth 10·markerHeight 7, refX 10·refY 3.5 삼각형(선 굵기 1배)
fn draw_arrowhead(scene: &mut Scene, from: Point, to: Point, color: Color) {
    let length = (to.x - from.x).hypot(to.y - from.y);
    if length == 0.0 {
        return;
    }
    let (unit_x, unit_y) = ((to.x - from.x) / length, (to.y - from.y) / length);
    let corner = |along: f64, across: f64| {
        Point::new(
            to.x + along * unit_x - across * unit_y,
            to.y + along * unit_y + across * unit_x,
        )
    };
    let arrow = [corner(-10.0, -3.5), corner(0.0, 0.0), corner(-10.0, 3.5)];
    scene.push_path(polygon(&arrow), Some(color), None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    #[test]
    fn 상자_글자_폭은_b_태그까지_잰다() {
        let fonts = bundled_fonts();
        let model = parser::parse("eventmodeling\ntf 01 ui AddItemScreen").unwrap();
        let content = content::box_content(&fonts, &model.frames[0], &[]);
        // eventmodeling-basic 기준 SVG: AddItemScreen 상자 폭 227 = 글자 187 + 20 + 20
        assert_eq!((content.width, content.height), (187.0, 22.0));
    }
}
