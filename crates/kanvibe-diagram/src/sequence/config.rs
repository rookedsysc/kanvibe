//! 시퀀스 렌더러가 읽는 `sequence` 설정(앞머리 `config`·`%%{init}%%` 지시문). 비어 있는 값은 Mermaid 기본값이다.
//!
//! 글꼴 크기 설정(`actorFontSize`·`messageFontSize`·`noteFontSize`)은 Mermaid가 최상위 `fontSize`(16)로 덮어쓰므로
//! 읽지 않는다.

use serde_json::{Map, Value};

use crate::directive::read_config;

/// 메시지·노트 글 가로 맞춤(`messageAlign`·`noteAlign`)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

impl TextAlign {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "left" | "start" => Some(Self::Left),
            "center" | "middle" => Some(Self::Center),
            "right" | "end" => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SequenceConfig {
    pub activation_width: f64,
    /// `width`·`height`: 참여자 상자 최소 크기
    pub actor_width: f64,
    pub actor_height: f64,
    pub actor_margin: f64,
    pub box_margin: f64,
    pub box_text_margin: f64,
    pub note_margin: f64,
    pub wrap_padding: f64,
    pub label_box_width: f64,
    pub label_box_height: f64,
    pub diagram_margin_x: f64,
    pub diagram_margin_y: f64,
    pub bottom_margin_adj: f64,
    pub message_align: TextAlign,
    pub note_align: TextAlign,
    /// 아래쪽 거울 참여자를 그린다
    pub should_mirror_actors: bool,
    /// 자기 메시지를 곡선 대신 꺾은선으로 그린다
    pub has_right_angles: bool,
    /// `autonumber` 없이도 번호를 그린다
    pub should_show_numbers: bool,
    /// `wrap:`·`nowrap:` 머리말이 없는 글을 감싼다(SequenceDB.autoWrap)
    pub is_auto_wrapped: bool,
}

impl Default for SequenceConfig {
    fn default() -> Self {
        Self {
            activation_width: 10.0,
            actor_width: 150.0,
            actor_height: 65.0,
            actor_margin: 50.0,
            box_margin: 10.0,
            box_text_margin: 5.0,
            note_margin: 10.0,
            wrap_padding: 10.0,
            label_box_width: 50.0,
            label_box_height: 20.0,
            diagram_margin_x: 50.0,
            diagram_margin_y: 10.0,
            bottom_margin_adj: 1.0,
            message_align: TextAlign::Center,
            note_align: TextAlign::Center,
            should_mirror_actors: true,
            has_right_angles: false,
            should_show_numbers: false,
            is_auto_wrapped: false,
        }
    }
}

impl SequenceConfig {
    /// 정의의 `sequence` 설정 묶음. 최상위 `wrap`이 있으면 `sequence.wrap`보다 앞선다
    pub fn read(definition: &str) -> Self {
        let config = read_config(definition);
        let empty = Map::new();
        let section = config.section("sequence").unwrap_or(&empty);
        let mut settings = Self::default();
        settings.read_numbers(section);
        let flag = |key: &str, fallback: bool| {
            section
                .get(key)
                .and_then(Value::as_bool)
                .unwrap_or(fallback)
        };
        let align = |key: &str| {
            section
                .get(key)
                .and_then(Value::as_str)
                .and_then(TextAlign::from_name)
                .unwrap_or_default()
        };
        settings.message_align = align("messageAlign");
        settings.note_align = align("noteAlign");
        settings.should_mirror_actors = flag("mirrorActors", true);
        settings.has_right_angles = flag("rightAngles", false);
        settings.should_show_numbers = flag("showSequenceNumbers", false);
        let top_level_wrap = config.tree.get("wrap").and_then(Value::as_bool);
        settings.is_auto_wrapped = top_level_wrap.unwrap_or_else(|| flag("wrap", false));
        settings
    }

    fn read_numbers(&mut self, section: &Map<String, Value>) {
        let fields: [(&str, &mut f64); 13] = [
            ("activationWidth", &mut self.activation_width),
            ("width", &mut self.actor_width),
            ("height", &mut self.actor_height),
            ("actorMargin", &mut self.actor_margin),
            ("boxMargin", &mut self.box_margin),
            ("boxTextMargin", &mut self.box_text_margin),
            ("noteMargin", &mut self.note_margin),
            ("wrapPadding", &mut self.wrap_padding),
            ("labelBoxWidth", &mut self.label_box_width),
            ("labelBoxHeight", &mut self.label_box_height),
            ("diagramMarginX", &mut self.diagram_margin_x),
            ("diagramMarginY", &mut self.diagram_margin_y),
            ("bottomMarginAdj", &mut self.bottom_margin_adj),
        ];
        for (key, slot) in fields {
            if let Some(number) = section.get(key).and_then(Value::as_f64) {
                *slot = number;
            }
        }
    }

    /// 감싸는 글이 들어갈 폭(conf.width − 2·wrapPadding)
    pub fn wrapped_text_width(&self) -> f64 {
        self.actor_width - 2.0 * self.wrap_padding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 지시문과_앞머리의_sequence_설정을_읽는다() {
        let directive = SequenceConfig::read(
            "%%{init: {\"sequence\": {\"mirrorActors\": false, \"actorMargin\": 80, \"messageAlign\": \"left\", \"rightAngles\": true}}}%%\nsequenceDiagram",
        );
        assert!(!directive.should_mirror_actors);
        assert!(directive.has_right_angles);
        assert_eq!(directive.actor_margin, 80.0);
        assert_eq!(directive.message_align, TextAlign::Left);
        assert_eq!(directive.note_align, TextAlign::Center);
        let front = SequenceConfig::read(
            "---\nconfig:\n  wrap: false\n  sequence:\n    wrap: true\n    noteMargin: 16\n---\nsequenceDiagram",
        );
        assert_eq!(front.note_margin, 16.0);
        // 최상위 wrap이 sequence.wrap보다 앞선다
        assert!(!front.is_auto_wrapped);
    }
}
