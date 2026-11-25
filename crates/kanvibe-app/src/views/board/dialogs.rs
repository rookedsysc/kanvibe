//! 보드 위에 뜨는 대화상자: 새 작업(TS `CreateTaskModal`), 브랜치 기반 TODO, 브랜치 분기(`BranchTaskModal`),
//! 프로젝트 등록(`ProjectRegistryDialog`).

use gpui_kit::{AnyElement, AppContext, Context, Entity, IntoElement, Window};

use super::BoardView;
use super::board_model::find_task;
use crate::components::create_task::{
    CreateTaskDefaults, CreateTaskEvent, CreateTaskModal, CreateTaskMode,
};
use crate::components::dialog::modal_layer;
use crate::components::toast::ToastKind;
use crate::theme::ThemeTokens;
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

impl BoardView {
    /// 새 작업·브랜치 모달이 열려 있는지
    pub fn is_create_task_open(&self) -> bool {
        self.create_task.is_some()
    }

    /// 열린 새 작업 모달의 베이스 브랜치
    pub fn create_task_base_branch(&self, cx: &gpui_kit::App) -> Option<String> {
        let modal = self.create_task.as_ref()?;
        Some(modal.read(cx).base_branch().to_string())
    }

    /// 열린 새 작업 모달의 베이스 브랜치·브랜치 이름·설명 입력 글자
    pub fn create_task_field_texts(&self, cx: &gpui_kit::App) -> Option<[String; 3]> {
        Some(self.create_task.as_ref()?.read(cx).field_texts(cx))
    }

    pub(super) fn is_board_dialog_open(&self) -> bool {
        self.create_task.is_some() || self.is_registry_open
    }

    pub(super) fn render_board_dialogs(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.is_registry_open {
            return self.render_project_registry(theme, window, cx);
        }
        let modal = self.create_task.clone()?;
        Some(modal_layer(theme, self.overlay_viewport(window), modal).into_any_element())
    }

    /// 새 작업 모달을 연다. `defaults`는 (베이스 브랜치, 프로젝트 ID)다
    pub(crate) fn open_create_task(
        &mut self,
        defaults: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_task_modal(CreateTaskMode::Create, defaults, window, cx);
    }

    /// 브랜치가 없는 작업에서 브랜치를 분기하는 모달
    fn open_branch_task(&mut self, task_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = self
            .board
            .as_ref()
            .and_then(|board| find_task(&board.tasks, task_id))
            .cloned()
        else {
            return;
        };
        let mode = CreateTaskMode::Branch {
            task_id: task.id,
            task_title: task.title,
        };
        self.open_task_modal(mode, None, window, cx);
    }

    fn open_task_modal(
        &mut self,
        mode: CreateTaskMode,
        defaults: Option<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.create_task.is_some() {
            return;
        }
        let Some(board) = &self.board else {
            return;
        };
        let single_selected_project = (self.filters.selected_project_ids.len() == 1)
            .then(|| self.filters.selected_project_ids[0].clone());
        let defaults = match defaults {
            Some((base_branch, project_id)) => CreateTaskDefaults {
                project_id: Some(project_id),
                base_branch: Some(base_branch),
            },
            None => CreateTaskDefaults {
                project_id: single_selected_project,
                base_branch: None,
            },
        };
        let locale = self.locale();
        let services = self.services.clone();
        let projects = board.catalog.projects.clone();
        let session_type = board.default_session_type;
        let modal = cx.new(|cx| {
            CreateTaskModal::new(
                locale,
                mode,
                services,
                projects,
                defaults,
                session_type,
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
                self.create_task = None;
                self.board_focus.focus(window, cx);
            }
            CreateTaskEvent::Created {
                task_id,
                should_navigate,
            } => {
                self.create_task = None;
                self.board_focus.focus(window, cx);
                if !*should_navigate {
                    self.refresh(cx);
                    return;
                }
                let request = TaskNavigationRequest {
                    origin_window: Some(window.window_handle().window_id()),
                    should_open_new_window: false,
                };
                navigate_to_task_detail(task_id, self.locale(), request, cx);
            }
            CreateTaskEvent::FinishedInBackground { task_id, message } => {
                self.background_create_tasks
                    .retain(|entity| entity != modal);
                self.push_toast(ToastKind::Success, message.clone(), task_id.clone(), cx);
                self.refresh(cx);
            }
            CreateTaskEvent::FailedInBackground { message } => {
                self.background_create_tasks
                    .retain(|entity| entity != modal);
                self.show_failure_toast(message.clone(), cx);
            }
        }
        cx.notify();
    }

    /// 메뉴의 첫 항목. 브랜치가 있는 카드는 그 브랜치를 베이스로 새 TODO를, 없는 카드는 브랜치 분기를 연다
    pub(super) fn open_branch_action(
        &mut self,
        task_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(task) = self
            .board
            .as_ref()
            .and_then(|board| find_task(&board.tasks, task_id))
            .cloned()
        else {
            return;
        };
        match (task.branch_name, task.project_id) {
            (Some(branch_name), Some(project_id)) => {
                self.open_create_task(Some((branch_name, project_id)), window, cx)
            }
            _ => self.open_branch_task(task_id, window, cx),
        }
    }
}
