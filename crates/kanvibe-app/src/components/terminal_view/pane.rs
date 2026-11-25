//! 작업 터미널 하나(TS `Terminal.tsx`). 레지스트리 화면을 그리고, 키·IME·붙여넣기·마우스를 PTY 입력으로 보낸다.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui_kit::{
    App, ClipboardItem, Context, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render,
    ScrollEvent, Styled, Task, Window, canvas, div,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::terminal_tabs::HERDR_SERVER_REPLACED_CLOSE_REASON;
use kanvibe_runtime::DesktopRuntime;
use kanvibe_terminal::mouse_selection::TerminalMousePress;
use kanvibe_terminal::osc52_clipboard::{
    Osc52Body, Osc52SequenceScanner, read_osc52_clipboard_text,
};
use kanvibe_terminal::screen::{ScreenInputModes, TerminalScreen};

use super::image_paste::ImagePasteToast;
use super::input::{KeyEncodingModes, encode_keystroke, encode_paste};
use super::mouse::{
    GridPoint, GridSelection, build_mouse_button_report, build_wheel_input, is_selection_press,
    word_bounds,
};
use super::paint::{GridLayout, GridPaintState, measure_cell_size, paint_screen};
use super::status::{TerminalConnection, render_connection_overlay};
use super::transport::{BridgeTransport, TerminalTransport};
use super::window::{TerminalWindowEvent, ViewTerminalWindow, open_terminal_window};
use crate::shortcuts::TERMINAL_KEY_CONTEXT;

/// xterm FitAddon이 칸 수를 셀 때 폭에서 빼는 스크롤 막대 자리(`ViewportConstants.DEFAULT_SCROLL_BAR_WIDTH`)
const XTERM_SCROLL_BAR_WIDTH: f32 = 14.0;
/// TS 터미널이 연결이 끝났을 때 화면에 남기던 기본 문구
const CONNECTION_CLOSED_MESSAGE: &str = "연결이 종료되었습니다.";

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
    pub(super) target: TerminalTarget,
    /// 레지스트리 화면을 읽을 런타임. 입출력은 `transport`로 한다
    runtime: Option<Arc<DesktopRuntime>>,
    pub(super) transport: Option<Arc<dyn TerminalTransport>>,
    pub(super) window: Option<Arc<ViewTerminalWindow>>,
    pub(super) connection: TerminalConnection,
    /// 연결이 끝난 뒤 남길 사유(TS는 xterm에 빨간 줄로 썼다)
    pub(super) closed_reason: Option<String>,
    /// 마지막 그리기의 칸 크기와 자리. 마우스 좌표를 칸으로 바꿀 때 쓴다
    layout: Rc<Cell<Option<GridLayout>>>,
    /// PTY에 마지막으로 알린 크기
    pub(super) reported_size: Option<(u16, u16)>,
    focus_handle: FocusHandle,
    marked_text: Option<String>,
    selection: Option<GridSelection>,
    is_selecting: bool,
    reporting_button: Option<u8>,
    osc52: Osc52SequenceScanner,
    pub(super) is_mac_platform: bool,
    pub(super) has_retried_after_cooldown: bool,
    /// 비활성 탭이거나 메인 영역이 채팅 기록이라 가려져 있다
    pub(super) is_hidden: bool,
    pub(super) translator: Translator,
    /// 원격 이미지 전송 진행·실패 알림
    pub(super) image_toasts: Vec<ImagePasteToast>,
    pub(super) next_toast_id: u64,
    /// 원격 이미지 경로 쓰기 줄. 앞 이미지의 경로를 먼저 쓴다
    pub(super) image_write_queue: Option<Task<()>>,
    _event_task: Option<Task<()>>,
    pub(super) _open_task: Option<Task<()>>,
    /// 쿨다운 뒤 다시 열기 예약. 가려지거나 다시 연결하면 버린다
    pub(super) _cooldown_wait: Option<Task<()>>,
    _window_activation: Option<gpui_kit::Subscription>,
}

impl TerminalPane {
    /// 런타임 브리지로 붙는 터미널. 데이터 디렉터리 없이 뜬 실행이면 연결 실패를 보인다
    pub fn new(target: TerminalTarget, translator: Translator, cx: &mut Context<Self>) -> Self {
        let transport = cx
            .global::<crate::services::AppServices>()
            .runtime()
            .map(|runtime| {
                Arc::new(BridgeTransport(Arc::clone(runtime))) as Arc<dyn TerminalTransport>
            });
        Self::with_transport(target, translator, transport, cx)
    }

    /// 입출력 길을 정해 만든다. 화면은 늘 런타임 레지스트리에서 읽는다
    pub fn with_transport(
        target: TerminalTarget,
        translator: Translator,
        transport: Option<Arc<dyn TerminalTransport>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let runtime = cx
            .global::<crate::services::AppServices>()
            .runtime()
            .map(Arc::clone);
        let (window, event_task) = match transport.as_ref() {
            Some(_) => {
                let (window, receiver) = open_terminal_window();
                (Some(window), Some(spawn_event_loop(receiver, cx)))
            }
            None => (None, None),
        };
        let connection = match transport {
            Some(_) => TerminalConnection::Connecting,
            None => TerminalConnection::Failed(translator.translate("terminalConnectionFailed")),
        };
        Self {
            target,
            runtime,
            transport,
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
            is_hidden: false,
            translator,
            image_toasts: Vec::new(),
            next_toast_id: 0,
            image_write_queue: None,
            _event_task: event_task,
            _open_task: None,
            _cooldown_wait: None,
            _window_activation: None,
        }
    }

    pub fn target(&self) -> &TerminalTarget {
        &self.target
    }

    /// PTY 입력. 브리지가 창·탭 클라이언트를 찾아 보낸다
    pub fn write_input(&self, data: &str) {
        let (Some(transport), Some(window)) = (self.transport.as_ref(), self.window.as_ref())
        else {
            return;
        };
        if self.connection != TerminalConnection::Ready {
            return;
        }
        transport.write(window.id(), &self.target, data);
    }

    /// 레지스트리 화면을 읽는다. 화면이 없으면(연결 전·종료 후) `None`
    pub fn read_screen<R>(&self, read: impl FnOnce(&TerminalScreen) -> R) -> Option<R> {
        self.runtime
            .as_ref()?
            .terminal_registry()
            .read_terminal_screen(&self.target.task_id, self.target.tab_id.as_deref(), read)
    }

    fn input_modes(&self) -> ScreenInputModes {
        self.read_screen(TerminalScreen::input_modes)
            .unwrap_or_default()
    }

    /// 창이 다시 앞으로 오면 크기를 다시 알린다(TS `focus` 이벤트의 `scheduleTerminalSync`)
    fn observe_window_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self._window_activation.is_some() {
            return;
        }
        self._window_activation = Some(cx.observe_window_activation(window, |pane, window, _| {
            if window.is_window_active() && !pane.is_hidden {
                pane.reassert_size();
            }
        }));
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
            runtime.terminal_registry().update_terminal_screen_view(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                TerminalScreen::scroll_display_to_bottom,
            );
        }
        cx.notify();
    }

    /// 복사(`Mod+C`·Linux `Ctrl+Shift+C`)와 붙여넣기(`Mod+V`·Linux `Ctrl+Shift+V`·`Shift+Insert`).
    /// 원격 세션은 `Ctrl+V`·`Ctrl+Shift+V`·`Shift+Insert`에 클립보드 이미지가 있으면 이미지를 보낸다
    fn handle_clipboard_shortcut(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if is_image_paste_chord(keystroke) && self.try_paste_remote_image(cx) {
            return true;
        }
        let modifiers = &keystroke.modifiers;
        let is_clipboard_chord = match self.is_mac_platform {
            true => modifiers.platform && !modifiers.control && !modifiers.alt,
            false => modifiers.control && modifiers.shift && !modifiers.alt,
        };
        let is_shift_insert = !self.is_mac_platform && is_shift_insert(keystroke);
        match keystroke.key.as_str() {
            "c" if is_clipboard_chord => self.copy_selection(cx),
            "v" if is_clipboard_chord => self.paste_from_clipboard(cx),
            "insert" if is_shift_insert => self.paste_from_clipboard(cx),
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
        if self.try_paste_remote_image(cx) {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let modes = self.input_modes();
        self.prepare_for_input(cx);
        self.write_input(&encode_paste(&text, modes.is_bracketed_paste));
    }
}

/// TS `attachCustomKeyEventHandler`가 이미지 붙여넣기로 본 조합: `Ctrl+V`, `Ctrl+Shift+V`, `Shift+Insert`
fn is_image_paste_chord(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    let is_ctrl_v = modifiers.control
        && !modifiers.platform
        && !modifiers.alt
        && keystroke.key.eq_ignore_ascii_case("v");
    is_ctrl_v || is_shift_insert(keystroke)
}

fn is_shift_insert(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.shift
        && !modifiers.control
        && !modifiers.platform
        && !modifiers.alt
        && keystroke.key == "insert"
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
        if let (Some(transport), Some(window)) = (self.transport.as_ref(), self.window.as_ref()) {
            window.mark_destroyed();
            transport.close(window.id(), &self.target);
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

    /// X11 중간 클릭 붙여넣기 관례. 원격 세션에서 클립보드 이미지가 있을 때만 가로챈다
    fn handle_middle_mouse_down(&mut self, cx: &mut Context<Self>) {
        if self.try_paste_remote_image(cx) {
            cx.stop_propagation();
        }
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
            runtime.terminal_registry().update_terminal_screen_view(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                |screen| screen.scroll_display(lines),
            );
        }
    }
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.observe_window_activation(window, cx);
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
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|pane, _, _, cx| pane.handle_middle_mouse_down(cx)),
            )
            .on_mouse_move(cx.listener(|pane, event, _, cx| pane.handle_mouse_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pane, event, _, cx| pane.handle_mouse_up(event, cx)),
            )
            .on_scroll(cx.listener(|pane, event, _, cx| pane.handle_scroll(event, cx)))
            .child(grid)
            .children(self.render_qa_row_targets(cx))
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
            .children(self.render_image_toasts(window, cx))
    }
}

impl TerminalPane {
    /// 화면 칸 영역의 폭. 연결되어 한 번 그려진 뒤에만 있다. 화면이 칸 영역 오른쪽 끝에 붙는 버튼을 놓을 때 쓴다
    pub fn grid_width(&self) -> Option<gpui_kit::Pixels> {
        if self.connection != TerminalConnection::Ready {
            return None;
        }
        let layout = self.layout.get()?;
        Some(layout.cell_size.width * f32::from(layout.columns))
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
    // xterm FitAddon처럼 CSS 폭·높이를 정수로 자르고, 폭에서는 스크롤 막대 자리를 먼저 뺀다
    let available_width = f32::from(bounds.size.width).trunc() - XTERM_SCROLL_BAR_WIDTH;
    let columns = (available_width / f32::from(cell_size.width)).floor();
    let rows = (f32::from(bounds.size.height).trunc() / f32::from(cell_size.height)).floor();
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
    runtime: Option<Arc<DesktopRuntime>>,
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
            runtime.terminal_registry().read_terminal_screen(
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
