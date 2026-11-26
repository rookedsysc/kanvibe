//! 터미널 위 pane 채팅 층(TS `TerminalPaneChatLayer`). 터미널의 pane 배치를 1초마다 읽어, 채팅으로 바꾼 pane만 그 칸 위에
//! 채팅 화면으로 덮고 나머지 pane과 터미널은 그대로 둔다. 모드는 pane마다 저장해 화면을 다시 열면 복원한다.
//!
//! 저장된 채팅 모드나 기본 채팅 모드로 자동으로 여는 pane은, 그 pane의 CLI와 대화를 연결할 수 있는 것이 확인된 뒤에만 연다.
//! 확인 전에는 연결 화면 없이 터미널을 그대로 보인다. 사용자가 직접 바꾼 pane은 CLI가 없어도 연다.

use std::collections::{BTreeSet, HashMap, HashSet};

use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Pixels, Render, Styled, Subscription, Task, Window, div, px,
};
use kanvibe_ai::pane_chat::snapshot::{PaneChatLayout, PaneChatLayoutPane, PaneChatSnapshot};
use kanvibe_domain::i18n::Translator;

use super::client::PaneChatClient;
use super::preferences::PaneChatPreferences;
use super::toggle::render_pane_chat_toggle;
use super::view::{PERF_SCREEN, PaneChatView, PaneChatViewEvent, PaneChatViewProps};
use super::widgets::view_button;
use crate::components::TailwindText;
use crate::qa::perf_markers::mark_screen_phase;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};

const LAYOUT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1_000);
/// Codex가 "대화 ID 표시"를 기다리는 상태. 채팅에서 버튼을 눌러야 연결되므로 연다
const CODEX_UNCONFIRMED_REASON: &str = "codex-thread-unconfirmed";

/// 터미널 칸 영역. 칸 단위 pane 좌표를 화면 좌표로 바꿀 때 쓴다
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalChatGrid {
    pub columns: usize,
    pub rows: usize,
    pub width: Pixels,
    pub height: Pixels,
}

/// pane이 차지하는 자리. 분할 없는 PTY는 층 전체다
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaneArea {
    Full,
    Cells {
        left: Pixels,
        top: Pixels,
        width: Pixels,
        height: Pixels,
    },
}

/// pane의 화면 자리. 칸 영역을 모르거나 좌표가 칸 영역을 벗어나면 그리지 않는다
pub fn pane_area(
    pane: Option<&PaneChatLayoutPane>,
    grid: Option<TerminalChatGrid>,
) -> Option<PaneArea> {
    let pane = pane?;
    let Some(bounds) = pane.bounds else {
        return Some(PaneArea::Full);
    };
    let grid = grid.filter(|grid| grid.columns > 0 && grid.rows > 0)?;
    let (columns, rows) = (grid.columns as i64, grid.rows as i64);
    let is_inside = bounds.left >= 0
        && bounds.top >= 0
        && bounds.width > 0
        && bounds.height > 0
        && bounds.left + bounds.width <= columns
        && bounds.top + bounds.height <= rows;
    if !is_inside {
        return None;
    }
    let cell_width = f32::from(grid.width) / columns as f32;
    let cell_height = f32::from(grid.height) / rows as f32;
    Some(PaneArea::Cells {
        left: px(bounds.left as f32 * cell_width),
        top: px(bounds.top as f32 * cell_height),
        width: px(bounds.width as f32 * cell_width),
        height: px(bounds.height as f32 * cell_height),
    })
}

/// 부모(작업 상세)에 알리는 요청
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneChatLayerEvent {
    /// 채팅을 닫고 터미널로 돌아왔다. 터미널에 입력 포커스를 준다
    ReturnToTerminal,
    /// 채팅의 "히스토리"
    OpenHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModesStatus {
    Loading,
    Loaded,
    Failed,
}

pub struct PaneChatLayerProps {
    pub client: PaneChatClient,
    pub task_id: String,
    pub terminal_tab_id: Option<String>,
    pub task_title: Option<String>,
    pub translator: Translator,
}

pub struct PaneChatLayer {
    client: PaneChatClient,
    task_id: String,
    terminal_tab_id: Option<String>,
    task_title: Option<String>,
    translator: Translator,
    preferences: Entity<PaneChatPreferences>,
    is_visible: bool,
    grid: Option<TerminalChatGrid>,
    layout: Option<PaneChatLayout>,
    is_layout_failed: bool,
    is_return_failed: bool,
    /// 지금 채팅으로 보이는 pane
    chat_panes: BTreeSet<String>,
    /// 한 번이라도 연 채팅 화면. 터미널로 돌려도 초안이 남도록 화면을 지우지 않는다
    views: Vec<(String, Entity<PaneChatView>)>,
    modes_status: ModesStatus,
    /// 모드가 정해진 pane. 저장소에서 복원했거나 이 화면에서 사용자나 기본 채팅 모드가 정했다
    decided_modes: HashMap<String, bool>,
    /// 저장된 모드는 채팅이지만 그 pane의 CLI를 아직 확인하지 못해 터미널로 보이는 pane
    saved_chat_panes_awaiting_agent: HashSet<String>,
    probing_panes: HashSet<String>,
    /// 확인하는 중에 연결 신호를 받은 pane. 그 확인이 연결하지 못하고 끝나면 한 번 더 확인한다
    reprobe_panes: HashSet<String>,
    focused_pane: Option<String>,
    is_chat_focused: bool,
    /// 보이기가 바뀔 때마다 올린다. 그 전에 시작한 응답은 버린다
    generation: u64,
    is_toggle_pending: bool,
    /// 배치를 새로 읽은 뒤 열 pane(직접 고른 pane 또는 활성 pane)
    pending_toggle: Option<Option<String>>,
    should_restore_focus: bool,
    is_reading: bool,
    needs_fresh_read: bool,
    /// 확인이 끝난 pane을 열 때 쓰는 창. 작업 상세가 처음 그릴 때 정해진다
    window_handle: Option<gpui_kit::AnyWindowHandle>,
    _read: Option<Task<()>>,
    _poll_timer: Option<Task<()>>,
    _close: Option<Task<()>>,
    _modes: Option<Task<()>>,
    _connection_signals: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PaneChatLayerEvent> for PaneChatLayer {}

impl PaneChatLayer {
    pub fn new(props: PaneChatLayerProps, cx: &mut Context<Self>) -> Self {
        let client = props.client.clone();
        let preferences = cx.new(|cx| PaneChatPreferences::new(client, cx));
        let observe_preferences = cx.observe(&preferences, |layer, _, cx| {
            layer.probe_agent_panes(cx);
            cx.notify();
        });
        let mut layer = Self {
            client: props.client,
            task_id: props.task_id,
            terminal_tab_id: props.terminal_tab_id,
            task_title: props.task_title,
            translator: props.translator,
            preferences,
            is_visible: false,
            grid: None,
            layout: None,
            is_layout_failed: false,
            is_return_failed: false,
            chat_panes: BTreeSet::new(),
            views: Vec::new(),
            modes_status: ModesStatus::Loading,
            decided_modes: HashMap::new(),
            saved_chat_panes_awaiting_agent: HashSet::new(),
            probing_panes: HashSet::new(),
            reprobe_panes: HashSet::new(),
            focused_pane: None,
            is_chat_focused: false,
            generation: 0,
            is_toggle_pending: false,
            pending_toggle: None,
            should_restore_focus: false,
            is_reading: false,
            needs_fresh_read: false,
            window_handle: None,
            _read: None,
            _poll_timer: None,
            _close: None,
            _modes: None,
            _connection_signals: None,
            _subscriptions: vec![observe_preferences],
        };
        layer.load_modes(cx);
        layer.listen_for_connection_signals(cx);
        layer
    }

    /// CLI가 새 대화 ID를 알리면(hook, 세션 기록 파일) 다음 배치 조회를 기다리지 않고 자동으로 열 pane을 신호마다 한 번 확인한다
    fn listen_for_connection_signals(&mut self, cx: &mut Context<Self>) {
        let Some(mut signals) = self.client.watch_connection(&self.task_id) else {
            return;
        };
        self._connection_signals = Some(cx.spawn(async move |this, cx| {
            while signals.next_signal().await.is_some() {
                let handled = this.update(cx, |layer, cx| layer.handle_connection_signal(cx));
                if handled.is_err() {
                    break;
                }
            }
        }));
    }

    fn handle_connection_signal(&mut self, cx: &mut Context<Self>) {
        if !self.is_visible {
            return;
        }
        let probing: Vec<String> = self.probing_panes.iter().cloned().collect();
        self.reprobe_panes.extend(probing);
        self.probe_agent_panes(cx);
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn terminal_tab_id(&self) -> Option<&str> {
        self.terminal_tab_id.as_deref()
    }

    /// 입력이 채팅 화면에 있는지. 터미널은 이때 포커스를 가져가지 않는다
    pub fn is_chat_focused(&self) -> bool {
        self.is_chat_focused
    }

    pub fn layout(&self) -> Option<&PaneChatLayout> {
        self.layout.as_ref()
    }

    /// 채팅으로 보이는 pane의 화면. 숨겨진 채팅 화면은 `None`이다
    pub fn visible_chat(&self, pane_id: &str) -> Option<Entity<PaneChatView>> {
        self.chat_view(pane_id)
            .filter(|_| self.is_pane_chat_visible(pane_id))
    }

    /// 한 번이라도 연 채팅 화면(숨겨졌어도)
    pub fn chat_view(&self, pane_id: &str) -> Option<Entity<PaneChatView>> {
        self.views
            .iter()
            .find(|(id, _)| id == pane_id)
            .map(|(_, view)| view.clone())
    }

    pub fn is_pane_chat_visible(&self, pane_id: &str) -> bool {
        self.is_visible && self.chat_panes.contains(pane_id) && self.area_of(pane_id).is_some()
    }

    /// 배치를 다시 읽는 동안 채팅 pane 자리에 보이는 연결 화면
    pub fn is_loading_shown(&self) -> bool {
        self.is_visible
            && self.layout.is_none()
            && !self.is_layout_failed
            && !self.chat_panes.is_empty()
    }

    /// 아래쪽 안내(배치를 읽지 못함·터미널로 돌아가지 못함)
    pub fn alert_message_key(&self) -> Option<&'static str> {
        let is_shown = self.is_visible && (self.is_layout_failed || self.is_return_failed);
        is_shown.then_some(match self.is_return_failed {
            true => "returnError",
            false => "readError",
        })
    }

    /// 전환 버튼이 붙은 pane(활성 pane이 채팅이 아닐 때)
    pub fn toggle_pane(&self) -> Option<&str> {
        let layout = self.layout.as_ref().filter(|_| self.is_visible)?;
        let active = layout.active_pane_id.as_deref()?;
        let is_placed = self.area_of(active).is_some();
        (is_placed && !self.chat_panes.contains(active)).then_some(active)
    }

    pub fn area_of(&self, pane_id: &str) -> Option<PaneArea> {
        let pane = self
            .layout
            .as_ref()
            .and_then(|layout| layout.panes.iter().find(|pane| pane.id == pane_id));
        pane_area(pane, self.grid)
    }

    /// 작업 상세가 그릴 때마다 터미널이 보이는지와 칸 영역을 알린다
    pub fn set_visibility(
        &mut self,
        is_visible: bool,
        grid: Option<TerminalChatGrid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.window_handle = Some(window.window_handle());
        if self.grid != grid {
            self.grid = grid;
            self.sync_children(window, cx);
        }
        if self.is_visible != is_visible {
            self.is_visible = is_visible;
            self.restart(window, cx);
        }
    }

    /// 보이기가 바뀌면 진행 중인 읽기와 전환을 버리고 새 세대로 시작한다
    fn restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.generation += 1;
        self._read = None;
        self._poll_timer = None;
        self.is_reading = false;
        self.needs_fresh_read = false;
        self.pending_toggle = None;
        self.is_toggle_pending = false;
        self.should_restore_focus = self.is_visible && window.is_window_active();
        let has_focused_chat = self
            .focused_pane
            .as_ref()
            .is_some_and(|pane| self.chat_panes.contains(pane));
        if self.is_visible {
            self.is_chat_focused = self.is_chat_focused || has_focused_chat;
            self.load_layout(false, window, cx);
        } else {
            self.layout = None;
            self.is_chat_focused = false;
        }
        self.sync_children(window, cx);
        cx.notify();
    }

    fn load_modes(&mut self, cx: &mut Context<Self>) {
        let request = self.client.get_modes(&self.task_id, cx);
        self._modes = Some(cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |layer, cx| {
                match result {
                    Ok(saved) => layer.restore_modes(saved),
                    Err(_) => layer.modes_status = ModesStatus::Failed,
                }
                layer.probe_agent_panes(cx);
                cx.notify();
            });
        }));
    }

    /// 조회가 끝나기 전에 사용자가 먼저 바꾼 pane은 저장된 값으로 덮지 않는다
    fn restore_modes(&mut self, saved: std::collections::BTreeMap<String, bool>) {
        for (pane_id, is_chat) in saved {
            if self.decided_modes.contains_key(&pane_id) {
                continue;
            }
            self.decided_modes.insert(pane_id.clone(), is_chat);
            if is_chat {
                self.saved_chat_panes_awaiting_agent.insert(pane_id);
            }
        }
        self.modes_status = ModesStatus::Loaded;
    }

    /// 저장 실패는 화면 동작을 막지 않는다. 지금 화면에는 반영되고 다음에 열 때만 복원되지 않는다
    fn save_mode(&mut self, pane_id: &str, is_chat: bool, cx: &mut Context<Self>) {
        self.decided_modes.insert(pane_id.to_string(), is_chat);
        self.client
            .set_mode(&self.task_id, pane_id, is_chat, cx)
            .detach();
    }

    /// pane 배치를 읽는다. 이미 읽는 중이면 그 뒤에 한 번 더 읽어(`is_fresh`) 방금 바뀐 포커스를 놓치지 않는다
    fn load_layout(&mut self, is_fresh: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_reading {
            self.needs_fresh_read |= is_fresh;
            return;
        }
        self._poll_timer = None;
        self.is_reading = true;
        mark_screen_phase(PERF_SCREEN, "layout_start");
        let generation = self.generation;
        let request = self
            .client
            .get_layout(&self.task_id, self.terminal_tab_id.as_deref(), cx);
        self._read = Some(cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |layer, window, cx| {
                layer.finish_layout(generation, result.ok(), window, cx)
            });
        }));
    }

    fn finish_layout(
        &mut self,
        generation: u64,
        layout: Option<PaneChatLayout>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation || !self.is_visible {
            return;
        }
        let shown_before = self.layout_shown_fields();
        self.is_reading = false;
        mark_screen_phase(PERF_SCREEN, "layout_done");
        match layout {
            Some(layout) => self.apply_layout(layout, window, cx),
            // 이전 좌표로 다른 pane을 덮지 않도록 조회 실패 시 원래 터미널을 보여 준다
            None => {
                self.layout = None;
                self.is_layout_failed = true;
                self.is_chat_focused = false;
            }
        }
        if std::mem::take(&mut self.needs_fresh_read) {
            self.load_layout(false, window, cx);
        } else {
            self.complete_pending_toggle(window, cx);
            self.schedule_layout_poll(window, cx);
        }
        self.sync_children(window, cx);
        self.probe_agent_panes(cx);
        // 배치가 그대로인 주기 조회까지 알리면 작업 상세와 창 전체가 1초마다 다시 그려진다. 포커스 복원, 전환 완료,
        // 확인 결과, 자식 화면은 각자 알린다
        if self.layout_shown_fields() != shown_before {
            cx.notify();
        }
    }

    /// 배치 조회가 끝날 때 바뀌어 화면에 드러날 수 있는 값(덮개 위치·전환 버튼, 안내, 포커스)
    fn layout_shown_fields(&self) -> (Option<PaneChatLayout>, bool, bool, Option<String>) {
        (
            self.layout.clone(),
            self.is_layout_failed,
            self.is_chat_focused,
            self.focused_pane.clone(),
        )
    }

    fn apply_layout(
        &mut self,
        layout: PaneChatLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_layout_failed = false;
        let has_focused_pane = self
            .focused_pane
            .as_ref()
            .is_none_or(|focused| layout.panes.iter().any(|pane| &pane.id == focused));
        if !has_focused_pane {
            self.focused_pane = None;
            self.is_chat_focused = false;
        }
        let restore = std::mem::take(&mut self.should_restore_focus)
            .then(|| {
                self.focused_pane
                    .clone()
                    .or_else(|| layout.active_pane_id.clone())
            })
            .flatten()
            .filter(|pane| {
                self.chat_panes.contains(pane) && layout.panes.iter().any(|entry| &entry.id == pane)
            });
        self.layout = Some(layout);
        if let Some(pane) = restore {
            self.sync_children(window, cx);
            self.focus_pane(&pane, window, cx);
        }
    }

    fn schedule_layout_poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible || !window.is_window_active() {
            return;
        }
        let generation = self.generation;
        self._poll_timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(LAYOUT_POLL_INTERVAL).await;
            let _ = this.update_in(cx, |layer, window, cx| {
                if layer.generation == generation {
                    layer.load_layout(false, window, cx);
                }
            });
        }));
    }

    /// 단축키(`Mod+Shift+J`)·전환 버튼. 포커스가 채팅에 있으면 그 pane을 터미널로 돌리고, 아니면 배치를 새로 읽어 활성 pane(또는 누른 버튼의 pane)을 채팅으로 연다
    pub fn toggle(
        &mut self,
        requested_pane: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_visible || self.is_toggle_pending {
            return;
        }
        let focused_chat = self.focused_pane.clone().filter(|focused| {
            self.chat_panes.contains(focused)
                && self
                    .layout
                    .as_ref()
                    .is_some_and(|layout| layout.panes.iter().any(|pane| &pane.id == focused))
        });
        if let (None, Some(focused)) = (&requested_pane, focused_chat) {
            self.close_pane(focused, window, cx);
            return;
        }
        self.is_toggle_pending = true;
        self.pending_toggle = Some(requested_pane);
        self.load_layout(true, window, cx);
    }

    fn complete_pending_toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(requested) = self.pending_toggle.take() else {
            return;
        };
        self.is_toggle_pending = false;
        let Some(layout) = self.layout.as_ref() else {
            return;
        };
        let pane = requested.or_else(|| layout.active_pane_id.clone());
        let Some(pane) = pane.filter(|pane| layout.panes.iter().any(|entry| &entry.id == pane))
        else {
            return;
        };
        self.saved_chat_panes_awaiting_agent.remove(&pane);
        self.open_pane(&pane, None, window, cx);
        self.save_mode(&pane, true, cx);
        self.chat_panes.insert(pane.clone());
        self.sync_children(window, cx);
        self.focus_pane(&pane, window, cx);
    }

    /// 채팅 pane을 터미널로 돌린다. 런타임의 활성 pane을 그 pane으로 되돌린 뒤에만 채팅을 걷고, 실패하면 채팅을 둔 채 알린다
    pub fn close_pane(&mut self, pane_id: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible || self.is_toggle_pending {
            return;
        }
        let generation = self.generation;
        self.is_toggle_pending = true;
        self.is_return_failed = false;
        let request = self.client.focus_terminal(&self.task_id, &pane_id, cx);
        self._close = Some(cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |layer, window, cx| {
                layer.finish_close(generation, pane_id, matches!(result, Ok(true)), window, cx)
            });
        }));
        cx.notify();
    }

    fn finish_close(
        &mut self,
        generation: u64,
        pane_id: String,
        is_returned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_toggle_pending = false;
        if !self.is_visible || self.generation != generation {
            return;
        }
        if !is_returned {
            self.is_return_failed = true;
            self.focus_pane(&pane_id, window, cx);
            cx.notify();
            return;
        }
        self.save_mode(&pane_id, false, cx);
        self.chat_panes.remove(&pane_id);
        self.focused_pane = None;
        self.is_chat_focused = false;
        self.sync_children(window, cx);
        cx.emit(PaneChatLayerEvent::ReturnToTerminal);
        cx.notify();
    }

    /// 채팅 pane에 입력을 옮긴다. 입력창을 쓸 수 있으면 입력창, 아니면 채팅 화면 자체다
    fn focus_pane(&mut self, pane_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_pane = Some(pane_id.to_string());
        self.is_chat_focused = true;
        if let Some(view) = self.visible_chat(pane_id) {
            view.update(cx, |view, cx| view.focus_input(window, cx));
        }
        cx.notify();
    }

    /// 작업 상세가 터미널 대신 채팅에 입력을 줄 때
    pub fn focus_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.focused_pane.clone() {
            self.focus_pane(&pane, window, cx);
        }
    }

    /// 터미널을 누르면 입력이 터미널로 가고 배치를 다시 읽는다
    pub fn handle_terminal_pointer_down(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_pane = None;
        self.is_chat_focused = false;
        if self.is_visible {
            self.load_layout(false, window, cx);
        }
    }

    fn handle_chat_pointer_down(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        self.focused_pane = Some(pane_id.to_string());
        self.is_chat_focused = true;
        cx.notify();
    }

    /// `initial_snapshot`은 자동으로 열기 전에 방금 확인한 그 pane의 상태다. 화면은 그것으로 첫 화면을 그리고 곧바로 다시 읽는다
    fn open_pane(
        &mut self,
        pane_id: &str,
        initial_snapshot: Option<PaneChatSnapshot>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.views.iter().any(|(id, _)| id == pane_id) {
            return;
        }
        let props = PaneChatViewProps {
            client: self.client.clone(),
            task_id: self.task_id.clone(),
            pane_id: Some(pane_id.to_string()),
            task_title: self.task_title.clone(),
            translator: self.translator,
            preferences: self.preferences.clone(),
            initial_snapshot,
        };
        let view = cx.new(|cx| PaneChatView::new(props, window, cx));
        let owner = pane_id.to_string();
        self._subscriptions.push(cx.subscribe_in(
            &view,
            window,
            move |layer, _, event, window, cx| match event {
                PaneChatViewEvent::OpenTerminal => layer.close_pane(owner.clone(), window, cx),
                PaneChatViewEvent::OpenHistory => cx.emit(PaneChatLayerEvent::OpenHistory),
            },
        ));
        self.views.push((pane_id.to_string(), view));
    }

    /// 열린 채팅 화면마다 보이는지 알린다. 숨겨진 화면은 읽기를 멈춘다
    fn sync_children(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visibility: Vec<(Entity<PaneChatView>, bool)> = self
            .views
            .iter()
            .map(|(pane_id, view)| (view.clone(), self.is_pane_chat_visible(pane_id)))
            .collect();
        for (view, is_visible) in visibility {
            view.update(cx, |view, cx| view.set_visible(is_visible, window, cx));
        }
    }
}

/// 자동으로 채팅을 열지 정하는 확인
impl PaneChatLayer {
    /// 저장된 채팅 모드나 기본 채팅 모드로 열 pane을, 그 pane의 CLI와 대화를 연결할 수 있는지 확인한 뒤 연다.
    /// 기본 채팅 모드는 저장된 기록을 읽은 뒤에만 적용해 사용자가 터미널로 둔 pane을 다시 열지 않는다
    fn probe_agent_panes(&mut self, cx: &mut Context<Self>) {
        let Some(layout) = self.layout.clone().filter(|_| self.is_visible) else {
            return;
        };
        let preferences = self.preferences.read(cx);
        let (is_loaded, is_default_enabled) =
            (preferences.is_loaded, preferences.is_default_enabled);
        let active = layout.active_pane_id.clone();
        let is_left_to_default = is_loaded
            && self.modes_status == ModesStatus::Loaded
            && active
                .as_ref()
                .is_some_and(|pane| !self.decided_modes.contains_key(pane));
        if let (true, false, Some(pane)) = (is_left_to_default, is_default_enabled, active.as_ref())
        {
            self.decided_modes.insert(pane.clone(), false);
        }
        let default_chat_pane = active
            .clone()
            .filter(|_| is_left_to_default && is_default_enabled);
        for pane in &layout.panes {
            let is_awaiting = Some(&pane.id) == default_chat_pane.as_ref()
                || self.saved_chat_panes_awaiting_agent.contains(&pane.id);
            if is_awaiting && self.probing_panes.insert(pane.id.clone()) {
                self.probe_pane(pane.id.clone(), active.clone(), cx);
            }
        }
    }

    /// CLI가 실행 중이고, 대화를 보낼 수 있거나 Codex가 "대화 ID 표시"를 기다리면 열 수 있다. CLI가 없는 pane이 많아 값싼 확인부터 한다
    fn probe_pane(&mut self, pane_id: String, active: Option<String>, cx: &mut Context<Self>) {
        let generation = self.generation;
        let client = self.client.clone();
        let task_id = self.task_id.clone();
        mark_screen_phase(PERF_SCREEN, "probe_start");
        let provider = client.get_agent_provider(&task_id, &pane_id, cx);
        cx.spawn(async move |this, cx| {
            let confirmed = match provider.await {
                Ok(Some(_)) => {
                    let snapshot =
                        cx.update(|cx| client.get_snapshot(&task_id, Some(&pane_id), cx));
                    snapshot.await.ok().filter(|snapshot| {
                        snapshot.can_send
                            || snapshot.transcript.reason.as_deref()
                                == Some(CODEX_UNCONFIRMED_REASON)
                    })
                }
                _ => None,
            };
            mark_screen_phase(PERF_SCREEN, "probe_done");
            let is_ready = confirmed.is_some();
            let window_handle = this
                .update(cx, |layer, cx| {
                    layer.probing_panes.remove(&pane_id);
                    // 확인하는 사이 온 신호는 이 확인이 보지 못했을 수 있다. 연결하지 못했으면 그 신호 몫으로 한 번 더 확인한다
                    if layer.reprobe_panes.remove(&pane_id) && !is_ready {
                        layer.probe_agent_panes(cx);
                    }
                    layer.window_handle
                })
                .ok()
                .flatten();
            let (Some(window_handle), Some(snapshot)) = (window_handle, confirmed) else {
                return;
            };
            let _ = cx.update_window(window_handle, |_, window, cx| {
                this.update(cx, |layer, cx| {
                    if layer.generation == generation && layer.is_visible {
                        layer.open_confirmed_pane(pane_id, active, snapshot, window, cx);
                        cx.notify();
                    }
                })
            });
        })
        .detach();
    }

    /// 확인을 기다리는 사이 사용자가 직접 바꿨거나 기본 채팅 모드를 껐으면 그 선택을 따른다
    fn open_confirmed_pane(
        &mut self,
        pane_id: String,
        active: Option<String>,
        snapshot: PaneChatSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let is_saved_chat_pane = self.saved_chat_panes_awaiting_agent.remove(&pane_id);
        if !is_saved_chat_pane && self.decided_modes.contains_key(&pane_id) {
            return;
        }
        if !is_saved_chat_pane {
            self.save_mode(&pane_id, true, cx);
        }
        self.open_pane(&pane_id, Some(snapshot), window, cx);
        self.chat_panes.insert(pane_id.clone());
        self.sync_children(window, cx);
        // 채팅이 덮는 pane이 지금 쓰는 pane이면 입력도 채팅으로 옮겨야, 가려진 터미널에 글자가 들어가지 않는다
        if Some(&pane_id) == active.as_ref() && self.focused_pane.is_none() {
            self.focus_pane(&pane_id, window, cx);
        }
    }
}

impl PaneChatLayer {
    fn render_loading(&self, theme: &ThemeTokens) -> AnyElement {
        let title = self
            .task_title
            .clone()
            .unwrap_or_else(|| self.translator.translate("paneChat.toggle"));
        div()
            .id("pane-chat-loading")
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .bg(theme.background.page)
            .occlude()
            .child(
                div()
                    .flex()
                    .min_h(px(56.0))
                    .items_center()
                    .gap(spacing(2.0))
                    .border_b_1()
                    .border_color(theme.border.default)
                    .px(spacing(4.0))
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.primary)
                    .child(super::super::icons::detail_icon(
                        super::super::icons::DetailIcon::Chatting,
                        theme.text.primary,
                        px(18.0),
                    ))
                    .child(title),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_scale(text_scale::SM)
                    .text_color(theme.text.secondary)
                    .child(self.translator.translate("paneChat.loading")),
            )
            .into_any_element()
    }

    fn positioned(area: PaneArea) -> gpui_kit::Div {
        match area {
            PaneArea::Full => div().absolute().inset_0(),
            PaneArea::Cells {
                left,
                top,
                width,
                height,
            } => div().absolute().left(left).top(top).w(width).h(height),
        }
    }

    fn render_toggle(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pane = self.toggle_pane()?.to_string();
        let area = self.area_of(&pane)?;
        let label = self.translator.translate("paneChat.toggle");
        let owner = pane.clone();
        let toggle = render_pane_chat_toggle(
            label,
            &pane,
            theme,
            cx.listener(move |layer, _, window, cx| layer.toggle(Some(owner.clone()), window, cx)),
        );
        Some(Self::positioned(area).child(toggle).into_any_element())
    }

    fn render_chat_panes(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = *cx.global::<ThemeTokens>();
        self.views
            .iter()
            .filter_map(|(pane_id, view)| {
                let area = self
                    .area_of(pane_id)
                    .filter(|_| self.is_pane_chat_visible(pane_id))?;
                let owner = pane_id.clone();
                Some(
                    Self::positioned(area)
                        .id(gpui_kit::SharedString::from(format!(
                            "pane-chat-cover-{pane_id}"
                        )))
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .min_w_0()
                        .overflow_hidden()
                        .bg(theme.background.page)
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |layer, _, _, cx| {
                                layer.handle_chat_pointer_down(&owner, cx)
                            }),
                        )
                        .child(view.clone())
                        .into_any_element(),
                )
            })
            .collect()
    }

    fn render_alert(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> Option<AnyElement> {
        let key = self.alert_message_key()?;
        let retry = view_button(
            theme,
            "pane-chat-layer-retry",
            self.translator.translate("paneChat.retry"),
            false,
        )
        .on_click(cx.listener(|layer, _, window, cx| layer.toggle(None, window, cx)));
        Some(
            div()
                .absolute()
                .bottom(spacing(3.0))
                .left(spacing(3.0))
                .right(spacing(3.0))
                .flex()
                .items_center()
                .justify_between()
                .gap(spacing(2.0))
                .rounded(radius::LG)
                .border_1()
                .border_color(theme.border.default)
                .bg(theme.background.surface)
                .p(spacing(3.0))
                .text_scale(text_scale::XS)
                .text_color(theme.text.primary)
                .occlude()
                .child(self.translator.translate(&format!("paneChat.{key}")))
                .child(retry)
                .into_any_element(),
        )
    }
}

impl Render for PaneChatLayer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        // TS 층은 터미널 칸 영역(xterm 화면) 위에 놓여, 칸으로 나누어떨어지지 않는 오른쪽·아래 여백은 터미널 그대로 보인다
        let mut layer = div().absolute().top_0().left_0().overflow_hidden();
        layer = match self.grid {
            Some(grid) => layer.w(grid.width).h(grid.height),
            None => layer.size_full(),
        };
        layer
            .children(self.is_loading_shown().then(|| self.render_loading(&theme)))
            .children(self.render_toggle(&theme, cx))
            .children(self.render_chat_panes(cx))
            .children(self.render_alert(&theme, cx))
    }
}
