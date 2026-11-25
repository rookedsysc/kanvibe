//! eventModeling.langium 문법 중 시간 틀(`tf`·`timeframe`), 초기화 틀(`rf`·`resetframe`), `data` 묶음을 읽는다.
//!
//! `tf 01 evt Orders.OrderPlaced ->> 02 [[참조]] {인라인}` 꼴이다. 틀 끝에는 `data` 묶음 참조(`[[이름]]`)와
//! 인라인 데이터(`{…}`·`"…"`·`'…'`, 앞에 `` `json` `` 같은 형식 표시가 올 수 있다)가 붙는다. `data 이름 { … }`
//! 묶음은 줄바꿈·들여쓰기를 그대로 담는다. `entity`·`note`·`gwt` 문장은 그림에 나오지 않아 건너뛴다.

use crate::DiagramError;

/// 틀이 가리키는 모델 요소 종류
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityType {
    Ui,
    Processor,
    ReadModel,
    Command,
    Event,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub name: String,
    pub entity_type: EntityType,
    /// `이름` 또는 `이름공간.이름`
    pub identifier: String,
    pub source_frames: Vec<String>,
    pub is_reset: bool,
    /// `[[이름]]`으로 가리킨 `data` 묶음 이름
    pub data_reference: Option<String>,
    /// 인라인 데이터 원문(`{…}` 등 감싼 기호 포함)
    pub inline_data: Option<String>,
}

/// `data 이름 { … }` 묶음. `block`은 `{`부터 `}`까지의 원문이다
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataBlock {
    pub name: String,
    pub block: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventModel {
    pub frames: Vec<Frame>,
    pub data_blocks: Vec<DataBlock>,
}

pub fn parse(definition: &str) -> Result<EventModel, DiagramError> {
    let mut lines = definition.lines().filter(|line| !is_skipped(line));
    if !lines
        .next()
        .is_some_and(|line| line.trim_start().starts_with("eventmodeling"))
    {
        return Err(DiagramError::Syntax("eventmodeling 머리줄".to_string()));
    }
    let mut model = EventModel::default();
    while let Some(line) = lines.next() {
        let trimmed = strip_trailing_comment(line).trim();
        let (keyword, rest) = trimmed
            .split_once(char::is_whitespace)
            .unwrap_or((trimmed, ""));
        match keyword {
            "tf" | "timeframe" => model.frames.push(parse_frame(rest, false)?),
            "rf" | "resetframe" => model.frames.push(parse_frame(rest, true)?),
            "data" if trimmed.ends_with('{') => {
                let name = rest.trim_end_matches('{').trim().to_string();
                let block = read_block(&mut lines);
                model.data_blocks.push(DataBlock { name, block });
            }
            _ if trimmed.ends_with('{') => {
                read_block(&mut lines);
            }
            _ => {}
        }
    }
    Ok(model)
}

/// 빈 줄과 줄 머리 주석(`%%`·`//`)
fn is_skipped(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.is_empty() || trimmed.starts_with("%%") || trimmed.starts_with("//")
}

fn strip_trailing_comment(line: &str) -> &str {
    line.find("%%").map_or(line, |end| &line[..end])
}

/// `{` 다음 줄부터 `}` 줄까지를 `{\n…\n}` 원문으로 모은다
fn read_block<'a>(lines: &mut impl Iterator<Item = &'a str>) -> String {
    let mut block = String::from("{\n");
    for line in lines.by_ref() {
        if line.trim_start().starts_with('}') {
            break;
        }
        block.push_str(line);
        block.push('\n');
    }
    block.push('}');
    block
}

fn parse_frame(rest: &str, is_reset: bool) -> Result<Frame, DiagramError> {
    let invalid = || DiagramError::Syntax(format!("eventmodeling 틀: {rest}"));
    let mut words = rest.splitn(4, char::is_whitespace);
    let name = words.next().ok_or_else(invalid)?.to_string();
    let entity_type = match words.next().ok_or_else(invalid)? {
        "ui" => EntityType::Ui,
        "pcr" | "processor" => EntityType::Processor,
        "rmo" | "readmodel" => EntityType::ReadModel,
        "cmd" | "command" => EntityType::Command,
        "evt" | "event" => EntityType::Event,
        _ => return Err(invalid()),
    };
    let identifier = words.next().ok_or_else(invalid)?.to_string();
    let mut frame = Frame {
        name,
        entity_type,
        identifier,
        source_frames: Vec::new(),
        is_reset,
        data_reference: None,
        inline_data: None,
    };
    read_frame_tail(words.next().unwrap_or(""), &mut frame).ok_or_else(invalid)?;
    Ok(frame)
}

/// 식별자 뒤: `->> 틀`들, `[[참조]]`, 형식 표시와 인라인 데이터
fn read_frame_tail(tail: &str, frame: &mut Frame) -> Option<()> {
    let mut rest = tail.trim_start();
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("->>") {
            let after = after.trim_start();
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            frame.source_frames.push(after[..end].to_string());
            rest = after[end..].trim_start();
        } else if let Some(after) = rest.strip_prefix("[[") {
            let (reference, after) = after.split_once("]]")?;
            frame.data_reference = Some(reference.trim().to_string());
            rest = after.trim_start();
        } else if let Some(after) = rest.strip_prefix('`') {
            rest = after.split_once('`')?.1.trim_start();
        } else {
            frame.inline_data = Some(inline_value(rest)?.to_string());
            return Some(());
        }
    }
    Some(())
}

/// EM_DATA_INLINE: `\{(.*)\}|"(.*)"|'(.*)'`(욕심 많은 일치)
fn inline_value(text: &str) -> Option<&str> {
    let close = match text.chars().next()? {
        '{' => '}',
        '"' => '"',
        '\'' => '\'',
        _ => return None,
    };
    let end = text.rfind(close).filter(|end| *end > 0)?;
    Some(&text[..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 틀과_원천_틀을_읽는다() {
        let model = parse(
            "eventmodeling\ntf 01 ui Shop.Catalog\nnote 01 {\n  tf 99 ui X\n}\nrf 02 evt Orders.Done ->> 01",
        )
        .unwrap();
        assert_eq!(model.frames.len(), 2);
        assert_eq!(model.frames[0].identifier, "Shop.Catalog");
        assert!(model.frames[1].is_reset);
        assert_eq!(model.frames[1].source_frames, vec!["01"]);
    }

    #[test]
    fn 틀_데이터와_data_묶음을_읽는다() {
        let model = parse(
            "eventmodeling\ntf 01 cmd Add { id: 1 }\ntf 02 evt Added ->> 01 [[AddedData]]\ndata AddedData {\n  id: 1\n}",
        )
        .unwrap();
        assert_eq!(model.frames[0].inline_data.as_deref(), Some("{ id: 1 }"));
        assert_eq!(model.frames[1].data_reference.as_deref(), Some("AddedData"));
        assert_eq!(model.data_blocks[0].block, "{\n  id: 1\n}");
    }
}
