//! sankey 문법(sankey.jison). 머리줄 `sankey-beta`·`sankey` 다음 줄부터 CSV 레코드 `출발,도착,값`을 읽는다.
//! 큰따옴표로 감싼 칸은 쉼표·줄바꿈을 담을 수 있고 `""`는 따옴표 하나다. 감싸지 않은 칸은 jison 어휘처럼
//! 출력 가능한 ASCII만 받는다.

use crate::DiagramError;

/// 레코드 한 줄이 만든 흐름. 노드는 처음 나온 순서대로 번호를 받는다
#[derive(Clone, Debug, PartialEq)]
pub struct SankeyFlow {
    pub source: usize,
    pub target: usize,
    pub value: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SankeyGraph {
    /// 노드 이름(= id). 처음 나온 순서
    pub nodes: Vec<String>,
    pub flows: Vec<SankeyFlow>,
}

impl SankeyGraph {
    fn find_or_create_node(&mut self, name: String) -> usize {
        if let Some(index) = self.nodes.iter().position(|node| *node == name) {
            return index;
        }
        self.nodes.push(name);
        self.nodes.len() - 1
    }
}

pub fn parse(body: &str) -> Result<SankeyGraph, DiagramError> {
    let text = prepare_text(body);
    let (header, records) = text.split_once('\n').unwrap_or((text.as_str(), ""));
    let keyword = header.trim().to_ascii_lowercase();
    if keyword != "sankey-beta" && keyword != "sankey" {
        return Err(DiagramError::Syntax(format!("sankey 머리줄: {header}")));
    }
    let mut graph = SankeyGraph::default();
    for record in split_records(records)? {
        let [source, target, value] = <[String; 3]>::try_from(record)
            .map_err(|fields| DiagramError::Syntax(format!("sankey 레코드 칸 수: {fields:?}")))?;
        let value = parse_float_prefix(&value)
            .ok_or_else(|| DiagramError::Syntax(format!("sankey 값: {value}")))?;
        let source = graph.find_or_create_node(source);
        let target = graph.find_or_create_node(target);
        graph.flows.push(SankeyFlow {
            source,
            target,
            value,
        });
    }
    Ok(graph)
}

/// `prepareTextForParsing`: 줄마다 앞뒤 공백을 지우고 빈 줄을 접는다. `%%` 주석 줄은 Mermaid 전처리처럼 버린다
fn prepare_text(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 레코드들을 칸 목록으로 나눈다. 따옴표 칸은 따옴표를 벗기고 `""`를 `"`로 바꾸며, 모든 칸은 앞뒤 공백을 지운다
fn split_records(text: &str) -> Result<Vec<Vec<String>>, DiagramError> {
    let mut records = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut characters = text.chars().peekable();
    loop {
        let field = match characters.peek() {
            Some('"') => {
                characters.next();
                read_escaped_field(&mut characters)?
            }
            _ => read_plain_field(&mut characters)?,
        };
        fields.push(field.trim().to_string());
        match characters.next() {
            Some(',') => {}
            Some('\n') => records.push(std::mem::take(&mut fields)),
            None => {
                records.push(fields);
                return Ok(records);
            }
            Some(other) => {
                return Err(DiagramError::Syntax(format!("sankey 칸 뒤 글자: {other}")));
            }
        }
    }
}

fn read_plain_field(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, DiagramError> {
    let mut field = String::new();
    while let Some(&character) = characters.peek() {
        if character == ',' || character == '\n' {
            break;
        }
        if !(' '..='~').contains(&character) || character == '"' {
            return Err(DiagramError::Syntax(format!(
                "sankey 칸에 쓸 수 없는 글자: {character}"
            )));
        }
        field.push(character);
        characters.next();
    }
    Ok(field)
}

fn read_escaped_field(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, DiagramError> {
    let mut field = String::new();
    loop {
        match characters.next() {
            Some('"') if characters.peek() == Some(&'"') => {
                characters.next();
                field.push('"');
            }
            Some('"') => return Ok(field),
            Some(character) if (' '..='~').contains(&character) || character == '\n' => {
                field.push(character)
            }
            Some(character) => {
                return Err(DiagramError::Syntax(format!(
                    "sankey 따옴표 칸에 쓸 수 없는 글자: {character}"
                )));
            }
            None => return Err(DiagramError::Syntax("sankey 따옴표가 닫히지 않음".into())),
        }
    }
}

/// JS `parseFloat`: 앞에서부터 읽을 수 있는 만큼의 십진수
fn parse_float_prefix(text: &str) -> Option<f64> {
    let text = text.trim_start();
    let mut end = 0;
    let mut has_digit = false;
    let mut has_dot = false;
    for (index, character) in text.char_indices() {
        let is_sign = index == 0 && (character == '+' || character == '-');
        if character.is_ascii_digit() {
            has_digit = true;
        } else if character == '.' && !has_dot {
            has_dot = true;
        } else if !is_sign {
            break;
        }
        end = index + character.len_utf8();
    }
    has_digit.then(|| text[..end].parse::<f64>().ok()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 레코드에서_노드를_처음_나온_순서로_만든다() {
        let graph = parse("sankey-beta\n\nA,B,10\nB, C ,2.5\n").unwrap();
        assert_eq!(graph.nodes, vec!["A", "B", "C"]);
        assert_eq!(
            graph.flows[1],
            SankeyFlow {
                source: 1,
                target: 2,
                value: 2.5
            }
        );
    }

    #[test]
    fn 따옴표_칸은_쉼표와_겹따옴표를_담는다() {
        let graph = parse("sankey\n%% 주석\n\"Revenue, total\",\"R&D \"\"core\"\"\",25").unwrap();
        assert_eq!(graph.nodes, vec!["Revenue, total", "R&D \"core\""]);
    }

    #[test]
    fn 칸이_셋이_아니거나_비ascii면_문법_오류다() {
        assert!(parse("sankey-beta\nA,B").is_err());
        assert!(parse("sankey-beta\n가,B,1").is_err());
    }
}
