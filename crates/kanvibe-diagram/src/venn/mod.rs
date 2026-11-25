//! venn-beta 다이어그램(vennRenderer.ts와 @upsetjs/venn.js `VennDiagram`).
//!
//! 기본 800×450 그림에서 제목(있으면 위 24px)을 빼고 남은 상자에 venn.js 배치(여백 15)로 원을 놓는다. 영역은
//! 정의 순서대로 원(집합 하나) 또는 겹침 경로와 라벨을 그린다. 원은 테마 색 `venn1..8`을 10% 불투명도로 채우고,
//! 겹침은 `style`로 `fill`을 준 경우에만 채운다. 라벨은 venn.js가 분리된 DOM에서 그려 글자 폭이 0으로 재지므로
//! 줄바꿈 없이 한 줄이다. `text`로 적은 글자 상자는 [`text_nodes`]가 영역 안 격자에 HTML 글자로 둔다. 두 글자 모두
//! `htmlLabels` 설정과 상관없이 그려지므로 라벨 방식과 상관없이 같은 장면이 나온다.

mod config;
mod geometry;
mod layout;
mod nelder_mead;
mod parser;
mod text_centre;
mod text_nodes;

use std::collections::HashMap;
use std::f64::consts::PI;

use crate::chart_text::{AnchoredText, BaselineAnchor, TextAnchor, push_anchored_text};
use crate::directive::read_config;
use crate::sanitize::sanitize_text;
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, ellipse};
use crate::text::FontBook;
use crate::{DiagramError, RenderContext};

use config::{VennConfig, VennPalette, adjust_lightness};
use geometry::{Arc, Circle, intersection_area};
use layout::{AreaSize, Solution};
use parser::{Subset, TextNode, VennChart};
use text_nodes::{TextNodeStyle, draw_text_nodes};

/// 글자·선 크기를 정하는 기준 폭(REFERENCE_WIDTH). 배율은 그림 폭 ÷ 이 값이다
const REFERENCE_WIDTH: f64 = 1600.0;
/// venn.js VennDiagram 기본 여백
const DIAGRAM_PADDING: f64 = 15.0;
const CIRCLE_FILL_OPACITY: f32 = 0.1;
const CIRCLE_STROKE_OPACITY: f32 = 0.95;
/// 라벨 `dy=".35em"`
const LABEL_BASELINE_EM: f64 = 0.35;
/// `.venn-title` CSS 글자 크기. `font-size` 속성(32 × 배율)보다 CSS가 앞선다
const TITLE_FONT_SIZE: f64 = 32.0;

/// 집합 목록 열쇠(`A|B`)별로 겹친 스타일
fn styles_by_key(chart: &VennChart) -> HashMap<String, HashMap<String, String>> {
    let mut styles: HashMap<String, HashMap<String, String>> = HashMap::new();
    for entry in &chart.styles {
        let target = styles.entry(entry.targets.join("|")).or_default();
        for (key, value) in &entry.styles {
            target.insert(key.clone(), value.clone());
        }
    }
    styles
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let mut chart = parser::parse(definition)?;
    let directives = read_config(definition);
    if chart.title.is_empty() {
        chart.title = directives
            .title
            .as_deref()
            .map(sanitize_text)
            .unwrap_or_default();
    }
    let config = VennConfig::read(&directives);
    let colors = VennPalette::for_theme(context.palette.theme)
        .with_theme_variables(&context.theme_variables);
    let scale = config.width / REFERENCE_WIDTH;
    let title_height = if chart.title.is_empty() {
        0.0
    } else {
        48.0 * scale
    };
    let mut scene = Scene {
        width: config.width,
        height: config.height,
        items: Vec::new(),
        is_start_aligned: false,
    };
    draw_title(&mut scene, context.fonts, &chart.title, colors.title, scale);
    let areas = drawn_areas(&chart.subsets);
    if areas.is_empty() {
        return Ok(scene);
    }
    let normalized = normalized_solution(&areas);
    let height = config.height - title_height;
    let mut diagram = Scene::default();
    let painter = AreaPainter {
        fonts: context.fonts,
        colors: &colors,
        styles: styles_by_key(&chart),
        scale,
        is_debug_layout: config.is_debug_layout,
    };
    let circles = layout::scale_solution(&normalized, config.width, height, DIAGRAM_PADDING);
    painter.draw_areas(&mut diagram, &areas, &circles);
    if !chart.text_nodes.is_empty() {
        let text_circles =
            layout::scale_solution(&normalized, config.width, height, config.padding);
        painter.draw_text_nodes(&mut diagram, &chart.text_nodes, &areas, &text_circles);
    }
    diagram.translate(0.0, title_height);
    scene.items.extend(diagram.items);
    Ok(scene)
}

/// 제목은 CSS 글자 크기 32px로 위 가운데(y = 32 × 배율)에 둔다. 가로 위치는 `x="50%"`라 그림 폭의 절반이다
fn draw_title(scene: &mut Scene, fonts: &FontBook, title: &str, color: Color, scale: f64) {
    let text = AnchoredText {
        text: title,
        x: scene.width / 2.0,
        y: 32.0 * scale,
        font_size: TITLE_FONT_SIZE,
        color,
        anchor: TextAnchor::Middle,
        baseline: BaselineAnchor::Middle,
        degrees: 0.0,
    };
    push_anchored_text(scene, fonts, &text);
}

/// 크기 0인 집합과 그 집합이 든 영역은 그리지 않는다
fn drawn_areas(subsets: &[Subset]) -> Vec<Subset> {
    let removed: Vec<&String> = subsets
        .iter()
        .filter(|subset| subset.size == 0.0 && subset.sets.len() == 1)
        .map(|subset| &subset.sets[0])
        .collect();
    subsets
        .iter()
        .filter(|subset| !subset.sets.iter().any(|set| removed.contains(&set)))
        .cloned()
        .collect()
}

/// venn 배치 뒤 방향을 맞춘 해. 그림 상자 맞춤은 원(여백 15)과 글자 상자(여백 8)가 따로 한다
fn normalized_solution(areas: &[Subset]) -> Solution {
    let sizes: Vec<AreaSize> = areas
        .iter()
        .map(|area| AreaSize {
            sets: area.sets.clone(),
            size: area.size,
        })
        .collect();
    layout::normalize_solution(&layout::venn(&sizes))
}

/// 영역마다 (영역, 속한 원, 라벨 점)
fn placed_areas(areas: &[Subset], circles: &Solution) -> Vec<(Subset, Vec<Circle>, (f64, f64))> {
    let area_sets: Vec<Vec<String>> = areas.iter().map(|area| area.sets.clone()).collect();
    let centres = text_centre::compute_text_centres(circles, &area_sets);
    areas
        .iter()
        .zip(centres)
        .map(|(area, centre)| {
            let members = area
                .sets
                .iter()
                .map(|set| circle_of(circles, set))
                .collect();
            (area.clone(), members, centre)
        })
        .collect()
}

fn circle_of(circles: &Solution, set: &str) -> Circle {
    circles
        .iter()
        .find(|(name, _)| name == set)
        .map(|(_, circle)| *circle)
        .unwrap_or(Circle {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
        })
}

/// 영역 하나(경로와 라벨)를 그리는 데 쓰는 값
struct AreaPainter<'a> {
    fonts: &'a FontBook,
    colors: &'a VennPalette,
    styles: HashMap<String, HashMap<String, String>>,
    /// 그림 폭 ÷ 1600. 라벨 48px·선 5px·글자 상자 40px에 곱한다
    scale: f64,
    is_debug_layout: bool,
}

impl AreaPainter<'_> {
    /// 글자 상자는 원과 따로 배치한 `text_circles` 기준으로 놓는다. 글자 색은 글자 이름의 `style color`가 앞선다
    fn draw_text_nodes(
        &self,
        scene: &mut Scene,
        nodes: &[TextNode],
        areas: &[Subset],
        text_circles: &Solution,
    ) {
        let placed = placed_areas(areas, text_circles);
        let style = TextNodeStyle {
            scale: self.scale,
            color: self.colors.set_text,
            is_debug_layout: self.is_debug_layout,
        };
        let node_color = |id: &str| {
            self.styles
                .get(id)
                .and_then(|styles| styles.get("color"))
                .and_then(|value| Color::parse_css(value))
        };
        draw_text_nodes(scene, self.fonts, nodes, &placed, &style, node_color);
    }

    /// 정의 순서대로 원(집합 하나)과 겹침을 그린다. 원 색 번호는 원끼리만 센다
    fn draw_areas(&self, scene: &mut Scene, areas: &[Subset], circles: &Solution) {
        let mut circle_index = 0;
        for (area, members, centre) in placed_areas(areas, circles) {
            if area.sets.len() == 1 {
                self.draw_circle(scene, &area, members[0], circle_index, centre);
                circle_index += 1;
            } else {
                self.draw_intersection(scene, &area, &members, centre);
            }
        }
    }

    fn style(&self, area: &Subset, key: &str) -> Option<&String> {
        self.styles.get(&area.sets.join("|"))?.get(key)
    }

    fn style_color(&self, area: &Subset, key: &str) -> Option<Color> {
        self.style(area, key)
            .and_then(|value| Color::parse_css(value))
    }

    fn style_number(&self, area: &Subset, key: &str) -> Option<f64> {
        let value = self.style(area, key)?;
        value.trim().trim_end_matches("px").parse().ok()
    }

    /// 원: 기본 색은 테마 `venn{i+1}`, 라벨 색은 기본 색을 30만큼 어둡게(다크 테마는 밝게) 한 색이다
    fn draw_circle(
        &self,
        scene: &mut Scene,
        area: &Subset,
        circle: Circle,
        index: usize,
        centre: (f64, f64),
    ) {
        let base = self
            .style_color(area, "fill")
            .unwrap_or(self.colors.sets[index % self.colors.sets.len()]);
        let fill_opacity = self
            .style_number(area, "fill-opacity")
            .map_or(CIRCLE_FILL_OPACITY, |value| value as f32);
        let stroke_color = self.style_color(area, "stroke").unwrap_or(base);
        let stroke = StrokeStyle {
            color: with_opacity(stroke_color, CIRCLE_STROKE_OPACITY),
            width: self
                .style_number(area, "stroke-width")
                .unwrap_or(5.0 * self.scale),
            dash: Vec::new(),
        };
        let commands = ellipse(Point::new(circle.x, circle.y), circle.radius, circle.radius);
        scene.push_path(
            commands,
            Some(with_opacity(base, fill_opacity)),
            Some(stroke),
        );
        let shift = if self.colors.is_dark { 30.0 } else { -30.0 };
        let text_color = self
            .style_color(area, "color")
            .unwrap_or_else(|| adjust_lightness(base, shift));
        let label = area.label.clone().unwrap_or_else(|| area.sets[0].clone());
        self.draw_label(scene, &label, centre, text_color);
    }

    /// 겹침: `fill` 스타일이 있으면 겹친 영역을 불투명하게 채운다
    fn draw_intersection(
        &self,
        scene: &mut Scene,
        area: &Subset,
        members: &[Circle],
        centre: (f64, f64),
    ) {
        if let Some(fill) = self.style_color(area, "fill") {
            let (_, arcs) = intersection_area(members);
            scene.push_path(arcs_path(&arcs), Some(fill), None);
        }
        let text_color = self
            .style_color(area, "color")
            .unwrap_or(self.colors.set_text);
        let label = area.label.clone().unwrap_or_default();
        self.draw_label(scene, &label, centre, text_color);
    }

    /// 라벨은 라벨 점을 내림한 좌표에 가운데 맞춤으로 둔다
    fn draw_label(&self, scene: &mut Scene, label: &str, centre: (f64, f64), color: Color) {
        let text = AnchoredText {
            text: label,
            x: centre.0.floor(),
            y: centre.1.floor(),
            font_size: 48.0 * self.scale,
            color,
            anchor: TextAnchor::Middle,
            baseline: BaselineAnchor::Shifted(LABEL_BASELINE_EM),
            degrees: 0.0,
        };
        push_anchored_text(scene, self.fonts, &text);
    }
}

fn with_opacity(color: Color, opacity: f32) -> Color {
    Color {
        alpha: color.alpha * opacity,
        ..color
    }
}

/// arcsToPath: 호가 하나면 원 전체, 여럿이면 첫 호의 시작점에서 호를 차례로 잇는다
fn arcs_path(arcs: &[Arc]) -> Vec<PathCommand> {
    match arcs {
        [] => Vec::new(),
        [only] => ellipse(
            Point::new(only.circle.x, only.circle.y),
            only.circle.radius,
            only.circle.radius,
        ),
        [first, ..] => {
            let start = Point::new(first.p2.0, first.p2.1);
            let mut commands = vec![PathCommand::MoveTo(start)];
            let mut current = start;
            for arc in arcs {
                let end = Point::new(arc.p1.0, arc.p1.1);
                commands.extend(circular_arc(arc.circle, current, end));
                current = end;
            }
            commands.push(PathCommand::Close);
            commands
        }
    }
}

/// 원 위 `from`에서 `to`까지 화면 시계 방향(SVG sweep-flag 1)으로 도는 호를 90° 이하 3차 베지어로 나눈다
fn circular_arc(circle: Circle, from: Point, to: Point) -> Vec<PathCommand> {
    let start = (from.y - circle.y).atan2(from.x - circle.x);
    let end = (to.y - circle.y).atan2(to.x - circle.x);
    let sweep = (end - start).rem_euclid(2.0 * PI);
    let segments = (sweep / (PI / 2.0)).ceil().max(1.0) as usize;
    let step = sweep / segments as f64;
    let handle = 4.0 / 3.0 * (step / 4.0).tan() * circle.radius;
    let at = |angle: f64| {
        Point::new(
            circle.x + circle.radius * angle.cos(),
            circle.y + circle.radius * angle.sin(),
        )
    };
    (0..segments)
        .map(|index| {
            let first = start + step * index as f64;
            let second = first + step;
            let (from_point, to_point) = (at(first), at(second));
            PathCommand::CubicTo(
                Point::new(
                    from_point.x - handle * first.sin(),
                    from_point.y + handle * first.cos(),
                ),
                Point::new(
                    to_point.x + handle * second.sin(),
                    to_point.y - handle * second.cos(),
                ),
                to_point,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 세_집합_원_배치는_기준_svg와_같다() {
        let chart = parser::parse(
            "venn-beta\n title T\n set D:20\n set E:30\n set P:15\n union D,E:6\n union E,P:5\n union D,P:3\n union D,E,P:2",
        )
        .unwrap();
        let normalized = normalized_solution(&drawn_areas(&chart.subsets));
        let circles = layout::scale_solution(&normalized, 800.0, 450.0 - 24.0, DIAGRAM_PADDING);
        let design = circle_of(&circles, "D");
        assert!((design.x - 500.953_300_749_343_5).abs() < 1e-6);
        assert!((design.y - 260.442_277_621_242_9).abs() < 1e-6);
        assert!((design.radius - 122.929_865_554_521).abs() < 1e-6);
    }
}
