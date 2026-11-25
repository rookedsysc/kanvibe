//! 작업 정보 패널(TS `TaskDetailTitleCard` + `TaskDetailInfoCard` + `TaskDiffSummary`).
//! 타이틀 카드는 상태 배지·제목·같은 프로젝트 작업 보기·설명 편집, 정보 카드는 프로젝트·색·우선순위·변경 파일·
//! 에이전트·세션·SSH·생성/수정일이다. 변경 파일은 작업 정보 패널이 보이는 동안 3초마다 다시 읽는다.

use std::time::Duration;

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Rgba,
    SharedString, Size, StatefulInteractiveElement, Styled, Task, Window, div, px, relative,
};
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_domain::task_diff_stats::{
    DiffFile, DiffFileStatus, TaskDiffStats, summarize_diff_files,
};

use super::TaskDetailView;
use super::data::{TaskDetailSnapshot, desktop_runtime};
use super::icons::{DetailIcon, detail_icon};
use super::panel::{card, status_badge_label};
use super::usage_cards::visible_text;
use crate::components::TailwindText;
use crate::locale_format::format_date;
use crate::polled_value::{PollingSubscription, start_polling};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::settings::service_call::{call_service, failure};
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

/// TS `TASK_DIFF_FILES_POLL_INTERVAL_MS`
const DIFF_FILES_POLL_INTERVAL: Duration = Duration::from_millis(3_000);
/// `max-w-[140px]`
const PROJECT_BADGE_MAX_WIDTH: f32 = 140.0;

/// 변경 파일 조회와 목록 펼침
#[derive(Default)]
pub struct DiffSummaryState {
    pub files: Vec<DiffFile>,
    pub is_expanded: bool,
    _poll: Option<PollingSubscription<Vec<DiffFile>>>,
}

/// 정보 카드 프로젝트 바로가기(TS `shortcut-link`)가 갈 곳과 제목.
/// base 브랜치 작업이 있으면 그 작업 상세, 없으면 보드다. 제목은 base 브랜치(없으면 프로젝트 이름)다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectShortcut {
    pub target_task_id: Option<String>,
    pub title: String,
}

pub fn project_shortcut(snapshot: &TaskDetailSnapshot) -> Option<ProjectShortcut> {
    let project = snapshot.project.as_ref()?;
    Some(ProjectShortcut {
        target_task_id: snapshot.base_branch_task_id.clone(),
        title: snapshot
            .task
            .base_branch
            .clone()
            .unwrap_or_else(|| project.name.clone()),
    })
}

/// 추가 줄이 차지하는 비율(%)
pub fn addition_percent(stats: &TaskDiffStats) -> f32 {
    let changed = stats.additions + stats.deletions;
    match changed {
        0 => 0.0,
        _ => (stats.additions as f32 / changed as f32 * 100.0).round(),
    }
}

fn status_badge_colors(theme: &ThemeTokens, status: TaskStatus) -> (Rgba, Rgba) {
    // Tailwind 기본 팔레트 `bg-gray-100`·`bg-yellow-50`·`bg-purple-50`·`bg-blue-50`·`bg-green-50`
    match status {
        TaskStatus::Todo => (gpui_kit::rgb(0xf3f4f6), theme.status.todo),
        TaskStatus::Progress => (gpui_kit::rgb(0xfefce8), theme.status.progress),
        TaskStatus::Pending => (gpui_kit::rgb(0xfaf5ff), theme.status.pending),
        TaskStatus::Review => (gpui_kit::rgb(0xeff6ff), theme.status.review),
        TaskStatus::Done => (gpui_kit::rgb(0xf0fdf4), theme.status.done),
    }
}

fn info_row(theme: &ThemeTokens, label: String, value: impl IntoElement) -> gpui_kit::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(spacing(2.0))
        .child(visible_text(label, text_scale::XS).text_color(theme.text.muted))
        .child(value)
}

fn pill(background: Rgba, text: Rgba, label: String) -> gpui_kit::Div {
    visible_text(label, text_scale::XS)
        .rounded_full()
        .px(spacing(2.0))
        .py(spacing(0.5))
        .bg(background)
        .text_color(text)
}

impl TaskDetailView {
    /// 작업 정보 패널이 보이고 브랜치가 있을 때만 변경 파일을 읽는다
    pub(super) fn sync_diff_polling(
        &mut self,
        is_visible: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_branch = self
            .snapshot()
            .is_some_and(|snapshot| snapshot.task.branch_name.is_some());
        let should_poll = is_visible && has_branch;
        let is_polling = self.editors.diff_summary._poll.is_some();
        if should_poll == is_polling {
            return;
        }
        if !should_poll {
            self.editors.diff_summary._poll = None;
            return;
        }
        let task_id = self.task_id.clone();
        self.editors.diff_summary._poll = Some(start_polling(
            DIFF_FILES_POLL_INTERVAL,
            move |cx| read_diff_files(task_id.clone(), cx),
            |view: &mut Self, files, cx| {
                view.editors.diff_summary.files = files;
                cx.notify();
            },
            window,
            cx,
        ));
    }

    pub(super) fn render_overview(
        &self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let description = self.render_description(theme, &snapshot.task, cx);
        div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .child(self.render_title_card(theme, &snapshot.task, description, cx))
            .child(self.render_info_card(theme, snapshot, viewport, cx))
            .children(self.render_branch_tasks_modal(theme, viewport, cx))
            .into_any_element()
    }

    /// `TaskDetailTitleCard`
    fn render_title_card(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        description: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let can_view_branch_tasks = task.branch_name.is_some() && task.project_id.is_some();
        let branch_tasks_button = can_view_branch_tasks.then(|| {
            let description = QaTargetDescription {
                role: "button",
                name: "View other tasks in this project".into(),
                text: SharedString::default(),
            };
            project_chevron_button(theme, "view-branch-tasks", ChevronSize::Large, description)
                .on_click(cx.listener(|view, _, _, cx| view.open_branch_tasks(cx)))
        });
        card(theme)
            .p(spacing(5.0))
            .child(
                div()
                    .mb(spacing(3.0))
                    .flex()
                    .child(status_badge(theme, task.status)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(spacing(2.0))
                    .child(
                        div()
                            .text_scale(text_scale::XL)
                            .line_height(px(25.0))
                            .css_weight(700)
                            .text_color(theme.text.primary)
                            .child(task.title.clone()),
                    )
                    .children(branch_tasks_button),
            )
            .child(div().mt(spacing(3.0)).child(description))
            .into_any_element()
    }

    /// `TaskDetailInfoCard`
    fn render_info_card(
        &self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let task = &snapshot.task;
        let mut rows = div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .children(self.render_project_rows(theme, snapshot, viewport, cx));
        rows = rows.child(info_row(
            theme,
            self.translator.translate("priority"),
            self.render_priority_editor(theme, task.priority, cx),
        ));
        if task.branch_name.is_some() {
            rows = rows.child(self.render_diff_summary(theme, cx));
        }
        rows = rows.children(self.render_agent_row(theme, task));
        rows = rows.children(self.render_session_row(theme, task, cx));
        rows = rows.children(task.ssh_host.clone().map(|ssh_host| {
            let host = visible_text(ssh_host, text_scale::XS)
                .mono(400)
                .rounded(radius::BASE)
                .px(spacing(2.0))
                .py(spacing(0.5))
                .bg(theme.tag.ssh.background)
                .text_color(theme.tag.ssh.text);
            info_row(theme, self.translator.translate("sshHost"), host)
        }));
        rows = rows.child(self.render_dates(theme, task));
        card(theme)
            .p(spacing(5.0))
            .child(
                visible_text(
                    self.translator.translate("info").to_uppercase(),
                    text_scale::XS,
                )
                .mb(spacing(4.0))
                .css_weight(600)
                .text_color(theme.text.muted),
            )
            .child(rows)
            .into_any_element()
    }

    /// 프로젝트가 있으면 프로젝트(배지·바로가기)와 프로젝트 색 두 줄
    fn render_project_rows(
        &self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui_kit::Div> {
        let Some(project) = snapshot.project.as_ref() else {
            return Vec::new();
        };
        vec![
            info_row(
                theme,
                self.translator.translate("project"),
                self.render_project_badge(theme, snapshot, cx),
            ),
            info_row(
                theme,
                self.translator.translate("projectColor"),
                self.render_project_color_editor(theme, project, viewport, cx),
            ),
        ]
    }

    /// 프로젝트 배지와 바로가기(TS `shortcut-link`)
    fn render_project_badge(
        &self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(project) = snapshot.project.as_ref() else {
            return div().into_any_element();
        };
        let shortcut = project_shortcut(snapshot);
        let shortcut_link = shortcut.map(|shortcut| {
            let description = QaTargetDescription {
                role: "link",
                name: shortcut.title.clone().into(),
                text: SharedString::default(),
            };
            project_chevron_button(
                theme,
                "project-shortcut-link",
                ChevronSize::Small,
                description,
            )
            .on_click(cx.listener(move |view, _, window, cx| {
                view.follow_project_shortcut(shortcut.target_task_id.clone(), window, cx)
            }))
        });
        div()
            .flex()
            .items_center()
            .gap(spacing(1.0))
            .child(
                pill(
                    theme.tag.project.background,
                    gpui_kit::rgb(0xffffff),
                    project.name.clone(),
                )
                .max_w(px(PROJECT_BADGE_MAX_WIDTH))
                .truncate()
                .css_weight(500),
            )
            .children(shortcut_link)
            .into_any_element()
    }

    fn follow_project_shortcut(
        &mut self,
        target_task_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target_task_id {
            Some(task_id) => {
                let request = TaskNavigationRequest {
                    origin_window: Some(window.window_handle().window_id()),
                    should_open_new_window: false,
                };
                navigate_to_task_detail(&task_id, self.screen.route.locale, request, cx);
            }
            None => self.screen.navigator.navigate("/", cx),
        }
    }

    fn render_agent_row(&self, theme: &ThemeTokens, task: &KanbanTask) -> Option<gpui_kit::Div> {
        let agent = task.agent_type.as_ref()?;
        let colors = match agent.as_str() {
            "claude" => theme.tag.claude,
            "gemini" => theme.tag.gemini,
            "codex" => theme.tag.codex,
            _ => theme.tag.neutral,
        };
        Some(info_row(
            theme,
            self.translator.translate("agent"),
            pill(colors.background, colors.text, agent.clone()).css_weight(500),
        ))
    }

    /// 세션 이름이 있으면 세션 타입을 바꿀 수 있고, 없으면 종류만 보인다
    fn render_session_row(
        &self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::Div> {
        let session_type = task.session_type?;
        let value = match task.session_name.is_some() {
            true => self.render_session_type_editor(theme, session_type, cx),
            false => pill(
                theme.tag.session.background,
                theme.tag.session.text,
                session_type.as_str().to_string(),
            )
            .into_any_element(),
        };
        Some(info_row(theme, self.translator.translate("session"), value))
    }

    fn render_dates(&self, theme: &ThemeTokens, task: &KanbanTask) -> gpui_kit::Div {
        let date = |value: &str| {
            visible_text(format_date(value), text_scale::XS).text_color(theme.text.secondary)
        };
        // TS `mt-3`은 `space-y-3` 간격과 같은 자리라 겹치지 않는다
        div()
            .border_t_1()
            .border_color(theme.border.subtle)
            .pt(spacing(3.0))
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .child(info_row(
                theme,
                self.translator.translate("createdAt"),
                date(&task.created_at),
            ))
            .child(info_row(
                theme,
                self.translator.translate("updatedAt"),
                date(&task.updated_at),
            ))
    }

    /// `TaskDiffSummary`: 접어 둔 집계와 diff 화면 링크, 추가·삭제 비율 막대, 펼치면 파일 목록
    fn render_diff_summary(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let summary = &self.editors.diff_summary;
        let stats = summarize_diff_files(&summary.files);
        let has_changes = stats.file_count > 0;
        let toggle = has_changes.then(|| self.render_diff_toggle(theme, &stats, cx));
        let link = self.render_diff_link(theme, cx);
        div()
            .flex()
            .flex_col()
            .gap(spacing(1.5))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        visible_text(self.translator.translate("diffFiles"), text_scale::XS)
                            .text_color(theme.text.muted),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(spacing(1.5))
                            .children(toggle)
                            .child(link),
                    ),
            )
            .children(has_changes.then(|| change_ratio_bar(theme, &stats)))
            .children(
                (has_changes && summary.is_expanded)
                    .then(|| changed_file_list(theme, &summary.files)),
            )
            .into_any_element()
    }
}

impl TaskDetailView {
    /// 집계를 누르면 파일 목록을 펼치거나 접는다
    fn render_diff_toggle(
        &self,
        theme: &ThemeTokens,
        stats: &TaskDiffStats,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let label_key = match self.editors.diff_summary.is_expanded {
            true => "diffStats.collapse",
            false => "diffStats.expand",
        };
        div()
            .id("task-diff-summary-toggle")
            .relative()
            .flex()
            .items_center()
            .gap(spacing(1.0))
            .rounded(radius::BASE)
            .px(spacing(1.0))
            .py(spacing(0.5))
            .cursor_pointer()
            .children(diff_counts(theme, stats))
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: self.translator.translate(label_key).into(),
                text: SharedString::default(),
            }))
            .on_click(cx.listener(|view, _, _, cx| {
                let summary = &mut view.editors.diff_summary;
                summary.is_expanded = !summary.is_expanded;
                cx.notify();
            }))
    }

    /// 전체 diff 화면으로 가는 `bg-tag-branch-bg` 링크
    fn render_diff_link(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let diff_href = format!(
            "/{}/task/{}/diff",
            self.screen.route.locale.as_str(),
            self.task_id
        );
        div()
            .id("task-diff-link")
            .relative()
            .flex()
            .items_center()
            .rounded(radius::BASE)
            .bg(theme.tag.branch.background)
            .px(spacing(2.0))
            .py(spacing(0.5))
            .h(px(20.0))
            .cursor_pointer()
            .hover(|style| style.opacity(0.8))
            .child(detail_icon(
                DetailIcon::FileDiff,
                theme.tag.branch.text,
                px(12.0),
            ))
            .child(qa_target_layer(QaTargetDescription {
                role: "link",
                name: self.translator.translate("viewDiff").into(),
                text: SharedString::default(),
            }))
            .on_click(
                cx.listener(move |view, _, _, cx| view.screen.navigator.navigate(&diff_href, cx)),
            )
    }
}

/// 상태 배지(`TaskStatusBadge`: `rounded-full px-2.5 py-1 text-xs font-medium`)
fn status_badge(theme: &ThemeTokens, status: TaskStatus) -> gpui_kit::Div {
    let (background, text) = status_badge_colors(theme, status);
    visible_text(status_badge_label(status), text_scale::XS)
        .rounded_full()
        .px(spacing(2.5))
        .py(spacing(1.0))
        .css_weight(500)
        .bg(background)
        .text_color(text)
}

/// 프로젝트 바로가기 원 크기
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChevronSize {
    /// `w-6 h-6`, 16px 꺾쇠(타이틀 카드)
    Large,
    /// `w-5 h-5`, 굵은 10px 꺾쇠(정보 카드)
    Small,
    /// `w-6 h-6`, 12px 꺾쇠(같은 프로젝트 작업 모달)
    Compact,
}

/// `rounded-full bg-tag-project-bg` 원 안의 흰 꺾쇠. 프로젝트로 가는 버튼들이 같은 모양을 쓴다
pub fn project_chevron_button(
    theme: &ThemeTokens,
    id: &'static str,
    size: ChevronSize,
    description: QaTargetDescription,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let (diameter, icon, icon_size) = match size {
        ChevronSize::Large => (6.0, DetailIcon::ForwardChevron, 16.0),
        ChevronSize::Small => (5.0, DetailIcon::ForwardChevronBold, 10.0),
        ChevronSize::Compact => (6.0, DetailIcon::ForwardChevron, 12.0),
    };
    div()
        .id(id)
        .relative()
        .size(spacing(diameter))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(theme.tag.project.background)
        .cursor_pointer()
        .hover(|style| style.opacity(0.8))
        .child(detail_icon(icon, gpui_kit::rgb(0xffffff), px(icon_size)))
        .child(qa_target_layer(description))
}

/// 파일 수(`font-semibold text-text-secondary`), `+추가`(success), `-삭제`(error)
fn diff_counts(theme: &ThemeTokens, stats: &TaskDiffStats) -> [gpui_kit::Div; 3] {
    [
        visible_text(stats.file_count.to_string(), text_scale::XS)
            .css_weight(600)
            .text_color(theme.text.secondary),
        visible_text(format!("+{}", stats.additions), text_scale::XS)
            .text_color(theme.status.success),
        visible_text(format!("-{}", stats.deletions), text_scale::XS)
            .text_color(theme.status.error),
    ]
}

/// `flex h-1 w-full overflow-hidden rounded-full bg-border-subtle`
fn change_ratio_bar(theme: &ThemeTokens, stats: &TaskDiffStats) -> gpui_kit::Div {
    let addition = addition_percent(stats) / 100.0;
    div()
        .flex()
        .h(px(4.0))
        .w_full()
        .overflow_hidden()
        .rounded_full()
        .bg(theme.border.subtle)
        .child(
            div()
                .h_full()
                .w(relative(addition))
                .bg(theme.status.success),
        )
        .child(
            div()
                .h_full()
                .w(relative(1.0 - addition))
                .bg(theme.status.error),
        )
}

fn changed_file_row(theme: &ThemeTokens, file: &DiffFile) -> gpui_kit::Div {
    let (letter, color) = match file.status {
        DiffFileStatus::Added => ("A", theme.status.success),
        DiffFileStatus::Modified => ("M", theme.status.warning),
        DiffFileStatus::Deleted => ("D", theme.status.error),
        DiffFileStatus::Renamed => ("R", theme.status.info),
    };
    let path = div()
        .relative()
        .min_w_0()
        .flex_1()
        .truncate()
        .text_color(theme.text.secondary)
        .child(file.path.clone())
        .child(qa_target_layer(QaTargetDescription {
            role: "listitem",
            name: file.path.clone().into(),
            text: file.path.clone().into(),
        }));
    div()
        .flex()
        .items_center()
        .gap(spacing(1.5))
        .text_size(px(11.0))
        .line_height(px(16.0))
        .child(
            div()
                .w(spacing(2.5))
                .flex_none()
                .mono(600)
                .text_color(color)
                .child(letter),
        )
        .child(path)
        .child(
            div()
                .flex_none()
                .text_color(theme.status.success)
                .child(format!("+{}", file.additions)),
        )
        .child(
            div()
                .flex_none()
                .text_color(theme.status.error)
                .child(format!("-{}", file.deletions)),
        )
}

/// 펼친 변경 파일 목록(`max-h-48 overflow-y-auto`)
fn changed_file_list(theme: &ThemeTokens, files: &[DiffFile]) -> gpui_kit::Div {
    let list = div()
        .id("task-diff-file-list")
        .max_h(px(192.0))
        .w_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(spacing(0.5))
        .children(files.iter().map(|file| changed_file_row(theme, file)));
    div().w_full().child(list)
}

fn read_diff_files(
    task_id: String,
    cx: &gpui_kit::App,
) -> Task<crate::services::ServiceResult<Vec<DiffFile>>> {
    let runtime = desktop_runtime(cx);
    let arguments = serde_json::json!([task_id]).to_string();
    call_service(cx, "diff.getGitDiffFiles", arguments, move || {
        let runtime = runtime.ok_or_else(|| failure("diff service unavailable"))?;
        Ok(runtime.diff().get_git_diff_files(&task_id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_domain::project::Project;

    fn snapshot(
        base_branch: Option<&str>,
        base_branch_task_id: Option<&str>,
    ) -> TaskDetailSnapshot {
        let task = KanbanTask {
            id: "task-1".into(),
            title: "작업".into(),
            description: None,
            status: TaskStatus::Progress,
            branch_name: Some("feat/x".into()),
            worktree_path: None,
            session_type: None,
            session_name: None,
            ssh_host: None,
            agent_type: None,
            project_id: Some("project-1".into()),
            base_branch: base_branch.map(str::to_string),
            pr_url: None,
            priority: None,
            created_at: "2026-01-14 01:00:00".into(),
            updated_at: "2026-01-15 01:00:00".into(),
        };
        let project = Project {
            id: "project-1".into(),
            name: "kanvibe".into(),
            repo_path: "/repo".into(),
            default_branch: "main".into(),
            ssh_host: None,
            is_worktree: false,
            color: None,
            icon_data_url: None,
            created_at: "2026-01-14 01:00:00".into(),
        };
        TaskDetailSnapshot {
            task,
            project: Some(project),
            projects: Vec::new(),
            sidebar_default_collapsed: false,
            default_session_type: kanvibe_domain::task::SessionType::Tmux,
            done_alert_dismissed: false,
            base_branch_task_id: base_branch_task_id.map(str::to_string),
        }
    }

    // parity: TS-components-TaskDetailInfoCard-6a616c
    #[test]
    fn project_shortcut_goes_to_the_board_without_a_base_branch_task() {
        let shortcut = project_shortcut(&snapshot(Some("main"), None)).expect("shortcut");

        assert_eq!(shortcut.target_task_id, None);
        assert_eq!(shortcut.title, "main");
    }

    #[test]
    fn project_shortcut_goes_to_the_base_branch_task() {
        let shortcut = project_shortcut(&snapshot(None, Some("main-task-123"))).expect("shortcut");

        assert_eq!(shortcut.target_task_id.as_deref(), Some("main-task-123"));
        assert_eq!(shortcut.title, "kanvibe");
    }

    #[test]
    fn ratio_bar_splits_by_changed_lines() {
        let stats = TaskDiffStats {
            file_count: 3,
            additions: 14,
            deletions: 3,
        };
        assert_eq!(addition_percent(&stats), 82.0);
        assert_eq!(addition_percent(&TaskDiffStats::default()), 0.0);
    }
}
