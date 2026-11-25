//! dock 옆 360px 패널(TS `<section className="absolute ... w-[360px]">`)과 작업 정보·상태 패널 내용.
//! 작업 정보는 `TaskDetailTitleCard`·`TaskDetailInfoCard`, 상태는 상태 이동 버튼·`DoneStatusButton`·삭제·hook 카드다.

use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Pixels, Size,
    StatefulInteractiveElement, Styled, div, px,
};
use kanvibe_domain::task::{KanbanTask, TaskStatus};

use super::TaskDetailView;
use super::data::TaskDetailSnapshot;
use super::layout_state::DetailPanel;
use super::load_states::render_inline_load_error;
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

impl TaskDetailView {
    pub(super) fn render_panel(
        &self,
        theme: &ThemeTokens,
        panel: DetailPanel,
        snapshot: &TaskDetailSnapshot,
        viewport: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = match panel {
            DetailPanel::Overview => self.translator.translate("info"),
            DetailPanel::Status => super::dock::status_panel_label(&self.translator),
            DetailPanel::LiveSessions => self.translator.translate("liveSessions.title"),
            DetailPanel::Usage => self.translator.translate("aiUsage.title"),
        };
        let body: AnyElement = match panel {
            DetailPanel::Overview => self.render_overview(theme, snapshot, viewport, cx),
            DetailPanel::Status => self.render_status_panel(theme, snapshot, cx),
            DetailPanel::LiveSessions => self.render_live_sessions_body(theme, cx),
            DetailPanel::Usage => match self.usage_panel.clone() {
                Some(usage_panel) => usage_panel.into_any_element(),
                None => div().into_any_element(),
            },
        };
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
            .child(self.render_panel_header(theme, title, cx))
            .child(body)
            .into_any_element()
    }

    /// 패널 머리(`mb-3 flex items-center justify-between border-b pb-2`): 대문자 제목과 닫기 버튼
    fn render_panel_header(
        &self,
        theme: &ThemeTokens,
        title: String,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let close_label = self.common.translate("close");
        let heading = div()
            .relative()
            .text_scale(text_scale::XS)
            .css_weight(600)
            .text_color(theme.text.muted)
            .child(title.to_uppercase())
            .child(qa_target_layer(QaTargetDescription {
                role: "heading",
                name: title.clone().into(),
                text: title.into(),
            }));
        let close = div()
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
            .on_click(cx.listener(|view, _, window, cx| view.close_panel(window, cx)));
        div()
            .mb(spacing(3.0))
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(theme.border.subtle)
            .pb(spacing(2.0))
            .child(heading)
            .child(close)
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
        div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .child(
                card(theme)
                    .p(spacing(4.0))
                    .child(buttons)
                    .children(self.render_status_move_failure(theme, cx))
                    .child(
                        div()
                            .mt(spacing(3.0))
                            .border_t_1()
                            .border_color(theme.border.subtle)
                            .pt(spacing(3.0))
                            .child(self.render_delete_task(theme, cx)),
                    ),
            )
            .children(self.hooks_card.clone())
            .into_any_element()
    }

    /// TS `InlineLoadError className="mt-3"`: 사유와 같은 상태로 다시 옮기는 다시 시도
    fn render_status_move_failure(
        &self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<gpui_kit::Div> {
        let failed = self.status_move.failure.as_ref()?;
        let message = self
            .translator
            .translate_with("statusChangeFailed", &[("reason", &failed.reason)]);
        let alert = render_inline_load_error(theme, message, self.common.translate("retry"))
            .on_click_retry(cx.listener(|view, _: &ClickEvent, _, cx| view.retry_status_move(cx)));
        Some(div().mt(spacing(3.0)).child(alert))
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

pub fn card(theme: &ThemeTokens) -> gpui_kit::Div {
    div()
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .shadow(box_shadows(&theme.shadow.sm))
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
    fn status_badge_uses_the_status_name() {
        assert_eq!(status_badge_label(TaskStatus::Progress), "Progress");
    }
}
