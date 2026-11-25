//! sequenceDiagram 문법(sequenceDiagram.jison)과 SequenceDB에서 쓰는 부분을 줄 단위로 옮겼다.
//!
//! 문장은 줄바꿈과 `;`로 나뉜다. 메시지·노트에 처음 나온 참여자는 그 자리에서 자동으로 추가되고, 활성화
//! 단축형(`->>+`, `-->>-`)과 중앙 연결(`()`)은 메시지 뒤에 활성화 사건을 덧붙인다. 참여자 선언·묶음·생성/소멸·
//! 링크 문장은 [`super::participant_parser`]가 읽는다.

use std::collections::HashMap;

use super::document::{
    Arrow, ArrowHead, BlockKind, CentralConnection, NotePlacement, ParticipantKind,
    SequenceDocument, SequenceEvent, SequenceMessage, SequenceNote,
};
use crate::DiagramError;

/// 렉서 규칙의 화살표 표기. 같은 자리에서는 긴 표기를 먼저 맞춘다
const ARROW_TOKENS: [(&str, ArrowHead, bool); 26] = [
    ("<<-->>", ArrowHead::Bidirectional, true),
    ("<<->>", ArrowHead::Bidirectional, false),
    ("-->>", ArrowHead::Filled, true),
    ("--|\\", ArrowHead::SolidTop, true),
    ("--|/", ArrowHead::SolidBottom, true),
    ("--\\\\", ArrowHead::StickTop, true),
    ("--//", ArrowHead::StickBottom, true),
    ("/|--", ArrowHead::ReverseSolidTop, true),
    ("\\|--", ArrowHead::ReverseSolidBottom, true),
    ("//--", ArrowHead::ReverseStickTop, true),
    ("\\\\--", ArrowHead::ReverseStickBottom, true),
    ("->>", ArrowHead::Filled, false),
    ("-->", ArrowHead::Open, true),
    ("--x", ArrowHead::Cross, true),
    ("--)", ArrowHead::Async, true),
    ("-|\\", ArrowHead::SolidTop, false),
    ("-|/", ArrowHead::SolidBottom, false),
    ("-\\\\", ArrowHead::StickTop, false),
    ("-//", ArrowHead::StickBottom, false),
    ("/|-", ArrowHead::ReverseSolidTop, false),
    ("\\|-", ArrowHead::ReverseSolidBottom, false),
    ("//-", ArrowHead::ReverseStickTop, false),
    ("\\\\-", ArrowHead::ReverseStickBottom, false),
    ("->", ArrowHead::Open, false),
    ("-x", ArrowHead::Cross, false),
    ("-)", ArrowHead::Async, false),
];

/// 중앙 연결 표기
const CENTRAL_MARK: &str = "()";

#[derive(Default)]
pub(super) struct SequenceParser {
    pub document: SequenceDocument,
    pub participant_ids: HashMap<String, usize>,
    pub previous_participant: Option<usize>,
    open_blocks: Vec<BlockKind>,
    /// 지금 열린 참여자 묶음
    pub open_group: Option<usize>,
    /// 만든 뒤 아직 만드는 메시지가 오지 않은 참여자
    pub last_created: Option<usize>,
    /// 없앤다고 한 뒤 아직 없애는 메시지가 오지 않은 참여자 이름
    pub last_destroyed: Option<String>,
    /// `destroy`한 이름 → 그때의 사건 순번(이름은 메시지에서 처음 나올 수도 있어 끝에 순번으로 바꾼다)
    pub destroyed_names: Vec<(String, usize)>,
    /// 여러 줄 `accDescr { ... }` 안
    is_in_description: bool,
}

pub(super) fn parse(definition: &str) -> Result<SequenceDocument, DiagramError> {
    let mut parser = SequenceParser::default();
    let mut has_header = false;
    for raw_line in definition.lines() {
        let line = strip_comment(raw_line);
        for statement in line.split(';').map(str::trim) {
            if statement.is_empty() || parser.skip_description(statement) {
                continue;
            }
            if !has_header {
                has_header = statement.starts_with("sequenceDiagram");
                if !has_header {
                    return Err(syntax("sequenceDiagram 머리줄이 없습니다"));
                }
                continue;
            }
            parser.parse_statement(statement)?;
        }
    }
    parser.finish()
}

pub(super) fn syntax(message: &str) -> DiagramError {
    DiagramError::Syntax(message.to_string())
}

/// `%%` 뒤와 줄 머리 `#` 주석을 지운다
fn strip_comment(line: &str) -> &str {
    let line = line.trim_start();
    if line.starts_with('#') {
        return "";
    }
    match line.find("%%") {
        Some(index) => &line[..index],
        None => line,
    }
}

/// parseMessage: 앞뒤 공백과 `wrap:`·`nowrap:` 머리말을 지우고, `wrap:`이었는지 함께 돌려준다
pub(super) fn clean_text(text: &str) -> (String, bool) {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let lower = lower.strip_prefix(':').unwrap_or(&lower);
    let colon = usize::from(text.starts_with(':'));
    for (prefix, is_wrapped) in [("nowrap:", false), ("wrap:", true)] {
        if lower.starts_with(prefix) {
            let rest = &text[colon + prefix.len()..];
            return (rest.trim().to_string(), is_wrapped);
        }
    }
    (text.to_string(), false)
}

/// 문장 맨 앞 낱말(소문자)과 나머지
pub(super) fn split_keyword(statement: &str) -> (String, &str) {
    let end = statement
        .find(char::is_whitespace)
        .unwrap_or(statement.len());
    (statement[..end].to_ascii_lowercase(), &statement[end..])
}

impl SequenceParser {
    fn parse_statement(&mut self, statement: &str) -> Result<(), DiagramError> {
        let (keyword, rest) = split_keyword(statement);
        if self.open_group.is_some() {
            return self.parse_group_statement(&keyword, rest);
        }
        match keyword.as_str() {
            "participant" => self.parse_participant(rest, ParticipantKind::Participant),
            "actor" => self.parse_participant(rest, ParticipantKind::Actor),
            "box" => self.open_participant_group(rest),
            "create" => self.parse_create(rest),
            "destroy" => self.parse_destroy(rest),
            "links" | "link" | "properties" | "details" => self.parse_menu(&keyword, rest),
            "title" | "title:" => self.parse_title(rest),
            "note" => self.parse_note(rest),
            "activate" => self.parse_activation(rest, true),
            "deactivate" => self.parse_activation(rest, false),
            "autonumber" => self.parse_autonumber(rest),
            "end" => self.close_block(),
            "else" | "and" | "option" => self.add_section(&keyword, rest),
            _ => self.parse_other(&keyword, statement, rest),
        }
    }

    fn parse_other(
        &mut self,
        keyword: &str,
        statement: &str,
        rest: &str,
    ) -> Result<(), DiagramError> {
        let block = match keyword {
            "loop" => Some(BlockKind::Loop),
            "alt" => Some(BlockKind::Alt),
            "opt" => Some(BlockKind::Opt),
            "par" | "par_over" => Some(BlockKind::Par),
            "critical" => Some(BlockKind::Critical),
            "break" => Some(BlockKind::Break),
            "rect" => Some(BlockKind::Rect),
            _ => None,
        };
        if let Some(kind) = block {
            return self.open_block(kind, rest);
        }
        if keyword.starts_with("acctitle") || keyword.starts_with("accdescr") {
            self.is_in_description = statement.ends_with('{');
            return Ok(());
        }
        self.parse_message(statement)
    }

    /// 여러 줄 접근성 설명은 `}`까지 건너뛴다
    fn skip_description(&mut self, statement: &str) -> bool {
        if !self.is_in_description {
            return false;
        }
        self.is_in_description = !statement.ends_with('}');
        true
    }

    fn parse_title(&mut self, rest: &str) -> Result<(), DiagramError> {
        let title = rest.trim();
        self.document.title = (!title.is_empty()).then(|| title.to_string());
        Ok(())
    }

    /// 메시지·노트에서 이름을 쓰면 참여자로 더한다
    pub(super) fn reference_participant(&mut self, id: &str) -> Result<usize, DiagramError> {
        if id.is_empty() {
            return Err(syntax("참여자 이름이 비었습니다"));
        }
        self.add_participant(id, None, ParticipantKind::Participant)?;
        Ok(self.participant_ids[id])
    }

    fn parse_note(&mut self, rest: &str) -> Result<(), DiagramError> {
        let rest = rest.trim();
        let lower = rest.to_ascii_lowercase();
        let (placement, prefix_length) = [
            ("left of", NotePlacement::LeftOf),
            ("right of", NotePlacement::RightOf),
            ("over", NotePlacement::Over),
        ]
        .into_iter()
        .find(|(prefix, _)| lower.starts_with(prefix))
        .map(|(prefix, placement)| (placement, prefix.len()))
        .ok_or_else(|| syntax("노트 위치는 left of·right of·over 중 하나여야 합니다"))?;
        let (targets, text) = rest[prefix_length..]
            .split_once(':')
            .ok_or_else(|| syntax("노트에 `:` 글자가 없습니다"))?;
        let mut names = targets.split(',').map(str::trim);
        let from = self.reference_participant(names.next().unwrap_or(""))?;
        let to = match names.next() {
            Some(second) if placement == NotePlacement::Over => {
                self.reference_participant(second)?
            }
            Some(_) => return Err(syntax("두 참여자 노트는 over만 쓸 수 있습니다")),
            None => from,
        };
        let (text, is_wrapped) = clean_text(text.split('#').next().unwrap_or(""));
        self.document.events.push(SequenceEvent::Note(SequenceNote {
            from,
            to,
            placement,
            text,
            is_wrapped,
        }));
        Ok(())
    }

    fn parse_activation(&mut self, rest: &str, is_start: bool) -> Result<(), DiagramError> {
        let id = rest.trim();
        let actor = *self
            .participant_ids
            .get(id)
            .ok_or_else(|| syntax(&format!("없는 참여자를 활성화합니다: {id}")))?;
        self.push_activation(actor, is_start)
    }

    /// 활성화 끝은 열린 활성화가 있어야 한다(addSignal의 검사)
    fn push_activation(&mut self, actor: usize, is_start: bool) -> Result<(), DiagramError> {
        if is_start {
            self.document
                .events
                .push(SequenceEvent::ActivationStart(actor));
            return Ok(());
        }
        let open_count = self
            .document
            .events
            .iter()
            .fold(0_i64, |count, event| match event {
                SequenceEvent::ActivationStart(index) if *index == actor => count + 1,
                SequenceEvent::ActivationEnd(index) if *index == actor => count - 1,
                _ => count,
            });
        if open_count < 1 {
            return Err(syntax("활성화되지 않은 참여자를 비활성화합니다"));
        }
        self.document
            .events
            .push(SequenceEvent::ActivationEnd(actor));
        Ok(())
    }

    fn parse_autonumber(&mut self, rest: &str) -> Result<(), DiagramError> {
        let words: Vec<&str> = rest.split_whitespace().collect();
        if words.len() == 1 && words[0].eq_ignore_ascii_case("off") {
            self.document.events.push(SequenceEvent::Autonumber {
                start: None,
                step: None,
                is_visible: false,
            });
            return Ok(());
        }
        let numbers: Vec<f64> = words
            .iter()
            .map(|word| word.parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| syntax("autonumber 뒤에는 숫자만 올 수 있습니다"))?;
        if numbers.len() > 2 {
            return Err(syntax("autonumber 숫자는 두 개까지입니다"));
        }
        self.document.events.push(SequenceEvent::Autonumber {
            start: numbers.first().copied(),
            step: numbers.get(1).copied(),
            is_visible: true,
        });
        Ok(())
    }

    fn open_block(&mut self, kind: BlockKind, rest: &str) -> Result<(), DiagramError> {
        self.open_blocks.push(kind);
        self.document.events.push(SequenceEvent::BlockStart {
            kind,
            text: clean_text(rest).0,
        });
        Ok(())
    }

    fn add_section(&mut self, keyword: &str, rest: &str) -> Result<(), DiagramError> {
        let parent = self.open_blocks.last().copied();
        if parent.and_then(BlockKind::section_keyword) != Some(keyword) {
            return Err(syntax(&format!("{keyword}가 맞는 블록 안에 있지 않습니다")));
        }
        self.document
            .events
            .push(SequenceEvent::BlockSection(clean_text(rest).0));
        Ok(())
    }

    fn close_block(&mut self) -> Result<(), DiagramError> {
        let kind = self
            .open_blocks
            .pop()
            .ok_or_else(|| syntax("짝이 없는 end"))?;
        self.document.events.push(SequenceEvent::BlockEnd(kind));
        Ok(())
    }

    /// `보내는이[()] 화살표 [+|-|()] 받는이 : 글`
    fn parse_message(&mut self, statement: &str) -> Result<(), DiagramError> {
        let unknown = || syntax(&format!("알 수 없는 문장: {statement}"));
        let (head, text) = statement.split_once(':').ok_or_else(unknown)?;
        let (arrow_start, token_length, arrow) = find_arrow(head).ok_or_else(unknown)?;
        let source = head[..arrow_start].trim();
        let after_arrow = head[arrow_start + token_length..].trim_start();
        let (source, has_source_mark) = match source.strip_suffix(CENTRAL_MARK) {
            Some(name) => (name.trim(), true),
            None => (source, false),
        };
        let (target, has_target_mark) = match after_arrow.strip_prefix(CENTRAL_MARK) {
            Some(name) => (name, true),
            None => (after_arrow, false),
        };
        let (activation, target) = match target.chars().next() {
            Some(sign @ ('+' | '-')) if !has_source_mark && !has_target_mark => {
                (Some(sign == '+'), &target[1..])
            }
            _ => (None, target),
        };
        let central = match (has_source_mark, has_target_mark) {
            (true, true) => Some(CentralConnection::Both),
            (true, false) => Some(CentralConnection::Source),
            (false, true) => Some(CentralConnection::Target),
            (false, false) => None,
        };
        let from = self.reference_participant(source)?;
        let to = self.reference_participant(target.trim())?;
        self.check_lifecycle_message(from, to)?;
        let (text, is_wrapped) = clean_text(text.split('#').next().unwrap_or(""));
        let message = SequenceMessage {
            from,
            to,
            text,
            arrow,
            is_activating: activation == Some(true) || has_target_mark,
            is_wrapped,
            central,
        };
        self.push_message(message, activation)
    }

    fn push_message(
        &mut self,
        message: SequenceMessage,
        activation: Option<bool>,
    ) -> Result<(), DiagramError> {
        let (from, to, central) = (message.from, message.to, message.central);
        self.document.events.push(SequenceEvent::Message(message));
        let events = &mut self.document.events;
        if matches!(
            central,
            Some(CentralConnection::Target | CentralConnection::Both)
        ) {
            events.push(SequenceEvent::CentralActivation(to));
        }
        if matches!(
            central,
            Some(CentralConnection::Source | CentralConnection::Both)
        ) {
            events.push(SequenceEvent::CentralActivation(from));
        }
        match activation {
            Some(true) => self.push_activation(to, true),
            Some(false) => self.push_activation(from, false),
            None => Ok(()),
        }
    }

    /// 묶음·블록이 닫혔는지 보고 소멸 이름을 순번으로 바꾼다
    fn finish(mut self) -> Result<SequenceDocument, DiagramError> {
        if self.open_group.is_some() {
            return Err(syntax("box가 end로 닫히지 않았습니다"));
        }
        if let Some(kind) = self.open_blocks.last() {
            return Err(syntax(&format!(
                "{} 블록이 end로 닫히지 않았습니다",
                kind.keyword()
            )));
        }
        for (name, event_index) in std::mem::take(&mut self.destroyed_names) {
            if let Some(index) = self.participant_ids.get(&name) {
                self.document.destroyed.insert(*index, event_index);
            }
        }
        Ok(self.document)
    }
}

/// 가장 앞에서 시작하는 화살표 표기(시작 위치, 표기 길이, 화살표)
fn find_arrow(head: &str) -> Option<(usize, usize, Arrow)> {
    let bytes = head.as_bytes();
    (0..bytes.len()).find_map(|start| {
        ARROW_TOKENS
            .iter()
            .find_map(|(token, arrow_head, is_dotted)| {
                let candidate = bytes.get(start..start + token.len())?;
                let arrow = Arrow {
                    head: *arrow_head,
                    is_dotted: *is_dotted,
                };
                candidate.eq_ignore_ascii_case(token.as_bytes()).then_some((
                    start,
                    token.len(),
                    arrow,
                ))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrows(document: &SequenceDocument) -> Vec<Arrow> {
        document
            .events
            .iter()
            .filter_map(|event| match event {
                SequenceEvent::Message(message) => Some(message.arrow),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn 별칭과_자동_참여자를_순서대로_모은다() {
        let document = parse(
            "sequenceDiagram\n participant M as Mobile\n actor U as User\n U->>M: Open\n M->>X: hi",
        )
        .unwrap();
        let names: Vec<&str> = document
            .participants
            .iter()
            .map(|participant| participant.description.as_str())
            .collect();
        assert_eq!(names, ["Mobile", "User", "X"]);
        assert_eq!(document.participants[1].kind, ParticipantKind::Actor);
        assert_eq!(document.participants[0].next, Some(1));
        assert_eq!(document.participants[2].previous, Some(1));
    }

    #[test]
    fn 화살표_종류와_활성화_단축형을_읽는다() {
        let document =
            parse("sequenceDiagram\nA--xB: a\nA<<-->>B: b\nA->>+B: c\nB-->>-A: d\nA--)B: e")
                .unwrap();
        let heads: Vec<(ArrowHead, bool)> = arrows(&document)
            .iter()
            .map(|arrow| (arrow.head, arrow.is_dotted))
            .collect();
        assert_eq!(
            heads,
            [
                (ArrowHead::Cross, true),
                (ArrowHead::Bidirectional, true),
                (ArrowHead::Filled, false),
                (ArrowHead::Filled, true),
                (ArrowHead::Async, true)
            ]
        );
        assert_eq!(document.events[3], SequenceEvent::ActivationStart(1));
        assert_eq!(document.events[5], SequenceEvent::ActivationEnd(1));
    }

    #[test]
    fn 반쪽_화살표와_중앙_연결을_읽는다() {
        let document = parse(
            "sequenceDiagram\nA-|\\B: a\nA--//B: b\nA/|-B: c\nA\\\\--B: d\nA->>()B: e\nA()-)()B: f",
        )
        .unwrap();
        let heads: Vec<(ArrowHead, bool)> = arrows(&document)
            .iter()
            .map(|arrow| (arrow.head, arrow.is_dotted))
            .collect();
        assert_eq!(
            heads[..4],
            [
                (ArrowHead::SolidTop, false),
                (ArrowHead::StickBottom, true),
                (ArrowHead::ReverseSolidTop, false),
                (ArrowHead::ReverseStickBottom, true)
            ]
        );
        assert_eq!(document.events[5], SequenceEvent::CentralActivation(1));
        assert!(matches!(
            &document.events[6],
            SequenceEvent::Message(message)
                if message.central == Some(CentralConnection::Both) && message.is_activating
        ));
        assert_eq!(document.participants.len(), 2);
    }

    #[test]
    fn 블록과_노트를_읽는다() {
        let document = parse(
            "sequenceDiagram\nalt bad\nA->>B: x\nelse ok\nNote over A,B: wrap: n\nend\nrect rgb(1, 2, 3)\nA->>A: s\nend",
        )
        .unwrap();
        assert!(matches!(
            &document.events[3],
            SequenceEvent::Note(note) if note.from == 0 && note.to == 1
                && note.placement == NotePlacement::Over && note.text == "n" && note.is_wrapped
        ));
        assert_eq!(
            document.events[5],
            SequenceEvent::BlockStart {
                kind: BlockKind::Rect,
                text: "rgb(1, 2, 3)".to_string()
            }
        );
    }

    #[test]
    fn 잘못된_문장은_오류다() {
        assert!(parse("sequenceDiagram\nloop x\nA->>B: y").is_err());
        assert!(parse("sequenceDiagram\nA->>B: y\ndeactivate B").is_err());
        assert!(parse("sequenceDiagram\nhello world").is_err());
        assert!(parse("sequenceDiagram\nopt x\nelse y\nend").is_err());
    }
}
