//! 작업 종류 필터의 URL 쿼리와 창 세션 기억(TS `useTaskKindFilterParams`의 `?taskKind=`·sessionStorage).
//! 쿼리가 없으면 그 창에서 마지막으로 고른 필터를 되살리고, 모르는 값이면 쿼리와 기억을 함께 지운다.
//! sessionStorage가 창(탭)마다 따로였으므로 기억도 창마다 둔다. 앱을 끄면 사라진다.

use std::collections::HashMap;

use gpui_kit::{Context, Global, Window, WindowId};

use super::BoardView;
use super::task_filters::TaskKindFilter;

/// 창마다 마지막으로 고른 작업 종류 필터. 기본값(`All`)은 남기지 않는다
#[derive(Default)]
struct TaskKindFilterSessions(HashMap<WindowId, TaskKindFilter>);

impl Global for TaskKindFilterSessions {}

/// `key=value&...` 검색 문자열에서 `taskKind`만 바꾸거나 뺀다
pub(super) fn search_with_task_kind(search: &str, filter: TaskKindFilter) -> String {
    let mut pairs: Vec<String> = search
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| pair.split('=').next() != Some(TaskKindFilter::QUERY_KEY))
        .map(str::to_string)
        .collect();
    if filter != TaskKindFilter::All {
        pairs.push(format!("{}={}", TaskKindFilter::QUERY_KEY, filter.as_str()));
    }
    pairs.join("&")
}

/// 첫 진입 때 쿼리와 창 기억을 맞춘 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TaskKindEntry {
    pub filter: TaskKindFilter,
    /// 창 기억에 남길 값(`None`이면 지운다)
    pub remembered: Option<TaskKindFilter>,
    /// 쿼리를 바꿔야 하면 새 검색 문자열
    pub replaced_search: Option<String>,
}

/// TS 훅의 첫 효과: 쿼리가 있으면 그 값(모르면 기본값으로 되돌리고 쿼리 제거), 없으면 창 기억을 되살린다
pub(super) fn resolve_task_kind_entry(
    search: &str,
    raw_value: Option<&str>,
    remembered: Option<TaskKindFilter>,
) -> TaskKindEntry {
    let remember = |filter: TaskKindFilter| (filter != TaskKindFilter::All).then_some(filter);
    match raw_value {
        Some(raw) => {
            let filter = TaskKindFilter::parse(Some(raw));
            let is_unknown = filter.as_str() != raw;
            TaskKindEntry {
                filter,
                remembered: remember(filter),
                replaced_search: is_unknown.then(|| search_with_task_kind(search, filter)),
            }
        }
        None => {
            let filter = remembered.unwrap_or(TaskKindFilter::All);
            TaskKindEntry {
                filter,
                remembered: remember(filter),
                replaced_search: (filter != TaskKindFilter::All)
                    .then(|| search_with_task_kind(search, filter)),
            }
        }
    }
}

impl BoardView {
    /// 첫 렌더에서 창을 알게 되면 쿼리와 창 기억을 맞춘다
    pub(super) fn restore_task_kind_session(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.task_kind_window.is_some() {
            return;
        }
        let window_id = window.window_handle().window_id();
        self.task_kind_window = Some(window_id);
        let search = self.screen.route.search.clone();
        let raw_value = super::query_value(&search, TaskKindFilter::QUERY_KEY);
        let remembered = cx
            .try_global::<TaskKindFilterSessions>()
            .and_then(|sessions| sessions.0.get(&window_id).copied());
        let entry = resolve_task_kind_entry(&search, raw_value, remembered);
        self.filters.task_kind = entry.filter;
        self.remember_task_kind(entry.remembered, cx);
        if let Some(search) = entry.replaced_search {
            self.replace_board_search(search, cx);
        }
    }

    /// 필터를 바꾸면 쿼리와 창 기억도 바꾼다(TS `setTaskKindFilter`)
    pub(super) fn sync_task_kind_query(&mut self, filter: TaskKindFilter, cx: &mut Context<Self>) {
        self.remember_task_kind((filter != TaskKindFilter::All).then_some(filter), cx);
        let search = search_with_task_kind(&self.screen.route.search, filter);
        self.replace_board_search(search, cx);
    }

    fn remember_task_kind(&mut self, filter: Option<TaskKindFilter>, cx: &mut Context<Self>) {
        let Some(window_id) = self.task_kind_window else {
            return;
        };
        let sessions = &mut cx.default_global::<TaskKindFilterSessions>().0;
        match filter {
            Some(filter) => sessions.insert(window_id, filter),
            None => sessions.remove(&window_id),
        };
    }

    fn replace_board_search(&mut self, search: String, cx: &mut Context<Self>) {
        let href = if search.is_empty() {
            "/".to_string()
        } else {
            format!("/?{search}")
        };
        self.screen.route.search = search;
        let navigator = self.screen.navigator.clone();
        // 렌더 중에 창 루트를 고치지 않도록 이번 갱신이 끝난 뒤에 바꾼다
        cx.defer(move |cx| navigator.replace_search(&href, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // parity: TS-renderer-useTaskKindFilterParams-714d3d
    #[test]
    fn without_a_query_the_window_memory_is_restored_into_the_query() {
        let entry = resolve_task_kind_entry("", None, Some(TaskKindFilter::Task));
        assert_eq!(entry.filter, TaskKindFilter::Task);
        assert_eq!(entry.replaced_search.as_deref(), Some("taskKind=task"));
    }

    // parity: TS-renderer-useTaskKindFilterParams-b17552
    #[test]
    fn an_unknown_query_clears_both_the_query_and_the_memory() {
        let entry = resolve_task_kind_entry("taskKind=unknown", Some("unknown"), None);
        assert_eq!(entry.filter, TaskKindFilter::All);
        assert_eq!(entry.remembered, None);
        assert_eq!(entry.replaced_search.as_deref(), Some(""));
    }

    #[test]
    fn changing_the_filter_keeps_other_query_values() {
        assert_eq!(
            search_with_task_kind("taskId=t1&taskKind=task", TaskKindFilter::Project),
            "taskId=t1&taskKind=project"
        );
        assert_eq!(
            search_with_task_kind("taskKind=task", TaskKindFilter::All),
            ""
        );
    }
}
