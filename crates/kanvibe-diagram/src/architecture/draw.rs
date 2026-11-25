//! architecture svgDraw.ts: 간선 → 서비스·접점 → 그룹 순서로 그린 뒤 전체 getBBox에 여백(`padding`)을 둘러
//! viewBox를 잡는다.
//!
//! 서비스는 노드 위치를 왼쪽 위로 삼아 iconSize 크기 아이콘을 그리고(그래서 간선 끝점 `0 50%`가 아이콘 왼쪽
//! 가운데에 온다), 그 아래 가운데에 제목을 쓴다. 글자는 `iconText`만 빼고 모두 SVG `<text>`(createText
//! `useHtmlLabels: false`)라 라벨 방식과 상관없이 같은 장면이 나온다. 서비스·곧은 간선 라벨은
//! `dominant-baseline: middle`, 그룹·꺾인 간선 라벨은 알파벳 기준선이다.

use super::Arrangement;
use super::config::ArchitectureConfig;
use super::cytoscape::{Bounds, CyElement, CyGraph};
use super::icon_text::icon_text_block;
use super::icons::{Icon, draw_icon};
use super::parser::{Architecture, ArchitectureEdge, NodeKind, Side};
use crate::RenderContext;
use crate::label::{SVG_LINE_HEIGHT_EM, measure_svg_label};
use crate::scene::{Color, PathCommand, Point, Rotation, Scene, StrokeStyle, TextItem, polygon};
use crate::text::{FontBook, FontWeight};
use crate::theme::{Theme, hsl};
use crate::theme_variables::ThemeVariables;

/// 라벨 글자 크기는 CSS(16px)를 물려받는다
const FONT_SIZE: f64 = 16.0;
/// `dominant-baseline: middle`은 기준선을 Noto Sans x-height(0.536em)의 절반만큼 내린다
const MIDDLE_BASELINE_SHIFT: f64 = 0.536 * FONT_SIZE / 2.0;
const EDGE_WIDTH: f64 = 3.0;
const GROUP_BORDER_WIDTH: f64 = 2.0;
const GROUP_DASH: f64 = 8.0;
/// 그룹 아이콘 크기(× padding)
const GROUP_ICON_RATIO: f64 = 0.75;
/// `{group}` 간선 끝을 그룹 밖으로 빼는 양(padding + 4)
const GROUP_EDGE_EXTRA_SHIFT: f64 = 4.0;
/// 그룹 아래쪽으로 나가는 `{group}` 간선은 그룹 라벨 높이만큼 더 내려간다
const GROUP_EDGE_BOTTOM_EXTRA: f64 = 18.0;
/// 꺾인 간선 라벨을 돌리는 각도
const BEND_LABEL_DEGREES: f64 = 45.0;

struct ArchitecturePalette {
    text: Color,
    edge: Color,
    arrow: Color,
    group_border: Color,
    edge_width: f64,
    group_border_width: f64,
}

fn palette_for(theme: Theme) -> ArchitecturePalette {
    let (text, edge, group_border) = match theme {
        Theme::Light => (
            Color::rgb(0x33, 0x33, 0x33),
            Color::rgb(0x33, 0x33, 0x33),
            hsl(240.0, 60.0, 86.274_509_803_9),
        ),
        Theme::Dark => (
            Color::rgb(0xCC, 0xCC, 0xCC),
            Color::rgb(211, 211, 211),
            Color::rgb(0xCC, 0xCC, 0xCC),
        ),
    };
    ArchitecturePalette {
        text,
        edge,
        arrow: edge,
        group_border,
        edge_width: EDGE_WIDTH,
        group_border_width: GROUP_BORDER_WIDTH,
    }
}

/// architectureStyles.ts: `.edge`는 `archEdgeColor`·`archEdgeWidth`, `.arrow`는 `archEdgeArrowColor`, 그룹 테두리는
/// `archGroupBorderColor`·`archGroupBorderWidth`이고 라벨 글자는 루트 `fill`(`textColor`)을 물려받는다
fn palette_with_variables(theme: Theme, variables: &ThemeVariables) -> ArchitecturePalette {
    let mut palette = palette_for(theme);
    variables.read_color("textColor", &mut palette.text);
    variables.read_color("archEdgeColor", &mut palette.edge);
    variables.read_color("archEdgeArrowColor", &mut palette.arrow);
    variables.read_color("archGroupBorderColor", &mut palette.group_border);
    if let Some(width) = variables.css_length("archEdgeWidth") {
        palette.edge_width = width;
    }
    if let Some(width) = variables.css_length("archGroupBorderWidth") {
        palette.group_border_width = width;
    }
    palette
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Anchor {
    Start,
    Middle,
}

/// 라벨 줄을 놓는 방식: 가로 기준(`text-anchor`)과 기준선을 내리는 양
type LabelStyle = (Anchor, f64);

/// 그리는 동안 장면과 getBBox 범위를 함께 모은다
struct Canvas<'a> {
    scene: Scene,
    bounds: Bounds,
    fonts: &'a FontBook,
    palette: ArchitecturePalette,
    config: ArchitectureConfig,
}

pub(crate) fn draw(
    architecture: &Architecture,
    graph: &CyGraph,
    arrangement: &Arrangement,
    context: &RenderContext,
) -> Scene {
    let centers = &arrangement.centers;
    let mut canvas = Canvas {
        scene: Scene::default(),
        bounds: Bounds::EMPTY,
        fonts: context.fonts,
        palette: palette_with_variables(context.palette.theme, &context.theme_variables),
        config: graph.config,
    };
    let edges = architecture.edges.iter().zip(&graph.edges);
    for ((edge, cy_edge), &segment) in edges.zip(&arrangement.segments) {
        let midpoint = graph.edge_midpoint(cy_edge, centers, segment);
        let start = graph.endpoint(centers[cy_edge.source], edge.source_side);
        let end = graph.endpoint(centers[cy_edge.target], edge.target_side);
        canvas.draw_edge(architecture, edge, (start, midpoint, end));
    }
    for kind in [NodeKind::Service, NodeKind::Junction] {
        for (position, node) in graph.nodes.iter().enumerate() {
            let CyElement::Node(index) = node.element else {
                continue;
            };
            if architecture.nodes[index].kind == kind {
                canvas.draw_node(architecture, index, centers[position]);
            }
        }
    }
    let rects = graph.outer_rects(centers);
    for (position, node) in graph.nodes.iter().enumerate() {
        if let CyElement::Group(index) = node.element {
            let bounding_box = graph.node_bounding_box(position, &rects, centers);
            canvas.draw_group(architecture, index, bounding_box);
        }
    }
    canvas.finish()
}

impl Canvas<'_> {
    fn include(&mut self, bounds: Bounds) {
        self.bounds = self.bounds.union(bounds);
    }

    /// createText 라벨 한 덩이를 놓는다. 줄마다 `tspan`(y = (1.1·i − 0.1)em, dy = 1.1em)이라 i번째 기준선이
    /// 원점 아래 (1 + 1.1·i)em에 기준선 이동을 더한 곳이다. 돌리지 않은 글자와 글자 상자를 돌려준다
    fn lay_out_label(
        &self,
        label: &str,
        wrap_width: f64,
        origin: Point,
        (anchor, baseline_shift): LabelStyle,
    ) -> (Vec<TextItem>, Bounds) {
        let measured = measure_svg_label(self.fonts, label, FONT_SIZE, wrap_width);
        let metrics = self.fonts.primary_metrics(FONT_SIZE);
        let mut bounds = Bounds::EMPTY;
        let mut items = Vec::new();
        for (index, line) in measured.lines.iter().enumerate() {
            let baseline =
                origin.y + FONT_SIZE * (1.0 + SVG_LINE_HEIGHT_EM * index as f64) + baseline_shift;
            let extent = line_extent(self.fonts, &line.text);
            let start = match anchor {
                Anchor::Start => origin.x,
                Anchor::Middle => origin.x - extent.advance / 2.0,
            };
            bounds = bounds.union(Bounds {
                x1: start + extent.left,
                y1: baseline - metrics.ascent,
                x2: start + extent.right,
                y2: baseline + metrics.descent,
            });
            if !line.text.is_empty() {
                items.push(TextItem {
                    text: line.text.clone(),
                    x: start,
                    baseline,
                    font_size: FONT_SIZE,
                    weight: FontWeight::Regular,
                    color: self.palette.text,
                    rotation: None,
                });
            }
        }
        (items, bounds)
    }

    /// 라벨을 그리고 (돌린 뒤의) 글자 상자를 getBBox 범위에 넣는다
    fn write_label(
        &mut self,
        label: &str,
        wrap_width: f64,
        origin: Point,
        style: LabelStyle,
        rotation: Option<Rotation>,
    ) {
        let (items, bounds) = self.lay_out_label(label, wrap_width, origin, style);
        for mut item in items {
            item.rotation = rotation;
            self.scene.push_text(item);
        }
        self.include(rotation.map_or(bounds, |rotation| rotated_bounds(bounds, rotation)));
    }

    fn draw_node(&mut self, architecture: &Architecture, index: usize, position: (f64, f64)) {
        let node = &architecture.nodes[index];
        let (left, top) = position;
        let size = self.config.icon_size;
        self.include(Bounds {
            x1: left,
            y1: top,
            x2: left + size,
            y2: top + size,
        });
        if node.kind == NodeKind::Junction {
            return;
        }
        if let Some(title) = &node.title {
            let origin = Point::new(left + size / 2.0, top + size);
            let style = (Anchor::Middle, MIDDLE_BASELINE_SHIFT);
            self.write_label(title, size * 1.5, origin, style, None);
        }
        match (&node.icon, &node.icon_text) {
            (Some(icon), _) => draw_icon(&mut self.scene, Icon::named(icon), left, top, size),
            (None, Some(text)) => {
                draw_icon(&mut self.scene, Icon::Blank, left, top, size);
                let block = icon_text_block(self.fonts, text, (left, top), size);
                self.scene.items.push(block);
            }
            (None, None) => self.draw_empty_service(left, top),
        }
    }

    /// 아이콘이 없는 서비스: 위 모서리만 둥근 점선 테두리(`node-bkg`)
    fn draw_empty_service(&mut self, left: f64, top: f64) {
        let at = |x: f64, y: f64| Point::new(left + x, top + y);
        let size = self.config.icon_size;
        let commands = vec![
            PathCommand::MoveTo(at(0.0, size)),
            PathCommand::LineTo(at(0.0, 5.0)),
            PathCommand::QuadTo(at(0.0, 0.0), at(5.0, 0.0)),
            PathCommand::LineTo(at(size - 5.0, 0.0)),
            PathCommand::QuadTo(at(size, 0.0), at(size, 5.0)),
            PathCommand::LineTo(at(size, size)),
            PathCommand::Close,
        ];
        let stroke = self.group_stroke();
        self.scene.push_path(commands, None, Some(stroke));
    }

    fn group_stroke(&self) -> StrokeStyle {
        StrokeStyle {
            color: self.palette.group_border,
            width: self.palette.group_border_width,
            dash: vec![GROUP_DASH, GROUP_DASH],
        }
    }

    fn draw_group(&mut self, architecture: &Architecture, index: usize, bounding_box: Bounds) {
        let group = &architecture.groups[index];
        let half_icon = self.config.icon_size / 2.0;
        let (x1, y1) = (bounding_box.x1, bounding_box.y1);
        let rect = Bounds {
            x1: x1 + half_icon,
            y1: y1 + half_icon,
            x2: x1 + half_icon + bounding_box.width(),
            y2: y1 + half_icon + bounding_box.height(),
        };
        let corners = [
            Point::new(rect.x1, rect.y1),
            Point::new(rect.x2, rect.y1),
            Point::new(rect.x2, rect.y2),
            Point::new(rect.x1, rect.y2),
        ];
        let stroke = self.group_stroke();
        self.scene.push_path(polygon(&corners), None, Some(stroke));
        self.include(rect);
        let (mut label_x, mut label_y) = (x1, y1);
        if let Some(icon) = &group.icon {
            let icon_size = self.config.padding * GROUP_ICON_RATIO;
            let (icon_left, icon_top) = (x1 + half_icon + 1.0, y1 + half_icon + 1.0);
            draw_icon(
                &mut self.scene,
                Icon::named(icon),
                icon_left,
                icon_top,
                icon_size,
            );
            self.include(Bounds {
                x1: icon_left,
                y1: icon_top,
                x2: icon_left + icon_size,
                y2: icon_top + icon_size,
            });
            label_x += icon_size;
            label_y += self.config.font_size / 2.0 - 1.0 - 2.0;
        }
        if let Some(title) = &group.title {
            let origin = Point::new(label_x + half_icon + 4.0, label_y + half_icon + 2.0);
            self.write_label(
                title,
                bounding_box.width(),
                origin,
                (Anchor::Start, 0.0),
                None,
            );
        }
    }

    /// `points`는 cytoscape 시작 끝점·가운데 점·끝 끝점
    fn draw_edge(
        &mut self,
        architecture: &Architecture,
        edge: &ArchitectureEdge,
        points: ((f64, f64), (f64, f64), (f64, f64)),
    ) {
        let is_junction = |id: &str| {
            architecture
                .node(id)
                .is_some_and(|node| node.kind == NodeKind::Junction)
        };
        let start = self.adjusted_end(
            points.0,
            edge.source_side,
            edge.is_source_group,
            is_junction(&edge.source),
        );
        let end = self.adjusted_end(
            points.2,
            edge.target_side,
            edge.is_target_group,
            is_junction(&edge.target),
        );
        let middle = Point::new(points.1.0, points.1.1);
        self.draw_edge_line([start, middle, end]);
        if edge.has_source_arrow {
            self.draw_arrow(start, edge.source_side);
        }
        if edge.has_target_arrow {
            self.draw_arrow(end, edge.target_side);
        }
        if let Some(title) = &edge.title {
            let sides = (edge.source_side, edge.target_side);
            self.draw_edge_label(title, (start, middle, end), sides);
        }
    }

    /// `{group}` 간선은 그룹 밖으로, 접점 간선은 접점 가운데로 끝을 옮긴다
    fn adjusted_end(
        &self,
        endpoint: (f64, f64),
        side: Side,
        is_group: bool,
        is_junction: bool,
    ) -> Point {
        let (mut x, mut y) = endpoint;
        let half_icon = self.config.icon_size / 2.0;
        let group_shift = self.config.padding + GROUP_EDGE_EXTRA_SHIFT;
        if is_group {
            match side {
                Side::Left => x -= group_shift,
                Side::Right => x += group_shift,
                Side::Top => y -= group_shift,
                Side::Bottom => y += group_shift + GROUP_EDGE_BOTTOM_EXTRA,
            }
        } else if is_junction {
            match side {
                Side::Left => x += half_icon,
                Side::Right => x -= half_icon,
                Side::Top => y += half_icon,
                Side::Bottom => y -= half_icon,
            }
        }
        Point::new(x, y)
    }

    /// `M 시작 L 가운데 L 끝` 굵기 3 선
    fn draw_edge_line(&mut self, points: [Point; 3]) {
        let [start, middle, end] = points;
        let commands = vec![
            PathCommand::MoveTo(start),
            PathCommand::LineTo(middle),
            PathCommand::LineTo(end),
        ];
        let stroke = StrokeStyle {
            color: self.palette.edge,
            width: self.palette.edge_width,
            dash: Vec::new(),
        };
        self.scene.push_path(commands, None, Some(stroke));
        for point in points {
            self.include(Bounds::around((point.x, point.y), 0.0, 0.0));
        }
    }

    /// ArchitectureDirectionArrow 삼각형(iconSize / 6)을 ArchitectureDirectionArrowShift만큼 옮겨 끝점에 붙인다
    fn draw_arrow(&mut self, tip: Point, side: Side) {
        let size = self.config.icon_size / 6.0;
        let half = size / 2.0;
        let shift = |origin: f64| match side {
            Side::Left | Side::Top => origin - size + 2.0,
            Side::Right | Side::Bottom => origin - 2.0,
        };
        let (left, top) = if side.is_horizontal() {
            (shift(tip.x), tip.y - half)
        } else {
            (tip.x - half, shift(tip.y))
        };
        let local: [(f64, f64); 3] = match side {
            Side::Left => [(size, half), (0.0, size), (0.0, 0.0)],
            Side::Right => [(0.0, half), (size, 0.0), (size, size)],
            Side::Top => [(0.0, 0.0), (size, 0.0), (half, size)],
            Side::Bottom => [(half, 0.0), (size, size), (0.0, size)],
        };
        let points: Vec<Point> = local
            .iter()
            .map(|&(x, y)| Point::new(left + x, top + y))
            .collect();
        for point in &points {
            self.include(Bounds::around((point.x, point.y), 0.0, 0.0));
        }
        self.scene
            .push_path(polygon(&points), Some(self.palette.arrow), None);
    }

    /// 간선 라벨: 가로 간선은 가운데 점에, 세로 간선은 −90° 돌려서, 꺾인 간선은 ±45° 돌려서 쓴다
    fn draw_edge_label(&mut self, title: &str, points: (Point, Point, Point), sides: (Side, Side)) {
        let (start, middle, end) = points;
        let is_bend = sides.0.is_horizontal() != sides.1.is_horizontal();
        let style = (Anchor::Middle, MIDDLE_BASELINE_SHIFT);
        if is_bend {
            self.draw_bend_label(title, points, sides);
        } else if sides.0.is_horizontal() {
            self.write_label(title, (start.x - end.x).abs(), middle, style, None);
        } else {
            let rotation = Rotation {
                degrees: -90.0,
                center: middle,
            };
            let wrap = (start.y - end.y).abs() / 1.5;
            self.write_label(title, wrap, middle, style, Some(rotation));
        }
    }

    /// 꺾인 간선 라벨. 가운데 기준선 상자 높이 h를 재고, 알파벳 기준선으로 바꿔 원점에서 돌린 상자의 폭·높이
    /// W·H로 `translate(mid.x + x·W/2, mid.y − h/2 + y·H/2) rotate(−x·y·45, 0, h/2)`에 둔다
    fn draw_bend_label(&mut self, title: &str, points: (Point, Point, Point), sides: (Side, Side)) {
        let (start, middle, end) = points;
        let (factor_x, factor_y) = bend_factors(sides);
        let degrees = -factor_x * factor_y * BEND_LABEL_DEGREES;
        let wrap = (start.x - end.x).abs() / 2.0;
        let origin_style = (Anchor::Middle, 0.0);
        let (_, local) = self.lay_out_label(title, wrap, Point::new(0.0, 0.0), origin_style);
        let original_height = local.height();
        let pivot = Point::new(0.0, 0.0);
        let turned = rotated_bounds(
            local,
            Rotation {
                degrees,
                center: pivot,
            },
        );
        let origin = Point::new(
            middle.x + factor_x * turned.width() / 2.0,
            middle.y - original_height / 2.0 + factor_y * turned.height() / 2.0,
        );
        let rotation = Rotation {
            degrees,
            center: Point::new(origin.x, origin.y + original_height / 2.0),
        };
        self.write_label(title, wrap, origin, origin_style, Some(rotation));
    }

    fn finish(mut self) -> Scene {
        let padding = self.config.padding;
        let as_float = |value: f64| f64::from(value as f32);
        let (x, y) = (as_float(self.bounds.x1), as_float(self.bounds.y1));
        let width = as_float(as_float(self.bounds.x2) - x);
        let height = as_float(as_float(self.bounds.y2) - y);
        let (left, top) = (as_float(x - padding), as_float(y - padding));
        self.scene.width = as_float(width + padding * 2.0);
        self.scene.height = as_float(height + padding * 2.0);
        self.scene.translate(-left, -top);
        self.scene
    }
}

/// getArchitectureDirectionXYFactors
fn bend_factors(sides: (Side, Side)) -> (f64, f64) {
    let pair = [sides.0.letter(), sides.1.letter()];
    match pair {
        ['L', 'T'] | ['T', 'L'] => (1.0, 1.0),
        ['B', 'L'] | ['L', 'B'] => (1.0, -1.0),
        ['B', 'R'] | ['R', 'B'] => (-1.0, -1.0),
        _ => (-1.0, 1.0),
    }
}

/// 단어마다 `tspan`을 둔 한 줄의 SVG 글자 범위(label.rs와 같은 나눔)
fn line_extent(fonts: &FontBook, line: &str) -> crate::text::SvgTextExtent {
    let runs: Vec<String> = line
        .split(' ')
        .filter(|word| !word.is_empty())
        .enumerate()
        .map(|(index, word)| {
            if index == 0 {
                word.to_string()
            } else {
                format!(" {word}")
            }
        })
        .collect();
    let runs: Vec<&str> = runs.iter().map(String::as_str).collect();
    fonts.measure_svg_runs(&runs, FONT_SIZE, FontWeight::Regular)
}

/// 상자를 `rotation`으로 돌린 뒤의 축 정렬 상자
fn rotated_bounds(bounds: Bounds, rotation: Rotation) -> Bounds {
    let radians = rotation.degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let center = rotation.center;
    let corners = [
        (bounds.x1, bounds.y1),
        (bounds.x2, bounds.y1),
        (bounds.x2, bounds.y2),
        (bounds.x1, bounds.y2),
    ];
    corners.iter().fold(Bounds::EMPTY, |rotated, &(x, y)| {
        let (dx, dy) = (x - center.x, y - center.y);
        let point = (
            center.x + dx * cos - dy * sin,
            center.y + dx * sin + dy * cos,
        );
        rotated.union(Bounds::around(point, 0.0, 0.0))
    })
}
