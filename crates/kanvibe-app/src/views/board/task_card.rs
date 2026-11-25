//! 칸반 카드(TS `TaskCard.tsx` + `TaskCardContent.tsx` + `TaskDiffStatsBadge`).
//! 카드는 포커스를 받는 링크다. 누르면 작업 상세로 가고(같은 작업 창이 있으면 그 창을 앞으로), 우클릭·Shift+F10은 메뉴를,
//! Shift+Enter는 새 창 상세를 연다. 삭제 중·이동 저장 중에는 끌 수 없다.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{
    AnyElement, AppContext, Context, Div, Image, ImageFormat, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Rgba, StatefulInteractiveElement, Styled, Window, div, img, px,
    rgb,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::{KanbanTask, TaskPriority};
use kanvibe_domain::task_diff_stats::TaskDiffStats;
use kanvibe_process::project_color::readable_text_color;

use super::BoardView;
use super::board_model::{TaskDeletionState, TaskMoveState};
use super::icons::{BoardIcon, board_icon};
use super::task_filters::{ProjectDisplay, is_inherited_priority, resolve_effective_priority};
use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ColorPair, ThemeTokens, radius, spacing, text_scale};

/// 카드 그리기에 필요한 값
pub(super) struct TaskCardInput<'a> {
    pub task: &'a KanbanTask,
    pub project: Option<&'a ProjectDisplay>,
    pub unread_count: usize,
    pub root_priorities: &'a HashMap<String, TaskPriority>,
    pub diff_stats: Option<&'a TaskDiffStats>,
    pub deletion_state: Option<&'a TaskDeletionState>,
    pub move_state: Option<&'a TaskMoveState>,
}

/// 끌고 있는 카드(드롭 대상 열이 받는 값)
#[derive(Debug, Clone)]
pub(super) struct DraggedTask {
    pub task_id: String,
    pub title: String,
}

/// 끄는 동안 마우스를 따라다니는 카드 모양
pub(super) struct DraggedTaskPreview {
    title: String,
    theme: ThemeTokens,
}

impl gpui_kit::Render for DraggedTaskPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .w(px(280.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.brand)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.md))
            .px(px(10.0))
            .py(spacing(2.0))
            .text_size(px(13.0))
            .line_height(px(20.0))
            .css_weight(500)
            .text_color(theme.text.primary)
            .truncate()
            .child(self.title.clone())
    }
}

/// `#RRGGBB` 프로젝트 색. 형식이 다르면 기본 파랑(`#93C5FD`)
pub(super) fn parse_project_color(value: &str) -> Rgba {
    let hex = value.trim_start_matches('#');
    match u32::from_str_radix(hex, 16) {
        Ok(color) if hex.len() == 6 => rgb(color),
        _ => rgb(0x93C5FD),
    }
}

/// `data:image/...;base64,...` 아이콘을 이미지로 바꾼다
fn data_url_image(data_url: &str) -> Option<Arc<Image>> {
    let (header, payload) = data_url.strip_prefix("data:")?.split_once(',')?;
    let (mime, encoding) = header.split_once(';')?;
    if encoding != "base64" {
        return None;
    }
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        "image/svg+xml" => ImageFormat::Svg,
        _ => return None,
    };
    Some(Arc::new(Image::from_bytes(format, decode_base64(payload)?)))
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let value_of = |byte: u8| -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let symbols: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace() && *byte != b'=')
        .collect();
    let mut bytes = Vec::with_capacity(symbols.len() * 3 / 4);
    for chunk in symbols.chunks(4) {
        let mut buffer = 0u32;
        for (position, symbol) in chunk.iter().enumerate() {
            buffer |= value_of(*symbol)? << (18 - 6 * position);
        }
        let produced = chunk.len().saturating_sub(1);
        bytes.extend(buffer.to_be_bytes()[1..1 + produced].iter());
    }
    Some(bytes)
}

/// `ProjectIcon`: GitHub 아이콘이 있으면 그 이미지, 없으면 프로젝트 색 이니셜(14px)
pub(super) fn project_marker(display: &ProjectDisplay, size: f32) -> AnyElement {
    if let Some(image) = display.icon_data_url().and_then(data_url_image) {
        return img(image)
            .size(px(size))
            .flex_none()
            .rounded(px(2.0))
            .into_any_element();
    }
    let color = display.color();
    let initial: String = display
        .name
        .trim()
        .chars()
        .next()
        .map(|character| character.to_uppercase().collect())
        .unwrap_or_default();
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.0))
        .bg(parse_project_color(&color))
        .text_color(parse_project_color(readable_text_color(&color)))
        .text_size(px(9.0))
        .line_height(px(9.0))
        .css_weight(700)
        .child(initial)
        .into_any_element()
}

/// `inline-flex items-center rounded border border-border-subtle px-1.5 py-0.5 text-[10px]`
fn badge(theme: &ThemeTokens, colors: ColorPair) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .rounded(radius::BASE)
        .border_1()
        .border_color(theme.border.subtle)
        .px(px(6.0))
        .py(px(2.0))
        .text_scale(text_scale::PX_10)
        .bg(colors.background)
        .text_color(colors.text)
}

/// 배지 글자를 QA·시험이 읽을 수 있게 남긴다
fn badge_label_layer(role: &'static str, name: &str, text: &str) -> impl IntoElement {
    qa_target_layer(QaTargetDescription {
        role,
        name: name.to_string().into(),
        text: text.to_string().into(),
    })
}

fn agent_colors(theme: &ThemeTokens, agent_type: &str) -> ColorPair {
    match agent_type {
        "claude" => theme.tag.claude,
        "gemini" => theme.tag.gemini,
        "codex" => theme.tag.codex,
        _ => theme.tag.neutral,
    }
}

fn priority_badge(
    theme: &ThemeTokens,
    priority: TaskPriority,
    is_inherited: bool,
) -> impl IntoElement + use<> {
    let (label, colors) = match priority {
        TaskPriority::Low => ("P3", theme.priority.low),
        TaskPriority::Medium => ("P2", theme.priority.medium),
        TaskPriority::High => ("P1", theme.priority.high),
    };
    let base = div()
        .ml_auto()
        .flex_none()
        .rounded(radius::BASE)
        .border_1()
        .px(px(6.0))
        .py(px(2.0))
        .text_scale(text_scale::PX_10)
        .css_weight(600)
        .bg(colors.background)
        .text_color(colors.text)
        .relative()
        .child(label)
        .child(badge_label_layer(
            "status",
            if is_inherited {
                "inherited-priority"
            } else {
                "priority"
            },
            label,
        ));
    if is_inherited {
        base.border_dashed()
            .border_color(theme.border.default)
            .opacity(0.7)
    } else {
        base.border_color(theme.border.subtle)
    }
}

/// 파일 수와 추가·삭제 줄 수. 바뀐 파일이 없으면 그리지 않는다
fn diff_stats_badge(
    theme: &ThemeTokens,
    stats: &TaskDiffStats,
    summary: String,
) -> Option<impl IntoElement + use<>> {
    if stats.file_count == 0 {
        return None;
    }
    Some(
        div()
            .relative()
            .child(badge_label_layer("status", &summary, ""))
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing(1.0))
            .rounded(radius::BASE)
            .border_1()
            .border_color(theme.border.subtle)
            .px(px(6.0))
            .py(px(2.0))
            .text_size(px(10.0))
            .line_height(px(10.0))
            .text_color(theme.text.primary)
            .child(board_icon(BoardIcon::FileDiff, theme.text.primary, 12.0))
            .child(
                div()
                    .css_weight(600)
                    .text_color(theme.text.secondary)
                    .child(stats.file_count.to_string()),
            )
            .child(
                div()
                    .text_color(theme.status.success)
                    .child(format!("+{}", stats.additions)),
            )
            .child(
                div()
                    .text_color(theme.status.error)
                    .child(format!("-{}", stats.deletions)),
            ),
    )
}

fn unread_badge(
    theme: &ThemeTokens,
    unread_count: usize,
    aria_label: String,
) -> impl IntoElement + use<> {
    div()
        .absolute()
        .right_0()
        .top(px(-2.0))
        .h(spacing(5.0))
        .flex()
        .items_center()
        .gap(spacing(1.0))
        .rounded(radius::BASE)
        .px(px(6.0))
        .bg(theme.brand.primary)
        .text_scale(text_scale::PX_10)
        .css_weight(600)
        .text_color(theme.text.inverse)
        .child(board_icon(BoardIcon::Bell, theme.text.inverse, 11.0))
        .child(unread_count.to_string())
        .child(badge_label_layer(
            "status",
            &aria_label,
            &unread_count.to_string(),
        ))
}

fn crown_marker(theme: &ThemeTokens) -> impl IntoElement + use<> {
    div()
        .absolute()
        .right(spacing(2.0))
        .top(spacing(2.0))
        .size(spacing(5.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(radius::FULL)
        .border_1()
        .border_color(theme.border.subtle)
        .bg(theme.tag.base_branch.background)
        .shadow(box_shadows(&theme.shadow.sm))
        .child(board_icon(
            BoardIcon::Crown,
            theme.tag.base_branch.text,
            12.0,
        ))
        .child(badge_label_layer("img", "Base branch", ""))
}

/// worktree 경로가 `__worktrees`가 아니면 기본 브랜치에서 일하는 카드다
fn is_base_project(task: &KanbanTask) -> bool {
    task.worktree_path
        .as_deref()
        .is_some_and(|path| !path.contains("__worktrees"))
}

/// 링크 안에 둘 수 없는 재시도 버튼을 카드 바로 아래에 붙인다
fn request_failure(
    theme: &ThemeTokens,
    message: String,
    retry_label: String,
    retry_id: String,
    on_retry: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> impl IntoElement {
    div()
        .mt(px(-4.0))
        .mb(px(6.0))
        .flex()
        .items_start()
        .justify_between()
        .gap(spacing(2.0))
        .rounded(radius::MD)
        .border_1()
        .border_color(fade(theme.status.error, 0.2))
        .bg(theme.background.page)
        .px(px(10.0))
        .py(px(6.0))
        .text_scale(text_scale::PX_11)
        .text_color(theme.status.error)
        .relative()
        .child(badge_label_layer("alert", &message, &message))
        .child(div().min_w_0().flex_1().child(message))
        .child(
            div()
                .id(gpui_kit::SharedString::from(retry_id))
                .relative()
                .flex_none()
                .rounded(radius::BASE)
                .px(px(6.0))
                .py(px(2.0))
                .css_weight(500)
                .cursor_pointer()
                .on_click(on_retry)
                .child(retry_label.clone())
                .child(qa_target_layer(QaTargetDescription {
                    role: "button",
                    name: retry_label.clone().into(),
                    text: retry_label.into(),
                })),
        )
}

fn request_progress(theme: &ThemeTokens, label: String) -> impl IntoElement {
    div()
        .mt(px(6.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_scale(text_scale::PX_11)
        .text_color(theme.text.muted)
        .child(
            div()
                .size(spacing(3.0))
                .rounded(radius::FULL)
                .border_2()
                .border_color(theme.text.muted),
        )
        .relative()
        .child(label.clone())
        .child(badge_label_layer("status", &label, &label))
}

impl BoardView {
    pub(super) fn render_task_card(
        &mut self,
        theme: &ThemeTokens,
        input: TaskCardInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let task = input.task;
        let labels = Translator::new(self.locale(), "task");
        let is_deleting = matches!(input.deletion_state, Some(TaskDeletionState::Deleting));
        let is_saving_move = matches!(input.move_state, Some(TaskMoveState::Saving));
        let focus_handle = self.card_focus_handle(&task.id, cx);
        let is_focused = focus_handle.is_focused(window);
        let project_color = input
            .project
            .map(|display| parse_project_color(&display.color()));
        let mut card = div()
            .id(gpui_kit::SharedString::from(format!(
                "task-card-{}",
                task.id
            )))
            .track_focus(&focus_handle)
            .relative()
            .mb(px(6.0))
            .overflow_hidden()
            .rounded(radius::MD)
            .border_1()
            .border_color(project_color.unwrap_or(theme.border.subtle))
            .px(px(10.0))
            .py(spacing(2.0))
            .cursor_pointer()
            .when_base_project(is_base_project(task))
            .children(self.task_card_content(theme, &input));
        if is_focused {
            card = card
                .border_color(theme.border.brand)
                .bg(fade(theme.background.surface, 0.9))
                .shadow(box_shadows(&theme.shadow.kanban_task_focus));
        } else {
            let hover_background = fade(theme.background.surface, 0.7);
            card = card.hover(move |style| style.bg(hover_background));
        }
        if is_deleting {
            card = card
                .opacity(0.6)
                .child(request_progress(theme, labels.translate("deleting")));
        } else if is_saving_move {
            card = card.child(request_progress(theme, labels.translate("moveSaving")));
        }
        let card =
            self.attach_card_interactions(card, task, is_deleting || is_saving_move, theme, cx);
        let mut elements = vec![card.into_any_element()];
        elements.extend(self.render_card_failures(theme, task, &input, &labels, cx));
        elements
    }

    pub(super) fn task_card_content(
        &self,
        theme: &ThemeTokens,
        input: &TaskCardInput,
    ) -> Vec<AnyElement> {
        let task = input.task;
        let unread_label = Translator::new(self.locale(), "common")
            .translate_with("unreadCount", &[("count", &input.unread_count.to_string())]);
        let unread =
            (input.unread_count > 0).then(|| unread_badge(theme, input.unread_count, unread_label));
        let mut content = Vec::new();
        if is_base_project(task) {
            content.push(crown_marker(theme).into_any_element());
        }
        let title = div()
            .relative()
            .child(
                div()
                    .truncate()
                    .text_size(px(13.0))
                    .line_height(px(20.0))
                    .css_weight(500)
                    .text_color(theme.text.primary)
                    .child(task.title.clone()),
            )
            .child(qa_target_layer(QaTargetDescription {
                role: "link",
                name: task.title.clone().into(),
                text: task.title.clone().into(),
            }));
        let description = task
            .description
            .as_ref()
            .filter(|text| !text.is_empty())
            .map(|text| {
                div()
                    .mt(px(2.0))
                    .truncate()
                    .text_size(px(11.0))
                    .line_height(px(16.0))
                    .text_color(theme.text.muted)
                    .child(text.lines().next().unwrap_or_default().to_string())
            });
        match input.project {
            Some(display) => {
                let color = parse_project_color(&display.color());
                content.push(
                    div()
                        .mb(spacing(1.0))
                        .flex()
                        .items_center()
                        .gap(spacing(2.0))
                        .child(project_marker(display, 14.0))
                        .child(
                            div()
                                .relative()
                                .min_w_0()
                                .flex_1()
                                .child(
                                    div()
                                        .truncate()
                                        .text_scale(text_scale::XS)
                                        .line_height(px(16.0))
                                        .css_weight(600)
                                        .text_color(color)
                                        .child(display.name.clone()),
                                )
                                .children(unread),
                        )
                        .into_any_element(),
                );
                content.push(
                    div()
                        .pl(px(22.0))
                        .child(title)
                        .children(description)
                        .into_any_element(),
                );
            }
            None => {
                content.push(div().child(title.children(unread)).into_any_element());
                content.extend(description.map(IntoElement::into_any_element));
            }
        }
        content.push(self.task_card_badges(theme, input).into_any_element());
        content
    }

    fn task_card_badges(
        &self,
        theme: &ThemeTokens,
        input: &TaskCardInput,
    ) -> impl IntoElement + use<> {
        let task = input.task;
        let mut badges: Vec<AnyElement> = Vec::new();
        if task.pr_url.is_some() {
            badges.push(
                badge(theme, theme.tag.pull_request)
                    .gap(spacing(1.0))
                    .relative()
                    .child(board_icon(
                        BoardIcon::PullRequest,
                        theme.tag.pull_request.text,
                        12.0,
                    ))
                    .child("PR")
                    .child(badge_label_layer("link", "PR", "PR"))
                    .into_any_element(),
            );
        }
        if let Some(agent_type) = &task.agent_type {
            badges.push(
                badge(theme, agent_colors(theme, agent_type))
                    .relative()
                    .child(agent_type.clone())
                    .child(badge_label_layer("status", "agent", agent_type))
                    .into_any_element(),
            );
        }
        if let Some(session_type) = task.session_type {
            badges.push(
                badge(theme, theme.tag.session)
                    .relative()
                    .child(session_type.as_str())
                    .child(badge_label_layer(
                        "status",
                        "session",
                        session_type.as_str(),
                    ))
                    .into_any_element(),
            );
        }
        let diff = input.diff_stats.and_then(|stats| {
            let summary = Translator::new(self.locale(), "taskDetail.diffStats").translate_with(
                "summary",
                &[
                    ("files", &stats.file_count.to_string()),
                    ("additions", &stats.additions.to_string()),
                    ("deletions", &stats.deletions.to_string()),
                ],
            );
            diff_stats_badge(theme, stats, summary)
        });
        if let Some(diff) = diff {
            badges.push(diff.into_any_element());
        }
        if let Some(ssh_host) = &task.ssh_host {
            badges.push(
                badge(theme, theme.tag.ssh)
                    .relative()
                    .child(ssh_host.clone())
                    .child(badge_label_layer("status", "ssh", ssh_host))
                    .into_any_element(),
            );
        }
        if let Some(priority) = resolve_effective_priority(task, input.root_priorities) {
            let is_inherited = is_inherited_priority(task, input.root_priorities);
            badges.push(priority_badge(theme, priority, is_inherited).into_any_element());
        }
        div()
            .mt(px(6.0))
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.0))
            .children(badges)
    }

    fn render_card_failures(
        &mut self,
        theme: &ThemeTokens,
        task: &KanbanTask,
        input: &TaskCardInput,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut failures = Vec::new();
        if let Some(TaskDeletionState::Failed { reason }) = input.deletion_state {
            let task_id = task.id.clone();
            failures.push(
                request_failure(
                    theme,
                    labels.translate_with("deleteFailed", &[("reason", reason)]),
                    labels.translate("retryDelete"),
                    format!("retry-delete-{}", task.id),
                    cx.listener(move |this, _, _, cx| this.delete_task(task_id.clone(), cx)),
                )
                .into_any_element(),
            );
        }
        if let Some(TaskMoveState::Failed {
            reason,
            target_status,
        }) = input.move_state
        {
            let task_id = task.id.clone();
            let target_status = *target_status;
            failures.push(
                request_failure(
                    theme,
                    labels.translate_with("moveFailed", &[("reason", reason)]),
                    labels.translate("retryMove"),
                    format!("retry-move-{}", task.id),
                    cx.listener(move |this, _, window, cx| {
                        this.move_task_to_status(&task_id, target_status, window, cx)
                    }),
                )
                .into_any_element(),
            );
        }
        failures
    }

    fn attach_card_interactions(
        &mut self,
        card: gpui_kit::Stateful<Div>,
        task: &KanbanTask,
        is_drag_disabled: bool,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<Div> {
        let click_task_id = task.id.clone();
        let menu_task_id = task.id.clone();
        let card = card
            .on_click(
                cx.listener(move |this, event: &gpui_kit::ClickEvent, window, cx| {
                    this.activate_task_card(&click_task_id, event.modifiers().shift, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, cx| {
                    this.open_context_menu(&menu_task_id, event.position, window, cx)
                }),
            );
        if is_drag_disabled {
            return card;
        }
        let dragged = DraggedTask {
            task_id: task.id.clone(),
            title: task.title.clone(),
        };
        let theme = *theme;
        card.on_drag(dragged, move |dragged, _, _, cx| {
            cx.new(|_| DraggedTaskPreview {
                title: dragged.title.clone(),
                theme,
            })
        })
    }
}

trait BaseProjectPadding: Styled + Sized {
    /// 기본 브랜치 표시가 붙는 카드는 오른쪽 여백을 넓힌다(`pr-8`)
    fn when_base_project(self, is_base_project: bool) -> Self {
        if is_base_project {
            self.pr(spacing(8.0))
        } else {
            self
        }
    }
}

impl<E: Styled> BaseProjectPadding for E {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_data_urls_decode_to_the_original_bytes() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi");
        assert!(data_url_image("data:image/png;base64,aGk=").is_some());
        assert!(data_url_image("data:text/plain,hi").is_none());
    }

    #[test]
    fn project_colors_fall_back_to_the_default_blue() {
        assert_eq!(parse_project_color("#ff0000"), rgb(0xff0000));
        assert_eq!(parse_project_color("red"), rgb(0x93C5FD));
    }
}
