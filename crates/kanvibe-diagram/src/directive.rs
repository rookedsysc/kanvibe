//! 정의 앞머리의 설정: YAML 앞머리(`---` … `---`)와 `%%{init: {...}}%%` 지시문.
//!
//! Mermaid `preprocessDiagram`처럼 앞머리의 `config`와 `title`을 읽고, `init`·`initialize` 지시문의 JSON
//! (작은따옴표는 큰따옴표로 바꾼다)을 그 위에 깊게 겹친다. 지시문 하나라도 JSON이 깨지면 Mermaid처럼 지시문
//! 설정을 모두 버린다. YAML은 앞머리에 쓰이는 만큼만 읽는다: 들여쓰기 매핑, JSON 스키마 스칼라
//! (`true`·`false`·`null`·숫자, 나머지는 글자), 따옴표 글자, JSON으로 읽히는 흐름 표기(`{…}`·`[…]`).

use serde_json::{Map, Value};

const FENCE: &str = "---";
const DIRECTIVE_OPEN: &str = "%%{";
const DIRECTIVE_CLOSE: &str = "}%%";

/// 정의에서 읽은 설정
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiagramConfig {
    /// 앞머리 `title`. 다이어그램의 `title` 문장이 있으면 그쪽이 이긴다
    pub title: Option<String>,
    /// 앞머리 `config` 위에 지시문을 겹친 설정 나무
    pub tree: Value,
}

impl DiagramConfig {
    /// 다이어그램 종류별 설정 묶음(`gitGraph`, `quadrantChart` 등)
    pub fn section(&self, key: &str) -> Option<&Map<String, Value>> {
        self.tree.get(key).and_then(Value::as_object)
    }
}

/// 앞머리와 지시문을 읽는다
pub fn read_config(definition: &str) -> DiagramConfig {
    let (front_matter, body) = split_front_matter(definition);
    let mut config = DiagramConfig {
        title: None,
        tree: Value::Object(Map::new()),
    };
    if let Some(Value::Object(front)) = front_matter.map(parse_yaml) {
        config.title = front.get("title").and_then(scalar_text);
        if let Some(tree) = front.get("config").filter(|value| value.is_object()) {
            config.tree = tree.clone();
        }
        // processFrontmatter는 앞머리 `displayMode`를 `gantt.displayMode` 설정으로 옮긴다
        if let Some(display_mode) = front.get("displayMode").and_then(scalar_text) {
            let gantt = serde_json::json!({ "gantt": { "displayMode": display_mode } });
            merge_deep(&mut config.tree, gantt);
        }
    }
    if let Some(directives) = read_init_directives(body) {
        merge_deep(&mut config.tree, directives);
    }
    config
}

/// 앞머리와 `%%{…}%%` 지시문을 걷어 낸 본문. 여러 줄 지시문도 지운다
pub fn diagram_body(definition: &str) -> String {
    let (_, body) = split_front_matter(definition);
    let mut text = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find(DIRECTIVE_OPEN) {
        text.push_str(&rest[..start]);
        rest = match rest[start..].find(DIRECTIVE_CLOSE) {
            Some(end) => &rest[start + end + DIRECTIVE_CLOSE.len()..],
            None => "",
        };
    }
    text.push_str(rest);
    text
}

/// `^-{3}\s*[\n\r](.*?)[\n\r]-{3}\s*[\n\r]+`: 맨 앞 `---` 줄과 다음 `---` 줄 사이
fn split_front_matter(definition: &str) -> (Option<&str>, &str) {
    let Some(after_open) = definition.strip_prefix(FENCE) else {
        return (None, definition);
    };
    let Some(first_break) = after_open.find(['\n', '\r']) else {
        return (None, definition);
    };
    if !after_open[..first_break].trim().is_empty() {
        return (None, definition);
    }
    let content_start = first_break + 1;
    let mut offset = content_start;
    for line in after_open[content_start..].split_inclusive('\n') {
        if line.trim_end().starts_with(FENCE) && line[FENCE.len()..].trim().is_empty() {
            let yaml = &after_open[content_start..offset];
            return (Some(yaml), &after_open[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, definition)
}

/// `init`·`initialize` 지시문 인자를 차례로 겹친다. 지시문이 없거나 JSON이 깨지면 `None`
fn read_init_directives(body: &str) -> Option<Value> {
    let mut merged = Value::Object(Map::new());
    let mut has_init = false;
    let mut rest = body;
    while let Some(start) = rest.find(DIRECTIVE_OPEN) {
        let inner = &rest[start + DIRECTIVE_OPEN.len()..];
        let end = inner.find(DIRECTIVE_CLOSE).unwrap_or(inner.len());
        rest = &inner[end..];
        let (kind, arguments) = split_directive(&inner[..end]);
        if kind != "init" && kind != "initialize" {
            continue;
        }
        let json = arguments.replace('\'', "\"");
        merge_deep(&mut merged, serde_json::from_str(&json).ok()?);
        has_init = true;
    }
    has_init.then_some(merged)
}

/// `종류: 인자` 또는 `종류 인자`
fn split_directive(inner: &str) -> (&str, &str) {
    let trimmed = inner.trim_start();
    let kind_end = trimmed
        .find(|character: char| !character.is_alphanumeric() && character != '_')
        .unwrap_or(trimmed.len());
    let (kind, rest) = trimmed.split_at(kind_end);
    let rest = rest.trim_start();
    (kind, rest.strip_prefix(':').unwrap_or(rest).trim())
}

/// 객체끼리는 열쇠마다 겹치고, 나머지는 덮어쓴다
fn merge_deep(target: &mut Value, source: Value) {
    match (target, source) {
        (Value::Object(target_map), Value::Object(source_map)) => {
            for (key, value) in source_map {
                match target_map.get_mut(&key) {
                    Some(existing) => merge_deep(existing, value),
                    None => {
                        target_map.insert(key, value);
                    }
                }
            }
        }
        (target, source) => *target = source,
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// 들여쓰기와 주석을 걷은 YAML 줄
struct YamlLine<'a> {
    indent: usize,
    content: &'a str,
}

fn parse_yaml(text: &str) -> Value {
    let lines: Vec<YamlLine> = text
        .lines()
        .filter_map(|line| {
            let content = strip_comment(line.trim_end()).trim_end();
            let indent = content.len() - content.trim_start().len();
            let content = content.trim_start();
            (!content.is_empty()).then_some(YamlLine { indent, content })
        })
        .collect();
    let mut index = 0;
    let indent = lines.first().map_or(0, |line| line.indent);
    Value::Object(parse_mapping(&lines, &mut index, indent))
}

/// 따옴표 밖의 ` #`부터 줄 끝까지(줄 머리의 `#` 포함)를 주석으로 버린다
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut previous = ' ';
    for (index, character) in line.char_indices() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == '#' && previous.is_whitespace() => return &line[..index],
            None => {}
        }
        previous = character;
    }
    line
}

/// 같은 들여쓰기의 `열쇠: 값` 줄들. 값이 비면 더 깊은 다음 줄들이 그 값이다
fn parse_mapping(lines: &[YamlLine], index: &mut usize, indent: usize) -> Map<String, Value> {
    let mut map = Map::new();
    while let Some(line) = lines.get(*index) {
        if line.indent < indent {
            break;
        }
        *index += 1;
        if line.indent > indent {
            continue;
        }
        let Some((key, value)) = split_key(line.content) else {
            continue;
        };
        let value = match lines.get(*index) {
            Some(next) if value.is_empty() && next.indent > indent => {
                Value::Object(parse_mapping(lines, index, next.indent))
            }
            _ => parse_scalar(value),
        };
        map.insert(unquote(key), value);
    }
    map
}

/// 첫 `: `(또는 줄 끝 `:`)에서 열쇠와 값을 가른다
fn split_key(content: &str) -> Option<(&str, &str)> {
    if let Some(key) = content.strip_suffix(':') {
        return Some((key.trim(), ""));
    }
    content
        .split_once(": ")
        .map(|(key, value)| (key.trim(), value.trim()))
}

fn unquote(text: &str) -> String {
    let is_quoted = text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')));
    if !is_quoted {
        return text.to_string();
    }
    let inner = &text[1..text.len() - 1];
    if text.starts_with('\'') {
        return inner.replace("''", "'");
    }
    serde_json::from_str::<String>(text).unwrap_or_else(|_| inner.to_string())
}

/// js-yaml JSON_SCHEMA 스칼라
fn parse_scalar(text: &str) -> Value {
    match text {
        "" | "null" => return Value::Null,
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if text.starts_with(['"', '\'']) {
        return Value::String(unquote(text));
    }
    let is_numeric = text
        .trim_start_matches('-')
        .starts_with(|character: char| character.is_ascii_digit());
    let is_flow = text.starts_with(['{', '[']);
    let parsed = (is_numeric || is_flow)
        .then(|| serde_json::from_str::<Value>(text).ok())
        .flatten();
    parsed.unwrap_or_else(|| Value::String(text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 앞머리_설정과_제목을_읽고_본문을_남긴다() {
        let definition = "---\ntitle: Flow  # 주석\nconfig:\n  gitGraph:\n    showBranches: false\n    mainBranchName: 'trunk'\n    mainBranchOrder: 2\n---\ngitGraph\n  commit";
        let config = read_config(definition);
        assert_eq!(config.title.as_deref(), Some("Flow"));
        let section = config.section("gitGraph").unwrap();
        assert_eq!(section["showBranches"], json!(false));
        assert_eq!(section["mainBranchName"], json!("trunk"));
        assert_eq!(section["mainBranchOrder"], json!(2));
        assert_eq!(diagram_body(definition), "gitGraph\n  commit");
    }

    #[test]
    fn 지시문은_앞머리_위에_깊게_겹친다() {
        let definition = "---\nconfig:\n  gitGraph:\n    showBranches: false\n    rotateCommitLabel: false\n---\n%%{init: { 'gitGraph': {'showBranches': true}}}%%\n%%{\n  initialize: {\"quadrantChart\": {\"chartWidth\": 400}}\n}%%\ngitGraph";
        let config = read_config(definition);
        let git = config.section("gitGraph").unwrap();
        assert_eq!(git["showBranches"], json!(true));
        assert_eq!(git["rotateCommitLabel"], json!(false));
        assert_eq!(
            config.section("quadrantChart").unwrap()["chartWidth"],
            json!(400)
        );
        assert_eq!(diagram_body(definition).trim(), "gitGraph");
    }

    #[test]
    fn 깨진_지시문은_설정을_모두_버린다() {
        let config = read_config("%%{init: {'a': 1}}%%\n%%{init: {broken}}%%\npie");
        assert_eq!(config.tree, json!({}));
    }

    #[test]
    fn 앞머리가_닫히지_않으면_본문이다() {
        assert_eq!(diagram_body("---\ntitle: x\npie"), "---\ntitle: x\npie");
    }
}
