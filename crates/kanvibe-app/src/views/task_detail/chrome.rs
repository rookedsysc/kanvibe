//! 터미널 위 띠(TS `bg-terminal-chrome` 줄): 프로젝트 색 작업 배지, 탭 바(또는 세션 이름), 알림 버튼.

use gpui_kit::{
    AnyElement, App, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kanvibe_process::project_color::{compute_project_color, readable_text_color};

use super::data::TaskDetailSnapshot;
use crate::components::icon::{IconName, icon};
use crate::components::tooltip::text_tooltip;
use crate::components::{TailwindText, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 배지에 그릴 프로젝트 표시 값(TS `taskContextProject`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContextProject {
    pub name: String,
    pub icon_data_url: Option<String>,
    /// `#rrggbb`. 프로젝트 색이 없으면 이름 해시 색
    pub background_color: String,
    pub text_color: String,
}

impl TaskContextProject {
    pub fn of(snapshot: &TaskDetailSnapshot) -> Option<Self> {
        let project = snapshot.project.as_ref()?;
        let background_color = project
            .color
            .clone()
            .filter(|color| !color.is_empty())
            .unwrap_or_else(|| compute_project_color(&project.name).to_string());
        Some(Self {
            name: project.name.clone(),
            icon_data_url: project.icon_data_url.clone(),
            text_color: readable_text_color(&background_color).to_string(),
            background_color,
        })
    }
}

/// `#rrggbb`을 색으로. 형식이 다르면 검정
pub fn css_hex(color: &str) -> Rgba {
    let digits = color.trim().trim_start_matches('#');
    let value = match digits.len() {
        6 => u32::from_str_radix(digits, 16).ok(),
        _ => None,
    };
    gpui_kit::rgb(value.unwrap_or(0))
}

/// 배지 제목(`프로젝트 | 작업`)
pub fn task_context_title(project: Option<&TaskContextProject>, task_title: &str) -> String {
    match project {
        Some(project) => format!("{} | {task_title}", project.name),
        None => task_title.to_string(),
    }
}

/// `flex max-w-96 items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-semibold`.
/// 프로젝트가 없으면 `bg-green-600 text-white`로 작업 이름만 보인다
pub fn render_task_context(
    project: Option<&TaskContextProject>,
    task_title: &str,
) -> impl IntoElement {
    let (background, text) = match project {
        Some(project) => (
            css_hex(&project.background_color),
            css_hex(&project.text_color),
        ),
        None => (gpui_kit::rgb(0x16a34a), gpui_kit::rgb(0xffffff)),
    };
    let mut pill = div()
        .id("terminal-task-context")
        .flex()
        .flex_none()
        .min_w_0()
        .max_w(px(384.0))
        .items_center()
        .gap(spacing(1.5))
        .rounded(radius::MD)
        .px(spacing(2.5))
        .py(spacing(1.0))
        .text_scale(text_scale::XS)
        .css_weight(600)
        .bg(background)
        .text_color(text)
        .tooltip(text_tooltip(task_context_title(project, task_title)));
    if let Some(project) = project {
        pill = pill
            .children(render_project_icon(
                &project.name,
                project.icon_data_url.as_deref(),
                Some(&project.text_color),
            ))
            .child(
                div()
                    .flex_none()
                    .max_w(px(128.0))
                    .truncate()
                    .child(project.name.clone()),
            )
            .child(div().flex_none().opacity(0.5).child("|"));
    }
    pill.child(div().min_w_0().truncate().child(task_title.to_string()))
}

/// TS `ProjectIcon`. GitHub 아이콘이 있으면 그 그림, 없으면 색 위 첫 글자 칩, 색도 없으면 그리지 않는다
pub fn render_project_icon(
    project_name: &str,
    icon_data_url: Option<&str>,
    color: Option<&str>,
) -> Option<AnyElement> {
    if let Some(image) = icon_data_url.and_then(super::project_icon::data_url_image) {
        return Some(
            gpui_kit::img(image)
                .size(px(14.0))
                .flex_none()
                .rounded(radius::SM)
                .into_any_element(),
        );
    }
    let initial = project_initial(project_name)?;
    let color = color?;
    Some(
        div()
            .size(px(14.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(radius::SM)
            .text_size(px(9.0))
            .line_height(px(9.0))
            .css_weight(700)
            .bg(css_hex(color))
            .text_color(css_hex(readable_text_color(color)))
            .child(initial.to_uppercase())
            .into_any_element(),
    )
}

/// TS `getProjectInitial`: 앞뒤 공백을 뗀 첫 글자(코드 포인트)
pub fn project_initial(project_name: &str) -> Option<String> {
    project_name.trim().chars().next().map(String::from)
}

/// 알림 버튼(`relative rounded-md p-1.5 text-terminal-text hover:text-white hover:bg-white/10`)
pub fn render_notification_button(
    theme: &ThemeTokens,
    label: String,
    unread_count: usize,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id("terminal-notification-button")
        .relative()
        .rounded(radius::MD)
        .p(spacing(1.5))
        .cursor_pointer()
        .text_color(theme.terminal.text)
        .hover(|style| style.bg(fade(gpui_kit::rgb(0xffffff), 0.1)))
        .tooltip(text_tooltip(label.clone()))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(icon(IconName::Bell, px(20.0)).text_color(theme.terminal.text))
        .children(
            crate::notifications::unread_badge_label(unread_count).map(|badge| {
                div()
                    .absolute()
                    .top(px(-4.0))
                    .right(px(-4.0))
                    .min_w(px(16.0))
                    .h(px(16.0))
                    .px(spacing(1.0))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(theme.brand.primary)
                    .text_size(px(10.0))
                    .css_weight(600)
                    .text_color(gpui_kit::rgb(0xffffff))
                    .child(badge)
            }),
        )
        .child(qa_target_layer(QaTargetDescription {
            role: "button",
            name: label.into(),
            text: String::new().into(),
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // parity: TS-components-ProjectIcon-5e5fb8
    #[test]
    fn missing_icons_fall_back_to_a_colored_initial() {
        assert_eq!(project_initial("  kanvibe").as_deref(), Some("k"));
        assert!(render_project_icon("kanvibe", None, Some("#8B5CF6")).is_some());
    }

    // parity: TS-components-ProjectIcon-15b7cf
    #[test]
    fn nothing_is_drawn_without_icon_or_color() {
        assert!(render_project_icon("kanvibe", None, None).is_none());
        assert!(render_project_icon("   ", None, Some("#8B5CF6")).is_none());
    }

    // parity: TS-components-ProjectIcon-89c5e0
    #[test]
    fn dark_project_colors_draw_white_initials() {
        assert_eq!(readable_text_color("#1f2937"), "#FFFFFF");
        assert_eq!(readable_text_color("#FDE047"), "#111827");
    }

    // parity: TS-renderer-TaskDetailRoute-d46cbf
    #[test]
    fn context_badge_title_joins_project_and_task_names() {
        let project = TaskContextProject {
            name: "kanvibe-demo".into(),
            icon_data_url: None,
            background_color: "#8B5CF6".into(),
            text_color: "#FFFFFF".into(),
        };
        assert_eq!(
            task_context_title(Some(&project), "보드 필터 추가"),
            "kanvibe-demo | 보드 필터 추가"
        );
        assert_eq!(task_context_title(None, "보드 필터 추가"), "보드 필터 추가");
    }
}
