//! sequenceRenderer.draw의 본 배치. 사건을 차례로 읽으며 세로 위치를 쌓고, 그릴 것들을 SVG에 들어가는 순서대로 모은다.
//! 참여자·묶음·아래 참여자 자리는 [`super::actor_layout`]이 맡는다.

use super::bounds::{BlockFrame, Bounds};
use super::config::SequenceConfig;
use super::document::{
    Arrow, BlockKind, CentralConnection, SequenceDocument, SequenceEvent, SequenceMessage,
};
use super::measure::{js_round, line_height, split_breaks, text_size, wrap_label};
use super::models::{
    ActorBox, EventModels, GroupBox, MessageModel, NoteModel, build_event_models, size_actors,
};
use crate::text::FontBook;

/// 제목이 있으면 viewBox 위쪽을 이만큼 더 연다
const TITLE_SPACE: f64 = 40.0;

/// 참여자 상자와 메시지 사이 층(활성화·노트·블록). 활성화는 시작한 자리 순서로 놓인다
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LayeredItem {
    Activation {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    Note {
        model: NoteModel,
        start_y: f64,
        height: f64,
    },
    Block {
        frame: BlockFrame,
        kind: BlockKind,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlacedMessage {
    pub model: MessageModel,
    pub from: usize,
    pub to: usize,
    pub arrow: Arrow,
    pub central: Option<CentralConnection>,
    pub start_y: f64,
    pub line_y: f64,
    /// autonumber가 켜졌을 때의 번호
    pub number: Option<String>,
}

/// 그릴 참여자 묶음 상자(drawBox)
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlacedGroup {
    pub start_x: f64,
    pub start_y: f64,
    pub stop_x: f64,
    pub stop_y: f64,
    pub fill: String,
    pub title: Option<String>,
    /// 제목 줄들의 가운데
    pub title_center_x: f64,
    pub title_center_y: f64,
}

/// 그림 제목(왼쪽 정렬, 알파벳 기준선)
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlacedTitle {
    pub text: String,
    pub x: f64,
    pub baseline: f64,
}

#[derive(Debug, Default)]
pub(super) struct SequenceLayout {
    pub actors: Vec<ActorBox>,
    pub groups: Vec<PlacedGroup>,
    pub layered: Vec<LayeredItem>,
    pub messages: Vec<PlacedMessage>,
    pub backgrounds: Vec<BlockFrame>,
    pub title: Option<PlacedTitle>,
    pub view_x: f64,
    pub view_y: f64,
    pub view_width: f64,
    pub view_height: f64,
}

/// autonumber 상태
struct Numbering {
    next: f64,
    step: f64,
    is_visible: bool,
}

pub(super) struct LayoutPass<'a> {
    pub fonts: &'a FontBook,
    pub config: &'a SequenceConfig,
    pub document: &'a SequenceDocument,
    pub actors: Vec<ActorBox>,
    pub groups: Vec<GroupBox>,
    /// 묶음을 그리는 순서(bounds.models.boxes)
    pub group_order: Vec<usize>,
    pub placed_groups: Vec<PlacedGroup>,
    models: EventModels,
    pub bounds: Bounds,
    layered: Vec<Option<LayeredItem>>,
    messages: Vec<PlacedMessage>,
    backgrounds: Vec<BlockFrame>,
    numbering: Numbering,
}

pub(super) fn layout(
    document: &SequenceDocument,
    fonts: &FontBook,
    config: &SequenceConfig,
) -> SequenceLayout {
    let (actors, groups, group_title_height) = size_actors(document, fonts, config);
    let mut pass = LayoutPass {
        fonts,
        config,
        document,
        actors,
        groups,
        group_order: Vec::new(),
        placed_groups: Vec::new(),
        models: EventModels::default(),
        bounds: Bounds::new(config),
        layered: Vec::new(),
        messages: Vec::new(),
        backgrounds: Vec::new(),
        numbering: Numbering {
            next: 1.0,
            step: 1.0,
            is_visible: false,
        },
    };
    pass.place_actors(group_title_height);
    pass.models = build_event_models(document, fonts, config, &pass.actors);
    for (index, event) in document.events.iter().enumerate() {
        pass.place_event(index, event);
    }
    pass.place_footer();
    pass.place_groups();
    pass.finish()
}

/// JavaScript 숫자를 글로 바꾼 모양(정수면 소수점 없이)
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

impl LayoutPass<'_> {
    fn place_event(&mut self, index: usize, event: &SequenceEvent) {
        match event {
            SequenceEvent::Note(_) => self.place_note(index),
            SequenceEvent::ActivationStart(actor) | SequenceEvent::CentralActivation(actor) => {
                let center = self.actors[*actor].center();
                self.bounds
                    .new_activation(*actor, center, self.layered.len());
                self.layered.push(None);
            }
            SequenceEvent::ActivationEnd(actor) => self.end_activation(*actor),
            SequenceEvent::BlockStart { kind, text } => self.start_block(index, *kind, text),
            SequenceEvent::BlockSection(text) => self.add_section(index, text),
            SequenceEvent::BlockEnd(kind) => self.end_block(*kind),
            SequenceEvent::Autonumber {
                start,
                step,
                is_visible,
            } => {
                let numbering = &mut self.numbering;
                numbering.next = start
                    .filter(|value| *value != 0.0)
                    .unwrap_or(numbering.next);
                numbering.step = step.filter(|value| *value != 0.0).unwrap_or(numbering.step);
                numbering.is_visible = *is_visible;
            }
            SequenceEvent::Message(message) => self.place_message(index, message),
        }
    }

    /// drawNote: 글 높이 + 위아래 noteMargin 상자
    fn place_note(&mut self, index: usize) {
        let Some(model) = self.models.notes.get(&index).cloned() else {
            return;
        };
        self.bounds.bump(self.config.box_margin);
        let start_y = self.bounds.vertical_pos;
        let line_count = split_breaks(&model.text).len() as f64;
        let note_margin = self.config.note_margin;
        let height = js_round(line_count * line_height(self.fonts)) + 2.0 * note_margin;
        self.bounds.bump(height);
        let stop_x = model.start_x + model.width;
        self.bounds
            .insert(model.start_x, start_y, stop_x, start_y + height);
        self.layered.push(Some(LayeredItem::Note {
            model,
            start_y,
            height,
        }));
    }

    /// activeEnd: 너무 짧은 막대는 위로 늘린다
    fn end_activation(&mut self, actor: usize) {
        let Some(mut frame) = self.bounds.end_activation(actor) else {
            return;
        };
        let mut vertical = self.bounds.vertical_pos;
        if frame.start_y + 18.0 > vertical {
            frame.start_y = vertical - 6.0;
            vertical += 12.0;
        }
        self.layered[frame.slot] = Some(LayeredItem::Activation {
            x: frame.start_x,
            y: frame.start_y,
            width: frame.stop_x - frame.start_x,
            height: vertical - frame.start_y,
        });
        self.bounds
            .insert(frame.start_x, vertical - 10.0, frame.stop_x, vertical);
    }

    /// adjustLoopHeightForWrap: 미리 구한 폭이 있으면 제목을 `[...]`로 감싸 줄을 바꾸고 그 높이만큼 내린다
    fn block_title(&mut self, index: usize, text: &str, before: f64, after: f64) -> (String, f64) {
        self.bounds.bump(before);
        let width = self.models.block_widths.get(&index).copied();
        match width.filter(|_| !text.is_empty()) {
            Some(width) => {
                let wrap_width = width - 2.0 * self.config.wrap_padding;
                let title = wrap_label(self.fonts, &format!("[{text}]"), wrap_width);
                let title_height = text_size(self.fonts, &title)
                    .height
                    .max(self.config.label_box_height);
                (title, after + title_height)
            }
            None => (text.to_string(), after),
        }
    }

    fn start_block(&mut self, index: usize, kind: BlockKind, text: &str) {
        let is_background = kind == BlockKind::Rect;
        let box_margin = self.config.box_margin;
        let after = if is_background {
            box_margin
        } else {
            box_margin + self.config.box_text_margin
        };
        let (title, height_adjust) = self.block_title(index, text, box_margin, after);
        if is_background {
            self.bounds.new_block(String::new(), Some(title));
        } else {
            self.bounds.new_block(title, None);
        }
        self.bounds.bump(height_adjust);
    }

    fn add_section(&mut self, index: usize, text: &str) {
        let box_margin = self.config.box_margin;
        let before = box_margin + self.config.box_text_margin;
        let (title, height_adjust) = self.block_title(index, text, before, box_margin);
        self.bounds.add_section(title);
        self.bounds.bump(height_adjust);
    }

    fn end_block(&mut self, kind: BlockKind) {
        let Some(frame) = self.bounds.blocks.pop() else {
            return;
        };
        if let Some(stop_y) = frame.stop_y {
            self.bounds.bump(stop_y - self.bounds.vertical_pos);
        }
        if kind == BlockKind::Rect {
            self.backgrounds.push(frame);
        } else {
            self.layered.push(Some(LayeredItem::Block { frame, kind }));
        }
    }

    fn place_message(&mut self, index: usize, message: &SequenceMessage) {
        let Some(mut model) = self.models.messages.get(&index).cloned() else {
            return;
        };
        let start_y = self.bounds.vertical_pos;
        let (line_y, stop_y) = self.bound_message(&model, start_y);
        let span = (start_y, line_y, stop_y);
        self.adjust_lifecycle(index, message, &mut model, span);
        let numbering = &mut self.numbering;
        let is_numbered = numbering.is_visible || self.config.should_show_numbers;
        let number = is_numbered.then(|| format_number(numbering.next));
        numbering.next = js_round((numbering.next + numbering.step) * 100.0) / 100.0;
        self.messages.push(PlacedMessage {
            model,
            from: message.from,
            to: message.to,
            arrow: message.arrow,
            central: message.central,
            start_y,
            line_y,
            number,
        });
    }

    /// boundMessage: 글 한 줄 높이만큼 내린 뒤 선을 긋고, 자기 메시지는 고리 높이만큼 더 내린다.
    /// 꺾은 자기 메시지(rightAngles)는 boxMargin을 띄우지 않는다. 선 y와 메시지 아래 끝을 돌려준다
    fn bound_message(&mut self, model: &MessageModel, start_y: f64) -> (f64, f64) {
        let config = self.config;
        self.bounds.bump(10.0);
        let size = text_size(self.fonts, &model.text);
        let line_count = split_breaks(&model.text).len() as f64;
        let first_line = size.height / line_count;
        self.bounds.bump(first_line);
        let vertical = self.bounds.vertical_pos;
        let is_self = model.start_x == model.stop_x;
        let mut offset = size.height - 10.0;
        if !is_self || !config.has_right_angles {
            offset += config.box_margin;
        }
        let line_y = vertical + offset;
        if is_self {
            offset += 30.0;
            let half = (size.width / 2.0).max(config.actor_width / 2.0);
            self.bounds.insert(
                model.start_x - half,
                vertical - 10.0 + offset,
                model.stop_x + half,
                vertical + 30.0 + offset,
            );
        } else {
            self.bounds
                .insert(model.start_x, line_y - 10.0, model.stop_x, line_y);
        }
        self.bounds.bump(offset);
        let stop_y = start_y + first_line + offset;
        self.bounds
            .insert(model.from_bounds, start_y, model.to_bounds, stop_y);
        (line_y, stop_y)
    }

    /// viewBox: 경계 상자(팝업 메뉴가 더 크면 그 크기)에 diagramMargin을 두르고, 아래 거울 참여자가 있으면
    /// 높이를 고친다. 제목은 위쪽에 40px을 더 연다
    fn finish(self) -> SequenceLayout {
        let config = self.config;
        let extent = self.bounds.extent;
        let start_x = extent.start_x.unwrap_or(0.0);
        let stop_x = extent.stop_x.unwrap_or(0.0);
        let (menu_width, menu_height) = self.popup_menu_extent();
        let box_height =
            (extent.stop_y.unwrap_or(0.0) - extent.start_y.unwrap_or(0.0)).max(menu_height);
        let box_width = (stop_x - start_x).max(menu_width);
        let title = self.document.title.clone().map(|text| PlacedTitle {
            text,
            x: (stop_x - start_x) / 2.0 - 2.0 * config.diagram_margin_x,
            baseline: -25.0,
        });
        let title_space = if title.is_some() { TITLE_SPACE } else { 0.0 };
        let mirror_adjust = match config.should_mirror_actors {
            true => config.bottom_margin_adj - config.box_margin,
            false => 0.0,
        };
        SequenceLayout {
            actors: self.actors,
            groups: self.placed_groups,
            layered: self.layered.into_iter().flatten().collect(),
            messages: self.messages,
            backgrounds: self.backgrounds,
            title,
            view_x: start_x - config.diagram_margin_x,
            view_y: -(config.diagram_margin_y + title_space),
            view_width: box_width + 2.0 * config.diagram_margin_x,
            view_height: box_height + 2.0 * config.diagram_margin_y + mirror_adjust + title_space,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequence::parser::parse;
    use crate::text::tests::bundled_fonts;

    const BASIC: &str = include_str!("../../../../parity/diagrams/baseline/seq-basic.mmd");
    const FULL: &str = include_str!("../../../../parity/diagrams/baseline/seq-full.mmd");

    #[test]
    fn 기준_svg와_같은_viewbox를_만든다() {
        let fonts = bundled_fonts();
        let basic = layout(&parse(BASIC).unwrap(), &fonts, &SequenceConfig::default());
        assert_eq!(
            (basic.view_x, basic.view_width, basic.view_height),
            (-62.5, 796.5, 790.0)
        );
        let full = layout(&parse(FULL).unwrap(), &fonts, &SequenceConfig::default());
        assert_eq!(
            (full.view_x, full.view_width, full.view_height),
            (-150.0, 750.0, 1767.0)
        );
    }

    #[test]
    fn 메시지_선과_활성화_자리가_기준과_같다() {
        let fonts = bundled_fonts();
        let basic = layout(&parse(BASIC).unwrap(), &fonts, &SequenceConfig::default());
        let lines: Vec<f64> = basic
            .messages
            .iter()
            .map(|message| message.line_y)
            .collect();
        assert_eq!(lines, [119.0, 173.0, 227.0, 380.0, 481.0, 614.0]);
        assert_eq!(
            basic.layered[0],
            LayeredItem::Activation {
                x: 404.0,
                y: 173.0,
                width: 10.0,
                height: 54.0
            }
        );
        let full = layout(&parse(FULL).unwrap(), &fonts, &SequenceConfig::default());
        assert_eq!(full.backgrounds[0].start_x, Some(-9.0));
        assert_eq!(full.backgrounds[0].stop_y, Some(1661.0));
        assert_eq!(full.messages[19].number.as_deref(), Some("20"));
    }

    #[test]
    fn sequence_설정이_간격과_거울_참여자와_꺾은_자기_메시지를_바꾼다() {
        let fonts = bundled_fonts();
        let source = include_str!("../../../../parity/diagrams/baseline/seq-config.mmd");
        let config = SequenceConfig::read(source);
        let placed = layout(&parse(source).unwrap(), &fonts, &config);
        // seq-config 기준 SVG viewBox: -20 -30 361 463
        assert_eq!(
            (
                placed.view_x,
                placed.view_y,
                placed.view_width,
                placed.view_height
            ),
            (-20.0, -30.0, 361.0, 463.0)
        );
        let lines: Vec<f64> = placed
            .messages
            .iter()
            .map(|message| message.line_y)
            .collect();
        assert_eq!(lines, [114.0, 158.0, 252.0, 383.0]);
        assert_eq!(placed.actors[1].x, 200.0);
        assert!(placed.actors.iter().all(|actor| actor.stop_y.is_none()));
        assert_eq!(placed.messages[0].number.as_deref(), Some("1"));
    }

    #[test]
    fn 제목과_묶음과_생성_참여자를_배치한다() {
        let fonts = bundled_fonts();
        let definition = "sequenceDiagram\ntitle T\nbox Aqua G\nparticipant A\nend\nA->>B: x\ncreate participant C\nA->>C: new";
        let placed = layout(
            &parse(definition).unwrap(),
            &fonts,
            &SequenceConfig::default(),
        );
        assert_eq!(placed.view_y, -50.0);
        assert_eq!(placed.groups.len(), 1);
        assert_eq!(placed.groups[0].start_y, -5.0);
        // 묶음 제목 높이(22)와 묶음 여백(10)만큼 참여자가 내려온다
        assert_eq!(placed.actors[0].start_y, 32.0);
        let created = &placed.actors[2];
        assert!(created.start_y > placed.actors[1].start_y);
        assert_eq!(
            placed.title.as_ref().map(|title| title.baseline),
            Some(-25.0)
        );
    }
}
