//! 태스크 빠른 검색(TS `TaskQuickSearchDialog`). `Mod+Shift+O`로 열고 닫는다. 브랜치·프로젝트·제목을 퍼지 검색하고
//! 점수 순으로 보이며, Enter는 작업 상세(워크스페이스에서는 그 작업 선택), Shift+Enter는 새 창으로 연다.
//! 열려 있는 동안 새 작업 단축키는 고른 작업의 브랜치에서 시작하는 TODO를 만든다(보드가 떠 있을 때).

use std::ops::Range;

use gpui_kit::{
    AnyElement, Context, FontWeight, HighlightStyle, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, StyledText, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::shortcuts::bindings::ShortcutCommandId;
use kanvibe_domain::shortcuts::notation::format_shortcut_for_display;
use kanvibe_process::project_search::{FuzzyMatch, fuzzy_match};
use kanvibe_services::kanban::SearchableTask;

use super::LocaleShell;
use super::command_palette::{
    RESULT_LIST_MAX_HEIGHT, dialog_footer, global_dialog_layer, global_dialog_panel,
};
use crate::components::{TailwindText, fade};
use crate::fonts::MONO_FAMILY;
use crate::qa::targets::{qa_button_layer, qa_text_layer};
use crate::services::AppServices;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::board::inline_load_error;
use crate::views::board::services::{BoardViewMode, board_services};
use crate::views::board::text_field::TextFieldEvent;
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

/// `max-w-2xl`
const QUICK_SEARCH_WIDTH: f32 = 672.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchField {
    Branch,
    Project,
    Title,
}

/// 검색 결과 한 줄과 필드별 일치 위치(문자 단위)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickSearchResult {
    pub task: SearchableTask,
    pub score: i64,
    pub branch_indices: Option<Vec<usize>>,
    pub project_indices: Option<Vec<usize>>,
    pub title_indices: Option<Vec<usize>>,
}

#[derive(Clone)]
struct WeightedMatch {
    field: SearchField,
    weight: i64,
    found: FuzzyMatch,
}

impl WeightedMatch {
    fn score(&self) -> i64 {
        self.found.score + self.weight
    }
}

fn find_field_matches(
    token: &str,
    fields: &[(SearchField, Option<&str>, i64)],
) -> Vec<WeightedMatch> {
    fields
        .iter()
        .filter_map(|(field, value, weight)| {
            let found = fuzzy_match(token, (*value)?)?;
            Some(WeightedMatch {
                field: *field,
                weight: *weight,
                found,
            })
        })
        .collect()
}

/// 토큰 하나가 한 필드에 맞거나, 둘로 나눠 서로 다른 두 필드에 맞는 가장 좋은 조합
fn find_token_matches(
    token: &str,
    fields: &[(SearchField, Option<&str>, i64)],
) -> Option<(Vec<WeightedMatch>, i64)> {
    let direct = find_field_matches(token, fields);
    if !direct.is_empty() {
        let score = direct.iter().map(WeightedMatch::score).max().unwrap_or(0);
        return Some((direct, score));
    }
    let characters: Vec<char> = token.chars().collect();
    let mut best: Option<(Vec<WeightedMatch>, i64)> = None;
    for split in 1..characters.len() {
        let left: String = characters[..split].iter().collect();
        let right: String = characters[split..].iter().collect();
        let left_matches = find_field_matches(&left, fields);
        let right_matches = find_field_matches(&right, fields);
        for left_match in &left_matches {
            for right_match in &right_matches {
                let score = left_match.score() + right_match.score();
                let is_better = best
                    .as_ref()
                    .is_none_or(|(_, best_score)| score > *best_score);
                if left_match.field != right_match.field && is_better {
                    best = Some((vec![left_match.clone(), right_match.clone()], score));
                }
            }
        }
    }
    best
}

fn merged_indices(matches: &[FuzzyMatch]) -> Option<Vec<usize>> {
    if matches.is_empty() {
        return None;
    }
    let mut indices: Vec<usize> = matches
        .iter()
        .flat_map(|found| found.matched_indices.iter().copied())
        .collect();
    indices.sort_unstable();
    indices.dedup();
    Some(indices)
}

fn search_one(task: &SearchableTask, tokens: &[&str]) -> Option<QuickSearchResult> {
    let fields = [
        (SearchField::Branch, task.branch_name.as_deref(), 3),
        (SearchField::Project, task.project_name.as_deref(), 2),
        (SearchField::Title, Some(task.title.as_str()), 1),
    ];
    let mut by_field: [Vec<FuzzyMatch>; 3] = Default::default();
    let mut score = 0;
    for token in tokens {
        let (matches, token_score) = find_token_matches(token, &fields)?;
        for found in matches {
            by_field[found.field as usize].push(found.found);
        }
        score += token_score;
    }
    let [branch, project, title] = by_field;
    Some(QuickSearchResult {
        task: task.clone(),
        score,
        branch_indices: task
            .branch_name
            .as_ref()
            .and_then(|_| merged_indices(&branch)),
        project_indices: task
            .project_name
            .as_ref()
            .and_then(|_| merged_indices(&project)),
        title_indices: merged_indices(&title),
    })
}

/// TS `buildSearchResults`: 검색어가 없으면 받은 순서 그대로, 있으면 모든 토큰이 맞는 작업을 점수 내림차순으로
pub fn build_search_results(tasks: &[SearchableTask], query: &str) -> Vec<QuickSearchResult> {
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return tasks
            .iter()
            .map(|task| QuickSearchResult {
                task: task.clone(),
                score: 0,
                branch_indices: None,
                project_indices: None,
                title_indices: None,
            })
            .collect();
    }
    let mut results: Vec<QuickSearchResult> = tasks
        .iter()
        .filter_map(|task| search_one(task, &tokens))
        .collect();
    results.sort_by_key(|result| std::cmp::Reverse(result.score));
    results
}

/// 열린 빠른 검색
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct QuickSearchState {
    pub selected_index: usize,
    pub tasks: Vec<SearchableTask>,
    pub is_loading: bool,
    pub load_failure: Option<String>,
}

impl LocaleShell {
    pub fn is_quick_search_open(&self) -> bool {
        self.quick_search.is_some()
    }

    pub fn quick_search_results(&self, cx: &gpui_kit::App) -> Vec<QuickSearchResult> {
        let Some(state) = &self.quick_search else {
            return Vec::new();
        };
        build_search_results(&state.tasks, self.quick_search_field.read(cx).text())
    }

    /// 단축키로 열고, 열려 있으면 닫는다
    pub fn toggle_quick_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_search.is_some() {
            return self.close_quick_search(window, cx);
        }
        if self.has_open_overlay(cx) || self.has_board_blocker(cx) {
            return;
        }
        self.remember_focus(window, cx);
        self.quick_search = Some(QuickSearchState {
            is_loading: true,
            ..QuickSearchState::default()
        });
        // 새로 연 목록은 맨 위에서 시작한다
        self.quick_search_scroll
            .set_offset(gpui_kit::point(px(0.0), px(0.0)));
        self.quick_search_field
            .update(cx, |field, cx| field.set_text("", cx));
        let field_focus = gpui_kit::Focusable::focus_handle(self.quick_search_field.read(cx), cx);
        field_focus.focus(window, cx);
        self.load_quick_search_tasks(cx);
        cx.notify();
    }

    fn load_quick_search_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(services) = board_services(cx) else {
            if let Some(state) = self.quick_search.as_mut() {
                state.is_loading = false;
            }
            return;
        };
        let load = services.load_searchable_tasks(cx);
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                let Some(state) = this.quick_search.as_mut() else {
                    return;
                };
                state.is_loading = false;
                match result {
                    Ok(tasks) => state.tasks = tasks,
                    Err(failure) => state.load_failure = Some(failure.reason),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn retry_quick_search(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.quick_search.as_mut() {
            state.load_failure = None;
            state.is_loading = true;
        }
        self.load_quick_search_tasks(cx);
        cx.notify();
    }

    fn close_quick_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_search.take().is_none() {
            return;
        }
        self.restore_focus(window, cx);
        cx.notify();
    }

    pub(super) fn handle_quick_search_field_event(
        &mut self,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.quick_search_results(cx).len();
        let Some(state) = self.quick_search.as_mut() else {
            return;
        };
        match event {
            TextFieldEvent::Changed(_) => state.selected_index = 0,
            // 키보드로 고른 결과는 목록 안에서 보이게 굴린다(TS `scrollIntoView({ block: "nearest" })`)
            TextFieldEvent::NavigationKey(key) if key == "down" => {
                state.selected_index = (state.selected_index + 1).min(count.saturating_sub(1));
                self.quick_search_scroll
                    .scroll_to_item(state.selected_index);
            }
            TextFieldEvent::NavigationKey(key) if key == "up" => {
                state.selected_index = state.selected_index.saturating_sub(1);
                self.quick_search_scroll
                    .scroll_to_item(state.selected_index);
            }
            TextFieldEvent::NavigationKey(_) => {}
            TextFieldEvent::Submitted { is_shift_pressed } => {
                let index = state.selected_index.min(count.saturating_sub(1));
                let selected = self.quick_search_results(cx).get(index).cloned();
                if let Some(result) = selected {
                    self.open_quick_search_task(&result.task.id, *is_shift_pressed, window, cx);
                }
                return;
            }
            TextFieldEvent::Escaped => return self.close_quick_search(window, cx),
        }
        cx.notify();
    }

    /// 워크스페이스에서는 그 작업을 고르고, 칸반에서는 작업 상세로 간다
    fn open_quick_search_task(
        &mut self,
        task_id: &str,
        should_open_new_window: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace_board = self
            .board()
            .filter(|board| board.read(cx).view_mode() == BoardViewMode::Workspace);
        if let Some(board) = workspace_board.filter(|_| !should_open_new_window) {
            board.update(cx, |board, cx| {
                board.request_workspace_selection(task_id, cx)
            });
            self.close_quick_search(window, cx);
            return;
        }
        let request = TaskNavigationRequest {
            origin_window: Some(window.window_handle().window_id()),
            should_open_new_window,
        };
        let locale = self.locale;
        self.close_quick_search(window, cx);
        // 같은 창 이동은 창 루트가 이 덮개를 다시 읽으므로, 덮개 갱신이 끝난 뒤에 옮긴다
        let task_id = task_id.to_string();
        cx.defer(move |cx| {
            navigate_to_task_detail(&task_id, locale, request, cx);
        });
    }

    /// 빠른 검색이 열린 채 새 작업 단축키를 누르면, 고른 작업의 브랜치에서 시작하는 TODO 모달을 연다(TS `requestCreateBranchTodo`)
    pub(super) fn create_branch_todo_from_quick_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self
            .quick_search
            .as_ref()
            .map(|state| state.selected_index)
            .unwrap_or(0);
        let results = self.quick_search_results(cx);
        let Some(task) = results
            .get(index.min(results.len().saturating_sub(1)))
            .map(|result| result.task.clone())
        else {
            return;
        };
        let (Some(project_id), Some(branch_name)) = (task.project_id, task.branch_name) else {
            return;
        };
        let defaults = Some((branch_name, project_id));
        if let Some(detail) = self.task_detail() {
            self.close_quick_search(window, cx);
            detail.update(cx, |detail, cx| {
                detail.open_create_task(defaults, window, cx)
            });
            return;
        }
        let Some(board) = self.board() else {
            return;
        };
        self.close_quick_search(window, cx);
        board.update(cx, |board, cx| board.open_create_task(defaults, window, cx));
    }

    pub(super) fn render_quick_search(
        &mut self,
        theme: &ThemeTokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.quick_search.clone()?;
        let labels = Translator::new(self.locale, "taskSearch");
        let header = div()
            .border_b_1()
            .border_color(theme.border.default)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .child(
                div()
                    .relative()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(labels.translate("title"))
                    .child(qa_text_layer("heading", &labels.translate("title"))),
            )
            .child(
                div()
                    .mt(spacing(3.0))
                    .w_full()
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.brand.primary)
                    .bg(theme.background.page)
                    .px(spacing(3.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::SM)
                    .child(self.quick_search_field.clone()),
            );
        let body = self.render_quick_search_body(theme, &state, cx);
        let panel = global_dialog_panel(theme, QUICK_SEARCH_WIDTH)
            .child(header)
            .child(
                div()
                    .id("quick-search-results")
                    .max_h(px(RESULT_LIST_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .track_scroll(&self.quick_search_scroll)
                    .children(body),
            )
            .child(dialog_footer(theme, self.quick_search_hint(cx)));
        let close = cx.listener(|this, _, window, cx| this.close_quick_search(window, cx));
        Some(global_dialog_layer(
            theme,
            self.overlay_viewport(window, cx),
            "quick-search-backdrop",
            close,
            panel,
        ))
    }

    /// 안내 줄: 기본 안내와 새 branch TODO 단축키
    fn quick_search_hint(&self, cx: &gpui_kit::App) -> String {
        let labels = Translator::new(self.locale, "taskSearch");
        let app_services = cx.global::<AppServices>();
        let bindings = app_services.shortcut_bindings();
        let shortcut = format_shortcut_for_display(
            bindings.shortcut(ShortcutCommandId::CreateTask),
            app_services.shortcut_platform,
        );
        format!(
            "{} · {}",
            labels.translate("hint"),
            labels.translate_with("branchTodoHint", &[("shortcut", &shortcut)])
        )
    }

    /// 결과 목록의 행들(스크롤 칸의 바로 아래 자식이라야 고른 행으로 굴릴 수 있다)
    fn render_quick_search_body(
        &self,
        theme: &ThemeTokens,
        state: &QuickSearchState,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let common = Translator::new(self.locale, "common");
        let message = |text: String| {
            div()
                .relative()
                .px(spacing(4.0))
                .py(spacing(6.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(text.clone())
                .child(qa_text_layer("status", &text))
                .into_any_element()
        };
        if state.is_loading {
            return vec![message(common.translate("loading"))];
        }
        if let Some(reason) = state.load_failure.clone() {
            let text = common.translate_with("loadFailed", &[("reason", &reason)]);
            let retry = cx.listener(|this, _, _, cx| this.retry_quick_search(cx));
            return vec![
                div()
                    .m(spacing(4.0))
                    .child(inline_load_error(
                        theme,
                        self.locale,
                        text,
                        "quick-search-retry",
                        retry,
                    ))
                    .into_any_element(),
            ];
        }
        let results = self.quick_search_results(cx);
        if results.is_empty() {
            return vec![message(
                Translator::new(self.locale, "taskSearch").translate("empty"),
            )];
        }
        let selected = state.selected_index.min(results.len() - 1);
        results
            .iter()
            .enumerate()
            .map(|(index, result)| {
                self.quick_search_row(theme, result, index, index == selected, cx)
            })
            .collect()
    }

    fn quick_search_row(
        &self,
        theme: &ThemeTokens,
        result: &QuickSearchResult,
        index: usize,
        is_selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let task = &result.task;
        let (primary_label, primary_indices) = match &task.branch_name {
            Some(branch) => (branch.clone(), result.branch_indices.clone()),
            None => (task.title.clone(), result.title_indices.clone()),
        };
        let mut text_column = div().min_w_0().child(
            div()
                .text_scale(text_scale::SM)
                .css_weight(500)
                .text_color(theme.text.primary)
                .child(match_text(
                    theme,
                    &primary_label,
                    primary_indices.as_deref(),
                )),
        );
        if task
            .branch_name
            .as_deref()
            .is_some_and(|branch| branch != task.title)
        {
            text_column = text_column.child(secondary_line(
                theme,
                &task.title,
                result.title_indices.as_deref(),
            ));
        }
        if let Some(project_name) = &task.project_name {
            text_column = text_column.child(secondary_line(
                theme,
                project_name,
                result.project_indices.as_deref(),
            ));
        }
        let task_id = task.id.clone();
        let hover_background = theme.background.page;
        let row = div()
            .id(("quick-search-result", index))
            .relative()
            .flex()
            .w_full()
            .items_start()
            .justify_between()
            .gap(spacing(4.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .cursor_pointer()
            .child(text_column)
            .children(self.remote_host_badges(theme, task))
            .child(qa_button_layer(&primary_label, &primary_label))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if let Some(state) = this.quick_search.as_mut()
                    && state.selected_index != index
                {
                    state.selected_index = index;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_quick_search_task(&task_id, false, window, cx)
            }));
        if is_selected {
            row.bg(fade(theme.brand.primary, 0.1)).into_any_element()
        } else {
            row.hover(move |style| style.bg(hover_background))
                .into_any_element()
        }
    }

    fn remote_host_badges(&self, theme: &ThemeTokens, task: &SearchableTask) -> Option<AnyElement> {
        let host = task.ssh_host.clone().filter(|host| !host.is_empty())?;
        let remote = Translator::new(self.locale, "common").translate("remote");
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing(2.0))
                .child(
                    div()
                        .relative()
                        .rounded(radius::FULL)
                        .bg(theme.tag.gemini.background)
                        .px(spacing(2.0))
                        .py(px(2.0))
                        .text_scale(text_scale::PX_11)
                        .css_weight(500)
                        .text_color(theme.tag.gemini.text)
                        .child(remote.clone())
                        .child(qa_text_layer("status", &remote)),
                )
                .child(
                    div()
                        .relative()
                        .text_scale(text_scale::PX_11)
                        .text_color(theme.text.muted)
                        .child(host.clone())
                        .child(qa_text_layer("note", &host)),
                )
                .into_any_element(),
        )
    }
}

/// `mt-1 truncate text-xs text-text-muted` 보조 줄
fn secondary_line(theme: &ThemeTokens, text: &str, indices: Option<&[usize]>) -> impl IntoElement {
    div()
        .mt(spacing(1.0))
        .truncate()
        .text_scale(text_scale::XS)
        .text_color(theme.text.muted)
        .child(match_text(theme, text, indices))
}

/// 일치한 글자가 있으면 TS `HighlightedText`(`font-mono text-xs`, 일치 글자 `text-brand-primary font-bold`)로 그린다
fn match_text(theme: &ThemeTokens, text: &str, indices: Option<&[usize]>) -> AnyElement {
    let Some(indices) = indices else {
        return div().child(text.to_string()).into_any_element();
    };
    let highlight = HighlightStyle {
        color: Some(theme.brand.primary.into()),
        font_weight: Some(FontWeight::BOLD),
        ..HighlightStyle::default()
    };
    let ranges: Vec<(Range<usize>, HighlightStyle)> = highlighted_byte_ranges(text, indices)
        .into_iter()
        .map(|range| (range, highlight))
        .collect();
    div()
        .font_family(MONO_FAMILY)
        .text_scale(text_scale::XS)
        .child(StyledText::new(text.to_string()).with_highlights(ranges))
        .into_any_element()
}

/// 문자 위치 목록을 이어진 바이트 구간으로 묶는다
pub fn highlighted_byte_ranges(text: &str, indices: &[usize]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (char_index, (byte_index, character)) in text.char_indices().enumerate() {
        if !indices.contains(&char_index) {
            continue;
        }
        let end = byte_index + character.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == byte_index => last.end = end,
            _ => ranges.push(byte_index..end),
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_domain::task::TaskStatus;

    fn searchable(
        id: &str,
        title: &str,
        branch: Option<&str>,
        project: Option<&str>,
    ) -> SearchableTask {
        SearchableTask {
            id: id.into(),
            title: title.into(),
            branch_name: branch.map(str::to_string),
            project_id: project.map(|_| "p1".to_string()),
            project_name: project.map(str::to_string),
            ssh_host: None,
            status: TaskStatus::Todo,
            updated_at: "2026-01-15T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn empty_query_keeps_the_received_order() {
        let tasks = [
            searchable("a", "A", None, None),
            searchable("b", "B", None, None),
        ];
        let ids: Vec<String> = build_search_results(&tasks, "  ")
            .into_iter()
            .map(|result| result.task.id)
            .collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn branch_matches_outrank_title_matches_and_mark_title_characters() {
        let tasks = [
            searchable(
                "title",
                "보드 필터 추가",
                Some("feat/board-filters"),
                Some("kanvibe-demo"),
            ),
            searchable("branch", "다른 일", Some("feat/filter"), None),
        ];
        let results = build_search_results(&tasks, "필터");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task.id, "title");
        assert_eq!(results[0].title_indices.as_deref(), Some(&[3, 4][..]));
        assert_eq!(results[0].branch_indices, None);
    }

    #[test]
    fn a_token_can_split_across_two_fields() {
        let tasks = [searchable("t", "login", Some("fix"), None)];
        let results = build_search_results(&tasks, "fixlogin");
        assert_eq!(results.len(), 1);
        assert!(results[0].branch_indices.is_some());
        assert!(results[0].title_indices.is_some());
    }

    #[test]
    fn token_order_does_not_change_the_result() {
        let tasks = [searchable(
            "t",
            "로그인",
            Some("fix/login-redirect"),
            Some("api-server"),
        )];
        let forward = build_search_results(&tasks, "api login");
        let backward = build_search_results(&tasks, "login api");
        assert_eq!(forward.len(), 1);
        assert_eq!(backward.len(), 1);
        assert_eq!(forward[0].score, backward[0].score);
    }

    #[test]
    fn one_query_finds_project_and_branch_together() {
        let tasks = [
            searchable("hit", "로그인", Some("feat/push"), Some("mobile-app")),
            searchable("miss", "다른 일", Some("feat/push"), Some("api-server")),
        ];
        for query in ["mobile push", "mobilepush"] {
            let results = build_search_results(&tasks, query);
            assert_eq!(results.len(), 1, "{query}");
            assert_eq!(results[0].task.id, "hit");
            assert!(results[0].project_indices.is_some());
            assert!(results[0].branch_indices.is_some());
        }
    }

    #[test]
    fn highlight_ranges_join_adjacent_characters() {
        assert_eq!(
            highlighted_byte_ranges("ab필터", &[0, 2, 3]),
            vec![0..1, 2..8]
        );
    }
}
