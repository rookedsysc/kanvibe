//! 커맨드 팔레트(TS `CommandPaletteDialog`). `Mod+Shift+P`나 vim `:`로 열고, 글자로 명령을 거른 뒤 ↑↓·Enter로 실행한다.
//! 명령: 백그라운드 sync, 포커스된 카드를 다른 열로 옮기기, 칸반·워크스페이스 보기 전환.

use gpui_kit::{
    AnyElement, Context, Focusable, InteractiveElement, IntoElement, ParentElement, Rgba,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, px,
};
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::task::{TASK_STATUS_ORDER, TaskStatus};

use super::BoardView;
use super::header::{qa_button_layer, qa_text_layer};
use super::services::{BOARD_VIEW_MODE_KEY, BoardViewMode};
use super::text_field::TextFieldEvent;
use crate::components::{TailwindText, box_shadows, fade};
use crate::fonts::MONO_FAMILY;
use crate::services::AppServices;
use crate::theme::{ShadowLayer, ThemeTokens, spacing, text_scale};

/// `z-[500]`: 보드 대화상자보다 위
pub(super) const GLOBAL_DIALOG_LAYER_PRIORITY: usize = 500;
/// `pt-24`
const DIALOG_TOP_OFFSET: f32 = 96.0;
/// `max-w-md`
const PALETTE_WIDTH: f32 = 448.0;
/// `max-h-[420px]`
pub(super) const RESULT_LIST_MAX_HEIGHT: f32 = 420.0;
/// `rounded-2xl`
pub(super) const DIALOG_RADIUS: f32 = 16.0;

/// `shadow-2xl`(Tailwind 기본 `0 25px 50px -12px rgb(0 0 0 / 0.25)`)
pub(super) fn shadow_2xl() -> [ShadowLayer; 1] {
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

/// TS `commands`: sync, (옮길 카드가 있으면) 열 이동, 보기 전환
pub fn palette_items(
    locale: Locale,
    is_sync_busy: bool,
    has_move_target: bool,
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
    if has_move_target {
        items.extend(TASK_STATUS_ORDER.into_iter().map(|status| PaletteItem {
            command: PaletteCommand::Move(status),
            label: format!("move {}", status.as_str()),
            description: labels.translate_with(
                "moveToStatusLabel",
                &[("status", &columns.translate(status.as_str()))],
            ),
            is_disabled: false,
        }));
    }
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

/// 열린 팔레트. `move_task_id`는 열 때 포커스돼 있던 카드다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CommandPaletteState {
    pub selected_index: usize,
    pub move_task_id: Option<String>,
}

impl BoardView {
    pub fn is_command_palette_open(&self) -> bool {
        self.command_palette.is_some()
    }

    /// 지금 거른 명령 목록
    pub fn command_palette_items(&self, cx: &gpui_kit::App) -> Vec<PaletteItem> {
        let Some(state) = &self.command_palette else {
            return Vec::new();
        };
        let items = palette_items(
            self.locale(),
            self.background_sync.is_running(),
            state.move_task_id.is_some(),
        );
        filter_palette_items(items, self.palette_field.read(cx).text())
    }

    pub(super) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.is_some() || self.is_quick_search_open() {
            return;
        }
        let move_task_id = self.focused_card(window);
        self.command_palette = Some(CommandPaletteState {
            selected_index: 0,
            move_task_id,
        });
        self.palette_field
            .update(cx, |field, cx| field.set_text("", cx));
        let field_focus = self.palette_field.read(cx).focus_handle(cx);
        field_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.command_palette.take() else {
            return;
        };
        self.restore_focus_after_overlay(state.move_task_id.as_deref(), window, cx);
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
        match item.command {
            PaletteCommand::Sync => {
                self.press_background_sync_button(cx);
                self.close_command_palette(window, cx);
            }
            PaletteCommand::Move(status) => {
                self.close_command_palette(window, cx);
                if let Some(task_id) = move_task_id {
                    self.move_task_to_status(&task_id, status, window, cx);
                }
            }
            PaletteCommand::View(mode) => {
                self.command_palette = None;
                switch_board_view(mode, self, cx);
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
        let labels = Translator::new(self.locale(), "commandPalette");
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
            self.overlay_viewport(window),
            "command-palette-backdrop",
            close,
            panel,
        ))
    }
}

/// 뷰를 저장한 뒤 보드 경로로 간다. 보드 경로는 저장된 뷰를 읽어 칸반이나 워크스페이스를 그린다
fn switch_board_view(mode: BoardViewMode, view: &BoardView, cx: &mut Context<BoardView>) {
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
    let navigator = view.screen.navigator.clone();
    navigator.navigate("/", cx);
}

fn palette_row(
    theme: &ThemeTokens,
    item: &PaletteItem,
    index: usize,
    is_selected: bool,
    cx: &mut Context<BoardView>,
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
pub(super) fn global_dialog_panel(theme: &ThemeTokens, max_width: f32) -> gpui_kit::Div {
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
pub(super) fn dialog_footer(theme: &ThemeTokens, hint: String) -> impl IntoElement {
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
pub(super) fn global_dialog_layer(
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
        let without = palette_items(Locale::Ko, false, false);
        let labels: Vec<&str> = without.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["sync", "view kanban", "view workspace"]);
        let with = palette_items(Locale::Ko, false, true);
        assert_eq!(with.len(), 8);
        assert_eq!(with[1].label, "move todo");
        assert_eq!(with[1].description, "Todo 상태로 이동");
    }

    #[test]
    fn filter_matches_command_and_description_case_insensitively() {
        let items = palette_items(Locale::Ko, false, true);
        let filtered = filter_palette_items(items, "MOVE review");
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].command,
            PaletteCommand::Move(TaskStatus::Review)
        );
        let none = filter_palette_items(palette_items(Locale::Ko, false, false), "zzz");
        assert!(none.is_empty());
    }

    #[test]
    fn sync_is_disabled_while_running() {
        let items = palette_items(Locale::Ko, true, false);
        assert!(items[0].is_disabled);
        assert_eq!(items[0].description, "동기화하는 중…");
    }
}
