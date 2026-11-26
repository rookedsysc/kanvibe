//! venn-beta 문법(venn.jison)과 vennDB.ts의 값 정리.
//!
//! `set 이름["라벨"]:크기`, `union 이름, 이름["라벨"]:크기`, `text 집합들 글자이름["라벨"]`, `style 집합들
//! 키:값, …`, `title`을 읽는다. 집합 이름 목록은 늘 사전순으로 정렬하고, 크기를 적지 않으면
//! `10 / (집합 수)²`다. `set`·`union` 다음 줄에서 들여 쓴 `text 글자이름`은 바로 앞 영역에 붙는다.

use crate::DiagramError;
use crate::directive::diagram_body;
use crate::sanitize::sanitize_text;

/// 집합 하나 또는 여러 집합의 겹침
#[derive(Clone, Debug, PartialEq)]
pub struct Subset {
    pub sets: Vec<String>,
    pub size: f64,
    pub label: Option<String>,
}

/// 영역 안에 놓는 글자 상자
#[derive(Clone, Debug, PartialEq)]
pub struct TextNode {
    pub sets: Vec<String>,
    pub id: String,
    pub label: Option<String>,
}

/// `style` 문장 하나
#[derive(Clone, Debug, PartialEq)]
pub struct StyleEntry {
    pub targets: Vec<String>,
    pub styles: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VennChart {
    pub title: String,
    pub subsets: Vec<Subset>,
    pub text_nodes: Vec<TextNode>,
    pub styles: Vec<StyleEntry>,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    /// 이름·숫자·색 같은 맨 낱말
    Word(String),
    Quoted(String),
    Label(String),
    Colon,
    Comma,
}

/// 문장을 읽는 동안의 상태: 들여 쓴 `text`가 붙을 바로 앞 영역
#[derive(Default)]
struct ReaderState {
    current_sets: Option<Vec<String>>,
    is_indent_mode: bool,
}

pub fn parse(definition: &str) -> Result<VennChart, DiagramError> {
    let body = diagram_body(definition);
    let mut lines = body
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("%%"));
    let header = lines
        .next()
        .ok_or_else(|| DiagramError::Syntax("빈 정의".to_string()))?;
    if header.trim() != "venn-beta" {
        return Err(DiagramError::Syntax("venn-beta 머리줄".to_string()));
    }
    let mut chart = VennChart::default();
    let mut state = ReaderState::default();
    for line in lines {
        let is_indented = line.starts_with([' ', '\t']);
        if !is_indented {
            state.is_indent_mode = false;
        }
        read_statement(line.trim(), is_indented, &mut chart, &mut state)?;
    }
    Ok(chart)
}

fn read_statement(
    line: &str,
    is_indented: bool,
    chart: &mut VennChart,
    state: &mut ReaderState,
) -> Result<(), DiagramError> {
    let (keyword, rest) = line
        .split_once([' ', '\t'])
        .map_or((line, ""), |(keyword, rest)| (keyword, rest));
    match keyword.to_ascii_lowercase().as_str() {
        "title" => chart.title = sanitize_text(rest.split(['#', ';']).next().unwrap_or("").trim()),
        "set" | "union" => {
            let subset = read_subset(&tokenize(rest)?, keyword.eq_ignore_ascii_case("union"))?;
            state.current_sets = Some(subset.sets.clone());
            state.is_indent_mode = true;
            chart.subsets.push(subset);
        }
        "text" => {
            let tokens = tokenize(rest)?;
            let node = if is_indented && state.is_indent_mode {
                let sets = state.current_sets.clone().ok_or_else(|| {
                    DiagramError::Syntax("text에 앞선 집합이 없습니다".to_string())
                })?;
                read_text_tail(sets, &tokens)?
            } else {
                read_text_node(&tokens)?
            };
            chart.text_nodes.push(node);
        }
        "style" => chart.styles.push(read_style(&tokenize(rest)?)?),
        _ if keyword.starts_with("accTitle") || keyword.starts_with("accDescr") => {}
        other => return Err(DiagramError::Syntax(format!("venn 문장: {other}"))),
    }
    Ok(())
}

/// normalizeText: 앞뒤 공백을 지우고 감싼 큰따옴표를 벗긴다
fn normalize_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

fn tokenize(text: &str) -> Result<Vec<Token>, DiagramError> {
    let mut tokens = Vec::new();
    let mut rest = text.trim_start();
    while let Some(character) = rest.chars().next() {
        let consumed = match character {
            ':' => {
                tokens.push(Token::Colon);
                1
            }
            ',' => {
                tokens.push(Token::Comma);
                1
            }
            '"' => {
                let end = rest[1..]
                    .find('"')
                    .ok_or_else(|| DiagramError::Syntax("닫히지 않은 따옴표".to_string()))?;
                tokens.push(Token::Quoted(rest[1..=end].to_string()));
                end + 2
            }
            '[' => {
                let end = rest
                    .find(']')
                    .ok_or_else(|| DiagramError::Syntax("닫히지 않은 라벨".to_string()))?;
                tokens.push(Token::Label(bracket_label(&rest[..=end])));
                end + 1
            }
            _ => {
                let end = word_end(rest);
                tokens.push(Token::Word(rest[..end].to_string()));
                end
            }
        };
        rest = rest[consumed..].trim_start();
    }
    Ok(tokens)
}

/// `["…"]`는 안쪽 그대로, `[…]`는 앞뒤 공백을 지운다
fn bracket_label(token: &str) -> String {
    let inner = &token[1..token.len() - 1];
    match inner
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
    {
        Some(quoted) => quoted.to_string(),
        None => inner.trim().to_string(),
    }
}

/// 낱말 끝. `rgb(…)`·`rgba(…)`는 괄호까지 한 낱말이다
fn word_end(text: &str) -> usize {
    if text.starts_with("rgb")
        && let Some(close) = text.find(')')
    {
        return close + 1;
    }
    text.find([' ', '\t', ':', ',', '[', '"'])
        .unwrap_or(text.len())
}

fn name_of(token: &Token) -> Option<String> {
    match token {
        Token::Word(word) => Some(word.clone()),
        Token::Quoted(text) => Some(text.clone()),
        _ => None,
    }
}

/// 쉼표로 이은 집합 이름들과 그 뒤 위치
fn read_identifier_list(tokens: &[Token]) -> Result<(Vec<String>, usize), DiagramError> {
    let mut names = Vec::new();
    let mut position = 0;
    loop {
        let name = tokens
            .get(position)
            .and_then(name_of)
            .ok_or_else(|| DiagramError::Syntax("venn 집합 이름 자리".to_string()))?;
        names.push(normalize_text(&name));
        position += 1;
        if tokens.get(position) != Some(&Token::Comma) {
            return Ok((names, position));
        }
        position += 1;
    }
}

/// addSubsetData: `[라벨]`과 `:크기`는 둘 다 생략할 수 있다
fn read_subset(tokens: &[Token], is_union: bool) -> Result<Subset, DiagramError> {
    let (mut sets, mut position) = read_identifier_list(tokens)?;
    if is_union && sets.len() < 2 {
        return Err(DiagramError::Syntax(
            "union requires multiple identifiers".to_string(),
        ));
    }
    let mut label = None;
    if let Some(Token::Label(text)) = tokens.get(position) {
        label = Some(normalize_text(text));
        position += 1;
    }
    let mut size = None;
    if tokens.get(position) == Some(&Token::Colon) {
        let value = tokens
            .get(position + 1)
            .and_then(name_of)
            .unwrap_or_default();
        size = Some(
            value
                .parse::<f64>()
                .map_err(|_| DiagramError::Syntax(format!("venn 크기: {value}")))?,
        );
    }
    let count = sets.len() as f64;
    sets.sort();
    Ok(Subset {
        sets,
        size: size.unwrap_or(10.0 / (count * count)),
        label,
    })
}

fn read_text_tail(sets: Vec<String>, tokens: &[Token]) -> Result<TextNode, DiagramError> {
    let id = tokens
        .first()
        .and_then(name_of)
        .ok_or_else(|| DiagramError::Syntax("venn 글자 이름 자리".to_string()))?;
    let label = match tokens.get(1) {
        Some(Token::Label(text)) => Some(normalize_text(text)),
        _ => None,
    };
    Ok(TextNode {
        sets,
        id: normalize_text(&id),
        label,
    })
}

fn read_text_node(tokens: &[Token]) -> Result<TextNode, DiagramError> {
    let (mut sets, position) = read_identifier_list(tokens)?;
    sets.sort();
    read_text_tail(sets, &tokens[position..])
}

/// `키:값` 쌍을 쉼표로 잇는다. 맨 낱말 값 여러 개는 공백으로 이어 붙인다
fn read_style(tokens: &[Token]) -> Result<StyleEntry, DiagramError> {
    let (mut targets, mut position) = read_identifier_list(tokens)?;
    targets.sort();
    let mut styles = Vec::new();
    while let Some(Token::Word(key)) = tokens.get(position) {
        if tokens.get(position + 1) != Some(&Token::Colon) {
            return Err(DiagramError::Syntax(format!("venn 스타일: {key}")));
        }
        position += 2;
        let mut words = Vec::new();
        while let Some(token) = tokens.get(position) {
            match token {
                Token::Word(word) => words.push(word.clone()),
                Token::Quoted(text) => words.push(text.clone()),
                _ => break,
            }
            position += 1;
        }
        styles.push((key.clone(), normalize_text(&words.join(" "))));
        if tokens.get(position) == Some(&Token::Comma) {
            position += 1;
        }
    }
    Ok(StyleEntry { targets, styles })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 집합_겹침_스타일을_읽는다() {
        let chart = parse(
            "venn-beta\n  title \"Team\"\n  set B[\"Back\"]:20\n  set A\n  union B,A[\"Both\"]:6\n  style A fill:#ff9900, stroke-width:3",
        )
        .unwrap();
        assert_eq!(chart.title, "\"Team\"");
        assert_eq!(chart.subsets[0].label.as_deref(), Some("Back"));
        assert_eq!(chart.subsets[1].size, 10.0);
        assert_eq!(
            chart.subsets[2].sets,
            vec!["A".to_string(), "B".to_string()]
        );
        assert_eq!(chart.subsets[2].size, 6.0);
        assert_eq!(
            chart.styles[0].styles,
            vec![
                ("fill".to_string(), "#ff9900".to_string()),
                ("stroke-width".to_string(), "3".to_string()),
            ]
        );
    }

    #[test]
    fn 들여_쓴_text는_앞선_영역에_붙는다() {
        let chart = parse("venn-beta\nset A\n  text a1[\"First\"]\ntext A a2").unwrap();
        assert_eq!(chart.text_nodes[0].sets, vec!["A".to_string()]);
        assert_eq!(chart.text_nodes[0].label.as_deref(), Some("First"));
        assert_eq!(chart.text_nodes[1].id, "a2");
    }
}
