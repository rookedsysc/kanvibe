//! treemap 문법(@mermaid-js/parser `treemap.langium`). 줄마다 들여쓰기 길이가 깊이이고, 따옴표 이름만 있으면
//! 구역, `: 값`이나 `, 값`이 붙으면 잎이다. `:::이름`으로 `classDef` 스타일을 붙이며, `title`은 제목이다.

use std::collections::HashMap;

use crate::DiagramError;

/// 문법이 읽은 한 줄(구역 또는 잎)
#[derive(Clone, Debug, PartialEq)]
pub struct TreemapRow {
    /// 들여쓰기 글자 수
    pub level: usize,
    pub name: String,
    /// 잎이면 값이 있다
    pub value: Option<f64>,
    pub class_name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreemapDocument {
    pub title: Option<String>,
    pub rows: Vec<TreemapRow>,
    /// `classDef` 이름 → `속성:값` 목록
    pub classes: HashMap<String, Vec<String>>,
}

pub fn parse(body: &str) -> Result<TreemapDocument, DiagramError> {
    let mut lines = body
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("%%"));
    let header = lines.next().unwrap_or("").trim();
    if header != "treemap" && header != "treemap-beta" {
        return Err(DiagramError::Syntax(format!("treemap 머리줄: {header}")));
    }
    let mut document = TreemapDocument::default();
    for line in lines {
        let content = line.trim_start_matches([' ', '\t']);
        let level = line.len() - content.len();
        let content = strip_comment(content).trim_end();
        if let Some(title) = content
            .strip_prefix("title")
            .filter(|title| title.is_empty() || title.starts_with([' ', '\t']))
        {
            document.title = Some(title.trim().to_string());
            continue;
        }
        if content.starts_with("accTitle") || content.starts_with("accDescr") {
            continue;
        }
        if let Some(definition) = content.strip_prefix("classDef") {
            add_class(&mut document.classes, definition)?;
            continue;
        }
        document.rows.push(parse_row(level, content)?);
    }
    Ok(document)
}

/// 줄 끝 `%%` 주석을 뗀다. 따옴표 이름 안의 `%%`는 그대로 둔다
fn strip_comment(content: &str) -> &str {
    let mut quote: Option<char> = None;
    for (index, character) in content.char_indices() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if content[index..].starts_with("%%") => return &content[..index],
            None => {}
        }
    }
    content
}

/// `classDef 이름 스타일`. 스타일의 `\,`는 쉼표로 남기고 `,`·`;`로 선언을 나눈다
fn add_class(
    classes: &mut HashMap<String, Vec<String>>,
    definition: &str,
) -> Result<(), DiagramError> {
    let definition = definition.trim_start();
    let name_end = definition
        .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .unwrap_or(definition.len());
    let name = &definition[..name_end];
    if name.len() < 2 || name.starts_with(|character: char| character.is_ascii_digit()) {
        return Err(DiagramError::Syntax(format!(
            "treemap classDef 이름: {name}"
        )));
    }
    let style_text = definition[name_end..].trim_start();
    let style_text = style_text.split(';').next().unwrap_or("");
    let declarations = style_text
        .replace("\\,", "\u{a7}\u{a7}\u{a7}")
        .replace(',', ";")
        .replace("\u{a7}\u{a7}\u{a7}", ",");
    let entry = classes.entry(name.to_string()).or_default();
    entry.extend(declarations.split(';').map(str::to_string));
    Ok(())
}

fn parse_row(level: usize, content: &str) -> Result<TreemapRow, DiagramError> {
    let invalid = || DiagramError::Syntax(format!("treemap 줄 형식: {content}"));
    let quote = content.chars().next().filter(|c| *c == '"' || *c == '\'');
    let quote = quote.ok_or_else(invalid)?;
    let name_end = content[1..].find(quote).ok_or_else(invalid)? + 1;
    let name = content[1..name_end].to_string();
    let rest = content[name_end + 1..].trim_start();
    let (value, rest) = match rest.strip_prefix(":::") {
        Some(_) => (None, rest),
        None => match rest.strip_prefix([':', ',']) {
            Some(after) => read_number(after.trim_start()).ok_or_else(invalid)?,
            None => (None, rest),
        },
    };
    let rest = rest.trim();
    let class_name = match rest.strip_prefix(":::") {
        Some(class_name) => Some(class_name.trim().to_string()),
        None if rest.is_empty() => None,
        None => return Err(invalid()),
    };
    Ok(TreemapRow {
        level,
        name,
        value,
        class_name,
    })
}

/// `NUMBER2`(`[0-9_.,]+`)를 읽어 쉼표를 지운 `parseFloat` 값과 나머지를 돌려준다
fn read_number(text: &str) -> Option<(Option<f64>, &str)> {
    let end = text
        .find(|character: char| !(character.is_ascii_digit() || "_.,".contains(character)))
        .unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    let digits: String = text[..end].chars().filter(|c| *c != ',').collect();
    let number_end = digits
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(digits.len());
    let value = digits[..number_end].parse::<f64>().ok()?;
    Some((Some(value), &text[end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 들여쓰기와_값으로_구역과_잎을_나눈다() {
        let document = parse("treemap-beta\n\"A\"\n    \"B\": 1,500\n  \"C\":::muted\n").unwrap();
        assert_eq!(document.rows[0].value, None);
        assert_eq!(document.rows[1].level, 4);
        assert_eq!(document.rows[1].value, Some(1500.0));
        assert_eq!(document.rows[2].class_name.as_deref(), Some("muted"));
    }

    #[test]
    fn 제목과_classdef를_읽는다() {
        let document = parse(
            "treemap\ntitle 매출\n\"A\": 900:::accent\nclassDef accent fill:#f96,stroke:#333;",
        )
        .unwrap();
        assert_eq!(document.title.as_deref(), Some("매출"));
        assert_eq!(document.rows[0].class_name.as_deref(), Some("accent"));
        assert_eq!(document.classes["accent"], vec!["fill:#f96", "stroke:#333"]);
    }
}
