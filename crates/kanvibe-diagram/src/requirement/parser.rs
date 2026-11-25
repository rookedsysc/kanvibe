//! requirementDiagram 정의 파서. Mermaid requirementDiagram.jison의 문장과 RequirementDB의 저장 순서를 따른다.
//!
//! 지원: `direction`, 요구사항 여섯 종류(`id`·`text`·`risk`·`verifymethod`), `element`(`type`·`docref`),
//! 관계 일곱 종류(`a - rel -> b`, `a <- rel - b`), 따옴표 값, `accTitle`·`accDescr`, `%%` 주석.
//! `classDef`·`class`·`style`·`:::`는 문법 오류로 돌려 원문 표시로 넘긴다.

use crate::DiagramError;
use crate::flowchart::parser::Direction;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Requirement {
    pub name: String,
    /// 표시 이름(`Functional Requirement` 등)
    pub kind: String,
    pub requirement_id: String,
    pub text: String,
    pub risk: String,
    pub verify_method: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub element_type: String,
    pub doc_ref: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationKind {
    Contains,
    Copies,
    Derives,
    Satisfies,
    Verifies,
    Refines,
    Traces,
}

impl RelationKind {
    fn parse(word: &str) -> Option<Self> {
        let kind = match word.to_ascii_lowercase().as_str() {
            "contains" => Self::Contains,
            "copies" => Self::Copies,
            "derives" => Self::Derives,
            "satisfies" => Self::Satisfies,
            "verifies" => Self::Verifies,
            "refines" => Self::Refines,
            "traces" => Self::Traces,
            _ => return None,
        };
        Some(kind)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Copies => "copies",
            Self::Derives => "derives",
            Self::Satisfies => "satisfies",
            Self::Verifies => "verifies",
            Self::Refines => "refines",
            Self::Traces => "traces",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub kind: RelationKind,
    pub source: String,
    pub target: String,
}

#[derive(Clone, Debug)]
pub struct RequirementDiagram {
    pub direction: Direction,
    pub requirements: Vec<Requirement>,
    pub elements: Vec<Element>,
    pub relations: Vec<Relation>,
}

pub fn parse(definition: &str) -> Result<RequirementDiagram, DiagramError> {
    let mut diagram = RequirementDiagram {
        direction: Direction::TopBottom,
        requirements: Vec::new(),
        elements: Vec::new(),
        relations: Vec::new(),
    };
    let lines: Vec<&str> = definition
        .lines()
        .map(|line| line.split("%%").next().unwrap_or("").trim())
        .collect();
    let header = lines
        .iter()
        .position(|line| !line.is_empty())
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    if !matches!(lines[header], "requirementDiagram" | "requirement") {
        return Err(syntax_error(lines[header]));
    }
    let mut index = header + 1;
    while index < lines.len() {
        index = parse_statement(&mut diagram, &lines, index)?;
    }
    Ok(diagram)
}

fn syntax_error(line: &str) -> DiagramError {
    DiagramError::Syntax(format!("알 수 없는 문장: {line}"))
}

/// 한 문장을 읽고 다음 줄 번호를 돌려준다(블록은 `}`까지 먹는다)
fn parse_statement(
    diagram: &mut RequirementDiagram,
    lines: &[&str],
    index: usize,
) -> Result<usize, DiagramError> {
    let line = lines[index];
    let keyword = line.split_whitespace().next().unwrap_or("");
    if line.is_empty() || line.starts_with("accTitle") || line.starts_with("title ") {
        return Ok(index + 1);
    }
    if line.starts_with("accDescr") {
        return Ok(skip_description(lines, index));
    }
    if let Some(direction) = parse_direction(line) {
        diagram.direction = direction;
        return Ok(index + 1);
    }
    if matches!(keyword, "classDef" | "class" | "style") || line.contains(":::") {
        return Err(syntax_error(line));
    }
    if let Some(kind) = requirement_kind(keyword) {
        return parse_requirement(diagram, lines, index, kind);
    }
    if keyword.eq_ignore_ascii_case("element") {
        return parse_element(diagram, lines, index);
    }
    let relation = parse_relation(line).ok_or_else(|| syntax_error(line))?;
    diagram.relations.push(relation);
    Ok(index + 1)
}

fn skip_description(lines: &[&str], index: usize) -> usize {
    let is_block = lines[index]
        .strip_prefix("accDescr")
        .is_some_and(|rest| rest.trim_start().starts_with('{'));
    if !is_block {
        return index + 1;
    }
    lines[index..]
        .iter()
        .position(|line| line.contains('}'))
        .map_or(lines.len(), |offset| index + offset + 1)
}

fn parse_direction(line: &str) -> Option<Direction> {
    match line.strip_prefix("direction")?.trim() {
        "TB" => Some(Direction::TopBottom),
        "BT" => Some(Direction::BottomTop),
        "LR" => Some(Direction::LeftRight),
        "RL" => Some(Direction::RightLeft),
        _ => None,
    }
}

/// RequirementDB.RequirementType
fn requirement_kind(keyword: &str) -> Option<&'static str> {
    let kind = match keyword.to_ascii_lowercase().as_str() {
        "requirement" => "Requirement",
        "functionalrequirement" => "Functional Requirement",
        "interfacerequirement" => "Interface Requirement",
        "performancerequirement" => "Performance Requirement",
        "physicalrequirement" => "Physical Requirement",
        "designconstraint" => "Design Constraint",
        _ => return None,
    };
    Some(kind)
}

/// `<종류> <이름> {` 머리 줄의 이름
fn block_name(line: &str) -> Result<String, DiagramError> {
    let rest = line
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest)
        .ok_or_else(|| syntax_error(line))?;
    let name = rest
        .trim()
        .strip_suffix('{')
        .ok_or_else(|| syntax_error(line))?;
    Ok(value_text(name))
}

/// 따옴표 값은 안쪽 글자, 아니면 앞뒤 공백을 뺀 글자
fn value_text(value: &str) -> String {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

/// 블록 본문의 `키: 값` 줄들과 블록 다음 줄 번호
fn block_fields(
    lines: &[&str],
    index: usize,
) -> Result<(Vec<(String, String)>, usize), DiagramError> {
    let mut fields = Vec::new();
    for (offset, line) in lines[index + 1..].iter().enumerate() {
        if *line == "}" {
            return Ok((fields, index + offset + 2));
        }
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| syntax_error(line))?;
        fields.push((key.trim().to_ascii_lowercase(), value_text(value)));
    }
    Err(DiagramError::Syntax("블록이 닫히지 않음".to_string()))
}

fn parse_requirement(
    diagram: &mut RequirementDiagram,
    lines: &[&str],
    index: usize,
    kind: &str,
) -> Result<usize, DiagramError> {
    let name = block_name(lines[index])?;
    let (fields, next) = block_fields(lines, index)?;
    let mut requirement = Requirement {
        name,
        kind: kind.to_string(),
        ..Requirement::default()
    };
    for (key, value) in fields {
        match key.as_str() {
            "id" => requirement.requirement_id = value,
            "text" => requirement.text = value,
            "risk" => requirement.risk = keyword_value(&value, &["Low", "Medium", "High"])?,
            "verifymethod" => {
                let methods = ["Analysis", "Demonstration", "Inspection", "Test"];
                requirement.verify_method = keyword_value(&value, &methods)?;
            }
            _ => return Err(syntax_error(&key)),
        }
    }
    if !diagram
        .requirements
        .iter()
        .any(|existing| existing.name == requirement.name)
    {
        diagram.requirements.push(requirement);
    }
    Ok(next)
}

/// 대소문자를 가리지 않는 낱말 값(위험도·검증 방법)을 표시 글자로 바꾼다
fn keyword_value(value: &str, choices: &[&str]) -> Result<String, DiagramError> {
    choices
        .iter()
        .find(|choice| choice.eq_ignore_ascii_case(value))
        .map(|choice| choice.to_string())
        .ok_or_else(|| syntax_error(value))
}

fn parse_element(
    diagram: &mut RequirementDiagram,
    lines: &[&str],
    index: usize,
) -> Result<usize, DiagramError> {
    let name = block_name(lines[index])?;
    let (fields, next) = block_fields(lines, index)?;
    let mut element = Element {
        name,
        ..Element::default()
    };
    for (key, value) in fields {
        match key.as_str() {
            "type" => element.element_type = value,
            "docref" => element.doc_ref = value,
            _ => return Err(syntax_error(&key)),
        }
    }
    if !diagram
        .elements
        .iter()
        .any(|existing| existing.name == element.name)
    {
        diagram.elements.push(element);
    }
    Ok(next)
}

/// `a - rel -> b`(a에서 b로) 또는 `a <- rel - b`(b에서 a로)
fn parse_relation(line: &str) -> Option<Relation> {
    if let Some((left, rest)) = line.split_once("<-") {
        let (kind, right) = rest.split_once('-')?;
        return Some(Relation {
            kind: RelationKind::parse(kind.trim())?,
            source: value_text(right),
            target: value_text(left),
        });
    }
    let (left, rest) = line.split_once('-')?;
    let (kind, right) = rest.split_once("->")?;
    Some(Relation {
        kind: RelationKind::parse(kind.trim())?,
        source: value_text(left),
        target: value_text(right),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 요구사항과_요소와_관계를_읽는다() {
        let diagram = parse(
            "requirementDiagram\n    functionalRequirement test_req {\n    id: 1\n    text: \"the test text.\"\n    risk: high\n    verifymethod: test\n    }\n    element test_entity {\n    type: simulation\n    docRef: reqs/test_entity\n    }\n    test_entity - satisfies -> test_req\n    test_req <- contains - test_entity",
        )
        .unwrap();
        let requirement = &diagram.requirements[0];
        assert_eq!(requirement.kind, "Functional Requirement");
        assert_eq!(requirement.text, "the test text.");
        assert_eq!(requirement.risk, "High");
        assert_eq!(requirement.verify_method, "Test");
        assert_eq!(diagram.elements[0].doc_ref, "reqs/test_entity");
        assert_eq!(diagram.relations[0].source, "test_entity");
        assert_eq!(diagram.relations[1].kind, RelationKind::Contains);
        assert_eq!(diagram.relations[1].source, "test_entity");
        assert_eq!(diagram.relations[1].target, "test_req");
    }

    #[test]
    fn 알_수_없는_위험도는_문법_오류다() {
        assert!(parse("requirementDiagram\n  requirement r {\n  risk: extreme\n  }").is_err());
    }
}
