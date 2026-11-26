//! 작업 상세에서 여는 새 작업 모달(TS `TaskDetailRoute`의 `CreateTaskModal`과 `openCreateTaskModal`).
//! 새 작업 단축키는 지금 작업의 브랜치(없으면 base 브랜치)를 베이스로, 그 프로젝트를 고른 채 연다.
//! 모달이 닫히면 터미널 입력 포커스로 돌아간다(TS `requestActiveTerminalFocusAfterUiSettles`).

use gpui_kit::{AnyElement, AppContext, Context, Entity, IntoElement, Window};

use super::TaskDetailView;
use crate::components::create_task::{
    CreateTaskDefaults, CreateTaskEvent, CreateTaskModal, CreateTaskMode,
};
use crate::components::dialog::modal_layer;
use crate::components::toast::ToastKind;
use crate::theme::ThemeTokens;
use crate::views::board::services::board_services;
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

impl TaskDetailView {
    /// 새 작업 모달이 열려 있는지
    pub fn is_create_task_open(&self) -> bool {
        self.create_task.is_some()
    }

    /// 열린 새 작업 모달의 (프로젝트 ID, 베이스 브랜치, 세션 타입)
    pub fn create_task_defaults(&self, cx: &gpui_kit::App) -> Option<(String, String, String)> {
        let modal = self.create_task.as_ref()?.read(cx);
        Some((
            modal.selected_project_id().unwrap_or_default().to_string(),
            modal.base_branch().to_string(),
            modal.session_type().as_str().to_string(),
        ))
    }

    /// TS `openCreateTaskModal(defaults)`. `defaults`는 (베이스 브랜치, 프로젝트 ID)이고, 없으면 지금 작업에서 정한다
    pub fn open_create_task(
        &mut self,
        defaults: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.create_task.is_some() {
            return;
        }
        let Some(snapshot) = self.snapshot().cloned() else {
            return;
        };
        let defaults = match defaults {
            Some((base_branch, project_id)) => CreateTaskDefaults {
                project_id: Some(project_id),
                base_branch: Some(base_branch),
            },
            None => branch_todo_defaults(&snapshot.task),
        };
        let locale = self.screen.route.locale;
        let services = board_services(cx);
        let modal = cx.new(|cx| {
            CreateTaskModal::new(
                locale,
                CreateTaskMode::Create,
                services,
                snapshot.projects.clone(),
                defaults,
                snapshot.default_session_type,
                window,
                cx,
            )
        });
        let subscription = cx.subscribe_in(&modal, window, Self::handle_create_task_event);
        self.create_task_subscriptions.push(subscription);
        self.create_task = Some(modal);
        cx.notify();
    }

    fn handle_create_task_event(
        &mut self,
        modal: &Entity<CreateTaskModal>,
        event: &CreateTaskEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            CreateTaskEvent::Closed => {
                if modal.read(cx).is_submitting() {
                    modal.update(cx, |modal, _| modal.mark_closed());
                    self.background_create_tasks.push(modal.clone());
                }
                self.close_create_task();
            }
            CreateTaskEvent::Created {
                task_id,
                should_navigate,
            } => {
                self.close_create_task();
                if *should_navigate {
                    let request = TaskNavigationRequest {
                        origin_window: Some(window.window_handle().window_id()),
                        should_open_new_window: false,
                    };
                    navigate_to_task_detail(task_id, self.screen.route.locale, request, cx);
                }
            }
            CreateTaskEvent::FinishedInBackground { task_id, message } => {
                self.background_create_tasks
                    .retain(|entity| entity != modal);
                self.show_action_status(ToastKind::Success, message.clone(), task_id.clone(), cx);
            }
            CreateTaskEvent::FailedInBackground { message } => {
                self.background_create_tasks
                    .retain(|entity| entity != modal);
                self.show_action_status(ToastKind::Failure, message.clone(), None, cx);
            }
        }
        cx.notify();
    }

    fn close_create_task(&mut self) {
        self.create_task = None;
        self.should_refocus_terminal = true;
    }

    fn show_action_status(
        &self,
        kind: ToastKind,
        message: String,
        task_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(shell) = self.screen.navigator.locale_shell(cx) else {
            eprintln!("[kanvibe] {message}");
            return;
        };
        shell.update(cx, |shell, cx| {
            shell.show_action_status(kind, message, task_id, cx)
        });
    }

    pub(super) fn render_create_task(
        &self,
        theme: &ThemeTokens,
        window: &Window,
    ) -> Option<AnyElement> {
        let modal = self.create_task.clone()?;
        Some(modal_layer(theme, window.viewport_size(), modal).into_any_element())
    }
}

/// TS `getBranchTodoDefaultsFromTask`: 프로젝트가 없으면 기본값 없이 연다
fn branch_todo_defaults(task: &kanvibe_domain::task::KanbanTask) -> CreateTaskDefaults {
    let Some(project_id) = task.project_id.clone() else {
        return CreateTaskDefaults {
            project_id: None,
            base_branch: None,
        };
    };
    let base_branch = [task.branch_name.as_deref(), task.base_branch.as_deref()]
        .into_iter()
        .flatten()
        .find(|branch| !branch.is_empty())
        .unwrap_or_default()
        .to_string();
    CreateTaskDefaults {
        project_id: Some(project_id),
        base_branch: Some(base_branch),
    }
}
