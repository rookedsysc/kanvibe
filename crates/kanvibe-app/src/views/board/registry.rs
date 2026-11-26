//! 프로젝트 등록 대화상자(TS `ProjectRegistryDialog`). 스캔 폼(호스트·폴더 경로)과 등록된 프로젝트 목록·검색을 보인다.
//! 열려 있는 동안 보드 단축키를 막고, Esc나 바탕·닫기 버튼으로 닫는다.
//!
//! 스캔·삭제는 런타임 `ProjectService`가 맡는다(동작은 `registry_actions`).

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::project::Project;
use kanvibe_process::project_search::matches_project_search;

use super::BoardView;
use super::folder_search::FolderListView;
use super::header::{qa_button_layer, qa_text_layer};
use super::icons::{BoardIcon, board_icon};
use super::registry_actions::RegistryState;
use super::task_card::project_marker;
use crate::components::dialog::MODAL_LAYER_PRIORITY;
use crate::components::{TailwindText, box_shadows};
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
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
        // TS는 닫힌 대화상자를 내리므로 다시 열면 상태가 새로 시작한다
        self.registry = RegistryState {
            generation: self.registry.generation + 1,
            ..RegistryState::default()
        };
        let input = self.registry.folder.input.clone();
        self.registry_path
            .update(cx, |field, cx| field.set_text(input, cx));
        self.registry_search
            .update(cx, |field, cx| field.set_text("", cx));
        self.dialog_focus.focus(window, cx);
        self.request_folder_listing(cx);
        cx.notify();
    }

    pub(super) fn close_project_registry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                .map(|project| {
                    self.registry_row(theme, &labels, project, displays.get(&project.id), cx)
                })
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
            .child(self.render_scan_form(theme, &labels, &common, &ssh_hosts, cx))
            .children(self.render_scan_outcome(theme, &labels, cx))
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
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let host_select =
            (!ssh_hosts.is_empty()).then(|| self.render_host_select(theme, common, ssh_hosts, cx));
        let is_pending = self.registry.scan.is_pending;
        let scan_label = labels.translate(if is_pending { "scanning" } else { "scanButton" });
        let hover_background = theme.brand.hover;
        let mut scan_button = div()
            .id("registry-scan")
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
            .child(qa_button_layer(&scan_label, &scan_label));
        scan_button = if is_pending {
            scan_button.opacity(0.5)
        } else {
            scan_button
                .cursor_pointer()
                .hover(move |style| style.bg(hover_background))
                .on_click(cx.listener(|this, _, _, cx| this.submit_registry_scan(cx)))
        };
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
            .child(self.render_folder_input(theme, labels, cx))
            .child(scan_button)
    }

    /// 스캔 대상 PC(TS 네이티브 `select`): 로컬과 SSH 호스트
    fn render_host_select(
        &self,
        theme: &ThemeTokens,
        common: &Translator,
        ssh_hosts: &[String],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let local_label = common.translate("local");
        let selected = self
            .registry
            .folder
            .ssh_host()
            .map(str::to_string)
            .unwrap_or_else(|| local_label.clone());
        let options: Vec<(Option<String>, String)> = std::iter::once((None, local_label))
            .chain(
                ssh_hosts
                    .iter()
                    .map(|host| (Some(host.clone()), host.clone())),
            )
            .collect();
        let menu = self.registry.is_host_menu_open.then(|| {
            let items = options
                .into_iter()
                .enumerate()
                .map(|(index, (host, label))| {
                    div()
                        .id(("registry-host-option", index))
                        .relative()
                        .px(spacing(3.0))
                        .py(px(6.0))
                        .cursor_pointer()
                        .hover(|style| style.bg(theme.background.page))
                        .child(label.clone())
                        .child(qa_button_layer(&label, &label))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_scan_host(host.clone(), cx)
                        }))
                });
            deferred(
                anchored().child(
                    div()
                        .occlude()
                        .mt(px(38.0))
                        .w(px(FORM_CONTENT_WIDTH))
                        .rounded(radius::MD)
                        .border_1()
                        .border_color(theme.border.default)
                        .bg(theme.background.surface)
                        .shadow(box_shadows(&theme.shadow.lg))
                        .text_scale(text_scale::SM)
                        .text_color(theme.text.primary)
                        .children(items),
                ),
            )
            .with_priority(MODAL_LAYER_PRIORITY + 1)
        });
        div()
            .id("registry-host-select")
            .relative()
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
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.registry.is_host_menu_open = !this.registry.is_host_menu_open;
                cx.notify();
            }))
            .child(selected.clone())
            .child(qa_target_layer(QaTargetDescription {
                role: "combobox",
                name: selected.clone().into(),
                text: selected.into(),
            }))
            .child(div().ml_auto().child(board_icon(
                BoardIcon::ChevronDown,
                theme.text.primary,
                14.0,
            )))
            .children(menu)
    }

    /// 경로 입력칸(TS `FolderSearchInput`)과 하위 폴더 드롭다운
    fn render_folder_input(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let dropdown = self
            .registry
            .folder
            .is_open
            .then(|| self.render_folder_dropdown(theme, labels, cx));
        div()
            .id("registry-folder-input")
            .relative()
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.registry.folder.is_open = true;
                    this.request_folder_listing(cx);
                    cx.notify();
                }),
            )
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
            .children(dropdown)
    }

    fn render_folder_dropdown(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let folder = &self.registry.folder;
        let (parent_path, _) = folder.parent_and_term();
        let breadcrumb = if parent_path == "/" {
            "/".to_string()
        } else {
            format!("{parent_path}/")
        };
        let small = |text: String| {
            div()
                .px(spacing(3.0))
                .py(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(text)
        };
        let mut content: Vec<AnyElement> = Vec::new();
        match folder.view() {
            FolderListView::Loading => {
                content.push(small(labels.translate("loadingFolders")).into_any_element())
            }
            FolderListView::Failed(reason) => {
                let message = Translator::new(self.locale(), "common")
                    .translate_with("loadFailed", &[("reason", &reason)]);
                let retry = cx.listener(|this, _, _, cx| this.retry_folder_listing(cx));
                content.push(
                    div()
                        .m(spacing(2.0))
                        .child(super::inline_load_error(
                            theme,
                            self.locale(),
                            message,
                            "registry-folder-retry",
                            retry,
                        ))
                        .into_any_element(),
                );
            }
            FolderListView::Empty => {
                content.push(small(labels.translate("noFoldersFound")).into_any_element())
            }
            FolderListView::Directories(matches) => {
                let count = matches.len().to_string();
                content.push(
                    div()
                        .px(spacing(3.0))
                        .py(spacing(1.0))
                        .text_scale(text_scale::PX_10)
                        .text_color(theme.text.muted)
                        .child(labels.translate_with("folderCount", &[("count", &count)]))
                        .into_any_element(),
                );
                content.extend(matches.iter().enumerate().map(|(index, found)| {
                    self.folder_row(theme, found, index == folder.selected_index, index, cx)
                }));
            }
        }
        let tiny = |text: String| {
            div()
                .px(spacing(3.0))
                .py(px(6.0))
                .text_scale(text_scale::PX_10)
                .text_color(theme.text.muted)
                .child(text)
        };
        deferred(
            anchored().child(
                div()
                    .id("registry-folder-dropdown")
                    .occlude()
                    .mt(spacing(1.0))
                    .w(px(FORM_CONTENT_WIDTH))
                    .max_h(px(240.0))
                    .overflow_y_scroll()
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.border.default)
                    .bg(theme.background.surface)
                    .shadow(box_shadows(&theme.shadow.lg))
                    .child(
                        tiny(breadcrumb)
                            .mono(400)
                            .border_b_1()
                            .border_color(theme.border.default),
                    )
                    .children(content)
                    .child(
                        tiny(labels.translate("folderNavHint"))
                            .border_t_1()
                            .border_color(theme.border.default),
                    ),
            ),
        )
        .with_priority(MODAL_LAYER_PRIORITY + 1)
    }

    fn folder_row(
        &self,
        theme: &ThemeTokens,
        found: &kanvibe_process::project_search::FuzzyMatch,
        is_selected: bool,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let directory = found.path.clone();
        let highlight = gpui_kit::HighlightStyle {
            color: Some(theme.brand.primary.into()),
            font_weight: Some(gpui_kit::FontWeight::BOLD),
            ..gpui_kit::HighlightStyle::default()
        };
        let ranges: Vec<_> = crate::views::locale_shell::highlighted_byte_ranges(
            &found.path,
            &found.matched_indices,
        )
        .into_iter()
        .map(|range| (range, highlight))
        .collect();
        let selected_background = crate::components::fade(theme.brand.primary, 0.1);
        let row = div()
            .id(("registry-folder", index))
            .relative()
            .px(spacing(3.0))
            .py(px(6.0))
            .mono(400)
            .text_scale(text_scale::XS)
            .cursor_pointer()
            .hover(move |style| style.bg(selected_background))
            .child(gpui_kit::StyledText::new(found.path.clone()).with_highlights(ranges))
            .child(qa_button_layer(&found.path, &found.path))
            .on_click(cx.listener(move |this, _, _, cx| this.choose_folder(&directory, cx)));
        if is_selected {
            row.bg(selected_background)
                .text_color(theme.text.primary)
                .into_any_element()
        } else {
            row.text_color(theme.text.secondary).into_any_element()
        }
    }

    /// 스캔 실패(다시 시도), 스캔 결과 상자, 결과 안내 한 줄
    fn render_scan_outcome(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut parts = Vec::new();
        if let Some(reason) = self.registry.scan.failure.clone() {
            let message = labels.translate_with("scanFailed", &[("reason", &reason)]);
            let retry = cx.listener(|this, _, _, cx| this.retry_registry_scan(cx));
            parts.push(
                div()
                    .mt(spacing(3.0))
                    .child(super::inline_load_error(
                        theme,
                        self.locale(),
                        message,
                        "registry-scan-retry",
                        retry,
                    ))
                    .into_any_element(),
            );
        }
        if let Some(result) = &self.registry.scan.result {
            parts.push(scan_result_box(theme, labels, result).into_any_element());
        }
        if let Some(notice) = &self.registry.notice {
            let color = if notice.is_error {
                theme.status.error
            } else {
                theme.status.success
            };
            parts.push(
                div()
                    .relative()
                    .mt(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(color)
                    .child(notice.message.clone())
                    .child(qa_text_layer("status", &notice.message))
                    .into_any_element(),
            );
        }
        parts
    }

    fn registry_row(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        project: &Project,
        display: Option<&super::task_filters::ProjectDisplay>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let icon = project
            .icon_data_url
            .as_ref()
            .filter(|icon| !icon.is_empty())
            .and(display)
            .map(|display| project_marker(display, 14.0));
        let delete = &self.registry.delete;
        let is_deleting = delete
            .pending
            .as_ref()
            .is_some_and(|(project_id, _)| *project_id == project.id);
        let failure = delete
            .failed
            .as_ref()
            .filter(|(project_id, _, _)| delete.pending.is_none() && *project_id == project.id)
            .map(|(_, _, reason)| reason.clone());
        let delete_label = labels.translate(if is_deleting {
            "deletingProject"
        } else {
            "deleteProject"
        });
        let (project_id, project_name) = (project.id.clone(), project.name.clone());
        let mut delete_button = div()
            .id(gpui_kit::SharedString::from(format!(
                "registry-delete-{}",
                project.id
            )))
            .relative()
            .flex_none()
            .text_scale(text_scale::XS)
            .text_color(theme.status.error)
            .child(delete_label.clone())
            .child(qa_button_layer(&delete_label, &delete_label));
        delete_button = if delete.pending.is_some() {
            delete_button.opacity(0.5)
        } else {
            delete_button
                .cursor_pointer()
                .hover(|style| style.opacity(0.8))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.request_project_deletion(
                        project_id.clone(),
                        project_name.clone(),
                        window,
                        cx,
                    )
                }))
        };
        let failure_banner = failure.map(|reason| {
            let message = labels.translate_with("deleteProjectFailed", &[("reason", &reason)]);
            let retry = cx.listener(|this, _, _, cx| this.retry_project_deletion(cx));
            div().mt(spacing(2.0)).child(super::inline_load_error(
                theme,
                self.locale(),
                message,
                "registry-delete-retry",
                retry,
            ))
        });
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
                    .child(project_summary(theme, labels, project, icon))
                    .child(delete_button),
            )
            .children(failure_banner)
            .into_any_element()
    }
}

/// 스캔 폼 안쪽 폭(대화상자 672 - 테두리 2 - 본문 px-5 40 - 폼 테두리 2 - 폼 p-3 24)
const FORM_CONTENT_WIDTH: f32 = 604.0;

fn project_summary(
    theme: &ThemeTokens,
    labels: &Translator,
    project: &Project,
    icon: Option<AnyElement>,
) -> impl IntoElement {
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
        )
}

/// 스캔 결과 상자: 등록한 이름, 건너뛴 수, 등록하지 못한 저장소의 사유
fn scan_result_box(
    theme: &ThemeTokens,
    labels: &Translator,
    result: &kanvibe_services::project::ScanResult,
) -> impl IntoElement {
    let line = |color, text: String| {
        div()
            .relative()
            .text_scale(text_scale::XS)
            .text_color(color)
            .child(text.clone())
            .child(qa_text_layer("status", &text))
    };
    let registered = (!result.registered.is_empty()).then(|| {
        let text = format!(
            "{}: {}",
            labels.translate("registered"),
            result.registered.join(", ")
        );
        line(theme.status.success, text)
    });
    let skipped = (!result.skipped.is_empty()).then(|| {
        let text = format!(
            "{}: {}{}",
            labels.translate("skipped"),
            result.skipped.len(),
            labels.translate("skippedSuffix")
        );
        line(theme.text.muted, text)
    });
    let errors = (!result.errors.is_empty()).then(|| {
        let heading = format!(
            "{}: {}{}",
            labels.translate("errors"),
            result.errors.len(),
            labels.translate("errorsSuffix")
        );
        let items = result.errors.iter().map(|message| {
            div()
                .flex()
                .gap(spacing(2.0))
                .child("•")
                .child(line(theme.status.error, message.clone()))
        });
        div()
            .child(line(theme.status.error, heading))
            .child(div().mt(spacing(1.0)).pl(spacing(4.0)).children(items))
    });
    div()
        .mt(spacing(3.0))
        .flex()
        .flex_col()
        .gap(spacing(1.0))
        .rounded(radius::LG)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.page)
        .p(spacing(3.0))
        .children(registered)
        .children(skipped)
        .children(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::board::task_filters::tests::project;

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
