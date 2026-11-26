//! 커밋 모양(drawCommitBullet), 돌린 id 라벨(drawCommitLabel), 태그(drawCommitTags).

use crate::scene::{Color, PathCommand, Point, Rotation, StrokeStyle, TextItem, ellipse, polygon};

use super::layout::{CommitPlace, GraphLayout, LAYOUT_OFFSET};
use super::parse::{Commit, CommitKind, GitGraph};
use super::{COLOR_COUNT, Canvas};

const COMMIT_RADIUS: f64 = 10.0;
const MERGE_INNER_RADIUS: f64 = 6.0;
const REVERSE_CROSS_HALF: f64 = 5.0;
const REVERSE_CROSS_WIDTH: f64 = 3.0;
const CHERRY_MARK_RADIUS: f64 = 2.75;
const LABEL_DEGREES: f64 = -45.0;
const TAG_DEGREES: f64 = 45.0;
/// 라벨·태그 바탕 여백(PX, PY)
const PADDING_X: f64 = 4.0;
const PADDING_Y: f64 = 2.0;
const TAG_SPACING: f64 = 20.0;
const TAG_HOLE_RADIUS: f64 = 1.5;

/// `translate(shift) rotate(degrees, center)`
#[derive(Clone, Copy)]
struct LabelTransform {
    shift: Point,
    degrees: f64,
    center: Point,
}

impl LabelTransform {
    fn apply(&self, point: Point) -> Point {
        let (sin, cos) = self.degrees.to_radians().sin_cos();
        let (dx, dy) = (point.x - self.center.x, point.y - self.center.y);
        Point::new(
            self.center.x + dx * cos - dy * sin + self.shift.x,
            self.center.y + dx * sin + dy * cos + self.shift.y,
        )
    }

    /// 옮기지도 돌리지도 않는다(`rotateCommitLabel: false`)
    fn identity() -> Self {
        Self {
            shift: Point::new(0.0, 0.0),
            degrees: 0.0,
            center: Point::new(0.0, 0.0),
        }
    }

    /// 옮긴 뒤 돌리는 것은 옮긴 중심 둘레로 돌리는 것과 같다
    fn text(&self, mut item: TextItem) -> TextItem {
        item.x += self.shift.x;
        item.baseline += self.shift.y;
        item.rotation = (self.degrees != 0.0).then_some(Rotation {
            degrees: self.degrees,
            center: Point::new(self.center.x + self.shift.x, self.center.y + self.shift.y),
        });
        item
    }
}

/// 사각형 (왼쪽, 위, 폭, 높이)
type BoxRect = (f64, f64, f64, f64);

fn union(first: BoxRect, second: BoxRect) -> BoxRect {
    let left = first.0.min(second.0);
    let top = first.1.min(second.1);
    let right = (first.0 + first.2).max(second.0 + second.2);
    let bottom = (first.1 + first.3).max(second.1 + second.3);
    (left, top, right - left, bottom - top)
}

fn corners((left, top, width, height): BoxRect) -> [Point; 4] {
    [
        Point::new(left, top),
        Point::new(left + width, top),
        Point::new(left + width, top + height),
        Point::new(left, top + height),
    ]
}

impl Canvas<'_> {
    /// 변환한 요소의 getBBox: 지역 테두리 네 귀퉁이를 옮긴 범위
    fn include_transformed(&mut self, local: BoxRect, transform: LabelTransform) {
        for corner in corners(local) {
            self.bounds.include(transform.apply(corner));
        }
    }

    fn fill_shape(&mut self, commands: Vec<PathCommand>, color: Color) {
        let stroke = StrokeStyle {
            color,
            width: 1.0,
            dash: Vec::new(),
        };
        self.path(commands, Some(color), Some(stroke));
    }
}

/// 모양을 모두 그린 뒤 라벨과 태그를 그린다(commit-bullets 묶음이 commit-labels보다 앞이다)
pub fn draw_commits(canvas: &mut Canvas, graph: &GitGraph, layout: &GraphLayout) {
    let ordered = ordered_commits(graph, layout);
    for (commit, place) in &ordered {
        let color_index = layout.branch(&commit.branch).index % COLOR_COUNT;
        draw_bullet(canvas, commit, place.center, color_index);
    }
    for (commit, place) in &ordered {
        if graph.options.show_commit_label && has_visible_label(commit) {
            draw_commit_label(
                canvas,
                commit,
                *place,
                layout,
                graph.options.rotate_commit_label,
            );
        }
        draw_tags(canvas, commit, *place, layout);
    }
}

fn ordered_commits<'a>(
    graph: &'a GitGraph,
    layout: &GraphLayout,
) -> Vec<(&'a Commit, CommitPlace)> {
    let mut ordered: Vec<(&Commit, CommitPlace)> = graph
        .commits
        .iter()
        .filter_map(|commit| Some((commit, layout.commit(&commit.id)?)))
        .collect();
    if layout.direction == super::Direction::BottomToTop {
        ordered.reverse();
    }
    ordered
}

/// 체리픽과 id를 주지 않은 병합은 라벨이 없다
fn has_visible_label(commit: &Commit) -> bool {
    match commit.kind {
        CommitKind::CherryPick => false,
        CommitKind::Merge => commit.has_custom_merge_id,
        _ => true,
    }
}

fn draw_bullet(canvas: &mut Canvas, commit: &Commit, center: Point, color_index: usize) {
    let (x, y) = (center.x, center.y);
    let primary = canvas.colors.primary;
    match commit.symbol {
        CommitKind::Highlight => {
            let outer = square(center, COMMIT_RADIUS);
            canvas.fill_shape(outer, canvas.colors.highlight[color_index]);
            canvas.fill_shape(square(center, MERGE_INNER_RADIUS), primary);
        }
        CommitKind::CherryPick => draw_cherry_pick(canvas, center),
        symbol => {
            let circle = ellipse(center, COMMIT_RADIUS, COMMIT_RADIUS);
            canvas.fill_shape(circle, canvas.colors.branch[color_index]);
            if symbol == CommitKind::Merge {
                let inner = ellipse(center, MERGE_INNER_RADIUS, MERGE_INNER_RADIUS);
                canvas.fill_shape(inner, primary);
            }
            if symbol == CommitKind::Reverse {
                let half = REVERSE_CROSS_HALF;
                let cross = vec![
                    PathCommand::MoveTo(Point::new(x - half, y - half)),
                    PathCommand::LineTo(Point::new(x + half, y + half)),
                    PathCommand::MoveTo(Point::new(x - half, y + half)),
                    PathCommand::LineTo(Point::new(x + half, y - half)),
                ];
                let stroke = StrokeStyle {
                    color: primary,
                    width: REVERSE_CROSS_WIDTH,
                    dash: Vec::new(),
                };
                canvas.path(cross, None, Some(stroke));
            }
        }
    }
}

fn square(center: Point, half: f64) -> Vec<PathCommand> {
    polygon(&corners((
        center.x - half,
        center.y - half,
        half * 2.0,
        half * 2.0,
    )))
}

/// 체리픽: 글자색 원 위에 두 점과 V자 표시. 원은 `.commitN`이 없어 루트 채움색을 물려받는다
fn draw_cherry_pick(canvas: &mut Canvas, center: Point) {
    let (x, y) = (center.x, center.y);
    let circle = ellipse(center, COMMIT_RADIUS, COMMIT_RADIUS);
    canvas.path(circle, Some(canvas.colors.text), None);
    let mark = canvas.colors.cherry_mark;
    for dot_x in [x - 3.0, x + 3.0] {
        let dot = ellipse(
            Point::new(dot_x, y + 2.0),
            CHERRY_MARK_RADIUS,
            CHERRY_MARK_RADIUS,
        );
        canvas.path(dot, Some(mark), None);
    }
    for foot_x in [x + 3.0, x - 3.0] {
        let line = vec![
            PathCommand::MoveTo(Point::new(foot_x, y + 1.0)),
            PathCommand::LineTo(Point::new(x, y - 5.0)),
        ];
        let stroke = StrokeStyle {
            color: mark,
            width: 1.0,
            dash: Vec::new(),
        };
        canvas.path(line, None, Some(stroke));
    }
}

/// drawCommitLabel: 반투명 바탕 위 10px id. LR은 묶음째 옮겨 -45° 돌리고, 세로는 원 왼쪽에서 각자 돌린다
fn draw_commit_label(
    canvas: &mut Canvas,
    commit: &Commit,
    place: CommitPlace,
    layout: &GraphLayout,
    is_rotated: bool,
) {
    let font_size = canvas.colors.commit_label_font_size;
    let width = canvas.text_width(&commit.id, font_size);
    let metrics = canvas.fonts.primary_metrics(font_size);
    let height = metrics.ascent + metrics.descent;
    let is_vertical = layout.direction.is_vertical();
    let geometry = label_geometry(width, height, place, is_vertical);
    let transform = if is_rotated {
        geometry.transform
    } else {
        LabelTransform::identity()
    };
    let rect = corners(geometry.background).map(|corner| transform.apply(corner));
    let background_color = canvas.colors.commit_label_background;
    canvas
        .scene
        .push_path(polygon(&rect), Some(background_color), None);
    let color = canvas.colors.commit_label_text;
    let item = canvas.text_item(
        &commit.id,
        geometry.text_left,
        geometry.baseline,
        font_size,
        color,
    );
    let text_box = canvas.text_box(&item);
    // LR은 바탕과 글자를 묶은 g 하나를, 세로는 둘을 따로 돌린다
    if is_vertical {
        canvas.include_transformed(geometry.background, transform);
        canvas.include_transformed(text_box, transform);
    } else {
        canvas.include_transformed(union(geometry.background, text_box), transform);
    }
    canvas.scene.push_text(transform.text(item));
}

/// 돌리기 전 라벨 바탕·글자 자리와 돌림
struct LabelGeometry {
    background: BoxRect,
    text_left: f64,
    baseline: f64,
    transform: LabelTransform,
}

/// LR은 커밋 아래 가운데에 두고 묶음째 옮겨 돌린다. 세로는 원 왼쪽에 두고 원 중심 둘레로 돌린다
fn label_geometry(width: f64, height: f64, place: CommitPlace, is_vertical: bool) -> LabelGeometry {
    let (x, y) = (place.center.x, place.center.y);
    let size = (width + 2.0 * PADDING_Y, height + 2.0 * PADDING_Y);
    if is_vertical {
        return LabelGeometry {
            background: (
                x - (width + 4.0 * PADDING_X + 5.0),
                y - 12.0,
                size.0,
                size.1,
            ),
            text_left: x - (width + 4.0 * PADDING_X),
            baseline: y + height - 12.0,
            transform: LabelTransform {
                shift: Point::new(0.0, 0.0),
                degrees: LABEL_DEGREES,
                center: place.center,
            },
        };
    }
    let along = place.pos + LAYOUT_OFFSET;
    LabelGeometry {
        background: (along - width / 2.0 - PADDING_Y, y + 13.5, size.0, size.1),
        text_left: along - width / 2.0,
        baseline: y + 25.0,
        transform: LabelTransform {
            shift: Point::new(
                -7.5 - (width + 10.0) / 25.0 * 9.5,
                10.0 + width / 25.0 * 8.5,
            ),
            degrees: LABEL_DEGREES,
            center: Point::new(place.pos, y),
        },
    }
}

/// drawCommitTags: 태그를 거꾸로 20씩 위로 쌓고, 가장 큰 태그 크기로 뾰족한 이름표와 구멍을 그린다
fn draw_tags(canvas: &mut Canvas, commit: &Commit, place: CommitPlace, layout: &GraphLayout) {
    let tags: Vec<&String> = commit.tags.iter().rev().collect();
    let widths: Vec<f64> = tags
        .iter()
        .map(|tag| canvas.text_width(tag, canvas.colors.tag_label_font_size))
        .collect();
    let max_width = widths.iter().copied().fold(0.0, f64::max);
    let metrics = canvas
        .fonts
        .primary_metrics(canvas.colors.tag_label_font_size);
    let half_height = (metrics.ascent + metrics.descent) / 2.0;
    for (index, (tag, width)) in tags.into_iter().zip(widths).enumerate() {
        let shape = TagShape {
            offset: index as f64 * TAG_SPACING,
            max_width,
            half_height,
        };
        if layout.direction.is_vertical() {
            draw_vertical_tag(canvas, tag, place, &shape);
        } else {
            draw_horizontal_tag(canvas, tag, width, place, &shape);
        }
    }
}

struct TagShape {
    offset: f64,
    max_width: f64,
    half_height: f64,
}

fn tag_stroke(canvas: &Canvas) -> StrokeStyle {
    StrokeStyle {
        color: canvas.colors.tag_border,
        width: 1.0,
        dash: Vec::new(),
    }
}

/// LR 태그: 커밋 위 19.2(+20·순번)를 가운데로 하는 이름표. 왼쪽 뾰족한 끝에 구멍이 있다
fn draw_horizontal_tag(
    canvas: &mut Canvas,
    tag: &str,
    width: f64,
    place: CommitPlace,
    shape: &TagShape,
) {
    let (pos, along, y) = (place.pos, place.pos + LAYOUT_OFFSET, place.center.y);
    let (half_width, half_height) = (shape.max_width / 2.0, shape.half_height);
    let middle = y - 19.2 - shape.offset;
    let points = [
        Point::new(pos - half_width - PADDING_X / 2.0, middle + PADDING_Y),
        Point::new(pos - half_width - PADDING_X / 2.0, middle - PADDING_Y),
        Point::new(
            along - half_width - PADDING_X,
            middle - half_height - PADDING_Y,
        ),
        Point::new(
            along + half_width + PADDING_X,
            middle - half_height - PADDING_Y,
        ),
        Point::new(
            along + half_width + PADDING_X,
            middle + half_height + PADDING_Y,
        ),
        Point::new(
            along - half_width - PADDING_X,
            middle + half_height + PADDING_Y,
        ),
    ];
    let stroke = tag_stroke(canvas);
    canvas.path(
        polygon(&points),
        Some(canvas.colors.tag_background),
        Some(stroke),
    );
    let hole_center = Point::new(pos - half_width + PADDING_X / 2.0, middle);
    let hole = ellipse(hole_center, TAG_HOLE_RADIUS, TAG_HOLE_RADIUS);
    canvas.path(hole, Some(canvas.colors.text), None);
    let color = canvas.colors.tag_text;
    canvas.text(
        tag,
        along - width / 2.0,
        y - 16.0 - shape.offset,
        canvas.colors.tag_label_font_size,
        color,
    );
}

/// 세로 태그: 커밋 오른쪽으로 뻗은 이름표를 커밋 자리 둘레로 45° 돌린다
fn draw_vertical_tag(canvas: &mut Canvas, tag: &str, place: CommitPlace, shape: &TagShape) {
    let (x, pos) = (place.center.x, place.pos);
    let origin = pos + shape.offset;
    let shape_transform = LabelTransform {
        shift: Point::new(12.0, 12.0),
        degrees: TAG_DEGREES,
        center: Point::new(x, pos),
    };
    draw_turned_tag_shape(canvas, Point::new(x, origin), shape, shape_transform);
    let text_transform = LabelTransform {
        shift: Point::new(14.0, 14.0),
        ..shape_transform
    };
    let color = canvas.colors.tag_text;
    let font_size = canvas.colors.tag_label_font_size;
    let item = canvas.text_item(tag, x + 5.0, origin + 3.0, font_size, color);
    let text_box = canvas.text_box(&item);
    canvas.include_transformed(text_box, text_transform);
    canvas.scene.push_text(text_transform.text(item));
}

/// 세로 태그의 이름표와 구멍. `anchor`는 (커밋 x, 태그 기준 y)이고 오른쪽으로 뻗는다
fn draw_turned_tag_shape(
    canvas: &mut Canvas,
    anchor: Point,
    shape: &TagShape,
    transform: LabelTransform,
) {
    let (x, origin, half_height) = (anchor.x, anchor.y, shape.half_height);
    let right = x + LAYOUT_OFFSET + shape.max_width + 4.0;
    let (top, bottom) = (origin - half_height - 2.0, origin + half_height + 2.0);
    let points = [
        Point::new(x, origin + 2.0),
        Point::new(x, origin - 2.0),
        Point::new(x + LAYOUT_OFFSET, top),
        Point::new(right, top),
        Point::new(right, bottom),
        Point::new(x + LAYOUT_OFFSET, bottom),
    ];
    canvas.include_transformed((x, top, right - x, bottom - top), transform);
    let turned = points.map(|point| transform.apply(point));
    let stroke = tag_stroke(canvas);
    let background = canvas.colors.tag_background;
    canvas
        .scene
        .push_path(polygon(&turned), Some(background), Some(stroke));
    let hole_center = Point::new(x + PADDING_X / 2.0, origin);
    let diameter = TAG_HOLE_RADIUS * 2.0;
    let hole_box = (
        hole_center.x - TAG_HOLE_RADIUS,
        origin - TAG_HOLE_RADIUS,
        diameter,
        diameter,
    );
    canvas.include_transformed(hole_box, transform);
    let hole = ellipse(
        transform.apply(hole_center),
        TAG_HOLE_RADIUS,
        TAG_HOLE_RADIUS,
    );
    canvas.scene.push_path(hole, Some(canvas.colors.text), None);
}
