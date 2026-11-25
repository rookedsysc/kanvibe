//! 칸반 카드(TS `TaskCard.tsx` + `TaskCardContent.tsx` + `TaskDiffStatsBadge`).
//! 카드는 포커스를 받는 링크다. 누르면 작업 상세로 가고(같은 작업 창이 있으면 그 창을 앞으로), 우클릭·Shift+F10은 메뉴를,
//! Shift+Enter는 새 창 상세를 연다. 삭제 중·이동 저장 중에는 끌 수 없다.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::{
    AnyElement, AppContext, Context, Div, Entity, Image, ImageFormat, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Rgba, StatefulInteractiveElement, Styled, Window, div,
    img, px, rgb,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::{KanbanTask, TaskPriority};
use kanvibe_domain::task_diff_stats::TaskDiffStats;
use kanvibe_process::project_color::readable_text_color;

use super::BoardView;
use super::board_model::{TaskDeletionState, TaskMoveState};
use super::count_up::CountUpNumber;
use super::icons::{BoardIcon, board_icon};
use super::keyboard::FindField;
use super::task_filters::{ProjectDisplay, is_inherited_priority, resolve_effective_priority};
use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{
    QaTargetBehavior, QaTargetDescription, qa_target_layer, qa_target_layer_with,
};
use crate::text_baseline::{TextFamily, baseline_lift};
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
        let icon_label = format!("{} icon", display.name);
        return div()
            .relative()
            .flex_none()
            .child(img(image).size(px(size)).rounded(px(2.0)))
            .child(badge_label_layer("img", &icon_label, ""))
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
        .relative()
        .child(initial.clone())
        .child(qa_target_layer_with(
            QaTargetDescription {
                role: "img",
                name: display.name.clone().into(),
                text: initial.into(),
            },
            QaTargetBehavior {
                selector: Some("[data-testid='project-initial-icon']".into()),
                ..QaTargetBehavior::default()
            },
        ))
        .into_any_element()
}

/// 찾기 바가 고른 글자 구간을 선택 색으로 칠한다(TS `window.find`의 선택 강조)
const FIND_SELECTION_BACKGROUND: u32 = 0x3390ff;

impl BoardView {
    fn find_highlighted(&self, task_id: &str, field: FindField, text: String) -> AnyElement {
        let found = self
            .find_match
            .as_ref()
            .filter(|found| found.task_id == task_id && found.field == field)
            .filter(|found| text.get(found.range.clone()).is_some());
        let Some(found) = found else {
            return text.into_any_element();
        };
        let highlight = gpui_kit::HighlightStyle {
            background_color: Some(gpui_kit::rgb(FIND_SELECTION_BACKGROUND).into()),
            ..gpui_kit::HighlightStyle::default()
        };
        gpui_kit::StyledText::new(text)
            .with_highlights(vec![(found.range.clone(), highlight)])
            .into_any_element()
    }
}

/// GPUI가 Chromium보다 글자를 아래에 찍는 만큼 글자 상자를 올린다(`text_baseline`). 배경 없는 글자 상자에만 쓴다
fn baseline_offset(text: &str, font_size: f32, line_height: f32) -> gpui_kit::Pixels {
    let scale = crate::theme::TextScale {
        font_size: px(font_size),
        line_height: px(line_height),
    };
    px(-baseline_lift(text, scale, TextFamily::Sans))
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
    counters: Option<&[Entity<CountUpNumber>; 3]>,
    summary: String,
) -> Option<impl IntoElement + use<>> {
    if stats.file_count == 0 {
        return None;
    }
    let number = |index: usize, value: i64| -> AnyElement {
        match counters {
            Some(counters) => counters[index].clone().into_any_element(),
            None => value.to_string().into_any_element(),
        }
    };
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
                    .child(number(0, stats.file_count)),
            )
            .child(
                div()
                    .flex()
                    .text_color(theme.status.success)
                    .child("+")
                    .child(number(1, stats.additions)),
            )
            .child(
                div()
                    .flex()
                    .text_color(theme.status.error)
                    .child("-")
                    .child(number(2, stats.deletions)),
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

/// 카드 전체(TS `data-kanban-task-card`·`data-kanban-status` 속성이 붙은 `tabIndex` 요소). 재생 드라이버가 선택자로 찾아 포커스를 준다
fn card_qa_target(task: &KanbanTask, focus_handle: gpui_kit::FocusHandle) -> impl IntoElement {
    qa_target_layer_with(
        QaTargetDescription {
            role: "article",
            name: task.title.clone().into(),
            text: gpui_kit::SharedString::default(),
        },
        QaTargetBehavior {
            selector: Some(
                format!(
                    "[data-kanban-task-card='true'][data-kanban-status='{}']",
                    task.status.as_str()
                )
                .into(),
            ),
            focus_handle: Some(focus_handle),
            ..QaTargetBehavior::default()
        },
    )
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
        self.watch_card_focus(&task.id, &focus_handle, window, cx);
        let is_focused = focus_handle.is_focused(window);
        // 세션 패널은 카드 아래로 넘치므로 열려 있는 동안에는 카드가 잘라내지 않는다
        let is_live_panel_open = self.is_live_panel_open(&task.id);
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
            .when_clipped(!is_live_panel_open)
            .rounded(radius::MD)
            .border_1()
            .border_color(project_color.unwrap_or(theme.border.subtle))
            .px(px(10.0))
            .py(spacing(2.0))
            .cursor_pointer()
            .when_base_project(is_base_project(task))
            .children(self.task_card_content(theme, &input))
            .children(self.render_live_session_popover(theme, task, cx))
            .child(card_qa_target(task, focus_handle.clone()));
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
                    .relative()
                    .top(baseline_offset(&task.title, 13.0, 20.0))
                    .text_size(px(13.0))
                    .line_height(px(20.0))
                    .css_weight(500)
                    .text_color(theme.text.primary)
                    .child(self.find_highlighted(&task.id, FindField::Title, task.title.clone())),
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
                let first_line = text.lines().next().unwrap_or_default().to_string();
                div()
                    .mt(px(2.0))
                    .truncate()
                    .relative()
                    .top(baseline_offset(&first_line, 11.0, 16.0))
                    .text_size(px(11.0))
                    .line_height(px(16.0))
                    .text_color(theme.text.muted)
                    .child(self.find_highlighted(&task.id, FindField::Description, first_line))
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
                                        .relative()
                                        .top(baseline_offset(&display.name, 12.0, 16.0))
                                        .text_scale(text_scale::XS)
                                        .line_height(px(16.0))
                                        .css_weight(600)
                                        .text_color(color)
                                        .child(self.find_highlighted(
                                            &task.id,
                                            FindField::ProjectName,
                                            display.name.clone(),
                                        )),
                                )
                                .child(badge_label_layer("text", &display.name, ""))
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
        let counters = self.diff_counters.get(&task.id);
        let diff = input.diff_stats.and_then(|stats| {
            let summary = Translator::new(self.locale(), "taskDetail.diffStats").translate_with(
                "summary",
                &[
                    ("files", &stats.file_count.to_string()),
                    ("additions", &stats.additions.to_string()),
                    ("deletions", &stats.deletions.to_string()),
                ],
            );
            diff_stats_badge(theme, stats, counters, summary)
        });
        if let Some(diff) = diff {
            badges.push(diff.into_any_element());
        }
        badges.extend(self.render_running_agents(theme, task));
        if let Some(ssh_host) = &task.ssh_host {
            // 긴 호스트는 말줄임 대신 배지 안에서 줄을 바꿔 전부 보인다(`break-all`)
            badges.push(
                badge(theme, theme.tag.ssh)
                    .flex_shrink(1.0)
                    .min_w_0()
                    .relative()
                    .child(div().min_w_0().child(ssh_host.clone()))
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
        let hover_task_id = task.id.clone();
        let card = card
            .on_hover(cx.listener(move |this, is_hovered: &bool, window, cx| {
                if *is_hovered {
                    this.activate_live_panel(&hover_task_id, window, cx);
                } else {
                    this.deactivate_live_panel(&hover_task_id, cx);
                }
            }))
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

trait CardClip: Styled + Sized {
    /// 평소에는 카드 밖으로 넘치는 내용을 자른다(`overflow-hidden`)
    fn when_clipped(self, is_clipped: bool) -> Self {
        if is_clipped {
            self.overflow_hidden()
        } else {
            self
        }
    }
}

impl<E: Styled> CardClip for E {}

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
