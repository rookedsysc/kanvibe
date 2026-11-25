//! stateDiagram·stateDiagram-v2 문법(stateDiagram.jison)의 줄 단위 해석과 `docTranslator`.
//!
//! 문장은 원본 파서처럼 문서(doc) 나무로 남긴다. 복합 상태(`state X { }`)는 자기 문서를 품고, `--`로 나눈
//! 동시 영역은 `docTranslator`가 divider 상태로 묶는다. `[*]`는 감싼 문서 id에 `_start`·`_end`를 붙여 바꾼다.

use crate::DiagramError;
use crate::flowchart::parser::Direction;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StateKind {
    #[default]
    Default,
    Fork,
    Join,
    Choice,
    Divider,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePosition {
    LeftOf,
    RightOf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub position: NotePosition,
    pub text: String,
}

/// 원본의 `{ stmt: 'state', ... }` 문장
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StateItem {
    pub id: String,
    pub kind: StateKind,
    /// 비어 있으면 설명 없음
    pub description: String,
    pub doc: Option<Vec<Statement>>,
    pub note: Option<Note>,
    pub classes: Vec<String>,
    /// `[*]`를 바꾼 노드: 시작이면 `Some(true)`, 끝이면 `Some(false)`
    pub start: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    State(StateItem),
    Relation {
        from: StateItem,
        to: StateItem,
        label: String,
    },
    Direction(Direction),
    ClassDef {
        name: String,
        styles: Vec<String>,
    },
    ApplyClass {
        ids: Vec<String>,
        class_name: String,
    },
    Style {
        ids: Vec<String>,
        styles: Vec<String>,
    },
}

const START_NODE: &str = "[*]";

/// 열린 복합 상태와 그 안에 쌓는 문장들
struct OpenDocument {
    header: Option<StateItem>,
    statements: Vec<Statement>,
}

/// 정의를 해석하고 `docTranslator`까지 마친 최상위 문서를 돌려준다
pub fn parse(definition: &str) -> Result<Vec<Statement>, DiagramError> {
    let mut lines = definition
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    if !header.starts_with("stateDiagram") {
        return Err(DiagramError::Syntax(format!(
            "stateDiagram 머리줄: {header}"
        )));
    }
    let mut parser = StateParser::default();
    while let Some(line) = lines.next() {
        parser.read_line(line, &mut lines)?;
    }
    let mut root = parser.finish()?;
    translate_document("root", &mut root, &mut parser.divider_count);
    Ok(root)
}

#[derive(Default)]
struct StateParser {
    stack: Vec<OpenDocument>,
    root: Vec<Statement>,
    divider_count: usize,
}

impl StateParser {
    fn push(&mut self, statement: Statement) {
        match self.stack.last_mut() {
            Some(open) => open.statements.push(statement),
            None => self.root.push(statement),
        }
    }

    fn finish(&mut self) -> Result<Vec<Statement>, DiagramError> {
        if !self.stack.is_empty() {
            return Err(DiagramError::Syntax("닫히지 않은 state { }".to_string()));
        }
        Ok(std::mem::take(&mut self.root))
    }

    fn read_line<'a>(
        &mut self,
        line: &str,
        rest: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), DiagramError> {
        if is_ignored_line(line) {
            skip_block(line, rest);
            return Ok(());
        }
        if line == "}" {
            return self.close_document();
        }
        if line == "--" {
            self.divider_count += 1;
            let id = format!("divider-id-{}", self.divider_count);
            self.push(Statement::State(StateItem {
                id,
                kind: StateKind::Divider,
                ..StateItem::default()
            }));
            return Ok(());
        }
        if let Some(note) = line.strip_prefix("note ") {
            return self.read_note(note.trim(), rest);
        }
        if let Some(statement) = parse_keyword_statement(line) {
            self.push(statement);
            return Ok(());
        }
        if let Some(state) = line.strip_prefix("state ") {
            return self.read_state(state.trim());
        }
        let statement = parse_plain_statement(line)?;
        self.push(statement);
        Ok(())
    }

    fn close_document(&mut self) -> Result<(), DiagramError> {
        let open = self
            .stack
            .pop()
            .ok_or_else(|| DiagramError::Syntax("짝 없는 }".to_string()))?;
        let mut state = open.header.unwrap_or_default();
        state.doc = Some(open.statements);
        self.push(Statement::State(state));
        Ok(())
    }

    /// `state …` 문장: 복합 상태를 열거나 fork·join·choice·설명 붙은 상태를 만든다
    fn read_state(&mut self, body: &str) -> Result<(), DiagramError> {
        if let Some(head) = body.strip_suffix('{') {
            let header = parse_state_header(head.trim())?;
            self.stack.push(OpenDocument {
                header: Some(header),
                statements: Vec::new(),
            });
            return Ok(());
        }
        let state = parse_state_header(body)?;
        self.push(Statement::State(state));
        Ok(())
    }

    /// `note right of X : 글` 한 줄 또는 `end note`로 끝나는 여러 줄 메모
    fn read_note<'a>(
        &mut self,
        body: &str,
        rest: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(), DiagramError> {
        let (position, target) = if let Some(target) = body.strip_prefix("left of") {
            (NotePosition::LeftOf, target)
        } else if let Some(target) = body.strip_prefix("right of") {
            (NotePosition::RightOf, target)
        } else {
            // 떠 있는 메모(`note "글" as N`)는 원본 렌더러도 그리지 않는다
            return Ok(());
        };
        let (id, text) = match target.split_once(':') {
            Some((id, text)) => (id.trim(), text.trim().to_string()),
            None => (target.trim(), read_note_block(rest)),
        };
        self.push(Statement::State(StateItem {
            id: id.to_string(),
            note: Some(Note { position, text }),
            ..StateItem::default()
        }));
        Ok(())
    }
}

/// 그림에 영향이 없는 문장(제목·접근성 설명·클릭·크기·주석)
fn is_ignored_line(line: &str) -> bool {
    [
        "accTitle",
        "accDescr",
        "title ",
        "click ",
        "scale ",
        "hide empty description",
        "#",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

/// `accDescr { … }` 같은 여러 줄 블록은 닫는 `}`까지 건너뛴다
fn skip_block<'a>(line: &str, rest: &mut impl Iterator<Item = &'a str>) {
    if line.starts_with("accDescr") && line.ends_with('{') {
        for inner in rest.by_ref() {
            if inner.ends_with('}') {
                break;
            }
        }
    }
}

fn read_note_block<'a>(rest: &mut impl Iterator<Item = &'a str>) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for line in rest.by_ref() {
        if line == "end note" {
            break;
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// direction·classDef·class·style 문장
fn parse_keyword_statement(line: &str) -> Option<Statement> {
    if let Some(value) = line.strip_prefix("direction ") {
        return parse_direction(value.trim()).map(Statement::Direction);
    }
    if let Some(body) = line.strip_prefix("classDef ") {
        let (name, styles) = body.trim().split_once(char::is_whitespace)?;
        return Some(Statement::ClassDef {
            name: name.to_string(),
            styles: split_styles(styles),
        });
    }
    if let Some(body) = line.strip_prefix("class ") {
        let (ids, class_name) = body.trim().rsplit_once(char::is_whitespace)?;
        return Some(Statement::ApplyClass {
            ids: split_ids(ids),
            class_name: class_name.trim().to_string(),
        });
    }
    let body = line.strip_prefix("style ")?;
    let (ids, styles) = body.trim().split_once(char::is_whitespace)?;
    Some(Statement::Style {
        ids: split_ids(ids),
        styles: split_styles(styles),
    })
}

fn parse_direction(value: &str) -> Option<Direction> {
    match value {
        "TB" | "TD" => Some(Direction::TopBottom),
        "BT" => Some(Direction::BottomTop),
        "LR" => Some(Direction::LeftRight),
        "RL" => Some(Direction::RightLeft),
        _ => None,
    }
}

fn split_ids(ids: &str) -> Vec<String> {
    ids.split(',')
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

/// addStyleClass·handleStyleDef: 쉼표로 나누고 `;`를 지운다
fn split_styles(styles: &str) -> Vec<String> {
    styles
        .split(',')
        .map(|style| style.replace(';', "").trim().to_string())
        .filter(|style| !style.is_empty())
        .collect()
}

/// `state` 뒤의 머리: `"설명" as X`, `X <<fork>>`, `X`
fn parse_state_header(body: &str) -> Result<StateItem, DiagramError> {
    if let Some(quoted) = body.strip_prefix('"') {
        let (description, rest) = quoted
            .split_once('"')
            .ok_or_else(|| DiagramError::Syntax(format!("닫히지 않은 따옴표: {body}")))?;
        let id = rest.trim().strip_prefix("as").unwrap_or(rest).trim();
        return Ok(StateItem {
            id: id.to_string(),
            description: description.trim().to_string(),
            ..StateItem::default()
        });
    }
    for (marker, kind) in [
        ("fork", StateKind::Fork),
        ("join", StateKind::Join),
        ("choice", StateKind::Choice),
    ] {
        let angle = format!("<<{marker}>>");
        let square = format!("[[{marker}]]");
        if let Some(id) = body
            .strip_suffix(angle.as_str())
            .or_else(|| body.strip_suffix(square.as_str()))
        {
            return Ok(StateItem {
                id: id.trim().to_string(),
                kind,
                ..StateItem::default()
            });
        }
    }
    let (id, description) = match body.split_once(':') {
        Some((id, description)) => (id.trim(), description.trim()),
        None => (body.trim(), ""),
    };
    Ok(state_reference(id, description))
}

/// 전이(`A --> B : 글`)나 설명(`A : 글`), 홀로 쓴 상태(`A`, `A:::클래스`)
fn parse_plain_statement(line: &str) -> Result<Statement, DiagramError> {
    if let Some((left, right)) = line.split_once("-->") {
        let from = parse_reference(left.trim())?;
        let (to, label) = split_target_label(right.trim())?;
        return Ok(Statement::Relation { from, to, label });
    }
    match split_description(line) {
        Some((id, description)) => Ok(Statement::State(state_reference(id, description))),
        None => Ok(Statement::State(parse_reference(line)?)),
    }
}

/// `:::`가 아닌 첫 `:`에서 id와 설명을 나눈다
fn split_description(line: &str) -> Option<(&str, &str)> {
    let (id, rest) = split_class_suffix(line);
    let index = rest.find(':')?;
    let description = rest[index + 1..].trim();
    let id_end = id.len() + index;
    Some((line[..id_end].trim(), description))
}

/// `X:::클래스` 꼴이면 (`X:::클래스`, 나머지)로, 아니면 (첫 낱말, 나머지)로 나눈다
fn split_class_suffix(text: &str) -> (&str, &str) {
    let id_end = text
        .find(|character: char| character == ':' || character.is_whitespace())
        .unwrap_or(text.len());
    let after_id = &text[id_end..];
    if let Some(class_part) = after_id.strip_prefix(":::") {
        let class_end = class_part
            .find(|character: char| character == ':' || character.is_whitespace())
            .unwrap_or(class_part.len());
        let end = id_end + 3 + class_end;
        return (&text[..end], &text[end..]);
    }
    (&text[..id_end], after_id)
}

fn split_target_label(right: &str) -> Result<(StateItem, String), DiagramError> {
    let (target, rest) = split_class_suffix(right);
    let rest = rest.trim();
    let label = rest
        .strip_prefix(':')
        .map(|label| label.trim().to_string())
        .unwrap_or_default();
    Ok((parse_reference(target.trim())?, label))
}

/// 전이 양 끝이나 홀로 쓴 상태 참조(`[*]`, `X`, `X:::클래스`)
fn parse_reference(text: &str) -> Result<StateItem, DiagramError> {
    if text.is_empty() {
        return Err(DiagramError::Syntax("상태 id가 비어 있습니다".to_string()));
    }
    let (id, class_name) = match text.split_once(":::") {
        Some((id, class_name)) => (id.trim(), Some(class_name.trim().to_string())),
        None => (text, None),
    };
    let mut state = state_reference(id, "");
    state.classes.extend(class_name);
    Ok(state)
}

fn state_reference(id: &str, description: &str) -> StateItem {
    StateItem {
        id: id.to_string(),
        description: description.to_string(),
        ..StateItem::default()
    }
}

/// docTranslator: `[*]`를 `<부모>_start`·`<부모>_end`로 바꾸고 동시 영역(divider)을 묶는다
fn translate_document(parent_id: &str, doc: &mut Vec<Statement>, divider_count: &mut usize) {
    group_dividers(doc, divider_count);
    for statement in doc.iter_mut() {
        match statement {
            Statement::Relation { from, to, .. } => {
                translate_reference(parent_id, from, true);
                translate_reference(parent_id, to, false);
            }
            Statement::State(state) => {
                translate_reference(parent_id, state, true);
                if let Some(inner) = state.doc.as_mut() {
                    translate_document(&state.id, inner, divider_count);
                }
            }
            _ => {}
        }
    }
}

fn translate_reference(parent_id: &str, state: &mut StateItem, is_first: bool) {
    if state.id == START_NODE {
        let suffix = if is_first { "_start" } else { "_end" };
        state.id = format!("{parent_id}{suffix}");
        state.start = Some(is_first);
    }
}

/// divider 문장마다 그 앞 문장들을 품게 하고, 남은 꼬리는 새 divider로 묶는다(원본처럼 꼬리가 없으면 그대로 둔다)
fn group_dividers(doc: &mut Vec<Statement>, divider_count: &mut usize) {
    let mut grouped: Vec<Statement> = Vec::new();
    let mut current: Vec<Statement> = Vec::new();
    for statement in doc.iter() {
        match statement {
            Statement::State(state) if state.kind == StateKind::Divider => {
                let mut divider = state.clone();
                divider.doc = Some(std::mem::take(&mut current));
                grouped.push(Statement::State(divider));
            }
            other => current.push(other.clone()),
        }
    }
    if grouped.is_empty() || current.is_empty() {
        return;
    }
    *divider_count += 1;
    grouped.push(Statement::State(StateItem {
        id: format!("divider-tail-{divider_count}"),
        kind: StateKind::Divider,
        doc: Some(current),
        ..StateItem::default()
    }));
    *doc = grouped;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relation_ids(statement: &Statement) -> (String, String, String) {
        match statement {
            Statement::Relation { from, to, label } => {
                (from.id.clone(), to.id.clone(), label.clone())
            }
            other => panic!("전이가 아님: {other:?}"),
        }
    }

    #[test]
    fn 시작_끝과_라벨을_읽는다() {
        let doc = parse("stateDiagram-v2\n[*] --> Todo\nTodo --> Done : merge it\nDone --> [*]")
            .expect("parse");
        assert_eq!(
            relation_ids(&doc[0]),
            ("root_start".into(), "Todo".into(), "".into())
        );
        assert_eq!(relation_ids(&doc[1]).2, "merge it");
        assert_eq!(relation_ids(&doc[2]).1, "root_end");
    }

    #[test]
    fn 복합_상태와_특수_상태와_메모를_읽는다() {
        let source = "stateDiagram\nstate Working {\n[*] --> Fetch\n}\nstate Split <<fork>>\n\
                      state \"긴 이름\" as Long\nnote left of Long\n첫 줄\n둘째 줄\nend note";
        let doc = parse(source).expect("parse");
        let Statement::State(working) = &doc[0] else {
            panic!("복합 상태")
        };
        let inner = working.doc.as_ref().expect("doc");
        assert_eq!(relation_ids(&inner[0]).0, "Working_start");
        let Statement::State(split) = &doc[1] else {
            panic!("fork")
        };
        assert_eq!(split.kind, StateKind::Fork);
        let Statement::State(long) = &doc[2] else {
            panic!("설명")
        };
        assert_eq!(
            (long.id.as_str(), long.description.as_str()),
            ("Long", "긴 이름")
        );
        let Statement::State(note) = &doc[3] else {
            panic!("메모")
        };
        assert_eq!(note.note.as_ref().expect("note").text, "첫 줄\n둘째 줄");
    }

    #[test]
    fn 동시_영역은_divider로_묶는다() {
        let doc = parse("stateDiagram-v2\nstate P {\nA --> B\n--\nC --> D\n}").expect("parse");
        let Statement::State(parallel) = &doc[0] else {
            panic!("복합 상태")
        };
        let regions = parallel.doc.as_ref().expect("doc");
        assert_eq!(regions.len(), 2);
        assert!(regions.iter().all(|region| matches!(
            region,
            Statement::State(state) if state.kind == StateKind::Divider && state.doc.is_some()
        )));
    }
}
