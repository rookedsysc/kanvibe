//! 보드 키보드: 카드 포커스 이동(방향키, vim hjkl), vim `n`·`:`·`/`·`dd`, 카드의 Enter·Shift+Enter·Shift+F10,
//! 앱 단축키 동작(알림 센터·프로젝트 필터·데스크톱 필터·찾기·새 작업), 그리고 찾기 바(TS `BoardPageFindBar`의 `window.find` 대신).
//!
//! 입력칸·메뉴·대화상자에 포커스가 있으면 vim 키를 쓰지 않는다. 그래서 vim 키는 보드 루트나 카드에 포커스가 있을 때만 본다.

use std::time::{Duration, Instant};

use gpui_kit::{
    AnyElement, Context, Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, point, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::TASK_STATUS_ORDER;

use super::BoardView;
use super::HeaderPopover;
use super::board_model::find_task;
use super::header::qa_button_layer;
use super::text_field::TextFieldEvent;
use crate::components::{TailwindText, box_shadows};
use crate::shortcuts::{
    OpenBoardPageFind, OpenCommandPalette, OpenCreateTask, OpenDesktopFilter, OpenProjectFilter,
    ToggleNotificationCenter,
};
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::windows::{TaskNavigationRequest, navigate_to_task_detail};

pub const BOARD_KEY_CONTEXT: &str = "BoardRoute";
/// `dd`의 두 번째 `d`를 기다리는 시간
const DELETE_SEQUENCE_TIMEOUT: Duration = Duration::from_millis(1_000);
/// Shift+F10 메뉴는 카드 왼쪽 위에서 이만큼 안쪽에 뜬다
const KEYBOARD_MENU_OFFSET: f32 = 12.0;

/// 카드 포커스 이동 방향
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FocusMove {
    Up,
    Down,
    Left,
    Right,
}

/// 방향키는 항상, hjkl은 vim 모드에서 수식키 없이 눌렀을 때만 이동이다
pub(super) fn focus_move_for_key(
    key: &str,
    has_modifier: bool,
    is_vim_mode_enabled: bool,
) -> Option<FocusMove> {
    match key {
        "up" => Some(FocusMove::Up),
        "down" => Some(FocusMove::Down),
        "left" => Some(FocusMove::Left),
        "right" => Some(FocusMove::Right),
        _ if has_modifier || !is_vim_mode_enabled => None,
        "k" => Some(FocusMove::Up),
        "j" => Some(FocusMove::Down),
        "h" => Some(FocusMove::Left),
        "l" => Some(FocusMove::Right),
        _ => None,
    }
}

/// 열별 카드 목록에서 다음 포커스 위치. 위·아래는 같은 열 안에서 멈추고, 좌우는 같은 순번(없으면 마지막)의 이웃 열 카드로 간다.
/// 빈 열은 건너뛴다
pub(super) fn next_focus_position(
    columns: &[Vec<String>],
    column_index: usize,
    row_index: usize,
    direction: FocusMove,
) -> Option<(usize, usize)> {
    let clamp_row = |column: usize, row: usize| -> Option<(usize, usize)> {
        let length = columns.get(column)?.len();
        (length > 0).then(|| (column, row.min(length - 1)))
    };
    match direction {
        FocusMove::Up => clamp_row(column_index, row_index.saturating_sub(1)),
        FocusMove::Down => clamp_row(column_index, row_index + 1),
        FocusMove::Left => (0..column_index)
            .rev()
            .find_map(|column| clamp_row(column, row_index)),
        FocusMove::Right => {
            (column_index + 1..columns.len()).find_map(|column| clamp_row(column, row_index))
        }
    }
}

/// 찾기 바 상태. `has_match`가 `Some(false)`면 일치 없음 안내를 보인다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct FindBarState {
    pub is_open: bool,
    pub has_match: Option<bool>,
    pub current_match: Option<usize>,
}

/// 카드 안 찾을 글자 칸. 화면 순서(프로젝트 이름 → 제목 → 설명)대로 늘어놓는다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindField {
    ProjectName,
    Title,
    Description,
}

/// 찾기 바가 고른 일치 하나(TS `window.find`가 선택하는 글자 구간)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindTextMatch {
    pub task_id: String,
    pub field: FindField,
    /// 그 칸 글자의 바이트 구간
    pub range: std::ops::Range<usize>,
}

/// 글자 안에서 검색어가 나오는 모든 구간(대소문자 없음, 겹치지 않게 앞에서부터)
pub fn match_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let characters: Vec<(usize, char)> = text.char_indices().collect();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start + needle.len() <= characters.len() {
        let is_match = needle.iter().enumerate().all(|(offset, expected)| {
            characters[start + offset]
                .1
                .to_lowercase()
                .eq(std::iter::once(*expected))
        });
        if !is_match {
            start += 1;
            continue;
        }
        let end_index = start + needle.len();
        let end = characters
            .get(end_index)
            .map(|(byte, _)| *byte)
            .unwrap_or(text.len());
        ranges.push(characters[start].0..end);
        start = end_index;
    }
    ranges
}

/// 카드 설명은 첫 줄만 보인다
pub(super) fn displayed_description(task: &kanvibe_domain::task::KanbanTask) -> Option<String> {
    task.description
        .as_ref()
        .filter(|text| !text.is_empty())
        .map(|text| text.lines().next().unwrap_or_default().to_string())
}

/// 보이는 카드에서 검색어가 나오는 글자 구간을 화면 순서대로 모은다
pub(super) fn find_text_matches(view: &BoardView, query: &str) -> Vec<FindTextMatch> {
    let Some(board) = &view.board else {
        return Vec::new();
    };
    if query.is_empty() {
        return Vec::new();
    }
    let displayed = view.displayed_tasks();
    let mut matches = Vec::new();
    for task in TASK_STATUS_ORDER
        .into_iter()
        .flat_map(|status| super::board_model::column(&displayed, status).clone())
    {
        let project_name = task
            .project_id
            .as_ref()
            .and_then(|id| board.catalog.displays.get(id))
            .map(|display| display.name.clone());
        let fields = [
            (FindField::ProjectName, project_name),
            (FindField::Title, Some(task.title.clone())),
            (FindField::Description, displayed_description(&task)),
        ];
        for (field, text) in fields {
            let Some(text) = text else { continue };
            matches.extend(
                match_ranges(&text, query)
                    .into_iter()
                    .map(|range| FindTextMatch {
                        task_id: task.id.clone(),
                        field,
                        range,
                    }),
            );
        }
    }
    matches
}

impl BoardView {
    pub(super) fn register_board_actions(
        &self,
        element: gpui_kit::Stateful<gpui_kit::Div>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        element
            .on_action(
                cx.listener(|this, _: &ToggleNotificationCenter, window, cx| {
                    this.notification_center
                        .update(cx, |center, cx| center.toggle_by_keyboard(window, cx))
                }),
            )
            .on_action(cx.listener(|this, _: &OpenProjectFilter, window, cx| {
                if this.open_popover != Some(HeaderPopover::ProjectFilter) {
                    this.toggle_popover(HeaderPopover::ProjectFilter, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &OpenDesktopFilter, window, cx| {
                if this.open_popover != Some(HeaderPopover::DesktopFilter) {
                    this.toggle_popover(HeaderPopover::DesktopFilter, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &OpenBoardPageFind, window, cx| {
                if !this.has_shortcut_blocker(cx) {
                    this.open_find_bar(window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &OpenCreateTask, window, cx| {
                if !this.has_shortcut_blocker(cx) {
                    this.open_create_task(None, window, cx)
                }
            }))
            .on_key_down(cx.listener(Self::handle_board_key_down))
    }

    /// 보드의 대화상자·메뉴가 열려 있는지(TS `registerShortcutBlocker`)
    pub(crate) fn has_board_blocker(&self) -> bool {
        self.context_menu.is_some()
            || self.pending_done_move.is_some()
            || self.is_board_dialog_open()
    }

    /// 보드 대화상자나 창의 전역 덮개(팔레트·빠른 검색·릴리스·sync 검토)가 열려 있으면 보드 키를 쓰지 않는다
    pub(super) fn has_shortcut_blocker(&self, cx: &gpui_kit::App) -> bool {
        self.has_board_blocker()
            || self
                .screen
                .navigator
                .locale_shell(cx)
                .is_some_and(|shell| shell.read(cx).has_open_overlay(cx))
    }

    pub(crate) fn focused_card(&self, window: &Window) -> Option<String> {
        self.card_focus
            .iter()
            .find(|(_, handle)| handle.is_focused(window))
            .map(|(task_id, _)| task_id.clone())
    }

    fn handle_board_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.board.is_none() || self.has_shortcut_blocker(cx) {
            return;
        }
        let focused_card = self.focused_card(window);
        let is_board_focused = self.board_focus.is_focused(window) || window.focused(cx).is_none();
        if focused_card.is_none() && !is_board_focused {
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let has_modifier = modifiers.control || modifiers.alt || modifiers.platform;
        let is_vim = self
            .board
            .as_ref()
            .is_some_and(|board| board.is_vim_mode_enabled);
        let key = keystroke.key.as_str();
        if key != "d" && !matches!(key, "shift" | "control" | "alt" | "platform") {
            self.pending_delete_sequence = None;
        }
        if let Some(task_id) = &focused_card
            && self.handle_card_key(task_id, key, modifiers.shift && !has_modifier, window, cx)
        {
            cx.stop_propagation();
            return;
        }
        if let Some(direction) = focus_move_for_key(key, has_modifier || modifiers.shift, is_vim) {
            self.move_card_focus(focused_card.as_deref(), direction, window, cx);
            cx.stop_propagation();
            return;
        }
        if !is_vim || has_modifier {
            return;
        }
        match keystroke.key_char.as_deref().unwrap_or(key) {
            "n" if !modifiers.shift => self.open_create_task(None, window, cx),
            ":" => window.dispatch_action(Box::new(OpenCommandPalette), cx),
            "/" => self.open_find_bar(window, cx),
            "d" if focused_card.is_some() => {
                self.advance_delete_sequence(focused_card.unwrap_or_default(), window, cx)
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// 카드에 포커스가 있을 때만 쓰는 키. 처리했으면 `true`
    fn handle_card_key(
        &mut self,
        task_id: &str,
        key: &str,
        is_shift_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match key {
            "enter" if is_shift_only => {
                let request = TaskNavigationRequest {
                    origin_window: Some(window.window_handle().window_id()),
                    should_open_new_window: true,
                };
                navigate_to_task_detail(task_id, self.locale(), request, cx);
            }
            "enter" => self.activate_task_card(task_id, false, window, cx),
            "f10" if is_shift_only => {
                let origin = self
                    .card_bounds
                    .borrow()
                    .get(task_id)
                    .map(|bounds| bounds.origin)
                    .unwrap_or_default();
                let position = origin + point(px(KEYBOARD_MENU_OFFSET), px(KEYBOARD_MENU_OFFSET));
                self.open_context_menu(task_id, position, window, cx);
            }
            _ => return false,
        }
        true
    }

    /// `d`를 1초 안에 두 번 누르면 삭제 확인을 띄운다
    fn advance_delete_sequence(
        &mut self,
        task_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_second_press =
            self.pending_delete_sequence
                .as_ref()
                .is_some_and(|(pending_id, started_at)| {
                    *pending_id == task_id && started_at.elapsed() < DELETE_SEQUENCE_TIMEOUT
                });
        if !is_second_press {
            self.pending_delete_sequence = Some((task_id, Instant::now()));
            return;
        }
        self.pending_delete_sequence = None;
        let is_known = self
            .board
            .as_ref()
            .is_some_and(|board| find_task(&board.tasks, &task_id).is_some());
        if is_known {
            self.confirm_and_delete_task(task_id, window, cx);
        }
    }

    /// 포커스된 카드에서 방향대로 옮긴다. 포커스된 카드가 없으면 첫 카드로 들어간다
    fn move_card_focus(
        &mut self,
        focused: Option<&str>,
        direction: FocusMove,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let columns = self.visible_card_ids();
        let target = match focused.and_then(|task_id| position_of(&columns, task_id)) {
            Some((column, row)) => next_focus_position(&columns, column, row, direction),
            None => columns
                .iter()
                .enumerate()
                .find(|(_, cards)| !cards.is_empty())
                .map(|(column, _)| (column, 0)),
        };
        if let Some((column, row)) = target {
            let task_id = columns[column][row].clone();
            self.card_focus_handle(&task_id, cx).focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn open_find_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_bar.is_open = true;
        self.find_bar.has_match = None;
        let focus = self.find_field.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        cx.notify();
    }

    fn close_find_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_bar = FindBarState::default();
        self.find_field
            .update(cx, |field, cx| field.set_text("", cx));
        self.board_focus.focus(window, cx);
        cx.notify();
    }

    /// Enter는 다음, Shift+Enter는 이전 일치로 간다. 일치한 카드로 화면을 옮기고 포커스한다
    fn run_find(&mut self, is_backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.find_field.read(cx).text().to_string();
        if query.trim().is_empty() {
            return;
        }
        let matches = find_text_matches(self, query.trim());
        if matches.is_empty() {
            self.find_bar.has_match = Some(false);
            self.find_bar.current_match = None;
            cx.notify();
            return;
        }
        let count = matches.len();
        let next = match (self.find_bar.current_match, is_backwards) {
            (None, false) => 0,
            (None, true) => count - 1,
            (Some(index), false) => (index + 1) % count,
            (Some(index), true) => (index + count - 1) % count,
        };
        self.find_bar.has_match = Some(true);
        self.find_bar.current_match = Some(next);
        let found = matches[next].clone();
        self.scroll_card_into_view(&found.task_id, window);
        self.find_match = Some(found);
        cx.notify();
    }

    /// 일치한 카드가 화면 밖이면 페이지를 굴려 보이게 한다(TS `window.find`의 스크롤)
    fn scroll_card_into_view(&mut self, task_id: &str, window: &Window) {
        let Some(bounds) = self.card_bounds.borrow().get(task_id).copied() else {
            return;
        };
        let viewport_height = window.viewport_size().height;
        let offset = self.page_scroll.offset();
        let shift = if bounds.bottom() > viewport_height {
            bounds.bottom() - viewport_height
        } else if bounds.top() < px(0.0) {
            bounds.top()
        } else {
            return;
        };
        self.page_scroll
            .set_offset(point(offset.x, offset.y - shift));
    }

    /// 찾기 바가 고른 일치 구간
    pub fn find_match(&self) -> Option<FindTextMatch> {
        self.find_match.clone()
    }

    pub(super) fn handle_find_field_event(
        &mut self,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Changed(_) => {
                self.find_bar.has_match = None;
                self.find_bar.current_match = None;
                self.find_match = None;
                cx.notify();
            }
            TextFieldEvent::Submitted { is_shift_pressed } => {
                self.run_find(*is_shift_pressed, window, cx)
            }
            TextFieldEvent::Escaped => self.close_find_bar(window, cx),
            TextFieldEvent::NavigationKey(_) => {}
        }
    }

    /// `fixed right-6 top-6 w-full max-w-md rounded-xl border bg-bg-surface p-3 shadow-xl`
    pub(super) fn render_find_bar(
        &mut self,
        theme: &ThemeTokens,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.find_bar.is_open {
            return None;
        }
        let labels = Translator::new(self.locale(), "board.pageFind");
        let close_label = Translator::new(self.locale(), "common").translate("close");
        let button = |id: &'static str, label: String| {
            let hover_border = theme.brand.primary;
            div()
                .id(id)
                .relative()
                .flex_none()
                .rounded(radius::MD)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.page)
                .px(px(10.0))
                .py(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.secondary)
                .cursor_pointer()
                .hover(move |style| style.border_color(hover_border))
                .child(label.clone())
                .child(qa_button_layer(&label, &label))
        };
        let no_match = (self.find_bar.has_match == Some(false)).then(|| {
            div()
                .mt(spacing(2.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(labels.translate("noMatch"))
        });
        let bar = div()
            .absolute()
            .top(spacing(6.0))
            .right(spacing(6.0))
            .w(px(448.0))
            .rounded(radius::XL)
            .border_1()
            .border_color(theme.border.default)
            .bg(theme.background.surface)
            .p(spacing(3.0))
            .shadow(box_shadows(&theme.shadow.lg))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing(2.0))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .h(px(38.0))
                            .rounded(radius::MD)
                            .border_1()
                            .border_color(theme.brand.primary)
                            .bg(theme.background.page)
                            .px(spacing(3.0))
                            .text_scale(text_scale::SM)
                            .flex()
                            .items_center()
                            .child(self.find_field.clone()),
                    )
                    .child(
                        button("find-previous", labels.translate("previous")).on_click(
                            cx.listener(|this, _, window, cx| this.run_find(true, window, cx)),
                        ),
                    )
                    .child(button("find-next", labels.translate("next")).on_click(
                        cx.listener(|this, _, window, cx| this.run_find(false, window, cx)),
                    ))
                    .child(button("find-close", close_label).on_click(
                        cx.listener(|this, _, window, cx| this.close_find_bar(window, cx)),
                    )),
            )
            .children(no_match);
        Some(bar.into_any_element())
    }
}

fn position_of(columns: &[Vec<String>], task_id: &str) -> Option<(usize, usize)> {
    columns.iter().enumerate().find_map(|(column, cards)| {
        cards
            .iter()
            .position(|id| id == task_id)
            .map(|row| (column, row))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns() -> Vec<Vec<String>> {
        let ids = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        vec![
            ids(&["t1", "t2"]),
            ids(&[]),
            ids(&["p1"]),
            ids(&["r1", "r2", "r3"]),
            ids(&[]),
        ]
    }

    // parity: TS-components-TaskCard-f12813
    #[test]
    fn arrow_up_and_down_move_within_the_column() {
        let columns = columns();
        assert_eq!(
            next_focus_position(&columns, 0, 0, FocusMove::Down),
            Some((0, 1))
        );
        assert_eq!(
            next_focus_position(&columns, 0, 1, FocusMove::Up),
            Some((0, 0))
        );
        assert_eq!(
            next_focus_position(&columns, 0, 1, FocusMove::Down),
            Some((0, 1))
        );
    }

    // parity: TS-components-TaskCard-a830d8
    #[test]
    fn vim_j_and_k_move_vertically() {
        assert_eq!(focus_move_for_key("j", false, true), Some(FocusMove::Down));
        assert_eq!(focus_move_for_key("k", false, true), Some(FocusMove::Up));
    }

    // parity: TS-components-TaskCard-40926d
    #[test]
    fn vim_keys_are_ignored_without_vim_mode_but_arrows_still_move() {
        assert_eq!(focus_move_for_key("j", false, false), None);
        assert_eq!(
            focus_move_for_key("down", false, false),
            Some(FocusMove::Down)
        );
    }

    // parity: TS-components-TaskCard-b8527a
    #[test]
    fn left_and_right_skip_empty_columns_and_keep_the_row() {
        let columns = columns();
        assert_eq!(
            next_focus_position(&columns, 0, 1, FocusMove::Right),
            Some((2, 0))
        );
        assert_eq!(
            next_focus_position(&columns, 2, 0, FocusMove::Right),
            Some((3, 0))
        );
        assert_eq!(
            next_focus_position(&columns, 3, 2, FocusMove::Left),
            Some((2, 0))
        );
        assert_eq!(next_focus_position(&columns, 3, 0, FocusMove::Right), None);
    }

    // parity: TS-components-TaskCard-0be9d5
    #[test]
    fn vim_h_and_l_move_horizontally() {
        assert_eq!(focus_move_for_key("h", false, true), Some(FocusMove::Left));
        assert_eq!(focus_move_for_key("l", false, true), Some(FocusMove::Right));
        assert_eq!(focus_move_for_key("l", true, true), None);
    }
}
