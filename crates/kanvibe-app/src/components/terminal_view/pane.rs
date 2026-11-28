//! 작업 터미널 하나(TS `Terminal.tsx`). 레지스트리 화면을 그리고, 키·IME·붙여넣기·마우스를 PTY 입력으로 보낸다.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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
use kanvibe_terminal::screen::{ScreenInputModes, TerminalScreen, VisibleSelection};

use super::image_paste::ImagePasteToast;
use super::input::{KeyEncodingModes, encode_keystroke, encode_paste, page_scroll_lines};
use super::mouse::{
    GridPoint, GridSelection, MouseReportModifiers, build_mouse_button_report,
    build_mouse_motion_report, build_wheel_input, is_selection_press, take_whole_wheel_lines,
    word_bounds,
};
use super::paint::{GridLayout, GridPaintState, ScreenPaintFrame, measure_cell_size, paint_screen};
use super::scroll_bar::ScrollbackPosition;
use super::status::{TerminalConnection, render_connection_overlay};
use super::transport::{BridgeTransport, TerminalTransport};
use super::window::{TerminalWindowEvent, ViewTerminalWindow, open_terminal_window};
use crate::components::scroll_area::{PAGE_SCROLLBAR_WIDTH, paint_page_scrollbar};
use crate::shortcuts::TERMINAL_KEY_CONTEXT;
use crate::theme::ThemeTokens;

/// xterm FitAddon이 칸 수를 셀 때 폭에서 빼는 스크롤 막대 자리(`ViewportConstants.DEFAULT_SCROLL_BAR_WIDTH`)
const XTERM_SCROLL_BAR_WIDTH: f32 = 14.0;
/// TS 터미널이 연결이 끝났을 때 화면에 남기던 기본 문구
const CONNECTION_CLOSED_MESSAGE: &str = "연결이 종료되었습니다.";
/// TS 터미널이 쓴 xterm.js DOM 렌더러의 커서 깜박임. 1초 `step-end` 애니메이션이 절반마다 켜고 끈다
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);

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
    /// 세션이 닫혔다. 셸이 끝난 terminal 세션 탭은 레지스트리가 지우므로 탭 목록을 다시 읽는다
    Closed,
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
    /// 마지막 그리기의 스크롤 막대 홈(오른쪽 끝 띠). 막대를 누른 자리를 가릴 때 쓴다
    scroll_bar_track: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    /// 스크롤 막대 thumb을 끄는 중이면 누른 자리와 thumb 윗변의 거리
    scroll_bar_grab_offset: Option<f32>,
    /// 그리기 전에 화면에서 복사해 두는 셀 버퍼. 프레임마다 다시 써서 칸마다 할당하지 않는다
    paint_frame: Rc<RefCell<ScreenPaintFrame>>,
    /// PTY에 마지막으로 알린 크기
    pub(super) reported_size: Option<(u16, u16)>,
    focus_handle: FocusHandle,
    composition: Option<TerminalComposition>,
    /// 드래그 선택을 시작한 칸. 이 칸을 벗어나기 전까지는 빈 선택이다. 선택 자체는 화면이 버퍼 줄에 묶어 쥔다
    selection_press_point: Option<GridPoint>,
    is_selecting: bool,
    reporting_button: Option<u8>,
    /// 마지막으로 이동을 살핀 칸. 같은 칸 안의 이동은 다시 보고하지 않는다(xterm.js)
    last_motion_point: Option<GridPoint>,
    /// 아직 한 줄이 되지 않은 휠 양(줄 단위). 트랙패드의 작은 이동을 버리지 않으려고 쌓는다
    wheel_remainder_lines: f32,
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
    _composition_blur: Option<gpui_kit::Subscription>,
    /// 끝나지 않은 동기화 출력의 시간 제한 시각과 그때 다시 그리는 예약. 출력이 더 오지 않아도 화면이 풀리게 한다
    sync_expiry_redraw: Option<(Instant, Task<()>)>,
    /// 커서 깜박임 타이머(xterm.js `cursorBlink: true`). 포커스가 있고 창이 앞에 있으며 보이는 동안만 쥔다
    cursor_blink: Option<Task<()>>,
    /// 깜박임에서 지금 커서를 그리는 차례인지. 깜박이지 않을 때는 늘 `true`다
    is_cursor_blink_on: bool,
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
            scroll_bar_track: Rc::default(),
            scroll_bar_grab_offset: None,
            paint_frame: Rc::default(),
            reported_size: None,
            focus_handle: cx.focus_handle(),
            composition: None,
            selection_press_point: None,
            is_selecting: false,
            reporting_button: None,
            last_motion_point: None,
            wheel_remainder_lines: 0.0,
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
            _composition_blur: None,
            sync_expiry_redraw: None,
            cursor_blink: None,
            is_cursor_blink_on: true,
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

    /// 시간 제한이 지난 동기화 출력을 화면에 반영하고, 아직 모으는 중이면 시간 제한에 맞춰 한 번 더 그린다
    fn finish_expired_screen_sync(&mut self, cx: &mut Context<Self>) {
        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };
        let now = Instant::now();
        let pending_deadline = runtime
            .terminal_registry()
            .update_terminal_screen_view(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                |screen| {
                    screen.finish_expired_sync(now);
                    screen.pending_sync_deadline()
                },
            )
            .flatten();
        let Some(deadline) = pending_deadline else {
            self.sync_expiry_redraw = None;
            return;
        };
        let is_already_scheduled = self
            .sync_expiry_redraw
            .as_ref()
            .is_some_and(|(scheduled, _)| *scheduled == deadline);
        if is_already_scheduled {
            return;
        }
        let wait = deadline.saturating_duration_since(now);
        let redraw = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        });
        self.sync_expiry_redraw = Some((deadline, redraw));
    }

    /// 포커스·창 활성·가림이 바뀌면 GPUI가 창을 다시 그리므로, 그릴 때 깜박임을 켜거나 끈다
    fn sync_cursor_blink(&mut self, should_blink: bool, cx: &mut Context<Self>) {
        if !should_blink {
            self.stop_cursor_blink();
        } else if self.cursor_blink.is_none() {
            self.restart_cursor_blink(cx);
        }
    }

    fn restart_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.is_cursor_blink_on = true;
        self.cursor_blink = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CURSOR_BLINK_INTERVAL).await;
                let is_alive = this.update(cx, |pane, cx| {
                    pane.is_cursor_blink_on = !pane.is_cursor_blink_on;
                    cx.notify();
                });
                if is_alive.is_err() {
                    break;
                }
            }
        }));
    }

    pub(super) fn stop_cursor_blink(&mut self) {
        self.cursor_blink = None;
        self.is_cursor_blink_on = true;
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
            TerminalWindowEvent::Output => {
                if let Some(window) = &self.window {
                    window.acknowledge_output();
                }
            }
            TerminalWindowEvent::ClipboardText(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
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
                cx.emit(TerminalPaneEvent::Closed);
            }
        }
        cx.notify();
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
        let page_lines = self
            .layout
            .get()
            .and_then(|layout| page_scroll_lines(keystroke, layout.rows));
        if let Some(lines) = page_lines {
            self.scroll_history(lines);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let modes = KeyEncodingModes {
            is_application_cursor: self.input_modes().is_application_cursor,
        };
        let Some(sequence) = encode_keystroke(keystroke, modes) else {
            return;
        };
        // IME가 넘겨준 키라도 조합이 남아 있으면 PTY로 제어 문자를 보내지 않는다.
        if self.composition.is_some() {
            cx.stop_propagation();
            return;
        }
        self.prepare_for_input(cx);
        self.write_input(&sequence);
        cx.stop_propagation();
        let _ = window;
    }

    /// 입력하면 스크롤백 보기를 맨 아래로 돌리고 선택을 지운다. 깜박이던 커서는 보인 상태로 다시 센다(xterm.js)
    fn prepare_for_input(&mut self, cx: &mut Context<Self>) {
        if self.cursor_blink.is_some() {
            self.restart_cursor_blink(cx);
        }
        self.selection_press_point = None;
        self.update_screen_view(|screen| {
            screen.clear_selection();
            screen.scroll_display_to_bottom();
        });
        cx.notify();
    }

    /// 데스크탑 뷰만 쓰는 화면 상태(선택·스크롤백 보기 위치)를 바꾼다
    fn update_screen_view(&self, update: impl FnOnce(&mut TerminalScreen)) {
        if let Some(runtime) = self.runtime.as_ref() {
            runtime.terminal_registry().update_terminal_screen_view(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                update,
            );
        }
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
        self.read_screen(TerminalScreen::selected_text).flatten()
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

/// 선택 영역의 글. 줄 끝 공백은 떼고 줄마다 줄바꿈으로 잇는다.
/// 자동 줄바꿈으로 다음 줄에 이어진 줄은 공백을 떼지 않고 개행 없이 잇는다(xterm.js `isWrapped`)
pub(crate) fn selection_text(
    screen: &TerminalScreen,
    selection: GridSelection,
    columns: usize,
) -> String {
    let (start, end) = selection.ordered();
    let mut text = String::new();
    for row in start.row..=end.row {
        let from = if row == start.row { start.column } else { 0 };
        let to = if row == end.row {
            end.column
        } else {
            columns.saturating_sub(1)
        };
        let row_text = selected_row_text(screen, row, from, to);
        let is_continued_on_next_row = row < end.row && screen.is_visible_row_wrapped(row);
        if is_continued_on_next_row {
            text.push_str(&row_text);
            continue;
        }
        text.push_str(row_text.trim_end());
        if row < end.row {
            text.push('\n');
        }
    }
    text
}

/// 한 줄에서 `from`..=`to` 칸의 글. 빈 칸은 공백으로 둔다
fn selected_row_text(screen: &TerminalScreen, row: usize, from: usize, to: usize) -> String {
    let cells = screen.visible_cells(row).unwrap_or_default();
    cells
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
        .collect()
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
        if event.button == MouseButton::Left && self.press_scroll_bar(event.position, cx) {
            return;
        }
        let point = layout.grid_point(event.position);
        let press = self.mouse_press(event, point);
        let modes = self.input_modes();
        if is_selection_press(&press, &modes, self.is_mac_platform) {
            if event.button == MouseButton::Left {
                self.start_selection(point, event.click_count, cx);
            }
            return;
        }
        let modifiers = mouse_report_modifiers(&event.modifiers);
        if let Some(report) =
            build_mouse_button_report(press.button, true, point, modifiers, &modes)
        {
            self.reporting_button = Some(press.button);
            self.write_input(&report);
        }
    }

    fn start_selection(&mut self, point: GridPoint, click_count: usize, cx: &mut Context<Self>) {
        let last_column = self
            .layout
            .get()
            .map_or(0, |layout| usize::from(layout.columns) - 1);
        let word = (click_count == 2)
            .then(|| self.word_columns(point))
            .flatten();
        self.update_screen_view(|screen| match (click_count, word) {
            (2, Some((start, end))) => {
                screen.select_visible_range((point.row, start), (point.row, end))
            }
            (2, None) => screen.clear_selection(),
            (3, _) => screen.select_visible_range((point.row, 0), (point.row, last_column)),
            _ => screen.start_selection(point.row, point.column),
        });
        self.selection_press_point = (click_count < 2).then_some(point);
        self.is_selecting = click_count < 2;
        cx.notify();
    }

    /// 두 번 누른 칸이 속한 단어의 첫 칸과 끝 칸
    fn word_columns(&self, point: GridPoint) -> Option<(usize, usize)> {
        let cells =
            self.read_screen(|screen| screen.visible_cells(point.row).unwrap_or_default())?;
        Some(word_bounds(&cells, point.column))
    }

    /// X11 중간 클릭 붙여넣기 관례. 원격 세션에서 클립보드 이미지가 있을 때만 가로채고, 아니면 마우스 보고로 보낸다
    fn handle_middle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.try_paste_remote_image(cx) {
            cx.stop_propagation();
            return;
        }
        self.handle_mouse_down(event, window, cx);
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.get() else {
            return;
        };
        if let Some(grab_offset) = self.scroll_bar_grab_offset
            && event.pressed_button == Some(MouseButton::Left)
        {
            self.drag_scroll_bar_thumb(event.position.y, grab_offset, cx);
            return;
        }
        let point = layout.grid_point(event.position);
        if self.is_selecting && event.pressed_button == Some(MouseButton::Left) {
            if self.selection_press_point != Some(point) {
                self.selection_press_point = None;
                self.update_screen_view(|screen| screen.extend_selection(point.row, point.column));
                cx.notify();
            }
            return;
        }
        self.report_mouse_motion(point, event);
    }

    /// 프로그램이 이동 보고(1002·1003)를 켰으면 칸이 바뀔 때마다 이동을 알린다
    fn report_mouse_motion(&mut self, point: GridPoint, event: &MouseMoveEvent) {
        if self.last_motion_point == Some(point) {
            return;
        }
        self.last_motion_point = Some(point);
        let modes = self.input_modes();
        let modifiers = mouse_report_modifiers(&event.modifiers);
        if let Some(report) =
            build_mouse_motion_report(self.reporting_button, point, modifiers, &modes)
        {
            self.write_input(&report);
        }
    }

    /// 끌지 않고 한 번 누른 칸이 링크면 연다(TS `WebLinksAddon`의 `window.open` → 창 열기 규칙).
    /// 마우스 보고 중이면 선택 누름이 아니어서 누른 칸이 없고, 클릭은 프로그램이 받는다
    fn open_clicked_link(&mut self, event: &MouseUpEvent, window: &Window, cx: &mut Context<Self>) {
        let press_point = self.selection_press_point.take();
        let Some(layout) = self.layout.get() else {
            return;
        };
        let point = layout.grid_point(event.position);
        if event.click_count != 1 || press_point != Some(point) {
            return;
        }
        let link = self.read_screen(|screen| screen.link_at(point.row, point.column));
        if let Some(url) = link.flatten() {
            crate::windows::open_href(&url, Some(window.window_handle().window_id()), cx);
        }
    }

    fn handle_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        self.is_selecting = false;
        self.scroll_bar_grab_offset = None;
        let Some(button) = self.reporting_button.take() else {
            return;
        };
        let (Some(layout), modes) = (self.layout.get(), self.input_modes()) else {
            return;
        };
        let point = layout.grid_point(event.position);
        let modifiers = mouse_report_modifiers(&event.modifiers);
        if let Some(report) = build_mouse_button_report(button, false, point, modifiers, &modes) {
            self.write_input(&report);
        }
        cx.notify();
    }

    fn handle_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.get() else {
            return;
        };
        let delta = event.delta.pixel_delta(layout.cell_size.height);
        let delta_lines = f32::from(delta.y) / f32::from(layout.cell_size.height);
        let lines = take_whole_wheel_lines(&mut self.wheel_remainder_lines, delta_lines);
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
        self.update_screen_view(|screen| screen.scroll_display(lines));
    }

    /// 지금 화면의 스크롤백 위치. 스크롤백이 없거나 그려진 적이 없으면 `None`이다
    fn scrollback_position(&self) -> Option<ScrollbackPosition> {
        let cell_height = f32::from(self.layout.get()?.cell_size.height);
        self.read_screen(|screen| ScrollbackPosition::read(screen, cell_height))
            .filter(|position| position.history_size > 0)
    }

    /// 스크롤 막대 홈을 눌렀으면 처리하고 참을 돌려준다. thumb을 누르면 끌기를 시작하고,
    /// 홈의 빈 곳을 누르면 그쪽으로 한 화면 넘긴다(Chromium 스크롤바)
    fn press_scroll_bar(
        &mut self,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(track) = self
            .scroll_bar_track
            .get()
            .filter(|track| track.contains(&position))
        else {
            return false;
        };
        let Some(scrollback) = self.scrollback_position() else {
            return false;
        };
        let track_height = f32::from(track.size.height);
        let Some((thumb_top, thumb_length)) = scrollback.thumb(track_height) else {
            return false;
        };
        let pressed_y = f32::from(position.y - track.origin.y);
        let page_lines = scrollback.rows as i32;
        if pressed_y < thumb_top {
            self.scroll_history(page_lines);
        } else if pressed_y > thumb_top + thumb_length {
            self.scroll_history(-page_lines);
        } else {
            self.scroll_bar_grab_offset = Some(pressed_y - thumb_top);
        }
        cx.notify();
        true
    }

    fn drag_scroll_bar_thumb(
        &mut self,
        pointer_y: gpui_kit::Pixels,
        grab_offset: f32,
        cx: &mut Context<Self>,
    ) {
        let (Some(track), Some(scrollback)) =
            (self.scroll_bar_track.get(), self.scrollback_position())
        else {
            return;
        };
        let thumb_top = f32::from(pointer_y - track.origin.y) - grab_offset;
        let target_offset =
            scrollback.display_offset_for_thumb_top(f32::from(track.size.height), thumb_top);
        let lines = target_offset as i32 - scrollback.display_offset as i32;
        if lines != 0 {
            self.scroll_history(lines);
            cx.notify();
        }
    }
}

/// 칸 영역 오른쪽 끝의 스크롤 막대 홈. xterm이 칸 수를 셀 때 비워 둔 자리에 놓는다
fn scroll_bar_track_bounds(
    bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    gpui_kit::Bounds::new(
        gpui_kit::point(bounds.right() - PAGE_SCROLLBAR_WIDTH, bounds.origin.y),
        gpui_kit::size(PAGE_SCROLLBAR_WIDTH, bounds.size.height),
    )
}

/// 화면이 보이는 칸으로 옮긴 선택을 그리기용 선택으로 바꾼다
fn grid_selection(selection: VisibleSelection) -> GridSelection {
    GridSelection {
        anchor: GridPoint {
            row: selection.start_row,
            column: selection.start_column,
        },
        head: GridPoint {
            row: selection.end_row,
            column: selection.end_column,
        },
    }
}

/// GPUI 수정자를 마우스 보고 수정자로 옮긴다. macOS Command는 보고에 넣지 않는다(xterm.js)
fn mouse_report_modifiers(modifiers: &gpui_kit::Modifiers) -> MouseReportModifiers {
    MouseReportModifiers {
        is_shift_pressed: modifiers.shift,
        is_alt_pressed: modifiers.alt,
        is_control_pressed: modifiers.control,
    }
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.observe_window_activation(window, cx);
        if self._composition_blur.is_none() {
            self._composition_blur =
                Some(cx.on_blur(&self.focus_handle, window, |pane, window, cx| {
                    if !pane.focus_handle.is_focused(window) && pane.composition.take().is_some() {
                        cx.notify();
                    }
                }));
        }
        self.finish_expired_screen_sync(cx);
        let is_focused = self.focus_handle.is_focused(window);
        self.sync_cursor_blink(
            is_focused && window.is_window_active() && !self.is_hidden,
            cx,
        );
        let entity = cx.entity();
        let layout_slot = Rc::clone(&self.layout);
        let paint_inputs = PaintInputs {
            paint_frame: Rc::clone(&self.paint_frame),
            runtime: self.runtime.clone(),
            target: self.target.clone(),
            is_focused,
            is_cursor_blink_on: self.is_cursor_blink_on,
            marked_text: self
                .composition
                .as_ref()
                .map(|composition| composition.text.clone()),
            scroll_bar_track: Rc::clone(&self.scroll_bar_track),
            scroll_bar_color: cx.global::<ThemeTokens>().border.strong.into(),
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
                cx.listener(|pane, event, window, cx| {
                    pane.handle_middle_mouse_down(event, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|pane, event, window, cx| pane.handle_mouse_down(event, window, cx)),
            )
            .on_mouse_move(cx.listener(|pane, event, _, cx| pane.handle_mouse_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|pane, event, window, cx| {
                    pane.open_clicked_link(event, window, cx);
                    pane.handle_mouse_up(event, cx)
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|pane, event, _, cx| pane.handle_mouse_up(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
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
    paint_frame: Rc<RefCell<ScreenPaintFrame>>,
    runtime: Option<Arc<DesktopRuntime>>,
    target: TerminalTarget,
    is_focused: bool,
    is_cursor_blink_on: bool,
    marked_text: Option<String>,
    scroll_bar_track: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
    scroll_bar_color: gpui_kit::Hsla,
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
        if let Some(runtime) = self.runtime.as_ref() {
            let mut frame = self.paint_frame.borrow_mut();
            let cell_height = f32::from(layout.cell_size.height);
            let painted = runtime.terminal_registry().read_terminal_screen(
                &self.target.task_id,
                self.target.tab_id.as_deref(),
                |screen| {
                    frame.copy_from(screen, usize::from(layout.rows));
                    let selection = screen.visible_selection().map(grid_selection);
                    (selection, ScrollbackPosition::read(screen, cell_height))
                },
            );
            let (selection, scrollback) = painted.unzip();
            let track = scroll_bar_track_bounds(bounds);
            self.scroll_bar_track.set(Some(track));
            if let Some(selection) = selection {
                let state = GridPaintState {
                    layout,
                    selection,
                    is_focused: self.is_focused,
                    is_cursor_blink_on: self.is_cursor_blink_on,
                    marked_text: self.marked_text.as_deref(),
                };
                paint_screen(&frame, &state, window, cx);
            }
            if let Some((content_height, scroll_top)) =
                scrollback.and_then(|scrollback| scrollback.page_extent())
            {
                paint_page_scrollbar(
                    track,
                    content_height,
                    scroll_top,
                    self.scroll_bar_color,
                    window,
                );
            }
        }
        // 글자 입력과 IME 조합은 이 입력기로 받는다. 글자가 아닌 키는 `handle_key_down`이 먼저 처리한다
        window.handle_input(
            &self.focus_handle,
            gpui_kit::ElementInputHandler::new(bounds, self.entity),
            cx,
        );
    }
}

#[derive(Default)]
struct TerminalComposition {
    text: String,
    selection: Range<usize>,
}

impl TerminalComposition {
    fn replace(&mut self, range: Option<Range<usize>>, replacement: &str) -> usize {
        let Some(range) = range else {
            replacement.clone_into(&mut self.text);
            return 0;
        };
        let (bytes, adjusted_range) = utf16_range(&self.text, range);
        self.text.replace_range(bytes, replacement);
        adjusted_range.start
    }
}

/// UTF-16 범위를 문자 경계에 맞춘 뒤 문자열 바이트 범위와 함께 돌려준다.
fn utf16_range(text: &str, range: Range<usize>) -> (Range<usize>, Range<usize>) {
    let index = |offset| {
        let mut utf16_offset = 0;
        for (byte, character) in text.char_indices() {
            if utf16_offset >= offset {
                return (byte, utf16_offset);
            }
            utf16_offset += character.len_utf16();
        }
        (text.len(), utf16_offset)
    };
    let (start_byte, start_offset) = index(range.start);
    let (end_byte, end_offset) = index(range.end.max(range.start));
    (start_byte..end_byte, start_offset..end_offset)
}

impl gpui_kit::EntityInputHandler for TerminalPane {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let composition = self.composition.as_ref()?;
        let (bytes, range) = utf16_range(&composition.text, range);
        *adjusted_range = Some(range);
        Some(composition.text[bytes].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui_kit::UTF16Selection> {
        let range = self
            .composition
            .as_ref()
            .map_or(0..0, |composition| composition.selection.clone());
        Some(gpui_kit::UTF16Selection {
            range,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.composition
            .as_ref()
            .map(|composition| 0..composition.text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.composition = None;
        cx.notify();
    }

    /// 확정된 글자(일반 타이핑과 IME 확정)를 PTY로 보낸다
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut composition = self.composition.take();
        let text = if let (Some(composition), Some(range)) = (&mut composition, range) {
            composition.replace(Some(range), text);
            composition.text.as_str()
        } else {
            text
        };
        if text.is_empty() {
            cx.notify();
            return;
        }
        self.prepare_for_input(cx);
        self.write_input(text);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut composition = self.composition.take().unwrap_or_default();
        let start = composition.replace(range, new_text);
        if !composition.text.is_empty() {
            // 선택 범위는 새로 들어온 문자열의 시작을 기준으로 한다.
            let selection = new_selected_range.map_or_else(
                || {
                    let end = new_text.encode_utf16().count();
                    end..end
                },
                |range| utf16_range(new_text, range).1,
            );
            composition.selection = start + selection.start..start + selection.end;
            self.composition = Some(composition);
        }
        cx.notify();
    }

    /// IME 후보 창을 커서 칸 옆에 띄운다
    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn whole_rows(first_row: usize, last_row: usize, columns: usize) -> GridSelection {
        GridSelection {
            anchor: GridPoint {
                row: first_row,
                column: 0,
            },
            head: GridPoint {
                row: last_row,
                column: columns - 1,
            },
        }
    }

    #[test]
    fn copying_an_auto_wrapped_line_keeps_it_on_one_line() {
        let mut screen = TerminalScreen::new(10, 4);
        screen.write(b"https://example.com/x\r\nnext");

        let copied = selection_text(&screen, whole_rows(0, 3, 10), 10);

        assert_eq!(copied, "https://example.com/x\nnext");
    }

    #[test]
    fn copying_separate_lines_keeps_their_line_breaks() {
        let mut screen = TerminalScreen::new(10, 3);
        screen.write(b"one   \r\ntwo");

        assert_eq!(
            selection_text(&screen, whole_rows(0, 1, 10), 10),
            "one\ntwo"
        );
    }
}

#[cfg(test)]
mod cursor_blink_tests {
    use gpui_kit::{TestAppContext, VisualTestContext, gpui};
    use kanvibe_domain::i18n::Locale;

    use super::*;
    use crate::qa::options::{DEFAULT_WINDOW_SIZE, LaunchOptions};

    /// xterm.js는 포커스된 터미널의 커서만 깜박이고, 포커스를 잃으면 깜박임을 멈춘 채 커서를 그린다
    #[gpui::test]
    fn only_the_focused_terminal_of_an_active_window_blinks_its_cursor(cx: &mut TestAppContext) {
        let launch = LaunchOptions {
            data_dir: None,
            initial_route: "/ko".into(),
            window_size: DEFAULT_WINDOW_SIZE,
            qa: None,
        };
        cx.update(|cx| crate::init_app(&launch, cx).expect("app init"));
        let target = TerminalTarget {
            task_id: "task-1".into(),
            tab_id: None,
            is_remote: false,
        };
        let (pane, cx) = cx.add_window_view(move |_, cx| {
            TerminalPane::with_transport(target, Translator::new(Locale::Ko, "terminal"), None, cx)
        });
        let blink_states = |cx: &mut VisualTestContext| {
            let mut states = Vec::new();
            for _ in 0..3 {
                cx.executor().advance_clock(CURSOR_BLINK_INTERVAL);
                cx.run_until_parked();
                states.push(pane.read_with(cx, |pane, _| pane.is_cursor_blink_on));
            }
            states
        };

        assert_eq!(blink_states(cx), [true, true, true]);
        let focus_handle = pane.read_with(cx, |pane, _| pane.focus_handle.clone());
        cx.update(|window, cx| {
            window.activate_window();
            focus_handle.focus(window, cx);
        });
        cx.run_until_parked();
        assert_eq!(blink_states(cx), [false, true, false]);
        cx.deactivate_window();
        assert_eq!(blink_states(cx), [true, true, true]);
        assert!(pane.read_with(cx, |pane, _| pane.cursor_blink.is_none()));
    }
}
