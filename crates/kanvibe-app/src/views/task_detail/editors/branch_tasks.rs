//! 같은 프로젝트 작업 모달(TS `ProjectBranchTasksModal`). 타이틀 카드의 바로가기 버튼으로 연다.
//! 보드 조회 결과에서 같은 프로젝트 작업만 골라 Progress·Pending·Review 세 열로 보이고, 개수는 모든 상태를 센다.
//! 조회 실패는 작업 없음과 구분해 사유와 다시 시도를 보인다. 작업을 누르면 그 작업 상세로 가고 모달을 닫는다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Rgba,
    SharedString, Size, StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::{KanbanTask, TaskStatus};

use crate::components::TailwindText;
use crate::components::box_shadows;
use crate::components::dialog::modal_layer;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::services::ServiceResult;
use crate::text_baseline::{TextFamily, aligned_text};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::service_call::{call_database_read, failure};
use crate::views::task_detail::TaskDetailView;
use crate::views::task_detail::icons::{DetailIcon, detail_icon};
use crate::views::task_detail::load_states::render_inline_load_error;
use crate::views::task_detail::overview::{ChevronSize, project_chevron_button};
use crate::views::task_detail::status_move::kanban_service;
use crate::views::task_detail::usage_cards::visible_text;
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

/// `max-w-6xl`
const MODAL_MAX_WIDTH: f32 = 1152.0;
/// 열 폭 `w-80`
const COLUMN_WIDTH: f32 = 320.0;
/// Tailwind `bg-purple-500`(TS가 토큰 없이 쓴 Pending 점 색)
const PENDING_DOT: u32 = 0xa855f7;

/// TS `COLUMNS`
pub const BRANCH_TASK_COLUMNS: [(TaskStatus, &str); 3] = [
    (TaskStatus::Progress, "progress"),
    (TaskStatus::Pending, "pending"),
    (TaskStatus::Review, "review"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchTasksLoad {
    Loading,
    Failed(String),
    /// 같은 프로젝트 작업(모든 상태)
    Loaded(Vec<KanbanTask>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTasksModal {
    pub project_id: String,
    pub load: BranchTasksLoad,
}

/// 보드 조회 결과에서 같은 프로젝트 작업만 고른다
pub fn project_tasks(all_tasks: Vec<KanbanTask>, project_id: &str) -> Vec<KanbanTask> {
    all_tasks
        .into_iter()
        .filter(|task| task.project_id.as_deref() == Some(project_id))
        .collect()
}

impl TaskDetailView {
    pub(in crate::views::task_detail) fn open_branch_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self
            .snapshot()
            .and_then(|snapshot| snapshot.task.project_id.clone())
        else {
            return;
        };
        self.editors.branch_tasks = Some(BranchTasksModal {
            project_id,
            load: BranchTasksLoad::Loading,
        });
        self.load_branch_tasks(cx);
    }

    fn load_branch_tasks(&mut self, cx: &mut Context<Self>) {
        let Some(project_id) = self
            .editors
            .branch_tasks
            .as_ref()
            .map(|modal| modal.project_id.clone())
        else {
            return;
        };
        let service = kanban_service(cx);
        let request = call_database_read(
            cx,
            "kanban.getTasksByStatus",
            move |_| -> ServiceResult<_> {
                let service = service.ok_or_else(|| failure("kanban service unavailable"))?;
                let board = service.get_tasks_by_status().map_err(failure)?;
                let columns = board.tasks;
                let all_tasks = [
                    columns.todo,
                    columns.progress,
                    columns.pending,
                    columns.review,
                    columns.done,
                ]
                .into_iter()
                .flatten()
                .collect();
                Ok(project_tasks(all_tasks, &project_id))
            },
        );
        self.editors._branch_tasks_load = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |view, cx| {
                if let Some(modal) = view.editors.branch_tasks.as_mut() {
                    modal.load = match result {
                        Ok(tasks) => BranchTasksLoad::Loaded(tasks),
                        Err(failure) => BranchTasksLoad::Failed(failure.display_message()),
                    };
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn retry_branch_tasks(&mut self, cx: &mut Context<Self>) {
        if let Some(modal) = self.editors.branch_tasks.as_mut() {
            modal.load = BranchTasksLoad::Loading;
        }
        self.load_branch_tasks(cx);
    }

    /// 열려 있으면 닫고 `true`
    pub(in crate::views::task_detail) fn close_branch_tasks(
        &mut self,
        cx: &mut Context<Self>,
    ) -> bool {
        let was_open = self.editors.branch_tasks.take().is_some();
        self.editors._branch_tasks_load = None;
        cx.notify();
        was_open
    }

    fn open_branch_task(&mut self, task_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let request = TaskNavigationRequest {
            origin_window: Some(window.window_handle().window_id()),
            should_open_new_window: false,
        };
        navigate_to_task_detail(&task_id, self.screen.route.locale, request, cx);
        self.close_branch_tasks(cx);
    }

    pub(in crate::views::task_detail) fn render_branch_tasks_modal(
        &self,
        theme: &ThemeTokens,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let modal = self.editors.branch_tasks.as_ref()?;
        let total = match &modal.load {
            BranchTasksLoad::Loaded(tasks) => tasks.len(),
            _ => 0,
        };
        let body = self.render_branch_tasks_body(theme, &modal.load, cx);
        let panel = div()
            .id("project-branch-tasks")
            .occlude()
            .w_full()
            .max_w(px(MODAL_MAX_WIDTH))
            .max_h(viewport.height * 0.85)
            .overflow_hidden()
            .flex()
            .flex_col()
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .shadow(box_shadows(&theme.shadow.lg))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(self.render_branch_tasks_header(theme, total, cx))
            .child(body);
        let backdrop = div()
            .id("project-branch-tasks-backdrop")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|view, _, _, cx| {
                view.close_branch_tasks(cx);
            }))
            .child(panel);
        Some(modal_layer(theme, viewport, backdrop).into_any_element())
    }

    fn render_branch_tasks_body(
        &self,
        theme: &ThemeTokens,
        load: &BranchTasksLoad,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match load {
            BranchTasksLoad::Loading => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(visible_text("로딩 중...", text_scale::SM).text_color(theme.text.secondary))
                .into_any_element(),
            BranchTasksLoad::Failed(reason) => div()
                .flex_1()
                .p(spacing(6.0))
                .child(
                    render_inline_load_error(
                        theme,
                        self.common
                            .translate_with("loadFailed", &[("reason", reason)]),
                        self.common.translate("retry"),
                    )
                    .on_click_retry(cx.listener(|view, _, _, cx| view.retry_branch_tasks(cx))),
                )
                .into_any_element(),
            BranchTasksLoad::Loaded(tasks) => self.render_branch_columns(theme, tasks, cx),
        }
    }

    fn render_branch_tasks_header(
        &self,
        theme: &ThemeTokens,
        total: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(spacing(2.0))
            .p(spacing(6.0))
            .border_b_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.surface)
            .child(
                div()
                    .child(
                        visible_text("프로젝트 작업", text_scale::LG)
                            .css_weight(600)
                            .text_color(theme.text.primary),
                    )
                    .child(
                        visible_text(format!("총 {total}개"), text_scale::XS)
                            .mt(spacing(1.0))
                            .text_color(theme.text.secondary),
                    ),
            )
            .child(
                div()
                    .id("project-branch-tasks-close")
                    .relative()
                    .cursor_pointer()
                    .child(detail_icon(
                        DetailIcon::Close,
                        theme.text.secondary,
                        px(20.0),
                    ))
                    .child(qa_target_layer(QaTargetDescription {
                        role: "button",
                        name: "Close".into(),
                        text: SharedString::default(),
                    }))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.close_branch_tasks(cx);
                    })),
            )
            .into_any_element()
    }

    fn render_branch_columns(
        &self,
        theme: &ThemeTokens,
        tasks: &[KanbanTask],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let columns = Translator::new(self.screen.route.locale, "board.columns");
        let column_views = BRANCH_TASK_COLUMNS.map(|(status, label_key)| {
            let column_tasks: Vec<&KanbanTask> =
                tasks.iter().filter(|task| task.status == status).collect();
            let dot = match status {
                TaskStatus::Pending => gpui_kit::rgb(PENDING_DOT),
                TaskStatus::Review => theme.status.review,
                _ => theme.status.progress,
            };
            div()
                .flex_none()
                .w(px(COLUMN_WIDTH))
                .child(column_header(
                    theme,
                    dot,
                    columns.translate(label_key),
                    column_tasks.len(),
                ))
                .child(self.render_column_tasks(theme, &column_tasks, cx))
        });
        div()
            .flex_1()
            .p(spacing(6.0))
            .child(
                div()
                    .flex()
                    .gap(spacing(4.0))
                    .min_w_full()
                    .children(column_views),
            )
            .into_any_element()
    }

    fn render_column_tasks(
        &self,
        theme: &ThemeTokens,
        tasks: &[&KanbanTask],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if tasks.is_empty() {
            return div()
                .py(spacing(8.0))
                .flex()
                .justify_center()
                .child(visible_text("작업 없음", text_scale::XS).text_color(theme.text.muted))
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .children(
                tasks
                    .iter()
                    .map(|task| self.render_branch_task(theme, task, cx)),
            )
            .into_any_element()
    }

    fn render_branch_task(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (task_id, hover_border) = (task.id.clone(), theme.brand.primary);
        div()
            .id(SharedString::from(format!("branch-task-{}", task.id)))
            .flex()
            .items_start()
            .justify_between()
            .gap(spacing(2.0))
            .p(spacing(3.0))
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .cursor_pointer()
            .hover(move |style| style.border_color(hover_border))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_branch_task(task_id.clone(), window, cx)
            }))
            .child(branch_task_text(theme, task))
            .child(project_chevron_button(
                theme,
                "branch-task-open",
                ChevronSize::Compact,
                QaTargetDescription {
                    role: "button",
                    name: "Open task".into(),
                    text: SharedString::default(),
                },
            ))
            .into_any_element()
    }
}

/// 작업 제목(`text-sm font-medium truncate`)과 브랜치(`text-xs mt-1 truncate`)
fn branch_task_text(theme: &ThemeTokens, task: &KanbanTask) -> gpui_kit::Div {
    div()
        .flex_1()
        .min_w_0()
        .child(
            visible_text(task.title.clone(), text_scale::SM)
                .truncate()
                .css_weight(500)
                .text_color(theme.text.primary),
        )
        .children(task.branch_name.clone().map(|branch| {
            visible_text(branch, text_scale::XS)
                .mt(spacing(1.0))
                .truncate()
                .text_color(theme.text.secondary)
        }))
}

/// `flex items-center gap-2 mb-3 pb-3 border-b`: 상태 점, 열 이름, 개수
fn column_header(theme: &ThemeTokens, dot: Rgba, label: String, count: usize) -> AnyElement {
    div()
        .mb(spacing(3.0))
        .pb(spacing(3.0))
        .flex()
        .items_center()
        .gap(spacing(2.0))
        .border_b_1()
        .border_color(theme.border.subtle)
        .child(div().size(spacing(2.0)).rounded_full().bg(dot))
        .child(
            visible_text(label, text_scale::SM)
                .css_weight(600)
                .text_color(theme.text.primary),
        )
        .child(
            div()
                .ml_auto()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(aligned_text(
                    count.to_string(),
                    text_scale::XS,
                    TextFamily::Sans,
                )),
        )
        .into_any_element()
}
