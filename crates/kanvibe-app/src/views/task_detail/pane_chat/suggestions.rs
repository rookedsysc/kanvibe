//! 입력창 자동 완성(TS `PaneChatComposer`의 메뉴 상태). 커서 앞 낱말이 `$`(skill)·`/`(명령)·`@`(파일)로 시작하면 메뉴를 연다.
//! skill·명령 목록은 메뉴를 열 때 한 번 받아 두고 화면에서 거르며, 받은 목록이 잘렸거나 파일을 찾을 때만 글자마다 묻는다.

use std::collections::{HashMap, HashSet};

use kanvibe_domain::pane_chat::{
    MAX_PANE_CHAT_SUGGESTIONS, PaneChatSuggestion, filter_pane_chat_suggestions,
};

use super::client::SuggestionKind;

/// 커서 앞에서 찾은 자동 완성 낱말. `start..end`가 고르면 바꿀 자리다(바이트 위치)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub kind: SuggestionKind,
    pub query: String,
    pub start: usize,
    pub end: usize,
}

/// 커서 앞 낱말(앞이 줄 처음이거나 공백)이 `$`·`/`·`@`로 시작하면 그 종류와 검색어
pub fn completion_at(text: &str, caret: usize) -> Option<Completion> {
    let before = text.get(..caret)?;
    let start = before
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_whitespace())
        .map(|(index, character)| index + character.len_utf8())
        .unwrap_or(0);
    let token = &before[start..];
    let kind = match token.chars().next()? {
        '$' => SuggestionKind::Skill,
        '/' => SuggestionKind::Command,
        '@' => SuggestionKind::File,
        _ => return None,
    };
    Some(Completion {
        kind,
        query: token[1..].to_string(),
        start,
        end: caret,
    })
}

/// 고른 제안을 낱말 자리에 넣은 글과 그 뒤 커서 위치. 앞 글자가 공백이 아니면 띄우고, 뒤에는 공백 하나를 붙인다
pub fn insert_suggestion(
    text: &str,
    completion: &Completion,
    insert_text: &str,
) -> (String, usize) {
    let prefix = &text[..completion.start];
    let suffix = &text[completion.end..];
    let separator = match !prefix.is_empty() && !prefix.ends_with(char::is_whitespace) {
        true => " ",
        false => "",
    };
    let insertion = format!("{separator}{insert_text} ");
    let caret = prefix.len() + insertion.len();
    (format!("{prefix}{insertion}{suffix}"), caret)
}

/// 메뉴를 열 때 받아 둔 skill·명령 목록
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestionCatalog {
    Loaded {
        items: Vec<PaneChatSuggestion>,
        is_complete: bool,
    },
    Failed,
}

/// 메뉴에 보일 결과
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SuggestionResults {
    pub items: Vec<PaneChatSuggestion>,
    pub is_loading: bool,
    pub has_error: bool,
    pub is_truncated: bool,
}

/// 글자마다 데스크톱에 물은 결과. 어느 메뉴 상태(`key`)의 답인지 함께 둔다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueriedSuggestions {
    pub key: String,
    pub results: SuggestionResults,
}

#[derive(Debug, Default)]
pub struct SuggestionState {
    /// "스킬" 버튼으로 연 검색 메뉴
    pub is_skill_picker_open: bool,
    pub skill_query: String,
    /// Esc나 선택으로 닫은 메뉴 상태. 글이나 커서가 바뀌면 다시 열린다
    pub dismissed_key: Option<String>,
    pub catalogs: HashMap<String, SuggestionCatalog>,
    pub catalog_requests: HashSet<String>,
    pub queried: Option<QueriedSuggestions>,
    pub active_index: usize,
    /// 다시 시도를 누를 때마다 올려 같은 질의도 다시 묻게 한다
    pub retry_count: u64,
    /// 마지막으로 활성 항목을 처음으로 되돌린 메뉴 상태
    pub last_completion_key: Option<String>,
    /// 마지막으로 목록을 받기 시작한 메뉴 상태와 다시 시도 횟수. 같으면 다시 묻지 않는다
    pub last_catalog_trigger: Option<String>,
    pub last_query_trigger: Option<String>,
}

impl SuggestionState {
    /// 받아 둔 목록이 전부면 화면에서 거르고, 실패했으면 오류를, 잘렸거나 파일이면 글자마다 물은 결과를 보인다
    pub fn current_results(
        &self,
        catalog_key: Option<&str>,
        completion_key: Option<&str>,
        query: &str,
    ) -> Option<SuggestionResults> {
        let catalog = catalog_key.and_then(|key| self.catalogs.get(key));
        match catalog {
            Some(SuggestionCatalog::Loaded {
                items,
                is_complete: true,
            }) => {
                let filtered =
                    filter_pane_chat_suggestions(items, query, MAX_PANE_CHAT_SUGGESTIONS);
                Some(SuggestionResults {
                    items: filtered.items,
                    is_loading: false,
                    has_error: false,
                    is_truncated: filtered.is_truncated,
                })
            }
            Some(SuggestionCatalog::Failed) => Some(SuggestionResults {
                has_error: true,
                ..SuggestionResults::default()
            }),
            _ => self
                .queried
                .as_ref()
                .filter(|queried| Some(queried.key.as_str()) == completion_key)
                .map(|queried| queried.results.clone()),
        }
    }

    /// 글자마다 데스크톱에 물어야 하는지. 파일이거나 받아 둔 목록이 잘렸을 때다
    pub fn asks_desktop_per_query(&self, kind: SuggestionKind, catalog_key: Option<&str>) -> bool {
        if kind == SuggestionKind::File {
            return true;
        }
        let catalog = catalog_key.and_then(|key| self.catalogs.get(key));
        matches!(
            catalog,
            Some(SuggestionCatalog::Loaded {
                is_complete: false,
                ..
            })
        )
    }

    /// 다시 받는 데 실패해도 이미 받아 둔 목록이 있으면 그대로 둔다
    pub fn store_catalog(&mut self, key: &str, result: Option<SuggestionCatalog>) {
        self.catalog_requests.remove(key);
        match result {
            Some(catalog) => {
                self.catalogs.insert(key.to_string(), catalog);
            }
            None if matches!(
                self.catalogs.get(key),
                Some(SuggestionCatalog::Loaded { .. })
            ) => {}
            None => {
                self.catalogs
                    .insert(key.to_string(), SuggestionCatalog::Failed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_token_finds_the_sigil_after_whitespace_only() {
        let completion = completion_at("Please $e", 9).expect("skill");
        assert_eq!(completion.kind, SuggestionKind::Skill);
        assert_eq!(completion.query, "e");
        assert_eq!(completion.start, 7);
        assert_eq!(completion_at("a/b", 3), None);
        assert_eq!(
            completion_at("@src", 4).map(|found| found.query),
            Some("src".into())
        );
    }

    #[test]
    fn chosen_suggestion_replaces_the_token_and_adds_a_space() {
        let completion = completion_at("Please $e", 9).unwrap();
        assert_eq!(
            insert_suggestion("Please $e", &completion, "$test"),
            ("Please $test ".to_string(), 13)
        );
    }
}
