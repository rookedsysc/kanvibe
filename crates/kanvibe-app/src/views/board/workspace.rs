//! 워크스페이스 뷰(TS `TaskWorkspaceRoute` + `WorkspaceTaskList` + `WorkspaceTerminalPane`).
//! 왼쪽에 상태 순서로 작업을 늘어놓고 검색·종류·프로젝트·상태 필터와 보드 정렬을 함께 적용한다. 오른쪽은 고른 작업의 터미널이다.
//! 처음에는 세션이 있는 첫 작업을 고르고, 세션 없는 작업은 사용자가 고를 때만 자동 연결한다.

use std::collections::HashSet;

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::board_sort::BoardSortPreference;
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::shortcuts::key_event::{
    KeyPhase, KeyTransition, ModifierState, ShortcutKeyEvent,
};
use kanvibe_domain::shortcuts::workspace_focus::WorkspaceListFocusTracker;
use kanvibe_domain::task::{KanbanTask, TASK_STATUS_ORDER, TaskStatus};
use kanvibe_services::kanban::TasksByStatus;

use super::BoardView;
use super::board_model::{ProjectCatalog, column, root_priorities};
use super::column::{status_dot_color, status_label};
use super::header::{HeaderLabels, qa_button_layer, qa_text_layer};
use super::task_card::TaskCardInput;
use super::task_filters::{
    BoardSortContext, TaskKindFilter, build_project_filter_set, matches_project_filter,
    matches_task_kind_filter, sort_tasks_for_board,
};
use crate::components::{TailwindText, fade};
use crate::text_collation::LocaleCollator;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// xterm 테마 배경(`Terminal.tsx` theme.background). 앱 테마 토큰이 아니라 터미널 고정색이다
const XTERM_BACKGROUND: u32 = 0x0a0a0a;

/// 워크스페이스 왼쪽 열 폭(`w-80`)
const TASK_LIST_WIDTH: f32 = 320.0;

/// 세션이 붙은 작업인지
pub(super) fn has_terminal_session(task: &KanbanTask) -> bool {
    task.session_type.is_some() && task.session_name.is_some()
}

/// 처음 고를 작업: 상태 순서대로 세션이 있는 첫 작업. 세션 없는 작업을 고르면 화면을 연 것만으로 세션이 생긴다
pub(super) fn initial_task_id(tasks: &TasksByStatus) -> Option<String> {
    TASK_STATUS_ORDER
        .into_iter()
        .flat_map(|status| column(tasks, status))
        .find(|task| has_terminal_session(task))
        .map(|task| task.id.clone())
}

/// 목록 거르기 조건
pub(super) struct WorkspaceListQuery<'a> {
    pub search: &'a str,
    pub statuses: &'a HashSet<TaskStatus>,
    pub task_kind: TaskKindFilter,
    pub selected_project_ids: &'a [String],
    pub sort_preference: &'a BoardSortPreference,
    pub collator: &'a LocaleCollator,
    pub selected_task_id: Option<&'a str>,
    /// 필터를 바꾼 사용자가 숨긴 선택. 필터 밖 선택을 맨 위에 붙이지 않는다
    pub hidden_selection_id: Option<&'a str>,
}

fn matches_search(task: &KanbanTask, query: &str, catalog: &ProjectCatalog) -> bool {
    if query.is_empty() {
        return true;
    }
    let project_name = task
        .project_id
        .as_ref()
        .and_then(|id| catalog.names.get(id))
        .cloned()
        .unwrap_or_default();
    let searchable = format!(
        "{} {} {}",
        task.title,
        task.branch_name.as_deref().unwrap_or_default(),
        project_name
    );
    searchable.to_lowercase().contains(query)
}

/// 왼쪽 목록에 늘어놓을 작업. 상태 순서 안에서 정렬 설정을 따르고, 필터 밖으로 밀린 선택은 맨 위에 둔다
pub(super) fn workspace_listed_tasks(
    tasks: &TasksByStatus,
    catalog: &ProjectCatalog,
    query: &WorkspaceListQuery,
) -> Vec<KanbanTask> {
    let normalized_search = query.search.trim().to_lowercase();
    let filter_set = build_project_filter_set(query.selected_project_ids, &catalog.projects);
    let roots = root_priorities(tasks, catalog);
    let context = BoardSortContext {
        root_priorities: &roots,
        project_names: &catalog.names,
        collator: query.collator,
    };
    let filtered: Vec<KanbanTask> = TASK_STATUS_ORDER
        .into_iter()
        .filter(|status| query.statuses.is_empty() || query.statuses.contains(status))
        .flat_map(|status| {
            let matching: Vec<KanbanTask> = column(tasks, status)
                .iter()
                .filter(|task| {
                    matches_project_filter(task, filter_set.as_ref())
                        && matches_task_kind_filter(task, query.task_kind, &catalog.by_id)
                        && matches_search(task, &normalized_search, catalog)
                })
                .cloned()
                .collect();
            sort_tasks_for_board(&matching, query.sort_preference, &context)
        })
        .collect();
    let selected = query.selected_task_id.and_then(|selected_id| {
        TASK_STATUS_ORDER
            .into_iter()
            .flat_map(|status| column(tasks, status))
            .find(|task| task.id == selected_id)
    });
    match selected {
        Some(selected)
            if query.hidden_selection_id != Some(selected.id.as_str())
                && !filtered.iter().any(|task| task.id == selected.id) =>
        {
            std::iter::once(selected.clone()).chain(filtered).collect()
        }
        _ => filtered,
    }
}

/// ↑↓ 다음 선택. 경계에서 멈춘다
pub(super) fn next_workspace_selection(
    listed: &[KanbanTask],
    current: Option<&str>,
    is_up: bool,
) -> Option<String> {
    let index = current
        .and_then(|id| listed.iter().position(|task| task.id == id))
        .unwrap_or(0);
    let next = if is_up {
        index.saturating_sub(1)
    } else {
        (index + 1).min(listed.len().saturating_sub(1))
    };
    listed.get(next).map(|task| task.id.clone())
}

fn modifier_state(modifiers: gpui_kit::Modifiers) -> ModifierState {
    ModifierState {
        has_meta: modifiers.platform,
        has_ctrl: modifiers.control,
        has_alt: modifiers.alt,
        has_shift: modifiers.shift,
    }
}

/// 수식키 상태 변화를 브라우저 `keydown`·`keyup`처럼 키 하나씩의 누름·뗌으로 바꾼다. 각 입력의 수식키는 바뀐 뒤 상태다
fn modifier_transitions(
    previous: gpui_kit::Modifiers,
    current: gpui_kit::Modifiers,
) -> Vec<KeyTransition> {
    let keys = [
        ("Control", previous.control, current.control),
        ("Alt", previous.alt, current.alt),
        ("Meta", previous.platform, current.platform),
        ("Shift", previous.shift, current.shift),
    ];
    keys.into_iter()
        .filter(|(_, was_pressed, is_pressed)| was_pressed != is_pressed)
        .map(|(key, _, is_pressed)| KeyTransition {
            phase: if is_pressed {
                KeyPhase::Down
            } else {
                KeyPhase::Up
            },
            is_auto_repeat: false,
            is_composing: false,
            event: ShortcutKeyEvent {
                key: key.to_string(),
                modifiers: modifier_state(current),
            },
        })
        .collect()
}

/// 워크스페이스 화면 상태
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WorkspaceState {
    pub selected_task_id: Option<String>,
    pub hidden_selection_id: Option<String>,
    pub statuses: HashSet<TaskStatus>,
    /// 세션 없는 작업을 고르면 연결을 시도한다. 그 작업 ID와 실패 사유
    pub connecting_task_id: Option<String>,
    pub connect_failure: Option<(String, String)>,
}

impl BoardView {
    fn workspace_listed(&self, cx: &gpui_kit::App) -> Vec<KanbanTask> {
        let Some(board) = &self.board else {
            return Vec::new();
        };
        let search = self.workspace_search.read(cx).text().to_string();
        let collator = LocaleCollator::for_locale(self.locale());
        let query = WorkspaceListQuery {
            search: &search,
            statuses: &self.workspace.statuses,
            task_kind: self.filters.task_kind,
            selected_project_ids: &self.filters.selected_project_ids,
            sort_preference: &board.sort_preference,
            collator: &collator,
            selected_task_id: self.workspace.selected_task_id.as_deref(),
            hidden_selection_id: self.workspace.hidden_selection_id.as_deref(),
        };
        workspace_listed_tasks(&board.tasks, &board.catalog, &query)
    }

    /// 처음 데이터를 받으면 쿼리(`?taskId=`)나 세션 있는 첫 작업을 고른다
    pub(super) fn choose_initial_workspace_task(&mut self) {
        if self.workspace.selected_task_id.is_some() {
            return;
        }
        let requested = super::query_value(&self.screen.route.search, "taskId").map(str::to_string);
        self.workspace.selected_task_id = requested.or_else(|| {
            self.board
                .as_ref()
                .and_then(|board| initial_task_id(&board.tasks))
        });
    }

    /// 워크스페이스에서 고른 작업
    pub fn selected_workspace_task_id(&self) -> Option<String> {
        self.workspace.selected_task_id.clone()
    }

    /// 빠른 검색처럼 밖에서 새로 고르라고 한 요청. 같은 작업이어도 필터로 숨긴 선택을 다시 보인다
    /// (TS `selectionRequestKey`가 바뀌면 `hiddenSelectionId`를 지운다)
    pub(crate) fn request_workspace_selection(&mut self, task_id: &str, cx: &mut Context<Self>) {
        self.workspace.hidden_selection_id = None;
        self.select_workspace_task(task_id, cx);
    }

    /// `Mod+Alt`만 눌렀다 떼면 왼쪽 목록에 포커스를 둔다(TS `createWorkspaceListFocusTracker`).
    /// 수식키 변화를 브라우저처럼 그 키의 누름·뗌으로 바꿔 공용 판정에 넘긴다
    fn track_workspace_list_focus(
        &mut self,
        event: &gpui_kit::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let platform = cx.global::<crate::shortcuts::ShortcutState>().platform;
        let previous = std::mem::replace(&mut self.workspace_modifiers, event.modifiers);
        let tracker = self
            .workspace_focus_tracker
            .get_or_insert_with(|| WorkspaceListFocusTracker::new(platform));
        let mut should_focus = false;
        for transition in modifier_transitions(previous, event.modifiers) {
            should_focus |= tracker.handle_key_transition(&transition);
        }
        let is_capturing = cx
            .global::<crate::shortcuts::ShortcutState>()
            .capturing_windows
            .contains(&window.window_handle().window_id());
        // 대화상자가 열렸거나 단축키를 녹화하는 중에는 포커스 요청을 버린다
        if is_capturing || self.has_shortcut_blocker(cx) {
            if let Some(tracker) = self.workspace_focus_tracker.as_mut() {
                tracker.reset();
            }
            return;
        }
        if should_focus {
            self.focus_workspace_list(window, cx);
        }
    }

    /// 다른 키가 끼면 `Mod+Alt` 단독 입력이 아니다. GPUI는 일반 키의 뗌을 따로 주지 않으므로,
    /// 수식키 없이 누른 키는 브라우저에서 그 키를 뗄 때처럼 판정을 처음으로 돌린다
    fn cancel_workspace_list_focus(&mut self, event: &KeyDownEvent) {
        let modifiers = event.keystroke.modifiers;
        let has_held_modifier =
            modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift;
        if !has_held_modifier {
            if let Some(tracker) = self.workspace_focus_tracker.as_mut() {
                tracker.reset();
            }
            return;
        }
        if let Some(tracker) = self.workspace_focus_tracker.as_mut() {
            let transition = KeyTransition {
                phase: KeyPhase::Down,
                is_auto_repeat: event.is_held,
                is_composing: false,
                event: ShortcutKeyEvent {
                    key: event.keystroke.key.clone(),
                    modifiers: modifier_state(event.keystroke.modifiers),
                },
            };
            tracker.handle_key_transition(&transition);
        }
    }

    /// 고른 작업(숨겨져 있으면 첫 표시 작업)을 고른 채 목록에 포커스를 둔다
    fn focus_workspace_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let listed = self.workspace_listed(cx);
        let selected = self.workspace.selected_task_id.clone();
        let is_selection_listed = listed
            .iter()
            .any(|task| Some(&task.id) == selected.as_ref());
        if !is_selection_listed && let Some(first) = listed.first() {
            let first_id = first.id.clone();
            self.select_workspace_task(&first_id, cx);
        }
        self.workspace_list_focus.focus(window, cx);
        cx.notify();
    }

    /// 작업을 고른다. 세션이 없으면 기본 세션 타입으로 연결을 시작한다
    pub(crate) fn select_workspace_task(&mut self, task_id: &str, cx: &mut Context<Self>) {
        self.workspace.selected_task_id = Some(task_id.to_string());
        self.workspace.connect_failure = None;
        let Some(board) = &self.board else {
            return;
        };
        let Some(task) = super::board_model::find_task(&board.tasks, task_id).cloned() else {
            return;
        };
        if !has_terminal_session(&task) && task.project_id.is_some() {
            self.connect_workspace_task(task.id, board.default_session_type, cx);
        }
        cx.notify();
    }

    fn connect_workspace_task(
        &mut self,
        task_id: String,
        session_type: kanvibe_domain::task::SessionType,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = self.services.clone() else {
            return;
        };
        if self.workspace.connecting_task_id.as_deref() == Some(task_id.as_str()) {
            return;
        }
        self.workspace.connecting_task_id = Some(task_id.clone());
        let connect = services.connect_terminal_session(task_id.clone(), session_type, cx);
        cx.spawn(async move |this, cx| {
            let result = connect.await;
            let _ = this.update(cx, |this, cx| {
                if this.workspace.connecting_task_id.as_deref() == Some(task_id.as_str()) {
                    this.workspace.connecting_task_id = None;
                }
                match result {
                    Ok(Some(connected)) => {
                        if let Some(board) = this.board.as_mut() {
                            for status in TASK_STATUS_ORDER {
                                if let Some(task) =
                                    super::board_model::column_mut(&mut board.tasks, status)
                                        .iter_mut()
                                        .find(|task| task.id == connected.id)
                                {
                                    *task = connected.clone();
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        this.workspace.connect_failure = Some((task_id.clone(), String::new()))
                    }
                    Err(failure) => {
                        this.workspace.connect_failure = Some((task_id.clone(), failure.reason))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn handle_workspace_list_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control
            || modifiers.alt
            || modifiers.platform
            || modifiers.shift
            || self.has_shortcut_blocker(cx)
        {
            return;
        }
        let listed = self.workspace_listed(cx);
        let current = self.workspace.selected_task_id.clone();
        match event.keystroke.key.as_str() {
            "up" | "down" => {
                if let Some(next) = next_workspace_selection(
                    &listed,
                    current.as_deref(),
                    event.keystroke.key == "up",
                ) {
                    self.select_workspace_task(&next, cx);
                }
            }
            // Enter는 상세 경로 대신 오른쪽 터미널로 포커스를 넘긴다
            "enter" => {
                if let Some(current) = current {
                    self.select_workspace_task(&current, cx);
                }
                self.workspace_terminal_focus.focus(window, cx);
            }
            "space" => {
                if let Some(current) = current {
                    self.select_workspace_task(&current, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn toggle_workspace_status(&mut self, status: TaskStatus, cx: &mut Context<Self>) {
        self.workspace.hidden_selection_id = self.workspace.selected_task_id.clone();
        if !self.workspace.statuses.remove(&status) {
            self.workspace.statuses.insert(status);
        }
        cx.notify();
    }

    pub(super) fn render_workspace(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = *cx.global::<ThemeTokens>();
        self.choose_initial_workspace_task();
        let list = self.render_workspace_list(&theme, window, cx);
        let pane = self.render_workspace_terminal_pane(&theme, cx);
        let overlays: Vec<AnyElement> = [
            self.render_workspace_refresh_banner(&theme, cx),
            self.render_board_dialogs(&theme, window, cx),
        ]
        .into_iter()
        .flatten()
        .collect();
        div()
            .relative()
            .size_full()
            .flex()
            .overflow_hidden()
            .bg(theme.background.page)
            .on_modifiers_changed(cx.listener(Self::track_workspace_list_focus))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, _| {
                this.cancel_workspace_list_focus(event)
            }))
            .child(list)
            .child(pane)
            .children(overlays)
    }

    fn render_workspace_refresh_banner(
        &mut self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let reason = self.load_status.failure_reason.clone()?;
        let message = Translator::new(self.locale(), "common")
            .translate_with("refreshFailed", &[("reason", &reason)]);
        Some(
            div()
                .absolute()
                .top(spacing(3.0))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    super::inline_load_error(
                        theme,
                        self.locale(),
                        message,
                        "workspace-refresh-retry",
                        cx.listener(|this, _, _, cx| this.retry_load(cx)),
                    )
                    .w(px(512.0)),
                )
                .into_any_element(),
        )
    }

    fn render_workspace_list(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let labels = Translator::new(self.locale(), "workspace");
        let header_labels = HeaderLabels::new(self.locale());
        let listed = self.workspace_listed(cx);
        let has_any_task = self.board.as_ref().is_some_and(|board| {
            TASK_STATUS_ORDER
                .into_iter()
                .any(|status| !column(&board.tasks, status).is_empty())
        });
        let cards: Vec<AnyElement> = listed
            .iter()
            .map(|task| self.render_workspace_card(theme, task, cx))
            .collect();
        let empty = listed.is_empty().then(|| {
            div()
                .relative()
                .px(px(10.0))
                .py(spacing(6.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.muted)
                .child(labels.translate(if has_any_task {
                    "noMatchingTasks"
                } else {
                    "emptyTasks"
                }))
                .child(qa_text_layer(
                    "status",
                    &labels.translate(if has_any_task {
                        "noMatchingTasks"
                    } else {
                        "emptyTasks"
                    }),
                ))
        });
        let load_more = self.render_workspace_load_more(theme, &labels, cx);
        let filters = self.render_workspace_filters(theme, &labels, &header_labels, window, cx);
        div()
            .flex()
            .flex_col()
            .h_full()
            .w(px(TASK_LIST_WIDTH))
            .flex_none()
            .border_r_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .child(filters)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(crate::components::scroll_area::scroll_area(
                        "workspace-task-list",
                        &self.workspace_scroll,
                        theme.border.strong.into(),
                        div()
                            .id("workspace-list-keys")
                            .track_focus(&self.workspace_list_focus)
                            .on_key_down(cx.listener(Self::handle_workspace_list_key))
                            .p(px(6.0))
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .children(cards)
                            .children(empty)
                            .children(load_more),
                    )),
            )
    }

    fn render_workspace_filters(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        header_labels: &HeaderLabels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let search_placeholder = labels.translate("searchPlaceholder");
        let is_search_focused =
            gpui_kit::Focusable::focus_handle(self.workspace_search.read(cx), cx)
                .is_focused(window);
        let search = div()
            .relative()
            .h(spacing(8.0))
            .w_full()
            .flex()
            .items_center()
            .rounded(radius::MD)
            .border_1()
            .border_color(if is_search_focused {
                theme.brand.primary
            } else {
                theme.border.default
            })
            .bg(theme.background.page)
            .px(px(10.0))
            .text_scale(text_scale::SM)
            .child(self.workspace_search.clone())
            .child(qa_text_layer("searchbox", &search_placeholder));
        let sort = self
            .render_sort_button(theme, header_labels, cx)
            .into_any_element();
        let project_filter = self
            .render_project_filter(theme, header_labels, true, cx)
            .into_any_element();
        let active_filter = self.filters.task_kind;
        let segment: Vec<_> = TaskKindFilter::ORDER
            .into_iter()
            .map(|filter| {
                let label = header_labels
                    .board
                    .translate(&format!("taskKindFilter.options.{}", filter.as_str()));
                let is_active = filter == active_filter;
                let button = div()
                    .id(gpui_kit::SharedString::from(format!(
                        "workspace-kind-{}",
                        filter.as_str()
                    )))
                    .relative()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .rounded(radius::BASE)
                    .text_scale(text_scale::XS)
                    .css_weight(500)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.workspace.hidden_selection_id =
                            this.workspace.selected_task_id.clone();
                        this.set_task_kind_filter(filter, cx)
                    }))
                    .child(label.clone())
                    .child(qa_button_layer(&label, &label));
                if is_active {
                    button
                        .bg(theme.brand.primary)
                        .text_color(theme.text.inverse)
                } else {
                    button.text_color(theme.text.secondary)
                }
            })
            .collect();
        let column_labels = Translator::new(self.locale(), "board.columns");
        let statuses: Vec<_> = TASK_STATUS_ORDER
            .into_iter()
            .map(|status| {
                let label = status_label(&column_labels, status);
                let is_active = self.workspace.statuses.contains(&status);
                let chip = div()
                    .id(gpui_kit::SharedString::from(format!(
                        "workspace-status-{}",
                        status.as_str()
                    )))
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(radius::FULL)
                    .px(px(10.0))
                    .py(spacing(1.0))
                    .text_scale(text_scale::XS)
                    .css_weight(500)
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_workspace_status(status, cx)),
                    )
                    .child(
                        div()
                            .size(spacing(2.0))
                            .rounded(radius::FULL)
                            .bg(status_dot_color(theme, status)),
                    )
                    .child(label.clone())
                    .child(qa_button_layer(&label, &label));
                if is_active {
                    chip.bg(theme.brand.subtle)
                        .text_color(theme.text.primary)
                        .border_1()
                        .border_color(theme.border.brand)
                } else {
                    chip.text_color(theme.text.secondary)
                }
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .border_b_1()
            .border_color(theme.border.default)
            .p(spacing(3.0))
            .child(search)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing(2.0))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .h(spacing(8.0))
                            .rounded(radius::MD)
                            .border_1()
                            .border_color(theme.border.default)
                            .bg(theme.background.page)
                            .p(px(2.0))
                            .children(segment),
                    )
                    .child(sort),
            )
            .child(project_filter)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(spacing(1.0))
                    .children(statuses),
            )
    }

    fn render_workspace_card(
        &mut self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(board) = &self.board else {
            return div().into_any_element();
        };
        let roots = root_priorities(&board.tasks, &board.catalog);
        let unread = board.unread_count_by_task();
        let display = task
            .project_id
            .as_ref()
            .and_then(|id| board.catalog.displays.get(id))
            .cloned();
        let diff_stats = board.diff_stats.get(&task.id).cloned();
        let input = TaskCardInput {
            task,
            project: display.as_ref(),
            unread_count: unread.get(&task.id).copied().unwrap_or(0),
            root_priorities: &roots,
            diff_stats: diff_stats.as_ref(),
            deletion_state: None,
            move_state: None,
        };
        let content = self.task_card_content(theme, &input);
        let is_selected = self.workspace.selected_task_id.as_deref() == Some(task.id.as_str());
        let is_base = task
            .worktree_path
            .as_deref()
            .is_some_and(|path| !path.contains("__worktrees"));
        let task_id = task.id.clone();
        let hover = theme.background.page;
        let card = div()
            .id(gpui_kit::SharedString::from(format!(
                "workspace-card-{}",
                task.id
            )))
            .relative()
            .w_full()
            .rounded(radius::MD)
            .px(px(10.0))
            .py(spacing(2.0))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_workspace_task(&task_id, cx);
                this.workspace_list_focus.focus(window, cx);
            }))
            .children(content);
        let card = if is_base { card.pr(spacing(8.0)) } else { card };
        let card = if is_selected {
            card.bg(theme.brand.subtle)
        } else {
            card.hover(move |style| style.bg(hover))
        };
        let card = if is_selected {
            card.child(qa_text_layer("current", &task.title))
        } else {
            card
        };
        card.into_any_element()
    }

    fn render_workspace_load_more(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(board) = &self.board else {
            return Vec::new();
        };
        if !board.done.has_more() {
            return Vec::new();
        }
        let label = labels.translate("loadMoreDone");
        let mut elements = vec![
            div()
                .id("workspace-load-more")
                .relative()
                .mt(spacing(1.0))
                .w_full()
                .rounded(radius::MD)
                .px(px(10.0))
                .py(spacing(2.0))
                .text_scale(text_scale::XS)
                .css_weight(500)
                .text_color(theme.brand.primary)
                .cursor_pointer()
                .when_loading(board.done.is_loading())
                .on_click(cx.listener(|this, _, _, cx| this.load_more_done(cx)))
                .child(label.clone())
                .child(qa_button_layer(&label, &label))
                .into_any_element(),
        ];
        if let Some(reason) = board.done.error.clone() {
            let message = Translator::new(self.locale(), "common")
                .translate_with("loadFailed", &[("reason", &reason)]);
            elements.push(
                super::inline_load_error(
                    theme,
                    self.locale(),
                    message,
                    "workspace-load-more-retry",
                    cx.listener(|this, _, _, cx| this.load_more_done(cx)),
                )
                .mt(spacing(1.0))
                .into_any_element(),
            );
        }
        elements
    }

    fn render_workspace_terminal_pane(
        &mut self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let labels = Translator::new(self.locale(), "workspace");
        let detail_labels = Translator::new(self.locale(), "taskDetail");
        let selected = self.workspace.selected_task_id.as_ref().and_then(|id| {
            self.board
                .as_ref()
                .and_then(|board| super::board_model::find_task(&board.tasks, id))
                .cloned()
        });
        let Some(task) = selected else {
            let message = labels.translate("noTaskSelected");
            return div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .justify_center()
                .bg(theme.background.page)
                .child(
                    div()
                        .relative()
                        .text_scale(text_scale::SM)
                        .text_color(theme.text.muted)
                        .child(message.clone())
                        .child(qa_text_layer("status", &message)),
                )
                .into_any_element();
        };
        let project_name = task
            .project_id
            .as_ref()
            .and_then(|id| {
                self.board
                    .as_ref()
                    .and_then(|board| board.catalog.by_id.get(id))
            })
            .map(|project| project.name.clone());
        let body: AnyElement = if has_terminal_session(&task) {
            // 터미널 화면은 작업 상세와 같은 공용 터미널 뷰가 그린다(공용 컴포넌트 요청 중). 그 전까지는 터미널 배경만 둔다
            div()
                .flex_1()
                .bg(gpui_kit::rgb(XTERM_BACKGROUND))
                .track_focus(&self.workspace_terminal_focus)
                .into_any_element()
        } else {
            let message = match (&self.workspace.connect_failure, task.project_id.is_some()) {
                (_, false) => detail_labels.translate("noTerminal"),
                (Some((failed_id, reason)), true) if *failed_id == task.id => {
                    if reason.is_empty() {
                        detail_labels.translate("connectFailed")
                    } else {
                        reason.clone()
                    }
                }
                _ => detail_labels.translate("connecting"),
            };
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .bg(theme.background.page)
                .child(
                    div()
                        .relative()
                        .text_scale(text_scale::SM)
                        .text_color(theme.text.muted)
                        .child(message.clone())
                        .child(qa_text_layer("status", &message)),
                )
                .into_any_element()
        };
        let _ = cx;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .bg(theme.terminal.background)
            .child(
                div()
                    .flex()
                    .h(spacing(10.0))
                    .flex_none()
                    .items_center()
                    .gap(spacing(3.0))
                    .border_b_1()
                    .border_color(fade(gpui_kit::rgb(0xffffff), 0.1))
                    .px(spacing(3.0))
                    .text_color(theme.terminal.text)
                    .child(
                        div()
                            .flex()
                            .text_scale(text_scale::SM)
                            .child(div().css_weight(500).child(task.title.clone()))
                            .children(
                                project_name
                                    .map(|name| div().opacity(0.6).child(format!(" · {name}"))),
                            ),
                    ),
            )
            .child(body)
            .into_any_element()
    }
}

trait LoadingStyle: Styled + Sized {
    fn when_loading(self, is_loading: bool) -> Self {
        if is_loading { self.opacity(0.6) } else { self }
    }
}

impl<E: Styled> LoadingStyle for E {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::board::board_model::column_mut;
    use crate::views::board::task_filters::tests::{project, task};
    use kanvibe_domain::board_sort::{BoardSortDirection, BoardSortField, BoardSortKey};
    use kanvibe_domain::task::{SessionType, TaskPriority};

    fn board(entries: &[(TaskStatus, KanbanTask)]) -> TasksByStatus {
        let mut tasks = TasksByStatus::default();
        for (status, entry) in entries {
            column_mut(&mut tasks, *status).push(KanbanTask {
                status: *status,
                ..entry.clone()
            });
        }
        tasks
    }

    fn listed(
        tasks: &TasksByStatus,
        catalog: &ProjectCatalog,
        edit: impl FnOnce(&mut WorkspaceListQuery),
    ) -> Vec<String> {
        let statuses = HashSet::new();
        let preference = BoardSortPreference::default();
        let collator = LocaleCollator::for_locale(kanvibe_domain::i18n::Locale::Ko);
        let mut query = WorkspaceListQuery {
            search: "",
            statuses: &statuses,
            task_kind: TaskKindFilter::All,
            selected_project_ids: &[],
            sort_preference: &preference,
            collator: &collator,
            selected_task_id: None,
            hidden_selection_id: None,
        };
        edit(&mut query);
        workspace_listed_tasks(tasks, catalog, &query)
            .into_iter()
            .map(|task| task.id)
            .collect()
    }

    fn sample() -> (TasksByStatus, ProjectCatalog) {
        let catalog = ProjectCatalog::new(vec![project("demo")]);
        let tasks = board(&[
            (TaskStatus::Done, task("done")),
            (
                TaskStatus::Todo,
                KanbanTask {
                    title: "Login fix".into(),
                    branch_name: Some("fix/login".into()),
                    ..task("todo")
                },
            ),
            (
                TaskStatus::Review,
                KanbanTask {
                    project_id: Some("demo".into()),
                    branch_name: Some("main".into()),
                    ..task("root")
                },
            ),
        ]);
        (tasks, catalog)
    }

    // parity: TS-renderer-WorkspaceTaskList-e41ec2
    #[test]
    fn without_filters_all_tasks_including_done_are_listed_in_status_order() {
        let (tasks, catalog) = sample();
        assert_eq!(listed(&tasks, &catalog, |_| {}), ["todo", "root", "done"]);
    }

    // parity: TS-renderer-WorkspaceTaskList-a9b506
    #[test]
    fn search_matches_title_branch_and_project_name_ignoring_case() {
        let (tasks, catalog) = sample();
        assert_eq!(
            listed(&tasks, &catalog, |query| query.search = "LOGIN"),
            ["todo"]
        );
        assert_eq!(
            listed(&tasks, &catalog, |query| query.search = "fix/"),
            ["todo"]
        );
        assert_eq!(
            listed(&tasks, &catalog, |query| query.search = "demo"),
            ["root"]
        );
    }

    // parity: TS-renderer-WorkspaceTaskList-8cb6b5
    #[test]
    fn status_chips_combine_and_toggle() {
        let (tasks, catalog) = sample();
        let statuses = HashSet::from([TaskStatus::Todo, TaskStatus::Done]);
        let ids = {
            let preference = BoardSortPreference::default();
            let collator = LocaleCollator::for_locale(kanvibe_domain::i18n::Locale::Ko);
            let query = WorkspaceListQuery {
                search: "",
                statuses: &statuses,
                task_kind: TaskKindFilter::All,
                selected_project_ids: &[],
                sort_preference: &preference,
                collator: &collator,
                selected_task_id: None,
                hidden_selection_id: None,
            };
            workspace_listed_tasks(&tasks, &catalog, &query)
                .into_iter()
                .map(|task| task.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids, ["todo", "done"]);
    }

    // parity: TS-renderer-WorkspaceTaskList-ba72b6
    #[test]
    fn project_filter_follows_the_kanban_rules() {
        let (tasks, catalog) = sample();
        let selected = ["demo".to_string()];
        let statuses = HashSet::new();
        let preference = BoardSortPreference::default();
        let collator = LocaleCollator::for_locale(kanvibe_domain::i18n::Locale::Ko);
        let query = WorkspaceListQuery {
            search: "",
            statuses: &statuses,
            task_kind: TaskKindFilter::All,
            selected_project_ids: &selected,
            sort_preference: &preference,
            collator: &collator,
            selected_task_id: None,
            hidden_selection_id: None,
        };
        assert_eq!(
            workspace_listed_tasks(&tasks, &catalog, &query)
                .into_iter()
                .map(|task| task.id)
                .collect::<Vec<_>>(),
            ["root"]
        );
    }

    // parity: TS-renderer-WorkspaceTaskList-e0f5b9
    #[test]
    fn project_kind_keeps_only_default_branch_tasks() {
        let (tasks, catalog) = sample();
        assert_eq!(
            listed(&tasks, &catalog, |query| query.task_kind =
                TaskKindFilter::Project),
            ["root"]
        );
    }

    // parity: TS-renderer-WorkspaceTaskList-869215
    #[test]
    fn sort_preference_orders_tasks_within_each_status() {
        let catalog = ProjectCatalog::default();
        let tasks = board(&[
            (
                TaskStatus::Todo,
                KanbanTask {
                    priority: Some(TaskPriority::Low),
                    ..task("low")
                },
            ),
            (
                TaskStatus::Todo,
                KanbanTask {
                    priority: Some(TaskPriority::High),
                    ..task("high")
                },
            ),
            (
                TaskStatus::Review,
                KanbanTask {
                    priority: Some(TaskPriority::High),
                    ..task("review")
                },
            ),
        ]);
        let preference = BoardSortPreference {
            keys: vec![BoardSortKey {
                field: BoardSortField::Priority,
                direction: BoardSortDirection::Asc,
            }],
        };
        let statuses = HashSet::new();
        let collator = LocaleCollator::for_locale(kanvibe_domain::i18n::Locale::Ko);
        let query = WorkspaceListQuery {
            search: "",
            statuses: &statuses,
            task_kind: TaskKindFilter::All,
            selected_project_ids: &[],
            sort_preference: &preference,
            collator: &collator,
            selected_task_id: None,
            hidden_selection_id: None,
        };
        let ids: Vec<_> = workspace_listed_tasks(&tasks, &catalog, &query)
            .into_iter()
            .map(|task| task.id)
            .collect();
        assert_eq!(ids, ["high", "low", "review"]);
    }

    // parity: TS-renderer-WorkspaceTaskList-914a90
    #[test]
    fn unmatched_filters_leave_an_empty_list() {
        let (tasks, catalog) = sample();
        assert!(listed(&tasks, &catalog, |query| query.search = "zzzz").is_empty());
    }

    // parity: TS-renderer-WorkspaceTaskList-f083a1
    #[test]
    fn arrow_selection_follows_the_listed_order_and_stops_at_the_edges() {
        let (tasks, catalog) = sample();
        let statuses = HashSet::new();
        let preference = BoardSortPreference::default();
        let collator = LocaleCollator::for_locale(kanvibe_domain::i18n::Locale::Ko);
        let query = WorkspaceListQuery {
            search: "",
            statuses: &statuses,
            task_kind: TaskKindFilter::All,
            selected_project_ids: &[],
            sort_preference: &preference,
            collator: &collator,
            selected_task_id: None,
            hidden_selection_id: None,
        };
        let listed = workspace_listed_tasks(&tasks, &catalog, &query);
        assert_eq!(
            next_workspace_selection(&listed, Some("todo"), false).as_deref(),
            Some("root")
        );
        assert_eq!(
            next_workspace_selection(&listed, Some("done"), false).as_deref(),
            Some("done")
        );
        assert_eq!(
            next_workspace_selection(&listed, Some("todo"), true).as_deref(),
            Some("todo")
        );
    }

    // parity: TS-renderer-WorkspaceTaskList-f0ffbe
    #[test]
    fn selected_task_stays_visible_outside_the_filter_until_the_user_hides_it() {
        let (tasks, catalog) = sample();
        assert_eq!(
            listed(&tasks, &catalog, |query| {
                query.search = "demo";
                query.selected_task_id = Some("todo");
            }),
            ["todo", "root"]
        );
        assert_eq!(
            listed(&tasks, &catalog, |query| {
                query.search = "demo";
                query.selected_task_id = Some("todo");
                query.hidden_selection_id = Some("todo");
            }),
            ["root"]
        );
    }

    // parity: TS-renderer-TaskWorkspaceRoute-66ed1a
    #[test]
    fn initial_selection_is_the_first_task_with_a_session() {
        let tasks = board(&[
            (TaskStatus::Todo, task("no-session")),
            (
                TaskStatus::Review,
                KanbanTask {
                    session_type: Some(SessionType::Tmux),
                    session_name: Some("s".into()),
                    ..task("with-session")
                },
            ),
        ]);
        assert_eq!(initial_task_id(&tasks).as_deref(), Some("with-session"));
        assert_eq!(
            initial_task_id(&board(&[(TaskStatus::Todo, task("no-session"))])),
            None
        );
    }
}
