//! erDiagram 정의 파서. Mermaid erDiagram.jison의 토큰 규칙과 ErDB의 저장 순서를 따른다.
//!
//! 지원: `direction`, 엔티티(따옴표 이름, `[별칭]`), 속성 블록(타입·이름·PK/FK/UK 목록·따옴표 설명),
//! 관계(기호·낱말 차수 전부, `--`·`to` 식별 관계, `..`·`.-`·`-.`·`optionally to` 비식별 관계, 따옴표 역할),
//! `accTitle`·`accDescr`, `%%` 주석, `classDef`·`class`·`style`·`:::`(엔티티 문장과 관계 양쪽).

use std::collections::HashMap;

use crate::DiagramError;
use crate::directive::diagram_body;
use crate::flowchart::parser::Direction;

/// ErDB.Cardinality
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cardinality {
    ZeroOrOne,
    ZeroOrMore,
    OneOrMore,
    OnlyOne,
    /// `u`: 원본에 표식이 없어 아무것도 그리지 않는다
    MdParent,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attribute {
    pub attribute_type: String,
    pub name: String,
    pub keys: Vec<String>,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    pub name: String,
    pub alias: String,
    pub attributes: Vec<Attribute>,
    /// `:::`·`class`로 붙인 클래스 이름(원본 cssClasses에서 `default`를 뺀 나머지)
    pub classes: Vec<String>,
    /// `style` 문장의 선언(원본 cssStyles)
    pub styles: Vec<String>,
}

/// 관계 한 줄. 왼쪽 차수는 시작(왼쪽 엔티티) 쪽, 오른쪽 차수는 끝 쪽에 그린다
#[derive(Clone, Debug, PartialEq)]
pub struct Relationship {
    pub start: String,
    pub end: String,
    pub role: String,
    pub start_cardinality: Cardinality,
    pub end_cardinality: Cardinality,
    pub is_identifying: bool,
    /// 양쪽 엔티티 이름 뒤 `:::`로 붙인 클래스
    pub start_classes: Vec<String>,
    pub end_classes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ErDiagram {
    pub direction: Direction,
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
    /// `classDef` 이름 → 선언 목록
    pub class_styles: HashMap<String, Vec<String>>,
}

impl ErDiagram {
    /// ErDB.addEntity: 처음 나온 순서를 지키고, 별칭은 비어 있을 때만 채운다
    fn add_entity(&mut self, name: &str, alias: &str) -> &mut Entity {
        let index = match self.entities.iter().position(|entity| entity.name == name) {
            Some(index) => index,
            None => {
                self.entities.push(Entity {
                    name: name.to_string(),
                    alias: String::new(),
                    attributes: Vec::new(),
                    classes: Vec::new(),
                    styles: Vec::new(),
                });
                self.entities.len() - 1
            }
        };
        let entity = &mut self.entities[index];
        if entity.alias.is_empty() && !alias.is_empty() {
            entity.alias = alias.to_string();
        }
        entity
    }

    /// ErDB.setClass: 있는 엔티티에만 클래스를 덧붙인다
    fn set_class(&mut self, ids: &[String], classes: &[String]) {
        for entity in &mut self.entities {
            if ids.contains(&entity.name) {
                entity.classes.extend(classes.iter().cloned());
            }
        }
    }

    /// ErDB.addCssStyles: 없는 엔티티를 만나면 거기서 멈춘다
    fn add_css_styles(&mut self, ids: &[String], styles: &[String]) {
        for id in ids {
            let Some(entity) = self.entities.iter_mut().find(|entity| &entity.name == id) else {
                return;
            };
            entity.styles.extend(styles.iter().cloned());
        }
    }

    /// cssCompiledStyles(`default`와 붙인 클래스의 classDef 선언) 뒤에 cssStyles를 이은 선언 목록
    pub fn entity_declarations(&self, entity: &Entity) -> Vec<String> {
        let class_names =
            std::iter::once("default").chain(entity.classes.iter().map(String::as_str));
        let mut declarations: Vec<String> = class_names
            .filter_map(|name| self.class_styles.get(name))
            .flatten()
            .cloned()
            .collect();
        declarations.extend(entity.styles.iter().cloned());
        declarations
    }
}

pub fn parse(definition: &str) -> Result<ErDiagram, DiagramError> {
    let mut diagram = ErDiagram {
        direction: Direction::TopBottom,
        entities: Vec::new(),
        relationships: Vec::new(),
        class_styles: HashMap::new(),
    };
    let body = diagram_body(definition);
    let lines: Vec<&str> = body
        .lines()
        .map(|line| line.split("%%").next().unwrap_or("").trim())
        .collect();
    let mut index = first_statement(&lines)?;
    while index < lines.len() {
        index = parse_statement(&mut diagram, &lines, index)?;
    }
    Ok(diagram)
}

/// 머리 줄(`erDiagram`) 다음 줄 번호
fn first_statement(lines: &[&str]) -> Result<usize, DiagramError> {
    let header = lines
        .iter()
        .position(|line| !line.is_empty())
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    if lines[header] != "erDiagram" {
        return Err(DiagramError::Syntax(format!(
            "erDiagram 머리 줄이 아님: {}",
            lines[header]
        )));
    }
    Ok(header + 1)
}

/// 한 문장을 읽고 다음 줄 번호를 돌려준다(속성 블록과 여러 줄 accDescr는 여러 줄을 먹는다)
fn parse_statement(
    diagram: &mut ErDiagram,
    lines: &[&str],
    index: usize,
) -> Result<usize, DiagramError> {
    let line = lines[index];
    if line.is_empty() || line.starts_with("accTitle") || is_single_line_description(line) {
        return Ok(index + 1);
    }
    if line.starts_with("accDescr") {
        return Ok(skip_until_block_end(lines, index));
    }
    if let Some(direction) = parse_direction(line) {
        diagram.direction = direction;
        return Ok(index + 1);
    }
    if parse_style_statement(diagram, line)? {
        return Ok(index + 1);
    }
    if let Some(relationship) = parse_relationship(line)? {
        diagram.add_entity(&relationship.start, "");
        diagram.add_entity(&relationship.end, "");
        let start = [relationship.start.clone()];
        let end = [relationship.end.clone()];
        diagram.set_class(&start, &relationship.start_classes);
        diagram.set_class(&end, &relationship.end_classes);
        diagram.relationships.push(relationship);
        return Ok(index + 1);
    }
    parse_entity_statement(diagram, lines, index)
}

fn is_single_line_description(line: &str) -> bool {
    line.strip_prefix("accDescr")
        .is_some_and(|rest| rest.trim_start().starts_with(':'))
}

fn skip_until_block_end(lines: &[&str], index: usize) -> usize {
    lines[index..]
        .iter()
        .position(|line| line.contains('}'))
        .map_or(lines.len(), |offset| index + offset + 1)
}

fn parse_direction(line: &str) -> Option<Direction> {
    let value = line.strip_prefix("direction")?.trim();
    match value {
        "TB" => Some(Direction::TopBottom),
        "BT" => Some(Direction::BottomTop),
        "LR" => Some(Direction::LeftRight),
        "RL" => Some(Direction::RightLeft),
        _ => None,
    }
}

/// `classDef 이름들 선언`, `class 엔티티들 클래스들`, `style 엔티티들 선언`. 이 문장이 아니면 `false`
fn parse_style_statement(diagram: &mut ErDiagram, line: &str) -> Result<bool, DiagramError> {
    let Some((keyword, rest)) = line.split_once(char::is_whitespace) else {
        return Ok(false);
    };
    if !matches!(keyword, "classDef" | "class" | "style") {
        return Ok(false);
    }
    let (ids, values) = rest
        .trim()
        .split_once(char::is_whitespace)
        .ok_or_else(|| DiagramError::Syntax(format!("스타일 문장이 짧음: {line}")))?;
    let ids = split_list(ids);
    let values = split_list(values);
    match keyword {
        "classDef" => {
            for id in ids {
                let styles = diagram.class_styles.entry(id).or_default();
                styles.extend(values.iter().cloned());
            }
        }
        "class" => diagram.set_class(&ids, &values),
        _ => diagram.add_css_styles(&ids, &values),
    }
    Ok(true)
}

/// 쉼표로 나눈 목록(빈 칸은 버린다)
fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// 엔티티 이름 바로 뒤의 `:::클래스[,클래스]`
fn read_class_suffix(text: &str) -> (Vec<String>, &str) {
    let Some(rest) = text.strip_prefix(":::") else {
        return (Vec::new(), text);
    };
    let end = rest
        .char_indices()
        .find(|(_, character)| !is_name_character(*character) && *character != ',')
        .map_or(rest.len(), |(index, _)| index);
    (split_list(&rest[..end]), &rest[end..])
}

/// 차수 토큰. 낱말형은 원본 정규식처럼 낱말 경계가 필요하다
const CARDINALITY_TOKENS: [(&str, Cardinality); 22] = [
    ("one or zero", Cardinality::ZeroOrOne),
    ("one or more", Cardinality::OneOrMore),
    ("one or many", Cardinality::OneOrMore),
    ("zero or one", Cardinality::ZeroOrOne),
    ("zero or more", Cardinality::ZeroOrMore),
    ("zero or many", Cardinality::ZeroOrMore),
    ("only one", Cardinality::OnlyOne),
    ("many(0)", Cardinality::ZeroOrMore),
    ("many(1)", Cardinality::OneOrMore),
    ("many", Cardinality::ZeroOrMore),
    ("one", Cardinality::OnlyOne),
    ("1+", Cardinality::OneOrMore),
    ("0+", Cardinality::ZeroOrMore),
    ("|o", Cardinality::ZeroOrOne),
    ("}o", Cardinality::ZeroOrMore),
    ("}|", Cardinality::OneOrMore),
    ("||", Cardinality::OnlyOne),
    ("o|", Cardinality::ZeroOrOne),
    ("o{", Cardinality::ZeroOrMore),
    ("|{", Cardinality::OneOrMore),
    ("1", Cardinality::OnlyOne),
    ("u", Cardinality::MdParent),
];

/// 관계 종류 토큰과 식별 여부
const RELATION_TOKENS: [(&str, bool); 6] = [
    ("optionally to", false),
    ("to", true),
    ("--", true),
    ("..", false),
    (".-", false),
    ("-.", false),
];

/// `A ||--o{ B : role`. 관계 기호가 없으면 `None`(엔티티 문장)
fn parse_relationship(line: &str) -> Result<Option<Relationship>, DiagramError> {
    let (start, rest) = read_entity_name(line);
    let (start_classes, rest) = read_class_suffix(rest);
    let rest = rest.trim_start();
    let Some((start_cardinality, rest)) = read_cardinality(rest) else {
        return Ok(None);
    };
    let Some((is_identifying, rest)) = read_relation(rest.trim_start()) else {
        return Ok(None);
    };
    let (end_cardinality, rest) = read_cardinality(rest.trim_start())
        .ok_or_else(|| DiagramError::Syntax(format!("관계 차수가 없음: {line}")))?;
    let (end, rest) = read_entity_name(rest.trim_start());
    let (end_classes, rest) = read_class_suffix(rest);
    let role = rest
        .trim_start()
        .strip_prefix(':')
        .ok_or_else(|| DiagramError::Syntax(format!("관계 역할이 없음: {line}")))?;
    if start.is_empty() || end.is_empty() {
        return Err(DiagramError::Syntax(format!("엔티티 이름이 없음: {line}")));
    }
    Ok(Some(Relationship {
        start,
        end,
        role: unquote(role.trim()).to_string(),
        start_cardinality,
        end_cardinality,
        is_identifying,
        start_classes,
        end_classes,
    }))
}

/// 따옴표 이름이나 UNICODE_TEXT(낱말 글자·`-`·`*`·비ASCII) 이름과 남은 글자
fn read_entity_name(text: &str) -> (String, &str) {
    if let Some(quoted) = text.strip_prefix('"')
        && let Some(end) = quoted.find('"')
    {
        return (quoted[..end].to_string(), &quoted[end + 1..]);
    }
    let end = text
        .char_indices()
        .find(|(_, character)| !is_name_character(*character))
        .map_or(text.len(), |(index, _)| index);
    (text[..end].to_string(), &text[end..])
}

fn is_name_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '*') || !character.is_ascii()
}

fn read_cardinality(text: &str) -> Option<(Cardinality, &str)> {
    let lowered = text.to_ascii_lowercase();
    CARDINALITY_TOKENS
        .iter()
        .find(|(token, _)| lowered.starts_with(token) && has_token_boundary(text, token))
        .map(|(token, cardinality)| (*cardinality, &text[token.len()..]))
}

/// 낱말형 차수 뒤에는 낱말 글자가 올 수 없다(원본 `\b`)
fn has_token_boundary(text: &str, token: &str) -> bool {
    let is_word_token = token.chars().last().is_some_and(char::is_alphanumeric);
    let next = text[token.len()..].chars().next();
    !is_word_token || !next.is_some_and(|character| character.is_alphanumeric() || character == '_')
}

fn read_relation(text: &str) -> Option<(bool, &str)> {
    let lowered = text.to_ascii_lowercase();
    RELATION_TOKENS
        .iter()
        .find(|(token, _)| lowered.starts_with(token) && has_token_boundary(text, token))
        .map(|(token, is_identifying)| (*is_identifying, &text[token.len()..]))
}

fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(text)
}

/// `NAME`, `NAME[별칭]`, 그 뒤의 `{ 속성 }` 블록(여러 줄 가능)
fn parse_entity_statement(
    diagram: &mut ErDiagram,
    lines: &[&str],
    index: usize,
) -> Result<usize, DiagramError> {
    let line = lines[index];
    let (name, rest) = read_entity_name(line);
    if name.is_empty() {
        return Err(DiagramError::Syntax(format!("알 수 없는 문장: {line}")));
    }
    let (alias, rest) = read_alias(rest.trim_start());
    let (classes, rest) = read_class_suffix(rest.trim_start());
    let rest = rest.trim_start();
    let entity = diagram.add_entity(&name, &alias);
    let next_index = match rest.strip_prefix('{') {
        _ if rest.is_empty() => index + 1,
        Some(block_start) => {
            let (block, next_index) = collect_block(block_start, lines, index)?;
            entity.attributes.extend(parse_attributes(&block)?);
            next_index
        }
        None => return Err(DiagramError::Syntax(format!("알 수 없는 문장: {line}"))),
    };
    diagram.set_class(std::slice::from_ref(&name), &classes);
    Ok(next_index)
}

fn read_alias(text: &str) -> (String, &str) {
    let Some(inner) = text.strip_prefix('[') else {
        return (String::new(), text);
    };
    match inner.find(']') {
        Some(end) => (unquote(inner[..end].trim()).to_string(), &inner[end + 1..]),
        None => (String::new(), text),
    }
}

/// `{` 뒤부터 `}`까지 모은 블록 본문과 다음 줄 번호
fn collect_block(
    first: &str,
    lines: &[&str],
    index: usize,
) -> Result<(String, usize), DiagramError> {
    let mut block = String::new();
    let mut current = first;
    let mut line_index = index;
    loop {
        if let Some(end) = current.find('}') {
            block.push_str(&current[..end]);
            return Ok((block, line_index + 1));
        }
        block.push_str(current);
        block.push('\n');
        line_index += 1;
        current = lines
            .get(line_index)
            .ok_or_else(|| DiagramError::Syntax("속성 블록이 닫히지 않음".to_string()))?;
    }
}

#[derive(Clone, Debug, PartialEq)]
enum BlockToken {
    Word(String),
    Key(String),
    Comma,
    Comment(String),
}

/// 블록 본문의 토큰: 따옴표 설명, PK·FK·UK, 쉼표, 그 밖의 낱말(제네릭 `~T~` 포함)
fn tokenize_block(block: &str) -> Vec<BlockToken> {
    let mut tokens = Vec::new();
    let mut rest = block.trim_start();
    while !rest.is_empty() {
        let (token, next) = next_block_token(rest);
        tokens.push(token);
        rest = next.trim_start();
    }
    tokens
}

fn next_block_token(text: &str) -> (BlockToken, &str) {
    if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"').unwrap_or(quoted.len());
        let rest = quoted.get(end + 1..).unwrap_or("");
        return (BlockToken::Comment(quoted[..end].to_string()), rest);
    }
    if let Some(rest) = text.strip_prefix(',') {
        return (BlockToken::Comma, rest);
    }
    let end = word_end(text);
    let word = &text[..end];
    let token = if matches!(word.to_ascii_uppercase().as_str(), "PK" | "FK" | "UK") {
        BlockToken::Key(word.to_string())
    } else {
        BlockToken::Word(word.to_string())
    };
    (token, &text[end..])
}

/// 낱말 끝. `~`가 홀수 개면 같은 줄의 마지막 `~`와 그 뒤 낱말 글자까지 이어 붙인다
fn word_end(text: &str) -> usize {
    let is_delimiter =
        |character: char| character.is_whitespace() || matches!(character, ',' | '"');
    let end = text.find(is_delimiter).unwrap_or(text.len());
    if text[..end].matches('~').count().is_multiple_of(2) {
        return end;
    }
    let line_end = text.find('\n').unwrap_or(text.len());
    let last_tilde = text[..line_end].rfind('~').unwrap_or(end);
    let tail = text[last_tilde..line_end]
        .find(char::is_whitespace)
        .map_or(line_end, |offset| last_tilde + offset);
    tail.max(end)
}

/// attribute: 타입 이름 [키 목록] [설명]
fn parse_attributes(block: &str) -> Result<Vec<Attribute>, DiagramError> {
    let tokens = tokenize_block(block);
    let mut attributes = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let (attribute, next) = parse_attribute(&tokens, index)?;
        attributes.push(attribute);
        index = next;
    }
    Ok(attributes)
}

fn parse_attribute(
    tokens: &[BlockToken],
    index: usize,
) -> Result<(Attribute, usize), DiagramError> {
    let word = |position: usize| match tokens.get(position) {
        Some(BlockToken::Word(word)) => Ok(word.clone()),
        other => Err(DiagramError::Syntax(format!("속성 낱말이 아님: {other:?}"))),
    };
    let mut attribute = Attribute {
        attribute_type: word(index)?,
        name: word(index + 1)?,
        ..Attribute::default()
    };
    let mut position = index + 2;
    while let Some(BlockToken::Key(key)) = tokens.get(position) {
        attribute.keys.push(key.clone());
        position += 1;
        if tokens.get(position) != Some(&BlockToken::Comma) {
            break;
        }
        position += 1;
    }
    if let Some(BlockToken::Comment(comment)) = tokens.get(position) {
        attribute.comment = comment.clone();
        position += 1;
    }
    Ok((attribute, position))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 관계와_속성_블록을_읽는다() {
        let diagram = parse(
            "erDiagram\n    PROJECT ||--o{ TASK : contains\n    TASK {\n        string id PK\n        string project_id FK \"owner id\"\n    }",
        )
        .unwrap();
        assert_eq!(diagram.entities.len(), 2);
        let relationship = &diagram.relationships[0];
        assert_eq!(relationship.start_cardinality, Cardinality::OnlyOne);
        assert_eq!(relationship.end_cardinality, Cardinality::ZeroOrMore);
        assert!(relationship.is_identifying);
        let attributes = &diagram.entities[1].attributes;
        assert_eq!(attributes[0].keys, vec!["PK".to_string()]);
        assert_eq!(attributes[1].comment, "owner id");
    }

    #[test]
    fn 낱말_차수와_비식별_관계와_따옴표_역할을_읽는다() {
        let diagram =
            parse("erDiagram\n  A one or more optionally to zero or one B : \"listed in\"")
                .unwrap();
        let relationship = &diagram.relationships[0];
        assert_eq!(relationship.start_cardinality, Cardinality::OneOrMore);
        assert_eq!(relationship.end_cardinality, Cardinality::ZeroOrOne);
        assert!(!relationship.is_identifying);
        assert_eq!(relationship.role, "listed in");
    }

    #[test]
    fn 별칭과_여러_키와_제네릭_타입을_읽는다() {
        let diagram =
            parse("erDiagram\n  p[\"Person\"] {\n    List~int~ ids PK, FK \"keys\"\n  }").unwrap();
        let entity = &diagram.entities[0];
        assert_eq!(entity.alias, "Person");
        assert_eq!(entity.attributes[0].attribute_type, "List~int~");
        assert_eq!(entity.attributes[0].keys.len(), 2);
    }

    #[test]
    fn 클래스와_스타일_문장을_엔티티에_붙인다() {
        let diagram = parse(
            "---\ntitle: T\n---\nerDiagram\n  A:::hot ||--o{ B : r\n  C[\"Cee\"]:::cool {\n    string x\n  }\n  classDef hot fill:#f96,stroke:#333\n  class B,C cool\n  style B color:#900\n  style Z,A fill:#000",
        )
        .unwrap();
        let [a, b, c] = &diagram.entities[..] else {
            panic!("엔티티 세 개");
        };
        assert_eq!(a.classes, ["hot"]);
        assert_eq!(b.classes, ["cool"]);
        assert_eq!(c.classes, ["cool", "cool"]);
        assert_eq!(c.alias, "Cee");
        assert_eq!(b.styles, ["color:#900"]);
        // 없는 Z에서 멈추므로 A에는 붙지 않는다
        assert!(a.styles.is_empty());
        assert_eq!(diagram.entity_declarations(a), ["fill:#f96", "stroke:#333"]);
    }
}
