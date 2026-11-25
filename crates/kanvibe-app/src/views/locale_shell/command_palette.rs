//! 커맨드 팔레트(TS `CommandPaletteDialog`). `Mod+Shift+P`나 vim `:`로 열고, 글자로 명령을 거른 뒤 ↑↓·Enter로 실행한다.
//! 명령: 백그라운드 sync, 포커스된 카드를 다른 열로 옮기기, 칸반·워크스페이스 보기 전환.
//! 어느 경로에서든 열린다. 카드 이동은 보드가 떠 있고 열 때 카드에 포커스가 있었을 때만 보인다.

use gpui_kit::{
    AnyElement, Context, Focusable, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task::{TASK_STATUS_ORDER, TaskStatus};

use super::LocaleShell;
use crate::components::{TailwindText, box_shadows, fade};
use crate::fonts::MONO_FAMILY;
use crate::qa::targets::{qa_button_layer, qa_text_layer};
use crate::services::AppServices;
use crate::theme::{ShadowLayer, ThemeTokens, spacing, text_scale};
use crate::views::board::services::{BOARD_VIEW_MODE_KEY, BoardViewMode};
use crate::views::board::text_field::TextFieldEvent;

/// `z-[500]`: 보드 대화상자보다 위
pub(crate) const GLOBAL_DIALOG_LAYER_PRIORITY: usize = 500;
/// `pt-24`
const DIALOG_TOP_OFFSET: f32 = 96.0;
/// `max-w-md`
const PALETTE_WIDTH: f32 = 448.0;
/// `max-h-[420px]`
pub(super) const RESULT_LIST_MAX_HEIGHT: f32 = 420.0;
/// `rounded-2xl`
const DIALOG_RADIUS: f32 = 16.0;

/// `shadow-2xl`(Tailwind 기본 `0 25px 50px -12px rgb(0 0 0 / 0.25)`)
pub(crate) fn shadow_2xl() -> [ShadowLayer; 1] {
    [ShadowLayer {
        offset_y: 25.0,
        blur: 50.0,
        spread: -12.0,
        color: Rgba {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.25,
        },
    }]
}

/// 팔레트 명령
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    Sync,
    Move(TaskStatus),
    View(BoardViewMode),
}

/// 화면에 보이는 명령 한 줄
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub command: PaletteCommand,
    pub label: String,
    pub description: String,
    pub is_disabled: bool,
}

/// TS `commands`: sync, (옮길 작업이 있으면) 열 이동, 보기 전환. `move_statuses`는 옮길 수 있는 상태다
pub fn palette_items(
    locale: Locale,
    is_sync_busy: bool,
    move_statuses: &[TaskStatus],
) -> Vec<PaletteItem> {
    let labels = Translator::new(locale, "commandPalette");
    let columns = Translator::new(locale, "board.columns");
    let mut items = vec![PaletteItem {
        command: PaletteCommand::Sync,
        label: "sync".into(),
        description: labels.translate(if is_sync_busy {
            "syncRunning"
        } else {
            "syncLabel"
        }),
        is_disabled: is_sync_busy,
    }];
    items.extend(move_statuses.iter().map(|&status| PaletteItem {
        command: PaletteCommand::Move(status),
        label: format!("move {}", status.as_str()),
        description: labels.translate_with(
            "moveToStatusLabel",
            &[("status", &columns.translate(status.as_str()))],
        ),
        is_disabled: false,
    }));
    items.push(PaletteItem {
        command: PaletteCommand::View(BoardViewMode::Kanban),
        label: "view kanban".into(),
        description: labels.translate("viewKanbanLabel"),
        is_disabled: false,
    });
    items.push(PaletteItem {
        command: PaletteCommand::View(BoardViewMode::Workspace),
        label: "view workspace".into(),
        description: labels.translate("viewWorkspaceLabel"),
        is_disabled: false,
    });
    items
}

/// 명령어와 설명을 합친 글자에 검색어(소문자)가 들어 있는 명령만
pub fn filter_palette_items(items: Vec<PaletteItem>, query: &str) -> Vec<PaletteItem> {
    let normalized = query.trim().to_lowercase();
    if normalized.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|item| {
            format!("{} {}", item.label, item.description)
                .to_lowercase()
                .contains(&normalized)
        })
        .collect()
}

/// 열린 팔레트. `move_task_id`는 열 때 포커스돼 있던 카드나 열려 있던 작업 상세의 작업이다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CommandPaletteState {
    pub selected_index: usize,
    pub move_task_id: Option<String>,
    /// 작업 상세가 이동 대상이면 그 작업의 지금 상태. 그 상태로 옮기는 명령은 빼고, 이동은 작업 상세가 한다
    /// (TS `hasActiveTaskContext`·`activeTaskCurrentStatus`)
    pub active_task_status: Option<TaskStatus>,
}

impl CommandPaletteState {
    /// 옮길 수 있는 상태. 작업 상세는 지금 상태를 빼고, 보드 카드는 모든 상태다
    fn move_statuses(&self) -> Vec<TaskStatus> {
        if self.move_task_id.is_none() {
            return Vec::new();
        }
        TASK_STATUS_ORDER
            .into_iter()
            .filter(|status| Some(*status) != self.active_task_status)
            .collect()
    }
}

impl LocaleShell {
    pub fn is_command_palette_open(&self) -> bool {
        self.command_palette.is_some()
    }

    /// 지금 거른 명령 목록
    pub fn command_palette_items(&self, cx: &gpui_kit::App) -> Vec<PaletteItem> {
        let Some(state) = &self.command_palette else {
            return Vec::new();
        };
        let items = palette_items(
            self.locale,
            self.background_sync.read(cx).state().is_running(),
            &state.move_statuses(),
        );
        filter_palette_items(items, self.palette_field.read(cx).text())
    }

    /// TS `openCommandPalette`. 다른 덮개나 보드 대화상자가 열려 있으면 열지 않는다
    pub fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_open_overlay(cx) || self.has_board_blocker(cx) {
            return;
        }
        let active_task = self
            .task_detail()
            .and_then(|detail| detail.read(cx).palette_move_target());
        let focused_card = self
            .board()
            .and_then(|board| board.read(cx).focused_card(window));
        let (move_task_id, active_task_status) = match active_task {
            Some((task_id, status)) => (Some(task_id), Some(status)),
            None => (focused_card, None),
        };
        self.remember_focus(window, cx);
        self.command_palette = Some(CommandPaletteState {
            selected_index: 0,
            move_task_id,
            active_task_status,
        });
        self.palette_field
            .update(cx, |field, cx| field.set_text("", cx));
        let field_focus = self.palette_field.read(cx).focus_handle(cx);
        field_focus.focus(window, cx);
        cx.notify();
    }

    fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.take().is_none() {
            return;
        }
        self.restore_focus(window, cx);
        cx.notify();
    }

    pub(super) fn handle_palette_field_event(
        &mut self,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.command_palette_items(cx).len();
        let Some(state) = self.command_palette.as_mut() else {
            return;
        };
        match event {
            TextFieldEvent::Changed(_) => state.selected_index = 0,
            TextFieldEvent::NavigationKey(key) if key == "down" => {
                state.selected_index = (state.selected_index + 1).min(count.saturating_sub(1));
            }
            TextFieldEvent::NavigationKey(key) if key == "up" => {
                state.selected_index = state.selected_index.saturating_sub(1);
            }
            TextFieldEvent::NavigationKey(_) => {}
            TextFieldEvent::Submitted { .. } => {
                let index = state.selected_index.min(count.saturating_sub(1));
                if let Some(item) = self.command_palette_items(cx).get(index).cloned() {
                    self.run_palette_command(&item, window, cx);
                }
                return;
            }
            TextFieldEvent::Escaped => return self.close_command_palette(window, cx),
        }
        cx.notify();
    }

    pub(super) fn run_palette_command(
        &mut self,
        item: &PaletteItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if item.is_disabled {
            return;
        }
        let move_task_id = self
            .command_palette
            .as_ref()
            .and_then(|state| state.move_task_id.clone());
        let is_active_task_move = self
            .command_palette
            .as_ref()
            .is_some_and(|state| state.active_task_status.is_some());
        match item.command {
            PaletteCommand::Sync => {
                self.background_sync.update(cx, |sync, cx| sync.start(cx));
                self.close_command_palette(window, cx);
            }
            PaletteCommand::Move(status) if is_active_task_move => {
                self.close_command_palette(window, cx);
                if let Some(detail) = self.task_detail() {
                    detail.update(cx, |detail, cx| detail.move_status_from_palette(status, cx));
                }
            }
            PaletteCommand::Move(status) => {
                self.close_command_palette(window, cx);
                if let (Some(task_id), Some(board)) = (move_task_id, self.board()) {
                    board.update(cx, |board, cx| {
                        board.move_task_to_status(&task_id, status, window, cx)
                    });
                }
            }
            PaletteCommand::View(mode) => {
                self.command_palette = None;
                self.return_focus = None;
                switch_board_view(mode, &self.navigator, cx);
            }
        }
    }

    pub(super) fn render_command_palette(
        &mut self,
        theme: &ThemeTokens,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let state = self.command_palette.clone()?;
        let labels = Translator::new(self.locale, "commandPalette");
        let items = self.command_palette_items(cx);
        let selected = state.selected_index.min(items.len().saturating_sub(1));
        let rows: Vec<AnyElement> = if items.is_empty() {
            let empty = labels.translate("empty");
            vec![
                div()
                    .relative()
                    .px(spacing(4.0))
                    .py(spacing(6.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.muted)
                    .child(empty.clone())
                    .child(qa_text_layer("status", &empty))
                    .into_any_element(),
            ]
        } else {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| palette_row(theme, item, index, index == selected, cx))
                .collect()
        };
        let header = div()
            .flex()
            .items_center()
            .gap(spacing(2.0))
            .border_b_1()
            .border_color(theme.border.default)
            .px(spacing(4.0))
            .py(spacing(3.0))
            .text_scale(text_scale::SM)
            .child(
                div()
                    .flex_none()
                    .css_weight(500)
                    .text_color(theme.text.muted)
                    .child(">"),
            )
            .child(div().flex_1().min_w_0().child(self.palette_field.clone()));
        let panel = global_dialog_panel(theme, PALETTE_WIDTH)
            .child(header)
            .child(
                div()
                    .id("command-palette-results")
                    .max_h(px(RESULT_LIST_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(dialog_footer(theme, labels.translate("hint")));
        let close = cx.listener(|this, _, window, cx| this.close_command_palette(window, cx));
        Some(global_dialog_layer(
            theme,
            self.overlay_viewport(window, cx),
            "command-palette-backdrop",
            close,
            panel,
        ))
    }
}

/// 뷰를 저장한 뒤 보드 경로로 간다. 보드 경로는 저장된 뷰를 읽어 칸반이나 워크스페이스를 그린다
fn switch_board_view(
    mode: BoardViewMode,
    navigator: &crate::views::WindowNavigator,
    cx: &mut Context<LocaleShell>,
) {
    let value = match mode {
        BoardViewMode::Kanban => "kanban",
        BoardViewMode::Workspace => "workspace",
    };
    if let Some(runtime) = cx.global::<AppServices>().runtime()
        && let Err(error) = runtime
            .app_settings()
            .set_app_setting(BOARD_VIEW_MODE_KEY, value)
    {
        eprintln!("[kanvibe] Failed to save board view mode: {error}");
    }
    let navigator = navigator.clone();
    // 이동은 창 루트를 갱신하므로 덮개 갱신이 끝난 뒤로 미룬다
    cx.defer(move |cx| navigator.navigate("/", cx));
}

fn palette_row(
    theme: &ThemeTokens,
    item: &PaletteItem,
    index: usize,
    is_selected: bool,
    cx: &mut Context<LocaleShell>,
) -> AnyElement {
    let item_for_click = item.clone();
    let hover_background = theme.background.page;
    let row = div()
        .id(("command-palette-item", index))
        .relative()
        .flex()
        .w_full()
        .items_center()
        .gap(spacing(3.0))
        .px(spacing(4.0))
        .py(px(10.0))
        .text_scale(text_scale::SM)
        .child(
            div()
                .flex_none()
                .font_family(MONO_FAMILY)
                .text_scale(text_scale::XS)
                .css_weight(600)
                .text_color(theme.text.primary)
                .child(item.label.clone()),
        )
        .child(
            div()
                .relative()
                .min_w_0()
                .truncate()
                .text_color(theme.text.muted)
                .child(item.description.clone())
                .child(qa_text_layer("note", &item.description)),
        )
        .child(qa_button_layer(
            &format!("{} {}", item.label, item.description),
            &item.label,
        ))
        .on_mouse_move(cx.listener(move |this, _, _, cx| {
            if let Some(state) = this.command_palette.as_mut()
                && state.selected_index != index
            {
                state.selected_index = index;
                cx.notify();
            }
        }))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.run_palette_command(&item_for_click, window, cx)
        }));
    let row = if is_selected {
        row.bg(fade(theme.brand.primary, 0.1))
    } else {
        row.hover(move |style| style.bg(hover_background))
    };
    if item.is_disabled {
        row.opacity(0.6).into_any_element()
    } else {
        row.cursor_pointer().into_any_element()
    }
}

/// `relative w-full overflow-hidden rounded-2xl border bg-bg-surface shadow-2xl`
pub(crate) fn global_dialog_panel(theme: &ThemeTokens, max_width: f32) -> gpui_kit::Div {
    div()
        .occlude()
        .relative()
        .w_full()
        .max_w(px(max_width))
        .overflow_hidden()
        .rounded(px(DIALOG_RADIUS))
        .border_1()
        .border_color(theme.border.default)
        .bg(theme.background.surface)
        .shadow(box_shadows(&shadow_2xl()))
}

/// `border-t px-4 py-2 text-xs text-text-muted` 안내 줄
pub(crate) fn dialog_footer(theme: &ThemeTokens, hint: String) -> impl IntoElement {
    div()
        .border_t_1()
        .border_color(theme.border.default)
        .px(spacing(4.0))
        .py(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(theme.text.muted)
        .child(hint)
}

/// `fixed inset-0 z-[500] flex items-start justify-center bg-black/45 px-4 pt-24`. 바탕을 누르면 닫는다
pub(crate) fn global_dialog_layer(
    _theme: &ThemeTokens,
    viewport: gpui_kit::Size<gpui_kit::Pixels>,
    id: &'static str,
    on_close: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    panel: impl IntoElement,
) -> AnyElement {
    let backdrop = div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .on_click(on_close);
    deferred(
        anchored().position(point(px(0.0), px(0.0))).child(
            div()
                .occlude()
                .relative()
                .w(viewport.width)
                .h(viewport.height)
                .flex()
                .items_start()
                .justify_center()
                .bg(Rgba {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.45,
                })
                .px(spacing(4.0))
                .pt(px(DIALOG_TOP_OFFSET))
                .child(backdrop)
                .child(panel),
        ),
    )
    .with_priority(GLOBAL_DIALOG_LAYER_PRIORITY)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_commands_appear_only_with_a_focused_card() {
        let without = palette_items(Locale::Ko, false, &[]);
        let labels: Vec<&str> = without.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["sync", "view kanban", "view workspace"]);
        let with = palette_items(Locale::Ko, false, &TASK_STATUS_ORDER);
        assert_eq!(with.len(), 8);
        assert_eq!(with[1].label, "move todo");
        assert_eq!(with[1].description, "Todo 상태로 이동");
    }

    #[test]
    fn filter_matches_command_and_description_case_insensitively() {
        let items = palette_items(Locale::Ko, false, &TASK_STATUS_ORDER);
        let filtered = filter_palette_items(items, "MOVE review");
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].command,
            PaletteCommand::Move(TaskStatus::Review)
        );
        let none = filter_palette_items(palette_items(Locale::Ko, false, &[]), "zzz");
        assert!(none.is_empty());
    }

    #[test]
    fn an_open_task_detail_offers_every_status_but_its_own() {
        let state = CommandPaletteState {
            selected_index: 0,
            move_task_id: Some("task-1".into()),
            active_task_status: Some(TaskStatus::Todo),
        };
        let statuses = state.move_statuses();
        assert_eq!(statuses.len(), TASK_STATUS_ORDER.len() - 1);
        assert!(!statuses.contains(&TaskStatus::Todo));
    }

    #[test]
    fn sync_is_disabled_while_running() {
        let items = palette_items(Locale::Ko, true, &[]);
        assert!(items[0].is_disabled);
        assert_eq!(items[0].description, "동기화하는 중…");
    }
}
