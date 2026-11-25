//! user journey 다이어그램(journeyRenderer.ts, svgDraw.js). 작업마다 150×50 상자를 200 간격으로 늘어놓고,
//! 그 위에 구역 상자, 아래에 점수 얼굴(5점이 맨 위)을 둔다. 왼쪽에는 참여자 범례가 있다.
//!
//! 상자 글자는 `textPlacement: "fo"`라 언제나 foreignObject 안 HTML(16px, 줄 높이 normal)로 그려지므로
//! 라벨 방식과 상관없이 같다.

mod colors;
mod parser;

use crate::scene::{
    Color, PathCommand, Point, Scene, StrokeStyle, TextItem, ellipse, polygon, rounded_rect,
};
use crate::text::{FontBook, FontWeight, snap_layout_unit};
use crate::{DiagramError, RenderContext};
use colors::JourneyColors;
use parser::JourneyTask;

const LEFT_MARGIN: f64 = 150.0;
const MAX_LABEL_WIDTH: f64 = 360.0;
const BOX_WIDTH: f64 = 150.0;
const BOX_HEIGHT: f64 = 50.0;
const TASK_MARGIN: f64 = 50.0;
const DIAGRAM_MARGIN_X: f64 = 50.0;
const DIAGRAM_MARGIN_Y: f64 = 10.0;
const SECTION_Y: f64 = 50.0;
/// 작업 상자 y(높이 × 2 + 세로 여백)
const TASK_Y: f64 = BOX_HEIGHT * 2.0 + DIAGRAM_MARGIN_Y;
/// 점수 얼굴과 점선이 내려가는 끝(300 + 5 × 30)
const SCORE_BOTTOM: f64 = 450.0;
const FACE_RADIUS: f64 = 15.0;
const ACTOR_RADIUS: f64 = 7.0;
const FONT_SIZE: f64 = 16.0;
/// Noto Sans OS/2 sxHeight(글꼴 단위 1000 기준). 제목 `4ex`의 기준이다
const X_HEIGHT_RATIO: f64 = 0.536;
const TITLE_FONT_SIZE: f64 = 4.0 * X_HEIGHT_RATIO * FONT_SIZE;
/// 제목이 있으면 viewBox를 이만큼 늘린다
const TITLE_EXTRA_HEIGHT: f64 = 70.0;
/// viewBox 위쪽 시작(−25)
const VIEW_TOP: f64 = -25.0;

/// 그리는 동안 함께 쓰는 값
struct JourneyCanvas<'a> {
    fonts: &'a FontBook,
    colors: JourneyColors,
    /// 범례 순서대로 (참여자 이름, 색)
    actors: Vec<(String, Color)>,
    left_margin: f64,
    scene: Scene,
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let journey = parser::parse(definition)?;
    let colors = JourneyColors::for_theme(context.palette.theme);
    let actors = journey
        .actors()
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, colors.actor(index)))
        .collect();
    let mut canvas = JourneyCanvas {
        fonts: context.fonts,
        colors,
        actors,
        left_margin: LEFT_MARGIN,
        scene: Scene::default(),
    };
    let legend_width = draw_actor_legend(&mut canvas);
    canvas.left_margin = LEFT_MARGIN + legend_width;
    draw_tasks(&mut canvas, &journey.tasks);
    let task_count = journey.tasks.len() as f64;
    let last_task_x = (task_count - 1.0) * (TASK_MARGIN + BOX_WIDTH) + canvas.left_margin;
    let content_right = (last_task_x + DIAGRAM_MARGIN_X + TASK_MARGIN).max(canvas.left_margin);
    let width = canvas.left_margin + content_right + 2.0 * DIAGRAM_MARGIN_X;
    let content_bottom = SCORE_BOTTOM.max(canvas.actors.len() as f64 * 50.0);
    let height = content_bottom + 2.0 * DIAGRAM_MARGIN_Y;
    let has_title = !journey.title.is_empty();
    if has_title {
        draw_title(&mut canvas, &journey.title);
    }
    draw_axis_arrow(&mut canvas, width);
    let mut scene = canvas.scene;
    scene.width = width;
    scene.height = height + if has_title { TITLE_EXTRA_HEIGHT } else { 0.0 };
    // journey SVG는 preserveAspectRatio="xMinYMin meet"이다
    scene.is_start_aligned = true;
    scene.translate(0.0, -VIEW_TOP);
    Ok(scene)
}

fn stroke(color: Color, width: f64) -> Option<StrokeStyle> {
    Some(StrokeStyle {
        color,
        width,
        dash: Vec::new(),
    })
}

fn line(from: Point, to: Point) -> Vec<PathCommand> {
    vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)]
}

fn text_item(text: String, x: f64, baseline: f64, color: Color) -> TextItem {
    TextItem {
        text,
        x,
        baseline,
        font_size: FONT_SIZE,
        weight: FontWeight::Regular,
        color,
        rotation: None,
    }
}

/// 왼쪽 범례: 참여자마다 색 원과 이름. 75px보다 넓은 이름이 있으면 그 폭만큼 상자들을 오른쪽으로 민다
fn draw_actor_legend(canvas: &mut JourneyCanvas) -> f64 {
    let mut widest: f64 = 0.0;
    let mut y = 60.0;
    for (name, color) in canvas.actors.clone() {
        let center = Point::new(20.0, y);
        let outline = stroke(Color::rgb(0, 0, 0), 1.0);
        let circle = ellipse(center, ACTOR_RADIUS, ACTOR_RADIUS);
        canvas.scene.push_path(circle, Some(color), outline);
        let lines = legend_lines(canvas.fonts, &name);
        for (index, text) in lines.iter().enumerate() {
            let width = measure_svg(canvas.fonts, text);
            if width > widest && width > LEFT_MARGIN - width {
                widest = width;
            }
            let baseline = y + 7.0 + index as f64 * 20.0;
            let item = text_item(text.clone(), 50.0, baseline, canvas.colors.legend);
            canvas.scene.push_text(item);
        }
        y += (lines.len() as f64 * 20.0).max(20.0);
    }
    widest
}

fn measure_svg(fonts: &FontBook, text: &str) -> f64 {
    fonts
        .measure_svg_runs(&[text], FONT_SIZE, FontWeight::Regular)
        .width()
}

/// 360px을 넘는 이름은 낱말 단위로, 낱말 하나가 넘치면 글자 단위로(`-`를 붙여) 나눈다
fn legend_lines(fonts: &FontBook, name: &str) -> Vec<String> {
    if measure_svg(fonts, name) <= MAX_LABEL_WIDTH {
        return vec![name.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in name.split(' ') {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if measure_svg(fonts, &candidate) <= MAX_LABEL_WIDTH {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        current = break_long_word(fonts, word, &mut lines);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// 넘치는 낱말을 글자 단위로 끊어 앞 조각들을 `lines`에 넣고 남은 조각을 돌려준다
fn break_long_word(fonts: &FontBook, word: &str, lines: &mut Vec<String>) -> String {
    if measure_svg(fonts, word) <= MAX_LABEL_WIDTH {
        return word.to_string();
    }
    let mut piece = String::new();
    for character in word.chars() {
        piece.push(character);
        if measure_svg(fonts, &format!("{piece}-")) > MAX_LABEL_WIDTH {
            piece.pop();
            lines.push(format!("{piece}-"));
            piece = character.to_string();
        }
    }
    piece
}

fn draw_tasks(canvas: &mut JourneyCanvas, tasks: &[JourneyTask]) {
    let mut section_number = 0;
    for (index, task) in tasks.iter().enumerate() {
        let x = index as f64 * (TASK_MARGIN + BOX_WIDTH) + canvas.left_margin;
        let starts_section = index == 0 || tasks[index - 1].section != task.section;
        if starts_section {
            let count = tasks[index..]
                .iter()
                .take_while(|other| other.section == task.section)
                .count() as f64;
            let width = BOX_WIDTH * count + DIAGRAM_MARGIN_X * (count - 1.0);
            let fill = canvas.colors.section_fill(section_number);
            draw_box(canvas, &task.section, (x, SECTION_Y, width), fill);
            section_number += 1;
        }
        let fill = canvas.colors.section_fill(section_number.saturating_sub(1));
        draw_task(canvas, task, x, fill);
    }
}

/// 둥근 상자와 가운데 HTML 글자
fn draw_box(canvas: &mut JourneyCanvas, text: &str, frame: (f64, f64, f64), fill: Color) {
    let (x, y, width) = frame;
    let outline = stroke(canvas.colors.box_stroke, 1.0);
    let commands = rounded_rect(x, y, width, BOX_HEIGHT, 3.0);
    canvas.scene.push_path(commands, Some(fill), outline);
    draw_box_label(canvas, text, frame);
}

/// 상자 가운데의 HTML 글자(display: table-cell, 가운데 정렬, 줄 높이 normal)
fn draw_box_label(canvas: &mut JourneyCanvas, text: &str, (x, y, width): (f64, f64, f64)) {
    let lines = wrap_html_words(canvas.fonts, text, width);
    let metrics = canvas.fonts.primary_metrics(FONT_SIZE);
    let line_height = metrics.ascent + metrics.descent;
    let top = y + (BOX_HEIGHT - lines.len() as f64 * line_height) / 2.0;
    for (index, (line, line_width)) in lines.into_iter().enumerate() {
        let left = x + snap_layout_unit((width - line_width) / 2.0);
        let baseline = top + index as f64 * line_height + metrics.ascent;
        let item = text_item(line, left, baseline, canvas.colors.label);
        canvas.scene.push_text(item);
    }
}

/// HTML 글자 줄바꿈: 낱말을 이어 붙이다 폭을 넘으면 다음 줄로 넘긴다. (줄, 폭)
fn wrap_html_words(fonts: &FontBook, text: &str, width: f64) -> Vec<(String, f64)> {
    let measure = |line: &str| fonts.measure_line(line, FONT_SIZE, FontWeight::Regular);
    let mut lines: Vec<(String, f64)> = Vec::new();
    for word in text.split_whitespace() {
        let Some(last) = lines.last_mut() else {
            lines.push((word.to_string(), measure(word)));
            continue;
        };
        let candidate = format!("{} {word}", last.0);
        let candidate_width = measure(&candidate);
        if candidate_width <= width {
            *last = (candidate, candidate_width);
        } else {
            lines.push((word.to_string(), measure(word)));
        }
    }
    lines
}

/// 점선, 점수 얼굴, 상자, 참여자 원, 글자 순서로 그린다
fn draw_task(canvas: &mut JourneyCanvas, task: &JourneyTask, x: f64, fill: Color) {
    let center_x = x + BOX_WIDTH / 2.0;
    let guide = StrokeStyle {
        color: canvas.colors.line,
        width: 1.0,
        dash: vec![4.0, 2.0],
    };
    let guide_line = line(
        Point::new(center_x, TASK_Y),
        Point::new(center_x, SCORE_BOTTOM),
    );
    canvas.scene.push_path(guide_line, None, Some(guide));
    let face_y = 300.0 + (5.0 - task.score) * 30.0;
    draw_face(canvas, Point::new(center_x, face_y), task.score);
    let outline = stroke(canvas.colors.box_stroke, 1.0);
    let frame = rounded_rect(x, TASK_Y, BOX_WIDTH, BOX_HEIGHT, 3.0);
    canvas.scene.push_path(frame, Some(fill), outline);
    let mut actor_x = x + 14.0;
    for person in &task.people {
        let Some((_, color)) = canvas.actors.iter().find(|(name, _)| name == person) else {
            continue;
        };
        let circle = ellipse(Point::new(actor_x, TASK_Y), ACTOR_RADIUS, ACTOR_RADIUS);
        let outline = stroke(Color::rgb(0, 0, 0), 1.0);
        canvas.scene.push_path(circle, Some(*color), outline);
        actor_x += 10.0;
    }
    draw_box_label(canvas, &task.name, (x, TASK_Y, BOX_WIDTH));
}

/// 점수 얼굴: 3보다 크면 웃고, 작으면 찡그리고, 3이면 일자 입이다
fn draw_face(canvas: &mut JourneyCanvas, center: Point, score: f64) {
    let colors = &canvas.colors;
    let face = ellipse(center, FACE_RADIUS, FACE_RADIUS);
    let face_stroke = stroke(colors.face_stroke, 2.0);
    canvas
        .scene
        .push_path(face, Some(colors.face_fill), face_stroke);
    let eye_offset = FACE_RADIUS / 3.0;
    for eye_x in [center.x - eye_offset, center.x + eye_offset] {
        let eye = ellipse(Point::new(eye_x, center.y - eye_offset), 1.5, 1.5);
        canvas
            .scene
            .push_path(eye, Some(colors.feature), stroke(colors.feature, 2.0));
    }
    let mouth_stroke = stroke(colors.feature, 1.0);
    let mouth_fill = Some(colors.mouth_fill);
    if score > 3.0 {
        let mouth = half_ring(Point::new(center.x, center.y + 2.0), 1.0);
        canvas.scene.push_path(mouth, mouth_fill, mouth_stroke);
    } else if score < 3.0 {
        let mouth = half_ring(Point::new(center.x, center.y + 7.0), -1.0);
        canvas.scene.push_path(mouth, mouth_fill, mouth_stroke);
    } else {
        let mouth_y = center.y + 7.0;
        let left = Point::new(center.x - 5.0, mouth_y);
        let mouth = line(left, Point::new(center.x + 5.0, mouth_y));
        canvas.scene.push_path(mouth, None, mouth_stroke);
    }
}

/// d3 `arc()` 반 고리(안 반지름 15/2.2, 바깥 15/2). `direction`이 1이면 아래쪽(웃는 입), −1이면 위쪽
fn half_ring(center: Point, direction: f64) -> Vec<PathCommand> {
    let outer = FACE_RADIUS / 2.0;
    let inner = FACE_RADIUS / 2.2;
    let at = |x: f64, y: f64| Point::new(center.x + x, center.y + y);
    vec![
        PathCommand::MoveTo(at(direction * outer, 0.0)),
        quarter_arc(
            center,
            at(direction * outer, 0.0),
            at(0.0, direction * outer),
        ),
        quarter_arc(
            center,
            at(0.0, direction * outer),
            at(-direction * outer, 0.0),
        ),
        PathCommand::LineTo(at(-direction * inner, 0.0)),
        quarter_arc(
            center,
            at(-direction * inner, 0.0),
            at(0.0, direction * inner),
        ),
        quarter_arc(
            center,
            at(0.0, direction * inner),
            at(direction * inner, 0.0),
        ),
        PathCommand::Close,
    ]
}

/// 중심 둘레 90° 원호를 3차 베지어 하나로 근사한다
fn quarter_arc(center: Point, from: Point, to: Point) -> PathCommand {
    const HANDLE: f64 = 0.552_284_749_8;
    let first = Point::new(
        from.x + HANDLE * (to.x - center.x),
        from.y + HANDLE * (to.y - center.y),
    );
    let second = Point::new(
        to.x + HANDLE * (from.x - center.x),
        to.y + HANDLE * (from.y - center.y),
    );
    PathCommand::CubicTo(first, second, to)
}

fn draw_title(canvas: &mut JourneyCanvas, title: &str) {
    canvas.scene.push_text(TextItem {
        text: title.to_string(),
        x: canvas.left_margin,
        baseline: 25.0,
        font_size: TITLE_FONT_SIZE,
        weight: FontWeight::Bold,
        color: canvas.colors.title,
        rotation: None,
    });
}

/// 작업 상자 아래를 가로지르는 굵은 화살표. 화살촉(`M0,0 V4 L6,2 Z`, 기준점 5,2)은 선 굵기 배율로 커진다
fn draw_axis_arrow(canvas: &mut JourneyCanvas, width: f64) {
    let y = BOX_HEIGHT * 4.0;
    let end_x = width - canvas.left_margin - 4.0;
    let axis = line(Point::new(canvas.left_margin, y), Point::new(end_x, y));
    let stroke_width = 4.0;
    canvas
        .scene
        .push_path(axis, None, stroke(canvas.colors.line, stroke_width));
    let head = [
        Point::new(end_x - 5.0 * stroke_width, y - 2.0 * stroke_width),
        Point::new(end_x - 5.0 * stroke_width, y + 2.0 * stroke_width),
        Point::new(end_x + stroke_width, y),
    ];
    canvas
        .scene
        .push_path(polygon(&head), Some(canvas.colors.arrow_head), None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;
    use crate::theme::{Palette, Theme};

    #[test]
    fn 작업_수와_제목으로_viewbox를_정한다() {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: crate::LabelMode::Html,
            theme_variables: Default::default(),
        };
        let definition = "journey\ntitle Day\nsection A\nTea: 5: Me\nWork: 1: Me, Cat\n";
        let scene = render(definition, &context).unwrap();
        assert_eq!(scene.width, 150.0 + 450.0 + 100.0);
        assert_eq!(scene.height, 540.0);
    }
}
