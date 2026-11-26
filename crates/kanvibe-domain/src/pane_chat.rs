//! pane 채팅의 입력 제한과 선택 화면 응답. 렌더러가 보낸 값을 그대로 믿지 않고 여기서 거른다.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::locale_order::compare_like_locale;

/// 채팅이 CLI에 대신 눌러 줄 수 있는 키
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
}

pub const MAX_PANE_CHAT_ATTACHMENTS: usize = 10;
pub const MAX_PANE_CHAT_SUGGESTIONS: usize = 20;
/// 입력창이 메뉴를 열 때 한 번에 받아 두고 자기 쪽에서 거르는 skill·명령 목록의 상한
pub const MAX_PANE_CHAT_CATALOG_SUGGESTIONS: usize = 512;
/// 숫자 키 하나로 고를 수 있는 선택지 번호의 상한. 두 자리 번호는 첫 숫자가 다른 선택지를 골라 버린다
pub const MAX_PANE_CHAT_PROMPT_OPTION_NUMBER: u8 = 9;
/// 직접 입력 줄에 넣을 수 있는 글자 수. 넣은 글이 그 줄에 그대로 보이는지 확인할 수 있는 길이로 둔다
pub const MAX_PANE_CHAT_PROMPT_TEXT_LENGTH: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneChatSuggestion {
    pub id: String,
    pub label: String,
    pub description: String,
    pub insert_text: String,
}

/// 거른 제안과 상한 때문에 잘렸는지 여부
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredPaneChatSuggestions {
    pub items: Vec<PaneChatSuggestion>,
    pub is_truncated: bool,
}

/// 이름이나 설명에 검색어가 들어 있는 제안을 이름순으로 `limit`개까지 남기고, 더 있었는지 알린다
pub fn filter_pane_chat_suggestions(
    suggestions: &[PaneChatSuggestion],
    query: &str,
    limit: usize,
) -> FilteredPaneChatSuggestions {
    let lowered_query = query.to_lowercase();
    let mut matches: Vec<PaneChatSuggestion> = suggestions
        .iter()
        .filter(|suggestion| {
            format!("{} {}", suggestion.label, suggestion.description)
                .to_lowercase()
                .contains(&lowered_query)
        })
        .cloned()
        .collect();
    matches.sort_by(|left, right| compare_like_locale(&left.label, &right.label));

    let is_truncated = matches.len() > limit;
    matches.truncate(limit);
    FilteredPaneChatSuggestions {
        items: matches,
        is_truncated,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneChatPromptStep {
    Previous,
    Next,
}

/// 선택 화면에 보낼 답. option은 번호 선택, text는 직접 입력 줄에 글을 넣어 고르기, step은 질문 사이 이동, cancel은 Esc다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PaneChatPromptAnswer {
    Option { number: u8 },
    Text { number: u8, text: String },
    Step { direction: PaneChatPromptStep },
    Cancel,
}

fn parse_option_number(value: Option<&Value>) -> Option<u8> {
    let number = value?.as_f64()?;
    let is_valid = number.fract() == 0.0
        && number >= 1.0
        && number <= f64::from(MAX_PANE_CHAT_PROMPT_OPTION_NUMBER);
    is_valid.then_some(number as u8)
}

/// 직접 입력 글은 앞뒤 공백을 걷고, 비었거나 너무 길거나 제어 문자가 섞이면 버린다
fn parse_prompt_text(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    let is_within_length = text.encode_utf16().count() <= MAX_PANE_CHAT_PROMPT_TEXT_LENGTH;
    let has_control_character = text
        .chars()
        .any(|character| character <= '\u{1f}' || character == '\u{7f}');
    (!text.is_empty() && is_within_length && !has_control_character).then(|| text.to_string())
}

/// 화면이 보낸 값이 선택 화면에 보낼 수 있는 답이면 그 답을, 아니면 `None`을 돌려준다
pub fn parse_pane_chat_prompt_answer(value: &Value) -> Option<PaneChatPromptAnswer> {
    let answer_type = value.get("type").and_then(Value::as_str);
    match answer_type {
        Some("cancel") => return Some(PaneChatPromptAnswer::Cancel),
        Some("step") => {
            let direction = serde_json::from_value(value.get("direction")?.clone()).ok()?;
            return Some(PaneChatPromptAnswer::Step { direction });
        }
        _ => {}
    }

    let number = parse_option_number(value.get("number"))?;
    match answer_type {
        Some("option") => Some(PaneChatPromptAnswer::Option { number }),
        Some("text") => {
            let text = parse_prompt_text(value.get("text"))?;
            Some(PaneChatPromptAnswer::Text { number, text })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn suggestion(label: &str, description: &str) -> PaneChatSuggestion {
        PaneChatSuggestion {
            id: format!("skill:{label}"),
            label: label.into(),
            description: description.into(),
            insert_text: format!("/{label}"),
        }
    }

    fn labels(result: &FilteredPaneChatSuggestions) -> Vec<&str> {
        result
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect()
    }

    #[test]
    fn keeps_matches_in_label_or_description_case_insensitively_by_name() {
        let suggestions = [
            suggestion("release", "Ship a build"),
            suggestion("commit", "Write a REVIEW-ready message"),
            suggestion("Review", "Check a diff"),
            suggestion("plan", "Outline work"),
        ];

        let result = filter_pane_chat_suggestions(&suggestions, "review", 20);

        assert_eq!(labels(&result), ["commit", "Review"]);
        assert!(!result.is_truncated);
    }

    #[test]
    fn empty_query_returns_everything_by_name() {
        let result =
            filter_pane_chat_suggestions(&[suggestion("b", ""), suggestion("a", "")], "", 20);

        assert_eq!(labels(&result), ["a", "b"]);
    }

    #[test]
    fn reports_truncation_only_beyond_the_limit() {
        let suggestions: Vec<PaneChatSuggestion> = (0..3)
            .map(|index| suggestion(&format!("skill-{index}"), ""))
            .collect();

        let exact = filter_pane_chat_suggestions(&suggestions, "skill", 3);
        assert!(!exact.is_truncated);
        assert_eq!(exact.items.len(), 3);

        let bounded = filter_pane_chat_suggestions(&suggestions, "skill", 2);
        assert_eq!(labels(&bounded), ["skill-0", "skill-1"]);
        assert!(bounded.is_truncated);
    }

    #[test]
    fn no_match_gives_an_empty_list() {
        let result = filter_pane_chat_suggestions(&[suggestion("plan", "")], "zzz", 20);

        assert_eq!(
            result,
            FilteredPaneChatSuggestions {
                items: vec![],
                is_truncated: false
            }
        );
    }

    #[test]
    fn prompt_answers_accept_only_valid_shapes() {
        assert_eq!(
            parse_pane_chat_prompt_answer(&json!({"type": "cancel"})),
            Some(PaneChatPromptAnswer::Cancel)
        );
        assert_eq!(
            parse_pane_chat_prompt_answer(&json!({"type": "option", "number": 2})),
            Some(PaneChatPromptAnswer::Option { number: 2 })
        );
        assert_eq!(
            parse_pane_chat_prompt_answer(&json!({"type": "text", "number": 3, "text": "  yes  "})),
            Some(PaneChatPromptAnswer::Text {
                number: 3,
                text: "yes".into()
            })
        );
        assert_eq!(
            parse_pane_chat_prompt_answer(&json!({"type": "option", "number": 10})),
            None
        );
        assert_eq!(
            parse_pane_chat_prompt_answer(&json!({"type": "option", "number": 1.5})),
            None
        );
        assert_eq!(
            parse_pane_chat_prompt_answer(&json!({"type": "text", "number": 1, "text": "a\nb"})),
            None
        );
        assert_eq!(
            parse_pane_chat_prompt_answer(&json!({"type": "step", "direction": "up"})),
            None
        );
        assert_eq!(parse_pane_chat_prompt_answer(&json!("cancel")), None);
    }
}
