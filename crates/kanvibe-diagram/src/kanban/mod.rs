//! kanban 다이어그램(kanbanRenderer.ts, 모양 kanbanSection·kanbanItem). 열은 폭 200 상자를 205 간격으로
//! 늘어놓고, 카드(폭 185)를 열 제목 아래 5 간격으로 쌓는다. 카드는 제목, 아래 왼쪽 티켓, 아래 오른쪽 담당자를
//! 두고 우선순위가 있으면 왼쪽 끝에 색 막대를 그린다. 라벨은 모바일이면 HTML, 데스크톱이면 SVG 글자로 잰다.

mod parser;

use crate::label::{FONT_SIZE, LINE_HEIGHT, LabelBox, measure_html_label, measure_svg_label};
use crate::scene::{
    Color, PathCommand, Point, Scene, SceneItem, StrokeStyle, TextItem, TextStrokeItem,
    rounded_rect,
};
use crate::text::{FontBook, FontWeight, snap_layout_unit};
use crate::theme::{DARK_SCALE, LIGHT_SCALE_HUES, Theme, hsl, lighten};
use crate::{DiagramError, LabelMode, RenderContext};
use parser::{KanbanCard, KanbanColumn};

const SECTION_WIDTH: f64 = 200.0;
const PADDING: f64 = 10.0;
/// 카드 폭(`sectionWidth − 1.5 × padding`)
const CARD_WIDTH: f64 = SECTION_WIDTH - 1.5 * PADDING;
/// 카드 라벨 줄바꿈 폭(카드 폭 − 10)
const CARD_LABEL_WIDTH: f64 = CARD_WIDTH - 10.0;
const CARD_LABEL_PADDING: f64 = 10.0;
const CORNER_RADIUS: f64 = 5.0;
const MIN_LABEL_HEIGHT: f64 = 25.0;
const MIN_SECTION_HEIGHT: f64 = 50.0;
/// 열 상자 위 끝(높이 600 상자를 가운데 0에 둔 위치)
const SECTION_TOP: f64 = -1.5 * SECTION_WIDTH;
const PRIORITY_BAR_WIDTH: f64 = 4.0;
/// `.section-N` 색 규칙이 있는 마지막 열 번호(THEME_COLOR_LIMIT 12 → −1..=10)
const LAST_STYLED_SECTION: usize = 10;

/// 기본·dark 테마의 kanban 색
struct KanbanColors {
    theme: Theme,
    card_fill: Color,
    card_stroke: Color,
    text: Color,
}

impl KanbanColors {
    fn for_context(context: &RenderContext) -> Self {
        let theme = context.palette.theme;
        Self {
            theme,
            card_fill: match theme {
                Theme::Light => Color::rgb(255, 255, 255),
                Theme::Dark => Color::rgb(0x33, 0x33, 0x33),
            },
            card_stroke: context.palette.node_stroke,
            text: context.palette.text,
        }
    }

    /// `.section-N rect`의 칠과 테두리: `cScale(N+1)`을 10% 밝힌 색. 규칙이 없는 열은 `None`
    fn section_fill(&self, section: usize) -> Option<Color> {
        if section > LAST_STYLED_SECTION {
            return None;
        }
        Some(match self.theme {
            Theme::Light => hsl(LIGHT_SCALE_HUES[section + 1], 100.0, 86.274_509_803_9),
            Theme::Dark => lighten(DARK_SCALE[section + 1], 10.0),
        })
    }

    /// SVG 글자 열 제목에만 걸리는 `.section-N text`(`cScaleLabel(N+1)`)
    fn section_svg_text(&self, section: usize) -> Color {
        match self.theme {
            _ if section > LAST_STYLED_SECTION => self.text,
            Theme::Light if section + 1 == 3 => Color::rgb(255, 255, 255),
            Theme::Light => Color::rgb(0, 0, 0),
            Theme::Dark => Color::rgb(211, 211, 211),
        }
    }
}

/// SVG getBBox와 같은 축 정렬 경계
#[derive(Clone, Copy, Debug)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Bounds {
    fn include(&mut self, left: f64, top: f64, right: f64, bottom: f64) {
        self.left = self.left.min(left);
        self.top = self.top.min(top);
        self.right = self.right.max(right);
        self.bottom = self.bottom.max(bottom);
    }
}

/// 그리는 동안 함께 쓰는 값
struct KanbanCanvas<'a> {
    fonts: &'a FontBook,
    mode: LabelMode,
    colors: KanbanColors,
    /// `kanban.ticketBaseUrl`이 있으면 티켓을 링크로 그린다
    has_ticket_links: bool,
    scene: Scene,
    bounds: Bounds,
}

impl KanbanCanvas<'_> {
    fn measure(&self, text: &str, wrap_width: f64) -> LabelBox {
        match self.mode {
            LabelMode::Html => measure_html_label(self.fonts, text, FONT_SIZE, wrap_width),
            LabelMode::SvgText => measure_svg_label(self.fonts, text, FONT_SIZE, wrap_width),
        }
    }

    /// 라벨 묶음 원점(왼쪽 위)이 (left, top)일 때 줄마다 글자를 만들고 getBBox 범위를 넓힌다. HTML은 24px
    /// 줄 상자, SVG 글자는 `tspan`(첫 기준선 1em, 줄 간격 1.1em)이고, 열 제목만 HTML에서 가운데 맞춘다
    fn label_texts(
        &mut self,
        label: &LabelBox,
        origin: Point,
        is_centered: bool,
        color: Color,
    ) -> Vec<(TextItem, f64)> {
        let metrics = self.fonts.primary_metrics(FONT_SIZE);
        let half_leading = ((LINE_HEIGHT - (metrics.ascent + metrics.descent)) / 2.0).floor();
        let mut items = Vec::new();
        for (index, line) in label.lines.iter().enumerate() {
            if line.text.is_empty() {
                continue;
            }
            let (x, baseline) = match self.mode {
                LabelMode::Html => {
                    let offset = if is_centered {
                        snap_layout_unit((label.width - line.width) / 2.0)
                    } else {
                        0.0
                    };
                    let top = origin.y + index as f64 * LINE_HEIGHT;
                    (origin.x + offset, top + half_leading + metrics.ascent)
                }
                LabelMode::SvgText => (origin.x, origin.y + FONT_SIZE * (1.0 + 1.1 * index as f64)),
            };
            let item = TextItem {
                text: line.text.clone(),
                x,
                baseline,
                font_size: FONT_SIZE,
                weight: FontWeight::Regular,
                color,
                rotation: None,
            };
            items.push((item, line.width));
        }
        if !label.is_empty() {
            let top = label.box_top(self.fonts, origin.y);
            self.bounds
                .include(origin.x, top, origin.x + label.width, top + label.height);
        }
        items
    }

    fn push_label(&mut self, label: &LabelBox, origin: Point, is_centered: bool, color: Color) {
        for (item, _) in self.label_texts(label, origin, is_centered, color) {
            self.scene.push_text(item);
        }
    }

    /// `ticketBaseUrl`이 있으면 티켓 라벨을 `<a class="kanban-ticket-link">`로 감싼다. 밑줄(기준선 1px 아래, 굵기
    /// 1)이 생기고, SVG 글자는 링크의 `stroke`(노드 테두리색 1px)를 물려받아 글자와 밑줄 윤곽이 그어진다
    fn push_ticket_link(&mut self, label: &LabelBox, origin: Point, color: Color) {
        let outline = (self.mode == LabelMode::SvgText).then(|| StrokeStyle {
            color: self.colors.card_stroke,
            width: 1.0,
            dash: Vec::new(),
        });
        for (item, width) in self.label_texts(label, origin, false, color) {
            let underline = rounded_rect(item.x, item.baseline + 1.0, width, 1.0, 0.0);
            self.scene
                .push_path(underline, Some(color), outline.clone());
            self.scene.push_text(item.clone());
            if let Some(stroke) = &outline {
                self.scene.items.push(SceneItem::TextStroke(TextStrokeItem {
                    text: item,
                    color: stroke.color,
                    width: stroke.width,
                    is_round_join: false,
                }));
            }
        }
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let columns = parser::parse(&crate::directive::diagram_body(definition))?;
    let config = crate::directive::read_config(definition);
    let ticket_base_url = config
        .section("kanban")
        .and_then(|section| section.get("ticketBaseUrl"))
        .and_then(serde_json::Value::as_str);
    let mut canvas = KanbanCanvas {
        fonts: context.fonts,
        mode: context.label_mode,
        colors: KanbanColors::for_context(context),
        has_ticket_links: ticket_base_url.is_some_and(|url| !url.is_empty()),
        scene: Scene::default(),
        bounds: Bounds {
            left: f64::INFINITY,
            top: f64::INFINITY,
            right: f64::NEG_INFINITY,
            bottom: f64::NEG_INFINITY,
        },
    };
    let titles: Vec<LabelBox> = columns
        .iter()
        .map(|column| canvas.measure(&column.label, SECTION_WIDTH))
        .collect();
    let label_height = titles
        .iter()
        .map(|title| title.height)
        .fold(MIN_LABEL_HEIGHT, f64::max);
    let cards: Vec<Vec<PlacedCard>> = columns
        .iter()
        .map(|column| place_cards(&canvas, column, label_height))
        .collect();
    for (index, (title, column_cards)) in titles.iter().zip(&cards).enumerate() {
        draw_section(&mut canvas, index, title, column_cards, label_height);
    }
    for (index, column_cards) in cards.iter().enumerate() {
        let center_x = section_center_x(index);
        for card in column_cards {
            draw_card(&mut canvas, center_x, card);
        }
    }
    Ok(finish_scene(canvas))
}

fn finish_scene(canvas: KanbanCanvas) -> Scene {
    let mut scene = canvas.scene;
    let bounds = canvas.bounds;
    if bounds.left > bounds.right {
        return scene;
    }
    scene.translate(PADDING - bounds.left, PADDING - bounds.top);
    scene.width = bounds.right - bounds.left + 2.0 * PADDING;
    scene.height = bounds.bottom - bounds.top + 2.0 * PADDING;
    scene
}

/// `sectionWidth × 번호 + (번호 − 1) × padding / 2`(번호는 1부터)
fn section_center_x(index: usize) -> f64 {
    let number = (index + 1) as f64;
    SECTION_WIDTH * number + (number - 1.0) * PADDING / 2.0
}

/// 잰 라벨과 세로 가운데가 정해진 카드
struct PlacedCard<'a> {
    card: &'a KanbanCard,
    title: LabelBox,
    ticket: LabelBox,
    assigned: LabelBox,
    center_y: f64,
    height: f64,
}

/// 카드 높이는 제목 + 위아래 10 + 티켓·담당자 줄 높이의 절반이다. 카드 사이는 5
fn place_cards<'a>(
    canvas: &KanbanCanvas,
    column: &'a KanbanColumn,
    label_height: f64,
) -> Vec<PlacedCard<'a>> {
    let mut y = SECTION_TOP + label_height;
    let mut placed = Vec::with_capacity(column.cards.len());
    for card in &column.cards {
        let title = canvas.measure(&card.label, CARD_LABEL_WIDTH);
        let ticket = canvas.measure(&card.ticket, CARD_LABEL_WIDTH);
        let assigned = canvas.measure(&card.assigned, CARD_LABEL_WIDTH);
        let height =
            title.height + 2.0 * CARD_LABEL_PADDING + ticket.height.max(assigned.height) / 2.0;
        let center_y = y + height / 2.0;
        y = center_y + height / 2.0 + PADDING / 2.0;
        placed.push(PlacedCard {
            card,
            title,
            ticket,
            assigned,
            center_y,
            height,
        });
    }
    placed
}

/// 열 상자(카드가 끝난 곳 + 30, 최소 50)와 상자 위 끝 가운데의 제목
fn draw_section(
    canvas: &mut KanbanCanvas,
    index: usize,
    title: &LabelBox,
    cards: &[PlacedCard],
    label_height: f64,
) {
    let center_x = section_center_x(index);
    let top = SECTION_TOP + label_height;
    let end = cards.last().map_or(top, |card| {
        card.center_y + card.height / 2.0 + PADDING / 2.0
    });
    let height =
        (end - top + 3.0 * PADDING).max(MIN_SECTION_HEIGHT) + (label_height - MIN_LABEL_HEIGHT);
    let left = center_x - SECTION_WIDTH / 2.0;
    let section = index + 1;
    let fill = canvas.colors.section_fill(section);
    let stroke = fill.map(|color| StrokeStyle {
        color,
        width: 1.0,
        dash: Vec::new(),
    });
    canvas.scene.push_path(
        rounded_rect(left, SECTION_TOP, SECTION_WIDTH, height, CORNER_RADIUS),
        Some(fill.unwrap_or(Color::rgb(0, 0, 0))),
        stroke,
    );
    canvas.bounds.include(
        left,
        SECTION_TOP,
        left + SECTION_WIDTH,
        SECTION_TOP + height,
    );
    let color = match canvas.mode {
        LabelMode::Html => canvas.colors.text,
        LabelMode::SvgText => canvas.colors.section_svg_text(section),
    };
    let origin = Point::new(center_x - title.width / 2.0, SECTION_TOP);
    canvas.push_label(title, origin, true, color);
}

fn draw_card(canvas: &mut KanbanCanvas, center_x: f64, placed: &PlacedCard) {
    let (left, top) = (
        center_x - CARD_WIDTH / 2.0,
        placed.center_y - placed.height / 2.0,
    );
    canvas.scene.push_path(
        rounded_rect(left, top, CARD_WIDTH, placed.height, CORNER_RADIUS),
        Some(canvas.colors.card_fill),
        Some(StrokeStyle {
            color: canvas.colors.card_stroke,
            width: 1.0,
            dash: Vec::new(),
        }),
    );
    canvas
        .bounds
        .include(left, top, left + CARD_WIDTH, top + placed.height);
    let shift = placed.ticket.height.max(placed.assigned.height) / 2.0;
    let label_left = left + CARD_LABEL_PADDING;
    let title_top = placed.center_y - shift - placed.title.height / 2.0;
    let info_top = placed.center_y - shift + placed.title.height / 2.0;
    let color = canvas.colors.text;
    canvas.push_label(
        &placed.title,
        Point::new(label_left, title_top),
        false,
        color,
    );
    let ticket_origin = Point::new(label_left, info_top);
    if canvas.has_ticket_links && !placed.card.ticket.is_empty() {
        canvas.push_ticket_link(&placed.ticket, ticket_origin, color);
    } else {
        canvas.push_label(&placed.ticket, ticket_origin, false, color);
    }
    let assigned_left = center_x + CARD_LABEL_PADDING + CARD_WIDTH / 2.0
        - placed.assigned.width
        - 2.0 * CARD_LABEL_PADDING;
    canvas.push_label(
        &placed.assigned,
        Point::new(assigned_left, info_top),
        false,
        color,
    );
    if let Some(color) = placed.card.priority.as_deref().and_then(priority_color) {
        draw_priority_bar(canvas, Point::new(left, top), placed.height, color);
    }
}

/// 카드 왼쪽 끝 안쪽 2px에 세운 굵기 4 막대. 위아래는 모서리 반지름의 절반(내림)만큼 비운다
fn draw_priority_bar(canvas: &mut KanbanCanvas, card_origin: Point, height: f64, color: Color) {
    let bar_x = card_origin.x + 2.0;
    let inset = (CORNER_RADIUS / 2.0).floor();
    canvas.scene.push_path(
        vec![
            PathCommand::MoveTo(Point::new(bar_x, card_origin.y + inset)),
            PathCommand::LineTo(Point::new(bar_x, card_origin.y + height - inset)),
        ],
        None,
        Some(StrokeStyle {
            color,
            width: PRIORITY_BAR_WIDTH,
            dash: Vec::new(),
        }),
    );
}

/// kanbanItem `colorFromPriority`. `Medium`과 모르는 값은 막대가 없다
fn priority_color(priority: &str) -> Option<Color> {
    match priority {
        "Very High" => Some(Color::rgb(255, 0, 0)),
        "High" => Some(Color::rgb(255, 165, 0)),
        "Low" => Some(Color::rgb(0, 0, 255)),
        "Very Low" => Some(Color::rgb(173, 216, 230)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Palette;

    fn cards_for(definition: &str, mode: LabelMode) -> Vec<(f64, f64)> {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: mode,
        };
        let canvas = KanbanCanvas {
            fonts: &fonts,
            mode,
            colors: KanbanColors::for_context(&context),
            has_ticket_links: false,
            scene: Scene::default(),
            bounds: Bounds {
                left: 0.0,
                top: 0.0,
                right: 0.0,
                bottom: 0.0,
            },
        };
        let columns = parser::parse(definition).unwrap();
        place_cards(&canvas, &columns[0], MIN_LABEL_HEIGHT)
            .iter()
            .map(|card| (card.center_y, card.height))
            .collect()
    }

    #[test]
    fn 기준_svg와_같은_카드_위치와_높이를_낸다() {
        let definition = "kanban\n  id1[Backlog]\n    id2[Write release notes]@{ assigned: 'knsv' }\n    id3[Fix]@{ priority: 'Low' }";
        // kanban-metadata--light.svg: translate(200, -247) 높이 56, 다음 카드 높이 44
        assert_eq!(
            cards_for(definition, LabelMode::Html),
            vec![(-247.0, 56.0), (-192.0, 44.0)]
        );
        // kanban-metadata--desktop--light.svg: translate(200, -248.5) 높이 53
        assert_eq!(cards_for(definition, LabelMode::SvgText)[0], (-248.5, 53.0));
    }

    #[test]
    fn 열_색은_다음_테마_색을_밝힌_값이다() {
        let fonts = bundled_fonts();
        let palette = Palette::for_theme(Theme::Light);
        let context = RenderContext {
            fonts: &fonts,
            palette: &palette,
            label_mode: LabelMode::Html,
        };
        let colors = KanbanColors::for_context(&context);
        assert_eq!(
            colors.section_fill(1),
            Some(hsl(80.0, 100.0, 86.274_509_803_9))
        );
        assert_eq!(colors.section_fill(11), None);
        assert_eq!(colors.section_svg_text(2), Color::rgb(255, 255, 255));
    }
}
