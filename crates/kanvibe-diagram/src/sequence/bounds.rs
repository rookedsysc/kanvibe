//! sequenceRenderer의 `bounds` 객체. 세로 위치를 쌓아 가며 그림 전체·열린 블록·열린 활성화의 경계를 넓힌다.
//!
//! 무엇을 넣든 열린 블록은 바깥일수록 `boxMargin`을 한 칸씩 더 둘러 넓어지고, 활성화는 위쪽 끝만 당겨진다.

use super::config::SequenceConfig;

fn merge_min(slot: &mut Option<f64>, value: f64) {
    *slot = Some(slot.map_or(value, |current| current.min(value)));
}

fn merge_max(slot: &mut Option<f64>, value: f64) {
    *slot = Some(slot.map_or(value, |current| current.max(value)));
}

/// 경계 상자. 아직 아무것도 넣지 않은 변은 `None`이다
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Extent {
    pub start_x: Option<f64>,
    pub stop_x: Option<f64>,
    pub start_y: Option<f64>,
    pub stop_y: Option<f64>,
}

/// alt·else처럼 블록을 가르는 점선과 그 제목
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BlockSection {
    pub y: f64,
    pub title: String,
}

/// 열린 블록(loop·alt·rect 등, createLoop)
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BlockFrame {
    pub start_x: Option<f64>,
    pub stop_x: Option<f64>,
    pub start_y: f64,
    pub stop_y: Option<f64>,
    pub title: String,
    /// rect 블록의 배경색 글
    pub fill: Option<String>,
    pub sections: Vec<BlockSection>,
}

/// 열린 활성화 막대
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ActivationFrame {
    pub start_x: f64,
    pub stop_x: f64,
    pub start_y: f64,
    pub actor: usize,
    /// 그리는 순서 자리(막대는 시작한 자리에 놓인다)
    pub slot: usize,
}

#[derive(Debug)]
pub(super) struct Bounds {
    pub vertical_pos: f64,
    pub extent: Extent,
    pub blocks: Vec<BlockFrame>,
    pub activations: Vec<ActivationFrame>,
    /// conf.boxMargin·conf.activationWidth
    box_margin: f64,
    activation_width: f64,
}

impl Bounds {
    pub fn new(config: &SequenceConfig) -> Self {
        Self {
            vertical_pos: 0.0,
            extent: Extent::default(),
            blocks: Vec::new(),
            activations: Vec::new(),
            box_margin: config.box_margin,
            activation_width: config.activation_width,
        }
    }

    /// updateBounds: 열린 블록·활성화마다 바깥에서부터 n·boxMargin만큼 둘러 넓힌다
    fn update_open_frames(&mut self, start_x: f64, start_y: f64, stop_x: f64, stop_y: f64) {
        let open_blocks = self.blocks.len() as f64;
        let extent = &mut self.extent;
        let mut counter = 0.0;
        for block in &mut self.blocks {
            counter += 1.0;
            let margin = (open_blocks - counter + 1.0) * self.box_margin;
            block.start_y = block.start_y.min(start_y - margin);
            merge_max(&mut block.stop_y, stop_y + margin);
            merge_min(&mut block.start_x, start_x - margin);
            merge_max(&mut block.stop_x, stop_x + margin);
            merge_min(&mut extent.start_x, start_x - margin);
            merge_max(&mut extent.stop_x, stop_x + margin);
            merge_min(&mut extent.start_y, start_y - margin);
            merge_max(&mut extent.stop_y, stop_y + margin);
        }
        for activation in &mut self.activations {
            counter += 1.0;
            let margin = (open_blocks - counter + 1.0) * self.box_margin;
            activation.start_y = activation.start_y.min(start_y - margin);
            merge_min(&mut extent.start_x, start_x - margin);
            merge_max(&mut extent.stop_x, stop_x + margin);
        }
    }

    /// bounds.insert
    pub fn insert(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        let (start_x, stop_x) = (x1.min(x2), x1.max(x2));
        let (start_y, stop_y) = (y1.min(y2), y1.max(y2));
        merge_min(&mut self.extent.start_x, start_x);
        merge_min(&mut self.extent.start_y, start_y);
        merge_max(&mut self.extent.stop_x, stop_x);
        merge_max(&mut self.extent.stop_y, stop_y);
        self.update_open_frames(start_x, start_y, stop_x, stop_y);
    }

    /// bumpVerticalPos
    pub fn bump(&mut self, amount: f64) {
        self.vertical_pos += amount;
        merge_max(&mut self.extent.stop_y, self.vertical_pos);
    }

    pub fn new_block(&mut self, title: String, fill: Option<String>) {
        self.blocks.push(BlockFrame {
            start_x: None,
            stop_x: None,
            start_y: self.vertical_pos,
            stop_y: None,
            title,
            fill,
            sections: Vec::new(),
        });
    }

    pub fn add_section(&mut self, title: String) {
        let y = self.vertical_pos;
        if let Some(block) = self.blocks.last_mut() {
            block.sections.push(BlockSection { y, title });
        }
    }

    /// 참여자에게 열린 활성화 수
    pub fn activation_count(&self, actor: usize) -> usize {
        self.activations
            .iter()
            .filter(|activation| activation.actor == actor)
            .count()
    }

    /// 겹친 활성화는 반 폭씩 오른쪽으로 비켜 쌓인다
    pub fn new_activation(&mut self, actor: usize, actor_center: f64, slot: usize) {
        let stacked = self.activation_count(actor) as f64;
        let start_x = actor_center + (stacked - 1.0) * self.activation_width / 2.0;
        self.activations.push(ActivationFrame {
            start_x,
            stop_x: start_x + self.activation_width,
            start_y: self.vertical_pos + 2.0,
            actor,
            slot,
        });
    }

    /// 그 참여자의 마지막 활성화를 닫는다
    pub fn end_activation(&mut self, actor: usize) -> Option<ActivationFrame> {
        let index = self
            .activations
            .iter()
            .rposition(|activation| activation.actor == actor)?;
        Some(self.activations.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 바깥_블록일수록_여백이_크다() {
        let mut bounds = Bounds::new(&SequenceConfig::default());
        bounds.new_block("outer".to_string(), None);
        bounds.new_block("inner".to_string(), None);
        bounds.insert(100.0, 50.0, 200.0, 60.0);
        assert_eq!(bounds.blocks[0].start_x, Some(80.0));
        assert_eq!(bounds.blocks[1].start_x, Some(90.0));
        assert_eq!(bounds.blocks[1].stop_y, Some(70.0));
        assert_eq!(bounds.extent.start_x, Some(80.0));
    }
}
