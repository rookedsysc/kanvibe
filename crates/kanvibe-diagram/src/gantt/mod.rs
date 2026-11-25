//! gantt 다이어그램(ganttRenderer.js). 작업마다 한 줄(막대 높이 20, 간격 4)을 쓰고, 가로는 d3 시간 축이
//! 컨테이너 폭에서 좌우 여백 75를 뺀 만큼을 차지한다. 모든 글자는 SVG `<text>`라 라벨 방식과 상관없이 같다.

mod axis;
mod calendar;
mod colors;
mod date_format;
mod parser;
mod schedule;

use crate::scene::{
    Color, PathCommand, Point, Scene, StrokeStyle, TextItem, polygon, rounded_rect,
};
use crate::text::{FontBook, FontWeight};
use crate::{DiagramError, RenderContext};
use axis::{TickInterval, TimeUnit};
use colors::{BarColors, GanttColors};
use parser::GanttDocument;
use schedule::ScheduledTask;

/// 그림 폭. TS는 컨테이너 폭(`parentElement.offsetWidth`)을 쓰고, 기준 이미지의 모바일 워커는 800 − 16이다
const DIAGRAM_WIDTH: f64 = 784.0;
const TITLE_TOP_MARGIN: f64 = 25.0;
const BAR_HEIGHT: f64 = 20.0;
const BAR_GAP: f64 = 4.0;
const ROW_HEIGHT: f64 = BAR_HEIGHT + BAR_GAP;
const TOP_PADDING: f64 = 50.0;
const SIDE_PADDING: f64 = 75.0;
const GRID_LINE_START_PADDING: f64 = 35.0;
const FONT_SIZE: f64 = 11.0;
const TICK_FONT_SIZE: f64 = 10.0;
const TITLE_FONT_SIZE: f64 = 18.0;
const SECTION_STYLE_COUNT: usize = 4;
const BAR_RADIUS: f64 = 3.0;
const BAR_STROKE_WIDTH: f64 = 2.0;
/// d3 축 눈금은 기기 픽셀 비 1에서 0.5px 옮겨 선명하게 그린다
const TICK_OFFSET: f64 = 0.5;
/// d3 `tickPadding`
const TICK_PADDING: f64 = 3.0;
/// `.milestone{transform: rotate(45deg) scale(0.8)}`
const MILESTONE_SCALE: f64 = 0.8;

/// `scaleTime().domain([최소 시작, 최대 끝]).rangeRound([0, 폭])`
#[derive(Clone, Copy, Debug)]
struct TimeScale {
    start: i64,
    stop: i64,
    range: f64,
}

impl TimeScale {
    fn position(&self, millis: i64) -> f64 {
        let ratio = if self.stop == self.start {
            0.5
        } else {
            (millis - self.start) as f64 / (self.stop - self.start) as f64
        };
        round_half_up(self.range * ratio)
    }
}

/// JS `Math.round`
fn round_half_up(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// 그리는 동안 함께 쓰는 값
struct GanttLayout<'a> {
    fonts: &'a FontBook,
    document: &'a GanttDocument,
    colors: GanttColors,
    /// 작업에 처음 나온 순서의 구역 이름
    categories: Vec<String>,
    height: f64,
    scale: TimeScale,
}

impl GanttLayout<'_> {
    fn section_index(&self, section: &str) -> usize {
        let position = self.categories.iter().position(|name| name == section);
        position.unwrap_or(0) % SECTION_STYLE_COUNT
    }

    fn bar_x(&self, millis: i64) -> f64 {
        self.scale.position(millis) + SIDE_PADDING
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let document = parser::parse(definition)?;
    let mut tasks = schedule::schedule(&document)?;
    let categories = collect_categories(&tasks);
    let height = 2.0 * TOP_PADDING + tasks.len() as f64 * ROW_HEIGHT;
    let scale = TimeScale {
        start: tasks.iter().map(|task| task.start).min().unwrap_or(0),
        stop: tasks.iter().map(|task| task.end).max().unwrap_or(0),
        range: DIAGRAM_WIDTH - 2.0 * SIDE_PADDING,
    };
    tasks.sort_by_key(|task| task.start);
    let layout = GanttLayout {
        fonts: context.fonts,
        document: &document,
        colors: GanttColors::for_theme(context.palette.theme),
        categories,
        height,
        scale,
    };
    let mut scene = Scene {
        is_start_aligned: false,
        width: DIAGRAM_WIDTH,
        height,
        items: Vec::new(),
    };
    draw_excluded_days(&mut scene, &layout, &tasks);
    draw_axes(&mut scene, &layout);
    draw_section_rows(&mut scene, &layout, &tasks);
    tasks
        .iter()
        .for_each(|task| draw_bar(&mut scene, &layout, task));
    tasks
        .iter()
        .for_each(|task| draw_task_label(&mut scene, &layout, task));
    draw_section_titles(&mut scene, &layout, &tasks);
    draw_today_marker(&mut scene, &layout);
    let title = document.title.clone();
    let title_color = layout.colors.title;
    let title_item = centered_text(
        context.fonts,
        title,
        DIAGRAM_WIDTH / 2.0,
        TITLE_TOP_MARGIN,
        TITLE_FONT_SIZE,
        title_color,
    );
    scene.push_text(title_item);
    Ok(scene)
}

/// 작업에 처음 나온 순서의 구역 이름(ganttRenderer `checkUnique`)
fn collect_categories(tasks: &[ScheduledTask]) -> Vec<String> {
    let mut categories: Vec<String> = Vec::new();
    for task in tasks {
        if !categories.contains(&task.section) {
            categories.push(task.section.clone());
        }
    }
    categories
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Vec<PathCommand> {
    polygon(&[
        Point::new(x, y),
        Point::new(x + width, y),
        Point::new(x + width, y + height),
        Point::new(x, y + height),
    ])
}

fn plain_text(text: String, x: f64, baseline: f64, font_size: f64, color: Color) -> TextItem {
    TextItem {
        text,
        x,
        baseline,
        font_size,
        weight: FontWeight::Regular,
        color,
        rotation: None,
    }
}

fn centered_text(
    fonts: &FontBook,
    text: String,
    center_x: f64,
    baseline: f64,
    font_size: f64,
    color: Color,
) -> TextItem {
    let width = fonts.measure_line(&text, font_size, FontWeight::Regular);
    plain_text(text, center_x - width / 2.0, baseline, font_size, color)
}

/// SVG `<text>`는 이어진 공백을 하나로 줄이고 앞뒤 공백을 지운다
fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `excludes`로 뺀 날들을 이어진 구간마다 회색 띠로 칠한다(drawExcludeDays)
fn draw_excluded_days(scene: &mut Scene, layout: &GanttLayout, tasks: &[ScheduledTask]) {
    let document = layout.document;
    if document.excludes.is_empty() && document.includes.is_empty() || tasks.is_empty() {
        return;
    }
    let first = tasks.iter().map(|task| task.start).min().unwrap_or(0);
    let last = tasks.iter().map(|task| task.end).max().unwrap_or(0);
    let is_over_five_years = last - first > 5 * 366 * calendar::MILLIS_PER_DAY;
    if is_over_five_years {
        return;
    }
    let mut ranges: Vec<(i64, i64)> = Vec::new();
    let mut open_range: Option<(i64, i64)> = None;
    let mut day = first;
    while day <= last {
        if schedule::is_excluded_date(document, day) {
            open_range = Some((open_range.map_or(day, |(start, _)| start), day));
        } else if let Some(range) = open_range.take() {
            ranges.push(range);
        }
        day += calendar::MILLIS_PER_DAY;
    }
    for (start, end) in ranges {
        let x = layout.bar_x(calendar::start_of_day(start));
        let end_of_day = calendar::start_of_day(end) + calendar::MILLIS_PER_DAY - 1;
        let width = layout.scale.position(end_of_day)
            - layout.scale.position(calendar::start_of_day(start));
        let height = layout.height - TOP_PADDING - GRID_LINE_START_PADDING;
        let commands = rect(x, GRID_LINE_START_PADDING, width, height);
        scene.push_path(commands, Some(layout.colors.exclude), None);
    }
}

/// `tickInterval` 설정을 d3 간격으로 바꾼다. 눈금이 너무 많아지면 기본 눈금을 쓴다
fn custom_tick_interval(layout: &GanttLayout) -> Option<TickInterval> {
    let specifier = layout.document.tick_interval.as_deref()?;
    let digits_end = specifier.find(|character: char| !character.is_ascii_digit())?;
    let (count, unit_name) = specifier.split_at(digits_end);
    let step: i64 = count
        .parse()
        .ok()
        .filter(|step| *step > 0 && !count.starts_with('0'))?;
    let unit = match unit_name {
        "millisecond" => TimeUnit::Millisecond,
        "second" => TimeUnit::Second,
        "minute" => TimeUnit::Minute,
        "hour" => TimeUnit::Hour,
        "day" => TimeUnit::Day,
        "week" => TimeUnit::Week(layout.document.week_start),
        "month" => TimeUnit::Month,
        _ => return None,
    };
    let interval = TickInterval { unit, step };
    let estimated = axis::estimated_tick_count(layout.scale.start, layout.scale.stop, interval);
    (estimated <= axis::MAX_TICK_COUNT).then_some(interval)
}

fn draw_axes(scene: &mut Scene, layout: &GanttLayout) {
    let document = layout.document;
    let axis_format = match (&document.axis_format, document.date_format.as_str()) {
        (Some(format), _) => format.clone(),
        (None, "D") => "%d".to_string(),
        (None, _) => "%Y-%m-%d".to_string(),
    };
    let interval = custom_tick_interval(layout);
    let ticks = axis::ticks(layout.scale.start, layout.scale.stop, interval);
    let tick_length = layout.height - TOP_PADDING - GRID_LINE_START_PADDING;
    let bottom = layout.height - 50.0;
    let bottom_text = TICK_PADDING + TICK_FONT_SIZE;
    draw_axis(
        scene,
        layout,
        &ticks,
        &axis_format,
        (bottom, -tick_length, bottom_text),
    );
    if document.has_top_axis {
        draw_axis(
            scene,
            layout,
            &ticks,
            &axis_format,
            (TOP_PADDING, tick_length, -TICK_PADDING),
        );
    }
}

/// 축 하나. `(축 y, 눈금선 길이(아래가 +), 글자 기준선 y)`
fn draw_axis(
    scene: &mut Scene,
    layout: &GanttLayout,
    ticks: &[i64],
    axis_format: &str,
    (axis_y, tick_length, text_y): (f64, f64, f64),
) {
    let line_style = StrokeStyle {
        color: layout.colors.tick_line,
        width: 1.0,
        dash: Vec::new(),
    };
    for tick in ticks {
        let x = layout.bar_x(*tick) + TICK_OFFSET;
        let line = vec![
            PathCommand::MoveTo(Point::new(x, axis_y)),
            PathCommand::LineTo(Point::new(x, axis_y + tick_length)),
        ];
        scene.push_path(line, None, Some(line_style.clone()));
        let label = axis::format_time(*tick, axis_format);
        let color = layout.colors.tick_text;
        scene.push_text(centered_text(
            layout.fonts,
            label,
            x,
            axis_y + text_y,
            TICK_FONT_SIZE,
            color,
        ));
    }
}

/// 작업 줄마다 구역 색 띠(폭은 그림 폭 − 오른쪽 여백 절반)
fn draw_section_rows(scene: &mut Scene, layout: &GanttLayout, tasks: &[ScheduledTask]) {
    let width = DIAGRAM_WIDTH - SIDE_PADDING / 2.0;
    for task in tasks {
        let y = task.order as f64 * ROW_HEIGHT + TOP_PADDING - 2.0;
        let color = layout.colors.section_rows[layout.section_index(&task.section)];
        scene.push_path(rect(0.0, y, width, ROW_HEIGHT), Some(color), None);
    }
}

fn bar_colors(colors: &GanttColors, task: &ScheduledTask) -> BarColors {
    let tags = task.tags;
    match (tags.is_active, tags.is_done, tags.is_crit) {
        (true, _, true) => colors.active_crit,
        (true, _, false) => colors.active,
        (false, true, true) => colors.done_crit,
        (false, true, false) => colors.done,
        (false, false, true) => colors.crit,
        (false, false, false) => colors.task,
    }
}

fn draw_bar(scene: &mut Scene, layout: &GanttLayout, task: &ScheduledTask) {
    let colors = bar_colors(&layout.colors, task);
    let y = task.order as f64 * ROW_HEIGHT + TOP_PADDING;
    let start_x = layout.bar_x(task.start);
    let end_x = layout.bar_x(task.render_end.unwrap_or(task.end));
    let (commands, stroke_width) = if task.tags.is_milestone {
        let center_x = start_x + 0.5 * (layout.bar_x(task.end) - start_x);
        let center = Point::new(center_x, y + BAR_HEIGHT / 2.0);
        let square = rounded_rect(
            center_x - BAR_HEIGHT / 2.0,
            y,
            BAR_HEIGHT,
            BAR_HEIGHT,
            BAR_RADIUS,
        );
        (
            milestone_diamond(&square, center),
            BAR_STROKE_WIDTH * MILESTONE_SCALE,
        )
    } else if end_x > start_x {
        let commands = rounded_rect(start_x, y, end_x - start_x, BAR_HEIGHT, BAR_RADIUS);
        (commands, BAR_STROKE_WIDTH)
    } else {
        return;
    };
    let stroke = StrokeStyle {
        color: colors.stroke,
        width: stroke_width,
        dash: Vec::new(),
    };
    scene.push_path(commands, Some(colors.fill), Some(stroke));
}

/// 마일스톤 사각형을 가운데 둘레로 45° 돌리고 0.8배 줄인다
fn milestone_diamond(commands: &[PathCommand], center: Point) -> Vec<PathCommand> {
    let (sin, cos) = std::f64::consts::FRAC_PI_4.sin_cos();
    let turn = |point: Point| {
        let (dx, dy) = (point.x - center.x, point.y - center.y);
        Point::new(
            center.x + MILESTONE_SCALE * (dx * cos - dy * sin),
            center.y + MILESTONE_SCALE * (dx * sin + dy * cos),
        )
    };
    commands
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo(point) => PathCommand::MoveTo(turn(point)),
            PathCommand::LineTo(point) => PathCommand::LineTo(turn(point)),
            PathCommand::QuadTo(control, point) => PathCommand::QuadTo(turn(control), turn(point)),
            PathCommand::CubicTo(first, second, point) => {
                PathCommand::CubicTo(turn(first), turn(second), turn(point))
            }
            PathCommand::Close => PathCommand::Close,
        })
        .collect()
}

/// 작업 글자가 놓이는 곳
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LabelPlacement {
    Inside,
    OutsideLeft,
    OutsideRight,
}

/// 글자가 막대보다 넓으면 막대 밖으로, 오른쪽에 자리가 없으면 왼쪽으로 낸다
fn label_placement(text_width: f64, start_x: f64, end_x: f64) -> LabelPlacement {
    if text_width <= end_x - start_x {
        LabelPlacement::Inside
    } else if end_x + text_width + 1.5 * SIDE_PADDING > DIAGRAM_WIDTH {
        LabelPlacement::OutsideLeft
    } else {
        LabelPlacement::OutsideRight
    }
}

fn label_color(colors: &GanttColors, task: &ScheduledTask, placement: LabelPlacement) -> Color {
    let tags = task.tags;
    match placement {
        LabelPlacement::Inside if tags.is_active || tags.is_done => colors.text_tagged,
        LabelPlacement::Inside => colors.text_inside,
        _ if tags.is_done => colors.text_done_outside,
        _ if tags.is_active => colors.text_tagged,
        _ => colors.text_outside,
    }
}

/// 작업 이름. 자리(class)는 시작·끝 시각으로, x는 그리는 끝(`renderEndTime`)으로 정하는 TS 계산을 따른다
fn draw_task_label(scene: &mut Scene, layout: &GanttLayout, task: &ScheduledTask) {
    let text = collapse_whitespace(&task.name);
    let fonts = layout.fonts;
    let text_width = fonts
        .measure_svg_runs(&[&text], FONT_SIZE, FontWeight::Regular)
        .width();
    let scale = layout.scale;
    let class_start = scale.position(task.start);
    let class_end = if task.tags.is_milestone {
        class_start + BAR_HEIGHT
    } else {
        scale.position(task.end)
    };
    let placement = label_placement(text_width, class_start, class_end);
    let mut start_x = scale.position(task.start);
    let mut end_x = scale.position(task.render_end.unwrap_or(task.end));
    if task.tags.is_milestone {
        start_x += 0.5 * (scale.position(task.end) - start_x) - 0.5 * BAR_HEIGHT;
        end_x = start_x + BAR_HEIGHT;
    }
    let anchor_x = match label_placement(text_width, start_x, end_x) {
        LabelPlacement::Inside => (end_x - start_x) / 2.0 + start_x + SIDE_PADDING,
        LabelPlacement::OutsideLeft => start_x + SIDE_PADDING - 5.0,
        LabelPlacement::OutsideRight => end_x + SIDE_PADDING + 5.0,
    };
    let advance = fonts.measure_line(&text, FONT_SIZE, FontWeight::Regular);
    let x = match placement {
        LabelPlacement::Inside => anchor_x - advance / 2.0,
        LabelPlacement::OutsideLeft => anchor_x - advance,
        LabelPlacement::OutsideRight => anchor_x,
    };
    let baseline =
        task.order as f64 * ROW_HEIGHT + BAR_HEIGHT / 2.0 + (FONT_SIZE / 2.0 - 2.0) + TOP_PADDING;
    let color = label_color(&layout.colors, task, placement);
    scene.push_text(plain_text(text, x, baseline, FONT_SIZE, color));
}

/// 구역 이름을 그 구역 작업 줄들의 가운데에 `alignment-baseline: central`로 쓴다
fn draw_section_titles(scene: &mut Scene, layout: &GanttLayout, tasks: &[ScheduledTask]) {
    let metrics = layout.fonts.primary_metrics(FONT_SIZE);
    let central_shift = (metrics.ascent - metrics.descent) / 2.0;
    let mut rows_before = 0.0;
    for category in &layout.categories {
        let row_count = tasks
            .iter()
            .filter(|task| &task.section == category)
            .count() as f64;
        let center_y = row_count * ROW_HEIGHT / 2.0 + rows_before * ROW_HEIGHT + TOP_PADDING;
        rows_before += row_count;
        let lines = section_title_lines(category);
        let first_offset = -(lines.len() as f64 - 1.0) / 2.0;
        let color = layout.colors.section_title;
        for (line_index, line) in lines.into_iter().enumerate() {
            let baseline =
                center_y + (first_offset + line_index as f64) * FONT_SIZE + central_shift;
            scene.push_text(plain_text(line, 10.0, baseline, FONT_SIZE, color));
        }
    }
}

/// common `lineBreakRegex`(`<br>`·`<br/>`)로 나눈 줄
fn section_title_lines(category: &str) -> Vec<String> {
    let normalized = category
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n");
    normalized.split('\n').map(collapse_whitespace).collect()
}

/// 오늘 선. `todayMarker off`면 없고, 스타일 글자가 있으면 `stroke`·`stroke-width`·`opacity`를 읽는다
fn draw_today_marker(scene: &mut Scene, layout: &GanttLayout) {
    let marker = layout.document.today_marker.as_str();
    if marker == "off" {
        return;
    }
    let mut style = StrokeStyle {
        color: layout.colors.today,
        width: 2.0,
        dash: Vec::new(),
    };
    for declaration in marker.split(',') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        apply_today_style(&mut style, property.trim(), value.trim());
    }
    let x = layout.bar_x(calendar::now_millis());
    let line = vec![
        PathCommand::MoveTo(Point::new(x, TITLE_TOP_MARGIN)),
        PathCommand::LineTo(Point::new(x, layout.height - TITLE_TOP_MARGIN)),
    ];
    scene.push_path(line, None, Some(style));
}

fn apply_today_style(style: &mut StrokeStyle, property: &str, value: &str) {
    match property {
        "stroke" => style.color = Color::parse_css(value).unwrap_or(style.color),
        "stroke-width" => {
            let width = value.trim_end_matches("px").parse().ok();
            style.width = width.unwrap_or(style.width);
        }
        "opacity" | "stroke-opacity" => {
            let opacity: f32 = value.parse().unwrap_or(1.0);
            style.color.alpha *= opacity.clamp(0.0, 1.0);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;
    use crate::theme::{Palette, Theme};

    fn render_light(definition: &str) -> Scene {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: crate::LabelMode::SvgText,
        };
        render(definition, &context).unwrap()
    }

    #[test]
    fn 작업_수로_높이를_정하고_폭은_컨테이너_폭이다() {
        let scene = render_light(
            "gantt\ndateFormat YYYY-MM-DD\ntodayMarker off\nsection A\nOne :a1, 2024-01-01, 10d\nTwo :after a1, 14d\n",
        );
        assert_eq!(scene.width, 784.0);
        assert_eq!(scene.height, 148.0);
    }

    #[test]
    fn 좁은_막대의_글자는_밖으로_낸다() {
        assert_eq!(
            label_placement(60.0, 0.0, 20.0),
            LabelPlacement::OutsideRight
        );
        assert_eq!(
            label_placement(60.0, 600.0, 620.0),
            LabelPlacement::OutsideLeft
        );
        assert_eq!(label_placement(10.0, 0.0, 20.0), LabelPlacement::Inside);
    }
}
