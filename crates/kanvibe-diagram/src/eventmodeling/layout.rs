//! eventmodeling db.ts의 getState: 틀을 차례로 놓으며 수영 레인(swimlane)과 상자, 관계선을 정한다.
//!
//! 레인 번호는 UI·처리기 0번대, 명령·읽기 모델 100번대, 이벤트 200번대다. 이름공간이 있는 틀은 원본이 같은
//! 이름공간 레인을 찾지 못해(레인에 이름공간을 적지 않는다) 늘 그 번대의 다음 빈 번호로 새 레인을 연다.
//! 상자는 같은 레인이 이어지면 그 레인 오른쪽 끝 + 10, 레인이 바뀌면 앞 상자 오른쪽에서 80 겹쳐 놓는다.

use super::parser::{EntityType, Frame};

const SWIMLANE_MIN_HEIGHT: f64 = 70.0;
pub const SWIMLANE_PADDING: f64 = 15.0;
const SWIMLANE_GAP: f64 = 10.0;
const BOX_PADDING: f64 = 10.0;
const BOX_OVERLAP: f64 = 90.0;
const BOX_MIN_WIDTH: f64 = 80.0;
const BOX_MAX_WIDTH: f64 = 450.0;
const BOX_MIN_HEIGHT: f64 = 80.0;
const BOX_MAX_HEIGHT: f64 = 750.0;
const CONTENT_START_X: f64 = 250.0;
const BOX_TEXT_PADDING: f64 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Swimlane {
    pub index: u32,
    pub label: String,
    /// 마지막 상자의 오른쪽 끝(여백 제외)
    pub right: f64,
    pub y: f64,
    pub height: f64,
    max_height: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameBox {
    pub x: f64,
    pub width: f64,
    pub height: f64,
    /// 오른쪽 끝 + 여백 10
    right: f64,
    /// `swimlanes`에서의 자리
    pub swimlane: usize,
    pub entity_type: EntityType,
    frame_name: String,
}

/// 관계선: 출발 상자와 도착 상자의 자리
pub type Relation = (usize, usize);

#[derive(Debug, Default)]
pub struct ModelLayout {
    /// 번호 순으로 정렬한 레인
    pub swimlanes: Vec<Swimlane>,
    pub boxes: Vec<FrameBox>,
    pub relations: Vec<Relation>,
    /// 가장 오른쪽 상자의 오른쪽 끝(여백 포함)
    pub max_right: f64,
    previous_swimlane: Option<u32>,
}

impl ModelLayout {
    /// 틀마다 글자 크기(폭, 높이)를 받아 놓는다
    pub fn new(frames: &[Frame], text_sizes: &[(f64, f64)]) -> Self {
        let mut layout = Self::default();
        for (index, (frame, text_size)) in frames.iter().zip(text_sizes).enumerate() {
            layout.place_frame(frame, *text_size);
            layout.connect_frame(frames, index);
        }
        layout
    }

    fn place_frame(&mut self, frame: &Frame, text_size: (f64, f64)) {
        let lane = self.lane_for(frame);
        let width = (text_size.0 + 2.0 * BOX_TEXT_PADDING).clamp(BOX_MIN_WIDTH, BOX_MAX_WIDTH)
            + 2.0 * BOX_PADDING;
        let height = (text_size.1 + 2.0 * BOX_TEXT_PADDING).clamp(BOX_MIN_HEIGHT, BOX_MAX_HEIGHT)
            + 2.0 * BOX_PADDING;
        let x = self.next_box_x(lane);
        let right = x + width + BOX_PADDING;
        self.max_right = self
            .swimlanes
            .iter()
            .map(|lane| lane.right)
            .fold(right, f64::max);
        let swimlane = &mut self.swimlanes[lane];
        swimlane.right = x + width;
        swimlane.max_height = swimlane.max_height.max(height);
        swimlane.height = SWIMLANE_MIN_HEIGHT.max(swimlane.max_height) + 2.0 * SWIMLANE_PADDING;
        let lane_index = swimlane.index;
        self.boxes.push(FrameBox {
            x,
            width,
            height,
            right,
            swimlane: lane,
            entity_type: frame.entity_type,
            frame_name: frame.name.clone(),
        });
        self.previous_swimlane = Some(lane_index);
        self.stack_swimlanes();
    }

    /// 레인 번호를 정하고, 없으면 만든다. 돌려주는 값은 `swimlanes` 안의 자리다
    fn lane_for(&mut self, frame: &Frame) -> usize {
        let (base, default_label, prefix) = match frame.entity_type {
            EntityType::Ui | EntityType::Processor => (0, "UI/Automation", "UI/A: "),
            EntityType::ReadModel | EntityType::Command => (100, "Command/Read Model", "C/RM: "),
            EntityType::Event => (200, "Events", "Stream: "),
        };
        let (index, label) = match namespace(&frame.identifier) {
            Some(namespace) => (self.next_free_index(base), format!("{prefix}{namespace}")),
            None => (base, default_label.to_string()),
        };
        if let Some(position) = self.swimlanes.iter().position(|lane| lane.index == index) {
            return position;
        }
        let position = self.swimlanes.partition_point(|lane| lane.index < index);
        self.swimlanes.insert(
            position,
            Swimlane {
                index,
                label,
                right: 0.0,
                y: 0.0,
                height: SWIMLANE_MIN_HEIGHT,
                max_height: SWIMLANE_MIN_HEIGHT,
            },
        );
        self.boxes
            .iter_mut()
            .filter(|frame_box| frame_box.swimlane >= position)
            .for_each(|frame_box| frame_box.swimlane += 1);
        position
    }

    /// findNextAvailableIndex: 번대 안(경계 제외)에서 가장 큰 번호 + 1
    fn next_free_index(&self, base: u32) -> u32 {
        self.swimlanes
            .iter()
            .map(|lane| lane.index)
            .filter(|index| *index > base && *index < base + 100)
            .fold(base, u32::max)
            + 1
    }

    /// calculateX
    fn next_box_x(&self, lane: usize) -> f64 {
        let Some(previous) = self.previous_swimlane else {
            return CONTENT_START_X;
        };
        let swimlane = &self.swimlanes[lane];
        if previous == swimlane.index && swimlane.right != 0.0 {
            return swimlane.right + BOX_PADDING;
        }
        match self.boxes.last() {
            Some(last) => last.right - BOX_OVERLAP + BOX_PADDING,
            None => CONTENT_START_X,
        }
    }

    /// 레인을 번호 순으로 위에서부터 간격 10을 두고 쌓는다
    fn stack_swimlanes(&mut self) {
        let mut y = 0.0;
        for lane in &mut self.swimlanes {
            lane.y = y;
            y += lane.height + SWIMLANE_GAP;
        }
    }

    /// 관계선: 원천 틀이 있으면 그 틀들에서, 없으면 앞쪽에서 다른 레인에 놓인 가장 가까운 상자에서 잇는다.
    /// 초기화 틀과 원천 없는 첫 틀은 잇지 않는다
    fn connect_frame(&mut self, frames: &[Frame], index: usize) {
        let frame = &frames[index];
        if frame.is_reset || (index == 0 && frame.source_frames.is_empty()) {
            return;
        }
        let target = index;
        if frame.source_frames.is_empty() {
            let lane = self.boxes[target].swimlane;
            let source = (0..index)
                .rev()
                .find(|candidate| self.boxes[*candidate].swimlane != lane);
            self.relations.extend(source.map(|source| (source, target)));
            return;
        }
        let sources = frames
            .iter()
            .filter(|other| frame.source_frames.contains(&other.name));
        for other in sources {
            let source = self
                .boxes
                .iter()
                .position(|frame_box| frame_box.frame_name == other.name);
            self.relations.extend(source.map(|source| (source, target)));
        }
    }

    /// 레인 아래 끝(마지막 레인의 y + 높이)
    pub fn bottom(&self) -> f64 {
        self.swimlanes
            .last()
            .map_or(0.0, |lane| lane.y + lane.height)
    }
}

fn namespace(identifier: &str) -> Option<&str> {
    let parts: Vec<&str> = identifier.split('.').collect();
    (parts.len() == 2).then_some(parts[0])
}

/// extractName: `이름공간.이름`이면 이름, 아니면 식별자 전체
pub fn entity_name(identifier: &str) -> &str {
    let parts: Vec<&str> = identifier.split('.').collect();
    match parts.as_slice() {
        [_, name] => name,
        _ => identifier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eventmodeling::parser::parse;

    #[test]
    fn 이름공간마다_다음_번호로_새_레인을_연다() {
        let frames = parse(
            "eventmodeling\ntf 01 ui Shop.Catalog\ntf 02 cmd Orders.PlaceOrder\ntf 03 pcr Shipping.Fulfil\ntf 04 rmo Orders.Status",
        )
        .unwrap()
        .frames;
        let sizes = vec![(125.0, 22.0); frames.len()];
        let layout = ModelLayout::new(&frames, &sizes);
        let indexes: Vec<u32> = layout.swimlanes.iter().map(|lane| lane.index).collect();
        assert_eq!(indexes, vec![1, 2, 101, 102]);
        assert_eq!(layout.swimlanes[3].label, "C/RM: Orders");
        // 레인 높이 130, 간격 10
        assert_eq!(layout.swimlanes[2].y, 280.0);
    }

    #[test]
    fn 레인이_바뀌면_앞_상자에_80_겹쳐_놓는다() {
        let frames = parse("eventmodeling\ntf 01 ui A\ntf 02 cmd B\ntf 03 cmd C")
            .unwrap()
            .frames;
        // eventmodeling-basic 기준 SVG: 상자 폭 227 → 다음 상자 x 407
        let sizes = vec![(187.0, 22.0), (133.0, 22.0), (133.0, 22.0)];
        let layout = ModelLayout::new(&frames, &sizes);
        assert_eq!(layout.boxes[0].width, 227.0);
        assert_eq!(layout.boxes[1].x, 407.0);
        assert_eq!(layout.boxes[2].x, 407.0 + 173.0 + 10.0);
        assert_eq!(layout.relations, vec![(0, 1), (0, 2)]);
    }
}
