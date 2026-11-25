//! 메시지 그리기(drawMessage): 글, 선 또는 자기 메시지 고리, 선 끝 표식, 중앙 연결 동그라미, autonumber 동그라미.

use super::SequenceColors;
use super::WRAP_PADDING;
use super::document::CentralConnection;
use super::draw::{centered_text, line, middle_baseline, stacked_line_y, stroke};
use super::layout::PlacedMessage;
use super::markers::{Direction, MESSAGE_LINE_WIDTH, draw_end_marker, draw_start_marker};
use super::measure::{FONT_SIZE, js_round, split_breaks};
use super::models::ActorBox;
use crate::scene::{PathCommand, Point, Scene, ellipse};
use crate::text::FontBook;

const SEQUENCE_NUMBER_RADIUS: f64 = 6.0;
/// 번호 동그라미 표식 배율(번호용 선의 CSS 굵기 2px)
const SEQUENCE_NUMBER_SCALE: f64 = 2.0;
/// 중앙 연결 동그라미 반지름과, 번호가 있을 때 비켜 놓는 거리
const CENTRAL_CIRCLE_RADIUS: f64 = 5.0;
const CENTRAL_CIRCLE_OFFSET: f64 = 16.5;

pub(super) fn draw_message(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    actors: &[ActorBox],
    message: &PlacedMessage,
) {
    draw_message_text(scene, fonts, colors, message);
    let (commands, start_direction, end_direction) = message_path(message);
    let dash: &[f64] = if message.arrow.is_dotted {
        &[3.0, 3.0]
    } else {
        &[]
    };
    let (start, end) = path_ends(&commands);
    scene.push_path(
        commands,
        None,
        stroke(colors.message_line, MESSAGE_LINE_WIDTH, dash),
    );
    let head = message.arrow.head;
    draw_start_marker(scene, colors, head, start, start_direction.reversed());
    draw_end_marker(scene, colors, head, end, end_direction);
    draw_central_circles(scene, colors, actors, message);
    if let Some(number) = &message.number {
        draw_sequence_number(scene, fonts, colors, message, number);
    }
}

/// 글은 화살표 가운데 위, 줄마다 `dy = 1em`·`dominant-baseline: middle`
fn draw_message_text(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    message: &PlacedMessage,
) {
    let model = &message.model;
    let left = model.start_x.min(model.stop_x);
    let center_x = js_round(left + (model.stop_x - model.start_x).abs() / 2.0);
    let top = message.start_y + 10.0;
    for (index, text) in split_breaks(&model.text).iter().enumerate() {
        let y = stacked_line_y(fonts, top, index, WRAP_PADDING);
        scene.push_text(centered_text(
            fonts,
            text,
            center_x,
            middle_baseline(y, FONT_SIZE),
            FONT_SIZE,
            colors.message_text,
        ));
    }
}

/// 보내는 쪽에 중앙 연결 동그라미가 있는지
fn has_source_circle(message: &PlacedMessage) -> bool {
    matches!(
        message.central,
        Some(CentralConnection::Source | CentralConnection::Both)
    )
}

/// 번호가 있으면 선 끝을 번호 동그라미(와 중앙 연결 동그라미) 밖으로 민다
fn line_ends(message: &PlacedMessage) -> (f64, f64) {
    let (start, stop) = (message.model.start_x, message.model.stop_x);
    if message.number.is_none() {
        return (start, stop);
    }
    let source_shift = if has_source_circle(message) {
        -7.5
    } else {
        0.0
    };
    let has_central = message.central.is_some();
    let arrow = message.arrow;
    if arrow.is_bidirectional() {
        let central_shift = if has_central { -5.0 } else { 0.0 };
        return match start < stop {
            true => (start + SEQUENCE_NUMBER_RADIUS * 2.0, stop),
            false => (
                start - SEQUENCE_NUMBER_RADIUS + central_shift + source_shift,
                stop,
            ),
        };
    }
    if !arrow.is_reverse() {
        return (start + SEQUENCE_NUMBER_RADIUS, stop);
    }
    let central_extension = if has_central { 15.0 } else { 0.0 };
    match stop > start {
        true => (
            start,
            stop - SEQUENCE_NUMBER_RADIUS * 2.0 + central_extension,
        ),
        false => (
            start + source_shift,
            stop - SEQUENCE_NUMBER_RADIUS + central_extension,
        ),
    }
}

/// 선(또는 자기 메시지 고리)과 시작·끝 접선 방향
fn message_path(message: &PlacedMessage) -> (Vec<PathCommand>, Direction, Direction) {
    let (model, y) = (&message.model, message.line_y);
    if model.start_x != model.stop_x {
        let (x1, x2) = line_ends(message);
        let (from, to) = (Point::new(x1, y), Point::new(x2, y));
        let direction = Direction::between(from, to);
        return (line(from, to), direction, direction);
    }
    let arrow = message.arrow;
    let is_shifted = message.number.is_some() && (arrow.is_reverse() || arrow.is_bidirectional());
    let start = Point::new(model.start_x + if is_shifted { 10.0 } else { 0.0 }, y);
    let first = Point::new(start.x + 60.0, y - 10.0);
    let second = Point::new(model.start_x + 60.0, y + 30.0);
    let end = Point::new(model.start_x, y + 20.0);
    let commands = vec![
        PathCommand::MoveTo(start),
        PathCommand::CubicTo(first, second, end),
    ];
    (
        commands,
        Direction::between(start, first),
        Direction::between(second, end),
    )
}

fn path_ends(commands: &[PathCommand]) -> (Point, Point) {
    let point_of = |command: &PathCommand| match command {
        PathCommand::MoveTo(point) | PathCommand::LineTo(point) => *point,
        PathCommand::QuadTo(_, point) | PathCommand::CubicTo(_, _, point) => *point,
        PathCommand::Close => Point::new(0.0, 0.0),
    };
    let start = commands.first().map_or(Point::new(0.0, 0.0), point_of);
    let end = commands.last().map_or(start, point_of);
    (start, end)
}

/// drawCentralConnection: 참여자 생명선 위 동그라미. 번호가 있으면 번호 동그라미를 피해 비켜 놓는다
fn draw_central_circles(
    scene: &mut Scene,
    colors: &SequenceColors,
    actors: &[ActorBox],
    message: &PlacedMessage,
) {
    let Some(central) = message.central else {
        return;
    };
    let mut from_center = actors[message.from].center();
    let mut to_center = actors[message.to].center();
    let is_left_to_right = from_center <= to_center;
    let is_reverse = message.arrow.is_reverse();
    let offset = |is_flipped: bool| {
        let base = match is_left_to_right {
            true => CENTRAL_CIRCLE_OFFSET,
            false => -CENTRAL_CIRCLE_OFFSET,
        };
        if is_flipped { -base } else { base }
    };
    if message.number.is_some() {
        let moves_target = central != CentralConnection::Source && is_reverse;
        let moves_source = central != CentralConnection::Target && !is_reverse;
        to_center += if moves_target { offset(true) } else { 0.0 };
        from_center += if moves_source { offset(false) } else { 0.0 };
    }
    let centers: &[f64] = match central {
        CentralConnection::Target => &[to_center],
        CentralConnection::Source => &[from_center],
        CentralConnection::Both => &[from_center, to_center],
    };
    for x in centers {
        let center = Point::new(*x, message.line_y);
        let circle = ellipse(center, CENTRAL_CIRCLE_RADIUS, CENTRAL_CIRCLE_RADIUS);
        scene.push_path(circle, Some(colors.root_fill), None);
    }
}

/// 번호 동그라미: 화살표가 떠나는 쪽 경계 1px 안(거꾸로 반쪽 화살표는 화살촉 쪽)
fn draw_sequence_number(
    scene: &mut Scene,
    fonts: &FontBook,
    colors: &SequenceColors,
    message: &PlacedMessage,
    number: &str,
) {
    let model = &message.model;
    let is_left_to_right = model.start_x <= model.stop_x;
    let is_self = model.start_x == model.stop_x;
    let is_on_left = is_self || (is_left_to_right != message.arrow.is_reverse());
    let x = if is_on_left {
        model.from_bounds + 1.0
    } else {
        model.to_bounds - 1.0
    };
    let center = Point::new(x, message.line_y);
    let radius = SEQUENCE_NUMBER_RADIUS * SEQUENCE_NUMBER_SCALE;
    scene.push_path(
        ellipse(center, radius, radius),
        Some(colors.number_fill),
        None,
    );
    let font_size = match number.chars().count() {
        length if length > 5 => 7.0,
        length if length > 3 => 9.0,
        _ => 12.0,
    };
    scene.push_text(centered_text(
        fonts,
        number,
        x,
        message.line_y + 4.0,
        font_size,
        colors.number_text,
    ));
}
