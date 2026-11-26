//! 프로젝트 필터의 URL 쿼리와 창 세션 기억(TS `useProjectFilterParams`의 `?projects=`·sessionStorage).
//! 쿼리가 없으면 그 창에서 마지막으로 고른 프로젝트를 되살리고, 선택을 바꾸면 쿼리와 기억을 함께 바꾼다.
//! sessionStorage가 창(탭)마다 따로였으므로 기억도 창마다 둔다. 앱을 끄면 사라진다.

use std::collections::HashMap;

use gpui_kit::{Context, Global, WindowId};

use super::BoardView;

const PROJECTS_QUERY_KEY: &str = "projects";

/// 창마다 마지막으로 고른 프로젝트 ID들. 빈 선택은 남기지 않는다
#[derive(Default)]
struct ProjectFilterSessions(HashMap<WindowId, Vec<String>>);

impl Global for ProjectFilterSessions {}

/// 쿼리의 `projects=a,b`를 ID 목록으로 읽는다
pub(super) fn parse_selected_project_ids(search: &str) -> Vec<String> {
    super::query_value(search, PROJECTS_QUERY_KEY)
        .map(|value| {
            value
                .split(',')
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `key=value&...` 검색 문자열에서 `projects`만 바꾸거나 뺀다
fn search_with_selected_projects(search: &str, project_ids: &[String]) -> String {
    let mut pairs: Vec<String> = search
        .trim_start_matches('?')
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| pair.split('=').next() != Some(PROJECTS_QUERY_KEY))
        .map(str::to_string)
        .collect();
    if !project_ids.is_empty() {
        pairs.push(format!("{PROJECTS_QUERY_KEY}={}", project_ids.join(",")));
    }
    pairs.join("&")
}

impl BoardView {
    /// 첫 렌더에서 창을 알게 되면 쿼리와 창 기억을 맞춘다. 쿼리가 있으면 그 선택을 기억하고, 없으면 기억을 되살려 쿼리에 쓴다
    pub(super) fn restore_project_filter_session(
        &mut self,
        window_id: WindowId,
        cx: &mut Context<Self>,
    ) {
        if super::query_value(&self.screen.route.search, PROJECTS_QUERY_KEY).is_none() {
            self.filters.get_mut().selected_project_ids = cx
                .try_global::<ProjectFilterSessions>()
                .and_then(|sessions| sessions.0.get(&window_id).cloned())
                .unwrap_or_default();
        }
        self.sync_project_filter_query(cx);
    }

    /// 프로젝트 선택이 바뀌면 쿼리와 창 기억도 바꾼다(TS `setSelectedProjectIds`)
    pub(super) fn sync_project_filter_query(&mut self, cx: &mut Context<Self>) {
        let Some(window_id) = self.task_kind_window else {
            return;
        };
        let selected = self.filters.selected_project_ids.clone();
        let sessions = &mut cx.default_global::<ProjectFilterSessions>().0;
        if selected.is_empty() {
            sessions.remove(&window_id);
        } else {
            sessions.insert(window_id, selected.clone());
        }
        let search = search_with_selected_projects(&self.screen.route.search, &selected);
        if search != self.screen.route.search {
            self.replace_board_search(search, cx);
        }
    }

    /// 불러온 보드에 없는 프로젝트(지워졌거나 다른 데이터의 ID)는 선택에서 뺀다(TS `validProjectIds` 거르기)
    pub(super) fn drop_unknown_selected_projects(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.board.as_ref() else {
            return;
        };
        let known_projects = &board.catalog.by_id;
        let selected_count = self.filters.selected_project_ids.len();
        self.filters
            .get_mut()
            .selected_project_ids
            .retain(|project_id| known_projects.contains_key(project_id));
        if self.filters.selected_project_ids.len() != selected_count {
            self.sync_project_filter_query(cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_projects_are_read_from_the_query() {
        assert_eq!(
            parse_selected_project_ids("taskKind=task&projects=p1,p2"),
            ["p1", "p2"]
        );
        assert!(parse_selected_project_ids("taskKind=task").is_empty());
    }

    #[test]
    fn changing_the_selection_keeps_other_query_values() {
        let selected = ["p1".to_string(), "p2".to_string()];
        assert_eq!(
            search_with_selected_projects("taskKind=task&projects=old", &selected),
            "taskKind=task&projects=p1,p2"
        );
        assert_eq!(
            search_with_selected_projects("taskKind=task&projects=p1", &[]),
            "taskKind=task"
        );
    }
}
