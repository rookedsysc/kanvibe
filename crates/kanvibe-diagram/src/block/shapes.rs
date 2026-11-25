//! dagre-wrapper `shapes`(nodes.js)의 block 다이어그램 모양. 크기는 라벨 상자와 padding(8)으로 정하고, 사각형·
//! 묶음·육각형은 배치가 끝난 뒤 배치 크기로 다시 그린다. 화살표 블록은 여러 칸을 차지해 넓어졌을 때만 넓힌다.
//!
//! 돌려주는 [`NodeGeometry`]의 폭·높이는 `updateNodeBounds`가 남긴 값(간선 교점 계산용)이고, 윤곽선은 노드 중심
//! 기준 좌표다.

use std::collections::HashSet;

use super::BlockType;
use crate::flowchart::parser::NodeShape;
use crate::flowchart::shapes::{Intersect, NodeGeometry, node_geometry};
use crate::scene::{PathCommand, Point, ellipse, polygon, rounded_rect};

/// 모양을 정하는 데 필요한 노드 정보
pub(crate) struct ShapeInput<'a> {
    pub kind: BlockType,
    pub label_width: f64,
    pub label_height: f64,
    pub padding: f64,
    pub directions: &'a [String],
    pub width_in_columns: u32,
    /// 배치가 끝난 뒤 그릴 때의 배치 크기(positioned). 크기를 잴 때는 `None`
    pub layout_size: Option<(f64, f64)>,
}

/// 원 둘레를 겹으로 그릴 때 안쪽 원과의 간격(doublecircle gap)
const DOUBLE_CIRCLE_GAP: f64 = 5.0;

pub(crate) fn shape_geometry(input: &ShapeInput) -> NodeGeometry {
    let (label_width, label_height, padding) =
        (input.label_width, input.label_height, input.padding);
    let (width, height) = (label_width + padding, label_height + padding);
    match input.kind {
        BlockType::Round => rect_geometry(input, 5.0),
        BlockType::Diamond => diamond_geometry(width + height),
        BlockType::Hexagon => hexagon_geometry(input),
        BlockType::BlockArrow => block_arrow_geometry(input),
        BlockType::LeftInvArrow => left_inv_arrow_geometry(width, height),
        BlockType::LeanRight
        | BlockType::LeanLeft
        | BlockType::Trapezoid
        | BlockType::InvTrapezoid => slanted_geometry(input.kind, width, height),
        BlockType::Stadium => stadium_geometry(label_width, label_height, padding),
        BlockType::Circle => circle_geometry(label_width / 2.0 + padding / 2.0, 0.0),
        BlockType::DoubleCircle => {
            circle_geometry(label_width / 2.0 + padding / 2.0, DOUBLE_CIRCLE_GAP)
        }
        BlockType::Subroutine => {
            node_geometry(NodeShape::Subroutine, label_width, label_height, padding)
        }
        BlockType::Cylinder => cylinder_geometry(width, label_height + padding),
        _ => rect_geometry(input, 0.0),
    }
}

/// rect·composite: 배치가 끝나면 배치 크기를 쓴다
fn rect_geometry(input: &ShapeInput, radius: f64) -> NodeGeometry {
    let (width, height) = input.layout_size.unwrap_or((
        input.label_width + input.padding,
        input.label_height + input.padding,
    ));
    NodeGeometry {
        width,
        height,
        outlines: vec![rounded_rect(
            -width / 2.0,
            -height / 2.0,
            width,
            height,
            radius,
        )],
        intersect: Intersect::Rect,
        label_offset_y: 0.0,
    }
}

fn point_bounds(points: &[Point]) -> (f64, f64) {
    let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for point in points {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    (max_x - min_x, max_y - min_y)
}

/// insertPolygonShape: 점을 translate(-w/2, h/2)로 옮겨 그리고, 교점 크기는 점들의 bbox다
fn polygon_geometry(points: Vec<Point>, width: f64, height: f64) -> NodeGeometry {
    let (bounds_width, bounds_height) = point_bounds(&points);
    let drawn: Vec<Point> = points
        .iter()
        .map(|point| Point::new(point.x - width / 2.0, point.y + height / 2.0))
        .collect();
    NodeGeometry {
        width: bounds_width,
        height: bounds_height,
        outlines: vec![polygon(&drawn)],
        intersect: Intersect::Polygon {
            points,
            correction: 0.0,
        },
        label_offset_y: 0.0,
    }
}

/// question: 한 변이 (라벨 폭 + padding) + (라벨 높이 + padding)인 마름모
fn diamond_geometry(size: f64) -> NodeGeometry {
    let points = vec![
        Point::new(size / 2.0, 0.0),
        Point::new(size, -size / 2.0),
        Point::new(size / 2.0, -size),
        Point::new(0.0, -size / 2.0),
    ];
    polygon_geometry(points, size, size)
}

/// hexagon: 배치가 끝나면 배치 높이·폭을 그대로 쓴다
fn hexagon_geometry(input: &ShapeInput) -> NodeGeometry {
    let natural_height = input.label_height + input.padding;
    let height = input
        .layout_size
        .map_or(natural_height, |(_, height)| height);
    let notch = height / 4.0;
    let width = input.layout_size.map_or(
        input.label_width + 2.0 * notch + input.padding,
        |(width, _)| width,
    );
    let points = vec![
        Point::new(notch, 0.0),
        Point::new(width - notch, 0.0),
        Point::new(width, -height / 2.0),
        Point::new(width - notch, -height),
        Point::new(notch, -height),
        Point::new(0.0, -height / 2.0),
    ];
    polygon_geometry(points, width, height)
}

/// rect_left_inv_arrow(`>"이름"]`): 교점 크기는 bbox가 아니라 (w + h, h)로 둔다
fn left_inv_arrow_geometry(width: f64, height: f64) -> NodeGeometry {
    let points = vec![
        Point::new(-height / 2.0, 0.0),
        Point::new(width, 0.0),
        Point::new(width, -height),
        Point::new(-height / 2.0, -height),
        Point::new(0.0, -height / 2.0),
    ];
    let mut geometry = polygon_geometry(points, width, height);
    geometry.width = width + height;
    geometry.height = height;
    geometry
}

fn slanted_geometry(kind: BlockType, width: f64, height: f64) -> NodeGeometry {
    let sixth = height / 6.0;
    let corners = match kind {
        BlockType::LeanRight => [
            (-2.0 * sixth, 0.0),
            (width - sixth, 0.0),
            (width + 2.0 * sixth, -height),
            (sixth, -height),
        ],
        BlockType::LeanLeft => [
            (2.0 * sixth, 0.0),
            (width + sixth, 0.0),
            (width - 2.0 * sixth, -height),
            (-sixth, -height),
        ],
        BlockType::Trapezoid => [
            (-2.0 * sixth, 0.0),
            (width + 2.0 * sixth, 0.0),
            (width - sixth, -height),
            (sixth, -height),
        ],
        _ => [
            (sixth, 0.0),
            (width - sixth, 0.0),
            (width + 2.0 * sixth, -height),
            (-2.0 * sixth, -height),
        ],
    };
    let points = corners.iter().map(|(x, y)| Point::new(*x, *y)).collect();
    polygon_geometry(points, width, height)
}

/// stadium: 반지름이 높이 절반인 둥근 사각형, 교점은 사각형으로 잰다
fn stadium_geometry(label_width: f64, label_height: f64, padding: f64) -> NodeGeometry {
    let height = label_height + padding;
    let width = label_width + height / 4.0 + padding;
    NodeGeometry {
        width,
        height,
        outlines: vec![rounded_rect(
            -width / 2.0,
            -height / 2.0,
            width,
            height,
            height / 2.0,
        )],
        intersect: Intersect::Rect,
        label_offset_y: 0.0,
    }
}

/// 원기둥: `M 0,ry a … w 0 a … -w 0 l 0,H a … w 0 l 0,-H` 한 경로다. 윗면 타원과 몸통을 같은 방향으로 돌아
/// 겹친 곳도 채워진다. 라벨은 옮기지 않는다
fn cylinder_geometry(width: f64, label_box_height: f64) -> NodeGeometry {
    let radius_x = width / 2.0;
    let radius_y = radius_x / (2.5 + width / 50.0);
    let body_height = label_box_height + radius_y;
    let (left, top) = (-width / 2.0, -(body_height / 2.0 + radius_y));
    let k = 0.552_284_749_8;
    let at = |x: f64, y: f64| Point::new(left + x, top + y);
    let quarter = |from: (f64, f64), to: (f64, f64), corner: (f64, f64)| {
        let first = (
            from.0 + (corner.0 - from.0) * k,
            from.1 + (corner.1 - from.1) * k,
        );
        let second = (to.0 + (corner.0 - to.0) * k, to.1 + (corner.1 - to.1) * k);
        PathCommand::CubicTo(at(first.0, first.1), at(second.0, second.1), at(to.0, to.1))
    };
    let (rx, ry, w, h) = (radius_x, radius_y, width, body_height);
    let outline = vec![
        PathCommand::MoveTo(at(0.0, ry)),
        quarter((0.0, ry), (rx, 2.0 * ry), (0.0, 2.0 * ry)),
        quarter((rx, 2.0 * ry), (w, ry), (w, 2.0 * ry)),
        quarter((w, ry), (rx, 0.0), (w, 0.0)),
        quarter((rx, 0.0), (0.0, ry), (0.0, 0.0)),
        PathCommand::LineTo(at(0.0, ry + h)),
        quarter((0.0, ry + h), (rx, 2.0 * ry + h), (0.0, 2.0 * ry + h)),
        quarter((rx, 2.0 * ry + h), (w, ry + h), (w, 2.0 * ry + h)),
        // 닫지 않은 경로라 채울 때만 시작점으로 돌아가고 테두리는 긋지 않는다
        PathCommand::LineTo(at(w, ry)),
    ];
    NodeGeometry {
        width,
        height: body_height + 2.0 * radius_y,
        outlines: vec![outline],
        intersect: Intersect::Cylinder { radius_x, radius_y },
        label_offset_y: 0.0,
    }
}

/// circle·doublecircle: 바깥 원 반지름이 (라벨 폭 + padding)/2 + gap이다
fn circle_geometry(radius: f64, gap: f64) -> NodeGeometry {
    let center = Point::new(0.0, 0.0);
    let outer = radius + gap;
    let mut outlines = vec![ellipse(center, outer, outer)];
    if gap > 0.0 {
        outlines.push(ellipse(center, radius, radius));
    }
    NodeGeometry {
        width: outer * 2.0,
        height: outer * 2.0,
        outlines,
        intersect: Intersect::Circle(outer),
        label_offset_y: 0.0,
    }
}

/// block_arrow: 높이는 라벨 높이 + 2·padding, 화살촉 길이는 높이 절반이다
fn block_arrow_geometry(input: &ShapeInput) -> NodeGeometry {
    let height = input.label_height + 2.0 * input.padding;
    let midpoint = height / 2.0;
    let natural_width = input.label_width + 2.0 * midpoint + input.padding;
    let is_spanning = input.width_in_columns > 1
        && input
            .layout_size
            .is_some_and(|(width, _)| width > natural_width);
    let width = match input.layout_size {
        Some((layout_width, _)) if is_spanning => layout_width,
        _ => natural_width,
    };
    let frame = ArrowFrame {
        width,
        height,
        midpoint,
        half_padding: input.padding / 2.0,
    };
    let points = frame.points(&expand_directions(input.directions));
    polygon_geometry(points, width, height)
}

/// expandAndDeduplicateDirections: `x`는 좌우, `y`는 위아래
fn expand_directions(directions: &[String]) -> HashSet<&str> {
    let mut expanded = HashSet::new();
    for direction in directions {
        match direction.as_str() {
            "x" => expanded.extend(["right", "left"]),
            "y" => expanded.extend(["up", "down"]),
            other => {
                expanded.insert(other);
            }
        }
    }
    expanded
}

/// getArrowPoints의 치수
struct ArrowFrame {
    width: f64,
    height: f64,
    midpoint: f64,
    half_padding: f64,
}

fn to_points(corners: &[(f64, f64)]) -> Vec<Point> {
    corners.iter().map(|(x, y)| Point::new(*x, *y)).collect()
}

impl ArrowFrame {
    fn points(&self, directions: &HashSet<&str>) -> Vec<Point> {
        let has = |name: &str| directions.contains(name);
        match (has("right"), has("left"), has("up"), has("down")) {
            (true, true, true, true) => self.four_way(),
            (true, true, true, false) => self.three_way(ArrowSide::Up),
            (true, true, false, true) => self.three_way(ArrowSide::Down),
            (true, false, true, true) => self.three_way(ArrowSide::Right),
            (false, true, true, true) => self.three_way(ArrowSide::Left),
            (true, true, false, false) => self.horizontal_pair(),
            (false, false, true, true) => self.vertical_pair(),
            (true, false, true, false) => self.corner(ArrowSide::Right, ArrowSide::Up),
            (true, false, false, true) => self.corner(ArrowSide::Right, ArrowSide::Down),
            (false, true, true, false) => self.corner(ArrowSide::Left, ArrowSide::Up),
            (false, true, false, true) => self.corner(ArrowSide::Left, ArrowSide::Down),
            (true, false, false, false) => self.single(ArrowSide::Right),
            (false, true, false, false) => self.single(ArrowSide::Left),
            (false, false, true, false) => self.single(ArrowSide::Up),
            (false, false, false, true) => self.single(ArrowSide::Down),
            _ => vec![Point::new(0.0, 0.0)],
        }
    }

    fn four_way(&self) -> Vec<Point> {
        let (w, h, m, p) = (self.width, self.height, self.midpoint, self.half_padding);
        to_points(&[
            (0.0, 0.0),
            (m, 0.0),
            (w / 2.0, 2.0 * p),
            (w - m, 0.0),
            (w, 0.0),
            (w, -h / 3.0),
            (w + 2.0 * p, -h / 2.0),
            (w, -2.0 * h / 3.0),
            (w, -h),
            (w - m, -h),
            (w / 2.0, -h - 2.0 * p),
            (m, -h),
            (0.0, -h),
            (0.0, -2.0 * h / 3.0),
            (-2.0 * p, -h / 2.0),
            (0.0, -h / 3.0),
        ])
    }

    /// 세 방향 화살표. `branch`는 원본 분기에서 좌우·위아래 짝 말고 남는 한 방향이다
    fn three_way(&self, branch: ArrowSide) -> Vec<Point> {
        let (w, h, m) = (self.width, self.height, self.midpoint);
        match branch {
            ArrowSide::Up => to_points(&[
                (m, 0.0),
                (w - m, 0.0),
                (w, -h / 2.0),
                (w - m, -h),
                (m, -h),
                (0.0, -h / 2.0),
            ]),
            ArrowSide::Down => to_points(&[(0.0, 0.0), (m, -h), (w - m, -h), (w, 0.0)]),
            ArrowSide::Right => to_points(&[(0.0, 0.0), (w, -m), (w, -h + m), (0.0, -h)]),
            ArrowSide::Left => to_points(&[(w, 0.0), (0.0, -m), (0.0, -h + m), (w, -h)]),
        }
    }

    fn horizontal_pair(&self) -> Vec<Point> {
        let (w, h, m, p) = (self.width, self.height, self.midpoint, self.half_padding);
        to_points(&[
            (m, 0.0),
            (m, -p),
            (w - m, -p),
            (w - m, 0.0),
            (w, -h / 2.0),
            (w - m, -h),
            (w - m, -h + p),
            (m, -h + p),
            (m, -h),
            (0.0, -h / 2.0),
        ])
    }

    fn vertical_pair(&self) -> Vec<Point> {
        let (w, h, m, p) = (self.width, self.height, self.midpoint, self.half_padding);
        to_points(&[
            (w / 2.0, 0.0),
            (0.0, -p),
            (m, -p),
            (m, -h + p),
            (0.0, -h + p),
            (w / 2.0, -h),
            (w, -h + p),
            (w - m, -h + p),
            (w - m, -p),
            (w, -p),
        ])
    }

    fn corner(&self, horizontal: ArrowSide, vertical: ArrowSide) -> Vec<Point> {
        let (w, h, m) = (self.width, self.height, self.midpoint);
        match (horizontal, vertical) {
            (ArrowSide::Right, ArrowSide::Up) => to_points(&[(0.0, 0.0), (w, -m), (0.0, -h)]),
            (ArrowSide::Right, _) => to_points(&[(0.0, 0.0), (w, 0.0), (0.0, -h)]),
            (_, ArrowSide::Up) => to_points(&[(w, 0.0), (0.0, -m), (w, -h)]),
            _ => to_points(&[(w, 0.0), (0.0, 0.0), (w, -h)]),
        }
    }

    fn single(&self, side: ArrowSide) -> Vec<Point> {
        let (w, h, m, p) = (self.width, self.height, self.midpoint, self.half_padding);
        match side {
            ArrowSide::Right => to_points(&[
                (m, -p),
                (m, -p),
                (w - m, -p),
                (w - m, 0.0),
                (w, -h / 2.0),
                (w - m, -h),
                (w - m, -h + p),
                (m, -h + p),
                (m, -h + p),
            ]),
            ArrowSide::Left => to_points(&[
                (m, 0.0),
                (m, -p),
                (w - m, -p),
                (w - m, -h + p),
                (m, -h + p),
                (m, -h),
                (0.0, -h / 2.0),
            ]),
            ArrowSide::Up => to_points(&[
                (m, -p),
                (m, -h + p),
                (0.0, -h + p),
                (w / 2.0, -h),
                (w, -h + p),
                (w - m, -h + p),
                (w - m, -p),
            ]),
            ArrowSide::Down => to_points(&[
                (w / 2.0, 0.0),
                (0.0, -p),
                (m, -p),
                (m, -h + p),
                (w - m, -h + p),
                (w - m, -p),
                (w, -p),
            ]),
        }
    }
}

#[derive(Clone, Copy)]
enum ArrowSide {
    Right,
    Left,
    Up,
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: BlockType, label_width: f64, directions: &[String]) -> ShapeInput<'_> {
        ShapeInput {
            kind,
            label_width,
            label_height: 24.0,
            padding: 8.0,
            directions,
            width_in_columns: 1,
            layout_size: None,
        }
    }

    #[test]
    fn 기준_svg의_모양_크기와_같다() {
        // block-shapes·block-arrows 기준 SVG의 polygon·rect 크기
        let diamond = shape_geometry(&input(BlockType::Diamond, 63.875, &[]));
        assert_eq!((diamond.width, diamond.height), (103.875, 103.875));
        let stadium = shape_geometry(&input(BlockType::Stadium, 40.828125, &[]));
        assert_eq!(stadium.width, 56.828125);
        let lean = shape_geometry(&input(BlockType::LeanRight, 57.6875, &[]));
        assert!((lean.width - 87.020_833).abs() < 1e-3, "{}", lean.width);
    }

    #[test]
    fn 화살표_블록은_방향마다_점을_고른다() {
        let right = ["right".to_string()];
        let send = shape_geometry(&input(BlockType::BlockArrow, 37.546875, &right));
        // 점 x: 20 … 85.546875, 높이 40
        assert_eq!((send.width, send.height), (65.546875, 40.0));
        let both = ["x".to_string()];
        let lr = shape_geometry(&input(BlockType::BlockArrow, 35.75, &both));
        assert_eq!(lr.width, 83.75);
    }

    #[test]
    fn 배치가_끝난_사각형과_육각형은_배치_크기로_그린다() {
        let mut hexagon = input(BlockType::Hexagon, 67.421875, &[]);
        hexagon.layout_size = Some((91.421875, 48.0));
        let geometry = shape_geometry(&hexagon);
        assert_eq!((geometry.width, geometry.height), (91.421875, 48.0));
    }
}
