//! 헤더 팝오버: 프로젝트 필터(TS `ProjectSelector` 다중 선택), 데스크톱 필터(`DesktopFilter`), 정렬(`BoardSortPicker`).
//! 바깥을 누르거나 Esc를 누르면 닫히고 포커스는 보드로 돌아간다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, deferred, div, px,
};
use kanvibe_domain::board_sort::{
    BoardSortDirection, BoardSortField, BoardSortKey, BoardSortPreference,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::project::Project;
use kanvibe_process::project_search::matches_project_search;

use super::BoardView;
use super::board_model::column;
use super::header::{HeaderLabels, qa_button_layer};
use super::icons::{BoardIcon, board_icon, board_icon_sized};
use super::task_card::project_marker;
use super::task_filters::{ProjectDisplay, desktop_key, pick_available_desktops};
use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::components::{TailwindText, box_shadows, fade};
use crate::qa::targets::{QaTargetDescription, qa_target_layer, qa_text_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

pub(super) const MAX_VISIBLE_CHIPS: usize = 2;
const SORT_FIELDS: [BoardSortField; 5] = [
    BoardSortField::Priority,
    BoardSortField::CreatedAt,
    BoardSortField::UpdatedAt,
    BoardSortField::Title,
    BoardSortField::Project,
];

fn sort_field_key(field: BoardSortField) -> &'static str {
    match field {
        BoardSortField::Priority => "priority",
        BoardSortField::CreatedAt => "createdAt",
        BoardSortField::UpdatedAt => "updatedAt",
        BoardSortField::Title => "title",
        BoardSortField::Project => "project",
    }
}

fn direction_key(direction: BoardSortDirection) -> &'static str {
    match direction {
        BoardSortDirection::Asc => "asc",
        BoardSortDirection::Desc => "desc",
    }
}

/// 고른 기준을 켜고 끈다. 새 기준은 맨 뒤에 오름차순으로 쌓인다
pub(super) fn toggle_sort_field(
    preference: &BoardSortPreference,
    field: BoardSortField,
) -> BoardSortPreference {
    let mut keys = preference.keys.clone();
    match keys.iter().position(|key| key.field == field) {
        Some(index) => {
            keys.remove(index);
        }
        None => keys.push(BoardSortKey {
            field,
            direction: BoardSortDirection::Asc,
        }),
    }
    BoardSortPreference { keys }
}

pub(super) fn toggle_sort_direction(
    preference: &BoardSortPreference,
    field: BoardSortField,
) -> BoardSortPreference {
    let keys = preference
        .keys
        .iter()
        .map(|key| {
            if key.field != field {
                return *key;
            }
            let direction = match key.direction {
                BoardSortDirection::Asc => BoardSortDirection::Desc,
                BoardSortDirection::Desc => BoardSortDirection::Asc,
            };
            BoardSortKey { direction, ..*key }
        })
        .collect();
    BoardSortPreference { keys }
}

/// 고른 프로젝트를 먼저, 나머지는 원래 순서대로
pub(super) fn ordered_project_options(
    projects: &[Project],
    selected_ids: &[String],
    query: &str,
) -> Vec<Project> {
    let matching: Vec<&Project> = projects
        .iter()
        .filter(|project| matches_project_search(project, query))
        .collect();
    let (selected, others): (Vec<&Project>, Vec<&Project>) = matching
        .into_iter()
        .partition(|project| selected_ids.contains(&project.id));
    selected.into_iter().chain(others).cloned().collect()
}

/// 데스크톱 필터 버튼 글자
pub(super) fn desktop_filter_label(view: &BoardView, labels: &HeaderLabels) -> String {
    let options = view.desktop_options(labels);
    let selected: Vec<&(String, String)> = options
        .iter()
        .filter(|(value, _)| view.filters.selected_desktops.contains(value))
        .collect();
    match selected.as_slice() {
        [] => labels.board.translate("allDesktops"),
        [single] => single.1.clone(),
        many => labels
            .board
            .translate_with("selectedDesktops", &[("count", &many.len().to_string())]),
    }
}

/// 트리거 오른쪽 끝에 맞춰 아래로 펼친다(`absolute right-0 mt-1`)
fn right_aligned_popover(panel: impl IntoElement) -> AnyElement {
    div()
        .absolute()
        .top_full()
        .right_0()
        .w_0()
        .child(
            deferred(
                gpui_kit::anchored()
                    .anchor(gpui_kit::Anchor::TopRight)
                    .snap_to_window()
                    .child(panel),
            )
            .with_priority(POPOVER_LAYER_PRIORITY),
        )
        .into_any_element()
}

fn popover_shell(theme: &ThemeTokens) -> gpui_kit::Div {
    div()
        .occlude()
        .rounded(radius::MD)
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .shadow(box_shadows(&theme.shadow.md))
}

fn checkbox(
    theme: &ThemeTokens,
    is_checked: bool,
    size: f32,
    rounded: gpui_kit::Pixels,
) -> gpui_kit::Div {
    let mark = div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(rounded)
        .border_1();
    if is_checked {
        mark.border_color(theme.brand.primary)
            .bg(theme.brand.primary)
            .child(board_icon_sized(
                BoardIcon::Check,
                theme.text.inverse,
                10.0,
                8.0,
            ))
    } else {
        mark.border_color(theme.border.default)
    }
}

/// `h-3.5 w-3.5 rounded-sm border border-border-strong`, 고르면 글자색 체크
fn desktop_checkbox(theme: &ThemeTokens, is_checked: bool) -> gpui_kit::Div {
    div()
        .size(px(14.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.0))
        .border_1()
        .border_color(theme.border.strong)
        .children(
            is_checked.then(|| board_icon_sized(BoardIcon::Check, theme.text.primary, 10.0, 8.0)),
        )
}

impl BoardView {
    /// `local` + 보드가 아는 SSH 호스트
    pub(super) fn desktop_options(&self, labels: &HeaderLabels) -> Vec<(String, String)> {
        let mut options = vec![("local".to_string(), labels.common.translate("local"))];
        if let Some(board) = &self.board {
            options.extend(
                board
                    .catalog
                    .desktop_hosts(&board.ssh_hosts, &board.tasks)
                    .into_iter()
                    .map(|host| (desktop_key(Some(&host)), host)),
            );
        }
        options
    }

    fn filterable_projects(&self) -> Vec<Project> {
        let Some(board) = &self.board else {
            return Vec::new();
        };
        let hosts = board.catalog.desktop_hosts(&board.ssh_hosts, &board.tasks);
        let active = pick_available_desktops(&self.filters.selected_desktops, &hosts);
        board.catalog.filterable_projects(&active)
    }

    pub(super) fn render_project_chips(&self, theme: &ThemeTokens) -> impl IntoElement + use<> {
        let (visible_names, hidden_count) = self.selected_project_chips();
        let chip = |label: String| {
            div()
                .flex()
                .flex_none()
                .max_w(px(120.0))
                .items_center()
                .gap(px(2.0))
                .rounded(radius::BASE)
                .bg(fade(theme.brand.primary, 0.1))
                .px(px(6.0))
                .py(px(2.0))
                .text_scale(text_scale::XS)
                .css_weight(500)
                .text_color(theme.brand.primary)
                .relative()
                .child(div().truncate().child(label.clone()))
                .child(qa_text_layer("status", &label))
        };
        let visible = visible_names.into_iter().map(|name| chip(name).child("×"));
        let overflow = (hidden_count > 0).then(|| chip(format!("+{hidden_count}")));
        div()
            .flex()
            .gap(spacing(1.0))
            .children(visible)
            .children(overflow)
    }

    pub(super) fn render_project_filter_popover(
        &mut self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.project_search.read(cx).text().to_string();
        let options = ordered_project_options(
            &self.filterable_projects(),
            &self.filters.selected_project_ids,
            &query,
        );
        let highlight = self.popover_highlight.min(options.len().saturating_sub(1));
        let displays = self
            .board
            .as_ref()
            .map(|board| board.catalog.displays.clone())
            .unwrap_or_default();
        let rows: Vec<AnyElement> = if options.is_empty() {
            vec![
                div()
                    .px(spacing(3.0))
                    .py(spacing(2.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(Translator::new(self.locale(), "board").translate("allProjects"))
                    .into_any_element(),
            ]
        } else {
            options
                .iter()
                .enumerate()
                .map(|(index, project)| {
                    self.render_project_option(
                        theme,
                        project,
                        displays.get(&project.id),
                        index == highlight,
                        cx,
                    )
                })
                .collect()
        };
        let panel = popover_shell(theme)
            .id("board-project-filter-panel")
            .w(spacing(64.0))
            .mt(px(4.0))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_popover(window, cx)))
            .child(
                div()
                    .p(spacing(2.0))
                    .border_b_1()
                    .border_color(theme.border.default)
                    .child(
                        div()
                            .px(spacing(2.0))
                            .py(spacing(1.0))
                            .h(px(30.0))
                            .rounded(radius::BASE)
                            .border_1()
                            .border_color(theme.brand.primary)
                            .bg(theme.background.page)
                            .text_scale(text_scale::SM)
                            .child(self.project_search.clone()),
                    ),
            )
            .child(
                div()
                    .id("board-project-options")
                    .max_h(spacing(48.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.project_options_scroll)
                    .children(rows),
            );
        deferred(gpui_kit::anchored().snap_to_window().child(panel))
            .with_priority(POPOVER_LAYER_PRIORITY)
            .into_any_element()
    }

    fn render_project_option(
        &self,
        theme: &ThemeTokens,
        project: &Project,
        display: Option<&ProjectDisplay>,
        is_highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_checked = self.filters.selected_project_ids.contains(&project.id);
        let project_id = project.id.clone();
        let hover_background = theme.background.page;
        let row = div()
            .id(gpui_kit::SharedString::from(format!(
                "project-option-{}",
                project.id
            )))
            .relative()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .px(spacing(3.0))
            .py(spacing(2.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.primary)
            .cursor_pointer()
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.toggle_project_selection(&project_id, cx)),
            )
            .child(checkbox(theme, is_checked, 16.0, radius::BASE))
            .children(
                project
                    .icon_data_url
                    .as_ref()
                    .filter(|icon| !icon.is_empty())
                    .and(display)
                    .map(|display| project_marker(display, 14.0)),
            )
            .child(
                div()
                    .flex()
                    .min_w_0()
                    .truncate()
                    .child(project.name.clone())
                    .children(project.ssh_host.as_ref().map(|host| {
                        div()
                            .ml(spacing(1.0))
                            .text_color(theme.text.muted)
                            .child(format!("({host})"))
                    })),
            )
            .child(qa_target_layer(QaTargetDescription {
                role: "option",
                name: project.name.clone().into(),
                text: project.name.clone().into(),
            }));
        if is_highlighted {
            row.bg(fade(theme.brand.primary, 0.1)).into_any_element()
        } else {
            row.hover(move |style| style.bg(hover_background))
                .into_any_element()
        }
    }

    pub(super) fn toggle_project_selection(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let selected = &mut self.filters.selected_project_ids;
        match selected.iter().position(|id| id == project_id) {
            Some(index) => {
                selected.remove(index);
            }
            None => selected.push(project_id.to_string()),
        }
        cx.notify();
    }

    /// 프로젝트 필터 검색칸 ↑↓·Enter·Esc
    pub(super) fn handle_project_filter_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = self.project_search.read(cx).text().to_string();
        let options = ordered_project_options(
            &self.filterable_projects(),
            &self.filters.selected_project_ids,
            &query,
        );
        let count = options.len();
        match key {
            // 키보드로 고른 항목은 목록 안에서 보이게 굴린다(TS `scrollIntoView({ block: "nearest" })`)
            "down" if count > 0 => {
                self.popover_highlight = (self.popover_highlight + 1) % count;
                self.project_options_scroll
                    .scroll_to_item(self.popover_highlight);
            }
            "up" if count > 0 => {
                self.popover_highlight = (self.popover_highlight + count - 1) % count;
                self.project_options_scroll
                    .scroll_to_item(self.popover_highlight);
            }
            "enter" => {
                if let Some(project) =
                    options.get(self.popover_highlight.min(count.saturating_sub(1)))
                {
                    let project_id = project.id.clone();
                    self.toggle_project_selection(&project_id, cx);
                }
            }
            "escape" => self.close_popover(window, cx),
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn render_desktop_filter_popover(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let all_label = labels.board.translate("allDesktops");
        let mut rows = vec![(String::new(), all_label)];
        rows.extend(self.desktop_options(labels));
        let highlight = self.popover_highlight;
        let is_keyboard_focus = self.is_popover_keyboard_navigated;
        let row_elements = rows.into_iter().enumerate().map(|(index, (value, label))| {
            let is_checked = if value.is_empty() {
                self.filters.selected_desktops.is_empty()
            } else {
                self.filters.selected_desktops.contains(&value)
            };
            let hover_background = theme.background.page;
            let row = div()
                .id(gpui_kit::SharedString::from(format!("desktop-{index}")))
                .relative()
                .flex()
                .min_h(spacing(9.0))
                .w_full()
                .items_center()
                .gap(spacing(3.0))
                .rounded(radius::BASE)
                .px(spacing(2.0))
                .py(px(6.0))
                .text_scale(text_scale::SM)
                .text_color(theme.text.primary)
                .cursor_pointer()
                .hover(move |style| style.bg(hover_background))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_desktop(&value, cx)))
                .child(desktop_checkbox(theme, is_checked))
                .child(div().truncate().child(label.clone()))
                .child(qa_target_layer(QaTargetDescription {
                    role: "checkbox",
                    name: label.clone().into(),
                    text: label.into(),
                }));
            // 키보드로 옮겨 온 행에만 `focus-visible:outline-2 outline-border-strong`을 그린다
            if is_keyboard_focus && index == highlight {
                row.border_2().border_color(theme.border.strong)
            } else {
                row
            }
        });
        let dialog_label = labels.board.translate("desktopFilter");
        let panel = popover_shell(theme)
            .id("board-desktop-filter-panel")
            .track_focus(&self.popover_focus)
            .on_key_down(cx.listener(Self::handle_desktop_filter_key))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_popover(window, cx)))
            .relative()
            .mt(px(4.0))
            .w(spacing(56.0))
            .max_h(spacing(64.0))
            .p(spacing(1.0))
            .children(row_elements)
            .child(qa_target_layer(QaTargetDescription {
                role: "dialog",
                name: dialog_label.into(),
                text: "".into(),
            }));
        right_aligned_popover(panel)
    }

    /// 빈 값은 "전체"다. 행 이름을 눌러도 선택이 바뀌고 메뉴는 열린 채 남는다
    pub(super) fn toggle_desktop(&mut self, desktop: &str, cx: &mut Context<Self>) {
        let mut desktops = self.filters.selected_desktops.clone();
        if desktop.is_empty() {
            desktops.clear();
        } else if let Some(index) = desktops.iter().position(|value| value == desktop) {
            desktops.remove(index);
        } else {
            desktops.push(desktop.to_string());
        }
        self.change_desktop_filter(desktops, cx);
    }

    /// 데스크톱 필터를 바꾸면 다른 데스크톱 프로젝트의 선택을 지워, 숨은 선택이 보드를 비우지 않게 한다
    pub(super) fn change_desktop_filter(&mut self, desktops: Vec<String>, cx: &mut Context<Self>) {
        self.filters.selected_desktops = desktops;
        if let Some(board) = &self.board {
            let hosts = board.catalog.desktop_hosts(&board.ssh_hosts, &board.tasks);
            let active = pick_available_desktops(&self.filters.selected_desktops, &hosts);
            let catalog = &board.catalog;
            self.filters.selected_project_ids.retain(|project_id| {
                let host = catalog
                    .by_id
                    .get(project_id)
                    .and_then(|project| project.ssh_host.clone());
                super::task_filters::is_on_desktops(host.as_deref(), &active)
            });
        }
        cx.notify();
    }

    fn handle_desktop_filter_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let labels = HeaderLabels::new(self.locale());
        let options = self.desktop_options(&labels);
        let count = options.len() + 1;
        match event.keystroke.key.as_str() {
            "down" => self.popover_highlight = (self.popover_highlight + 1) % count,
            "up" => self.popover_highlight = (self.popover_highlight + count - 1) % count,
            "enter" | "space" => {
                let value = match self.popover_highlight {
                    0 => String::new(),
                    index => options[index - 1].0.clone(),
                };
                self.toggle_desktop(&value, cx);
            }
            "escape" => self.close_popover(window, cx),
            _ => return,
        }
        self.is_popover_keyboard_navigated = true;
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn render_sort_popover(
        &mut self,
        theme: &ThemeTokens,
        labels: &HeaderLabels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preference = self
            .board
            .as_ref()
            .map(|board| board.sort_preference.clone())
            .unwrap_or_default();
        let sort = &labels.sort;
        let summary: AnyElement =
            if preference.keys.is_empty() {
                div()
                    .mb(spacing(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(sort.translate("hint"))
                    .into_any_element()
            } else {
                div()
                    .mb(spacing(2.0))
                    .flex()
                    .flex_col()
                    .gap(spacing(1.0))
                    .children(
                        preference.keys.iter().enumerate().map(|(index, key)| {
                            self.render_sort_key_row(theme, sort, index, *key, cx)
                        }),
                    )
                    .into_any_element()
            };
        let options = SORT_FIELDS.into_iter().map(|field| {
            let is_active = preference.keys.iter().any(|key| key.field == field);
            let label = sort.translate(&format!("fields.{}", sort_field_key(field)));
            let hover_background = theme.background.page;
            div()
                .id(sort_field_key(field))
                .relative()
                .flex()
                .items_center()
                .gap(spacing(2.0))
                .rounded(radius::BASE)
                .px(spacing(2.0))
                .py(px(6.0))
                .text_scale(text_scale::SM)
                .text_color(if is_active {
                    theme.text.primary
                } else {
                    theme.text.secondary
                })
                .cursor_pointer()
                .hover(move |style| style.bg(hover_background))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_sort(|preference| toggle_sort_field(preference, field), cx)
                }))
                .child(checkbox(theme, is_active, 16.0, radius::BASE))
                .child(label.clone())
                .child(qa_target_layer(QaTargetDescription {
                    role: "option",
                    name: label.clone().into(),
                    text: label.into(),
                }))
        });
        let clear_label = sort.translate("clear");
        let done_label = sort.translate("done");
        let is_clear_enabled = !preference.keys.is_empty();
        let footer = div()
            .mt(spacing(3.0))
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(theme.border.subtle)
            .pt(spacing(2.0))
            .text_scale(text_scale::XS)
            .child(
                div()
                    .id("sort-clear")
                    .relative()
                    .text_color(theme.text.muted)
                    .when_enabled(is_clear_enabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // 고른 기준이 없으면 버튼이 비활성이라 누르지 않은 것과 같다
                        if is_clear_enabled {
                            this.update_sort(|_| BoardSortPreference::default(), cx)
                        }
                    }))
                    .child(clear_label.clone())
                    .child(qa_button_layer(&clear_label, &clear_label)),
            )
            .child(
                div()
                    .id("sort-done")
                    .relative()
                    .css_weight(500)
                    .text_color(theme.brand.primary)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, window, cx| this.close_popover(window, cx)))
                    .child(done_label.clone())
                    .child(qa_button_layer(&done_label, &done_label)),
            );
        let panel = popover_shell(theme)
            .id("board-sort-panel")
            .rounded(radius::LG)
            .shadow(box_shadows(&theme.shadow.lg))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_popover(window, cx)))
            .mt(px(4.0))
            .w(spacing(72.0))
            .p(spacing(3.0))
            .child(summary)
            .child(div().flex().flex_col().children(options))
            .child(footer);
        right_aligned_popover(panel)
    }

    fn render_sort_key_row(
        &self,
        theme: &ThemeTokens,
        sort: &Translator,
        index: usize,
        key: BoardSortKey,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field_label = sort.translate(&format!("fields.{}", sort_field_key(key.field)));
        let direction_label = sort.translate(&format!(
            "directions.{}.{}",
            sort_field_key(key.field),
            direction_key(key.direction)
        ));
        let direction_icon = match key.direction {
            BoardSortDirection::Asc => BoardIcon::ArrowUp,
            BoardSortDirection::Desc => BoardIcon::ArrowDown,
        };
        let field = key.field;
        div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.page)
            .px(spacing(2.0))
            .py(px(6.0))
            .child(
                div()
                    .text_scale(text_scale::PX_11)
                    .css_weight(600)
                    .text_color(theme.text.muted)
                    .child((index + 1).to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.primary)
                    .child(field_label),
            )
            .child(
                div()
                    .id(gpui_kit::SharedString::from(format!(
                        "sort-direction-{index}"
                    )))
                    .flex()
                    .items_center()
                    .gap(spacing(1.0))
                    .rounded(radius::BASE)
                    .px(px(6.0))
                    .py(px(2.0))
                    .text_scale(text_scale::PX_11)
                    .text_color(theme.text.secondary)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_sort(|preference| toggle_sort_direction(preference, field), cx)
                    }))
                    .child(board_icon(direction_icon, theme.text.secondary, 14.0))
                    .child(direction_label),
            )
            .child(
                div()
                    .id(gpui_kit::SharedString::from(format!("sort-remove-{index}")))
                    .rounded(radius::BASE)
                    .p(px(2.0))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_sort(|preference| toggle_sort_field(preference, field), cx)
                    }))
                    .child(board_icon(BoardIcon::Close, theme.text.muted, 14.0)),
            )
            .into_any_element()
    }

    /// 정렬 설정을 바꾸고 AppSettings `board_sort_preference`에 남긴다
    pub(super) fn update_sort(
        &mut self,
        change: impl FnOnce(&BoardSortPreference) -> BoardSortPreference,
        cx: &mut Context<Self>,
    ) {
        let Some(board) = self.board.as_mut() else {
            return;
        };
        board.sort_preference = change(&board.sort_preference);
        if let Some(services) = &self.services
            && let Err(failure) = services.save_board_sort_preference(&board.sort_preference)
        {
            eprintln!("[kanvibe] failed to save board sort: {}", failure.reason);
        }
        cx.notify();
    }

    /// 화면에 늘어놓을 열(필터·정렬 적용)
    pub(super) fn displayed_tasks(&self) -> kanvibe_services::kanban::TasksByStatus {
        let Some(board) = &self.board else {
            return Default::default();
        };
        let hosts = board.catalog.desktop_hosts(&board.ssh_hosts, &board.tasks);
        super::board_model::displayed_tasks(
            &board.tasks,
            &self.filters,
            &board.catalog,
            &hosts,
            &board.sort_preference,
            &crate::text_collation::LocaleCollator::for_locale(self.locale()),
        )
    }

    /// 지금 보이는 카드 수(열 순서대로)
    pub(super) fn visible_card_ids(&self) -> Vec<Vec<String>> {
        let displayed = self.displayed_tasks();
        kanvibe_domain::task::TASK_STATUS_ORDER
            .into_iter()
            .map(|status| {
                column(&displayed, status)
                    .iter()
                    .map(|task| task.id.clone())
                    .collect()
            })
            .collect()
    }
}

trait EnabledStyle: Styled + InteractiveElement + Sized {
    /// `disabled:opacity-40`
    fn when_enabled(self, is_enabled: bool) -> Self {
        if is_enabled {
            self.cursor_pointer()
        } else {
            self.opacity(0.4)
        }
    }
}

impl<E: Styled + InteractiveElement> EnabledStyle for E {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::board::task_filters::tests::project;

    fn fields(preference: &BoardSortPreference) -> Vec<(BoardSortField, BoardSortDirection)> {
        preference
            .keys
            .iter()
            .map(|key| (key.field, key.direction))
            .collect()
    }

    // parity: TS-components-BoardSortPicker-22bbff
    #[test]
    fn picked_fields_stack_in_ascending_order() {
        let first = toggle_sort_field(&BoardSortPreference::default(), BoardSortField::Priority);
        let second = toggle_sort_field(&first, BoardSortField::Title);
        assert_eq!(
            fields(&second),
            [
                (BoardSortField::Priority, BoardSortDirection::Asc),
                (BoardSortField::Title, BoardSortDirection::Asc)
            ]
        );
    }

    // parity: TS-components-BoardSortPicker-23f4d0
    #[test]
    fn picking_a_chosen_field_again_removes_it() {
        let picked = toggle_sort_field(&BoardSortPreference::default(), BoardSortField::Priority);
        assert!(
            toggle_sort_field(&picked, BoardSortField::Priority)
                .keys
                .is_empty()
        );
    }

    // parity: TS-components-BoardSortPicker-be20e3
    #[test]
    fn direction_toggles_for_the_chosen_field_only() {
        let picked = toggle_sort_field(
            &toggle_sort_field(&BoardSortPreference::default(), BoardSortField::Priority),
            BoardSortField::Title,
        );
        let flipped = toggle_sort_direction(&picked, BoardSortField::Title);
        assert_eq!(
            fields(&flipped),
            [
                (BoardSortField::Priority, BoardSortDirection::Asc),
                (BoardSortField::Title, BoardSortDirection::Desc)
            ]
        );
    }

    // parity: TS-components-ProjectSelector-dce11a
    #[test]
    fn remote_projects_match_by_ssh_host() {
        let remote = Project {
            ssh_host: Some("devbox".into()),
            ..project("api")
        };
        assert!(matches_project_search(&remote, "devb"));
        assert!(!matches_project_search(&project("api"), "devb"));
    }

    // parity: TS-components-ProjectSelector-d2ad60
    #[test]
    fn projects_match_by_repo_path() {
        assert!(matches_project_search(&project("api"), "/repo/a"));
    }

    // parity: TS-components-ProjectSelector-d75674
    #[test]
    fn selected_projects_are_listed_first() {
        let projects = [project("a"), project("b"), project("c")];
        let ordered = ordered_project_options(&projects, &["c".to_string()], "");
        let ids: Vec<_> = ordered.iter().map(|project| project.id.as_str()).collect();
        assert_eq!(ids, ["c", "a", "b"]);
    }
}
