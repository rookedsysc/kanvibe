//! sequenceDiagram 다이어그램(sequenceDiagram-*.mjs의 parser·SequenceDB·sequenceRenderer·svgDraw).
//!
//! 시퀀스 다이어그램은 HTML 라벨을 쓰지 않고 늘 SVG `<text>`로 그리므로 모바일·데스크톱 라벨 방식이 같은 장면을
//! 만든다. 글꼴은 메시지·노트·참여자 모두 16px이고, 크기는 `getBBox()`를 반올림한 값으로 잰다.

mod actor_layout;
mod arrows;
mod bounds;
mod css_color;
mod document;
mod draw;
mod layout;
mod markers;
mod measure;
mod models;
mod parser;
mod participant_parser;
mod participants;

use crate::scene::{Color, Scene};
use crate::theme::{Theme, hsl};
use crate::{DiagramError, RenderContext};

/// conf.activationWidth
const ACTIVATION_WIDTH: f64 = 10.0;
/// conf.width·conf.height: 참여자 상자 최소 크기
const ACTOR_WIDTH: f64 = 150.0;
const ACTOR_HEIGHT: f64 = 65.0;
/// conf.actorMargin
const ACTOR_MARGIN: f64 = 50.0;
const BOX_TEXT_MARGIN: f64 = 5.0;
const NOTE_MARGIN: f64 = 10.0;
const WRAP_PADDING: f64 = 10.0;
const LABEL_BOX_WIDTH: f64 = 50.0;
const LABEL_BOX_HEIGHT: f64 = 20.0;
const DIAGRAM_MARGIN_X: f64 = 50.0;
const DIAGRAM_MARGIN_Y: f64 = 10.0;

/// 기준 SVG `<style>`의 시퀀스 다이어그램 색
#[derive(Clone, Debug)]
struct SequenceColors {
    actor_fill: Color,
    actor_stroke: Color,
    actor_text: Color,
    message_line: Color,
    message_text: Color,
    /// SVG 뿌리의 `fill`. CSS가 없는 표식(비동기·반쪽 화살촉)·중앙 연결 동그라미·제목·묶음 제목이 물려받는다
    root_fill: Color,
    /// 막대 반쪽 화살촉은 `stroke="black"` 속성 그대로다
    stick_head: Color,
    /// 묶음 상자 `drop-shadow` 색
    shadow: Color,
    number_fill: Color,
    number_text: Color,
    label_box_fill: Color,
    label_box_stroke: Color,
    block_text: Color,
    block_line: Color,
    note_fill: Color,
    note_stroke: Color,
    note_text: Color,
    activation_fill: Color,
    activation_stroke: Color,
}

impl SequenceColors {
    fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self::light(),
            Theme::Dark => Self::dark(),
        }
    }

    fn light() -> Self {
        let purple = Color::rgb(0x93, 0x70, 0xDB);
        let lavender = Color::rgb(0xEC, 0xEC, 0xFF);
        let charcoal = Color::rgb(0x33, 0x33, 0x33);
        let black = Color::rgb(0, 0, 0);
        Self {
            actor_fill: lavender,
            actor_stroke: purple,
            actor_text: black,
            message_line: charcoal,
            message_text: charcoal,
            root_fill: charcoal,
            stick_head: black,
            shadow: Color::rgb(185, 185, 185),
            number_fill: charcoal,
            number_text: Color::rgb(255, 255, 255),
            label_box_fill: lavender,
            label_box_stroke: purple,
            block_text: black,
            block_line: purple,
            note_fill: Color::rgb(0xFF, 0xF5, 0xAD),
            note_stroke: Color::rgb(0xAA, 0xAA, 0x33),
            note_text: black,
            activation_fill: Color::rgb(0xF4, 0xF4, 0xF4),
            activation_stroke: Color::rgb(0x66, 0x66, 0x66),
        }
    }

    fn dark() -> Self {
        let light_grey = Color::rgb(211, 211, 211);
        let silver = Color::rgb(0xCC, 0xCC, 0xCC);
        let slate = Color::rgb(0x1F, 0x20, 0x20);
        let note_fill = hsl(180.0, 1.587_301_587_3, 28.352_941_176_5);
        Self {
            actor_fill: slate,
            actor_stroke: silver,
            actor_text: light_grey,
            message_line: light_grey,
            message_text: light_grey,
            root_fill: silver,
            stick_head: Color::rgb(0, 0, 0),
            shadow: Color::rgb(185, 185, 185),
            number_fill: light_grey,
            number_text: Color::rgb(0, 0, 0),
            label_box_fill: slate,
            label_box_stroke: silver,
            block_text: light_grey,
            block_line: silver,
            note_fill,
            note_stroke: hsl(180.0, 0.0, 18.352_941_176_5),
            note_text: Color::rgb(184, 182, 182),
            activation_fill: note_fill,
            activation_stroke: silver,
        }
    }
}

pub fn render(definition: &str, context: &RenderContext) -> Result<Scene, DiagramError> {
    let document = parser::parse(definition)?;
    participants::check_menu_anchors(&document)?;
    let layout = layout::layout(&document, context.fonts);
    let colors = SequenceColors::for_theme(context.palette.theme);
    Ok(draw::draw(&document, &layout, context.fonts, &colors))
}

#[cfg(test)]
mod tests {
    use crate::scene::SceneItem;
    use crate::text::tests::bundled_fonts;
    use crate::theme::Theme;
    use crate::{DiagramError, LabelMode, render_scene};

    #[test]
    fn 여러_줄_글과_겹친_블록을_그린다() {
        let fonts = bundled_fonts();
        let definition = "sequenceDiagram\nautonumber 10 5\nparticipant A as Client<br/>App\nactor B as Operator\nparticipant C\nA->>B: first line<br>second line\nNote over B: single over<br/>two lines\nloop retry until done\nB-)C: poll\nopt has work\nC--)B: job\nB->>B: process\nend\nend\nautonumber off\nC-->>A: result\nNote right of C: right of last";
        let scene = render_scene(definition, Theme::Dark, LabelMode::Html, &fonts).unwrap();
        let texts: Vec<&str> = scene
            .items
            .iter()
            .filter_map(|item| match item {
                SceneItem::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect();
        for expected in ["Client", "App", "second line", "two lines", "10", "25"] {
            assert!(texts.contains(&expected), "{expected} 없음: {texts:?}");
        }
        assert!(!texts.contains(&"30"));
    }

    #[test]
    fn 라벨_방식과_상관없이_같은_장면이다() {
        let fonts = bundled_fonts();
        let definition = "sequenceDiagram\nA->>B: hi\nNote left of A: n";
        let html = render_scene(definition, Theme::Light, LabelMode::Html, &fonts).unwrap();
        let svg = render_scene(definition, Theme::Light, LabelMode::SvgText, &fonts).unwrap();
        assert_eq!(html, svg);
    }

    #[test]
    fn 그림_모양_참여자에_링크_메뉴를_붙이면_mermaid처럼_실패한다() {
        let fonts = bundled_fonts();
        let invalid = "sequenceDiagram\nparticipant B@{ \"type\": \"boundary\" } as Gateway\nlink B: Runbook @ https://runbook.example.com\nB->>B: x";
        let result = render_scene(invalid, Theme::Light, LabelMode::Html, &fonts);
        assert!(matches!(result, Err(DiagramError::Syntax(_))), "{result:?}");
        let valid = "sequenceDiagram\nparticipant B@{ \"type\": \"database\" } as Store\nlink B: Runbook @ https://runbook.example.com\nB->>B: x";
        assert!(render_scene(valid, Theme::Light, LabelMode::Html, &fonts).is_ok());
    }
}
