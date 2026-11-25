//! kanban 문법(kanban.jison, mindmap과 같은 줄 문법). 첫 노드의 들여쓰기가 열(구역) 깊이이고, 그보다 깊은
//! 노드는 깊이와 상관없이 바로 앞 열의 카드가 된다. 열보다 얕은 노드는 오류다. 노드는 `id[설명]`, `[설명]`,
//! `id` 꼴이고 `@{ ticket: …, assigned: …, priority: … }`로 카드 정보를 붙인다. `:::클래스`·`::icon()` 줄은
//! 그림에 쓰이지 않아 버린다.

use crate::DiagramError;

/// 카드 정보(`@{ … }`)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KanbanCard {
    pub label: String,
    pub ticket: String,
    pub assigned: String,
    pub priority: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KanbanColumn {
    pub label: String,
    pub cards: Vec<KanbanCard>,
}

/// 노드를 여는 괄호. 긴 것부터 맞춘다
const OPENERS: [&str; 8] = ["((", "))", "(-", "-)", "{{", "(", ")", "["];
const CLOSERS: [&str; 7] = ["))", "}}", "(-", "-)", ")", "]", "(("];

pub fn parse(body: &str) -> Result<Vec<KanbanColumn>, DiagramError> {
    let mut lines = body
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with("%%"));
    let header = lines.next().unwrap_or("").trim();
    if header.split_whitespace().next() != Some("kanban") {
        return Err(DiagramError::Syntax(format!("kanban 머리줄: {header}")));
    }
    let mut columns: Vec<KanbanColumn> = Vec::new();
    let mut column_level: Option<usize> = None;
    // kanbanDb `getSection`은 열보다 얕은 노드가 이미 있으면 그다음 노드를 더할 때 오류를 낸다
    let mut shallow_label: Option<String> = None;
    while let Some(line) = lines.next() {
        let content = line.trim_start();
        if content.starts_with(":::") || content.starts_with("::icon(") {
            continue;
        }
        let level = line.len() - content.len();
        let node = join_shape_data(content.trim_end(), &mut lines);
        let card = parse_node(&node)?;
        if let Some(label) = &shallow_label {
            return Err(DiagramError::Syntax(format!(
                "Items without section detected, found section (\"{label}\")"
            )));
        }
        let section_level = *column_level.get_or_insert(level);
        if level < section_level {
            shallow_label = Some(card.label.clone());
        }
        match columns.last_mut() {
            Some(column) if level != section_level => column.cards.push(card),
            _ => columns.push(KanbanColumn {
                label: card.label,
                cards: Vec::new(),
            }),
        }
    }
    Ok(columns)
}

/// 닫히지 않은 `@{`는 `}`가 나오는 줄까지 이어 붙인다(여러 줄 YAML 블록)
fn join_shape_data<'a>(content: &str, lines: &mut impl Iterator<Item = &'a str>) -> String {
    let mut node = content.to_string();
    let Some(open) = node.find("@{") else {
        return node;
    };
    while !node[open..].contains('}') {
        let Some(next) = lines.next() else {
            break;
        };
        node.push('\n');
        node.push_str(next);
    }
    node
}

/// 한 줄의 노드: 아이디·설명과 뒤따르는 `@{ … }`
fn parse_node(content: &str) -> Result<KanbanCard, DiagramError> {
    let id_end = content
        .find(['(', '[', ')', '{', '}', '@'])
        .unwrap_or(content.len());
    let id = &content[..id_end];
    let mut rest = &content[id_end..];
    let mut label = id.to_string();
    if let Some(opener) = OPENERS.iter().find(|opener| rest.starts_with(**opener)) {
        let (description, after) = read_description(&rest[opener.len()..])
            .ok_or_else(|| DiagramError::Syntax(format!("kanban 노드 형식: {content}")))?;
        label = description;
        rest = after;
    }
    let mut card = KanbanCard {
        label,
        ..KanbanCard::default()
    };
    let rest = rest.trim_start();
    if let Some(shape_data) = rest.strip_prefix("@{") {
        let end = shape_data.rfind('}').unwrap_or(shape_data.len());
        apply_shape_data(&mut card, &shape_data[..end]);
    }
    Ok(card)
}

/// 괄호 안 설명과 닫는 괄호 뒤 나머지. `"…"`·`` "`…`" `` 따옴표 설명은 따옴표를 벗긴다
fn read_description(text: &str) -> Option<(String, &str)> {
    let (description, rest) = if let Some(quoted) = text.strip_prefix("\"`") {
        let end = quoted.find("`\"")?;
        (&quoted[..end], &quoted[end + 2..])
    } else if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], &quoted[end + 1..])
    } else {
        let end = text.find([')', ']', '(', '}'])?;
        (&text[..end], &text[end..])
    };
    let closer = CLOSERS.iter().find(|closer| rest.starts_with(**closer))?;
    Some((description.to_string(), &rest[closer.len()..]))
}

/// `@{ … }` 안의 YAML에서 카드가 쓰는 키를 읽는다. 한 줄이면 흐름 매핑(`키: 값, …`), 여러 줄이면 줄마다
/// `키: 값`인 블록 매핑이다
fn apply_shape_data(card: &mut KanbanCard, text: &str) {
    let entries = if text.contains('\n') {
        text.lines().map(str::to_string).collect()
    } else {
        split_entries(text)
    };
    for entry in entries {
        let Some((key, value)) = entry.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        match key.trim() {
            "label" => card.label = value,
            "ticket" => card.ticket = value,
            "assigned" => card.assigned = value,
            "priority" => card.priority = Some(value),
            _ => {}
        }
    }
}

/// 따옴표 밖의 쉼표로 나눈다
fn split_entries(text: &str) -> Vec<String> {
    let mut entries = vec![String::new()];
    let mut quote: Option<char> = None;
    for character in text.chars() {
        match (quote, character) {
            (None, ',') => entries.push(String::new()),
            (None, '\'' | '"') => quote = Some(character),
            (Some(open), _) if open == character => quote = None,
            _ => {}
        }
        let is_separator = quote.is_none() && character == ',';
        if let Some(entry) = entries.last_mut().filter(|_| !is_separator) {
            entry.push(character);
        }
    }
    entries
}

fn unquote(value: &str) -> String {
    for quote in ['\'', '"'] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.to_string();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 첫_노드_깊이가_열이고_더_깊은_노드는_카드다() {
        let columns =
            parse("kanban\n  Todo\n    [Write docs]\n      docs[Nested card]\n  id9[Done]\n")
                .unwrap();
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].label, "Todo");
        assert_eq!(columns[0].cards[1].label, "Nested card");
        assert_eq!(columns[1].label, "Done");
    }

    #[test]
    fn 카드_정보를_읽는다() {
        let columns = parse(
            "kanban\n  c[Col]\n    id3[Fix bug]@{ ticket: MC-2038, assigned: 'K.Sveidqvist', priority: 'Very High' }",
        )
        .unwrap();
        let card = &columns[0].cards[0];
        assert_eq!(card.ticket, "MC-2038");
        assert_eq!(card.assigned, "K.Sveidqvist");
        assert_eq!(card.priority.as_deref(), Some("Very High"));
    }

    #[test]
    fn 열보다_얕은_노드_다음에_노드가_오면_오류다() {
        // Mermaid처럼 얕은 노드는 앞 열의 카드가 되고, 그 뒤 노드를 더할 때 오류가 난다
        let columns = parse("kanban\n    Todo\n  Card").unwrap();
        assert_eq!(columns[0].cards[0].label, "Card");
        assert!(parse("kanban\n    Todo\n  Card\n    Done").is_err());
    }

    #[test]
    fn 여러_줄_카드_정보를_읽는다() {
        let columns = parse(
            "kanban\n  c[Col]\n    t2[Review]@{\n      ticket: KV-102\n      assigned: jun, kim\n      priority: 'Very Low'\n    }\n  d[Done]",
        )
        .unwrap();
        let card = &columns[0].cards[0];
        assert_eq!(card.ticket, "KV-102");
        assert_eq!(card.assigned, "jun, kim");
        assert_eq!(card.priority.as_deref(), Some("Very Low"));
        assert_eq!(columns[1].label, "Done");
    }
}
