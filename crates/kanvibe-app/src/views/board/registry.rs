//! 프로젝트 등록 대화상자(TS `ProjectRegistryDialog`). 스캔 폼(호스트·폴더 경로)과 등록된 프로젝트 목록·검색을 보인다.
//! 열려 있는 동안 보드 단축키를 막고, Esc나 바탕·닫기 버튼으로 닫는다.
//!
//! 스캔·삭제는 `ProjectService`가 맡는다. 이 서비스는 hook 설치·AI 세션 읽기 같은 앱 공용 협력 객체가 있어야 만들 수 있어
//! 앱 조립 지점에 요청해 두었다. 그 전까지 두 버튼은 진행하지 않는다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::project::Project;
use kanvibe_process::project_search::matches_project_search;

use super::BoardView;
use super::header::{qa_button_layer, qa_text_layer};
use super::icons::{BoardIcon, board_icon};
use super::task_card::project_marker;
use crate::components::dialog::MODAL_LAYER_PRIORITY;
use crate::components::{TailwindText, box_shadows};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 대화상자 폭(`max-w-2xl`)
const REGISTRY_WIDTH: f32 = 672.0;

/// 검색어에 맞는 등록 프로젝트
pub(super) fn registry_projects(projects: &[Project], query: &str) -> Vec<Project> {
    projects
        .iter()
        .filter(|project| matches_project_search(project, query))
        .cloned()
        .collect()
}

/// 목록 머리 개수: 검색 중이면 `걸린 수 / 전체`
pub(super) fn registry_count_label(matched: usize, total: usize, query: &str) -> String {
    if query.trim().is_empty() {
        total.to_string()
    } else {
        format!("{matched} / {total}")
    }
}

impl BoardView {
    pub(super) fn open_project_registry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_registry_open = true;
        self.registry_search
            .update(cx, |field, cx| field.set_text("", cx));
        self.dialog_focus.focus(window, cx);
        cx.notify();
    }

    fn close_project_registry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.is_registry_open = false;
        self.board_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn render_project_registry(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.is_registry_open {
            return None;
        }
        let labels = Translator::new(self.locale(), "settings");
        let common = Translator::new(self.locale(), "common");
        let projects = self
            .board
            .as_ref()
            .map(|board| board.catalog.projects.clone())
            .unwrap_or_default();
        let displays = self
            .board
            .as_ref()
            .map(|board| board.catalog.displays.clone())
            .unwrap_or_default();
        let ssh_hosts = self
            .board
            .as_ref()
            .map(|board| board.ssh_hosts.clone())
            .unwrap_or_default();
        let query = self.registry_search.read(cx).text().to_string();
        let matched = registry_projects(&projects, &query);
        let title = labels.translate("scanTitle");
        let close_label = common.translate("close");
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(theme.border.default)
            .px(spacing(5.0))
            .py(spacing(4.0))
            .child(
                div()
                    .relative()
                    .text_scale(text_scale::SM)
                    .css_weight(600)
                    .text_color(theme.text.primary)
                    .child(title.clone())
                    .child(qa_text_layer("heading", &title)),
            )
            .child(
                div()
                    .id("registry-close")
                    .relative()
                    .text_scale(text_scale::LG)
                    .text_color(theme.text.muted)
                    .cursor_pointer()
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_project_registry(window, cx)),
                    )
                    .child("×")
                    .child(qa_button_layer(&close_label, "")),
            );
        let list_title = format!(
            "{} ({})",
            labels.translate("projectList"),
            registry_count_label(matched.len(), projects.len(), &query)
        );
        let rows: Vec<AnyElement> = if projects.is_empty() {
            let message = labels.translate("noProjects");
            vec![
                div()
                    .relative()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(message.clone())
                    .child(qa_text_layer("status", &message))
                    .into_any_element(),
            ]
        } else if matched.is_empty() {
            let message = labels.translate("noMatchingProjects");
            vec![
                div()
                    .relative()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(message.clone())
                    .child(qa_text_layer("status", &message))
                    .into_any_element(),
            ]
        } else {
            matched
                .iter()
                .map(|project| registry_row(theme, &labels, project, displays.get(&project.id)))
                .collect()
        };
        let search = (!projects.is_empty()).then(|| {
            let placeholder = labels.translate("projectSearchPlaceholder");
            div()
                .relative()
                .h(px(34.0))
                .flex()
                .items_center()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.page)
                .px(spacing(3.0))
                .text_scale(text_scale::SM)
                .child(self.registry_search.clone())
                .child(qa_text_layer("searchbox", &placeholder))
        });
        let body = div()
            .id("registry-body")
            .min_h_0()
            .overflow_y_scroll()
            .px(spacing(5.0))
            .py(spacing(4.0))
            .child(self.render_scan_form(theme, &labels, &common, &ssh_hosts))
            .child(
                div()
                    .mt(spacing(5.0))
                    .flex()
                    .flex_col()
                    .gap(spacing(3.0))
                    .child(
                        div()
                            .text_scale(text_scale::XS)
                            .text_color(theme.text.muted)
                            .child(list_title),
                    )
                    .children(search)
                    .child(div().flex().flex_col().gap(spacing(2.0)).children(rows)),
            );
        let viewport = self.overlay_viewport(window);
        let dialog = div()
            .id("project-registry")
            .track_focus(&self.dialog_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.close_project_registry(window, cx);
                }
            }))
            .occlude()
            .relative()
            .flex()
            .flex_col()
            .w(px(REGISTRY_WIDTH))
            .max_h(viewport.height * 0.85)
            .overflow_hidden()
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .shadow(box_shadows(&theme.shadow.lg))
            .child(header)
            .child(body)
            .child(qa_text_layer("dialog", &title));
        let backdrop = div()
            .id("registry-backdrop")
            .occlude()
            .w(viewport.width)
            .h(viewport.height)
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background.overlay)
            .on_click(cx.listener(|this, _, window, cx| this.close_project_registry(window, cx)))
            .child(dialog.on_click(|_, _, cx| cx.stop_propagation()));
        Some(
            deferred(anchored().position(point(px(0.0), px(0.0))).child(backdrop))
                .with_priority(MODAL_LAYER_PRIORITY)
                .into_any_element(),
        )
    }

    fn render_scan_form(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        common: &Translator,
        ssh_hosts: &[String],
    ) -> impl IntoElement + use<> {
        let host_select = (!ssh_hosts.is_empty()).then(|| {
            div()
                .h(px(34.0))
                .flex()
                .items_center()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .px(spacing(3.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.primary)
                .child(common.translate("local"))
                .child(div().ml_auto().child(board_icon(
                    BoardIcon::ChevronDown,
                    theme.text.primary,
                    14.0,
                )))
        });
        let scan_label = labels.translate("scanButton");
        div()
            .flex()
            .flex_col()
            .gap(spacing(3.0))
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.page)
            .p(spacing(3.0))
            .children(host_select)
            .child(
                div()
                    .h(px(34.0))
                    .flex()
                    .items_center()
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.border.default)
                    .bg(theme.background.page)
                    .px(spacing(3.0))
                    .mono(400)
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.primary)
                    .child(self.registry_path.clone()),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .justify_center()
                    .rounded(radius::MD)
                    .bg(theme.brand.primary)
                    .py(px(6.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.inverse)
                    .child(scan_label.clone())
                    .child(qa_button_layer(&scan_label, &scan_label)),
            )
    }
}

fn registry_row(
    theme: &ThemeTokens,
    labels: &Translator,
    project: &Project,
    display: Option<&super::task_filters::ProjectDisplay>,
) -> AnyElement {
    let icon = project
        .icon_data_url
        .as_ref()
        .filter(|icon| !icon.is_empty())
        .and(display)
        .map(|display| project_marker(display, 14.0));
    let delete_label = labels.translate("deleteProject");
    div()
        .relative()
        .rounded(radius::MD)
        .bg(theme.background.page)
        .p(spacing(2.0))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(spacing(3.0))
                .child(
                    div()
                        .min_w_0()
                        .child(
                            div()
                                .relative()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .text_scale(text_scale::SM)
                                .css_weight(500)
                                .text_color(theme.text.primary)
                                .children(icon)
                                .child(project.name.clone())
                                .child(qa_text_layer("listitem", &project.name)),
                        )
                        .child(
                            div()
                                .flex()
                                .mono(400)
                                .text_scale(text_scale::XS)
                                .text_color(theme.text.muted)
                                .children(project.ssh_host.as_ref().map(|host| {
                                    div()
                                        .text_color(theme.tag.ssh.text)
                                        .child(format!("{host}:"))
                                }))
                                .child(project.repo_path.clone()),
                        )
                        .child(
                            div()
                                .text_scale(text_scale::XS)
                                .text_color(theme.text.muted)
                                .child(format!(
                                    "{}: {}",
                                    labels.translate("defaultBranch"),
                                    project.default_branch
                                )),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .text_scale(text_scale::XS)
                        .text_color(theme.status.error)
                        .child(delete_label.clone())
                        .child(qa_button_layer(&delete_label, &delete_label)),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::board::task_filters::tests::project;

    // parity: TS-components-ProjectRegistryDialog-4a4b2a
    #[test]
    fn search_keeps_only_matching_projects() {
        let projects = [project("kanvibe-demo"), project("api-server")];
        let matched = registry_projects(&projects, "api");
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "api-server");
        assert_eq!(registry_count_label(1, 2, "api"), "1 / 2");
        assert_eq!(registry_count_label(2, 2, ""), "2");
    }
}
