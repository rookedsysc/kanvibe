//! 원인 가지와 그 아래 자손 뼈(drawBranch·flattenTree).
//!
//! 원인은 짝수 번째가 위, 홀수 번째가 아래로 가고 두 개씩 한 쌍으로 왼쪽으로 늘어선다. 다음 쌍은 앞 쌍
//! 글자들의 가장 왼쪽 끝에서 시작한다. 자손은 가지를 따라 고르게 나눈 세로 자리에 놓이는데, 짝수 깊이는 제
//! 자리를 먼저, 홀수 깊이는 자기 자손 뒤에 자리를 받는다.

use crate::scene::Point;
use crate::text::FontWeight;

use super::Canvas;
use super::parser::CauseNode;
use super::text_block::{Anchor, FONT_SIZE, TextStyle, VerticalAlign, wrap_text};

const SPINE_BASE_LENGTH: f64 = 250.0;
/// 자손이 없는 가로 뼈 길이
const BONE_STUB: f64 = 30.0;
/// 자손이 있는 가로 뼈의 기본 길이와 자손 하나당 더하는 길이
const BONE_BASE: f64 = 60.0;
const BONE_PER_CHILD: f64 = 5.0;
/// 원인 가지가 등뼈와 이루는 각도(82°)
const ANGLE_DEGREES: f64 = 82.0;
/// 자손 글자를 감싸는 글자 수
const CAUSE_WRAP_CHARS: usize = 15;
const BRANCH_WIDTH: f64 = 2.0;
const SUB_BONE_WIDTH: f64 = 1.0;

/// 위·아래 가지 길이와 등뼈 높이
pub struct SpineLayout {
    pub upper_length: f64,
    pub lower_length: f64,
    pub spine_y: f64,
}

impl SpineLayout {
    pub fn new(causes: &[CauseNode]) -> Self {
        let upper = side_stats(causes.iter().step_by(2));
        let lower = side_stats(causes.iter().skip(1).step_by(2));
        let total = upper.0 + lower.0;
        let (mut upper_length, mut lower_length) = (SPINE_BASE_LENGTH, SPINE_BASE_LENGTH);
        if total > 0 {
            let pool = SPINE_BASE_LENGTH * 2.0;
            let min_length = SPINE_BASE_LENGTH * 0.3;
            upper_length = min_length.max(pool * (upper.0 as f64 / total as f64));
            lower_length = min_length.max(pool * (lower.0 as f64 / total as f64));
        }
        let min_spacing = FONT_SIZE * 2.0;
        upper_length = upper_length.max(upper.1 as f64 * min_spacing);
        lower_length = lower_length.max(lower.1 as f64 * min_spacing);
        Self {
            upper_length,
            lower_length,
            spine_y: upper_length.max(SPINE_BASE_LENGTH),
        }
    }
}

/// 한쪽 원인들의 자손 수 합과 최댓값
fn side_stats<'a>(causes: impl Iterator<Item = &'a CauseNode>) -> (usize, usize) {
    causes.fold((0, 0), |(total, max), cause| {
        let descendants = count_descendants(cause);
        (total + descendants, max.max(descendants))
    })
}

fn count_descendants(node: &CauseNode) -> usize {
    node.children
        .iter()
        .map(|child| 1 + count_descendants(child))
        .sum()
}

/// 쌍마다 위·아래 가지를 그리고 마지막 쌍 글자의 왼쪽 끝(등뼈 시작 x)을 돌려준다
pub fn draw_pairs(canvas: &mut Canvas, causes: &[CauseNode], layout: &SpineLayout) -> f64 {
    let mut spine_x = -20.0;
    for pair in causes.chunks(2) {
        canvas.pair_text_left = f64::INFINITY;
        let start = Point::new(spine_x, layout.spine_y);
        draw_branch(canvas, &pair[0], start, -1.0, layout.upper_length);
        if let Some(lower) = pair.get(1) {
            draw_branch(canvas, lower, start, 1.0, layout.lower_length);
        }
        spine_x = canvas.pair_text_left;
    }
    spine_x
}

fn draw_branch(canvas: &mut Canvas, cause: &CauseNode, start: Point, direction: f64, length: f64) {
    let angle = ANGLE_DEGREES.to_radians();
    let line_length = match cause.children.is_empty() {
        true => length * 0.2,
        false => length,
    };
    let end = Point::new(
        start.x - angle.cos() * line_length,
        start.y + angle.sin() * line_length * direction,
    );
    canvas.bone(start, end, BRANCH_WIDTH);
    draw_cause_label(canvas, &cause.text, end, direction);
    if !cause.children.is_empty() {
        let root_bone = Bone::new(start, end, cause.children.len());
        draw_sub_bones(canvas, &cause.children, root_bone, direction);
    }
}

/// 원인 글자와 그 둘레 상자(글자 상자보다 좌우 20, 위아래 2 크다). 상자가 글자보다 먼저 놓인다
fn draw_cause_label(canvas: &mut Canvas, text: &str, end: Point, direction: f64) {
    let style = TextStyle {
        font_size: FONT_SIZE,
        weight: FontWeight::Regular,
        anchor: Anchor::Middle,
        align: VerticalAlign::Middle,
    };
    let placed = super::text_block::place_text(
        canvas.fonts,
        text,
        end.x,
        end.y + 11.0 * direction,
        style,
        canvas.text_color,
    );
    let frame = placed.bounds;
    let corners = [
        Point::new(frame.left - 20.0, frame.top - 2.0),
        Point::new(frame.right + 20.0, frame.top - 2.0),
        Point::new(frame.right + 20.0, frame.bottom + 2.0),
        Point::new(frame.left - 20.0, frame.bottom + 2.0),
    ];
    corners
        .iter()
        .for_each(|corner| canvas.bounds.include_point(corner.x, corner.y));
    let fill = canvas.fill_color;
    canvas.push_path(crate::scene::polygon(&corners), Some(fill), BRANCH_WIDTH);
    canvas.push_placed(placed);
}

/// 자손이 달릴 뼈. 홀수 깊이 자손은 뼈 위 `(자손 수 − k) / (자손 수 + 1)` 자리에서 나온다
#[derive(Clone, Copy)]
struct Bone {
    from: Point,
    to: Point,
    child_count: usize,
    children_drawn: usize,
}

impl Bone {
    fn new(from: Point, to: Point, child_count: usize) -> Self {
        Self {
            from,
            to,
            child_count,
            children_drawn: 0,
        }
    }
}

/// flattenTree가 펼친 자손 하나. `parent`는 0이 원인 가지, i + 1이 i번째 자손이다
struct FlatEntry {
    depth: usize,
    text: String,
    parent: usize,
    child_count: usize,
}

fn flatten_tree(children: &[CauseNode], direction: f64) -> (Vec<FlatEntry>, Vec<usize>) {
    let mut entries = Vec::new();
    let mut y_order = Vec::new();
    walk(children, 0, 2, direction, &mut entries, &mut y_order);
    (entries, y_order)
}

fn walk(
    nodes: &[CauseNode],
    parent: usize,
    depth: usize,
    direction: f64,
    entries: &mut Vec<FlatEntry>,
    y_order: &mut Vec<usize>,
) {
    let ordered: Vec<&CauseNode> = match direction < 0.0 {
        true => nodes.iter().rev().collect(),
        false => nodes.iter().collect(),
    };
    for child in ordered {
        let index = entries.len();
        entries.push(FlatEntry {
            depth,
            text: wrap_text(&child.text, CAUSE_WRAP_CHARS),
            parent,
            child_count: child.children.len(),
        });
        let is_even_depth = depth.is_multiple_of(2);
        if is_even_depth {
            y_order.push(index);
        }
        walk(
            &child.children,
            index + 1,
            depth + 1,
            direction,
            entries,
            y_order,
        );
        if !is_even_depth {
            y_order.push(index);
        }
    }
}

fn draw_sub_bones(canvas: &mut Canvas, children: &[CauseNode], root: Bone, direction: f64) {
    let (entries, y_order) = flatten_tree(children, direction);
    let slots = entries.len() as f64 + 1.0;
    let mut ys = vec![0.0; entries.len()];
    for (slot, entry_index) in y_order.into_iter().enumerate() {
        ys[entry_index] = root.from.y + (root.to.y - root.from.y) * ((slot as f64 + 1.0) / slots);
    }
    let mut bones: Vec<Option<Bone>> = vec![None; entries.len() + 1];
    bones[0] = Some(root);
    for (index, entry) in entries.iter().enumerate() {
        let Some(parent) = bones[entry.parent].as_mut() else {
            continue;
        };
        let (from, to) = match entry.depth.is_multiple_of(2) {
            true => draw_level_bone(canvas, parent, entry, ys[index]),
            false => draw_slanted_bone(canvas, parent, entry, ys[index], direction),
        };
        if entry.child_count > 0 {
            bones[index + 1] = Some(Bone::new(from, to, entry.child_count));
        }
    }
}

/// 짝수 깊이: 부모 뼈 위 같은 높이에서 왼쪽으로 가는 가로 뼈와 끝에 붙은 글자(`middle` 맞춤)
fn draw_level_bone(
    canvas: &mut Canvas,
    parent: &Bone,
    entry: &FlatEntry,
    y: f64,
) -> (Point, Point) {
    let rise = parent.to.y - parent.from.y;
    let ratio = match rise == 0.0 {
        true => 0.5,
        false => (y - parent.from.y) / rise,
    };
    let start_x = lerp(parent.from.x, parent.to.x, ratio);
    let length = match entry.child_count {
        0 => BONE_STUB,
        count => BONE_BASE + count as f64 * BONE_PER_CHILD,
    };
    let (from, to) = (Point::new(start_x, y), Point::new(start_x - length, y));
    canvas.bone(from, to, SUB_BONE_WIDTH);
    let style = sub_label_style(VerticalAlign::Middle);
    canvas.text(&entry.text, to.x, y, style);
    (from, to)
}

/// 홀수 깊이: 가로 부모 뼈에서 원인 가지와 나란히 뻗는 사선 뼈. 글자는 위쪽이면 기준선, 아래쪽이면
/// `hanging`으로 뼈 끝에 맞춘다
fn draw_slanted_bone(
    canvas: &mut Canvas,
    parent: &mut Bone,
    entry: &FlatEntry,
    y: f64,
    direction: f64,
) -> (Point, Point) {
    let angle = ANGLE_DEGREES.to_radians();
    let drawn = parent.children_drawn;
    parent.children_drawn += 1;
    let ratio = (parent.child_count - drawn) as f64 / (parent.child_count as f64 + 1.0);
    let from = Point::new(lerp(parent.from.x, parent.to.x, ratio), parent.from.y);
    let to = Point::new(
        from.x - angle.cos() * ((y - from.y) / (angle.sin() * direction)),
        y,
    );
    canvas.bone(from, to, SUB_BONE_WIDTH);
    let align = match direction < 0.0 {
        true => VerticalAlign::Alphabetic,
        false => VerticalAlign::Hanging,
    };
    canvas.text(&entry.text, to.x, y, sub_label_style(align));
    (from, to)
}

fn sub_label_style(align: VerticalAlign) -> TextStyle {
    TextStyle {
        font_size: FONT_SIZE,
        weight: FontWeight::Regular,
        anchor: Anchor::End,
        align,
    }
}

fn lerp(from: f64, to: f64, ratio: f64) -> f64 {
    from + (to - from) * ratio
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(text: &str, children: Vec<CauseNode>) -> CauseNode {
        CauseNode {
            text: text.to_string(),
            children,
        }
    }

    #[test]
    fn 가지_길이는_양쪽_자손_수_비율로_나눈다() {
        // ishikawa-simple: 위(Network 1 + Hardware 0) 1, 아래(Software 2) 2
        let causes = vec![
            node("Network", vec![node("DNS failure", vec![])]),
            node("Software", vec![node("a", vec![]), node("b", vec![])]),
            node("Hardware", vec![]),
        ];
        let layout = SpineLayout::new(&causes);
        assert!((layout.upper_length - 500.0 / 3.0).abs() < 1e-9);
        assert!((layout.lower_length - 1000.0 / 3.0).abs() < 1e-9);
        assert_eq!(layout.spine_y, 250.0);
    }

    #[test]
    fn 홀수_깊이는_자손_뒤에_세로_자리를_받는다() {
        let children = vec![
            node("Manual scheduling", vec![node("No priority rules", vec![])]),
            node("Batch picking", vec![]),
        ];
        let (entries, y_order) = flatten_tree(&children, -1.0);
        assert_eq!(entries[0].text, "Batch picking");
        assert_eq!(entries[1].text, "Manual\nscheduling");
        assert_eq!(entries[2].parent, 2);
        assert_eq!(y_order, vec![0, 1, 2]);
    }
}
