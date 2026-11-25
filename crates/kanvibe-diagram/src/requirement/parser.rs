//! requirementDiagram 정의 파서. Mermaid requirementDiagram.jison의 문장과 RequirementDB의 저장 순서를 따른다.
//!
//! 지원: `direction`, 요구사항 여섯 종류(`id`·`text`·`risk`·`verifymethod`), `element`(`type`·`docref`),
//! 관계 일곱 종류(`a - rel -> b`, `a <- rel - b`), 따옴표 값, `accTitle`·`accDescr`, `%%` 주석,
//! `classDef`·`class`·`style`, 블록 머리와 홀로 쓴 `이름:::클래스`.

use std::collections::HashMap;

use crate::DiagramError;
use crate::directive::diagram_body;
use crate::flowchart::parser::Direction;

/// 노드에 붙은 클래스와 쌓인 스타일 선언(RequirementDB의 classes·cssStyles)
#[derive(Clone, Debug, PartialEq)]
pub struct StyleAssignment {
    pub classes: Vec<String>,
    pub declarations: Vec<String>,
}

impl Default for StyleAssignment {
    fn default() -> Self {
        Self {
            classes: vec!["default".to_string()],
            declarations: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Requirement {
    pub name: String,
    /// 표시 이름(`Functional Requirement` 등)
    pub kind: String,
    pub requirement_id: String,
    pub text: String,
    pub risk: String,
    pub verify_method: String,
    pub style: StyleAssignment,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub element_type: String,
    pub doc_ref: String,
    pub style: StyleAssignment,
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
    /// `classDef` 이름 → 선언 목록
    pub class_styles: HashMap<String, Vec<String>>,
}

impl RequirementDiagram {
    /// 요구사항을 먼저, 없으면 요소를 찾는다
    fn style_mut(&mut self, name: &str) -> Option<&mut StyleAssignment> {
        let requirement = self.requirements.iter_mut().find(|node| node.name == name);
        match requirement {
            Some(node) => Some(&mut node.style),
            None => self
                .elements
                .iter_mut()
                .find(|node| node.name == name)
                .map(|node| &mut node.style),
        }
    }

    /// RequirementDB.setClass: 클래스를 붙이고 그때까지 정의된 선언을 바로 쌓는다
    fn set_class(&mut self, ids: &[String], class_names: &[String]) {
        for id in ids {
            for class_name in class_names {
                let defined = self
                    .class_styles
                    .get(class_name)
                    .cloned()
                    .unwrap_or_default();
                if let Some(style) = self.style_mut(id) {
                    style.classes.push(class_name.clone());
                    style.declarations.extend(defined);
                }
            }
        }
    }

    /// RequirementDB.defineClass: 이미 그 클래스를 단 노드에도 선언을 쌓는다
    fn define_class(&mut self, ids: &[String], declarations: &[String]) {
        for id in ids {
            let styles = self.class_styles.entry(id.clone()).or_default();
            styles.extend(declarations.iter().cloned());
            let nodes = self.requirements.iter_mut().map(|node| &mut node.style);
            let nodes = nodes.chain(self.elements.iter_mut().map(|node| &mut node.style));
            for style in nodes.filter(|style| style.classes.contains(id)) {
                style.declarations.extend(declarations.iter().cloned());
            }
        }
    }

    /// RequirementDB.setCssStyle: 없는 노드를 만나면 거기서 멈춘다
    fn set_css_style(&mut self, ids: &[String], declarations: &[String]) {
        for id in ids {
            let Some(style) = self.style_mut(id) else {
                return;
            };
            style.declarations.extend(declarations.iter().cloned());
        }
    }
}

pub fn parse(definition: &str) -> Result<RequirementDiagram, DiagramError> {
    let mut diagram = RequirementDiagram {
        direction: Direction::TopBottom,
        requirements: Vec::new(),
        elements: Vec::new(),
        relations: Vec::new(),
        class_styles: HashMap::new(),
    };
    let body = diagram_body(definition);
    let lines: Vec<&str> = body
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
    if matches!(keyword, "classDef" | "class" | "style") {
        parse_style_statement(diagram, keyword, line)?;
        return Ok(index + 1);
    }
    if let Some(kind) = requirement_kind(keyword) {
        return parse_requirement(diagram, lines, index, kind);
    }
    if keyword.eq_ignore_ascii_case("element") {
        return parse_element(diagram, lines, index);
    }
    if let Some((name, classes)) = line.split_once(":::") {
        diagram.set_class(&[value_text(name)], &split_list(classes));
        return Ok(index + 1);
    }
    let relation = parse_relation(line).ok_or_else(|| syntax_error(line))?;
    diagram.relations.push(relation);
    Ok(index + 1)
}

/// `classDef 이름들 선언`, `class 노드들 클래스들`, `style 노드들 선언`
fn parse_style_statement(
    diagram: &mut RequirementDiagram,
    keyword: &str,
    line: &str,
) -> Result<(), DiagramError> {
    let rest = line[keyword.len()..].trim();
    let (ids, values) = rest
        .split_once(char::is_whitespace)
        .ok_or_else(|| syntax_error(line))?;
    let ids = split_list(ids);
    let values = split_list(values);
    match keyword {
        "classDef" => diagram.define_class(&ids, &values),
        "class" => diagram.set_class(&ids, &values),
        _ => diagram.set_css_style(&ids, &values),
    }
    Ok(())
}

/// 쉼표로 나눈 목록(빈 칸은 버린다)
fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
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

/// `<종류> <이름>[:::클래스들] {` 머리 줄의 이름과 클래스
fn block_name(line: &str) -> Result<(String, Vec<String>), DiagramError> {
    let rest = line
        .split_once(char::is_whitespace)
        .map(|(_, rest)| rest)
        .ok_or_else(|| syntax_error(line))?;
    let head = rest
        .trim()
        .strip_suffix('{')
        .ok_or_else(|| syntax_error(line))?;
    match head.split_once(":::") {
        Some((name, classes)) => Ok((value_text(name), split_list(classes))),
        None => Ok((value_text(head), Vec::new())),
    }
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
    let (name, classes) = block_name(lines[index])?;
    let (fields, next) = block_fields(lines, index)?;
    let mut requirement = Requirement {
        name: name.clone(),
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
    diagram.set_class(&[name], &classes);
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
    let (name, classes) = block_name(lines[index])?;
    let (fields, next) = block_fields(lines, index)?;
    let mut element = Element {
        name: name.clone(),
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
    diagram.set_class(&[name], &classes);
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
    fn 클래스와_스타일_선언을_순서대로_쌓는다() {
        let diagram = parse(
            "---\ntitle: T\n---\nrequirementDiagram\n  classDef early stroke:#111\n  requirement r:::early {\n  id: 1\n  }\n  element e {\n  type: doc\n  }\n  e:::late\n  classDef late fill:#fd8,color:#900\n  class r late\n  style e,r stroke-width:3px",
        )
        .unwrap();
        let requirement = &diagram.requirements[0].style;
        assert_eq!(requirement.classes, ["default", "early", "late"]);
        assert_eq!(
            requirement.declarations,
            ["stroke:#111", "fill:#fd8", "color:#900", "stroke-width:3px"]
        );
        let element = &diagram.elements[0].style;
        assert_eq!(
            element.declarations,
            ["fill:#fd8", "color:#900", "stroke-width:3px"]
        );
    }

    #[test]
    fn 알_수_없는_위험도는_문법_오류다() {
        assert!(parse("requirementDiagram\n  requirement r {\n  risk: extreme\n  }").is_err());
    }
}
