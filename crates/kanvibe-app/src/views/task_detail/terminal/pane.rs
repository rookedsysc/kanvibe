//! 작업 터미널 하나(TS `Terminal.tsx`). 레지스트리 화면을 그리고, 키·IME·붙여넣기·마우스를 PTY 입력으로 보낸다.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::{
    App, AppContext, ClipboardItem, Context, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Render, ScrollEvent, Styled, Task, Window, canvas, div,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::terminal_tabs::HERDR_SERVER_REPLACED_CLOSE_REASON;
use kanvibe_terminal::mouse_selection::TerminalMousePress;
use kanvibe_terminal::osc52_clipboard::{
    Osc52Body, Osc52SequenceScanner, read_osc52_clipboard_text,
};
use kanvibe_terminal::screen::{ScreenInputModes, TerminalScreen};

use super::image_paste::{clipboard_image, image_data_url};
use super::input::{KeyEncodingModes, encode_keystroke, encode_paste};
use super::mouse::{
    GridPoint, GridSelection, build_mouse_button_report, build_wheel_input, is_selection_press,
    word_bounds,
};
use super::paint::{GridLayout, GridPaintState, measure_cell_size, paint_screen};
use super::runtime::{TerminalRuntime, TerminalWindowEvent, ViewTerminalWindow, terminal_runtime};
use super::status::{TerminalConnection, render_connection_overlay};
use crate::shortcuts::TERMINAL_KEY_CONTEXT;

/// TS 터미널이 연결이 끝났을 때 화면에 남기던 기본 문구
const CONNECTION_CLOSED_MESSAGE: &str = "연결이 종료되었습니다.";
/// TS가 이미지 전송 실패 사유가 비었을 때 쓰던 문구
const IMAGE_TRANSFER_FAILED: &str = "이미지 전송 실패";

/// 터미널 뷰가 붙을 대상
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalTarget {
    pub task_id: String,
    pub tab_id: Option<String>,
    pub is_remote: bool,
}

/// 터미널 뷰가 화면에 알리는 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalPaneEvent {
    /// 세션에 붙었다. 멀티플렉서 세션은 이때 처음 생기므로 탭 목록을 다시 읽는다
    Connected,
}

impl gpui_kit::EventEmitter<TerminalPaneEvent> for TerminalPane {}

pub struct TerminalPane {
    target: TerminalTarget,
    runtime: Option<Arc<TerminalRuntime>>,
    window: Option<Arc<ViewTerminalWindow>>,
    connection: TerminalConnection,
    /// 연결이 끝난 뒤 남길 사유(TS는 xterm에 빨간 줄로 썼다)
    closed_reason: Option<String>,
    /// 마지막 그리기의 칸 크기와 자리. 마우스 좌표를 칸으로 바꿀 때 쓴다
    layout: Rc<Cell<Option<GridLayout>>>,
    /// PTY에 마지막으로 알린 크기
    reported_size: Option<(u16, u16)>,
    focus_handle: FocusHandle,
    marked_text: Option<String>,
    selection: Option<GridSelection>,
    is_selecting: bool,
    reporting_button: Option<u8>,
    osc52: Osc52SequenceScanner,
    is_mac_platform: bool,
    has_retried_after_cooldown: bool,
    translator: Translator,
    /// 원격 이미지 경로 쓰기 줄. 앞 이미지의 경로를 먼저 쓴다
    image_write_queue: Option<Task<()>>,
    _event_task: Option<Task<()>>,
    _open_task: Option<Task<()>>,
}

impl TerminalPane {
    pub fn new(target: TerminalTarget, translator: Translator, cx: &mut Context<Self>) -> Self {
        let runtime = terminal_runtime(cx);
        let (window, event_task) = match runtime.as_ref() {
            Some(runtime) => {
                let (window, receiver) = runtime.open_window();
                (Some(window), Some(spawn_event_loop(receiver, cx)))
            }
            None => (None, None),
        };
        let connection = match runtime {
            Some(_) => TerminalConnection::Connecting,
            None => TerminalConnection::Failed(translator.translate("terminalConnectionFailed")),
        };
        Self {
            target,
            runtime,
            window,
            connection,
            closed_reason: None,
            layout: Rc::new(Cell::new(None)),
            reported_size: None,
            focus_handle: cx.focus_handle(),
            marked_text: None,
            selection: None,
            is_selecting: false,
            reporting_button: None,
            osc52: Osc52SequenceScanner::default(),
            is_mac_platform: cfg!(target_os = "macos"),
            has_retried_after_cooldown: false,
            translator,
            image_write_queue: None,
            _event_task: event_task,
            _open_task: None,
        }
    }

    pub fn target(&self) -> &TerminalTarget {
        &self.target
    }

    /// PTY 입력. 브리지가 창·탭 클라이언트를 찾아 보낸다
    pub fn write_input(&self, data: &str) {
        let (Some(runtime), Some(window)) = (self.runtime.as_ref(), self.window.as_ref()) else {
            return;
        };
        if self.connection != TerminalConnection::Ready {
            return;
        }
        runtime.bridge.write_terminal(
            window.id(),
            &self.target.task_id,
            self.target.tab_id.as_deref(),
            data,
        );
    }

    /// 레지스트리 화면을 읽는다. 화면이 없으면(연결 전·종료 후) `None`
    pub fn read_screen<R>(&self, read: impl FnOnce(&TerminalScreen) -> R) -> Option<R> {
        self.runtime.as_ref()?.registry.read_terminal_screen(
            &self.target.task_id,
            self.target.tab_id.as_deref(),
            read,
        )
    }

    fn input_modes(&self) -> ScreenInputModes {
        self.read_screen(TerminalScreen::input_modes)
            .unwrap_or_default()
    }

    /// 처음 칸 수를 알게 되면 연결하고, 그 뒤에는 크기만 알린다
    fn apply_grid_size(&mut self, columns: u16, rows: u16, cx: &mut Context<Self>) {
        if self.reported_size == Some((columns, rows)) {
            return;
        }
        let is_first_size = self.reported_size.is_none();
        self.reported_size = Some((columns, rows));
        if is_first_size {
            self.open(cx);
            return;
        }
        if let (Some(runtime), Some(window)) = (self.runtime.as_ref(), self.window.as_ref()) {
            runtime.bridge.resize_terminal(
                window.id(),
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                columns,
                rows,
            );
        }
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        let (Some(runtime), Some(window), Some((columns, rows))) = (
            self.runtime.clone(),
            self.window.clone(),
            self.reported_size,
        ) else {
            return;
        };
        let target = self.target.clone();
        let open = cx.background_spawn(async move {
            runtime.bridge.open_terminal(
                window,
                &target.task_id,
                target.tab_id.as_deref(),
                columns,
                rows,
            )
        });
        self._open_task = Some(cx.spawn(async move |this, cx| {
            let result = open.await;
            let _ = this.update(cx, |pane, cx| pane.finish_open(result, cx));
        }));
    }

    fn finish_open(
        &mut self,
        result: kanvibe_terminal::terminal_bridge::TerminalOpenResult,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                self.connection = TerminalConnection::Ready;
                self.closed_reason = None;
                cx.emit(TerminalPaneEvent::Connected);
            }
            Err(error) => match error.retry_after_ms {
                Some(wait) if !self.has_retried_after_cooldown => {
                    self.schedule_cooldown_retry(Duration::from_millis(u64::from(wait.get())), cx);
                }
                _ => {
                    let message = match error.message.is_empty() {
                        true => self.translator.translate("terminalConnectionFailed"),
                        false => error.message,
                    };
                    self.connection = TerminalConnection::Failed(message);
                }
            },
        }
        cx.notify();
    }

    /// 전송 쿨다운 힌트가 오면 한 번만 기다렸다가 다시 연다
    fn schedule_cooldown_retry(&mut self, wait: Duration, cx: &mut Context<Self>) {
        self.has_retried_after_cooldown = true;
        self.connection = TerminalConnection::Reconnecting;
        self._open_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |pane, cx| pane.open(cx));
        }));
    }

    /// 다시 연결 버튼과 Herdr 서버 교체 뒤 재연결
    pub fn reconnect(&mut self, cx: &mut Context<Self>) {
        if let (Some(runtime), Some(window)) = (self.runtime.as_ref(), self.window.as_ref()) {
            runtime.bridge.close_terminal(
                window.id(),
                &self.target.task_id,
                self.target.tab_id.as_deref(),
            );
        }
        self.connection = TerminalConnection::Connecting;
        self.closed_reason = None;
        self.has_retried_after_cooldown = false;
        self.open(cx);
        cx.notify();
    }

    fn handle_window_event(&mut self, event: TerminalWindowEvent, cx: &mut Context<Self>) {
        match event {
            TerminalWindowEvent::Output(output) => self.forward_osc52(output.as_bytes(), cx),
            TerminalWindowEvent::Closed(reason)
                if reason.as_deref() == Some(HERDR_SERVER_REPLACED_CLOSE_REASON) =>
            {
                self.reconnect(cx);
            }
            TerminalWindowEvent::Closed(reason) => {
                if self.connection == TerminalConnection::Ready {
                    self.closed_reason =
                        Some(reason.unwrap_or_else(|| CONNECTION_CLOSED_MESSAGE.to_string()));
                }
            }
        }
        cx.notify();
    }

    /// 멀티플렉서 너머 프로세스의 OSC 52 복사 요청을 시스템 클립보드로 옮긴다. 읽기 요청에는 답하지 않는다
    fn forward_osc52(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        for body in self.osc52.scan(bytes) {
            let Osc52Body::Text(payload) = body else {
                continue;
            };
            if let Some(text) = read_osc52_clipboard_text(&payload) {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        if self.handle_clipboard_shortcut(keystroke, cx) {
            cx.stop_propagation();
            return;
        }
        let modes = KeyEncodingModes {
            is_application_cursor: self.input_modes().is_application_cursor,
        };
        let Some(sequence) = encode_keystroke(keystroke, modes) else {
            return;
        };
        self.prepare_for_input(cx);
        self.write_input(&sequence);
        cx.stop_propagation();
        let _ = window;
    }

    /// 입력하면 스크롤백 보기를 맨 아래로 돌리고 선택을 지운다(xterm.js)
    fn prepare_for_input(&mut self, cx: &mut Context<Self>) {
        self.selection = None;
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.registry.update_terminal_screen_view(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                TerminalScreen::scroll_display_to_bottom,
            );
        }
        cx.notify();
    }

    /// 복사(`Mod+C`·Linux `Ctrl+Shift+C`)와 붙여넣기(`Mod+V`·Linux `Ctrl+Shift+V`)
    fn handle_clipboard_shortcut(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = &keystroke.modifiers;
        let is_clipboard_chord = match self.is_mac_platform {
            true => modifiers.platform && !modifiers.control && !modifiers.alt,
            false => modifiers.control && modifiers.shift && !modifiers.alt,
        };
        if !is_clipboard_chord {
            return false;
        }
        match keystroke.key.as_str() {
            "c" => self.copy_selection(cx),
            "v" => self.paste_from_clipboard(cx),
            _ => return false,
        }
        true
    }

    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.selected_text() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection.filter(|selection| !selection.is_empty())?;
        let columns = usize::from(self.layout.get()?.columns);
        self.read_screen(|screen| selection_text(screen, selection, columns))
    }

    /// 붙여넣기. 원격 세션에서 클립보드에 이미지가 있으면 scp로 보내고 원격 경로를 쓴다
    pub fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if self.target.is_remote
            && let Some(image) = clipboard_image(&item)
        {
            self.send_image_to_remote(image_data_url(&image), cx);
            return;
        }
        if let Some(text) = item.text() {
            let modes = self.input_modes();
            self.prepare_for_input(cx);
            self.write_input(&encode_paste(&text, modes.is_bracketed_paste));
        }
    }

    /// 전송은 바로 시작하되, 경로 쓰기는 붙여넣은 순서대로 한 줄로 세운다
    fn send_image_to_remote(&mut self, image_data_url: String, cx: &mut Context<Self>) {
        let Some(runtime) = self.runtime.clone() else {
            return;
        };
        let task_id = self.target.task_id.clone();
        let transfer = cx.background_spawn(async move {
            runtime
                .bridge
                .paste_image_to_remote_terminal(&task_id, &image_data_url)
        });
        let previous_write = self.image_write_queue.take();
        self.image_write_queue = Some(cx.spawn(async move |this, cx| {
            if let Some(previous_write) = previous_write {
                previous_write.await;
            }
            let result: Result<String, String> = transfer.await;
            let _ = this.update(cx, |pane, cx| match result {
                Ok(remote_path) => pane.write_input(&remote_path),
                Err(error) => {
                    pane.closed_reason = Some(match error.is_empty() {
                        true => IMAGE_TRANSFER_FAILED.to_string(),
                        false => error,
                    });
                    cx.notify();
                }
            });
        }));
    }
}

fn spawn_event_loop(
    mut receiver: UnboundedReceiver<TerminalWindowEvent>,
    cx: &mut Context<TerminalPane>,
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        while let Some(event) = receiver.next().await {
            let mut events = vec![event];
            while let Ok(next) = receiver.try_recv() {
                events.push(next);
            }
            let updated = this.update(cx, |pane, cx| {
                for event in events {
                    pane.handle_window_event(event, cx);
                }
            });
            if updated.is_err() {
                return;
            }
        }
    })
}

/// 선택 영역의 글. 줄 끝 공백은 떼고 줄마다 줄바꿈으로 잇는다
fn selection_text(screen: &TerminalScreen, selection: GridSelection, columns: usize) -> String {
    let (start, end) = selection.ordered();
    let lines: Vec<String> = (start.row..=end.row)
        .map(|row| {
            let cells = screen.visible_cells(row).unwrap_or_default();
            let from = if row == start.row { start.column } else { 0 };
            let to = if row == end.row {
                end.column
            } else {
                columns.saturating_sub(1)
            };
            let text: String = cells
                .iter()
                .enumerate()
                .filter(|(column, cell)| *column >= from && *column <= to && cell.width > 0)
                .map(|(_, cell)| {
                    if cell.text.is_empty() {
                        " "
                    } else {
                        cell.text.as_str()
                    }
                })
                .collect();
            text.trim_end().to_string()
        })
        .collect();
    lines.join("\n")
}

impl Drop for TerminalPane {
    fn drop(&mut self) {
        if let (Some(runtime), Some(window)) = (self.runtime.as_ref(), self.window.as_ref()) {
            window.mark_destroyed();
            runtime.bridge.close_terminal(
                window.id(),
                &self.target.task_id,
                self.target.tab_id.as_deref(),
            );
        }
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TerminalPane {
    fn mouse_press(&self, event: &MouseDownEvent, point: GridPoint) -> TerminalMousePress {
        TerminalMousePress {
            button: match event.button {
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => 0,
            },
            is_shift_pressed: event.modifiers.shift,
            is_alt_pressed: event.modifiers.alt,
            is_control_pressed: event.modifiers.control,
            is_meta_pressed: event.modifiers.platform,
            column: point.column as f32,
            row: point.row as f32,
            click_count: event.click_count.min(u8::MAX as usize) as u8,
        }
    }

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        let Some(layout) = self.layout.get() else {
            return;
        };
        let point = layout.grid_point(event.position);
        let press = self.mouse_press(event, point);
        let modes = self.input_modes();
        if is_selection_press(&press, &modes, self.is_mac_platform) {
            self.start_selection(point, event.click_count, cx);
            return;
        }
        if let Some(report) = build_mouse_button_report(press.button, true, point, &modes) {
            self.reporting_button = Some(press.button);
            self.write_input(&report);
        }
    }

    fn start_selection(&mut self, point: GridPoint, click_count: usize, cx: &mut Context<Self>) {
        self.selection = match click_count {
            2 => self.word_selection(point),
            3 => Some(GridSelection {
                anchor: GridPoint {
                    row: point.row,
                    column: 0,
                },
                head: GridPoint {
                    row: point.row,
                    column: self
                        .layout
                        .get()
                        .map_or(0, |layout| usize::from(layout.columns) - 1),
                },
            }),
            _ => Some(GridSelection {
                anchor: point,
                head: point,
            }),
        };
        self.is_selecting = click_count < 2;
        cx.notify();
    }

    fn word_selection(&self, point: GridPoint) -> Option<GridSelection> {
        let characters: Vec<String> = self.read_screen(|screen| {
            screen
                .visible_cells(point.row)
                .unwrap_or_default()
                .into_iter()
                .map(|cell| cell.text)
                .collect()
        })?;
        let (start, end) = word_bounds(&characters, point.column);
        Some(GridSelection {
            anchor: GridPoint {
                row: point.row,
                column: start,
            },
            head: GridPoint {
                row: point.row,
                column: end,
            },
        })
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.is_selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some(layout) = self.layout.get() else {
            return;
        };
        if let Some(selection) = self.selection.as_mut() {
            selection.head = layout.grid_point(event.position);
            cx.notify();
        }
    }

    fn handle_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        self.is_selecting = false;
        let Some(button) = self.reporting_button.take() else {
            return;
        };
        let (Some(layout), modes) = (self.layout.get(), self.input_modes()) else {
            return;
        };
        if let Some(report) =
            build_mouse_button_report(button, false, layout.grid_point(event.position), &modes)
        {
            self.write_input(&report);
        }
        cx.notify();
    }

    fn handle_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.get() else {
            return;
        };
        let delta = event.delta.pixel_delta(layout.cell_size.height);
        let lines = (f32::from(delta.y) / f32::from(layout.cell_size.height)).round() as i32;
        if lines == 0 {
            return;
        }
        let point = layout.grid_point(event.position);
        let modes = self.input_modes();
        match build_wheel_input(lines > 0, point, &modes) {
            Some(sequence) => self.write_input(&sequence.repeat(lines.unsigned_abs() as usize)),
            None => self.scroll_history(lines),
        }
        cx.notify();
    }

    fn scroll_history(&self, lines: i32) {
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.registry.update_terminal_screen_view(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                |screen| screen.scroll_display(lines),
            );
        }
    }
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let layout_slot = Rc::clone(&self.layout);
        let paint_inputs = PaintInputs {
            runtime: self.runtime.clone(),
            target: self.target.clone(),
            selection: self.selection,
            is_focused: self.focus_handle.is_focused(window),
            marked_text: self.marked_text.clone(),
            focus_handle: self.focus_handle.clone(),
            entity: cx.entity(),
        };
        let grid = canvas(
            move |bounds, window, cx| prepaint_grid(bounds, &layout_slot, &entity, window, cx),
            move |bounds, layout, window, cx| paint_inputs.paint(bounds, layout, window, cx),
        )
        .size_full();
        div()
            .id("task-terminal")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(gpui_kit::rgb(kanvibe_terminal::screen::TERMINAL_BACKGROUND))
            .key_context(TERMINAL_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|pane, event, window, cx| pane.handle_mouse_down(event, window, cx)),
            )
            .on_mouse_move(cx.listener(|pane, event, _, cx| pane.handle_mouse_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pane, event, _, cx| pane.handle_mouse_up(event, cx)),
            )
            .on_scroll(cx.listener(|pane, event, _, cx| pane.handle_scroll(event, cx)))
            .child(grid)
            .children(self.render_qa_row_targets(cx))
            .children(self.render_pane_chat_toggle(cx))
            .children(
                self.closed_reason
                    .clone()
                    .map(super::status::render_closed_reason),
            )
            .children(render_connection_overlay(
                &self.connection,
                &self.translator,
                cx,
            ))
    }
}

impl TerminalPane {
    /// 현재 pane 위 오른쪽 위 `터미널/채팅 전환` 버튼(TS `pane-chat-toggle`: `absolute right-2 top-1 h-9 rounded-md bg-bg-surface px-2 text-xs`).
    /// 화면 칸 영역의 오른쪽 끝에 붙는다
    fn render_pane_chat_toggle(&self, cx: &App) -> Option<gpui_kit::AnyElement> {
        if self.connection != TerminalConnection::Ready {
            return None;
        }
        let layout = self.layout.get()?;
        let theme = *cx.global::<crate::theme::ThemeTokens>();
        let label = self.translator.translate("paneChat.toggle");
        let grid_right = layout.cell_size.width * f32::from(layout.columns);
        Some(
            div()
                .absolute()
                .top(gpui_kit::px(0.0))
                .left(gpui_kit::px(0.0))
                .w(grid_right)
                .child(
                    div()
                        .id("pane-chat-toggle")
                        .absolute()
                        .right(crate::theme::spacing(2.0))
                        .top(crate::theme::spacing(1.0))
                        .h(crate::theme::spacing(9.0))
                        .flex()
                        .items_center()
                        .gap(crate::theme::spacing(1.0))
                        .rounded(crate::theme::radius::MD)
                        .bg(theme.background.surface)
                        .px(crate::theme::spacing(2.0))
                        .text_size(crate::theme::text_scale::XS.font_size)
                        .line_height(crate::theme::text_scale::XS.line_height)
                        .text_color(theme.text.primary)
                        .child(super::super::icons::detail_icon(
                            super::super::icons::DetailIcon::Chatting,
                            theme.text.primary,
                            gpui_kit::px(16.0),
                        ))
                        .child(label.clone())
                        .child(crate::qa::targets::qa_target_layer(
                            crate::qa::targets::QaTargetDescription {
                                role: "button",
                                name: label.into(),
                                text: String::new().into(),
                            },
                        )),
                )
                .into_any_element(),
        )
    }

    /// QA 재생 드라이버가 터미널 글을 찾을 수 있게 줄마다 보이는 글을 대상으로 남긴다(xterm DOM 렌더러의 줄 요소 자리)
    fn render_qa_row_targets(&self, cx: &App) -> Vec<gpui_kit::AnyElement> {
        if !cx.has_global::<crate::qa::targets::QaTargetRegistry>() {
            return Vec::new();
        }
        let (Some(layout), Some(lines)) = (
            self.layout.get(),
            self.read_screen(TerminalScreen::viewport_lines),
        ) else {
            return Vec::new();
        };
        lines
            .into_iter()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(row, line)| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(layout.cell_size.height * row as f32)
                    .h(layout.cell_size.height)
                    .child(crate::qa::targets::qa_target_layer(
                        crate::qa::targets::QaTargetDescription {
                            role: "row",
                            name: line.clone().into(),
                            text: line.into(),
                        },
                    ))
                    .into_any_element()
            })
            .collect()
    }
}

/// 칸 수를 정하고, 바뀌었으면 PTY 크기를 다음 틱에 알린다
fn prepaint_grid(
    bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
    layout_slot: &Rc<Cell<Option<GridLayout>>>,
    entity: &gpui_kit::Entity<TerminalPane>,
    window: &mut Window,
    cx: &mut App,
) -> GridLayout {
    let cell_size = measure_cell_size(window);
    let columns = (f32::from(bounds.size.width) / f32::from(cell_size.width)).floor();
    let rows = (f32::from(bounds.size.height) / f32::from(cell_size.height)).floor();
    let layout = GridLayout {
        origin: bounds.origin,
        cell_size,
        columns: columns.clamp(2.0, u16::MAX as f32) as u16,
        rows: rows.clamp(1.0, u16::MAX as f32) as u16,
    };
    let previous = layout_slot.replace(Some(layout));
    let is_resized = previous
        .is_none_or(|previous| (previous.columns, previous.rows) != (layout.columns, layout.rows));
    if is_resized {
        let entity = entity.clone();
        window.defer(cx, move |_, cx| {
            entity.update(cx, |pane, cx| {
                pane.apply_grid_size(layout.columns, layout.rows, cx)
            });
        });
    }
    layout
}

/// 그리기에 필요한 값. 렌더 시점에 복사해 두어 그리는 동안 뷰를 빌리지 않는다
struct PaintInputs {
    runtime: Option<Arc<TerminalRuntime>>,
    target: TerminalTarget,
    selection: Option<GridSelection>,
    is_focused: bool,
    marked_text: Option<String>,
    focus_handle: FocusHandle,
    entity: gpui_kit::Entity<TerminalPane>,
}

impl PaintInputs {
    fn paint(
        self,
        bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
        layout: GridLayout,
        window: &mut Window,
        cx: &mut App,
    ) {
        let state = GridPaintState {
            layout,
            selection: self.selection,
            is_focused: self.is_focused,
            marked_text: self.marked_text.as_deref(),
        };
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.registry.read_terminal_screen(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                |screen| paint_screen(screen, &state, window, cx),
            );
        }
        // 글자 입력과 IME 조합은 이 입력기로 받는다. 글자가 아닌 키는 `handle_key_down`이 먼저 처리한다
        window.handle_input(
            &self.focus_handle,
            gpui_kit::ElementInputHandler::new(bounds, self.entity),
            cx,
        );
    }
}

impl gpui_kit::EntityInputHandler for TerminalPane {
    fn text_for_range(
        &mut self,
        _range: std::ops::Range<usize>,
        _adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui_kit::UTF16Selection> {
        let end = self
            .marked_text
            .as_deref()
            .map_or(0, |text| text.encode_utf16().count());
        Some(gpui_kit::UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.marked_text
            .as_deref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    /// 확정된 글자(일반 타이핑과 IME 확정)를 PTY로 보낸다
    fn replace_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        if text.is_empty() {
            cx.notify();
            return;
        }
        self.prepare_for_input(cx);
        self.write_input(text);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    /// IME 후보 창을 커서 칸 옆에 띄운다
    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        _element_bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
        let layout = self.layout.get()?;
        let cursor = self.read_screen(TerminalScreen::cursor)?;
        let origin = gpui_kit::point(
            layout.origin.x + layout.cell_size.width * cursor.column as f32,
            layout.origin.y + layout.cell_size.height * cursor.row as f32,
        );
        Some(gpui_kit::Bounds::new(origin, layout.cell_size))
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui_kit::Point<gpui_kit::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
