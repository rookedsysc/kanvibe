//! `/:locale/task/:id` 작업 상세 화면(TS `TaskDetailRoute`).
//!
//! 왼쪽 dock, dock 옆 360px 패널(작업 정보·상태·실행 중 세션·사용량), 메인 영역(터미널 또는 AI 대화 기록)으로 이뤄진다.
//! dock·탭·사용량 단축키는 앱 키 가로채기가 터미널보다 먼저 이 화면의 동작으로 보낸다(`shortcuts.rs`).

mod auto_connect;
mod chrome;
pub mod data;
mod description_edit;
mod dock;
mod icons;
pub mod layout_state;
mod load_states;
mod panel;
mod project_icon;
mod status_move;
mod tab_bar;
mod tab_close;
mod tabs;
pub mod terminal;

use std::time::Duration;

use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, ParentElement, Render, Styled, Task, Window, div, px,
};
use kanvibe_domain::i18n::Translator;
use kanvibe_domain::task::SessionType;

use crate::components::{TailwindText, box_shadows};
use crate::shortcuts::{
    OpenTaskDetailDockItem, RunTerminalTabCommand, ShortcutState, TASK_DETAIL_KEY_CONTEXT,
    ToggleTaskDetailUsage,
};
use crate::theme::{ThemeTokens, radius, spacing};
use crate::views::ScreenContext;
use chrome::{TaskContextProject, render_notification_button, render_task_context};
use data::{TaskDetailSnapshot, load_task_detail};
use dock::{DockAction, DockModel, render_dock};
use layout_state::{DetailPanel, DockBusyState, DockCommand, LayoutState, MainView, dock_entry_at};
use terminal::{TerminalPane, TerminalTarget};

/// TS `INITIAL_DESKTOP_LOAD_TIMEOUT_MS`. 첫 조회가 이보다 길면 지연 안내를 띄운다
const INITIAL_LOAD_TIMEOUT: Duration = Duration::from_millis(5_000);
/// dock(48px) + 간격(8px) = `ml-14`
const MAIN_LEFT_MARGIN: f32 = 56.0;

/// 첫 조회 상태(TS `state === undefined | null | 값`과 `useRouteLoadStatus`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailLoad {
    Loading { is_delayed: bool },
    Failed(String),
    NotFound,
    Loaded(Box<TaskDetailSnapshot>),
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
    terminal_tabs: tabs::TerminalTabsState,
    /// 이름을 바꾸는 중인 탭과 입력
    tab_rename: Option<(String, Entity<gpui_kit::base::input::InputState>)>,
    focus_handle: FocusHandle,
    translator: Translator,
    common: Translator,
    _load_task: Option<Task<()>>,
    _delay_task: Option<Task<()>>,
    _tab_poll: Option<Task<()>>,
    _tab_refresh: Option<Task<()>>,
    is_tab_refresh_queued: bool,
    _tab_command: Option<Task<()>>,
    _tab_rename_events: Option<gpui_kit::Subscription>,
    _terminal_events: Option<gpui_kit::Subscription>,
    is_closing_tab: bool,
    _tab_close: Option<Task<()>>,
    status_move: status_move::StatusMoveState,
    description_edit: Option<description_edit::DescriptionEdit>,
    _description_save: Option<Task<()>>,
    auto_connect: auto_connect::AutoConnectState,
    _auto_connect: Option<Task<()>>,
    _status_move: Option<Task<()>>,
    _status_confirm: Option<Task<()>>,
}

impl TaskDetailView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let task_id = match &screen.route.route {
            crate::routes::AppRoute::TaskDetail { task_id } => task_id.clone(),
            _ => String::new(),
        };
        let locale = screen.route.locale;
        let mut view = Self {
            screen,
            task_id,
            load: DetailLoad::Loading { is_delayed: false },
            refresh_failure: None,
            layout: LayoutState::default(),
            busy: DockBusyState::default(),
            terminal: None,
            terminal_tabs: tabs::TerminalTabsState::default(),
            tab_rename: None,
            focus_handle: cx.focus_handle(),
            translator: Translator::new(locale, "taskDetail"),
            common: Translator::new(locale, "common"),
            _load_task: None,
            _delay_task: None,
            _tab_poll: None,
            _tab_refresh: None,
            is_tab_refresh_queued: false,
            _tab_command: None,
            _tab_rename_events: None,
            _terminal_events: None,
            is_closing_tab: false,
            _tab_close: None,
            status_move: status_move::StatusMoveState::default(),
            description_edit: None,
            _description_save: None,
            auto_connect: auto_connect::AutoConnectState::Connecting,
            _auto_connect: None,
            _status_move: None,
            _status_confirm: None,
        };
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
            Ok(Some(snapshot)) => self.apply_snapshot(snapshot, cx),
            Ok(None) => self.load = DetailLoad::NotFound,
            Err(failure) => match self.load {
                DetailLoad::Loaded(_) => self.refresh_failure = Some(failure.reason),
                _ => self.load = DetailLoad::Failed(failure.reason),
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
        self.load = DetailLoad::Loaded(Box::new(snapshot));
        if is_session_changed {
            self.terminal = None;
            self.terminal_tabs = tabs::TerminalTabsState::default();
            if session_key.is_some() {
                self.start_tab_polling(is_multiplexer, is_remote, cx);
            }
        }
        self.sync_terminal_tab_pane(cx);
        let needs_auto_connect = self
            .snapshot()
            .is_some_and(|snapshot| !snapshot.has_terminal() && snapshot.task.project_id.is_some());
        if needs_auto_connect {
            self.start_auto_connect(cx);
        }
    }

    /// 터미널 뷰를 지금 세션에 맞춘다. 멀티플렉서는 작업당 PTY 하나, terminal 세션은 활성 탭마다 PTY 하나다.
    /// terminal 세션은 탭 목록을 받기 전에 PTY를 만들지 않는다(버려질 PTY를 만들지 않게)
    fn sync_terminal_tab_pane(&mut self, cx: &mut Context<Self>) {
        let Some(snapshot) = self.snapshot().filter(|snapshot| snapshot.has_terminal()) else {
            self.terminal = None;
            return;
        };
        let tab_id = match snapshot.task.session_type {
            Some(SessionType::Terminal) => match self.terminal_tabs.active_tab() {
                Some(tab) => Some(tab.id.clone()),
                None => {
                    self.terminal = None;
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
            let translator = self.translator;
            let terminal = cx.new(|cx| TerminalPane::new(target, translator, cx));
            self._terminal_events =
                Some(cx.subscribe(&terminal, |view, _, event, cx| match event {
                    terminal::TerminalPaneEvent::Connected => view.refresh_tabs(cx),
                }));
            self.terminal = Some(terminal);
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
            DockCommand::ToggleHistory => self.layout.toggle_history(),
            DockCommand::OpenPullRequest => self.open_pull_request(cx),
            DockCommand::OpenInVsCode => self.open_in_vscode(cx),
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

    fn open_pull_request(&mut self, cx: &mut Context<Self>) {
        if let Some(pr_url) = self
            .snapshot()
            .and_then(|snapshot| snapshot.task.pr_url.clone())
        {
            cx.open_url(&pr_url);
        }
    }

    fn open_in_vscode(&mut self, _cx: &mut Context<Self>) {}

    fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout.visible_panel(self.has_task()).is_none() {
            return;
        }
        self.layout.close_panel();
        self.focus_terminal(window, cx);
        cx.notify();
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
        // 처음 그릴 때와 터미널 뷰가 바뀐 뒤에는 포커스가 창 루트에 남아 dock 단축키가 이 화면에 닿지 않는다.
        // 포커스가 이 화면 밖에 있으면 터미널(없으면 화면 자체)로 옮긴다(TS `requestActiveTerminalFocusAfterUiSettles`)
        let terminal_has_focus = self
            .terminal
            .as_ref()
            .is_some_and(|terminal| terminal.read(cx).focus_handle(cx).is_focused(window));
        let is_loaded = matches!(self.load, DetailLoad::Loaded(_));
        if is_loaded && !self.focus_handle.contains_focused(window, cx) && !terminal_has_focus {
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
            .on_action(cx.listener(|view, _: &ToggleTaskDetailUsage, window, cx| {
                view.run_dock_command(DockCommand::TogglePanel(DetailPanel::Usage), window, cx)
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
                    if view.cancel_tab_rename(window, cx)
                        || view.cancel_description_edit(window, cx)
                    {
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
        let Some(snapshot) = self.snapshot().cloned() else {
            return div().into_any_element();
        };
        let panel = self
            .layout
            .visible_panel(true)
            .map(|panel| self.render_panel(theme, panel, &snapshot, cx));
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .child(render_notification_button(
                theme,
                self.common.translate("notifications"),
                0,
                |_, _| {},
            ));
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
        if !self.terminal_tabs.tabs.is_empty() {
            return self.render_tab_bar(theme, cx);
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
            Some(terminal) => terminal.into_any_element(),
            None => load_states::render_terminal_loader(
                theme,
                &self.translator,
                &self.common,
                &self.terminal_tabs,
                cx,
            ),
        }
    }
}
