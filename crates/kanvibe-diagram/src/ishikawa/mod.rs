//! ishikawa(fishbone) 다이어그램(ishikawaRenderer.ts).
//!
//! 오른쪽 끝 머리(결과)에서 왼쪽으로 가로 등뼈를 긋고, 원인(가지)을 위·아래로 번갈아 82° 사선으로 단다.
//! 위·아래 가지 길이는 양쪽 자손 수 비율로 500px을 나눈다. 원인 아래 자손은 깊이가 짝수면 가로 뼈, 홀수면
//! 부모 가지와 나란한 사선 뼈로 그린다. 글자는 모두 SVG `<text>`라 HTML 라벨 방식과 상관없이 같은 장면이 나온다.
//! 그림 범위는 `getBBox()`에 사방 20px 여백을 더한 viewBox다.

mod bones;
mod parser;
mod text_block;

use crate::directive;
use crate::scene::{Color, PathCommand, Point, Scene, SceneItem, StrokeStyle, polygon};
use crate::text::{FontBook, FontWeight};
use crate::{DiagramError, RenderContext};

use text_block::{
    Anchor, Bounds, FONT_SIZE, PlacedText, TextStyle, VerticalAlign, place_text, wrap_text,
};

const DIAGRAM_PADDING: f64 = 20.0;
const HEAD_FONT_SIZE: f64 = 14.0;
const SPINE_WIDTH: f64 = 2.0;

/// 그리는 동안 쌓는 장면 항목과 `getBBox()` 범위
struct Canvas<'a> {
    fonts: &'a FontBook,
    line_color: Color,
    fill_color: Color,
    text_color: Color,
    items: Vec<SceneItem>,
    bounds: Bounds,
    /// 지금 그리는 가지 쌍에 놓인 글자들의 왼쪽 끝. 다음 쌍의 등뼈 시작점이 된다
    pair_text_left: f64,
}

impl Canvas<'_> {
    fn push_path(&mut self, commands: Vec<PathCommand>, fill: Option<Color>, stroke: f64) {
        let stroke = (stroke > 0.0).then(|| StrokeStyle {
            color: self.line_color,
            width: stroke,
            dash: Vec::new(),
        });
        self.items.push(SceneItem::Path(crate::scene::PathItem {
            commands,
            fill,
            stroke,
        }));
    }

    /// 선과 시작점 화살표 표식(`marker-start`, 10×10 viewBox를 선 굵기 × 0.6 배율로 그린다)
    fn bone(&mut self, from: Point, to: Point, width: f64) {
        self.bounds.include_point(from.x, from.y);
        self.bounds.include_point(to.x, to.y);
        self.push_path(
            vec![PathCommand::MoveTo(from), PathCommand::LineTo(to)],
            None,
            width,
        );
        let length = (to.x - from.x).hypot(to.y - from.y);
        if length == 0.0 {
            return;
        }
        let scale = width * 0.6;
        let (unit_x, unit_y) = ((to.x - from.x) / length, (to.y - from.y) / length);
        let corner = |along: f64, across: f64| {
            Point::new(
                from.x + scale * (along * unit_x - across * unit_y),
                from.y + scale * (along * unit_y + across * unit_x),
            )
        };
        let arrow = [corner(10.0, -5.0), corner(0.0, 0.0), corner(10.0, 5.0)];
        let color = self.line_color;
        self.push_path(polygon(&arrow), Some(color), 0.0);
    }

    fn text(&mut self, text: &str, x: f64, y: f64, style: TextStyle) -> Bounds {
        let placed = place_text(self.fonts, text, x, y, style, self.text_color);
        self.push_placed(placed)
    }

    fn push_placed(&mut self, placed: PlacedText) -> Bounds {
        self.bounds.include(&placed.bounds);
        self.pair_text_left = self.pair_text_left.min(placed.bounds.left);
        self.items
            .extend(placed.items.into_iter().map(SceneItem::Text));
        placed.bounds
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let root = parser::parse(&directive::diagram_body(definition))?;
    let palette = context.palette;
    // ishikawaStyles.ts: 선·화살촉은 `lineColor`, 머리·라벨 상자는 `mainBkg`, 글자는 `textColor`
    let variables = &context.theme_variables;
    let (mut line_color, mut fill_color, mut text_color) =
        (palette.line, palette.node_fill, palette.text);
    variables.read_color("lineColor", &mut line_color);
    variables.read_color("mainBkg", &mut fill_color);
    variables.read_color("textColor", &mut text_color);
    let mut canvas = Canvas {
        fonts: context.fonts,
        line_color,
        fill_color,
        text_color,
        items: Vec::new(),
        bounds: Bounds::EMPTY,
        pair_text_left: f64::INFINITY,
    };
    let layout = bones::SpineLayout::new(&root.children);
    draw_head(&mut canvas, &root.text, layout.spine_y);
    let spine_start = match root.children.is_empty() {
        true => 0.0,
        false => bones::draw_pairs(&mut canvas, &root.children, &layout),
    };
    draw_spine(&mut canvas, spine_start, layout.spine_y);
    Ok(finish(canvas))
}

/// 등뼈는 DOM에서 머리보다 먼저 놓여 머리 아래에 깔린다
fn draw_spine(canvas: &mut Canvas, start_x: f64, spine_y: f64) {
    canvas.bounds.include_point(start_x, spine_y);
    canvas.bounds.include_point(0.0, spine_y);
    if start_x == 0.0 {
        return;
    }
    let spine = [
        PathCommand::MoveTo(Point::new(start_x, spine_y)),
        PathCommand::LineTo(Point::new(0.0, spine_y)),
    ];
    let mut items = std::mem::take(&mut canvas.items);
    canvas.push_path(spine.to_vec(), None, SPINE_WIDTH);
    canvas.items.append(&mut items);
}

/// drawHead: 굵은 14px 글자를 11자 폭으로 감싸고, 글자 상자보다 큰 반달(이차 베지어) 머리를 그린다
fn draw_head(canvas: &mut Canvas, label: &str, spine_y: f64) {
    let max_chars = 6_usize.max((110.0 / (FONT_SIZE * 0.6)).floor() as usize);
    let text = wrap_text(label, max_chars);
    let style = TextStyle {
        font_size: HEAD_FONT_SIZE,
        weight: FontWeight::Bold,
        anchor: Anchor::Middle,
        align: VerticalAlign::Middle,
    };
    let measured = place_text(canvas.fonts, &text, 0.0, 0.0, style, canvas.text_color).bounds;
    let width = (measured.width() + 6.0).max(60.0);
    let height = (measured.height() * 2.0 + 40.0).max(40.0);
    let head = vec![
        PathCommand::MoveTo(Point::new(0.0, spine_y - height / 2.0)),
        PathCommand::LineTo(Point::new(0.0, spine_y + height / 2.0)),
        PathCommand::QuadTo(
            Point::new(width * 2.4, spine_y),
            Point::new(0.0, spine_y - height / 2.0),
        ),
        PathCommand::Close,
    ];
    canvas.bounds.include(&Bounds {
        left: 0.0,
        top: spine_y - height / 2.0,
        right: width * 1.2,
        bottom: spine_y + height / 2.0,
    });
    let fill = canvas.fill_color;
    canvas.push_path(head, Some(fill), SPINE_WIDTH);
    let shift_x = (width - measured.width()) / 2.0 - measured.left + 3.0;
    let shift_y = -measured.top - measured.height() / 2.0;
    canvas.text(&text, shift_x, spine_y + shift_y, style);
}

/// applyPaddedViewBox: 범위에 여백을 더해 원점을 viewBox 왼쪽 위로 옮긴다
fn finish(canvas: Canvas) -> Scene {
    let bounds = canvas.bounds;
    let mut scene = Scene {
        width: bounds.width() + DIAGRAM_PADDING * 2.0,
        height: bounds.height() + DIAGRAM_PADDING * 2.0,
        items: canvas.items,
        is_start_aligned: false,
    };
    scene.translate(DIAGRAM_PADDING - bounds.left, DIAGRAM_PADDING - bounds.top);
    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LabelMode;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;

    fn render_light(definition: &str) -> Scene {
        let fonts = bundled_fonts();
        crate::render_scene(
            definition,
            Theme::Light,
            LabelMode::Html,
            crate::theme_variables::ThemeVariablesPolicy::Ignore,
            &fonts,
        )
        .unwrap()
    }

    #[test]
    fn 기준_svg와_같은_viewbox를_만든다() {
        // ishikawa-simple 기준 SVG viewBox: -246.2149 39.2366 338.2149 583.1340
        let scene = render_light(
            "ishikawa\n    Server Outage\n    Network\n        DNS failure\n    Software\n        Memory leak\n        Bad deploy\n    Hardware",
        );
        assert!((scene.width - 338.2149).abs() < 0.05, "{}", scene.width);
        assert!((scene.height - 583.1340).abs() < 0.05, "{}", scene.height);
    }

    #[test]
    fn 원인이_없으면_머리만_그린다() {
        let scene = render_light("ishikawa\n  Effect");
        // 머리 너비 60 → 오른쪽 끝 72, 높이 max(40, 19·2+40)=78
        assert!(
            (scene.width - (72.0 + 40.0)).abs() < 1e-9,
            "{}",
            scene.width
        );
        assert!(
            (scene.height - (78.0 + 40.0)).abs() < 1e-9,
            "{}",
            scene.height
        );
    }
}
