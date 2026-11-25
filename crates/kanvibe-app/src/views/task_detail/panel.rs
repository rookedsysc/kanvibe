//! dock 옆 360px 패널(TS `<section className="absolute ... w-[360px]">`)과 작업 정보·상태 패널 내용.
//! 작업 정보는 `TaskDetailTitleCard`·`TaskDetailInfoCard`, 상태는 상태 이동 버튼·`DoneStatusButton`·삭제·hook 카드다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::{KanbanTask, TaskPriority, TaskStatus};

use super::TaskDetailView;
use super::chrome::css_hex;
use super::data::TaskDetailSnapshot;
use super::layout_state::DetailPanel;
use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// `w-[360px]`
const PANEL_WIDTH: f32 = 360.0;
/// `left-[4.5rem]`
const PANEL_LEFT: f32 = 72.0;

/// TS `STATUS_TRANSITIONS`. 지금 상태는 버튼에서 뺀다
pub const STATUS_TRANSITIONS: [(TaskStatus, &str); 4] = [
    (TaskStatus::Todo, "moveToTodo"),
    (TaskStatus::Progress, "moveToProgress"),
    (TaskStatus::Review, "moveToReview"),
    (TaskStatus::Done, "moveToDone"),
];

pub fn visible_transitions(current: TaskStatus) -> Vec<(TaskStatus, &'static str)> {
    STATUS_TRANSITIONS
        .into_iter()
        .filter(|(status, _)| *status != current)
        .collect()
}

/// `TaskStatusBadge` 라벨
pub fn status_badge_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Todo => "Todo",
        TaskStatus::Progress => "Progress",
        TaskStatus::Pending => "Pending",
        TaskStatus::Review => "Review",
        TaskStatus::Done => "Done",
    }
}

/// `Done` 버튼이 보존 안내를 먼저 띄울지(`DoneStatusButton`)
pub fn should_confirm_done(task: &KanbanTask, is_done_alert_dismissed: bool) -> bool {
    let has_cleanable_resources = task.branch_name.is_some() || task.session_type.is_some();
    has_cleanable_resources && !is_done_alert_dismissed
}

/// `new Date(value).toLocaleDateString()`(en-US, UTC): `M/D/YYYY`
pub fn locale_date(value: &str) -> String {
    let date = value.get(..10).unwrap_or(value);
    let mut parts = date.split('-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(year), Some(month), Some(day)) => {
            let month: u32 = month.parse().unwrap_or(0);
            let day: u32 = day.parse().unwrap_or(0);
            format!("{month}/{day}/{year}")
        }
        _ => value.to_string(),
    }
}

impl TaskDetailView {
    pub(super) fn render_panel(
        &self,
        theme: &ThemeTokens,
        panel: DetailPanel,
        snapshot: &TaskDetailSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = match panel {
            DetailPanel::Overview => self.translator.translate("info"),
            DetailPanel::Status => super::dock::status_panel_label(&self.translator),
            DetailPanel::LiveSessions => self.translator.translate("liveSessions.title"),
            DetailPanel::Usage => self.translator.translate("aiUsage.title"),
        };
        let body: AnyElement = match panel {
            DetailPanel::Overview => {
                let description = self.render_description(theme, &snapshot.task, cx);
                render_overview(theme, snapshot, &self.translator, description).into_any_element()
            }
            DetailPanel::Status => self.render_status_panel(theme, snapshot, cx),
            DetailPanel::LiveSessions | DetailPanel::Usage => div().into_any_element(),
        };
        let close_label = self.common.translate("close");
        div()
            .id("task-detail-panel")
            .occlude()
            .absolute()
            .top(spacing(3.0))
            .bottom(spacing(3.0))
            .left(px(PANEL_LEFT))
            .w(px(PANEL_WIDTH))
            .overflow_y_scroll()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(fade(theme.background.surface, 0.95))
            .shadow(box_shadows(&theme.shadow.lg))
            .p(spacing(3.0))
            .child(
                div()
                    .mb(spacing(3.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(theme.border.subtle)
                    .pb(spacing(2.0))
                    .child(
                        div()
                            .relative()
                            .text_scale(text_scale::XS)
                            .css_weight(600)
                            .text_color(theme.text.muted)
                            .child(title.to_uppercase())
                            .child(qa_target_layer(QaTargetDescription {
                                role: "heading",
                                name: title.clone().into(),
                                text: title.into(),
                            })),
                    )
                    .child(
                        div()
                            .id("task-detail-panel-close")
                            .relative()
                            .rounded(radius::MD)
                            .p(spacing(1.0))
                            .text_color(theme.text.muted)
                            .cursor_pointer()
                            .child("×")
                            .child(qa_target_layer(QaTargetDescription {
                                role: "button",
                                name: close_label.into(),
                                text: "×".into(),
                            }))
                            .on_click(
                                cx.listener(|view, _, window, cx| view.close_panel(window, cx)),
                            ),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_status_panel(
        &self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut buttons = div().flex().flex_wrap().gap(spacing(2.0));
        for (status, label_key) in visible_transitions(snapshot.task.status) {
            let label = match self.status_move.pending == Some(status) {
                true => self.translator.translate_with(
                    "movingStatus",
                    &[("status", &self.translator.translate(label_key))],
                ),
                false => self.translator.translate(label_key),
            };
            buttons = buttons.child(self.render_status_button(theme, status, label, snapshot, cx));
        }
        let delete_label = self.translator.translate("delete");
        div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .child(
                card(theme)
                    .p(spacing(4.0))
                    .child(buttons)
                    .children(self.status_move.error.as_ref().map(|reason| {
                        div()
                            .mt(spacing(3.0))
                            .text_scale(text_scale::XS)
                            .text_color(theme.status.error)
                            .child(
                                self.translator
                                    .translate_with("statusChangeFailed", &[("reason", reason)]),
                            )
                    }))
                    .child(
                        div()
                            .mt(spacing(3.0))
                            .border_t_1()
                            .border_color(theme.border.subtle)
                            .pt(spacing(3.0))
                            .child(
                                div().w_auto().flex().child(
                                    div()
                                        .rounded(radius::MD)
                                        .px(spacing(3.0))
                                        .py(spacing(1.5))
                                        .text_scale(text_scale::XS)
                                        .bg(gpui_kit::rgb(0xfef2f2))
                                        .text_color(gpui_kit::rgb(0xef4444))
                                        .child(delete_label),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_status_button(
        &self,
        theme: &ThemeTokens,
        status: TaskStatus,
        label: String,
        snapshot: &TaskDetailSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let needs_confirmation = status == TaskStatus::Done
            && should_confirm_done(&snapshot.task, snapshot.done_alert_dismissed);
        let hover_border = theme.brand.primary;
        let hover_text = theme.text.brand;
        let mut button = div()
            .id(gpui_kit::SharedString::from(format!(
                "status-move-{}",
                status.as_str()
            )))
            .relative()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .px(spacing(3.0))
            .py(spacing(1.5))
            .text_scale(text_scale::XS)
            .text_color(theme.text.secondary)
            .child(label.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "button",
                name: label.clone().into(),
                text: label.into(),
            }));
        if self.status_move.pending.is_some() {
            button = button.opacity(0.6);
        } else {
            button = button
                .cursor_pointer()
                .hover(move |style| style.border_color(hover_border).text_color(hover_text))
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.request_status_move(status, needs_confirmation, window, cx)
                }));
        }
        button.into_any_element()
    }
}

fn card(theme: &ThemeTokens) -> gpui_kit::Div {
    div()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .shadow(box_shadows(&theme.shadow.sm))
}

/// 작업 정보 패널: 타이틀 카드 + 정보 카드(`space-y-3`)
fn render_overview(
    theme: &ThemeTokens,
    snapshot: &TaskDetailSnapshot,
    translator: &Translator,
    description: AnyElement,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(spacing(3.0))
        .child(render_title_card(theme, &snapshot.task, description))
        .child(render_info_card(theme, snapshot, translator))
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

/// `TaskDetailTitleCard`: 상태 배지, 제목, 같은 프로젝트 작업 보기 버튼, 설명(없으면 `addDescription`)
fn render_title_card(
    theme: &ThemeTokens,
    task: &KanbanTask,
    description: AnyElement,
) -> impl IntoElement {
    let (badge_background, badge_text) = status_badge_colors(theme, task.status);
    card(theme)
        .p(spacing(5.0))
        .child(
            div().mb(spacing(3.0)).flex().child(
                div()
                    .rounded_full()
                    .px(spacing(2.5))
                    .py(spacing(1.0))
                    .text_scale(text_scale::XS)
                    .css_weight(500)
                    .bg(badge_background)
                    .text_color(badge_text)
                    .child(status_badge_label(task.status)),
            ),
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
                .children(
                    (task.branch_name.is_some() && task.project_id.is_some()).then(|| {
                        div()
                            .size(spacing(6.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme.tag.project.background)
                            .child(super::icons::detail_icon(
                                super::icons::DetailIcon::ForwardChevron,
                                gpui_kit::rgb(0xffffff),
                                px(16.0),
                            ))
                    }),
                ),
        )
        .child(div().mt(spacing(3.0)).child(description))
}

fn info_row(theme: &ThemeTokens, label: String, value: impl IntoElement) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(spacing(2.0))
        .child(
            div()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(label),
        )
        .child(value)
}

fn pill(background: Rgba, text: Rgba, label: String) -> gpui_kit::Div {
    div()
        .rounded_full()
        .px(spacing(2.0))
        .py(spacing(0.5))
        .text_scale(text_scale::XS)
        .bg(background)
        .text_color(text)
        .child(label)
}

/// `TaskDetailInfoCard`(프로젝트·프로젝트 색·우선순위·에이전트·세션·SSH·생성/수정일)
fn render_info_card(
    theme: &ThemeTokens,
    snapshot: &TaskDetailSnapshot,
    translator: &Translator,
) -> impl IntoElement {
    let task = &snapshot.task;
    let mut rows = div().flex().flex_col().gap(spacing(3.0));
    if let Some(project) = snapshot.project.as_ref() {
        let color = project.color.clone().unwrap_or_else(|| {
            kanvibe_process::project_color::compute_project_color(&project.name).to_string()
        });
        rows = rows
            .child(info_row(
                theme,
                translator.translate("project"),
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
                        .css_weight(500),
                    )
                    .child(
                        div()
                            .size(spacing(5.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme.tag.project.background)
                            .child(super::icons::detail_icon(
                                super::icons::DetailIcon::ForwardChevronBold,
                                gpui_kit::rgb(0xffffff),
                                px(10.0),
                            )),
                    ),
            ))
            .child(info_row(
                theme,
                translator.translate("projectColor"),
                div()
                    .size(spacing(6.0))
                    .rounded_full()
                    .border_2()
                    .border_color(theme.border.default)
                    .bg(css_hex(&color)),
            ));
    }
    rows = rows.child(info_row(
        theme,
        translator.translate("priority"),
        render_priority(theme, task.priority),
    ));
    if let Some(agent) = task.agent_type.as_ref() {
        let colors = match agent.as_str() {
            "claude" => theme.tag.claude,
            "gemini" => theme.tag.gemini,
            "codex" => theme.tag.codex,
            _ => theme.tag.neutral,
        };
        rows = rows.child(info_row(
            theme,
            translator.translate("agent"),
            pill(colors.background, colors.text, agent.clone()).css_weight(500),
        ));
    }
    if let Some(session_type) = task.session_type {
        rows = rows.child(info_row(
            theme,
            translator.translate("session"),
            pill(
                theme.tag.session.background,
                theme.tag.session.text,
                session_type.as_str().to_string(),
            ),
        ));
    }
    if let Some(ssh_host) = task.ssh_host.as_ref() {
        rows = rows.child(info_row(
            theme,
            translator.translate("sshHost"),
            pill(
                theme.tag.ssh.background,
                theme.tag.ssh.text,
                ssh_host.clone(),
            )
            .mono(400),
        ));
    }
    rows = rows.child(
        div()
            .mt(spacing(3.0))
            .border_t_1()
            .border_color(theme.border.subtle)
            .pt(spacing(3.0))
            .flex()
            .flex_col()
            .gap(spacing(2.0))
            .child(info_row(
                theme,
                translator.translate("createdAt"),
                date_text(theme, &task.created_at),
            ))
            .child(info_row(
                theme,
                translator.translate("updatedAt"),
                date_text(theme, &task.updated_at),
            )),
    );
    card(theme)
        .p(spacing(5.0))
        .child(
            div()
                .mb(spacing(4.0))
                .text_scale(text_scale::XS)
                .css_weight(600)
                .text_color(theme.text.muted)
                .child(translator.translate("info").to_uppercase()),
        )
        .child(rows)
}

fn date_text(theme: &ThemeTokens, value: &str) -> impl IntoElement {
    div()
        .text_scale(text_scale::XS)
        .text_color(theme.text.secondary)
        .child(locale_date(value))
}

/// `PrioritySelector`: 없음·!·!!·!!! 네 칸, 고른 칸에 brand 테두리
fn render_priority(theme: &ThemeTokens, current: Option<TaskPriority>) -> impl IntoElement {
    let options: [(Option<TaskPriority>, &str, Rgba, Rgba); 4] = [
        (None, "없음", theme.background.page, theme.text.muted),
        (
            Some(TaskPriority::Low),
            "!",
            theme.priority.low.background,
            theme.priority.low.text,
        ),
        (
            Some(TaskPriority::Medium),
            "!!",
            theme.priority.medium.background,
            theme.priority.medium.text,
        ),
        (
            Some(TaskPriority::High),
            "!!!",
            theme.priority.high.background,
            theme.priority.high.text,
        ),
    ];
    div()
        .flex()
        .gap(spacing(1.5))
        .children(
            options
                .into_iter()
                .map(|(priority, label, background, text)| {
                    let is_selected = priority == current;
                    div()
                        .rounded(radius::MD)
                        .border_1()
                        .border_color(if is_selected {
                            theme.brand.primary
                        } else {
                            theme.border.default
                        })
                        .px(spacing(2.0))
                        .py(spacing(0.5))
                        .text_scale(text_scale::XS)
                        .bg(background)
                        .text_color(text)
                        .child(label)
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_domain::task::SessionType;

    fn task(status: TaskStatus) -> KanbanTask {
        KanbanTask {
            id: "task-1".into(),
            title: "보드 필터 추가".into(),
            description: None,
            status,
            branch_name: None,
            worktree_path: None,
            session_type: None,
            session_name: None,
            ssh_host: None,
            agent_type: None,
            project_id: None,
            base_branch: None,
            pr_url: None,
            priority: None,
            created_at: "2026-01-14 01:00:00".into(),
            updated_at: "2026-01-15T02:55:00.000Z".into(),
        }
    }

    // parity: TS-renderer-TaskDetailRoute-22fa59
    #[test]
    fn status_panel_offers_every_other_status() {
        let labels: Vec<&str> = visible_transitions(TaskStatus::Progress)
            .into_iter()
            .map(|(_, key)| key)
            .collect();
        assert_eq!(labels, ["moveToTodo", "moveToReview", "moveToDone"]);
    }

    // parity: TS-components-DoneStatusButton-252543
    #[test]
    fn done_with_cleanable_resources_asks_first() {
        let mut with_branch = task(TaskStatus::Progress);
        with_branch.branch_name = Some("feat/x".into());
        assert!(should_confirm_done(&with_branch, false));
    }

    // parity: TS-components-DoneStatusButton-60501e
    #[test]
    fn dismissed_done_alert_moves_directly() {
        let mut with_session = task(TaskStatus::Progress);
        with_session.session_type = Some(SessionType::Tmux);
        assert!(!should_confirm_done(&with_session, true));
    }

    // parity: TS-components-DoneStatusButton-a862fa
    #[test]
    fn tasks_without_resources_move_to_done_directly() {
        assert!(!should_confirm_done(&task(TaskStatus::Progress), false));
    }

    #[test]
    fn dates_render_like_en_us_locale_date_strings() {
        assert_eq!(locale_date("2026-01-14 01:00:00"), "1/14/2026");
        assert_eq!(locale_date("2026-01-15T02:55:00.000Z"), "1/15/2026");
        assert_eq!(status_badge_label(TaskStatus::Progress), "Progress");
    }
}
