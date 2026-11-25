//! 카드 우클릭 메뉴(TS `TaskContextMenu.tsx`). 첫 항목에 포커스가 가고 ↑↓로 돌며, 상태 변경 항목에서 →로 상태 목록을 연다.
//! Enter·Space는 고른 항목을 실행하고 Esc는 메뉴(상태 목록이면 목록)를 닫는다. 바깥을 누르면 닫힌다.

use gpui_kit::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::{TASK_STATUS_ORDER, TaskStatus};

use super::BoardView;
use super::board_model::find_task;
use super::column::{status_dot_color, status_label};
use crate::components::TailwindText;
use crate::components::box_shadows;
use crate::components::context_menu::context_menu_item;
use crate::components::popover::POPOVER_LAYER_PRIORITY;
use crate::qa::targets::{QaTargetDescription, qa_target_layer};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

/// 메뉴 항목 순서(첫 항목은 브랜치 유무에 따라 분기 또는 브랜치 기반 TODO)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuEntry {
    Branch,
    StatusList,
    Delete,
}

/// `min-w-[220px]`
const MENU_MIN_WIDTH: f32 = 220.0;

const MENU_ENTRIES: [MenuEntry; 3] = [MenuEntry::Branch, MenuEntry::StatusList, MenuEntry::Delete];

impl BoardView {
    pub(super) fn render_context_menu(
        &mut self,
        theme: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.context_menu.clone()?;
        let board = self.board.as_ref()?;
        let task = find_task(&board.tasks, &menu.task_id)?.clone();
        let labels = Translator::new(self.locale(), "contextMenu");
        let column_labels = Translator::new(self.locale(), "board.columns");
        let has_branch = task.branch_name.is_some();
        let highlighted = (!menu.is_status_list_open).then_some(menu.highlighted_index);
        let first_label = labels.translate(if has_branch {
            "createBranchTodo"
        } else {
            "branchOff"
        });
        let mut items: Vec<AnyElement> = Vec::new();
        items.push(
            self.menu_item(
                theme,
                "menu-branch",
                first_label,
                false,
                highlighted == Some(0),
                cx,
            )
            .on_click(
                cx.listener(|this, _, window, cx| {
                    this.run_menu_entry(MenuEntry::Branch, window, cx)
                }),
            )
            .into_any_element(),
        );
        items.push(self.render_status_entry(
            theme,
            &labels,
            &column_labels,
            task.status,
            highlighted == Some(1),
            cx,
        ));
        if menu.is_status_list_open {
            items.push(self.render_status_list(
                theme,
                &labels,
                &column_labels,
                task.status,
                menu.highlighted_index,
                cx,
            ));
        }
        items.push(
            self.menu_item(
                theme,
                "menu-delete",
                labels.translate("delete"),
                true,
                highlighted == Some(2),
                cx,
            )
            .on_click(
                cx.listener(|this, _, window, cx| {
                    this.run_menu_entry(MenuEntry::Delete, window, cx)
                }),
            )
            .into_any_element(),
        );
        let panel = div()
            .id("task-context-menu")
            .track_focus(&self.menu_focus)
            .on_key_down(cx.listener(Self::handle_menu_key))
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_context_menu(false, window, cx)),
            )
            .occlude()
            .relative()
            .min_w(px(MENU_MIN_WIDTH))
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .py(spacing(1.0))
            .shadow(box_shadows(&theme.shadow.lg))
            .children(items)
            .child(qa_target_layer(QaTargetDescription {
                role: "menu",
                name: "".into(),
                text: "".into(),
            }));
        let menu_element = deferred(
            anchored()
                .position(menu.position)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(POPOVER_LAYER_PRIORITY);
        Some(menu_element.into_any_element())
    }

    fn menu_item(
        &self,
        theme: &ThemeTokens,
        id: &'static str,
        label: String,
        is_destructive: bool,
        is_highlighted: bool,
        _cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let item = context_menu_item(id, theme, label, is_destructive);
        if is_highlighted {
            item.bg(if is_destructive {
                gpui_kit::rgb(0xfef2f2)
            } else {
                theme.background.page
            })
        } else {
            item
        }
    }

    fn render_status_entry(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        column_labels: &Translator,
        current_status: TaskStatus,
        is_highlighted: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let change_label = labels.translate("changeStatus");
        let current_label = status_label(column_labels, current_status);
        let is_open = self
            .context_menu
            .as_ref()
            .is_some_and(|menu| menu.is_status_list_open);
        let hover_background = theme.background.page;
        let mut entry = div()
            .id("menu-status")
            .relative()
            .flex()
            .w_full()
            .items_center()
            .gap(spacing(2.0))
            .px(spacing(4.0))
            .py(spacing(2.0))
            .text_scale(text_scale::SM)
            .text_color(theme.text.primary)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .on_click(cx.listener(|this, _, window, cx| {
                this.run_menu_entry(MenuEntry::StatusList, window, cx)
            }))
            .child(div().min_w_0().flex_1().child(change_label.clone()))
            .child(
                div()
                    .flex()
                    .max_w(px(96.0))
                    .items_center()
                    .gap(px(6.0))
                    .rounded(radius::MD)
                    .border_1()
                    .border_color(theme.border.subtle)
                    .bg(theme.background.page)
                    .px(px(6.0))
                    .py(px(2.0))
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.secondary)
                    .child(
                        div()
                            .size(px(6.0))
                            .flex_none()
                            .rounded(radius::FULL)
                            .bg(status_dot_color(theme, current_status)),
                    )
                    .child(div().truncate().child(current_label.clone())),
            )
            .child(
                div()
                    .text_scale(text_scale::XS)
                    .text_color(theme.text.muted)
                    .child(if is_open { "⌃" } else { "⌄" }),
            )
            .child(qa_target_layer(QaTargetDescription {
                role: "menuitem",
                name: format!("{change_label} {current_label}").into(),
                text: change_label.into(),
            }));
        if is_highlighted {
            entry = entry.bg(theme.background.page);
        }
        entry.into_any_element()
    }

    fn render_status_list(
        &self,
        theme: &ThemeTokens,
        labels: &Translator,
        column_labels: &Translator,
        current_status: TaskStatus,
        highlighted_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let options = TASK_STATUS_ORDER
            .into_iter()
            .enumerate()
            .map(|(index, status)| {
                let label = status_label(column_labels, status);
                let is_current = status == current_status;
                let hover_background = theme.background.surface;
                let mut option = div()
                    .id(status.as_str())
                    .relative()
                    .flex()
                    .w_full()
                    .items_center()
                    .gap(spacing(2.0))
                    .rounded(radius::BASE)
                    .px(px(10.0))
                    .py(px(6.0))
                    .text_scale(text_scale::SM)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select_menu_status(status, window, cx)
                    }))
                    .child(
                        div()
                            .size(spacing(2.0))
                            .flex_none()
                            .rounded(radius::FULL)
                            .bg(status_dot_color(theme, status)),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(label.clone()))
                    .child(qa_target_layer(QaTargetDescription {
                        role: "menuitemradio",
                        name: label.clone().into(),
                        text: label.into(),
                    }));
                if is_current {
                    option = option
                        .bg(theme.brand.subtle)
                        .text_color(theme.text.brand)
                        .child(
                            div()
                                .size(px(6.0))
                                .rounded(radius::FULL)
                                .bg(theme.brand.primary),
                        );
                } else {
                    option = option
                        .text_color(theme.text.primary)
                        .hover(move |style| style.bg(hover_background));
                    if index == highlighted_index {
                        option = option.bg(theme.background.surface);
                    }
                }
                option
            });
        div()
            .mx(spacing(1.0))
            .mb(spacing(1.0))
            .rounded(radius::MD)
            .border_1()
            .border_color(theme.border.subtle)
            .bg(theme.background.page)
            .p(spacing(1.0))
            .children(options)
            .child(qa_target_layer(QaTargetDescription {
                role: "menu",
                name: labels.translate("changeStatus").into(),
                text: "".into(),
            }))
            .into_any_element()
    }

    fn run_menu_entry(&mut self, entry: MenuEntry, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.context_menu.clone() else {
            return;
        };
        match entry {
            MenuEntry::Branch => {
                self.context_menu = None;
                self.open_branch_action(&menu.task_id, window, cx);
            }
            MenuEntry::StatusList => {
                if let Some(menu) = self.context_menu.as_mut() {
                    menu.is_status_list_open = !menu.is_status_list_open;
                    menu.highlighted_index = if menu.is_status_list_open { 0 } else { 1 };
                }
            }
            MenuEntry::Delete => {
                self.close_context_menu(false, window, cx);
                self.confirm_and_delete_task(menu.task_id, window, cx);
            }
        }
        cx.notify();
    }

    fn select_menu_status(
        &mut self,
        status: TaskStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.context_menu.clone() else {
            return;
        };
        let current_status = self
            .board
            .as_ref()
            .and_then(|board| find_task(&board.tasks, &menu.task_id))
            .map(|task| task.status);
        if current_status == Some(status) {
            if let Some(menu) = self.context_menu.as_mut() {
                menu.is_status_list_open = false;
                menu.highlighted_index = 1;
            }
            cx.notify();
            return;
        }
        self.close_context_menu(false, window, cx);
        self.move_task_to_status(&menu.task_id, status, window, cx);
    }

    fn handle_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.context_menu.clone() else {
            return;
        };
        let key = event.keystroke.key.as_str();
        cx.stop_propagation();
        if menu.is_status_list_open {
            self.handle_status_list_key(key, menu.highlighted_index, window, cx);
            return;
        }
        let count = MENU_ENTRIES.len();
        let index = menu.highlighted_index;
        let next_index = match key {
            "down" => Some((index + 1) % count),
            "up" => Some((index + count - 1) % count),
            _ => None,
        };
        if let Some(next_index) = next_index {
            self.set_menu_highlight(next_index, false, cx);
            return;
        }
        match key {
            "right" if index == 1 => self.set_menu_highlight(0, true, cx),
            "enter" | "space" => self.run_menu_entry(MENU_ENTRIES[index], window, cx),
            "escape" => self.close_context_menu(true, window, cx),
            _ => {}
        }
    }

    fn handle_status_list_key(
        &mut self,
        key: &str,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = TASK_STATUS_ORDER.len();
        match key {
            "down" => self.set_menu_highlight((index + 1) % count, true, cx),
            "up" => self.set_menu_highlight((index + count - 1) % count, true, cx),
            "home" => self.set_menu_highlight(0, true, cx),
            "end" => self.set_menu_highlight(count - 1, true, cx),
            "enter" | "space" => self.select_menu_status(TASK_STATUS_ORDER[index], window, cx),
            "left" | "escape" => self.set_menu_highlight(1, false, cx),
            _ => {}
        }
    }

    fn set_menu_highlight(
        &mut self,
        index: usize,
        is_status_list_open: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = self.context_menu.as_mut() {
            menu.highlighted_index = index;
            menu.is_status_list_open = is_status_list_open;
        }
        cx.notify();
    }
}
