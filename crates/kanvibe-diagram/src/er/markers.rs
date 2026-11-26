//! markers.js의 erDiagram 차수 표식(only_one·zero_or_one·one_or_more·zero_or_more). 원은 흰색으로 채우고
//! 선은 `.marker{fill:none}`라 테두리만 그린다.

use super::parser::Cardinality;
use crate::flowchart::parser::ArrowType;
use crate::flowchart::render::CustomMarker;
use crate::scene::{Color, PathCommand, Point};

const WHITE: Color = Color::rgb(255, 255, 255);

const fn move_to(x: f64, y: f64) -> PathCommand {
    PathCommand::MoveTo(Point::new(x, y))
}

const fn line_to(x: f64, y: f64) -> PathCommand {
    PathCommand::LineTo(Point::new(x, y))
}

const fn quad_to(control_x: f64, control_y: f64, x: f64, y: f64) -> PathCommand {
    PathCommand::QuadTo(Point::new(control_x, control_y), Point::new(x, y))
}

static ONLY_ONE_START: CustomMarker = CustomMarker {
    reference: Point::new(0.0, 9.0),
    circle: None,
    strokes: &[
        move_to(9.0, 0.0),
        line_to(9.0, 18.0),
        move_to(15.0, 0.0),
        line_to(15.0, 18.0),
    ],
};

static ONLY_ONE_END: CustomMarker = CustomMarker {
    reference: Point::new(18.0, 9.0),
    circle: None,
    strokes: &[
        move_to(3.0, 0.0),
        line_to(3.0, 18.0),
        move_to(9.0, 0.0),
        line_to(9.0, 18.0),
    ],
};

static ZERO_OR_ONE_START: CustomMarker = CustomMarker {
    reference: Point::new(0.0, 9.0),
    circle: Some((Point::new(21.0, 9.0), 6.0, Some(WHITE))),
    strokes: &[move_to(9.0, 0.0), line_to(9.0, 18.0)],
};

static ZERO_OR_ONE_END: CustomMarker = CustomMarker {
    reference: Point::new(30.0, 9.0),
    circle: Some((Point::new(9.0, 9.0), 6.0, Some(WHITE))),
    strokes: &[move_to(21.0, 0.0), line_to(21.0, 18.0)],
};

static ONE_OR_MORE_START: CustomMarker = CustomMarker {
    reference: Point::new(18.0, 18.0),
    circle: None,
    strokes: &[
        move_to(0.0, 18.0),
        quad_to(18.0, 0.0, 36.0, 18.0),
        quad_to(18.0, 36.0, 0.0, 18.0),
        move_to(42.0, 9.0),
        line_to(42.0, 27.0),
    ],
};

static ONE_OR_MORE_END: CustomMarker = CustomMarker {
    reference: Point::new(27.0, 18.0),
    circle: None,
    strokes: &[
        move_to(3.0, 9.0),
        line_to(3.0, 27.0),
        move_to(9.0, 18.0),
        quad_to(27.0, 0.0, 45.0, 18.0),
        quad_to(27.0, 36.0, 9.0, 18.0),
    ],
};

static ZERO_OR_MORE_START: CustomMarker = CustomMarker {
    reference: Point::new(18.0, 18.0),
    circle: Some((Point::new(48.0, 18.0), 6.0, Some(WHITE))),
    strokes: &[
        move_to(0.0, 18.0),
        quad_to(18.0, 0.0, 36.0, 18.0),
        quad_to(18.0, 36.0, 0.0, 18.0),
    ],
};

static ZERO_OR_MORE_END: CustomMarker = CustomMarker {
    reference: Point::new(39.0, 18.0),
    circle: Some((Point::new(9.0, 18.0), 6.0, Some(WHITE))),
    strokes: &[
        move_to(21.0, 18.0),
        quad_to(39.0, 0.0, 57.0, 18.0),
        quad_to(39.0, 36.0, 21.0, 18.0),
    ],
};

/// 간선 시작(`is_start`)이나 끝에 붙일 차수 표식. `u`(MD_PARENT)는 원본에 표식이 없다
pub fn cardinality_arrow(cardinality: Cardinality, is_start: bool) -> ArrowType {
    let marker = match (cardinality, is_start) {
        (Cardinality::OnlyOne, true) => &ONLY_ONE_START,
        (Cardinality::OnlyOne, false) => &ONLY_ONE_END,
        (Cardinality::ZeroOrOne, true) => &ZERO_OR_ONE_START,
        (Cardinality::ZeroOrOne, false) => &ZERO_OR_ONE_END,
        (Cardinality::OneOrMore, true) => &ONE_OR_MORE_START,
        (Cardinality::OneOrMore, false) => &ONE_OR_MORE_END,
        (Cardinality::ZeroOrMore, true) => &ZERO_OR_MORE_START,
        (Cardinality::ZeroOrMore, false) => &ZERO_OR_MORE_END,
        (Cardinality::MdParent, _) => return ArrowType::None,
    };
    ArrowType::Custom(marker)
}
