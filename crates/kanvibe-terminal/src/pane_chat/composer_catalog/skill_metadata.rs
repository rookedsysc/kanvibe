//! SKILL.md·명령 파일 앞머리(front matter)에서 이름과 설명을 읽는다. TS 정규식이 쓰는 JS 공백·줄 끝 규칙을 그대로 따른다.

use std::sync::LazyLock;

use kanvibe_ai::javascript::{JS_DOT, JS_WHITESPACE_CLASS, js_trim, utf16_prefix};
use regex::Regex;

const MAX_DESCRIPTION_UTF16_LENGTH: usize = 512;
/// JS `m` 플래그의 `^`·`$`가 기준으로 삼는 줄 끝 글자
const JS_LINE_TERMINATOR: &str = r"[\n\r\x{2028}\x{2029}]";

static FRONT_MATTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\A---\r?\n(.*?)\r?\n---(?:\r?\n|\z)").unwrap());
static NOT_USER_INVOCABLE: LazyLock<Regex> = LazyLock::new(|| {
    let pattern = format!(
        r"(?:\A|{JS_LINE_TERMINATOR})user-invocable:[{JS_WHITESPACE_CLASS}]*false[{JS_WHITESPACE_CLASS}]*(?:\z|{JS_LINE_TERMINATOR})"
    );
    Regex::new(&pattern).unwrap()
});
static NAME_FIELD: LazyLock<Regex> = LazyLock::new(|| field_pattern("name"));
static DESCRIPTION_FIELD: LazyLock<Regex> = LazyLock::new(|| field_pattern("description"));
static FOLDED_DESCRIPTION: LazyLock<Regex> = LazyLock::new(|| {
    let pattern = format!(
        r"(?:\A|{JS_LINE_TERMINATOR})description:[{JS_WHITESPACE_CLASS}]*[>|][{JS_WHITESPACE_CLASS}]*\n((?:[ \t]+{JS_DOT}*\n?)*)"
    );
    Regex::new(&pattern).unwrap()
});
static WHITESPACE_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!("[{JS_WHITESPACE_CLASS}]+")).unwrap());
static SKILL_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A[A-Za-z0-9][A-Za-z0-9_.:-]*\z").unwrap());

/// TS `^${key}:\s*(.*)$`(m 플래그)
fn field_pattern(key: &str) -> Regex {
    let pattern = format!(
        r"(?:\A|{JS_LINE_TERMINATOR}){key}:[{JS_WHITESPACE_CLASS}]*({JS_DOT}*)(?:\z|{JS_LINE_TERMINATOR})"
    );
    Regex::new(&pattern).unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SkillMetadata {
    pub name: String,
    pub description: String,
}

/// 앞뒤 공백을 떼고, 같은 따옴표로 감싼 값이면 따옴표를 벗긴다
fn read_field(header: Option<&str>, pattern: &Regex) -> Option<String> {
    let captured = pattern.captures(header?)?.get(1)?.as_str();
    let trimmed = js_trim(captured);
    let unquoted = match (trimmed.chars().next(), trimmed.chars().last()) {
        (Some(first @ ('\'' | '"')), Some(last)) if trimmed.len() >= 2 && first == last => {
            &trimmed[1..trimmed.len() - 1]
        }
        _ => trimmed,
    };
    Some(unquoted.to_string())
}

/// YAML `>`·`|` 블록 설명은 들여쓴 줄을 이어 붙여 한 줄로 만든다
fn read_block_description(header: Option<&str>) -> String {
    let block = header
        .and_then(|header| FOLDED_DESCRIPTION.captures(header))
        .and_then(|captures| captures.get(1))
        .map_or("", |block| block.as_str());
    let collapsed = WHITESPACE_RUN.replace_all(js_trim(block), " ");
    utf16_prefix(&collapsed, MAX_DESCRIPTION_UTF16_LENGTH).to_string()
}

/// 설명 필드가 없으면 본문에서 제목이 아닌 첫 줄을 설명으로 쓴다
fn first_body_line(content: &str) -> &str {
    content
        .split('\n')
        .find(|line| !js_trim(line).is_empty() && !line.starts_with('#'))
        .unwrap_or("")
}

pub(super) fn read_skill_metadata(content: &str, fallback_name: &str) -> Option<SkillMetadata> {
    if js_trim(content).is_empty() {
        return None;
    }
    let header = FRONT_MATTER
        .captures(content)
        .and_then(|captures| captures.get(1))
        .map(|header| header.as_str());
    if header.is_none() && content.starts_with("---") {
        return None;
    }
    if header.is_some_and(|header| !header.is_empty() && NOT_USER_INVOCABLE.is_match(header)) {
        return None;
    }
    let name = read_field(header, &NAME_FIELD).unwrap_or_else(|| fallback_name.to_string());
    if !SKILL_NAME.is_match(&name) {
        return None;
    }
    let description = match read_field(header, &DESCRIPTION_FIELD) {
        Some(field) if field == ">" || field == "|" => read_block_description(header),
        Some(field) => utf16_prefix(&field, MAX_DESCRIPTION_UTF16_LENGTH).to_string(),
        None => utf16_prefix(first_body_line(content), MAX_DESCRIPTION_UTF16_LENGTH).to_string(),
    };
    Some(SkillMetadata { name, description })
}
