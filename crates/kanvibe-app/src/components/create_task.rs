//! 새 작업 모달(TS `CreateTaskModal` + `BranchSearchInput` + `PrioritySelector` + `ProjectBranchesStatusNotice`).
//! 프로젝트를 고르면 브랜치 목록을 읽고 기본 브랜치를 베이스로 채운다. 만들면 서비스가 worktree·세션까지 준비하고,
//! 끝나면 만든 작업 상세로 간다. 만드는 동안 닫으면 결과를 토스트로 알린다(띄운 화면이 받는다).
//! 보드와 작업 상세(새 작업 단축키)가 함께 띄우므로 공용 부품에 둔다.

use std::sync::Arc;

use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    ScrollHandle, StatefulInteractiveElement, Styled, Subscription, Task, Window, deferred, div,
    px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{SessionType, TaskPriority};
use kanvibe_process::project_search::{fuzzy_match, matches_project_search};
use kanvibe_services::kanban::CreateTaskInput;

use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::components::session_dependency_prompt::ensure_session_dependency;
use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_button_layer, qa_target_layer, qa_text_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::board::services::{BoardServiceFailure, BoardServices, BranchFromTaskRequest};
use crate::views::board::text_field::{BoardTextField, TextFieldEvent};

/// 모달이 부모에게 알리는 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateTaskEvent {
    /// 사용자가 닫았다. 만드는 중이면 결과는 `Finished*`로 다시 온다
    Closed,
    /// 모달이 열린 채 마쳤다. 새 작업이면 그 상세로 가고, 분기면 모달만 닫는다
    Created {
        task_id: String,
        should_navigate: bool,
    },
    /// 닫은 뒤 마쳤다. 토스트 문구와 열 작업
    FinishedInBackground {
        task_id: Option<String>,
        message: String,
    },
    FailedInBackground {
        message: String,
    },
}

/// 새 작업인지, 기존 작업에서 브랜치를 분기하는지(TS `BranchTaskModal`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateTaskMode {
    Create,
    Branch { task_id: String, task_title: String },
}

/// 브랜치 목록 조회 상태(TS `useProjectBranches`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchesState {
    Idle,
    Loading,
    Loaded(Vec<String>),
    Failed(String),
}

/// 열 때 미리 채울 값(브랜치 기반 TODO)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CreateTaskDefaults {
    pub project_id: Option<String>,
    pub base_branch: Option<String>,
}

pub struct CreateTaskModal {
    locale: Locale,
    mode: CreateTaskMode,
    services: Option<Arc<BoardServices>>,
    projects: Vec<Project>,
    selected_project_id: Option<String>,
    default_base_branch: Option<String>,
    base_branch: String,
    branches: BranchesState,
    is_branch_list_open: bool,
    branch_highlight: usize,
    is_project_list_open: bool,
    project_highlight: usize,
    /// 프로젝트 목록 스크롤(키보드로 고른 항목을 보이게 굴린다)
    project_options_scroll: ScrollHandle,
    priority: Option<TaskPriority>,
    session_type: SessionType,
    is_session_list_open: bool,
    error: Option<String>,
    is_submitting: bool,
    is_closed: bool,
    focus_handle: FocusHandle,
    project_search: Entity<BoardTextField>,
    base_branch_field: Entity<BoardTextField>,
    branch_name_field: Entity<BoardTextField>,
    description_field: Entity<BoardTextField>,
    _branch_load: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CreateTaskEvent> for CreateTaskModal {}

impl Focusable for CreateTaskModal {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// 브랜치 검색: 공백으로 나눈 낱말이 모두 브랜치(또는 프로젝트 이름)에 순서대로 나와야 한다. 점수 높은 순
pub fn filter_branches(
    branches: &[String],
    query: &str,
    project_name: Option<&str>,
) -> Vec<String> {
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return branches.to_vec();
    }
    let mut matches: Vec<(i64, &String)> = branches
        .iter()
        .filter_map(|branch| {
            let mut score = 0;
            for token in &tokens {
                let branch_match = fuzzy_match(token, branch);
                let project_match = project_name.and_then(|name| fuzzy_match(token, name));
                match (branch_match, project_match) {
                    (Some(found), _) => score += found.score,
                    (None, Some(_)) => {}
                    (None, None) => return None,
                }
            }
            Some((score, branch))
        })
        .collect();
    matches.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    matches
        .into_iter()
        .map(|(_, branch)| branch.clone())
        .collect()
}

fn session_type_label(session_type: SessionType) -> &'static str {
    session_type.as_str()
}

impl CreateTaskModal {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        locale: Locale,
        mode: CreateTaskMode,
        services: Option<Arc<BoardServices>>,
        projects: Vec<Project>,
        defaults: CreateTaskDefaults,
        default_session_type: SessionType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = *cx.global::<ThemeTokens>();
        let labels = Translator::new(locale, "task");
        let colors = [theme.text.primary, theme.text.muted, theme.text.primary];
        let project_search =
            cx.new(|cx| BoardTextField::new(labels.translate("projectSearch"), colors, cx));
        let base_branch_field = cx
            .new(|cx| BoardTextField::new(labels.translate("branchSearchPlaceholder"), colors, cx));
        let branch_name_field = cx.new(|cx| BoardTextField::new("", colors, cx));
        let description_field = cx
            .new(|cx| BoardTextField::new(labels.translate("descriptionPlaceholder"), colors, cx));
        let subscriptions = vec![
            cx.subscribe_in(&project_search, window, Self::handle_project_search_event),
            cx.subscribe_in(&base_branch_field, window, Self::handle_base_branch_event),
            cx.subscribe_in(&branch_name_field, window, Self::handle_plain_field_event),
            cx.subscribe_in(&description_field, window, Self::handle_plain_field_event),
        ];
        let mut modal = Self {
            locale,
            mode: mode.clone(),
            services,
            projects: projects
                .into_iter()
                .filter(|project| !project.is_worktree)
                .collect(),
            selected_project_id: None,
            default_base_branch: defaults.base_branch.clone(),
            base_branch: String::new(),
            branches: BranchesState::Idle,
            is_branch_list_open: false,
            branch_highlight: 0,
            is_project_list_open: false,
            project_highlight: 0,
            project_options_scroll: ScrollHandle::new(),
            priority: None,
            session_type: default_session_type,
            is_session_list_open: false,
            error: None,
            is_submitting: false,
            is_closed: false,
            focus_handle: cx.focus_handle(),
            project_search,
            base_branch_field,
            branch_name_field,
            description_field,
            _branch_load: Task::ready(()),
            _subscriptions: subscriptions,
        };
        // 분기는 첫 프로젝트를 고른 채 연다
        let initial_project = match mode {
            CreateTaskMode::Create => defaults.project_id,
            CreateTaskMode::Branch { .. } => {
                modal.projects.first().map(|project| project.id.clone())
            }
        };
        match initial_project {
            Some(project_id) => {
                modal.select_project(&project_id, defaults.base_branch, window, cx);
            }
            None => modal.focus_handle.focus(window, cx),
        }
        modal
    }

    /// 부모가 모달을 치웠을 때 부른다. 진행 중인 만들기는 계속되고 결과는 토스트로 간다
    pub fn mark_closed(&mut self) {
        self.is_closed = true;
    }

    /// 지금 고른 베이스 브랜치
    pub fn base_branch(&self) -> &str {
        &self.base_branch
    }

    /// 베이스 브랜치·브랜치 이름·설명 입력에 지금 들어 있는 글자
    pub fn field_texts(&self, cx: &gpui_kit::App) -> [String; 3] {
        [
            &self.base_branch_field,
            &self.branch_name_field,
            &self.description_field,
        ]
        .map(|field| field.read(cx).text().to_string())
    }

    pub fn selected_project_id(&self) -> Option<&str> {
        self.selected_project_id.as_deref()
    }

    pub fn session_type(&self) -> SessionType {
        self.session_type
    }

    pub fn is_submitting(&self) -> bool {
        self.is_submitting
    }

    fn selected_project(&self) -> Option<&Project> {
        let id = self.selected_project_id.as_ref()?;
        self.projects.iter().find(|project| &project.id == id)
    }

    fn select_project(
        &mut self,
        project_id: &str,
        base_branch: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            return;
        };
        self.selected_project_id = Some(project.id.clone());
        self.is_project_list_open = false;
        self.base_branch = base_branch.unwrap_or(project.default_branch.clone());
        let base_branch = self.base_branch.clone();
        self.base_branch_field
            .update(cx, |field, cx| field.set_text(base_branch, cx));
        self.load_branches(cx);
        let focus = self.base_branch_field.focus_handle(cx);
        focus.focus(window, cx);
        cx.notify();
    }

    fn load_branches(&mut self, cx: &mut Context<Self>) {
        let (Some(services), Some(project_id)) =
            (self.services.clone(), self.selected_project_id.clone())
        else {
            return;
        };
        self.branches = BranchesState::Loading;
        let load = services.load_project_branches(project_id.clone(), cx);
        self._branch_load = cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                // 그 사이 다른 프로젝트를 골랐으면 이전 응답은 버린다
                if this.selected_project_id.as_deref() != Some(project_id.as_str()) {
                    return;
                }
                this.branches = match result {
                    Ok(branches) => BranchesState::Loaded(branches),
                    Err(failure) => BranchesState::Failed(failure.reason),
                };
                cx.notify();
            });
        });
    }

    /// 받아 온 목록에 없는 기본·현재 베이스 브랜치도 고를 수 있게 앞에 붙인다
    fn branch_options(&self) -> Vec<String> {
        let mut branches = match &self.branches {
            BranchesState::Loaded(branches) => branches.clone(),
            _ => Vec::new(),
        };
        for extra in [
            self.default_base_branch.clone(),
            Some(self.base_branch.clone()),
        ]
        .into_iter()
        .flatten()
        {
            if !extra.is_empty() && !branches.contains(&extra) {
                branches.insert(0, extra);
            }
        }
        branches
    }

    fn filtered_branches(&self, cx: &gpui_kit::App) -> Vec<String> {
        let query = self.base_branch_field.read(cx).text().to_string();
        let project_name = self.selected_project().map(|project| project.name.clone());
        filter_branches(&self.branch_options(), &query, project_name.as_deref())
    }

    fn filtered_projects(&self, cx: &gpui_kit::App) -> Vec<Project> {
        let query = self.project_search.read(cx).text().to_string();
        self.projects
            .iter()
            .filter(|project| matches_project_search(project, &query))
            .cloned()
            .collect()
    }

    fn handle_project_search_event(
        &mut self,
        _: &Entity<BoardTextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects = self.filtered_projects(cx);
        match event {
            TextFieldEvent::Changed(_) => self.project_highlight = 0,
            TextFieldEvent::NavigationKey(key) if key == "down" && !projects.is_empty() => {
                self.project_highlight = (self.project_highlight + 1) % projects.len();
                self.project_options_scroll
                    .scroll_to_item(self.project_highlight);
            }
            TextFieldEvent::NavigationKey(key) if key == "up" && !projects.is_empty() => {
                self.project_highlight =
                    (self.project_highlight + projects.len() - 1) % projects.len();
                self.project_options_scroll
                    .scroll_to_item(self.project_highlight);
            }
            TextFieldEvent::Submitted { .. } => {
                if let Some(project) =
                    projects.get(self.project_highlight.min(projects.len().saturating_sub(1)))
                {
                    let project_id = project.id.clone();
                    self.select_project(&project_id, None, window, cx);
                }
            }
            TextFieldEvent::Escaped => {
                self.is_project_list_open = false;
                self.focus_handle.focus(window, cx);
            }
            TextFieldEvent::NavigationKey(_) => {}
        }
        cx.notify();
    }

    fn handle_base_branch_event(
        &mut self,
        _: &Entity<BoardTextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let branches = self.filtered_branches(cx);
        match event {
            TextFieldEvent::Changed(_) => {
                self.is_branch_list_open = true;
                self.branch_highlight = 0;
            }
            TextFieldEvent::NavigationKey(key) if key == "down" => {
                self.is_branch_list_open = true;
                self.branch_highlight =
                    (self.branch_highlight + 1).min(branches.len().saturating_sub(1));
            }
            TextFieldEvent::NavigationKey(key) if key == "up" => {
                self.is_branch_list_open = true;
                self.branch_highlight = self.branch_highlight.saturating_sub(1);
            }
            TextFieldEvent::Submitted { .. } if self.is_branch_list_open => {
                if let Some(branch) = branches.get(self.branch_highlight).cloned() {
                    self.choose_branch(branch, cx);
                }
            }
            TextFieldEvent::Submitted { .. } => self.submit(window, cx),
            TextFieldEvent::Escaped if self.is_branch_list_open => self.restore_base_branch(cx),
            TextFieldEvent::Escaped => self.close(cx),
            // 목록이 열려 있어도 Tab은 다음 입력으로 간다(TS `BranchSearchInput`)
            TextFieldEvent::NavigationKey(key) if key == "tab" || key == "shift-tab" => {
                if self.is_branch_list_open {
                    self.restore_base_branch(cx);
                }
                self.move_field_focus(&self.base_branch_field.clone(), key == "tab", window, cx);
            }
            TextFieldEvent::NavigationKey(_) => {}
        }
        cx.notify();
    }

    fn handle_plain_field_event(
        &mut self,
        field: &Entity<BoardTextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Submitted {
                is_shift_pressed: false,
            } => self.submit(window, cx),
            TextFieldEvent::Escaped => self.close(cx),
            TextFieldEvent::NavigationKey(key) if key == "tab" || key == "shift-tab" => {
                self.move_field_focus(field, key == "tab", window, cx)
            }
            _ => {}
        }
    }

    /// 화면에 보이는 입력 순서(베이스 브랜치 → 브랜치 이름 → 설명)대로 Tab·Shift+Tab 포커스를 옮긴다
    fn move_field_focus(
        &mut self,
        from: &Entity<BoardTextField>,
        is_forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut fields = Vec::new();
        if self.selected_project_id.is_some() {
            fields.push(self.base_branch_field.clone());
        }
        fields.push(self.branch_name_field.clone());
        if self.mode == CreateTaskMode::Create {
            fields.push(self.description_field.clone());
        }
        let Some(position) = fields.iter().position(|field| field == from) else {
            return;
        };
        let target = if is_forward {
            fields.get(position + 1)
        } else {
            position.checked_sub(1).and_then(|index| fields.get(index))
        };
        if let Some(target) = target {
            target.focus_handle(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn choose_branch(&mut self, branch: String, cx: &mut Context<Self>) {
        self.base_branch = branch.clone();
        self.is_branch_list_open = false;
        self.base_branch_field
            .update(cx, |field, cx| field.set_text(branch, cx));
    }

    /// 목록에 없는 값을 두고 닫으면 이전 선택으로 돌린다
    fn restore_base_branch(&mut self, cx: &mut Context<Self>) {
        self.is_branch_list_open = false;
        let base_branch = self.base_branch.clone();
        self.base_branch_field
            .update(cx, |field, cx| field.set_text(base_branch, cx));
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(CreateTaskEvent::Closed);
    }

    /// 만들기를 보낸다. 응답을 기다리는 동안 Enter·버튼을 거듭 눌러도 한 번만 보낸다.
    /// Herdr 세션은 그 호스트에 도구가 준비된 것을 확인한 뒤에만 만든다(TS `ensureSessionDependencyWithPrompt`)
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let branch_name = self.branch_name_field.read(cx).text().trim().to_string();
        let Some(services) = self.services.clone() else {
            return;
        };
        if self.is_submitting {
            return;
        }
        let Some(project) = self
            .selected_project()
            .cloned()
            .filter(|_| !branch_name.is_empty())
        else {
            if matches!(self.mode, CreateTaskMode::Branch { .. }) {
                self.error =
                    Some(Translator::new(self.locale, "branch").translate("validationError"));
                cx.notify();
            }
            return;
        };
        self.is_submitting = true;
        self.error = None;
        let session_type = self.session_type;
        let locale = self.locale;
        cx.spawn_in(window, async move |this, cx| {
            if session_type == SessionType::Herdr {
                let readiness = ensure_session_dependency(
                    services,
                    session_type,
                    project.ssh_host.clone(),
                    locale,
                    cx,
                )
                .await;
                if readiness != Ok(true) {
                    let failure = readiness.err().map(BoardServiceFailure::new);
                    let _ = this.update(cx, |this, cx| this.stop_submit(failure, cx));
                    return;
                }
            }
            let Ok(request) =
                this.update(cx, |this, cx| this.send_request(&project, &branch_name, cx))
            else {
                return;
            };
            let result = request.await;
            let _ = this.update(cx, |this, cx| this.finish_submit(result, branch_name, cx));
        })
        .detach();
        cx.notify();
    }

    /// 지금 입력으로 만들기·분기 요청을 보낸다
    fn send_request(
        &mut self,
        project: &Project,
        branch_name: &str,
        cx: &mut Context<Self>,
    ) -> Task<Result<Option<String>, BoardServiceFailure>> {
        let Some(services) = self.services.clone() else {
            return Task::ready(Ok(None));
        };
        match &self.mode {
            CreateTaskMode::Create => {
                let description = self.description_field.read(cx).text().trim().to_string();
                services.create_task(
                    CreateTaskInput {
                        title: branch_name.to_string(),
                        description: (!description.is_empty()).then_some(description),
                        branch_name: Some(branch_name.to_string()),
                        base_branch: (!self.base_branch.is_empty())
                            .then(|| self.base_branch.clone()),
                        session_type: Some(self.session_type),
                        ssh_host: None,
                        project_id: Some(project.id.clone()),
                        priority: self.priority,
                    },
                    cx,
                )
            }
            CreateTaskMode::Branch { task_id, .. } => services.branch_from_task(
                BranchFromTaskRequest {
                    task_id: task_id.clone(),
                    project_id: project.id.clone(),
                    base_branch: self.base_branch.clone(),
                    branch_name: branch_name.to_string(),
                    session_type: self.session_type,
                },
                cx,
            ),
        }
    }

    /// 설치를 거절했거나 도구 준비가 실패해 만들지 않고 멈춘다
    fn stop_submit(&mut self, failure: Option<BoardServiceFailure>, cx: &mut Context<Self>) {
        match failure {
            Some(failure) => self.finish_submit(Err(failure), String::new(), cx),
            None => {
                self.is_submitting = false;
                cx.notify();
            }
        }
    }

    fn finish_submit(
        &mut self,
        result: Result<Option<String>, BoardServiceFailure>,
        branch_name: String,
        cx: &mut Context<Self>,
    ) {
        self.is_submitting = false;
        let is_branch = matches!(self.mode, CreateTaskMode::Branch { .. });
        let labels = Translator::new(self.locale, if is_branch { "branch" } else { "task" });
        let event = match (result, self.is_closed) {
            (Ok(task), false) => CreateTaskEvent::Created {
                task_id: task.unwrap_or_default(),
                should_navigate: !is_branch,
            },
            (Ok(task), true) => {
                let message = if is_branch {
                    labels.translate_with("succeeded", &[("branchName", &branch_name)])
                } else {
                    labels.translate_with("createSucceeded", &[("title", &branch_name)])
                };
                CreateTaskEvent::FinishedInBackground {
                    task_id: task,
                    message,
                }
            }
            (Err(failure), false) => {
                self.error = Some(failure.reason);
                cx.notify();
                return;
            }
            (Err(failure), true) => {
                let key = if is_branch {
                    "failedWithReason"
                } else {
                    "createFailedWithReason"
                };
                CreateTaskEvent::FailedInBackground {
                    message: labels.translate_with(key, &[("reason", &failure.reason)]),
                }
            }
        };
        cx.emit(event);
        cx.notify();
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                self.close(cx);
            }
            "enter"
                if self.focus_handle.is_focused(window) && self.selected_project_id.is_none() =>
            {
                self.is_project_list_open = true;
                let focus = self.project_search.focus_handle(cx);
                focus.focus(window, cx);
                cx.notify();
            }
            _ => {}
        }
    }
}

/// `block text-sm text-text-secondary mb-1`
fn field_label(theme: &ThemeTokens, label: String) -> impl IntoElement {
    div()
        .relative()
        .mb(spacing(1.0))
        .text_scale(text_scale::SM)
        .text_color(theme.text.secondary)
        .child(label.clone())
        .child(qa_text_layer("label", &label))
}

/// `w-full px-3 py-2 bg-bg-page border rounded-md` 입력칸 껍데기
fn input_box(theme: &ThemeTokens, is_focused: bool) -> gpui_kit::Div {
    div()
        .w_full()
        .px(spacing(3.0))
        .py(spacing(2.0))
        .bg(theme.background.page)
        .border_1()
        .border_color(if is_focused {
            theme.brand.primary
        } else {
            theme.border.default
        })
        .rounded(radius::MD)
        .text_scale(text_scale::BASE)
        .text_color(theme.text.primary)
}

impl Render for CreateTaskModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        let labels = Translator::new(self.locale, "task");
        let common = Translator::new(self.locale, "common");
        let mut form: Vec<AnyElement> = vec![
            self.render_project_field(&theme, &labels, window, cx)
                .into_any_element(),
        ];
        if self.selected_project_id.is_some() {
            form.push(
                self.render_base_branch_field(&theme, &labels, &common, window, cx)
                    .into_any_element(),
            );
        }
        let branch_focused = self.branch_name_field.focus_handle(cx).is_focused(window);
        let is_branch = matches!(self.mode, CreateTaskMode::Branch { .. });
        let branch_labels = Translator::new(self.locale, "branch");
        let branch_name_label = if is_branch {
            branch_labels.translate("newBranchName")
        } else {
            format!("{} *", labels.translate("branchName"))
        };
        form.push(
            div()
                .child(field_label(&theme, branch_name_label))
                .child(
                    input_box(&theme, branch_focused)
                        .h(px(39.0))
                        .flex()
                        .items_center()
                        .mono(400)
                        .child(self.branch_name_field.clone()),
                )
                .into_any_element(),
        );
        let description_focused = self.description_field.focus_handle(cx).is_focused(window);
        if !is_branch {
            form.push(
                div()
                    .child(field_label(&theme, labels.translate("descriptionLabel")))
                    .child(
                        input_box(&theme, description_focused)
                            .h(px(82.0))
                            .flex()
                            .items_start()
                            .child(self.description_field.clone()),
                    )
                    .into_any_element(),
            );
            form.push(
                self.render_priority_field(&theme, &labels, cx)
                    .into_any_element(),
            );
        }
        form.push(
            self.render_session_type_field(&theme, &labels, cx)
                .into_any_element(),
        );
        if let Some(error) = &self.error {
            form.push(
                div()
                    .relative()
                    .text_scale(text_scale::XS)
                    .text_color(theme.status.error)
                    .child(error.clone())
                    .child(qa_text_layer("paragraph", error))
                    .into_any_element(),
            );
        }
        form.push(self.render_actions(&theme, &common, cx).into_any_element());
        let title = match &self.mode {
            CreateTaskMode::Create => labels.translate("createTitle"),
            CreateTaskMode::Branch { .. } => branch_labels.translate("title"),
        };
        let subtitle = match &self.mode {
            CreateTaskMode::Create => None,
            CreateTaskMode::Branch { task_title, .. } => Some(
                div()
                    .mb(spacing(4.0))
                    .truncate()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.secondary)
                    .child(task_title.clone()),
            ),
        };
        div()
            .id("create-task-modal")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key))
            .occlude()
            .relative()
            .w(px(448.0))
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            .p(spacing(6.0))
            .child(
                div()
                    .relative()
                    .mb(spacing(if is_branch { 1.0 } else { 4.0 }))
                    .text_scale(text_scale::LG)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(title.clone())
                    .child(qa_text_layer("heading", &title)),
            )
            .children(subtitle)
            .child(div().flex().flex_col().gap(spacing(4.0)).children(form))
    }
}

impl CreateTaskModal {
    fn render_project_field(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let placeholder = labels.translate("projectSelect");
        let selected = self
            .selected_project()
            .map(|project| match &project.ssh_host {
                Some(host) => format!("{} ({host})", project.name),
                None => project.name.clone(),
            });
        // 모달을 열면 첫 입력인 프로젝트 선택에 포커스가 간다(`focus:border-brand-primary`)
        let is_trigger_focused = self.focus_handle.is_focused(window);
        let border = if self.is_project_list_open || is_trigger_focused {
            theme.brand.primary
        } else {
            theme.border.default
        };
        let trigger = div()
            .id("create-task-project")
            .relative()
            .w_full()
            .min_h(px(39.0))
            .px(spacing(2.0))
            .py(px(6.0))
            .flex()
            .items_center()
            .gap(spacing(1.0))
            .bg(theme.background.page)
            .border_1()
            .border_color(border)
            .rounded(radius::MD)
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                this.is_project_list_open = !this.is_project_list_open;
                if this.is_project_list_open {
                    let focus = this.project_search.focus_handle(cx);
                    focus.focus(window, cx);
                }
                cx.notify();
            }))
            .child(
                div()
                    .px(spacing(1.0))
                    .text_scale(text_scale::SM)
                    .text_color(if selected.is_some() {
                        theme.text.primary
                    } else {
                        theme.text.muted
                    })
                    .child(selected.clone().unwrap_or(placeholder.clone())),
            )
            .child(
                div()
                    .ml_auto()
                    .child(crate::views::board::icons::board_icon(
                        crate::views::board::icons::BoardIcon::ChevronDown,
                        theme.text.muted,
                        12.0,
                    )),
            )
            .child(qa_button_layer(
                &placeholder,
                &selected.unwrap_or(placeholder.clone()),
            ));
        let list = self
            .is_project_list_open
            .then(|| self.render_project_list(theme, cx));
        div()
            .child(field_label(
                theme,
                format!("{} *", labels.translate("project")),
            ))
            .child(div().relative().child(trigger).children(list))
    }

    fn render_project_list(&mut self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let projects = self.filtered_projects(cx);
        let highlight = self.project_highlight.min(projects.len().saturating_sub(1));
        let rows = projects.into_iter().enumerate().map(|(index, project)| {
            let project_id = project.id.clone();
            let hover = theme.background.page;
            let row = div()
                .id(gpui_kit::SharedString::from(format!(
                    "create-project-{}",
                    project.id
                )))
                .relative()
                .flex()
                .items_center()
                .gap(spacing(2.0))
                .px(spacing(3.0))
                .py(spacing(2.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.primary)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.select_project(&project_id, None, window, cx)
                    }),
                )
                .child(project.name.clone())
                .children(project.ssh_host.clone().map(|host| {
                    div()
                        .text_color(theme.text.muted)
                        .child(format!("({host})"))
                }))
                .child(qa_target_layer(QaTargetDescription {
                    role: "option",
                    name: project.name.clone().into(),
                    text: project.name.clone().into(),
                }));
            if index == highlight {
                row.bg(fade(theme.brand.primary, 0.1))
            } else {
                row.hover(move |style| style.bg(hover))
            }
        });
        let panel = div()
            .occlude()
            .mt(spacing(1.0))
            .w(px(398.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.md))
            .child(
                div()
                    .p(spacing(2.0))
                    .border_b_1()
                    .border_color(theme.border.default)
                    .child(
                        div()
                            .px(spacing(2.0))
                            .py(spacing(1.0))
                            .rounded(radius::BASE)
                            .border_1()
                            .border_color(theme.brand.primary)
                            .bg(theme.background.page)
                            .text_scale(text_scale::SM)
                            .child(self.project_search.clone()),
                    ),
            )
            .child(
                div()
                    .id("create-project-options")
                    .max_h(spacing(48.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.project_options_scroll)
                    .children(rows),
            );
        deferred(gpui_kit::anchored().snap_to_window().child(panel))
            .with_priority(POPOVER_LAYER_PRIORITY + 2)
            .into_any_element()
    }

    fn render_base_branch_field(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        common: &Translator,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let is_focused = self.base_branch_field.focus_handle(cx).is_focused(window);
        let dropdown = self
            .is_branch_list_open
            .then(|| self.render_branch_dropdown(theme, labels, cx));
        let notice: Option<AnyElement> = match &self.branches {
            BranchesState::Loading => Some({
                let message = labels.translate("branchesLoading");
                div()
                    .relative()
                    .mt(spacing(1.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(message.clone())
                    .child(qa_text_layer("status", &message))
                    .into_any_element()
            }),
            BranchesState::Failed(reason) => Some(
                crate::views::board::inline_load_error(
                    theme,
                    self.locale,
                    common.translate_with("loadFailed", &[("reason", reason)]),
                    "create-task-branches-retry",
                    cx.listener(|this, _, _, cx| this.load_branches(cx)),
                )
                .mt(spacing(1.0))
                .into_any_element(),
            ),
            _ => None,
        };
        let placeholder = labels.translate("branchSearchPlaceholder");
        div()
            .child(field_label(theme, labels.translate("baseBranch")))
            .child(
                div()
                    .relative()
                    .child(
                        input_box(theme, is_focused)
                            .relative()
                            .h(px(39.0))
                            .flex()
                            .items_center()
                            .mono(400)
                            .child(self.base_branch_field.clone())
                            .child(qa_target_layer(QaTargetDescription {
                                role: "textbox",
                                name: placeholder.clone().into(),
                                text: placeholder.into(),
                            })),
                    )
                    .children(dropdown),
            )
            .children(notice)
    }

    fn render_branch_dropdown(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let branches = self.filtered_branches(cx);
        let mut content: Vec<AnyElement> = Vec::new();
        if branches.is_empty() {
            content.push(
                div()
                    .px(spacing(3.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(labels.translate("noBranchesFound"))
                    .into_any_element(),
            );
        } else {
            content.push(
                div()
                    .px(spacing(3.0))
                    .py(spacing(1.0))
                    .text_scale(text_scale::PX_10)
                    .text_color(theme.text.muted)
                    .child(
                        labels.translate_with(
                            "branchCount",
                            &[("count", &branches.len().to_string())],
                        ),
                    )
                    .into_any_element(),
            );
            for (index, branch) in branches.into_iter().enumerate() {
                let chosen = branch.clone();
                let is_highlighted = index == self.branch_highlight;
                content.push(
                    div()
                        .id(gpui_kit::SharedString::from(format!(
                            "branch-option-{index}"
                        )))
                        .px(spacing(3.0))
                        .py(px(6.0))
                        .mono(400)
                        .text_scale(text_scale::BASE)
                        .text_color(if is_highlighted {
                            theme.text.primary
                        } else {
                            theme.text.secondary
                        })
                        .when_highlighted(is_highlighted, theme)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.choose_branch(chosen.clone(), cx);
                                cx.notify();
                            }),
                        )
                        .relative()
                        .child(branch.clone())
                        .child(qa_button_layer(&branch, &branch))
                        .into_any_element(),
                );
            }
        }
        content.push(
            div()
                .px(spacing(3.0))
                .py(px(6.0))
                .border_t_1()
                .border_color(theme.border.default)
                .text_scale(text_scale::PX_10)
                .text_color(theme.text.muted)
                .child(labels.translate("branchNavHint"))
                .into_any_element(),
        );
        let panel = div()
            .occlude()
            .mt(spacing(1.0))
            .w(px(398.0))
            .max_h(px(240.0))
            .overflow_hidden()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            .children(content);
        deferred(gpui_kit::anchored().snap_to_window().child(panel))
            .with_priority(POPOVER_LAYER_PRIORITY + 2)
            .into_any_element()
    }

    fn render_priority_field(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let options = [
            None,
            Some(TaskPriority::Low),
            Some(TaskPriority::Medium),
            Some(TaskPriority::High),
        ];
        let buttons = options.into_iter().map(|option| {
            let (key, background, text) = match option {
                None => ("priorityNone", theme.background.page, theme.text.muted),
                Some(TaskPriority::Low) => (
                    "priorityLow",
                    theme.priority.low.background,
                    theme.priority.low.text,
                ),
                Some(TaskPriority::Medium) => (
                    "priorityMedium",
                    theme.priority.medium.background,
                    theme.priority.medium.text,
                ),
                Some(TaskPriority::High) => (
                    "priorityHigh",
                    theme.priority.high.background,
                    theme.priority.high.text,
                ),
            };
            let border = if option.is_none() {
                theme.border.default
            } else {
                fade(text, 0.3)
            };
            let is_selected = self.priority == option;
            let label = labels.translate(key);
            let button = div()
                .id(key)
                .relative()
                .h(px(28.0))
                .flex()
                .items_center()
                .px(px(10.0))
                .rounded(radius::MD)
                .border_1()
                .border_color(border)
                .bg(background)
                .text_color(text)
                .text_scale(text_scale::XS)
                .css_weight(500)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.priority = option;
                    cx.notify();
                }))
                .child(label.clone())
                .child(qa_button_layer(&label, &label));
            if is_selected {
                button.shadow(priority_ring(theme))
            } else {
                button.opacity(0.6)
            }
        });
        div()
            .child(field_label(theme, labels.translate("priority")))
            .child(div().flex().items_center().gap(px(6.0)).children(buttons))
    }

    fn render_session_type_field(
        &mut self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let options = self.is_session_list_open.then(|| {
            let rows = SessionType::ALL.into_iter().map(|session_type| {
                div()
                    .id(session_type_label(session_type))
                    .px(spacing(3.0))
                    .py(spacing(1.0))
                    .text_scale(text_scale::SM)
                    .cursor_pointer()
                    .when_highlighted(session_type == self.session_type, theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.session_type = session_type;
                            this.is_session_list_open = false;
                            cx.notify();
                        }),
                    )
                    .child(session_type_label(session_type))
            });
            deferred(
                gpui_kit::anchored().snap_to_window().child(
                    div()
                        .occlude()
                        .w(px(398.0))
                        .rounded(radius::MD)
                        .border_1()
                        .border_color(theme.border.default)
                        .bg(theme.background.surface)
                        .shadow(box_shadows(&theme.shadow.md))
                        .children(rows),
                ),
            )
            .with_priority(POPOVER_LAYER_PRIORITY + 2)
        });
        div()
            .child(field_label(theme, labels.translate("sessionType")))
            .child(
                div()
                    .relative()
                    .child(
                        input_box(theme, false)
                            .id("create-task-session-type")
                            .h(px(39.0))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.is_session_list_open = !this.is_session_list_open;
                                cx.notify();
                            }))
                            .relative()
                            .child(session_type_label(self.session_type))
                            .child(qa_text_layer(
                                "combobox",
                                session_type_label(self.session_type),
                            ))
                            .child(
                                div()
                                    .ml_auto()
                                    .child(crate::views::board::icons::board_icon(
                                        crate::views::board::icons::BoardIcon::ChevronDown,
                                        theme.text.primary,
                                        14.0,
                                    )),
                            ),
                    )
                    .children(options),
            )
    }

    fn render_actions(
        &mut self,
        theme: &ThemeTokens,
        common: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let cancel_label = common.translate(if self.is_submitting {
            "closeAndContinue"
        } else {
            "cancel"
        });
        let create_label = match self.mode {
            CreateTaskMode::Create => common.translate(if self.is_submitting {
                "creating"
            } else {
                "create"
            }),
            CreateTaskMode::Branch { .. } => {
                Translator::new(self.locale, "branch").translate(if self.is_submitting {
                    "submitting"
                } else {
                    "submit"
                })
            }
        };
        let is_create_enabled = !self.is_submitting && self.selected_project_id.is_some();
        let hover_text = theme.text.primary;
        let hover_brand = theme.brand.hover;
        let mut create = div()
            .id("create-task-submit")
            .relative()
            .px(spacing(4.0))
            .py(spacing(2.0))
            .rounded(radius::MD)
            .bg(theme.brand.primary)
            .text_scale(text_scale::SM)
            .css_weight(500)
            .text_color(theme.text.inverse)
            .child(create_label.clone())
            .child(qa_button_layer(&create_label, &create_label));
        create = if is_create_enabled {
            create
                .cursor_pointer()
                .hover(move |style| style.bg(hover_brand))
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
        } else {
            create.opacity(0.5)
        };
        div()
            .flex()
            .justify_end()
            .gap(spacing(3.0))
            .pt(spacing(2.0))
            .child(
                div()
                    .id("create-task-cancel")
                    .relative()
                    .px(spacing(4.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .cursor_pointer()
                    .hover(move |style| style.text_color(hover_text))
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                    .child(cancel_label.clone())
                    .child(qa_button_layer(&cancel_label, &cancel_label)),
            )
            .child(create)
    }
}

/// `ring-2 ring-offset-1 ring-brand-primary`
fn priority_ring(theme: &ThemeTokens) -> Vec<gpui_kit::BoxShadow> {
    let ring = |spread: f32, color: gpui_kit::Rgba| gpui_kit::BoxShadow {
        color: color.into(),
        offset: gpui_kit::point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(spread),
        inset: false,
    };
    vec![
        ring(1.0, theme.background.surface),
        ring(3.0, theme.brand.primary),
    ]
}

trait HighlightedRow: Styled + Sized {
    fn when_highlighted(self, is_highlighted: bool, theme: &ThemeTokens) -> Self {
        if is_highlighted {
            self.bg(fade(theme.brand.primary, 0.1))
        } else {
            self
        }
    }
}

impl<E: Styled> HighlightedRow for E {}

#[cfg(test)]
mod tests {
    use super::*;

    fn branches(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn empty_query_keeps_all_branches_in_order() {
        let all = branches(&["main", "feat/a"]);
        assert_eq!(filter_branches(&all, "", None), all);
    }

    #[test]
    fn every_token_must_match_the_branch_or_project_name() {
        let all = branches(&["main", "feat/board-filters", "fix/login"]);
        assert_eq!(filter_branches(&all, "feat", None), ["feat/board-filters"]);
        assert_eq!(
            filter_branches(&all, "mainfeat", None),
            Vec::<String>::new()
        );
        assert_eq!(
            filter_branches(&all, "demo fix", Some("kanvibe-demo")),
            ["fix/login"]
        );
    }
}
