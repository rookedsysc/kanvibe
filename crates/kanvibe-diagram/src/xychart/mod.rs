//! xychart 다이어그램(xychartRenderer.ts와 chartBuilder/orchestrator.ts).
//!
//! 차트(기본 700×500)에서 그림 영역이 먼저 `plotReservedSpacePercent`(기본 절반)를 잡고, 제목·x축·y축이 남은
//! 공간을 차례로 나눠 가진 뒤 남는 공간은 다시 그림 영역에 돌려준다. 세로 차트는 x축이 아래·y축이 왼쪽이고,
//! 가로 차트(`horizontal`)는 x축이 왼쪽·y축이 위다. 그리는 순서는 배경, 제목, 계열(막대·꺾은선), x축, y축이다.
//! 크기·축 표시·데이터 라벨은 `config.xyChart`로 바꿀 수 있다.

mod axis;
mod config;
mod data_label;
mod parser;
mod ticks;

use crate::chart_text::{AnchoredText, BaselineAnchor, TextAnchor, push_anchored_text};
use crate::directive::read_config;
use crate::sanitize::sanitize_text;
use crate::scene::{Color, PathCommand, Point, Scene, StrokeStyle, polygon};
use crate::text::FontBook;
use crate::{DiagramError, RenderContext};

use axis::{AxisPosition, Bounds, ChartAxis, max_text_dimension};
use config::{ChartColors, ChartConfig};
use parser::{Plot, PlotKind, XyChart};

const LINE_STROKE_WIDTH: f64 = 2.0;
const BAR_PADDING_PERCENT: f64 = 0.05;

/// 배치가 끝난 차트: 제목 상자 높이, 그림 영역, 두 축
struct ChartLayout {
    title_height: f64,
    plot: Bounds,
    x_axis: ChartAxis,
    y_axis: ChartAxis,
}

/// 설정과 정의를 합친 그리기 입력
struct ChartInput {
    chart: XyChart,
    config: ChartConfig,
    colors: ChartColors,
    is_horizontal: bool,
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
    let config = ChartConfig::read(&directives);
    let input = ChartInput {
        is_horizontal: chart.header_orientation.unwrap_or(config.is_horizontal),
        colors: ChartColors::for_theme(context.palette.theme),
        chart,
        config,
    };
    let layout = ChartLayout::new(&input, context.fonts);
    let mut scene = Scene {
        width: input.config.width,
        height: input.config.height,
        items: Vec::new(),
        is_start_aligned: false,
    };
    draw_background_and_title(&mut scene, context.fonts, &input, layout.title_height);
    for plot in &input.chart.plots {
        let color = input.colors.plot(plot.index);
        match plot.kind {
            PlotKind::Line => draw_line(&mut scene, &layout, &input, plot, color),
            PlotKind::Bar => draw_bars(&mut scene, context.fonts, &layout, &input, plot),
        }
    }
    layout.x_axis.draw(&mut scene, context.fonts);
    layout.y_axis.draw(&mut scene, context.fonts);
    Ok(scene)
}

/// 차트 전체를 배경색으로 채우고, 제목은 제목 상자 가운데에 둔다
fn draw_background_and_title(
    scene: &mut Scene,
    fonts: &FontBook,
    input: &ChartInput,
    title_height: f64,
) {
    let (width, height) = (scene.width, scene.height);
    let background = [
        Point::new(0.0, 0.0),
        Point::new(width, 0.0),
        Point::new(width, height),
        Point::new(0.0, height),
    ];
    scene.push_path(polygon(&background), Some(input.colors.background), None);
    if title_height <= 0.0 {
        return;
    }
    let title = AnchoredText {
        text: &input.chart.title,
        x: width / 2.0,
        y: title_height / 2.0,
        font_size: input.config.title_font_size,
        color: input.colors.text,
        anchor: TextAnchor::Middle,
        baseline: BaselineAnchor::Middle,
        degrees: 0.0,
    };
    push_anchored_text(scene, fonts, &title);
}

/// 제목 높이(글자 높이 + 위아래 여백). 제목이 없거나 `showTitle`이 거짓이면 0이다
fn title_height(input: &ChartInput, fonts: &FontBook) -> f64 {
    let title = &input.chart.title;
    if title.is_empty() || !input.config.show_title {
        return 0.0;
    }
    let font_size = input.config.title_font_size;
    let (_, height) = max_text_dimension(fonts, std::slice::from_ref(title), font_size);
    height + input.config.title_padding * 2.0
}

impl ChartLayout {
    fn new(input: &ChartInput, fonts: &FontBook) -> Self {
        let (config, colors) = (&input.config, &input.colors);
        let (x_position, y_position) = if input.is_horizontal {
            (AxisPosition::Left, AxisPosition::Top)
        } else {
            (AxisPosition::Bottom, AxisPosition::Left)
        };
        let x_data = input.chart.x_axis.clone();
        let y_data = input.chart.y_axis.clone();
        let mut layout = Self {
            title_height: title_height(input, fonts),
            plot: Bounds::default(),
            x_axis: ChartAxis::new(x_data, x_position, config.x_axis, colors.text),
            y_axis: ChartAxis::new(y_data, y_position, config.y_axis, colors.text),
        };
        if input.is_horizontal {
            layout.place_horizontal(fonts, config);
        } else {
            layout.place_vertical(fonts, config);
        }
        let plots = &input.chart.plots;
        if plots.iter().any(|plot| plot.kind == PlotKind::Bar) {
            layout.x_axis.widen_outer_padding_for_bars();
        }
        layout
    }

    /// 그림 영역이 처음 잡는 몫(내림)
    fn reserved_plot_size(config: &ChartConfig) -> (f64, f64) {
        let ratio = config.plot_reserved_space_percent / 100.0;
        (
            (config.width * ratio).floor(),
            (config.height * ratio).floor(),
        )
    }

    /// calculateVerticalSpace: 제목 → 아래 x축 → 왼쪽 y축
    fn place_vertical(&mut self, fonts: &FontBook, config: &ChartConfig) {
        let (mut plot_width, mut plot_height) = Self::reserved_plot_size(config);
        let mut available_width = config.width - plot_width;
        let mut available_height = config.height - plot_height - self.title_height;
        let plot_y = self.title_height;
        let (_, x_axis_height) =
            self.x_axis
                .calculate_space(fonts, available_width, available_height);
        available_height -= x_axis_height;
        let (y_axis_width, _) =
            self.y_axis
                .calculate_space(fonts, available_width, available_height);
        let plot_x = y_axis_width;
        available_width -= y_axis_width;
        plot_width += available_width.max(0.0);
        plot_height += available_height.max(0.0);
        self.plot = Bounds {
            x: plot_x,
            y: plot_y,
            width: plot_width,
            height: plot_height,
        };
        self.x_axis.set_range(plot_x, plot_x + plot_width);
        self.x_axis.bounds.x = plot_x;
        self.x_axis.bounds.y = plot_y + plot_height;
        self.y_axis.set_range(plot_y, plot_y + plot_height);
        self.y_axis.bounds.x = 0.0;
        self.y_axis.bounds.y = plot_y;
    }

    /// calculateHorizontalSpace: 제목 → 왼쪽 x축 → 위 y축
    fn place_horizontal(&mut self, fonts: &FontBook, config: &ChartConfig) {
        let (mut plot_width, mut plot_height) = Self::reserved_plot_size(config);
        let mut available_width = config.width - plot_width;
        let mut available_height = config.height - plot_height - self.title_height;
        let (x_axis_width, _) =
            self.x_axis
                .calculate_space(fonts, available_width, available_height);
        available_width -= x_axis_width;
        let plot_x = x_axis_width;
        let (_, y_axis_height) =
            self.y_axis
                .calculate_space(fonts, available_width, available_height);
        available_height -= y_axis_height;
        let plot_y = self.title_height + y_axis_height;
        plot_width += available_width.max(0.0);
        plot_height += available_height.max(0.0);
        self.plot = Bounds {
            x: plot_x,
            y: plot_y,
            width: plot_width,
            height: plot_height,
        };
        self.y_axis.set_range(plot_x, plot_x + plot_width);
        self.y_axis.bounds.x = plot_x;
        self.y_axis.bounds.y = self.title_height;
        self.x_axis.set_range(plot_y, plot_y + plot_height);
        self.x_axis.bounds.x = 0.0;
        self.x_axis.bounds.y = plot_y;
    }
}

/// 계열 점의 (범주 위치, 값 위치)
fn plot_positions(layout: &ChartLayout, plot: &Plot) -> Vec<(f64, f64)> {
    plot.points
        .iter()
        .map(|(category, value)| {
            (
                layout.x_axis.scale_category(category),
                layout.y_axis.scale_value(*value),
            )
        })
        .collect()
}

fn draw_line(
    scene: &mut Scene,
    layout: &ChartLayout,
    input: &ChartInput,
    plot: &Plot,
    color: Color,
) {
    let points: Vec<Point> = plot_positions(layout, plot)
        .into_iter()
        .map(|(category, value)| {
            if input.is_horizontal {
                Point::new(value, category)
            } else {
                Point::new(category, value)
            }
        })
        .collect();
    let Some((first, rest)) = points.split_first() else {
        return;
    };
    let mut commands = vec![PathCommand::MoveTo(*first)];
    commands.extend(rest.iter().map(|point| PathCommand::LineTo(*point)));
    let stroke = StrokeStyle {
        color,
        width: LINE_STROKE_WIDTH,
        dash: Vec::new(),
    };
    scene.push_path(commands, None, Some(stroke));
}

/// 막대 폭은 바깥 여백 두 배와 눈금 간격 중 작은 값의 95%다. 폭이나 높이가 0 이하인 막대는 SVG가 그리지 않는다.
/// `showDataLabel`이면 막대를 다 그린 뒤 같은 묶음에 값 라벨을 둔다
fn draw_bars(
    scene: &mut Scene,
    fonts: &FontBook,
    layout: &ChartLayout,
    input: &ChartInput,
    plot: &Plot,
) {
    let bars = bar_bounds(layout, input, plot);
    let color = input.colors.plot(plot.index);
    for bar in bars
        .iter()
        .filter(|bar| bar.width > 0.0 && bar.height > 0.0)
    {
        let corners = [
            Point::new(bar.x, bar.y),
            Point::new(bar.x + bar.width, bar.y),
            Point::new(bar.x + bar.width, bar.y + bar.height),
            Point::new(bar.x, bar.y + bar.height),
        ];
        scene.push_path(polygon(&corners), Some(color), None);
    }
    if input.config.show_data_label {
        let labels = data_label::label_texts(&input.chart);
        let style = data_label::DataLabelStyle {
            is_horizontal: input.is_horizontal,
            is_outside: input.config.show_data_label_outside_bar,
            color: input.colors.text,
        };
        data_label::draw_data_labels(scene, fonts, &bars, &labels, &style);
    }
}

/// 막대 상자. 세로 차트는 그림 영역 아래 끝까지, 가로 차트는 그림 영역 왼쪽 끝부터 잡는다
fn bar_bounds(layout: &ChartLayout, input: &ChartInput, plot: &Plot) -> Vec<Bounds> {
    let x_axis = &layout.x_axis;
    let bar_width =
        f64::min(x_axis.outer_padding * 2.0, x_axis.tick_distance()) * (1.0 - BAR_PADDING_PERCENT);
    let half = bar_width / 2.0;
    let plot_bounds = layout.plot;
    plot_positions(layout, plot)
        .into_iter()
        .map(|(category, value)| {
            if input.is_horizontal {
                Bounds {
                    x: plot_bounds.x,
                    y: category - half,
                    width: value - plot_bounds.x,
                    height: bar_width,
                }
            } else {
                Bounds {
                    x: category - half,
                    y: value,
                    width: bar_width,
                    height: plot_bounds.y + plot_bounds.height - value,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::directive::DiagramConfig;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;

    fn default_input(definition: &str) -> ChartInput {
        let directives = DiagramConfig::default();
        let config = ChartConfig::read(&directives);
        ChartInput {
            chart: parser::parse(definition).unwrap(),
            colors: ChartColors::for_theme(Theme::Light),
            is_horizontal: false,
            config,
        }
    }

    #[test]
    fn 세로_차트_배치는_기준_svg와_같다() {
        let fonts = bundled_fonts();
        let input = default_input(
            "xychart-beta\n title \"Sales Revenue\"\n x-axis [jan, feb, mar, apr, may, jun, jul, aug]\n y-axis \"Revenue (in $)\" 4000 --> 11000\n bar [5000, 6000, 7500, 8200, 9500, 10500, 9000, 7800]",
        );
        let layout = ChartLayout::new(&input, &fonts);
        assert_eq!(layout.title_height, 47.0);
        assert!((layout.plot.x - 89.046_876).abs() < 0.01);
        assert_eq!(layout.plot.height, 417.0);
        assert_eq!(layout.x_axis.outer_padding, 25.0);
        let ticks = layout.y_axis.ticks();
        assert_eq!(ticks[0].0, "11000");
        assert_eq!(ticks[0].1, 56.5);
    }
}
