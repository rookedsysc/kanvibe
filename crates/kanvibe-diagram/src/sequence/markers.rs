//! 메시지 선 끝 표식(svgDraw의 insert*Head). 좌표는 표식 기준점을 뺀 값이고, 선 끝 방향으로 돌려 놓는다.
//!
//! `arrowhead`와 반쪽 화살촉은 userSpaceOnUse라 크기가 그대로고, `filled-head`·`crosshead`는 선 굵기(1.5)만큼
//! 커진다. 채움 CSS가 없는 표식은 SVG 뿌리의 `fill`을 물려받는다.

use super::SequenceColors;
use super::document::ArrowHead;
use super::draw::{line, stroke};
use crate::scene::{Color, Point, Scene, polygon};

pub(super) const MESSAGE_LINE_WIDTH: f64 = 1.5;
/// crosshead 선 굵기 1pt
const CROSS_STROKE_WIDTH: f64 = 4.0 / 3.0;
/// 막대 반쪽 화살촉 선 굵기
const STICK_STROKE_WIDTH: f64 = 1.5;

/// 선 끝 방향(단위 벡터)
#[derive(Clone, Copy, Debug)]
pub(super) struct Direction {
    x: f64,
    y: f64,
}

impl Direction {
    pub fn between(from: Point, to: Point) -> Self {
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let length = dx.hypot(dy).max(f64::EPSILON);
        Self {
            x: dx / length,
            y: dy / length,
        }
    }

    /// `orient="auto-start-reverse"`의 시작 표식 방향
    pub fn reversed(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

/// 반쪽 화살촉 표식 네 가지
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HalfMarker {
    SolidTop,
    SolidBottom,
    StickTop,
    StickBottom,
}

/// 표식 좌표(기준점을 뺀 뒤 배율을 곱한 값)를 선 끝 자리와 방향으로 옮긴다
fn place_marker(tip: Point, direction: Direction, points: &[(f64, f64)], scale: f64) -> Vec<Point> {
    points
        .iter()
        .map(|(u, v)| {
            let (u, v) = (u * scale, v * scale);
            Point::new(
                tip.x + u * direction.x - v * direction.y,
                tip.y + u * direction.y + v * direction.x,
            )
        })
        .collect()
}

/// 선 시작에 붙는 표식: 양방향은 화살촉, 거꾸로 반쪽 화살촉은 위아래가 바뀐 표식
pub(super) fn draw_start_marker(
    scene: &mut Scene,
    colors: &SequenceColors,
    head: ArrowHead,
    tip: Point,
    direction: Direction,
) {
    let half = match head {
        ArrowHead::Bidirectional => {
            return draw_arrowhead(scene, colors.message_line, tip, direction);
        }
        ArrowHead::ReverseSolidTop => HalfMarker::SolidBottom,
        ArrowHead::ReverseSolidBottom => HalfMarker::SolidTop,
        ArrowHead::ReverseStickTop => HalfMarker::StickBottom,
        ArrowHead::ReverseStickBottom => HalfMarker::StickTop,
        _ => return,
    };
    draw_half_marker(scene, colors, half, tip, direction);
}

pub(super) fn draw_end_marker(
    scene: &mut Scene,
    colors: &SequenceColors,
    head: ArrowHead,
    tip: Point,
    direction: Direction,
) {
    let half = match head {
        ArrowHead::Filled | ArrowHead::Bidirectional => {
            return draw_arrowhead(scene, colors.message_line, tip, direction);
        }
        ArrowHead::Async => return draw_async_head(scene, colors.root_fill, tip, direction),
        ArrowHead::Cross => return draw_crosshead(scene, colors.message_line, tip, direction),
        ArrowHead::SolidTop => HalfMarker::SolidTop,
        ArrowHead::SolidBottom => HalfMarker::SolidBottom,
        ArrowHead::StickTop => HalfMarker::StickTop,
        ArrowHead::StickBottom => HalfMarker::StickBottom,
        _ => return,
    };
    draw_half_marker(scene, colors, half, tip, direction);
}

/// arrowhead: M -1 0 L 10 5 L 0 10 z, 기준점 (7.9, 5), 채움과 1px 테두리
fn draw_arrowhead(scene: &mut Scene, color: Color, tip: Point, direction: Direction) {
    let points = [(-8.9, -5.0), (2.1, 0.0), (-7.9, 5.0)];
    let corners = place_marker(tip, direction, &points, 1.0);
    scene.push_path(polygon(&corners), Some(color), stroke(color, 1.0, &[]));
}

/// filled-head: M 18,7 L9,13 L14,7 L9,1 Z, 기준점 (15.5, 7)
fn draw_async_head(scene: &mut Scene, color: Color, tip: Point, direction: Direction) {
    let points = [(2.5, 0.0), (-6.5, 6.0), (-1.5, 0.0), (-6.5, -6.0)];
    let corners = place_marker(tip, direction, &points, MESSAGE_LINE_WIDTH);
    scene.push_path(polygon(&corners), Some(color), None);
}

/// crosshead: M 1,2 L 6,7 M 6,2 L 1,7, 기준점 (4, 4.5)
fn draw_crosshead(scene: &mut Scene, color: Color, tip: Point, direction: Direction) {
    let strokes = [[(-3.0, -2.5), (2.0, 2.5)], [(2.0, -2.5), (-3.0, 2.5)]];
    let width = CROSS_STROKE_WIDTH * MESSAGE_LINE_WIDTH;
    for points in strokes {
        let ends = place_marker(tip, direction, &points, MESSAGE_LINE_WIDTH);
        scene.push_path(line(ends[0], ends[1]), None, stroke(color, width, &[]));
    }
}

/// 반쪽 화살촉. 채운 삼각형은 뿌리 채움색, 막대는 검은 1.5px 선이다
fn draw_half_marker(
    scene: &mut Scene,
    colors: &SequenceColors,
    marker: HalfMarker,
    tip: Point,
    direction: Direction,
) {
    let (points, is_stick): (&[(f64, f64)], bool) = match marker {
        // M 0 0 L 10 8 L 0 8 z, 기준점 (7.9, 7.25)
        HalfMarker::SolidTop => (&[(-7.9, -7.25), (2.1, 0.75), (-7.9, 0.75)], false),
        // M 0 0 L 10 0 L 0 8 z, 기준점 (7.9, 0.75)
        HalfMarker::SolidBottom => (&[(-7.9, -0.75), (2.1, -0.75), (-7.9, 7.25)], false),
        // M 0 0 L 7 7, 기준점 (7.5, 7)
        HalfMarker::StickTop => (&[(-7.5, -7.0), (-0.5, 0.0)], true),
        // M 0 7 L 7 0, 기준점 (7.5, 0)
        HalfMarker::StickBottom => (&[(-7.5, 7.0), (-0.5, 0.0)], true),
    };
    let corners = place_marker(tip, direction, points, 1.0);
    if is_stick {
        let black = stroke(colors.stick_head, STICK_STROKE_WIDTH, &[]);
        scene.push_path(line(corners[0], corners[1]), None, black);
    } else {
        scene.push_path(polygon(&corners), Some(colors.root_fill), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 표식은_선_방향으로_돈다() {
        let tip = Point::new(100.0, 50.0);
        let left = Direction::between(Point::new(200.0, 50.0), tip);
        let corners = place_marker(tip, left, &[(2.1, 0.0)], 1.0);
        assert!((corners[0].x - 97.9).abs() < 1e-9);
    }
}
