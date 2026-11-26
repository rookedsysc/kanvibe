//! 입력창 메뉴의 조회와 키 처리(TS `PaneChatComposer`의 메뉴 effect들과 `handleCompletionKey`).
//! skill·명령 목록은 메뉴를 열 때마다 한 번 새로 받아 두고(받는 동안에는 지난 목록을 그대로 보인다),
//! 파일과 잘린 목록은 글자 입력이 180ms 멈춘 뒤에 묻는다.

use std::time::Duration;

use gpui_kit::{Context, Window};
use kanvibe_domain::pane_chat::{MAX_PANE_CHAT_CATALOG_SUGGESTIONS, PaneChatSuggestion};

use super::client::{PaneChatResult, PaneChatSuggestionList, SuggestionKind, SuggestionQuery};
use super::composer::CompletionKey;
use super::suggestions::{
    QueriedSuggestions, SuggestionCatalog, SuggestionResults, insert_suggestion,
};
use super::view::PaneChatView;

const SUGGESTION_DEBOUNCE: Duration = Duration::from_millis(180);

impl PaneChatView {
    /// 메뉴에 지금 보일 결과. 메뉴가 닫혀 있으면 `None`이다
    pub fn suggestion_results(&self) -> Option<SuggestionResults> {
        let context = self.completion_context();
        if !context.is_shown {
            return None;
        }
        let query = context
            .completion
            .as_ref()
            .map(|completion| completion.query.clone())
            .unwrap_or_default();
        Some(
            self.composer
                .suggestions
                .current_results(
                    context.catalog_key.as_deref(),
                    context.completion_key.as_deref(),
                    &query,
                )
                .unwrap_or(SuggestionResults {
                    is_loading: true,
                    ..SuggestionResults::default()
                }),
        )
    }

    /// 글·커서·대상이 바뀐 뒤 메뉴가 받아야 할 목록을 묻는다(TS의 메뉴 effect들)
    pub(super) fn sync_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let context = self.completion_context();
        let suggestions = &mut self.composer.suggestions;
        if context.completion_key != suggestions.last_completion_key {
            suggestions.last_completion_key = context.completion_key.clone();
            suggestions.active_index = 0;
        }
        let Some(completion) = context.completion.filter(|_| context.is_shown) else {
            self.composer._query = None;
            suggestions.last_catalog_trigger = None;
            suggestions.last_query_trigger = None;
            return;
        };
        let catalog_trigger = context
            .catalog_key
            .as_ref()
            .map(|key| format!("{key}|{}", suggestions.retry_count));
        if catalog_trigger != suggestions.last_catalog_trigger {
            suggestions.last_catalog_trigger = catalog_trigger;
            if let Some(catalog_key) = context.catalog_key.as_deref() {
                self.request_catalog(catalog_key, completion.kind, window, cx);
            }
        }
        let asks_per_query = self
            .composer
            .suggestions
            .asks_desktop_per_query(completion.kind, context.catalog_key.as_deref());
        let suggestions = &mut self.composer.suggestions;
        let query_trigger = context
            .completion_key
            .as_ref()
            .filter(|_| asks_per_query)
            .map(|key| format!("{key}|{}", suggestions.retry_count));
        if query_trigger == suggestions.last_query_trigger {
            return;
        }
        suggestions.last_query_trigger = query_trigger;
        match (asks_per_query, context.completion_key) {
            (true, Some(key)) => {
                self.request_query(key, completion.kind, completion.query, window, cx)
            }
            _ => self.composer._query = None,
        }
    }

    /// 메뉴를 열 때마다 skill·명령 목록을 한 번 받는다. 받는 중이면 다시 묻지 않는다
    fn request_catalog(
        &mut self,
        catalog_key: &str,
        kind: SuggestionKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let suggestions = &mut self.composer.suggestions;
        if suggestions.catalog_requests.contains(catalog_key) {
            return;
        }
        let Some(target) = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.target_token.clone())
        else {
            return;
        };
        suggestions.catalog_requests.insert(catalog_key.to_string());
        if suggestions.catalogs.get(catalog_key) == Some(&SuggestionCatalog::Failed) {
            suggestions.catalogs.remove(catalog_key);
        }
        let query = SuggestionQuery {
            kind,
            text: String::new(),
            limit: Some(MAX_PANE_CHAT_CATALOG_SUGGESTIONS),
        };
        let request = self
            .client
            .get_suggestions(&self.session.task_id, &target, &query, cx);
        let catalog_key = catalog_key.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let catalog = catalog_of(request.await, &target);
            // 받은 목록이 잘렸으면 이제부터 글자마다 물어야 하므로 메뉴 조회를 다시 맞춘다
            let _ = this.update_in(cx, |view, window, cx| {
                view.composer
                    .suggestions
                    .store_catalog(&catalog_key, catalog);
                view.sync_suggestions(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// 파일이나 잘린 목록은 글자 입력이 멈춘 뒤 데스크톱에 그 검색어를 묻는다. 다른 대상의 답은 오류로 본다
    fn request_query(
        &mut self,
        key: String,
        kind: SuggestionKind,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let suggestions = &mut self.composer.suggestions;
        let Some(target) = self
            .session
            .snapshot()
            .and_then(|snapshot| snapshot.target_token.clone())
        else {
            return;
        };
        let loading = SuggestionResults {
            is_loading: true,
            ..SuggestionResults::default()
        };
        suggestions.queried = Some(QueriedSuggestions {
            key: key.clone(),
            results: loading,
        });
        let client = self.client.clone();
        let task_id = self.session.task_id.clone();
        self.composer._query = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SUGGESTION_DEBOUNCE).await;
            let query = SuggestionQuery {
                kind,
                text,
                limit: None,
            };
            let Ok(request) =
                cx.update(|_, cx| client.get_suggestions(&task_id, &target, &query, cx))
            else {
                return;
            };
            let results = query_results(request.await, &target);
            let _ = this.update(cx, |view, cx| {
                view.composer.suggestions.queried = Some(QueriedSuggestions { key, results });
                cx.notify();
            });
        }));
    }

    /// 메뉴 항목을 고른다. 낱말 자리에 넣고 커서를 그 뒤로 옮긴 뒤 메뉴를 닫는다
    pub fn choose_suggestion(
        &mut self,
        item: &PaneChatSuggestion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let context = self.completion_context();
        let Some(completion) = context.completion else {
            return;
        };
        let text = self.composer.draft().text;
        let (next_text, caret) = insert_suggestion(&text, &completion, &item.insert_text);
        self.set_composer_text(&next_text, caret, window, cx);
        self.composer.suggestions.is_skill_picker_open = false;
        self.composer.suggestions.dismissed_key = self.completion_context().completion_key;
        self.sync_suggestions(window, cx);
        self.composer.focus(window, cx);
    }

    /// 메뉴가 열려 있을 때 키를 받는다. 받았으면 `true`다(TS `handleCompletionKey`)
    pub fn handle_completion_key(
        &mut self,
        key: CompletionKey,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(results) = self.suggestion_results() else {
            return false;
        };
        let items = results.items;
        let suggestions = &mut self.composer.suggestions;
        match key {
            CompletionKey::Up | CompletionKey::Down if !items.is_empty() => {
                let count = items.len();
                let step = if key == CompletionKey::Down {
                    1
                } else {
                    count - 1
                };
                suggestions.active_index = (suggestions.active_index + step) % count;
            }
            CompletionKey::Up | CompletionKey::Down => return false,
            CompletionKey::Escape => {
                suggestions.dismissed_key = suggestions.last_completion_key.clone();
                suggestions.is_skill_picker_open = false;
                self.composer.focus(window, cx);
            }
            CompletionKey::Tab | CompletionKey::Enter => {
                match items.get(suggestions.active_index).cloned() {
                    Some(item) => self.choose_suggestion(&item, window, cx),
                    None if key == CompletionKey::Enter => {}
                    None => return false,
                }
            }
        }
        cx.notify();
        true
    }

    /// "스킬" 버튼. 검색 메뉴를 열거나 닫는다
    pub fn toggle_skill_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let suggestions = &mut self.composer.suggestions;
        suggestions.is_skill_picker_open = !suggestions.is_skill_picker_open;
        suggestions.skill_query.clear();
        suggestions.dismissed_key = None;
        let is_open = suggestions.is_skill_picker_open;
        self.composer
            .skill_search
            .update(cx, |state, cx| state.set_value("", window, cx));
        if is_open {
            let handle = gpui_kit::Focusable::focus_handle(&self.composer.skill_search, cx);
            window.focus(&handle, cx);
        }
        self.sync_suggestions(window, cx);
        cx.notify();
    }

    /// 메뉴 오류의 "다시 시도"
    pub fn retry_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.suggestions.retry_count += 1;
        self.sync_suggestions(window, cx);
        cx.notify();
    }

    pub fn set_active_suggestion(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.composer.suggestions.active_index != index {
            self.composer.suggestions.active_index = index;
            cx.notify();
        }
    }
}

/// 받아 둘 목록. 다른 대상의 답이거나 실패했으면 `None`이다
fn catalog_of(
    result: PaneChatResult<PaneChatSuggestionList>,
    target: &str,
) -> Option<SuggestionCatalog> {
    result
        .ok()
        .filter(|list| list.target_token == target)
        .map(|list| SuggestionCatalog::Loaded {
            is_complete: !list.is_truncated,
            items: list.items,
        })
}

/// 글자마다 물은 결과. 다른 대상의 답은 오류로 본다
fn query_results(
    result: PaneChatResult<PaneChatSuggestionList>,
    target: &str,
) -> SuggestionResults {
    match result {
        Ok(list) if list.target_token == target => SuggestionResults {
            items: list.items,
            is_truncated: list.is_truncated,
            ..SuggestionResults::default()
        },
        _ => SuggestionResults {
            has_error: true,
            ..SuggestionResults::default()
        },
    }
}
