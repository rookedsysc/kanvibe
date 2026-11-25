//! 참여자 모양 그리기(drawActorType*): participant 상자, collections 겹친 상자, queue 누운 원통, database 선 원통,
//! actor 사람, control 화살 원, entity 밑줄 원, boundary 기둥 원. 각 모양의 생명선 시작 높이와 그린 뒤 높이
//! (getBBox로 다시 잰 값)도 여기서 정한다.

use super::SequenceColors;
use super::document::{Participant, ParticipantKind, SequenceDocument};
use super::draw::{centered_text, line, stroke};
use super::measure::{FONT_SIZE, split_breaks};
use super::models::ActorBox;
use super::{ACTOR_HEIGHT, LABEL_BOX_HEIGHT};
use crate::DiagramError;
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, ellipse, polygon, rounded_rect};
use crate::text::FontBook;

/// 사람 모양 그림의 반폭(ACTOR_TYPE_WIDTH / 2)
pub(super) const FIGURE_HALF_WIDTH: f64 = 18.0;
/// control·entity·boundary 원 반지름
const FIGURE_RADIUS: f64 = 22.0;
/// collections 뒤 상자가 비켜 놓이는 거리
const STACK_OFFSET: f64 = 6.0;
/// `properties`의 class로 `.actor` 모양을 잃은 상자의 getNoteRect 기본 색
const CUSTOM_FILL: Color = Color::rgb(0xED, 0xF2, 0xAE);
const CUSTOM_STROKE: Color = Color::rgb(0x66, 0x66, 0x66);

/// 생명선과 함께 아래로 내려 그리는(lower) 상자 모양. 나머지는 노트·블록 위에 그린다
pub(super) fn is_boxed(kind: ParticipantKind) -> bool {
    matches!(
        kind,
        ParticipantKind::Participant
            | ParticipantKind::Collections
            | ParticipantKind::Queue
            | ParticipantKind::Database
    )
}

/// 링크 메뉴는 상자 모양이 남기는 자리(rectData)에 붙는다. 사람·boundary·control·entity 모양에 링크가 있으면
/// Mermaid drawPopup이 비어 있는 rectData를 읽다 TypeError로 그리기 전체가 실패하므로 같은 입력을 거부한다.
/// 문법(jison)은 이 입력을 받아들인다
pub(super) fn check_menu_anchors(document: &SequenceDocument) -> Result<(), DiagramError> {
    let unanchored = document
        .participants
        .iter()
        .find(|participant| !participant.link_labels.is_empty() && !is_boxed(participant.kind));
    match unanchored {
        Some(participant) => Err(DiagramError::Syntax(format!(
            "{} 참여자 모양에는 link·links 메뉴를 붙일 수 없습니다",
            participant.description
        ))),
        None => Ok(()),
    }
}

/// 그린 뒤 getBBox로 다시 잰 높이. 아래 참여자 자리와 아래 이름 위치에 쓴다
pub(super) fn drawn_height(kind: ParticipantKind, actor: &ActorBox) -> f64 {
    match kind {
        ParticipantKind::Participant | ParticipantKind::Collections | ParticipantKind::Queue => {
            actor.height
        }
        ParticipantKind::Actor => ACTOR_HEIGHT,
        ParticipantKind::Control => 2.0 * FIGURE_RADIUS + 2.0 * LABEL_BOX_HEIGHT,
        ParticipantKind::Entity | ParticipantKind::Boundary => {
            2.0 * FIGURE_RADIUS + LABEL_BOX_HEIGHT
        }
        ParticipantKind::Database => actor.width / 3.0 + LABEL_BOX_HEIGHT,
    }
}

/// 위 참여자 위 끝에서 생명선이 시작하는 거리
pub(super) fn lifeline_offset(kind: ParticipantKind, actor: &ActorBox) -> f64 {
    match kind {
        ParticipantKind::Participant | ParticipantKind::Collections | ParticipantKind::Queue => {
            actor.height
        }
        ParticipantKind::Database => actor.height + 10.0,
        ParticipantKind::Control | ParticipantKind::Entity => 75.0,
        ParticipantKind::Actor | ParticipantKind::Boundary => 80.0,
    }
}

/// 한 참여자를 위(`is_footer = false`)나 아래 자리에 그린다
pub(super) struct ParticipantPainter<'a> {
    pub fonts: &'a FontBook,
    pub colors: &'a SequenceColors,
    pub participant: &'a Participant,
    pub actor: &'a ActorBox,
    pub is_footer: bool,
}

impl ParticipantPainter<'_> {
    pub fn paint(&self, scene: &mut Scene) {
        match self.participant.kind {
            ParticipantKind::Participant => self.paint_box(scene),
            ParticipantKind::Collections => self.paint_collections(scene),
            ParticipantKind::Queue => self.paint_queue(scene),
            ParticipantKind::Database => self.paint_database(scene),
            ParticipantKind::Actor => self.paint_person(scene),
            ParticipantKind::Control => self.paint_control(scene),
            ParticipantKind::Entity => self.paint_entity(scene),
            ParticipantKind::Boundary => self.paint_boundary(scene),
        }
    }

    fn top(&self) -> f64 {
        match self.is_footer {
            true => self.actor.stop_y.unwrap_or(self.actor.start_y),
            false => self.actor.start_y,
        }
    }

    /// 이름 자리에 쓰는 높이: 위는 배치 높이, 아래는 위를 그린 뒤 다시 잰 높이
    fn label_height(&self) -> f64 {
        match self.is_footer {
            true => drawn_height(self.participant.kind, self.actor),
            false => self.actor.height,
        }
    }

    /// 설명 줄들을 가운데 둘레로 1em 간격으로 놓는다(byTspan, `dominant-baseline: central`)
    fn paint_label(&self, scene: &mut Scene, center: Point) {
        let lines = split_breaks(&self.actor.label);
        let middle = (lines.len() as f64 - 1.0) / 2.0;
        let metrics = self.fonts.primary_metrics(FONT_SIZE);
        let central_shift = (metrics.ascent - metrics.descent) / 2.0;
        for (index, text) in lines.iter().enumerate() {
            let y = center.y + (index as f64 - middle) * FONT_SIZE;
            scene.push_text(centered_text(
                self.fonts,
                text,
                center.x,
                y + central_shift,
                FONT_SIZE,
                self.colors.actor_text,
            ));
        }
    }

    /// `.actor` 채움·테두리, class 속성이 있으면 getNoteRect 기본 색
    fn box_style(&self) -> (Option<Color>, Option<StrokeStyle>) {
        match self.participant.has_custom_class {
            true => (Some(CUSTOM_FILL), stroke(CUSTOM_STROKE, 1.0, &[])),
            false => (
                Some(self.colors.actor_fill),
                stroke(self.colors.actor_stroke, 1.0, &[]),
            ),
        }
    }

    /// 원통 모양은 class 속성이 있으면 채움 속성이 없어 검게 칠해진다
    fn cylinder_style(&self) -> (Option<Color>, Option<StrokeStyle>) {
        match self.participant.has_custom_class {
            true => (Some(Color::rgb(0, 0, 0)), None),
            false => self.box_style(),
        }
    }

    fn paint_box(&self, scene: &mut Scene) {
        let (actor, y) = (self.actor, self.top());
        let (fill, outline) = self.box_style();
        let height = self.label_height();
        scene.push_path(
            rounded_rect(actor.x, y, actor.width, height, 3.0),
            fill,
            outline,
        );
        self.paint_label(scene, Point::new(actor.center(), y + height / 2.0));
    }

    fn paint_collections(&self, scene: &mut Scene) {
        let (actor, y) = (self.actor, self.top());
        let height = self.label_height();
        let (fill, outline) = self.box_style();
        scene.push_path(
            rounded_rect(actor.x, y, actor.width, height, 0.0),
            fill,
            outline,
        );
        let (x, stacked_y) = (actor.x - STACK_OFFSET, y + STACK_OFFSET);
        scene.push_path(
            rounded_rect(x, stacked_y, actor.width, height, 0.0),
            Some(self.colors.actor_fill),
            stroke(self.colors.actor_stroke, 1.0, &[]),
        );
        let center = Point::new(actor.center() - STACK_OFFSET, stacked_y + height / 2.0);
        self.paint_label(scene, center);
    }

    /// 누운 원통: 왼쪽으로 부푼 끝, 오른쪽으로 부푼 끝, 그 앞면 반원
    fn paint_queue(&self, scene: &mut Scene) {
        let (actor, y) = (self.actor, self.top());
        let height = self.label_height();
        let radius_y = height / 2.0;
        let radius_x = radius_y / (2.5 + height / 50.0);
        let (left, right) = (actor.x + radius_x, actor.x + actor.width - radius_x);
        let middle = y + radius_y;
        let half_turn = -std::f64::consts::PI;
        let quarter = std::f64::consts::FRAC_PI_2;
        let mut body = vec![PathCommand::MoveTo(Point::new(left, y))];
        body.extend(ellipse_arc(
            Point::new(left, middle),
            radius_x,
            radius_y,
            -quarter,
            half_turn,
        ));
        body.push(PathCommand::LineTo(Point::new(right, y + height)));
        body.extend(ellipse_arc(
            Point::new(right, middle),
            radius_x,
            radius_y,
            quarter,
            half_turn,
        ));
        body.push(PathCommand::Close);
        let (fill, outline) = self.cylinder_style();
        scene.push_path(body, fill, outline.clone());
        let mut front = vec![PathCommand::MoveTo(Point::new(right, y))];
        front.extend(ellipse_arc(
            Point::new(right, middle),
            radius_x,
            radius_y,
            -quarter,
            half_turn,
        ));
        scene.push_path(front, fill, outline);
        self.paint_label(scene, Point::new(actor.center(), y + height / 2.0));
    }

    /// 선 원통: 위 타원 전체, 양옆 선, 아래 반타원(닫지 않는다)
    fn paint_database(&self, scene: &mut Scene) {
        let (actor, y) = (self.actor, self.top());
        let width = actor.width / 3.0;
        let radius_x = width / 2.0;
        let radius_y = radius_x / (2.5 + width / 50.0);
        let left = actor.x + width;
        let top = y + radius_y;
        let half_turn = -std::f64::consts::PI;
        let top_center = Point::new(left + radius_x, top + radius_y);
        let bottom_center = Point::new(left + radius_x, top + width - radius_y);
        let mut commands = vec![PathCommand::MoveTo(Point::new(left, top + radius_y))];
        commands.extend(ellipse_arc(
            top_center,
            radius_x,
            radius_y,
            std::f64::consts::PI,
            half_turn,
        ));
        commands.extend(ellipse_arc(top_center, radius_x, radius_y, 0.0, half_turn));
        commands.push(PathCommand::LineTo(Point::new(left, bottom_center.y)));
        commands.extend(ellipse_arc(
            bottom_center,
            radius_x,
            radius_y,
            std::f64::consts::PI,
            half_turn,
        ));
        commands.push(PathCommand::LineTo(Point::new(left + width, top_center.y)));
        let (fill, outline) = self.cylinder_style();
        scene.push_path(commands, fill, outline);
        let center = Point::new(actor.center(), y + 35.0 + self.label_height() / 2.0);
        self.paint_label(scene, center);
    }

    /// 사람 모양(drawActorTypeActor): 몸통·팔·두 다리 선과 머리 원, 그 아래 이름
    fn paint_person(&self, scene: &mut Scene) {
        let (center, y) = (self.actor.center(), self.top());
        let point = |dx: f64, dy: f64| Point::new(center + dx, y + dy);
        let limbs = [
            (point(0.0, 25.0), point(0.0, 45.0)),
            (
                point(-FIGURE_HALF_WIDTH, 33.0),
                point(FIGURE_HALF_WIDTH, 33.0),
            ),
            (point(-FIGURE_HALF_WIDTH, 60.0), point(0.0, 45.0)),
            (point(0.0, 45.0), point(FIGURE_HALF_WIDTH - 2.0, 60.0)),
        ];
        let figure_stroke = stroke(self.colors.actor_stroke, 2.0, &[]);
        for (from, to) in limbs {
            scene.push_path(line(from, to), None, figure_stroke.clone());
        }
        scene.push_path(
            ellipse(point(0.0, 10.0), 15.0, 15.0),
            Some(self.colors.actor_fill),
            figure_stroke,
        );
        self.paint_label(scene, point(0.0, 35.0 + ACTOR_HEIGHT / 2.0));
    }

    /// control: 원과 원 위 끝의 172.5° 돌린 화살촉 표식(선 굵기 2배)
    fn paint_control(&self, scene: &mut Scene) {
        let (center, y) = (self.actor.center(), self.top());
        let colors = self.colors;
        scene.push_path(
            ellipse(Point::new(center, y + 32.0), FIGURE_RADIUS, FIGURE_RADIUS),
            Some(colors.actor_fill),
            stroke(colors.actor_stroke, 2.0, &[]),
        );
        // filled-head-control: M 14.4 5.6 L 7.2 10.4 L 8.8 5.6 L 7.2 0.8 Z, 기준점 (11, 5.8)
        let points = [(3.4, -0.2), (-3.8, 4.6), (-2.2, -0.2), (-3.8, -5.0)];
        let (sine, cosine) = 172.5_f64.to_radians().sin_cos();
        let tip = Point::new(center, y + 32.0 - FIGURE_RADIUS);
        let corners: Vec<Point> = points
            .iter()
            .map(|(u, v)| {
                let (u, v) = (u * 2.0, v * 2.0);
                Point::new(tip.x + u * cosine - v * sine, tip.y + u * sine + v * cosine)
            })
            .collect();
        scene.push_path(
            polygon(&corners),
            Some(colors.actor_fill),
            stroke(colors.actor_stroke, 2.4, &[]),
        );
        let offset = if self.is_footer { 5.0 } else { 12.0 };
        let label_y = y + FIGURE_RADIUS + offset + self.label_height() / 2.0;
        self.paint_label(scene, Point::new(center, label_y));
    }

    /// entity: 밑줄 그은 원. 그룹 전체가 위는 6px, 아래는 22px 내려간다
    fn paint_entity(&self, scene: &mut Scene) {
        let (center, y) = (self.actor.center(), self.top());
        let (shift, circle_offset, label_offset) = match self.is_footer {
            true => (FIGURE_RADIUS, 10.0, 15.0),
            false => (FIGURE_RADIUS / 2.0 - 5.0, 25.0, 30.0),
        };
        let circle_y = y + circle_offset + shift;
        scene.push_path(
            ellipse(Point::new(center, circle_y), FIGURE_RADIUS, FIGURE_RADIUS),
            Some(self.colors.actor_fill),
            stroke(self.colors.actor_stroke, 1.0, &[]),
        );
        let underline_y = circle_y + FIGURE_RADIUS;
        scene.push_path(
            line(
                Point::new(center - FIGURE_RADIUS, underline_y),
                Point::new(center + FIGURE_RADIUS, underline_y),
            ),
            None,
            stroke(self.colors.actor_stroke, 2.0, &[]),
        );
        let label_y = y + label_offset + self.label_height() / 2.0 + shift;
        self.paint_label(scene, Point::new(center, label_y));
    }

    /// boundary: 왼쪽 기둥과 가로선, 원. 그룹 전체가 21px 내려간다
    fn paint_boundary(&self, scene: &mut Scene) {
        let (center, y) = (self.actor.center(), self.top() + FIGURE_RADIUS / 2.0 + 10.0);
        let figure_stroke = stroke(self.colors.actor_stroke, 2.0, &[]);
        let post_x = center - FIGURE_RADIUS * 2.5;
        let strokes = [
            (
                Point::new(post_x, y + 12.0),
                Point::new(center - 15.0, y + 12.0),
            ),
            (Point::new(post_x, y + 2.0), Point::new(post_x, y + 22.0)),
        ];
        for (from, to) in strokes {
            scene.push_path(line(from, to), None, figure_stroke.clone());
        }
        scene.push_path(
            ellipse(Point::new(center, y + 12.0), FIGURE_RADIUS, FIGURE_RADIUS),
            Some(self.colors.actor_fill),
            figure_stroke,
        );
        let label_y = y + 15.0 + self.label_height() / 2.0;
        self.paint_label(scene, Point::new(center, label_y));
    }
}

/// 타원 호를 90° 이하 3차 베지어로 근사한다. 각은 SVG 좌표(아래가 +90°) 라디안, `sweep`이 음수면 반시계
pub(super) fn ellipse_arc(
    center: Point,
    radius_x: f64,
    radius_y: f64,
    start: f64,
    sweep: f64,
) -> Vec<PathCommand> {
    let point = |angle: f64| {
        Point::new(
            center.x + radius_x * angle.cos(),
            center.y + radius_y * angle.sin(),
        )
    };
    let tangent = |angle: f64| Point::new(-radius_x * angle.sin(), radius_y * angle.cos());
    let segments = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / segments as f64;
    let handle = 4.0 / 3.0 * (step / 4.0).tan();
    (0..segments)
        .map(|index| {
            let from = start + step * index as f64;
            let to = from + step;
            let (from_point, to_point) = (point(from), point(to));
            let (from_tangent, to_tangent) = (tangent(from), tangent(to));
            PathCommand::CubicTo(
                Point::new(
                    from_point.x + handle * from_tangent.x,
                    from_point.y + handle * from_tangent.y,
                ),
                Point::new(
                    to_point.x - handle * to_tangent.x,
                    to_point.y - handle * to_tangent.y,
                ),
                to_point,
            )
        })
        .collect()
}
