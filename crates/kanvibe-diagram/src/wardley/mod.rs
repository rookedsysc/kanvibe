//! wardley-beta 다이어그램(wardleyRenderer.ts).
//!
//! 900×600(`size`로 바꾼다) 판 안쪽 48px에 축을 긋고, 구성 요소를 (진화, 가시성) 백분율 자리에 놓는다.
//! 진화 단계 라벨·점선, 파이프라인 상자, 연결선·흐름 화살표, 진화 추세(빨간 점선 화살표), 노드(조달 전략
//! 겹원·관성 막대), 주석, 노트, 가속·감속 화살표를 그 순서로 그린다. 기준 SVG에는 `<style>`이 없어 색은 모두
//! 속성 값이고, 글자는 기본 글꼴(Noto Sans)이다. 글자는 모두 SVG `<text>`라 라벨 방식과 상관없이 같다.

mod layers;
mod marks;
mod nodes;
mod parser;

use crate::scene::{Color, PathCommand, Point, Rotation, Scene, StrokeStyle, TextItem, ellipse};
use crate::text::{FontBook, FontWeight};
use crate::theme::Theme;
use crate::theme_variables::ThemeVariables;
use crate::{DiagramError, RenderContext, directive};

use parser::WardleyMap;

const DEFAULT_WIDTH: f64 = 900.0;
const DEFAULT_HEIGHT: f64 = 600.0;
const PADDING: f64 = 48.0;
/// Noto Sans 세로 지표(1em 기준)
const ASCENT_EM: f64 = 1.069;
const DESCENT_EM: f64 = 0.293;
const REGULAR_X_HEIGHT_EM: f64 = 0.536;
const BOLD_X_HEIGHT_EM: f64 = 0.547;

/// getTheme()이 테마 변수에서 고른 색. 렌더러가 요소 속성으로 넣고, 기준 SVG에서 styles.ts 규칙은 이 색을 바꾸지 않는다
/// (설명 동그라미·상자는 `annotationFill`이 아니라 속성의 흰색이 보인다)
#[derive(Clone, Copy)]
struct WardleyColors {
    background: Color,
    axis: Color,
    axis_text: Color,
    component_fill: Color,
    component_stroke: Color,
    component_label: Color,
    link: Color,
    evolution: Color,
}

impl WardleyColors {
    fn for_theme(theme: Theme) -> Self {
        let light_grey = Color::rgb(211, 211, 211);
        let (background, axis, axis_text, component_fill, evolution) = match theme {
            Theme::Light => (
                Color::rgb(255, 255, 255),
                Color::rgb(0x33, 0x33, 0x33),
                Color::rgb(0x13, 0x13, 0x00),
                Color::rgb(255, 255, 255),
                Color::rgb(0xDC, 0x35, 0x45),
            ),
            Theme::Dark => (
                Color::rgb(0x33, 0x33, 0x33),
                light_grey,
                Color::rgb(0xE0, 0xDF, 0xDF),
                Color::rgb(0x1F, 0x20, 0x20),
                Color::rgb(0xFF, 0x6B, 0x6B),
            ),
        };
        Self {
            background,
            axis,
            axis_text,
            component_fill,
            component_stroke: axis,
            component_label: axis_text,
            link: axis,
            evolution,
        }
    }

    /// wardleyRenderer.ts `getTheme`: `themeVariables.wardley.*`가 없으면 `background`·`primaryTextColor`로 물러난다
    fn read(theme: Theme, variables: &ThemeVariables) -> Self {
        let mut colors = Self::for_theme(theme);
        variables.read_color("background", &mut colors.background);
        for target in [&mut colors.axis_text, &mut colors.component_label] {
            variables.read_color("primaryTextColor", target);
        }
        let wardley = variables.section("wardley");
        for (key, target) in [
            ("backgroundColor", &mut colors.background),
            ("axisColor", &mut colors.axis),
            ("axisTextColor", &mut colors.axis_text),
            ("componentFill", &mut colors.component_fill),
            ("componentStroke", &mut colors.component_stroke),
            ("componentLabelColor", &mut colors.component_label),
            ("linkStroke", &mut colors.link),
            ("evolutionStroke", &mut colors.evolution),
        ] {
            wardley.read_color(key, target);
        }
        colors
    }
}

/// 판 크기와 백분율 → 좌표 변환
#[derive(Clone, Copy)]
struct Frame {
    width: f64,
    height: f64,
}

impl Frame {
    fn chart_width(&self) -> f64 {
        self.width - PADDING * 2.0
    }

    fn chart_height(&self) -> f64 {
        self.height - PADDING * 2.0
    }

    fn project_x(&self, percent: f64) -> f64 {
        PADDING + percent / 100.0 * self.chart_width()
    }

    fn project_y(&self, percent: f64) -> f64 {
        self.height - PADDING - percent / 100.0 * self.chart_height()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Start,
    Middle,
}

/// `dominant-baseline`
#[derive(Clone, Copy, PartialEq, Eq)]
enum Baseline {
    Alphabetic,
    Middle,
    Central,
}

/// SVG `<text>` 한 줄의 속성
#[derive(Clone, Copy)]
struct TextStyle {
    font_size: f64,
    weight: FontWeight,
    anchor: Anchor,
    baseline: Baseline,
    color: Color,
}

/// 장면에 경로·글자를 쌓는 붓
struct Painter<'a> {
    fonts: &'a FontBook,
    colors: WardleyColors,
    scene: Scene,
}

impl Painter<'_> {
    fn stroke(color: Color, width: f64, dash: &[f64]) -> Option<StrokeStyle> {
        Some(StrokeStyle {
            color,
            width,
            dash: dash.to_vec(),
        })
    }

    fn line(&mut self, from: Point, to: Point, color: Color, width: f64, dash: &[f64]) {
        let commands = vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)];
        self.scene
            .push_path(commands, None, Self::stroke(color, width, dash));
    }

    fn circle(&mut self, center: Point, radius: f64, fill: Color, stroke: Color, width: f64) {
        self.scene.push_path(
            ellipse(center, radius, radius),
            Some(fill),
            Self::stroke(stroke, width, &[]),
        );
    }

    /// 표식(marker): viewBox 점들을 `ref` 기준으로 `scale`배 해 `at`에서 `direction` 쪽으로 돌려 채운다
    fn marker(
        &mut self,
        at: Point,
        direction: (f64, f64),
        scale: f64,
        points: &[(f64, f64)],
        color: Color,
    ) {
        let corners: Vec<Point> = points
            .iter()
            .map(|(along, across)| {
                Point::new(
                    at.x + scale * (along * direction.0 - across * direction.1),
                    at.y + scale * (along * direction.1 + across * direction.0),
                )
            })
            .collect();
        self.scene
            .push_path(crate::scene::polygon(&corners), Some(color), None);
    }

    /// (x, y)에 글자를 놓는다. `rotation`은 (x, y)를 중심으로 돌리는 각도다
    fn text(&mut self, text: &str, at: Point, style: TextStyle, rotation: Option<f64>) {
        let advance = self
            .fonts
            .measure_svg_runs(&[text], style.font_size, style.weight)
            .advance;
        let left = match style.anchor {
            Anchor::Start => at.x,
            Anchor::Middle => at.x - advance / 2.0,
        };
        self.scene.push_text(TextItem {
            text: text.to_string(),
            x: left,
            baseline: at.y + baseline_shift(style),
            font_size: style.font_size,
            weight: style.weight,
            color: style.color,
            rotation: rotation.map(|degrees| Rotation {
                degrees,
                center: at,
            }),
        });
    }
}

/// `dominant-baseline`이 기준선을 `y`에서 아래로 내리는 거리
fn baseline_shift(style: TextStyle) -> f64 {
    match style.baseline {
        Baseline::Alphabetic => 0.0,
        Baseline::Central => (ASCENT_EM - DESCENT_EM) / 2.0 * style.font_size,
        Baseline::Middle => {
            let x_height = match style.weight {
                FontWeight::Bold | FontWeight::BoldItalic => BOLD_X_HEIGHT_EM,
                FontWeight::Regular | FontWeight::Italic | FontWeight::Monospace => {
                    REGULAR_X_HEIGHT_EM
                }
            };
            (x_height * style.font_size / 2.0 * 64.0).floor() / 64.0
        }
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let map = parser::parse(&directive::diagram_body(definition))?;
    let (width, height) = map.size.unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT));
    let frame = Frame { width, height };
    let mut painter = Painter {
        fonts: context.fonts,
        colors: WardleyColors::read(context.palette.theme, &context.theme_variables),
        scene: Scene {
            width,
            height,
            ..Scene::default()
        },
    };
    draw_map(&mut painter, &map, frame);
    Ok(painter.scene)
}

fn draw_map(painter: &mut Painter, map: &WardleyMap, frame: Frame) {
    let background = painter.colors.background;
    let corners = [
        Point::new(0.0, 0.0),
        Point::new(frame.width, 0.0),
        Point::new(frame.width, frame.height),
        Point::new(0.0, frame.height),
    ];
    painter
        .scene
        .push_path(crate::scene::polygon(&corners), Some(background), None);
    layers::draw_title_and_axes(painter, map, frame);
    layers::draw_stages(painter, map, frame);
    let mut positions: Vec<Point> = map
        .nodes
        .iter()
        .map(|node| {
            Point::new(
                frame.project_x(node.evolution),
                frame.project_y(node.visibility),
            )
        })
        .collect();
    layers::draw_pipelines(painter, map, &mut positions);
    layers::draw_links(painter, map, &positions);
    layers::draw_trends(painter, map, frame, &positions);
    nodes::draw_nodes(painter, map, &positions);
    marks::draw_annotations(painter, map, frame);
    marks::draw_notes_and_accelerators(painter, map, frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 백분율을_축_안쪽_좌표로_바꾼다() {
        let frame = Frame {
            width: 900.0,
            height: 600.0,
        };
        // wardley-basic 기준 SVG: Kettle 진화 35% → x 329.4, 추세 y 335.28(가시성 43%)
        assert!((frame.project_x(35.0) - 329.4).abs() < 1e-9);
        assert!((frame.project_y(43.0) - 335.28).abs() < 1e-9);
    }
}
