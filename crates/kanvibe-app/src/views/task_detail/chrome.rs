//! 터미널 위 띠(TS `bg-terminal-chrome` 줄): 프로젝트 색 작업 배지, 탭 바(또는 세션 이름). 알림 버튼은 공용 알림 센터를 쓴다.

use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, Rgba, StatefulInteractiveElement,
    Styled, div, px,
};
use kanvibe_process::project_color::{compute_project_color, readable_text_color};

use super::data::TaskDetailSnapshot;
use crate::components::TailwindText;
use crate::components::tooltip::text_tooltip;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::text_baseline::{TextFamily, baseline_lift};
use crate::theme::{radius, spacing, text_scale};

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
        snapshot.project.as_ref().map(Self::from_project)
    }

    fn from_project(project: &kanvibe_domain::project::Project) -> Self {
        let background_color = project
            .color
            .clone()
            .filter(|color| !color.is_empty())
            .unwrap_or_else(|| compute_project_color(&project.name).to_string());
        Self {
            name: project.name.clone(),
            icon_data_url: project.icon_data_url.clone(),
            text_color: readable_text_color(&background_color).to_string(),
            background_color,
        }
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
    let (background, text) = badge_colors(project);
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
        .relative()
        .tooltip(text_tooltip(task_context_title(project, task_title)))
        .child(qa_target_layer(QaTargetDescription {
            role: "group",
            name: task_context_title(project, task_title).into(),
            text: task_title.to_string().into(),
        }));
    if let Some(project) = project {
        pill = pill.children(render_project_prefix(project));
    }
    pill.child(lifted_text(task_title).min_w_0().truncate())
}

/// 프로젝트 아이콘, 이름(`max-w-32 truncate`), 구분선(`opacity-50`)
fn render_project_prefix(project: &TaskContextProject) -> Vec<AnyElement> {
    let mut prefix: Vec<AnyElement> = render_project_icon(
        &project.name,
        project.icon_data_url.as_deref(),
        Some(&project.text_color),
    )
    .into_iter()
    .collect();
    prefix.push(
        lifted_text(&project.name)
            .flex_none()
            .max_w(px(128.0))
            .truncate()
            .into_any_element(),
    );
    prefix.push(lifted_text("|").flex_none().opacity(0.5).into_any_element());
    prefix
}

/// 기준선을 Chromium 행으로 올린 배지 글자 한 덩이
fn lifted_text(text: &str) -> gpui_kit::Div {
    div()
        .relative()
        .top(chromium_lift(text))
        .child(text.to_string())
}

/// 배지 글자(`text-xs`)를 Chromium 기준선 행으로 올리는 거리
fn chromium_lift(text: &str) -> gpui_kit::Pixels {
    px(-baseline_lift(text, text_scale::XS, TextFamily::Sans))
}

/// 배지 배경·글자색. 프로젝트가 없으면 `bg-green-600 text-white`
fn badge_colors(project: Option<&TaskContextProject>) -> (Rgba, Rgba) {
    match project {
        Some(project) => (
            css_hex(&project.background_color),
            css_hex(&project.text_color),
        ),
        None => (gpui_kit::rgb(0x16a34a), gpui_kit::rgb(0xffffff)),
    }
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

    fn project(color: Option<&str>) -> kanvibe_domain::project::Project {
        kanvibe_domain::project::Project {
            id: "project-1".into(),
            name: "kanvibe".into(),
            repo_path: "/repo".into(),
            default_branch: "main".into(),
            ssh_host: None,
            is_worktree: false,
            color: color.map(str::to_string),
            icon_data_url: None,
            created_at: String::new(),
        }
    }

    // parity: TS-renderer-TaskDetailRoute-cf0648
    #[test]
    fn badge_uses_the_project_color_and_inverts_the_initial_chip() {
        let badge = TaskContextProject::from_project(&project(Some("#86EFAC")));
        assert_eq!(css_hex(&badge.background_color), gpui_kit::rgb(0x86efac));
        assert_eq!(css_hex(&badge.text_color), gpui_kit::rgb(0x111827));
        // 첫 글자 칩은 배지 글자색으로 칠해 배지 배경과 겹치지 않는다
        assert!(render_project_icon(&badge.name, None, Some(&badge.text_color)).is_some());
    }

    // parity: TS-renderer-TaskDetailRoute-7ce018
    #[test]
    fn badge_without_a_project_color_uses_the_name_hash_color() {
        let badge = TaskContextProject::from_project(&project(None));
        assert_eq!(css_hex(&badge.background_color), gpui_kit::rgb(0x5eead4));
    }

    // parity: TS-renderer-TaskDetailRoute-d84407
    #[test]
    fn task_without_a_project_shows_only_its_title_on_the_green_badge() {
        assert_eq!(
            task_context_title(None, "fix tab task name"),
            "fix tab task name"
        );
        assert_eq!(
            badge_colors(None),
            (gpui_kit::rgb(0x16a34a), gpui_kit::rgb(0xffffff))
        );
    }
}
