//! `/:locale/task/:id` 작업 상세 화면(TS `TaskDetailRoute`).
//!
//! 왼쪽 dock, dock 옆 360px 패널(작업 정보·상태·실행 중 세션·사용량), 메인 영역(터미널 또는 AI 대화 기록)으로 이뤄진다.
//! dock·탭·사용량 단축키는 앱 키 가로채기가 터미널보다 먼저 이 화면의 동작으로 보낸다(`shortcuts.rs`).

mod ai_provider_icon;
mod auto_connect;
mod call_graph;
mod chat_history;
mod chat_message;
mod chat_view;
mod chrome;
mod create_task;
pub mod data;
mod delete_task;
mod description_edit;
mod dock;
mod dock_commands;
mod editors;
mod file_preview;
mod hooks_status;
mod icons;
pub mod layout_state;
mod live_sessions;
pub(crate) mod load_states;
mod message_content;
mod overview;
pub mod pane_chat;
mod panel;
pub(crate) mod project_icon;
mod refresh;
mod session_queries;
mod status_move;
mod tab_shortcuts;
pub(crate) mod usage_cards;
mod usage_panel;
mod window_activation;

pub(crate) use dock_commands::show_alert;

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Task, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::SessionType;

use crate::components::notification_center::{NotificationCenterButton, NotificationPanelClosed};
use crate::components::spinner::is_spinner_animated;
use crate::components::terminal_tabs::{
    TerminalTabsController, TerminalTabsEvent, render_terminal_loader,
};
use crate::components::terminal_view::{TerminalPane, TerminalPaneEvent, TerminalTarget};
use crate::components::{TailwindText, box_shadows};
use crate::shortcuts::{
    OpenCreateTask, OpenTaskDetailDockItem, RunTerminalTabCommand, ShortcutState,
    TASK_DETAIL_KEY_CONTEXT, ToggleNotificationCenter, ToggleTaskDetailUsage, ToggleTerminalChat,
};
use crate::theme::{ThemeTokens, radius, spacing};
use crate::views::ScreenContext;
use chrome::{TaskContextProject, render_task_context};
use data::{TaskDetailSnapshot, load_task_detail};
use dock::{DockAction, DockModel, render_dock};
use layout_state::{DetailPanel, DockBusyState, DockCommand, LayoutState, MainView, dock_entry_at};

/// TS `INITIAL_DESKTOP_LOAD_TIMEOUT_MS`. 첫 조회가 이보다 길면 지연 안내를 띄운다
const INITIAL_LOAD_TIMEOUT: Duration = Duration::from_millis(5_000);
/// dock(48px) + 간격(8px) = `ml-14`
const MAIN_LEFT_MARGIN: f32 = 56.0;

/// 첫 조회 상태(TS `state === undefined | null | 값`과 `useRouteLoadStatus`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailLoad {
    Loading {
        is_delayed: bool,
    },
    Failed(String),
    NotFound,
    /// 터미널 출력마다 돌 수 있는 render가 스냅샷을 깊이 복제하지 않도록 공유한다. 고칠 때는 `Rc::make_mut`을 쓴다
    Loaded(Rc<TaskDetailSnapshot>),
}

pub struct TaskDetailView {
    screen: ScreenContext,
    task_id: String,
    load: DetailLoad,
    /// 데이터를 보여 주는 중 다시 읽기가 실패한 사유(`RouteRefreshFailureBanner`)
    refresh_failure: Option<String>,
    layout: LayoutState,
    busy: DockBusyState,
    terminal: Option<Entity<TerminalPane>>,
    /// 비활성 탭의 터미널. 연결과 스크롤백을 지키려고 숨긴 채 둔다
    hidden_terminals: Vec<Entity<TerminalPane>>,
    /// 숨긴 탭 터미널의 pane 채팅 층. TS는 탭마다 `TerminalPaneChatLayer`를 따로 두고 탭을 바꿔도 내리지 않았다
    hidden_pane_chats: Vec<(TerminalTarget, Entity<pane_chat::PaneChatLayer>)>,
    /// 터미널 탭 상태와 탭 바. 세션이 바뀌면 목록을 비우고 다시 읽는다
    terminal_tabs: Entity<TerminalTabsController>,
    _terminal_tabs_events: [gpui_kit::Subscription; 2],
    focus_handle: FocusHandle,
    translator: Translator,
    common: Translator,
    _load_task: Option<Task<()>>,
    _delay_task: Option<Task<()>>,
    /// 만든 터미널마다 하나씩 둔다. 숨긴 탭의 셸이 끝나도 탭 목록을 다시 읽어야 해서 활성 터미널만 구독하지 않는다
    _terminal_events: Vec<gpui_kit::Subscription>,
    /// 터미널 칸 크기가 바뀌면 전환 버튼 자리도 다시 그린다
    _terminal_redraw: Option<gpui_kit::Subscription>,
    status_move: status_move::StatusMoveState,
    description_edit: Option<description_edit::DescriptionEdit>,
    _description_save: Option<Task<()>>,
    auto_connect: auto_connect::AutoConnectState,
    _auto_connect: Option<Task<()>>,
    _status_move: Option<Task<()>>,
    /// 열린 Done 보존 안내(TS `DoneStatusButton`의 `isDialogOpen`)
    done_confirm: Option<status_move::DoneConfirmState>,
    done_confirm_focus: FocusHandle,
    live_sessions: live_sessions::LiveSessionsState,
    _refresh_signal: Option<gpui_kit::Subscription>,
    /// 터미널 위 띠의 알림 센터(TS `NotificationCenterButton`)
    notification_center: Entity<NotificationCenterButton>,
    _notification_panel_closed: gpui_kit::Subscription,
    /// 알림 패널이 닫혀 다음 그리기에서 터미널로 입력 포커스를 돌려야 한다
    should_refocus_terminal: bool,
    _automatic_pr_lookup: Option<Task<()>>,
    /// `gh` 설치 확인을 이미 거친 자동 PR 조회의 작업. 보드 갱신으로 다시 도는 조회는 확인·질문을 되풀이하지 않는다
    automatic_pr_lookup_task_id: Option<String>,
    /// 새 작업 단축키로 연 생성 모달(TS `isCreateTaskModalOpen`)
    create_task: Option<Entity<crate::components::create_task::CreateTaskModal>>,
    /// 만드는 도중 닫혀 결과를 기다리는 모달
    background_create_tasks: Vec<Entity<crate::components::create_task::CreateTaskModal>>,
    create_task_subscriptions: Vec<gpui_kit::Subscription>,
    _window_activation: Option<gpui_kit::Subscription>,
    /// 이 화면이 그려지는 창. 백그라운드 작업이 확인 대화상자를 띄울 때 쓴다
    window_handle: Option<gpui_kit::AnyWindowHandle>,
    is_window_active: bool,
    /// 지금 창 활성 구간에서 이 작업의 알림을 이미 읽음 처리했는지
    has_marked_notifications_read: bool,
    /// AI 대화 기록 화면. 메인 영역을 기록으로 바꿨을 때만 있다
    chat_view: Option<Entity<chat_view::InlineAiChatView>>,
    /// AI 사용량 패널. 사용량 패널이 보이는 동안만 있다
    usage_panel: Option<Entity<usage_panel::AiUsagePanel>>,
    /// 상태 패널의 hooks 카드. 패널을 닫아도 읽은 상태를 남긴다
    hooks_card: Option<Entity<hooks_status::HooksStatusCard>>,
    delete_task: delete_task::DeleteTaskState,
    _delete_task: Option<Task<()>>,
    /// 작업 정보 카드 편집기와 변경 파일 조회
    editors: editors::EditorState,
    /// 터미널 위 pane 채팅 층. 터미널 뷰와 함께 만들고 버린다
    pane_chat: Option<Entity<pane_chat::PaneChatLayer>>,
    _pane_chat_events: Option<gpui_kit::Subscription>,
}

impl TaskDetailView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let task_id = match &screen.route.route {
            crate::routes::AppRoute::TaskDetail { task_id } => task_id.clone(),
            _ => String::new(),
        };
        let locale = screen.route.locale;
        let navigator = screen.navigator.clone();
        let notification_center =
            cx.new(|cx| NotificationCenterButton::new(locale, navigator, cx).for_terminal_chrome());
        let notification_panel_closed = cx.subscribe(
            &notification_center,
            |view, _, _: &NotificationPanelClosed, cx| {
                view.should_refocus_terminal = true;
                cx.notify();
            },
        );
        let terminal_tabs = cx.new(|_| TerminalTabsController::new(task_id.clone(), locale));
        let terminal_tabs_events = [
            cx.observe(&terminal_tabs, |_, _, cx| cx.notify()),
            cx.subscribe(&terminal_tabs, |view, _, event, cx| {
                view.handle_terminal_tabs_event(*event, cx)
            }),
        ];
        let mut view = Self {
            screen,
            task_id,
            load: DetailLoad::Loading { is_delayed: false },
            refresh_failure: None,
            layout: LayoutState::default(),
            busy: DockBusyState::default(),
            terminal: None,
            hidden_terminals: Vec::new(),
            hidden_pane_chats: Vec::new(),
            terminal_tabs,
            _terminal_tabs_events: terminal_tabs_events,
            focus_handle: cx.focus_handle(),
            translator: Translator::new(locale, "taskDetail"),
            common: Translator::new(locale, "common"),
            _load_task: None,
            _delay_task: None,
            _terminal_events: Vec::new(),
            _terminal_redraw: None,
            status_move: status_move::StatusMoveState::default(),
            description_edit: None,
            _description_save: None,
            auto_connect: auto_connect::AutoConnectState::Connecting,
            _auto_connect: None,
            _status_move: None,
            done_confirm: None,
            done_confirm_focus: cx.focus_handle(),
            live_sessions: live_sessions::LiveSessionsState::default(),
            _refresh_signal: None,
            notification_center,
            _notification_panel_closed: notification_panel_closed,
            should_refocus_terminal: false,
            _automatic_pr_lookup: None,
            automatic_pr_lookup_task_id: None,
            create_task: None,
            background_create_tasks: Vec::new(),
            create_task_subscriptions: Vec::new(),
            _window_activation: None,
            window_handle: None,
            is_window_active: false,
            has_marked_notifications_read: false,
            chat_view: None,
            usage_panel: None,
            hooks_card: None,
            delete_task: delete_task::DeleteTaskState::default(),
            _delete_task: None,
            editors: editors::EditorState::default(),
            pane_chat: None,
            _pane_chat_events: None,
        };
        view.subscribe_to_refresh_signal(cx);
        view.begin_load(cx);
        view
    }

    /// 조회를 시작한다. 데이터가 없을 때만 지연 안내 타이머를 건다
    fn begin_load(&mut self, cx: &mut Context<Self>) {
        let load = load_task_detail(&self.task_id, cx);
        self._load_task = Some(cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |view, cx| view.finish_load(result, cx));
        }));
        self._delay_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(INITIAL_LOAD_TIMEOUT).await;
            let _ = this.update(cx, |view, cx| {
                if let DetailLoad::Loading { is_delayed } = &mut view.load {
                    *is_delayed = true;
                    cx.notify();
                }
            });
        }));
    }

    fn finish_load(
        &mut self,
        result: crate::services::ServiceResult<Option<TaskDetailSnapshot>>,
        cx: &mut Context<Self>,
    ) {
        self._delay_task = None;
        match result {
            Ok(Some(snapshot)) => {
                self.apply_snapshot(snapshot, cx);
                self.look_up_pull_request_automatically(cx);
            }
            Ok(None) => self.load = DetailLoad::NotFound,
            Err(failure) => match self.load {
                DetailLoad::Loaded(_) => self.refresh_failure = Some(failure.display_message()),
                _ => self.load = DetailLoad::Failed(failure.display_message()),
            },
        }
        cx.notify();
    }

    fn apply_snapshot(&mut self, snapshot: TaskDetailSnapshot, cx: &mut Context<Self>) {
        self.refresh_failure = None;
        self.layout.sidebar_default_collapsed = Some(snapshot.sidebar_default_collapsed);
        let session_key = snapshot.has_terminal().then(|| {
            (
                snapshot.task.session_type,
                snapshot.task.session_name.clone(),
            )
        });
        let previous_key = self.snapshot().and_then(|previous| {
            previous.has_terminal().then(|| {
                (
                    previous.task.session_type,
                    previous.task.session_name.clone(),
                )
            })
        });
        let is_session_changed =
            session_key != previous_key || !matches!(self.load, DetailLoad::Loaded(_));
        let is_multiplexer = snapshot.task.session_type != Some(SessionType::Terminal);
        let is_remote = snapshot.is_remote();
        self.load = DetailLoad::Loaded(Rc::new(snapshot));
        if is_session_changed {
            self.terminal = None;
            self.clear_hidden_terminals();
            self.terminal_tabs.update(cx, |tabs, cx| {
                tabs.reset_for_new_session();
                if session_key.is_some() {
                    tabs.start_polling(is_multiplexer, is_remote, cx);
                }
            });
        }
        self.sync_terminal_tab_pane(cx);
        self.mark_task_notifications_read(cx);
        self.sync_tab_polling(cx);
        let needs_auto_connect = self
            .snapshot()
            .is_some_and(|snapshot| !snapshot.has_terminal() && snapshot.task.project_id.is_some());
        if needs_auto_connect {
            self.start_auto_connect(cx);
        }
    }

    /// 터미널 뷰를 지금 세션에 맞춘다. 멀티플렉서는 작업당 PTY 하나, terminal 세션은 탭마다 PTY 하나다.
    /// terminal 세션은 탭 목록을 받기 전에 PTY를 만들지 않는다(버려질 PTY를 만들지 않게)
    fn sync_terminal_tab_pane(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.snapshot().filter(|snapshot| snapshot.has_terminal()) else {
            self.terminal = None;
            self.clear_hidden_terminals();
            return;
        };
        let tab_id = match snapshot.task.session_type {
            Some(SessionType::Terminal) => match self.terminal_tabs.read(cx).state().active_tab() {
                Some(tab) => Some(tab.id.clone()),
                None => {
                    self.terminal = None;
                    self.clear_hidden_terminals();
                    return;
                }
            },
            _ => None,
        };
        let target = TerminalTarget {
            task_id: snapshot.task.id.clone(),
            tab_id,
            is_remote: snapshot.is_remote(),
        };
        let is_current = self
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.read(cx).target() == &target);
        if !is_current {
            self.switch_terminal_pane(target, cx);
        }
        self.drop_closed_tab_terminals(cx);
    }

    /// 비활성 탭 터미널은 버리지 않고 숨긴다. 다시 고르면 같은 연결과 스크롤백, 같은 pane 채팅 층을 그대로 보인다
    /// (TS `TerminalLoader`가 탭마다 `Terminal`과 그 `TerminalPaneChatLayer`를 숨긴 채 두었다)
    fn switch_terminal_pane(&mut self, target: TerminalTarget, cx: &mut Context<Self>) {
        if let Some(previous) = self.terminal.take() {
            let previous_target = previous.read(cx).target().clone();
            previous.update(cx, |pane, cx| pane.set_hidden(true, cx));
            self.hidden_terminals.push(previous);
            if let Some(layer) = self.pane_chat.take() {
                self.hidden_pane_chats.push((previous_target, layer));
            }
        }
        let kept_chat = self
            .hidden_pane_chats
            .iter()
            .position(|(chat_target, _)| chat_target == &target)
            .map(|index| self.hidden_pane_chats.remove(index).1);
        self.pane_chat = Some(match kept_chat {
            Some(layer) => layer,
            None => self.new_pane_chat_layer(&target, cx),
        });
        self._pane_chat_events = None;
        let kept = self
            .hidden_terminals
            .iter()
            .position(|terminal| terminal.read(cx).target() == &target)
            .map(|index| self.hidden_terminals.remove(index));
        let terminal = match kept {
            Some(terminal) => terminal,
            None => {
                let translator = self.translator;
                let terminal = cx.new(|cx| TerminalPane::new(target, translator, cx));
                self.subscribe_terminal_events(&terminal, cx);
                terminal
            }
        };
        let is_history = self.layout.main_view == MainView::History;
        terminal.update(cx, |pane, cx| pane.set_hidden(is_history, cx));
        self._terminal_redraw = Some(cx.observe(&terminal, |_, _, cx| cx.notify()));
        self.terminal = Some(terminal);
    }

    fn subscribe_terminal_events(
        &mut self,
        terminal: &Entity<TerminalPane>,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.subscribe(terminal, |view, _, event, cx| match event {
            TerminalPaneEvent::Connected => view.refresh_tabs(cx),
            TerminalPaneEvent::Closed => view.refresh_tabs_after_shell_exit(cx),
        });
        self._terminal_events.push(subscription);
    }

    /// terminal 세션은 탭 목록을 주기 조회하지 않아, 셸이 끝나 지워진 탭이 탭 바에 남지 않게 닫힘 때 다시 읽는다
    /// (TS `useTerminalTabs`의 `onTerminalClose`). 멀티플렉서 세션은 주기 조회가 맡는다
    fn refresh_tabs_after_shell_exit(&mut self, cx: &mut Context<Self>) {
        let is_terminal_session = matches!(
            &self.load,
            DetailLoad::Loaded(snapshot) if snapshot.task.session_type == Some(SessionType::Terminal)
        );
        if is_terminal_session {
            self.refresh_tabs(cx);
        }
    }

    fn refresh_tabs(&mut self, cx: &mut Context<Self>) {
        self.terminal_tabs.update(cx, |tabs, cx| tabs.refresh(cx));
    }

    /// 탭 목록을 새로 읽으면 터미널 뷰를 활성 탭에 맞추고, 이름 바꾸기가 끝나면 입력 포커스를 터미널로 돌리고,
    /// 마지막 탭을 닫으면 창을 닫는다
    fn handle_terminal_tabs_event(&mut self, event: TerminalTabsEvent, cx: &mut Context<Self>) {
        match event {
            TerminalTabsEvent::TabsRead => self.sync_terminal_tab_pane(cx),
            TerminalTabsEvent::RenameEnded => {
                self.should_refocus_terminal = true;
                cx.notify();
            }
            TerminalTabsEvent::WindowCloseRequested => {
                if let Some(window_handle) = self.window_handle {
                    let _ = window_handle.update(cx, |_, window, _| window.remove_window());
                }
            }
        }
    }

    /// 목록에서 사라진 탭과 다른 작업·세션의 숨긴 터미널을 닫는다
    fn drop_closed_tab_terminals(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self
            .terminal
            .as_ref()
            .map(|terminal| terminal.read(cx).target().clone())
        else {
            self.clear_hidden_terminals();
            return;
        };
        let tabs = &self.terminal_tabs.read(cx).state().tabs;
        let is_kept = |target: &TerminalTarget| {
            target.task_id == active.task_id
                && target.tab_id.as_deref().is_some_and(|tab_id| {
                    active.tab_id.is_some() && tabs.iter().any(|tab| tab.id == tab_id)
                })
        };
        self.hidden_terminals
            .retain(|terminal| is_kept(terminal.read(cx).target()));
        self.hidden_pane_chats.retain(|(target, _)| is_kept(target));
    }

    fn clear_hidden_terminals(&mut self) {
        self.hidden_terminals.clear();
        self.hidden_pane_chats.clear();
        if self.terminal.is_none() {
            self._terminal_events.clear();
        }
    }

    /// 메인 영역이 채팅 기록이면 활성 터미널도 숨긴다. 연결은 그대로 두고 입력 포커스·재연결 예약만 멈춘다
    fn sync_terminal_visibility(&mut self, cx: &mut Context<Self>) {
        let is_hidden = self.layout.main_view == MainView::History;
        if let Some(terminal) = self.terminal.clone() {
            terminal.update(cx, |pane, cx| pane.set_hidden(is_hidden, cx));
        }
    }

    pub fn retry_load(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.load, DetailLoad::Loaded(_)) {
            self.load = DetailLoad::Loading { is_delayed: false };
        }
        self.refresh_failure = None;
        self.begin_load(cx);
        cx.notify();
    }

    fn snapshot(&self) -> Option<&TaskDetailSnapshot> {
        match &self.load {
            DetailLoad::Loaded(snapshot) => Some(snapshot),
            _ => None,
        }
    }

    fn has_task(&self) -> bool {
        self.snapshot().is_some()
    }

    /// 터미널 쪽으로 입력 포커스를 돌린다(TS `requestActiveTerminalFocusAfterUiSettles`)
    fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        match self.terminal.as_ref() {
            Some(_) if self.is_pane_chat_focused(cx) => self.focus_pane_chat(window, cx),
            Some(terminal) if self.layout.main_view == MainView::Terminal => {
                let focus_handle = terminal.read(cx).focus_handle(cx);
                window.focus(&focus_handle, cx);
            }
            _ => window.focus(&self.focus_handle, cx),
        }
    }

    fn run_dock_command(
        &mut self,
        command: DockCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let has_task = self.has_task();
        match command {
            DockCommand::TogglePanel(panel) => self.layout.toggle_panel(panel, has_task),
            DockCommand::ToggleHistory => {
                self.layout.toggle_history();
                self.sync_chat_view(window, cx);
            }
            DockCommand::OpenPullRequest => self.open_pull_request(window, cx),
            DockCommand::OpenInVsCode => self.open_in_vscode(window, cx),
        }
        if self.layout.visible_panel(has_task).is_none() {
            self.focus_terminal(window, cx);
        }
        cx.notify();
    }

    fn handle_dock_action(
        &mut self,
        action: DockAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            DockAction::BackToBoard => self.screen.navigator.navigate("/", cx),
            DockAction::Activate(position) => self.activate_dock_position(position, window, cx),
            DockAction::ToggleUsage => {
                self.run_dock_command(DockCommand::TogglePanel(DetailPanel::Usage), window, cx)
            }
        }
    }

    /// `Mod+{n}`. 범위 밖 번호는 아무것도 하지 않는다
    fn activate_dock_position(
        &mut self,
        position: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pr_url = self
            .snapshot()
            .and_then(|snapshot| snapshot.task.pr_url.clone());
        let entries = self
            .layout
            .dock_entries(self.has_task(), pr_url.as_deref(), self.busy);
        if let Some(entry) = dock_entry_at(&entries, position) {
            let command = entry.command();
            self.run_dock_command(command, window, cx);
        }
    }

    fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout.visible_panel(self.has_task()).is_none() {
            return;
        }
        self.layout.close_panel();
        self.focus_terminal(window, cx);
        cx.notify();
    }
}

impl TaskDetailView {
    /// 기록 화면으로 바꾸면 대화 기록 뷰를 새로 만들어 첫 쪽을 읽고, 터미널로 돌아가면 버린다
    fn sync_chat_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_terminal_visibility(cx);
        self.sync_tab_polling(cx);
        match self.layout.main_view {
            MainView::History if self.chat_view.is_none() => {
                let task_id = self.task_id.clone();
                let translator = self.translator;
                self.chat_view =
                    Some(cx.new(|cx| {
                        chat_view::InlineAiChatView::new(task_id, translator, window, cx)
                    }));
            }
            MainView::Terminal => self.chat_view = None,
            MainView::History => {}
        }
    }

    /// 기록 화면의 `← 터미널`
    fn return_to_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.layout.main_view = MainView::Terminal;
        self.sync_chat_view(window, cx);
        self.focus_terminal(window, cx);
        cx.notify();
    }

    fn render_history_main(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        let Some(chat_view) = self.chat_view.clone() else {
            return div().into_any_element();
        };
        let label = format!("← {}", self.translator.translate("terminal"));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div().flex().child(
                    div()
                        .id("task-detail-back-to-terminal")
                        .relative()
                        .rounded(radius::MD)
                        .px(spacing(4.0))
                        .py(spacing(2.0))
                        .text_scale(crate::theme::text_scale::XS)
                        .text_color(theme.text.brand)
                        .cursor_pointer()
                        .child(label.clone())
                        .child(crate::qa::targets::qa_target_layer(
                            crate::qa::targets::QaTargetDescription {
                                role: "button",
                                name: label.clone().into(),
                                text: label.into(),
                            },
                        ))
                        .on_click(
                            cx.listener(|view, _, window, cx| view.return_to_terminal(window, cx)),
                        ),
                ),
            )
            .child(chat_view)
            .into_any_element()
    }
}

impl Focusable for TaskDetailView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TaskDetailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<ThemeTokens>();
        self.observe_window_activation(window, cx);
        // 처음 그릴 때와 터미널 뷰가 바뀐 뒤에는 포커스가 창 루트에 남아 dock 단축키가 이 화면에 닿지 않는다.
        // 포커스가 이 화면 밖에 있으면 터미널(없으면 화면 자체)로 옮긴다(TS `requestActiveTerminalFocusAfterUiSettles`)
        let terminal_has_focus = self
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.read(cx).focus_handle(cx).is_focused(window));
        let is_loaded = matches!(self.load, DetailLoad::Loaded(_));
        // 방금 연 입력란(탭 이름 바꾸기·설명 편집·새 작업 모달)은 아직 그려지지 않아 화면 안 포커스로 잡히지 않으므로 빼앗지 않는다
        let has_pending_input = self.terminal_tabs.read(cx).is_renaming()
            || self.description_edit.is_some()
            || self.create_task.is_some()
            || self.done_confirm.is_some();
        let has_shell_overlay = self
            .screen
            .navigator
            .locale_shell(cx)
            .is_some_and(|shell| shell.read(cx).has_open_overlay(cx));
        if is_loaded
            && !has_shell_overlay
            && !has_pending_input
            && !self.focus_handle.contains_focused(window, cx)
            && !terminal_has_focus
            && !self.is_description_editor_focused(window, cx)
        {
            self.focus_terminal(window, cx);
        }
        if !has_shell_overlay && std::mem::take(&mut self.should_refocus_terminal) {
            self.focus_terminal(window, cx);
        }
        let body: AnyElement = match &self.load {
            DetailLoad::Loading { is_delayed } => {
                load_states::render_loading(&theme, &self.common, *is_delayed).into_any_element()
            }
            DetailLoad::Failed(reason) => {
                load_states::render_load_failure(&theme, &self.common, reason, cx)
                    .into_any_element()
            }
            DetailLoad::NotFound => {
                let navigator = self.screen.navigator.clone();
                load_states::render_not_found(&theme, &self.translator, move |_, _, cx| {
                    navigator.go_back(cx)
                })
                .into_any_element()
            }
            DetailLoad::Loaded(_) => self.render_loaded(&theme, window, cx),
        };
        div()
            .id("task-detail")
            .key_context(TASK_DETAIL_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(
                cx.listener(|view, action: &OpenTaskDetailDockItem, window, cx| {
                    view.activate_dock_position(action.position, window, cx)
                }),
            )
            .on_action(
                cx.listener(|view, _: &ToggleNotificationCenter, window, cx| {
                    view.notification_center
                        .update(cx, |center, cx| center.toggle_by_keyboard(window, cx))
                }),
            )
            .on_action(cx.listener(|view, _: &OpenCreateTask, window, cx| {
                view.open_create_task(None, window, cx)
            }))
            .on_action(cx.listener(|view, _: &ToggleTaskDetailUsage, window, cx| {
                view.run_dock_command(DockCommand::TogglePanel(DetailPanel::Usage), window, cx)
            }))
            .on_action(cx.listener(|view, _: &ToggleTerminalChat, window, cx| {
                view.toggle_pane_chat(window, cx)
            }))
            .on_action(
                cx.listener(|view, action: &RunTerminalTabCommand, window, cx| {
                    view.run_terminal_tab_shortcut(action.command, window, cx)
                }),
            )
            .on_key_down(
                cx.listener(|view, event: &gpui_kit::KeyDownEvent, window, cx| {
                    if event.keystroke.key != "escape" {
                        return;
                    }
                    if view.cancel_description_edit(window, cx) {
                        cx.stop_propagation();
                    } else if view.layout.visible_panel(view.has_task()).is_some() {
                        view.close_panel(window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .size_full()
            .child(body)
    }
}

impl TaskDetailView {
    fn render_loaded(
        &mut self,
        theme: &ThemeTokens,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let DetailLoad::Loaded(snapshot) = &self.load else {
            return div().into_any_element();
        };
        let snapshot = Rc::clone(snapshot);
        let visible_panel = self.layout.visible_panel(true);
        if let Some(chat_view) = self.chat_view.as_ref() {
            chat_view.update(cx, |view, _| {
                view.is_covered_by_panel = visible_panel.is_some()
            });
        }
        self.sync_live_session_polling(
            visible_panel == Some(DetailPanel::LiveSessions),
            window,
            cx,
        );
        self.sync_usage_panel(visible_panel == Some(DetailPanel::Usage), window, cx);
        self.sync_hooks_card(visible_panel == Some(DetailPanel::Status), cx);
        self.sync_diff_polling(visible_panel == Some(DetailPanel::Overview), window, cx);
        let viewport = window.viewport_size();
        let panel =
            visible_panel.map(|panel| self.render_panel(theme, panel, &snapshot, viewport, cx));
        div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(theme.background.page)
            .p(spacing(3.0))
            .child(self.render_dock(theme, &snapshot, cx))
            .child(
                div()
                    .ml(px(MAIN_LEFT_MARGIN))
                    .h_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(self.render_main(theme, &snapshot, window, cx)),
            )
            .children(panel)
            .children(self.render_refresh_failure_banner(theme, window.viewport_size().width, cx))
            .children(self.render_create_task(theme, window))
            .children(self.render_done_confirm(theme, window, cx))
            .into_any_element()
    }

    fn render_dock(
        &self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shortcut_state = cx.global::<ShortcutState>();
        let entries = self
            .layout
            .dock_entries(true, snapshot.task.pr_url.as_deref(), self.busy);
        let model = DockModel {
            entries: &entries,
            is_usage_active: self.layout.visible_panel(true) == Some(DetailPanel::Usage),
            bindings: &shortcut_state.bindings,
            platform: shortcut_state.platform,
            translator: &self.translator,
            needs_mac_header_offset: cfg!(target_os = "macos"),
            is_spinner_animated: is_spinner_animated(cx),
        };
        let entity = cx.entity().downgrade();
        render_dock(theme, &model, move |action, window, cx| {
            let _ = entity.update(cx, |view, cx| view.handle_dock_action(action, window, cx));
        })
        .into_any_element()
    }

    fn render_main(
        &mut self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_pane_chat_layer(window, cx);
        if self.layout.main_view == MainView::History {
            return self.render_history_main(theme, cx);
        }
        if !snapshot.has_terminal() {
            return match snapshot.task.project_id {
                Some(_) => self.render_auto_connect(theme, cx),
                None => load_states::render_no_terminal(theme, &self.translator).into_any_element(),
            };
        }
        let project = TaskContextProject::of(snapshot);
        let chrome = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing(3.0))
            .px(spacing(4.0))
            .py(spacing(2.5))
            .bg(theme.terminal.chrome)
            .child(render_task_context(project.as_ref(), &snapshot.task.title))
            .child(self.render_tabs_or_session_name(theme, snapshot, cx))
            .child(self.notification_center.clone());
        let entity = cx.entity().downgrade();
        div()
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .rounded(radius::LG)
            .overflow_hidden()
            .shadow(box_shadows(&theme.shadow.md))
            .child(chrome)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .bg(theme.terminal.background)
                    .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
                        let _ = entity.update(cx, |view, cx| view.close_panel(window, cx));
                    })
                    .child(self.render_terminal_area(theme, cx)),
            )
            .into_any_element()
    }

    /// 탭이 있으면 탭 바, 없으면 세션 이름(`text-xs text-terminal-text font-mono truncate`)
    fn render_tabs_or_session_name(
        &self,
        theme: &ThemeTokens,
        snapshot: &TaskDetailSnapshot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.terminal_tabs.read(cx).state().tabs.is_empty() {
            return self.terminal_tabs.clone().into_any_element();
        }
        let name = snapshot
            .task
            .session_name
            .clone()
            .unwrap_or_else(|| self.translator.translate("terminal"));
        div()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .truncate()
                    .text_scale(crate::theme::text_scale::XS)
                    .font(crate::fonts::mono_font(400))
                    .text_color(theme.terminal.text)
                    .child(name),
            )
            .into_any_element()
    }

    /// terminal 세션 탭 목록을 기다리는 중·실패·빈 목록이면 그 안내, 아니면 터미널 뷰(TS `TerminalLoader`)
    fn render_terminal_area(&self, theme: &ThemeTokens, cx: &mut Context<Self>) -> AnyElement {
        match self.terminal.clone() {
            Some(terminal) => {
                let pane_chat = self.pane_chat.clone();
                div()
                    .relative()
                    .size_full()
                    .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| {
                        if let Some(layer) = pane_chat.as_ref() {
                            layer.update(cx, |layer, cx| {
                                layer.handle_terminal_pointer_down(window, cx)
                            });
                        }
                    })
                    .child(terminal)
                    .child(pane_chat::terminal_screen_target())
                    .children(self.pane_chat.clone())
                    .into_any_element()
            }
            None => render_terminal_loader(&self.terminal_tabs, theme, cx),
        }
    }
}
