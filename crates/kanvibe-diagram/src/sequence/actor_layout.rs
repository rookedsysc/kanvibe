//! 참여자 자리: 왼쪽부터 늘어놓기(addActorRenderingData), 생성·소멸 메시지에 맞춘 상자 자리
//! (adjustCreatedDestroyedData), 아래 거울 참여자, 참여자 묶음 상자, 팝업 메뉴 크기(drawActorsPopup).

use super::bounds::BOX_MARGIN;
use super::document::SequenceMessage;
use super::layout::{LayoutPass, PlacedGroup};
use super::measure::text_size;
use super::models::MessageModel;
use super::participants::{FIGURE_HALF_WIDTH, drawn_height};
use super::{BOX_TEXT_MARGIN, NOTE_MARGIN, WRAP_PADDING};

/// 묶음 상자는 참여자 영역보다 좌우로 2·boxMargin 넓다
const GROUP_PADDING: f64 = BOX_MARGIN * 2.0;
/// 팝업 메뉴 첫 항목 위치와 항목 간격
const MENU_FIRST_LINK: f64 = 20.0;
const MENU_LINK_GAP: f64 = 30.0;

impl LayoutPass<'_> {
    /// 참여자를 묶음 여백·생성 참여자 반폭까지 넣어 왼쪽부터 늘어놓는다
    pub(super) fn place_actors(&mut self, group_title_height: f64) {
        let participants = &self.document.participants;
        if !self.groups.is_empty() {
            self.bounds.bump(BOX_MARGIN);
            if self.groups.iter().any(|group| group.title.is_some()) {
                self.bounds.bump(group_title_height);
            }
        }
        let (mut previous_width, mut previous_margin) = (0.0, 0.0);
        let mut previous_group: Option<usize> = None;
        for (index, participant) in participants.iter().enumerate() {
            let group = participant.group;
            if let Some(previous) = previous_group.filter(|previous| Some(*previous) != group) {
                self.group_order.push(previous);
                previous_margin += BOX_MARGIN + self.groups[previous].margin;
            }
            if let Some(current) = group.filter(|current| Some(*current) != previous_group) {
                self.groups[current].x = previous_width + previous_margin;
                previous_margin += self.groups[current].margin;
            }
            // createdActors.get(...)이 0이면 거짓으로 보는 것까지 그대로 따른다
            let is_created = self
                .document
                .created
                .get(&index)
                .is_some_and(|event| *event != 0);
            let actor = &mut self.actors[index];
            if is_created {
                previous_margin += actor.width / 2.0;
            }
            actor.x = previous_width + previous_margin;
            actor.start_y = self.bounds.vertical_pos;
            let (x, width, height) = (actor.x, actor.width, actor.height);
            self.bounds.insert(x, 0.0, x + width, height);
            previous_width += width + previous_margin;
            if let Some(current) = group {
                let group_box = &mut self.groups[current];
                group_box.width = previous_width + group_box.margin - group_box.x;
            }
            previous_margin = self.actors[index].margin();
            previous_group = group;
        }
        self.group_order.extend(previous_group);
        let tallest = self
            .actors
            .iter()
            .map(|actor| actor.height)
            .fold(0.0, f64::max);
        self.bounds.bump(tallest);
    }

    /// 만드는 메시지는 새 참여자 상자 옆에서 멈추고 상자가 선 높이로 내려온다. 없애는 메시지는 아래 상자를
    /// 그 자리로 끌어올린다
    pub(super) fn adjust_lifecycle(
        &mut self,
        index: usize,
        message: &SequenceMessage,
        model: &mut MessageModel,
        (start_y, line_y, stop_y): (f64, f64, f64),
    ) {
        let document = self.document;
        let half_width = |pass: &Self, actor: usize, extra: f64| match document.participants[actor]
            .kind
            .is_figure()
        {
            true => FIGURE_HALF_WIDTH + extra,
            false => pass.actors[actor].width / 2.0 + extra,
        };
        let span = (start_y, stop_y);
        let actor = if document.created.get(&message.to) == Some(&index) {
            let adjustment = half_width(self, message.to, 3.0);
            self.shorten_receiver(message, model, adjustment, span);
            self.actors[message.to].start_y = line_y - self.actors[message.to].height / 2.0;
            message.to
        } else if document.destroyed.get(&message.from) == Some(&index) {
            let adjustment = half_width(self, message.from, 0.0);
            self.shorten_sender(message, model, adjustment, span);
            self.actors[message.from].stop_y =
                Some(line_y - self.actors[message.from].height / 2.0);
            message.from
        } else if document.destroyed.get(&message.to) == Some(&index) {
            let adjustment = half_width(self, message.to, 3.0);
            self.shorten_receiver(message, model, adjustment, span);
            self.actors[message.to].stop_y = Some(line_y - self.actors[message.to].height / 2.0);
            message.to
        } else {
            return;
        };
        self.bounds.bump(self.actors[actor].height / 2.0);
    }

    /// receiverAdjustment
    fn shorten_receiver(
        &mut self,
        message: &SequenceMessage,
        model: &mut MessageModel,
        adjustment: f64,
        (start_y, stop_y): (f64, f64),
    ) {
        let receiver = &self.actors[message.to];
        let bottom = stop_y + receiver.height / 2.0 + NOTE_MARGIN;
        if receiver.x < self.actors[message.from].x {
            self.bounds
                .insert(model.stop_x - adjustment, start_y, model.start_x, bottom);
            model.stop_x += adjustment;
        } else {
            self.bounds
                .insert(model.start_x, start_y, model.stop_x + adjustment, bottom);
            model.stop_x -= adjustment;
        }
    }

    /// senderAdjustment
    fn shorten_sender(
        &mut self,
        message: &SequenceMessage,
        model: &mut MessageModel,
        adjustment: f64,
        (start_y, stop_y): (f64, f64),
    ) {
        let sender = &self.actors[message.from];
        let bottom = stop_y + sender.height / 2.0 + NOTE_MARGIN;
        if sender.x < self.actors[message.to].x {
            self.bounds
                .insert(model.start_x - adjustment, start_y, model.stop_x, bottom);
            model.start_x += adjustment;
        } else {
            self.bounds
                .insert(model.stop_x, start_y, model.start_x + adjustment, bottom);
            model.start_x -= adjustment;
        }
    }

    /// 아래쪽 거울 참여자(mirrorActors). 소멸한 참여자는 이미 정한 자리에 남는다
    pub(super) fn place_footer(&mut self) {
        if self.actors.is_empty() {
            return;
        }
        self.bounds.bump(BOX_MARGIN * 2.0);
        let footer_y = self.bounds.vertical_pos;
        let mut tallest: f64 = 0.0;
        for (participant, actor) in self.document.participants.iter().zip(&mut self.actors) {
            actor.stop_y.get_or_insert(footer_y);
            tallest = tallest.max(drawn_height(participant.kind, actor));
        }
        self.bounds.bump(tallest + BOX_MARGIN);
    }

    /// 묶음 상자는 그림 맨 위에서 아래 참여자 아래까지 덮는다
    pub(super) fn place_groups(&mut self) {
        let bottom = self.bounds.vertical_pos;
        for index in self.group_order.clone() {
            let group = &self.groups[index];
            let (x, width) = (group.x, group.width);
            self.bounds.insert(x, 0.0, x + width, bottom);
            let start_x = x - GROUP_PADDING;
            let start_y = -GROUP_PADDING * 0.25;
            let source = &self.document.groups[index];
            self.placed_groups.push(PlacedGroup {
                start_x,
                start_y,
                stop_x: start_x + width + 2.0 * GROUP_PADDING,
                stop_y: start_y + bottom + GROUP_PADDING * 0.75,
                fill: source.fill.clone(),
                title: group.title.clone(),
                title_center_x: x + width / 2.0,
                title_center_y: BOX_TEXT_MARGIN + self.group_title_height() / 2.0,
            });
        }
        if !self.groups.is_empty() {
            self.bounds.bump(BOX_MARGIN);
        }
    }

    fn group_title_height(&self) -> f64 {
        self.groups
            .iter()
            .filter_map(|group| group.title.as_deref())
            .map(|title| text_size(self.fonts, title).height)
            .fold(0.0, f64::max)
    }

    /// 숨은 팝업 메뉴(링크가 있는 참여자, 늘 상자 모양이다)의 오른쪽 끝과 높이
    pub(super) fn popup_menu_extent(&self) -> (f64, f64) {
        let participants = self.document.participants.iter().zip(&self.actors);
        let mut extent: (f64, f64) = (0.0, 0.0);
        for (participant, actor) in participants {
            if participant.link_labels.is_empty() {
                continue;
            }
            let widest_label = participant
                .link_labels
                .iter()
                .map(|label| text_size(self.fonts, label).width + 2.0 * (WRAP_PADDING + BOX_MARGIN))
                .fold(0.0, f64::max);
            let menu_width = actor.width.max(widest_label);
            let link_count = participant.link_labels.len() as f64;
            let menu_height = actor.height + MENU_FIRST_LINK + MENU_LINK_GAP * link_count;
            extent = (
                extent.0.max(menu_width + actor.x),
                extent.1.max(menu_height),
            );
        }
        extent
    }
}
