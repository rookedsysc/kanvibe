//! 칸반 열(TS `Column.tsx`)과 보드 본문. 같은 프로젝트 카드가 이어지면 한 묶음(`mb-1.5`)으로 감싸고,
//! Done 열은 더 보기 버튼과 실패 안내를 단다. 카드를 다른 열 위에 놓으면 그 열로 옮긴다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::{TASK_STATUS_ORDER, TaskStatus};
use kanvibe_services::kanban::TasksByStatus;

use super::BoardView;
use super::board_model::{column, contiguous_groups, root_priorities};
use super::flip_reflow::{flip_card, flip_column};
use super::header::qa_button_layer;
use super::task_card::{DraggedTask, TaskCardInput};
use crate::components::TailwindText;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 열 본문 최소 높이 `min-h-[calc(100vh-132px)]`
const COLUMN_BODY_VIEWPORT_OFFSET: f32 = 132.0;

pub(super) fn status_dot_color(theme: &ThemeTokens, status: TaskStatus) -> Rgba {
    match status {
        TaskStatus::Todo => theme.status.todo,
        TaskStatus::Progress => theme.status.progress,
        TaskStatus::Pending => theme.status.pending,
        TaskStatus::Review => theme.status.review,
        TaskStatus::Done => theme.status.done,
    }
}

pub(super) fn status_label(translator: &Translator, status: TaskStatus) -> String {
    translator.translate(status.as_str())
}

impl BoardView {
    pub(super) fn render_columns(
        &mut self,
        theme: &ThemeTokens,
        displayed: &TasksByStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        TASK_STATUS_ORDER
            .into_iter()
            .map(|status| {
                self.render_column(theme, status, column(displayed, status), window, cx)
                    .into_any_element()
            })
            .collect()
    }

    fn render_column(
        &mut self,
        theme: &ThemeTokens,
        status: TaskStatus,
        tasks: &[kanvibe_domain::task::KanbanTask],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let labels = Translator::new(self.locale(), "board.columns");
        let label = status_label(&labels, status).to_uppercase();
        let count = match status {
            TaskStatus::Done => self
                .board
                .as_ref()
                .map(|board| board.done.total)
                .unwrap_or(0),
            _ => tasks.len(),
        };
        let viewport_height = f32::from(window.viewport_size().height);
        let body = self.render_column_body(theme, status, tasks, window, cx);
        let drop_background = theme.brand.subtle;
        let drop_ring = theme.border.brand;
        let reflow = self.column_reflows.entry(status).or_default().clone();
        let order_key = tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let column = div()
            .flex_1()
            .min_w(px(284.0))
            .max_w(px(340.0))
            .overflow_hidden()
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .child(render_column_header(theme, status, label, count))
            .child(
                div()
                    .id(status.as_str())
                    .min_h(px(viewport_height - COLUMN_BODY_VIEWPORT_OFFSET))
                    .p(spacing(2.0))
                    .drag_over::<DraggedTask>(move |style, _, _, _| {
                        style.bg(drop_background).border_1().border_color(drop_ring)
                    })
                    .on_drop(cx.listener(move |this, dragged: &DraggedTask, window, cx| {
                        this.drop_task_on_column(&dragged.task_id, status, window, cx)
                    }))
                    .children(body),
            );
        flip_column(&reflow, order_key, column)
    }

    fn render_column_body(
        &mut self,
        theme: &ThemeTokens,
        status: TaskStatus,
        tasks: &[kanvibe_domain::task::KanbanTask],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(board) = self.board.as_ref() else {
            return Vec::new();
        };
        let catalog = board.catalog.clone();
        let roots = root_priorities(&board.tasks, &catalog);
        let unread = board.unread_count_by_task();
        let diff_stats = board.diff_stats.clone();
        let deletion_states = self.deletion_states.clone();
        let move_states = self.move_states.clone();
        let reflow = self.column_reflows.entry(status).or_default().clone();
        let mut elements = Vec::new();
        for group in contiguous_groups(tasks, &catalog) {
            let mut cards = Vec::new();
            for task in &group.tasks {
                let project = group
                    .project_name
                    .as_ref()
                    .and(task.project_id.as_ref())
                    .and_then(|project_id| catalog.displays.get(project_id));
                let input = TaskCardInput {
                    task,
                    project,
                    unread_count: unread.get(&task.id).copied().unwrap_or(0),
                    root_priorities: &roots,
                    diff_stats: diff_stats.get(&task.id),
                    deletion_state: deletion_states.get(&task.id),
                    move_state: move_states.get(&task.id),
                };
                let mut card = self.render_task_card(theme, input, window, cx).into_iter();
                cards.extend(
                    card.next()
                        .map(|element| flip_card(&reflow, &task.id, element).into_any_element()),
                );
                cards.extend(card);
            }
            if group.project_name.is_some() {
                elements.push(div().mb(px(6.0)).children(cards).into_any_element());
            } else {
                elements.extend(cards);
            }
        }
        if status == TaskStatus::Done {
            elements.extend(self.render_load_more_done(theme, cx));
        }
        elements
    }

    fn render_load_more_done(
        &mut self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(board) = self.board.as_ref() else {
            return Vec::new();
        };
        if !board.done.has_more() {
            return Vec::new();
        }
        let labels = Translator::new(self.locale(), "board");
        let is_loading = board.done.is_loading();
        let label = labels.translate(if is_loading {
            "loadingMore"
        } else {
            "loadMore"
        });
        let hover_background = theme.background.page;
        let hover_text = theme.text.secondary;
        let mut button = div()
            .id("board-load-more-done")
            .relative()
            .w_full()
            .flex()
            .justify_center()
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.subtle)
            .py(spacing(2.0))
            .text_scale(text_scale::XS)
            .text_color(theme.text.muted)
            .child(label.clone())
            .child(qa_button_layer(&label, &label));
        if is_loading {
            button = button.opacity(0.5);
        } else {
            button = button
                .cursor_pointer()
                .hover(move |style| style.bg(hover_background).text_color(hover_text))
                .on_click(cx.listener(|this, _, _, cx| this.load_more_done(cx)));
        }
        // TS `<button>`은 inline-block이라 위쪽 `mt-2`가 앞 카드의 `mb-1.5`와 겹치지 않는다(6px + 8px).
        // 블록 여백 겹침이 없도록 여백을 감싼 줄의 안쪽 여백으로 옮긴다
        let button_line = div().pt(spacing(2.0)).child(button);
        let mut elements = vec![button_line.into_any_element()];
        if let Some(reason) = board.done.error.clone() {
            let message = Translator::new(self.locale(), "common")
                .translate_with("loadFailed", &[("reason", &reason)]);
            elements.push(
                super::inline_load_error(
                    theme,
                    self.locale(),
                    message,
                    "board-load-more-retry",
                    cx.listener(|this, _, _, cx| this.load_more_done(cx)),
                )
                .mt(spacing(2.0))
                .into_any_element(),
            );
        }
        elements
    }
}

fn render_column_header(
    theme: &ThemeTokens,
    status: TaskStatus,
    label: String,
    count: usize,
) -> impl IntoElement {
    div()
        .flex()
        .h(spacing(10.0))
        .items_center()
        .gap(spacing(2.0))
        .border_b_1()
        .border_color(theme.border.subtle)
        .px(spacing(3.0))
        .child(
            div()
                .size(spacing(2.0))
                .rounded(radius::FULL)
                .bg(status_dot_color(theme, status)),
        )
        .child(
            div()
                .text_scale(text_scale::PX_11)
                .css_weight(600)
                .text_color(theme.text.secondary)
                .child(label),
        )
        .child(
            div()
                .ml_auto()
                .px(px(6.0))
                .py(px(2.0))
                .text_scale(text_scale::PX_11)
                .text_color(theme.text.muted)
                .child(count.to_string()),
        )
}
