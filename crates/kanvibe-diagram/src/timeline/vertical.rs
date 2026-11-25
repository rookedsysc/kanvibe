//! timeline 세로 렌더러(`timeline TD`, timelineRendererVertical.ts). 가운데 세로 축 왼쪽에 기간 상자를, 오른쪽에
//! 사건 상자를 위에서 아래로 쌓고, 사건마다 축에서 상자까지 점선을 긋는다. 구역이 있으면 구역 상자가 그 구역의
//! 기간들 위를 가로지른다. 선들은 없는 표식(`url(#arrowhead)`)을 가리켜 화살촉이 그려지지 않는다.

use super::colors::SectionColors;
use super::parser::{Period, Timeline};
use super::{
    Bounds, LEFT_MARGIN, MEASURED_PERIOD_TEXT, NodeFrame, TOP, TimelineCanvas, draw_node,
    draw_title, finish_scene, node_content_height, section_number, stroke,
};
use crate::RenderContext;
use crate::scene::{PathCommand, PathItem, Point, Scene, SceneItem};
use crate::text::FontBook;

/// 기간 상자 글자 폭(`NODE_WIDTH`)과 상자 안쪽 여백
const TASK_TEXT_WIDTH: f64 = 200.0;
const NODE_PADDING: f64 = 5.0;
const TASK_TOTAL_WIDTH: f64 = TASK_TEXT_WIDTH + 2.0 * NODE_PADDING;
const EVENT_TEXT_WIDTH: f64 = TASK_TEXT_WIDTH + 100.0;
const EVENT_TOTAL_WIDTH: f64 = EVENT_TEXT_WIDTH + 2.0 * NODE_PADDING;
const EVENT_SPACING: f64 = 10.0;
const SECTION_TASK_GAP: f64 = 20.0;
const TASK_AXIS_GAP: f64 = 20.0;
const TASK_VERTICAL_GAP: f64 = 30.0;
const EVENT_AXIS_GAP: f64 = 50.0;
/// 축 왼쪽 칸(기간 상자 + 간격)과 오른쪽 칸(사건 상자 + 간격)
const LEFT_WIDTH: f64 = TASK_TOTAL_WIDTH + TASK_AXIS_GAP;
const RIGHT_WIDTH: f64 = EVENT_TOTAL_WIDTH + EVENT_AXIS_GAP;
const MASTER_X: f64 = 50.0 + LEFT_MARGIN;
/// 구역이 있든 없든 축은 `masterX + leftWidth`에 온다
const AXIS_X: f64 = MASTER_X + LEFT_WIDTH;
const SECTION_TEXT_WIDTH: f64 = LEFT_WIDTH + RIGHT_WIDTH - NODE_PADDING * 2.0;
/// 축은 내용 위 2em에서 시작해 경계 아래 0.5em + 20까지 내려간다(글자 크기 16)
const AXIS_TOP_OFFSET: f64 = 32.0;
const AXIS_BOTTOM_PADDING: f64 = 28.0;

/// 미리 잰 높이들(draw의 첫 두 반복)
#[derive(Clone, Copy, Debug)]
struct VerticalHeights {
    section: f64,
    task: f64,
    /// 기간 한 칸의 세로 간격: 기간 높이와 사건 묶음 높이 중 큰 값 + 30
    task_spacing: f64,
}

pub(super) fn render(timeline: &Timeline, context: &RenderContext) -> Scene {
    let fonts = context.fonts;
    let heights = measure(fonts, timeline);
    let mut canvas = TimelineCanvas {
        fonts,
        colors: super::colors::TimelineColors::for_theme(context.palette.theme),
        scene: Scene::default(),
        bounds: Bounds::EMPTY,
    };
    if timeline.sections.is_empty() {
        draw_tasks(&mut canvas, &timeline.periods, (0, true), TOP, heights);
    } else {
        draw_sections(&mut canvas, timeline, heights);
    }
    if !timeline.title.is_empty() {
        let x = canvas.bounds.width() / 2.0 - LEFT_MARGIN;
        draw_title(&mut canvas, &timeline.title, x);
    }
    draw_axis(&mut canvas);
    finish_scene(canvas)
}

fn measure(fonts: &FontBook, timeline: &Timeline) -> VerticalHeights {
    let height = |text: &str, width: f64| node_content_height(fonts, text, width, NODE_PADDING);
    let section = timeline
        .sections
        .iter()
        .map(|name| height(name, SECTION_TEXT_WIDTH))
        .fold(0.0, f64::max);
    // TS는 기간 이름 대신 작업 객체를 글자로 넣어 잰다
    let task = if timeline.periods.is_empty() {
        0.0
    } else {
        height(MEASURED_PERIOD_TEXT, TASK_TEXT_WIDTH)
    };
    let event_stack = timeline
        .periods
        .iter()
        .map(|period| {
            let events: f64 = period
                .events
                .iter()
                .map(|event| height(event, EVENT_TEXT_WIDTH))
                .sum();
            events + period.events.len().saturating_sub(1) as f64 * EVENT_SPACING
        })
        .fold(0.0, f64::max);
    VerticalHeights {
        section,
        task,
        task_spacing: task.max(event_stack) + TASK_VERTICAL_GAP,
    }
}

fn draw_sections(canvas: &mut TimelineCanvas, timeline: &Timeline, heights: VerticalHeights) {
    let mut master_y = TOP;
    for (number, section) in timeline.sections.iter().enumerate() {
        let section_periods: Vec<Period> = timeline
            .periods
            .iter()
            .filter(|period| &period.section == section)
            .cloned()
            .collect();
        let colors = canvas.colors.section(section_number(number));
        let frame = NodeFrame {
            origin: Point::new(AXIS_X - LEFT_WIDTH, master_y),
            text_width: SECTION_TEXT_WIDTH,
            padding: NODE_PADDING,
        };
        let section_height = draw_node(canvas, section, frame, heights.section, colors);
        let task_start_y = master_y + section_height + SECTION_TASK_GAP;
        draw_tasks(
            canvas,
            &section_periods,
            (number, false),
            task_start_y,
            heights,
        );
        let task_count = section_periods.len();
        let trimmed_gap = if task_count > 0 {
            TASK_VERTICAL_GAP * 2.0
        } else {
            0.0
        };
        master_y +=
            section_height + SECTION_TASK_GAP + heights.task_spacing * task_count.max(1) as f64
                - trimmed_gap;
    }
}

/// 기간 상자를 축 왼쪽에 쌓고 사건을 오른쪽에 둔다. 구역이 없으면 기간마다 색을 바꾼다
fn draw_tasks(
    canvas: &mut TimelineCanvas,
    periods: &[Period],
    (mut color_index, is_without_sections): (usize, bool),
    start_y: f64,
    heights: VerticalHeights,
) {
    let mut task_min_height = heights.task;
    let mut y = start_y;
    for period in periods {
        let colors = canvas.colors.section(section_number(color_index));
        let frame = NodeFrame {
            origin: Point::new(AXIS_X - TASK_AXIS_GAP - TASK_TOTAL_WIDTH, y),
            text_width: TASK_TEXT_WIDTH,
            padding: NODE_PADDING,
        };
        let task_height = draw_node(canvas, &period.name, frame, task_min_height, colors);
        task_min_height = task_min_height.max(task_height);
        draw_events(canvas, &period.events, colors.brightened(), y);
        y += heights.task_spacing;
        if is_without_sections {
            color_index += 1;
        }
    }
}

/// 사건 상자(`.eventWrapper` 밝기 120%)와 축에서 상자 가운데까지 이어지는 점선
fn draw_events(canvas: &mut TimelineCanvas, events: &[String], colors: SectionColors, top: f64) {
    let events_x = AXIS_X + EVENT_AXIS_GAP;
    let mut y = top;
    for event in events {
        let frame = NodeFrame {
            origin: Point::new(events_x, y),
            text_width: EVENT_TEXT_WIDTH,
            padding: NODE_PADDING,
        };
        let event_height = draw_node(canvas, event, frame, 0.0, colors);
        let line_y = y + event_height / 2.0;
        let line = vec![
            PathCommand::MoveTo(Point::new(AXIS_X, line_y)),
            PathCommand::LineTo(Point::new(events_x, line_y)),
        ];
        let color = canvas.colors.connector;
        canvas
            .scene
            .push_path(line, None, stroke(color, 2.0, vec![5.0, 5.0]));
        canvas.bounds.include(AXIS_X, line_y, events_x, line_y);
        y += event_height + EVENT_SPACING;
    }
}

/// 세로 축(굵기 4). `lower()`로 맨 앞에 넣어 다른 그림 아래에 깔린다
fn draw_axis(canvas: &mut TimelineCanvas) {
    let top = TOP - AXIS_TOP_OFFSET;
    let bottom = canvas.bounds.bottom + AXIS_BOTTOM_PADDING;
    let axis = PathItem {
        commands: vec![
            PathCommand::MoveTo(Point::new(AXIS_X, top)),
            PathCommand::LineTo(Point::new(AXIS_X, bottom)),
        ],
        fill: None,
        stroke: stroke(canvas.colors.connector, 4.0, Vec::new()),
    };
    canvas.scene.items.insert(0, SceneItem::Path(axis));
    canvas.bounds.include(AXIS_X, top, AXIS_X, bottom);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;

    #[test]
    fn 기준_svg와_같은_간격으로_쌓는다() {
        // 기준 SVG timeline-td: 상자 높이 35.8, 사건 2개 묶음 81.6 → 기간 간격 111.6
        let fonts = bundled_fonts();
        let timeline = super::super::parser::parse(
            "timeline TD\nsection 2023\nQ1 : Alpha : Internal demo\nQ3 : Beta\n",
        )
        .unwrap();
        let heights = measure(&fonts, &timeline);
        assert!((heights.section - 35.8).abs() < 1e-9);
        assert!((heights.task_spacing - 111.6).abs() < 1e-9);
        assert_eq!(AXIS_X, 430.0);
        assert_eq!(SECTION_TEXT_WIDTH, 580.0);
    }
}
