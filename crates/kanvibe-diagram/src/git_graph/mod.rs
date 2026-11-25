//! gitGraph 다이어그램(gitGraphRenderer.ts·styles.js).
//!
//! 브랜치마다 점선 줄과 이름표를 두고, 커밋을 순번대로 50씩 띄워 놓은 뒤 부모에서 자식으로 굵은 곡선 화살표를
//! 잇는다. 커밋 id 라벨은 -45° 돌리고 태그는 커밋 위에 붙인다. 글자는 모두 SVG `<text>`라 HTML 라벨 방식과
//! 상관없이 같은 장면이 나오고, viewBox는 그린 것 전체의 getBBox에 여백 8을 둔다.

mod draw;
mod layout;
mod options;
mod parse;

use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, TextItem, rounded_rect};
use crate::text::{FontBook, FontWeight};
use crate::theme::{Theme, hsl};
use crate::{DiagramError, RenderContext};

use layout::{
    ArrowPath, BRANCH_FONT_SIZE, GraphLayout, VERTICAL_START, branch_label_height,
    branch_label_rows, branch_label_width,
};
use parse::{Direction, GitGraph};

const DIAGRAM_PADDING: f64 = 8.0;
const TITLE_TOP_MARGIN: f64 = 25.0;
const TITLE_FONT_SIZE: f64 = 18.0;
const ARROW_WIDTH: f64 = 8.0;
const BRANCH_LABEL_RADIUS: f64 = 4.0;
const ROTATED_BRANCH_LABEL_SHIFT: f64 = 30.0;
/// 브랜치·화살표·커밋 색 수(THEME_COLOR_LIMIT)
const COLOR_COUNT: usize = 8;
/// 원호를 3차 베지어로 근사하는 상수
const ARC_HANDLE: f64 = 0.552_284_749_8;

/// 기준 SVG `<style>`에서 읽은 gitGraph 색
pub(crate) struct GitPalette {
    /// `.commitN`·`.arrowN`·`.labelN`(브랜치 이름표 바탕)
    branch: [Color; COLOR_COUNT],
    /// `.commit-highlightN`
    highlight: [Color; COLOR_COUNT],
    /// `.branch-labelN`
    branch_text: [Color; COLOR_COUNT],
    branch_line: Color,
    commit_label_text: Color,
    commit_label_background: Color,
    tag_text: Color,
    tag_background: Color,
    tag_border: Color,
    /// 루트 `fill`과 `textColor`: 태그 구멍, 체리픽 원, 제목
    text: Color,
    /// `primaryColor`: 병합 안쪽 원, 되돌림 ×, 강조 안쪽 사각형
    primary: Color,
    /// 체리픽 원 안의 표시
    cherry_mark: Color,
}

fn rgb_css(value: &str) -> Color {
    Color::parse_css(value).unwrap_or(Color::rgb(0, 0, 0))
}

fn palette_for(theme: Theme) -> GitPalette {
    match theme {
        Theme::Light => light_palette(),
        Theme::Dark => dark_palette(),
    }
}

fn light_palette() -> GitPalette {
    let white = Color::rgb(255, 255, 255);
    let black = Color::rgb(0, 0, 0);
    GitPalette {
        branch: [
            hsl(240.0, 100.0, 46.274_509_803_9),
            hsl(60.0, 100.0, 43.529_411_764_7),
            hsl(80.0, 100.0, 46.274_509_803_9),
            hsl(210.0, 100.0, 46.274_509_803_9),
            hsl(180.0, 100.0, 46.274_509_803_9),
            hsl(150.0, 100.0, 46.274_509_803_9),
            hsl(300.0, 100.0, 46.274_509_803_9),
            hsl(0.0, 100.0, 46.274_509_803_9),
        ],
        highlight: [
            hsl(60.0, 100.0, 3.725_490_196_1),
            rgb_css("rgb(0, 0, 160.5)"),
            rgb_css("rgb(48.8333333334, 0, 146.5000000001)"),
            rgb_css("rgb(146.5000000001, 73.2500000001, 0)"),
            rgb_css("rgb(146.5000000001, 0, 0)"),
            rgb_css("rgb(146.5000000001, 0, 73.2500000001)"),
            rgb_css("rgb(0, 146.5000000001, 0)"),
            rgb_css("rgb(0, 146.5000000001, 146.5000000001)"),
        ],
        branch_text: [white, black, black, white, black, black, black, black],
        branch_line: Color::rgb(0x33, 0x33, 0x33),
        commit_label_text: Color::rgb(0x00, 0x00, 0x21),
        commit_label_background: Color::rgba(0xFF, 0xFF, 0xDE, 0.5),
        tag_text: Color::rgb(0x13, 0x13, 0x00),
        tag_background: Color::rgb(0xEC, 0xEC, 0xFF),
        tag_border: hsl(240.0, 60.0, 86.274_509_803_9),
        text: Color::rgb(0x33, 0x33, 0x33),
        primary: Color::rgb(0xEC, 0xEC, 0xFF),
        cherry_mark: white,
    }
}

fn dark_palette() -> GitPalette {
    let dark_label = Color::rgb(0x2C, 0x2C, 0x2C);
    let light_grey = Color::rgb(211, 211, 211);
    GitPalette {
        branch: [
            hsl(180.0, 1.587_301_587_3, 48.352_941_176_5),
            hsl(321.639_344_262_3, 65.591_397_849_5, 38.235_294_117_6),
            hsl(194.4, 16.556_291_390_7, 49.607_843_137_3),
            hsl(23.076_923_076_9, 49.056_603_773_6, 40.784_313_725_5),
            hsl(0.0, 83.333_333_333_3, 43.529_411_764_7),
            hsl(289.166_666_666_7, 100.0, 24.117_647_058_8),
            hsl(35.131_578_947_4, 98.701_298_701_3, 40.196_078_431_4),
            hsl(106.153_846_153_8, 84.415_584_415_6, 35.098_039_215_7),
        ],
        highlight: [
            rgb_css("rgb(133.6571428571, 129.7428571428, 129.7428571428)"),
            rgb_css("rgb(93.5483870969, 221.4516129033, 139.677419355)"),
            rgb_css("rgb(149.4437086091, 117.6092715231, 107.5562913906)"),
            rgb_css("rgb(99.9811320754, 162.7735849057, 202.0188679245)"),
            rgb_css("rgb(51.5000000001, 236.5, 236.5)"),
            rgb_css("rgb(154.2083333334, 255, 132.0000000001)"),
            rgb_css("rgb(51.331168831, 135.1948051946, 253.6688311688)"),
            rgb_css("rgb(206.1818181817, 89.948051948, 241.051948052)"),
        ],
        branch_text: [
            dark_label, light_grey, light_grey, dark_label, light_grey, light_grey, light_grey,
            light_grey,
        ],
        branch_line: light_grey,
        commit_label_text: rgb_css("rgb(183.8476190475, 181.5523809523, 181.5523809523)"),
        commit_label_background: Color {
            alpha: 0.5,
            ..hsl(180.0, 1.587_301_587_3, 28.352_941_176_5)
        },
        tag_text: Color::rgb(0xE0, 0xDF, 0xDF),
        tag_background: Color::rgb(0x1F, 0x20, 0x20),
        tag_border: Color::rgb(0xCC, 0xCC, 0xCC),
        text: Color::rgb(0xCC, 0xCC, 0xCC),
        primary: Color::rgb(0x1F, 0x20, 0x20),
        cherry_mark: Color::rgb(0, 0, 0),
    }
}

/// SVG getBBox처럼 그린 것의 테두리(획 굵기 제외)를 모은다
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bounds {
    min: Point,
    max: Point,
}

impl Bounds {
    fn new() -> Self {
        Self {
            min: Point::new(f64::INFINITY, f64::INFINITY),
            max: Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub(crate) fn include(&mut self, point: Point) {
        self.min = Point::new(self.min.x.min(point.x), self.min.y.min(point.y));
        self.max = Point::new(self.max.x.max(point.x), self.max.y.max(point.y));
    }

    pub(crate) fn include_rect(&mut self, left: f64, top: f64, width: f64, height: f64) {
        self.include(Point::new(left, top));
        self.include(Point::new(left + width, top + height));
    }

    fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
}

/// 장면에 그리며 getBBox 테두리를 함께 모으는 붓
pub(crate) struct Canvas<'a> {
    pub(crate) scene: Scene,
    pub(crate) bounds: Bounds,
    pub(crate) fonts: &'a FontBook,
    pub(crate) colors: GitPalette,
}

impl Canvas<'_> {
    /// 왼쪽 x, 기준선 y의 SVG 글자. 테두리는 글자 칸(ascent·descent)과 잉크 폭을 합친 것이다
    pub(crate) fn text(&mut self, text: &str, x: f64, baseline: f64, font_size: f64, color: Color) {
        let item = self.text_item(text, x, baseline, font_size, color);
        let (left, top, width, height) = self.text_box(&item);
        self.bounds.include_rect(left, top, width, height);
        self.scene.push_text(item);
    }

    pub(crate) fn text_item(
        &self,
        text: &str,
        x: f64,
        baseline: f64,
        font_size: f64,
        color: Color,
    ) -> TextItem {
        TextItem {
            text: text.to_string(),
            x,
            baseline,
            font_size,
            weight: FontWeight::Regular,
            color,
            rotation: None,
        }
    }

    /// 글자 getBBox (왼쪽, 위, 폭, 높이)
    pub(crate) fn text_box(&self, item: &TextItem) -> (f64, f64, f64, f64) {
        let extent = self
            .fonts
            .measure_svg_runs(&[&item.text], item.font_size, item.weight);
        let metrics = self.fonts.primary_metrics(item.font_size);
        (
            item.x + extent.left,
            item.baseline - metrics.ascent,
            extent.width(),
            metrics.ascent + metrics.descent,
        )
    }

    pub(crate) fn text_width(&self, text: &str, font_size: f64) -> f64 {
        self.fonts
            .measure_svg_runs(&[text], font_size, FontWeight::Regular)
            .width()
    }

    pub(crate) fn path(
        &mut self,
        commands: Vec<PathCommand>,
        fill: Option<Color>,
        stroke: Option<StrokeStyle>,
    ) {
        // 원·둥근 사각형도 곡선 끝점이 극점이라 끝점만 모으면 된다
        for command in &commands {
            if let PathCommand::MoveTo(point)
            | PathCommand::LineTo(point)
            | PathCommand::CubicTo(_, _, point) = command
            {
                self.bounds.include(*point);
            }
        }
        self.scene.push_path(commands, fill, stroke);
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let graph = parse::parse(definition)?;
    let fonts = context.fonts;
    let mut layout = GraphLayout::new(&graph, fonts);
    let mut canvas = Canvas {
        scene: Scene::default(),
        bounds: Bounds::new(),
        fonts,
        colors: palette_for(context.palette.theme),
    };
    if graph.options.show_branches {
        draw_branches(&mut canvas, &graph, &mut layout);
    }
    draw_arrows(&mut canvas, &graph, &mut layout);
    draw::draw_commits(&mut canvas, &graph, &layout);
    draw_title(&mut canvas, &graph.title);
    Ok(finish(canvas))
}

/// setupGraphViewbox: 테두리에 여백을 두르고 viewBox 원점을 0,0으로 옮긴다
fn finish(canvas: Canvas) -> Scene {
    let mut scene = canvas.scene;
    let bounds = if canvas.bounds.is_empty() {
        Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(0.0, 0.0),
        }
    } else {
        canvas.bounds
    };
    scene.width = bounds.max.x - bounds.min.x + DIAGRAM_PADDING * 2.0;
    scene.height = bounds.max.y - bounds.min.y + DIAGRAM_PADDING * 2.0;
    scene.translate(
        DIAGRAM_PADDING - bounds.min.x,
        DIAGRAM_PADDING - bounds.min.y,
    );
    scene
}

/// drawBranches: 줄(점선 2)과 둥근 이름표. 줄 자리는 화살표 우회 줄이 피하도록 예약한다
fn draw_branches(canvas: &mut Canvas, graph: &GitGraph, layout: &mut GraphLayout) {
    // 돌린 커밋 라벨 자리만큼 LR 이름표를 왼쪽으로 민다
    let label_shift = if graph.options.rotate_commit_label {
        ROTATED_BRANCH_LABEL_SHIFT
    } else {
        0.0
    };
    for (index, name) in graph.branches.iter().enumerate() {
        let color_index = index % COLOR_COUNT;
        let spine = layout.spine(name);
        let (start, end) = match layout.direction {
            Direction::LeftToRight => (Point::new(0.0, spine), Point::new(layout.max_pos, spine)),
            Direction::TopToBottom => (
                Point::new(spine, VERTICAL_START),
                Point::new(spine, layout.max_pos),
            ),
            Direction::BottomToTop => (
                Point::new(spine, layout.max_pos),
                Point::new(spine, VERTICAL_START),
            ),
        };
        let stroke = StrokeStyle {
            color: canvas.colors.branch_line,
            width: 1.0,
            dash: vec![2.0, 2.0],
        };
        let line = vec![PathCommand::MoveTo(start), PathCommand::LineTo(end)];
        canvas.path(line, None, Some(stroke));
        layout.reserve_lane(spine);
        let place = BranchLabelPlace {
            spine,
            color_index,
            label_shift,
        };
        draw_branch_label(canvas, name, layout, place);
    }
}

/// 이름표 바탕(폭 + 18, 높이 + 4)과 이름. LR은 줄 왼쪽, TB는 위, BT는 아래에 둔다
/// 브랜치 이름표를 놓을 줄 자리와 색
struct BranchLabelPlace {
    spine: f64,
    color_index: usize,
    label_shift: f64,
}

fn draw_branch_label(
    canvas: &mut Canvas,
    name: &str,
    layout: &GraphLayout,
    place: BranchLabelPlace,
) {
    let width = branch_label_width(canvas.fonts, name);
    let height = branch_label_height(canvas.fonts, name);
    let (rect_left, rect_top, text_left, text_top) =
        branch_label_origin(layout, &place, width, height);
    let background = canvas.colors.branch[place.color_index];
    let rect = rounded_rect(
        rect_left,
        rect_top,
        width + 18.0,
        height + 4.0,
        BRANCH_LABEL_RADIUS,
    );
    canvas.path(rect, Some(background), None);
    let color = canvas.colors.branch_text[place.color_index];
    for (index, row) in branch_label_rows(name).iter().enumerate() {
        let baseline = text_top + (index + 1) as f64 * BRANCH_FONT_SIZE;
        canvas.text(row, text_left, baseline, BRANCH_FONT_SIZE, color);
    }
}

/// 이름표 바탕 (왼쪽, 위)와 이름 g의 (왼쪽, 위). LR은 바탕과 이름 g 모두 글자 칸 위 - 2에 놓이고, 줄마다 tspan이
/// 1em씩 내려 기준선을 잡는다
fn branch_label_origin(
    layout: &GraphLayout,
    place: &BranchLabelPlace,
    width: f64,
    height: f64,
) -> (f64, f64, f64, f64) {
    let spine = place.spine;
    match layout.direction {
        Direction::LeftToRight => (
            -width - 23.0 - place.label_shift,
            spine - height / 2.0 - 2.0,
            -width - 14.0 - place.label_shift,
            spine - height / 2.0 - 2.0,
        ),
        Direction::TopToBottom => (
            spine - width / 2.0 - 10.0,
            0.0,
            spine - width / 2.0 - 5.0,
            0.0,
        ),
        Direction::BottomToTop => (
            spine - width / 2.0 - 10.0,
            layout.max_pos,
            spine - width / 2.0 - 5.0,
            layout.max_pos,
        ),
    }
}

/// drawArrows: 커밋마다 부모에서 오는 화살표(굵기 8, 모서리는 1/4 원)
fn draw_arrows(canvas: &mut Canvas, graph: &GitGraph, layout: &mut GraphLayout) {
    for commit in &graph.commits {
        for parent_id in &commit.parents {
            let Some(parent) = graph.commit(parent_id) else {
                continue;
            };
            let Some(arrow) = layout.arrow(graph, parent, commit) else {
                continue;
            };
            let stroke = StrokeStyle {
                color: canvas.colors.branch[arrow.color_index % COLOR_COUNT],
                width: ARROW_WIDTH,
                dash: Vec::new(),
            };
            canvas.path(rounded_polyline(&arrow), None, Some(stroke));
        }
    }
}

/// 꺾이는 점마다 앞뒤로 반지름만큼 물러난 두 점을 1/4 원으로 잇는다(SVG `A r r 0 0 s`)
fn rounded_polyline(arrow: &ArrowPath) -> Vec<PathCommand> {
    let points = &arrow.points;
    let mut commands = vec![PathCommand::MoveTo(points[0])];
    for window in points.windows(3) {
        let (previous, corner, next) = (window[0], window[1], window[2]);
        let arc_start = step_towards(corner, previous, arrow.radius);
        let arc_end = step_towards(corner, next, arrow.radius);
        commands.push(PathCommand::LineTo(arc_start));
        commands.push(PathCommand::CubicTo(
            lerp(arc_start, corner, ARC_HANDLE),
            lerp(arc_end, corner, ARC_HANDLE),
            arc_end,
        ));
    }
    commands.push(PathCommand::LineTo(points[points.len() - 1]));
    commands
}

/// `from`에서 `to` 쪽으로 `distance`만큼 간 점
fn step_towards(from: Point, to: Point, distance: f64) -> Point {
    let length = (to.x - from.x).hypot(to.y - from.y);
    if length == 0.0 {
        return from;
    }
    lerp(from, to, distance / length)
}

fn lerp(from: Point, to: Point, ratio: f64) -> Point {
    Point::new(
        from.x + (to.x - from.x) * ratio,
        from.y + (to.y - from.y) * ratio,
    )
}

/// insertTitle: 그때까지의 테두리 가운데 위(y = -25)에 18px 제목
fn draw_title(canvas: &mut Canvas, title: &str) {
    if title.is_empty() || canvas.bounds.is_empty() {
        return;
    }
    let center = (canvas.bounds.min.x + canvas.bounds.max.x) / 2.0;
    let width = canvas.text_width(title, TITLE_FONT_SIZE);
    let color = canvas.colors.text;
    canvas.text(
        title,
        center - width / 2.0,
        -TITLE_TOP_MARGIN,
        TITLE_FONT_SIZE,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LabelMode;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Palette;

    #[test]
    fn 기준_svg와_같은_viewbox_폭을_만든다() {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::Html,
            theme_variables: Default::default(),
        };
        let definition = "gitGraph\n commit\n commit\n branch develop\n checkout develop\n commit\n commit\n checkout main\n merge develop\n commit\n branch feature\n commit tag: \"v1\"\n checkout main\n commit";
        let scene = render(definition, &context).unwrap();
        // 기준 SVG viewBox 폭 528.359375(develop 이름표 왼쪽 끝 -112.36 ~ 브랜치 줄 끝 400, 여백 8)
        assert!((scene.width - 528.359_375).abs() < 0.5, "{}", scene.width);
    }

    #[test]
    fn 모서리는_반지름만큼_물러나_곡선으로_잇는다() {
        let arrow = ArrowPath {
            points: vec![
                Point::new(60.0, -2.0),
                Point::new(60.0, 88.0),
                Point::new(110.0, 88.0),
            ],
            radius: 20.0,
            color_index: 1,
        };
        let commands = rounded_polyline(&arrow);
        assert_eq!(commands[1], PathCommand::LineTo(Point::new(60.0, 68.0)));
        let PathCommand::CubicTo(_, _, end) = commands[2] else {
            panic!("곡선이어야 합니다");
        };
        assert_eq!(end, Point::new(80.0, 88.0));
    }
}
