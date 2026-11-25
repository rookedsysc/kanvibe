//! 본 배치 전에 미리 구하는 값: 참여자 크기·간격(getMaxMessageWidthPerActor·calculateActorMargins),
//! 메시지·노트 모델(buildMessageModel·buildNoteModel), 블록 제목 폭(calculateLoopBounds).

use std::collections::HashMap;

use super::bounds::{ActivationFrame, Bounds};
use super::config::SequenceConfig;
use super::document::{
    BlockKind, CentralConnection, NotePlacement, Participant, SequenceDocument, SequenceEvent,
    SequenceMessage, SequenceNote,
};
use super::measure::{text_size, wrap_label};
use crate::text::FontBook;

/// 참여자 상자 자리
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ActorBox {
    pub x: f64,
    pub width: f64,
    pub height: f64,
    /// 메시지 글이 요구한 다음 참여자까지 간격(없으면 기본 간격)
    pub required_margin: Option<f64>,
    pub start_y: f64,
    /// 아래 참여자 자리. 소멸한 참여자는 소멸 메시지 자리다
    pub stop_y: Option<f64>,
    /// 그릴 설명(`wrap:`이면 줄을 바꾼 글)
    pub label: String,
}

impl ActorBox {
    pub fn center(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn margin(&self, config: &SequenceConfig) -> f64 {
        self.required_margin.unwrap_or(config.actor_margin)
    }
}

/// 참여자 묶음(box) 자리
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct GroupBox {
    pub x: f64,
    pub width: f64,
    /// 묶음 안쪽 좌우 여백
    pub margin: f64,
    pub title: Option<String>,
}

/// buildMessageModel 결과
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct MessageModel {
    pub start_x: f64,
    pub stop_x: f64,
    pub width: f64,
    /// 양쪽 참여자 활성화 경계 중 가장 왼쪽·오른쪽
    pub from_bounds: f64,
    pub to_bounds: f64,
    /// 그릴 글(`wrap:`이면 줄을 바꾼 글)
    pub text: String,
}

/// buildNoteModel 결과(가로 자리와 그릴 글)
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct NoteModel {
    pub start_x: f64,
    pub width: f64,
    pub text: String,
}

/// 사건 순번별 모델과 블록 제목을 감쌀 폭(사건 순번 → 폭)
#[derive(Debug, Default)]
pub(super) struct EventModels {
    pub messages: HashMap<usize, MessageModel>,
    pub notes: HashMap<usize, NoteModel>,
    pub block_widths: HashMap<usize, f64>,
}

/// 메시지·노트 글이 참여자 사이 간격을 얼마나 요구하는지(getMaxMessageWidthPerActor)
fn message_width_per_actor(
    document: &SequenceDocument,
    fonts: &FontBook,
    config: &SequenceConfig,
) -> HashMap<usize, f64> {
    let mut widths: HashMap<usize, f64> = HashMap::new();
    for event in &document.events {
        let (from, to, text, is_wrapped, placement) = match event {
            SequenceEvent::Message(message) => (
                message.from,
                message.to,
                &message.text,
                message.is_wrapped,
                None,
            ),
            SequenceEvent::Note(note) => (
                note.from,
                note.to,
                &note.text,
                note.is_wrapped,
                Some(note.placement),
            ),
            _ => continue,
        };
        let text = if is_wrapped {
            wrap_label(fonts, text, config.wrapped_text_width())
        } else {
            text.clone()
        };
        let width = text_size(fonts, &text).width + 2.0 * config.wrap_padding;
        let receiver = &document.participants[to];
        for (target, required) in width_targets(receiver, from, to, placement, width) {
            let entry = widths.entry(target).or_insert(0.0);
            *entry = entry.max(required);
        }
    }
    widths
}

/// 글 폭을 어느 참여자의 간격에 얹는지
fn width_targets(
    receiver: &Participant,
    from: usize,
    to: usize,
    placement: Option<NotePlacement>,
    width: f64,
) -> Vec<(usize, f64)> {
    match placement {
        None if Some(from) == receiver.next => vec![(to, width)],
        None if Some(from) == receiver.previous => vec![(from, width)],
        None if from == to => vec![(from, width / 2.0), (to, width / 2.0)],
        None => Vec::new(),
        Some(NotePlacement::RightOf) if receiver.next.is_some() => vec![(from, width)],
        Some(NotePlacement::RightOf) => Vec::new(),
        Some(NotePlacement::LeftOf) => receiver
            .previous
            .map(|previous| (previous, width))
            .into_iter()
            .collect(),
        Some(NotePlacement::Over) => {
            let previous = receiver.previous.map(|previous| (previous, width / 2.0));
            let own = receiver.next.map(|_| (from, width / 2.0));
            previous.into_iter().chain(own).collect()
        }
    }
}

/// 참여자 크기. `wrap:` 설명은 상자 폭 안에서 줄을 바꾸고 높이가 늘 수 있다
fn sized_actor(fonts: &FontBook, config: &SequenceConfig, participant: &Participant) -> ActorBox {
    let label = if participant.is_wrapped {
        wrap_label(fonts, &participant.description, config.wrapped_text_width())
    } else {
        participant.description.clone()
    };
    let size = text_size(fonts, &label);
    let (width, height) = if participant.is_wrapped {
        (config.actor_width, size.height.max(config.actor_height))
    } else {
        (
            config
                .actor_width
                .max(size.width + 2.0 * config.wrap_padding),
            config.actor_height,
        )
    };
    ActorBox {
        width,
        height,
        label,
        ..ActorBox::default()
    }
}

/// calculateActorMargins: 참여자 크기·간격과 묶음 여백. 모든 참여자 높이는 가장 큰 높이로 맞춘다
pub(super) fn size_actors(
    document: &SequenceDocument,
    fonts: &FontBook,
    config: &SequenceConfig,
) -> (Vec<ActorBox>, Vec<GroupBox>, f64) {
    let mut actors: Vec<ActorBox> = document
        .participants
        .iter()
        .map(|participant| sized_actor(fonts, config, participant))
        .collect();
    let margin = config.actor_margin;
    for (index, message_width) in message_width_per_actor(document, fonts, config) {
        let next_half = document.participants[index]
            .next
            .map_or(0.0, |next| actors[next].width / 2.0);
        let required = message_width + margin - actors[index].width / 2.0 - next_half;
        actors[index].required_margin = Some(required.max(margin));
    }
    let tallest = actors
        .iter()
        .map(|actor| actor.height)
        .fold(config.actor_height, f64::max);
    for actor in &mut actors {
        actor.height = actor.height.max(tallest);
    }
    let (groups, title_height) = size_groups(document, fonts, config, &actors);
    (actors, groups, title_height)
}

/// 묶음 제목이 참여자 폭보다 넓으면 양옆 여백을 늘린다. 제목 높이 최댓값도 돌려준다
fn size_groups(
    document: &SequenceDocument,
    fonts: &FontBook,
    config: &SequenceConfig,
    actors: &[ActorBox],
) -> (Vec<GroupBox>, f64) {
    let mut title_height: f64 = 0.0;
    let mut groups = Vec::new();
    for group in &document.groups {
        let members_width: f64 = group
            .members
            .iter()
            .map(|member| actors[*member].width + actors[*member].required_margin.unwrap_or(0.0))
            .sum();
        let total_width = members_width + config.box_margin * 8.0 - 2.0 * config.box_text_margin;
        let title = group.title.as_ref().map(|title| match group.is_wrapped {
            true => wrap_label(fonts, title, total_width - 2.0 * config.wrap_padding),
            false => title.clone(),
        });
        let size = text_size(fonts, title.as_deref().unwrap_or(""));
        title_height = title_height.max(size.height);
        let minimum_width = total_width.max(size.width + 2.0 * config.wrap_padding);
        groups.push(GroupBox {
            margin: config.box_text_margin + (minimum_width - total_width) / 2.0,
            title,
            ..GroupBox::default()
        });
    }
    (groups, title_height)
}

/// 참여자 생명선 둘레의 활성화 막대까지 넣은 좌우 경계(activationBounds)
pub(super) fn activation_bounds(
    actors: &[ActorBox],
    activations: &[ActivationFrame],
    actor: usize,
) -> (f64, f64) {
    let center = actors[actor].center();
    activations
        .iter()
        .filter(|activation| activation.actor == actor)
        .fold((center - 1.0, center + 1.0), |(left, right), activation| {
            (left.min(activation.start_x), right.max(activation.stop_x))
        })
}

/// calculateCentralConnectionOffset: 보내는 쪽 동그라미만큼 선 시작을 민다
fn central_offset(message: &SequenceMessage, is_rightward: bool) -> f64 {
    let has_source_circle = matches!(
        message.central,
        Some(CentralConnection::Source | CentralConnection::Both)
    );
    match (has_source_circle, message.arrow.is_bidirectional()) {
        (false, _) => 0.0,
        (true, true) if !is_rightward => 4.0 - 6.0,
        (true, _) => 4.0,
    }
}

/// buildMessageModel: 화살표 끝을 활성화 막대와 화살촉 길이만큼 당긴다
fn message_model(
    fonts: &FontBook,
    config: &SequenceConfig,
    actors: &[ActorBox],
    activations: &[ActivationFrame],
    message: &SequenceMessage,
) -> MessageModel {
    let (from_left, from_right) = activation_bounds(actors, activations, message.from);
    let (to_left, to_right) = activation_bounds(actors, activations, message.to);
    let is_rightward = from_left <= to_left;
    let mut start_x = if is_rightward { from_right } else { from_left };
    let mut stop_x = if is_rightward { to_left } else { to_right };
    start_x += central_offset(message, is_rightward);
    let toward = |value: f64| if is_rightward { -value } else { value };
    if message.from == message.to {
        stop_x = start_x;
    } else {
        let is_to_activation = (to_left - to_right).abs() > 2.0;
        if message.is_activating && !is_to_activation {
            stop_x += toward(config.activation_width / 2.0 - 1.0);
        }
        if !message.arrow.keeps_stop_end() {
            stop_x += toward(3.0);
        }
        if message.arrow.pulls_start_end() {
            start_x -= toward(3.0);
        }
    }
    let bounded_width = (start_x - stop_x).abs() + 2.0 * config.wrap_padding;
    let text = match message.is_wrapped && !message.text.is_empty() {
        true => wrap_label(fonts, &message.text, bounded_width.max(config.actor_width)),
        false => message.text.clone(),
    };
    let text_width = match message.is_wrapped {
        true => 0.0,
        false => text_size(fonts, &text).width + 2.0 * config.wrap_padding,
    };
    let edges = [from_left, from_right, to_left, to_right];
    MessageModel {
        start_x,
        stop_x,
        width: text_width.max(bounded_width).max(config.actor_width),
        from_bounds: edges.iter().copied().fold(f64::INFINITY, f64::min),
        to_bounds: edges.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        text,
    }
}

/// buildNoteModel
fn note_model(
    fonts: &FontBook,
    config: &SequenceConfig,
    actors: &[ActorBox],
    note: &SequenceNote,
) -> NoteModel {
    let should_wrap = note.is_wrapped && !note.text.is_empty();
    let measured_width = |wrap_width: f64| {
        let text = wrapped_note_text(fonts, note, should_wrap, wrap_width);
        text_size(fonts, &text).width
    };
    let (start_x, width) = note_span(config, actors, note, &measured_width);
    let wrap_width = width - 2.0 * config.wrap_padding;
    NoteModel {
        start_x,
        width,
        text: wrapped_note_text(fonts, note, should_wrap, wrap_width),
    }
}

/// 노트의 시작 x와 폭. `measured_width`는 주어진 폭에서 감싼 글의 폭이다
fn note_span(
    config: &SequenceConfig,
    actors: &[ActorBox],
    note: &SequenceNote,
    measured_width: &dyn Fn(f64) -> f64,
) -> (f64, f64) {
    let (from, to) = (&actors[note.from], &actors[note.to]);
    let (actor_width, actor_margin) = (config.actor_width, config.actor_margin);
    let should_wrap = note.is_wrapped && !note.text.is_empty();
    let text_width = measured_width(actor_width);
    let padded = text_width + 2.0 * config.note_margin;
    let side_width = match should_wrap {
        true => actor_width.max(padded),
        false => (from.width / 2.0 + to.width / 2.0).max(padded),
    };
    match note.placement {
        NotePlacement::RightOf => {
            let width = match should_wrap {
                true => actor_width.max(text_width),
                false => side_width,
            };
            (from.x + (from.width + actor_margin) / 2.0, width)
        }
        NotePlacement::LeftOf => (
            from.x - side_width + (from.width - actor_margin) / 2.0,
            side_width,
        ),
        NotePlacement::Over if note.from == note.to => {
            let own_width = actor_width.max(from.width);
            let width = match should_wrap {
                true => own_width,
                false => own_width.max(measured_width(own_width) + 2.0 * config.note_margin),
            };
            (from.x + (from.width - width) / 2.0, width)
        }
        NotePlacement::Over => (
            from.center().min(to.center()) - actor_margin / 2.0,
            (from.center() - to.center()).abs() + actor_margin,
        ),
    }
}

/// 감싸는 노트는 노트 폭 안쪽(wrapPadding을 뺀 폭)에서 줄을 바꾼다
fn wrapped_note_text(
    fonts: &FontBook,
    note: &SequenceNote,
    should_wrap: bool,
    wrap_width: f64,
) -> String {
    match should_wrap {
        true => wrap_label(fonts, &note.text, wrap_width),
        false => note.text.clone(),
    }
}

/// 열린 블록의 가로 범위와 제목 감쌀 폭
#[derive(Clone, Copy, Debug)]
struct BlockWidth {
    start_event: usize,
    from: f64,
    to: f64,
    width: f64,
    /// conf.labelBoxWidth
    label_box_width: f64,
}

impl BlockWidth {
    fn include_note(&mut self, note: &NoteModel) {
        self.from = self.from.min(note.start_x);
        self.to = self.to.max(note.start_x + note.width);
        self.width = self.width.max((self.from - self.to).abs()) - self.label_box_width;
    }

    /// 자기 메시지는 참여자 왼쪽 끝(x)을 기준으로 넓힌다(Mermaid 그대로)
    fn include_message(&mut self, model: &MessageModel, from: &ActorBox, to: &ActorBox) {
        if model.start_x == model.stop_x {
            self.from = (from.x - model.width / 2.0)
                .min(from.x - from.width / 2.0)
                .min(self.from);
            self.to = (to.x + model.width / 2.0)
                .max(to.x + from.width / 2.0)
                .max(self.to);
            self.width = self.width.max((self.to - self.from).abs()) - self.label_box_width;
        } else {
            self.from = self.from.min(model.start_x);
            self.to = self.to.max(model.stop_x);
            self.width = self.width.max(model.width) - self.label_box_width;
        }
    }
}

/// calculateLoopBounds를 따라 사건마다 모델을 만들고 블록 제목 폭을 모은다
struct ModelBuilder<'a> {
    config: &'a SequenceConfig,
    models: EventModels,
    open_blocks: Vec<BlockWidth>,
    /// 제목 있는 구역(구역 사건 순번, 블록 시작 사건 순번)
    titled_sections: Vec<(usize, usize)>,
    activations: Bounds,
}

impl ModelBuilder<'_> {
    /// 블록이 끝날 때 그 폭을 시작 사건 순번에 건다
    fn track_block(&mut self, index: usize, event: &SequenceEvent) {
        match event {
            SequenceEvent::BlockStart { kind, .. } if *kind != BlockKind::Rect => {
                self.open_blocks.push(BlockWidth {
                    start_event: index,
                    from: f64::MAX,
                    to: f64::MIN,
                    width: 0.0,
                    label_box_width: self.config.label_box_width,
                });
            }
            SequenceEvent::BlockSection(text) if !text.is_empty() => {
                if let Some(block) = self.open_blocks.last() {
                    self.titled_sections.push((index, block.start_event));
                }
            }
            SequenceEvent::BlockEnd(kind) if *kind != BlockKind::Rect => {
                if let Some(block) = self.open_blocks.pop() {
                    self.models
                        .block_widths
                        .insert(block.start_event, block.width);
                }
            }
            _ => {}
        }
    }

    fn track_activation(&mut self, actors: &[ActorBox], event: &SequenceEvent) {
        match event {
            SequenceEvent::ActivationStart(actor) => {
                let center = actors[*actor].center();
                self.activations.new_activation(*actor, center, 0);
            }
            SequenceEvent::ActivationEnd(actor) => {
                self.activations.end_activation(*actor);
            }
            _ => {}
        }
    }

    fn track_model(
        &mut self,
        fonts: &FontBook,
        actors: &[ActorBox],
        index: usize,
        event: &SequenceEvent,
    ) {
        match event {
            SequenceEvent::Note(note) => {
                let model = note_model(fonts, self.config, actors, note);
                for block in &mut self.open_blocks {
                    block.include_note(&model);
                }
                self.models.notes.insert(index, model);
            }
            SequenceEvent::Message(message) => {
                let activations = &self.activations.activations;
                let model = message_model(fonts, self.config, actors, activations, message);
                if model.start_x != 0.0 && model.stop_x != 0.0 {
                    let (from, to) = (&actors[message.from], &actors[message.to]);
                    for block in &mut self.open_blocks {
                        block.include_message(&model, from, to);
                    }
                }
                self.models.messages.insert(index, model);
            }
            _ => {}
        }
    }
}

/// 구역 제목 폭은 Mermaid에서 같은 블록 객체를 가리키므로, 블록이 끝날 때의 폭을 쓴다
fn settle_section_widths(models: &mut EventModels, sections: &[(usize, usize)]) {
    for (section, start) in sections {
        if let Some(width) = models.block_widths.get(start).copied() {
            models.block_widths.insert(*section, width);
        }
    }
}

pub(super) fn build_event_models(
    document: &SequenceDocument,
    fonts: &FontBook,
    config: &SequenceConfig,
    actors: &[ActorBox],
) -> EventModels {
    let mut builder = ModelBuilder {
        config,
        models: EventModels::default(),
        open_blocks: Vec::new(),
        titled_sections: Vec::new(),
        activations: Bounds::new(config),
    };
    for (index, event) in document.events.iter().enumerate() {
        builder.track_block(index, event);
        builder.track_activation(actors, event);
        builder.track_model(fonts, actors, index, event);
    }
    let mut models = builder.models;
    settle_section_widths(&mut models, &builder.titled_sections);
    models
}
