//! `/:locale` 보드 화면. AppSettings `board_view_mode`에 따라 칸반(TS `BoardRoute`·`Board`)이나
//! 워크스페이스(TS `TaskWorkspaceRoute`)를 그린다.

mod actions;
mod board_model;
mod column;
mod context_menu;
mod count_up;
mod dialogs;
mod done_confirm;
mod flip_reflow;
mod folder_search;
mod header;
mod header_popovers;
pub(crate) mod icons;
pub mod kanban;
mod keyboard;
mod live_sessions;
mod polls;
mod registry;
mod registry_actions;
pub(crate) mod services;
mod task_card;
mod task_filters;
mod task_kind_session;
pub(crate) mod text_field;
mod workspace;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    AnyElement, AppContext, Bounds, Context, Entity, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Pixels, Point, Render, ScrollHandle, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, div, px,
};
use kanvibe_domain::board_sort::BoardSortPreference;
use kanvibe_domain::i18n::{Locale, Translator};
use kanvibe_domain::notifications::AppNotification;
use kanvibe_domain::task::{SessionType, TaskStatus};
use kanvibe_domain::task_diff_stats::TaskDiffStats;
use kanvibe_services::kanban::TasksByStatus;

use crate::app_events::{AppEvents, NotificationsChanged, RefreshScope, subscribe_to_refresh};
use crate::board_focus_target::consume_board_focus_task;
use crate::components::TailwindText;
use crate::components::notification_center::NotificationCenterButton;
use crate::components::toast::ToastKind;
use crate::notifications::NotificationCenter;
use crate::qa::perf_markers::mark_startup_phase;
use crate::theme::{ThemeTokens, radius, spacing, text_scale};
use crate::views::ScreenContext;
use board_model::{BoardFilters, DonePagination, ProjectCatalog, TaskDeletionState, TaskMoveState};
pub use count_up::MotionPreference;
pub use services::{BoardServiceFailure, BoardSnapshot, BoardViewMode};
use services::{BoardServices, board_services};
pub use task_filters::TaskKindFilter;
use text_field::{BoardTextField, TextFieldEvent};

/// 첫 조회가 이 시간보다 오래 걸리면 지연 안내를 띄운다(TS `INITIAL_DESKTOP_LOAD_TIMEOUT_MS`)
const LOAD_DELAY_NOTICE_AFTER: Duration = Duration::from_millis(5_000);

/// 화면을 만들 때 첫 조회를 기다리는 최대 시간. 이 안에 끝나면 첫 프레임에 카드를 그린다(60Hz 한 프레임)
const FIRST_LOAD_WAIT: Duration = Duration::from_millis(16);

/// 첫 조회 진행(TS `useRouteLoadStatus`)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RouteLoadStatus {
    is_delayed: bool,
    failure_reason: Option<String>,
}

/// 칸반이 받은 데이터와 그 위의 화면 상태
pub(crate) struct KanbanBoard {
    tasks: TasksByStatus,
    done: DonePagination,
    ssh_hosts: Vec<String>,
    catalog: ProjectCatalog,
    is_done_alert_dismissed: bool,
    default_session_type: SessionType,
    is_vim_mode_enabled: bool,
    sort_preference: BoardSortPreference,
    diff_stats: std::collections::BTreeMap<String, TaskDiffStats>,
    notifications: Vec<AppNotification>,
}

impl KanbanBoard {
    fn from_snapshot(snapshot: BoardSnapshot) -> Self {
        let first_page_size = snapshot.tasks.done.len();
        Self {
            done: DonePagination::new(snapshot.done_total, first_page_size),
            tasks: snapshot.tasks,
            ssh_hosts: snapshot.ssh_hosts,
            catalog: ProjectCatalog::new(snapshot.projects),
            is_done_alert_dismissed: snapshot.is_done_alert_dismissed,
            default_session_type: snapshot.default_session_type,
            is_vim_mode_enabled: snapshot.is_vim_mode_enabled,
            sort_preference: snapshot.sort_preference,
            diff_stats: snapshot.diff_stats_by_task_id,
            notifications: snapshot.notifications,
        }
    }

    /// 새로 받은 데이터로 바꾼다. 진행 중이던 Done 더 보기는 버린다
    fn replace_with(&mut self, snapshot: BoardSnapshot) {
        let first_page_size = snapshot.tasks.done.len();
        self.done.reset(snapshot.done_total, first_page_size);
        self.tasks = snapshot.tasks;
        self.ssh_hosts = snapshot.ssh_hosts;
        self.catalog = ProjectCatalog::new(snapshot.projects);
        self.is_done_alert_dismissed = snapshot.is_done_alert_dismissed;
        self.default_session_type = snapshot.default_session_type;
        self.is_vim_mode_enabled = snapshot.is_vim_mode_enabled;
        self.sort_preference = snapshot.sort_preference;
        self.diff_stats = snapshot.diff_stats_by_task_id;
        self.notifications = snapshot.notifications;
    }

    fn unread_count_by_task(&self) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for notification in self.notifications.iter().filter(|item| !item.is_read) {
            if let Some(task_id) = &notification.task_id {
                *counts.entry(task_id.clone()).or_insert(0) += 1;
            }
        }
        counts
    }
}

/// 우클릭 메뉴가 열린 카드와 위치
#[derive(Debug, Clone)]
pub(crate) struct OpenContextMenu {
    task_id: String,
    position: Point<Pixels>,
    is_status_list_open: bool,
    highlighted_index: usize,
}

/// 헤더에서 열린 팝오버
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeaderPopover {
    ProjectFilter,
    DesktopFilter,
    Sort,
}

pub struct BoardView {
    screen: ScreenContext,
    services: Option<Arc<BoardServices>>,
    view_mode: BoardViewMode,
    load_status: RouteLoadStatus,
    board: Option<KanbanBoard>,
    filters: BoardFilters,
    deletion_states: HashMap<String, TaskDeletionState>,
    move_states: HashMap<String, TaskMoveState>,
    deleting_task_ids: HashSet<String>,
    saving_move_task_ids: HashSet<String>,
    context_menu: Option<OpenContextMenu>,
    pending_done_move: Option<(String, TaskStatus)>,
    is_done_dont_ask_checked: bool,
    open_popover: Option<HeaderPopover>,
    popover_highlight: usize,
    /// 프로젝트 필터 목록 스크롤(키보드로 고른 항목을 보이게 굴린다)
    project_options_scroll: ScrollHandle,
    is_popover_keyboard_navigated: bool,
    popover_focus: FocusHandle,
    project_search: Entity<BoardTextField>,
    menu_focus: FocusHandle,
    dialog_focus: FocusHandle,
    card_focus: HashMap<String, FocusHandle>,
    card_bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    board_focus: FocusHandle,
    page_scroll: ScrollHandle,
    has_claimed_focus: bool,
    /// 작업 상세에서 돌아왔을 때 포커스를 줄 카드(TS `initialFocusTaskId`). 그 카드가 그려지면 한 번 준다
    initial_focus_task_id: Option<String>,
    pending_delete_sequence: Option<(String, std::time::Instant)>,
    find_bar: keyboard::FindBarState,
    find_field: Entity<BoardTextField>,
    find_match: Option<keyboard::FindTextMatch>,
    notification_center: Entity<NotificationCenterButton>,
    workspace: workspace::WorkspaceState,
    workspace_search: Entity<BoardTextField>,
    workspace_scroll: ScrollHandle,
    workspace_list_focus: FocusHandle,
    /// 워크스페이스 목록 포커스 단축키(`Mod+Alt` 단독) 판정과 직전 수식키 상태
    workspace_focus_tracker:
        Option<kanvibe_domain::shortcuts::workspace_focus::WorkspaceListFocusTracker>,
    workspace_modifiers: gpui_kit::Modifiers,
    workspace_terminal_focus: FocusHandle,
    is_registry_open: bool,
    registry_search: Entity<BoardTextField>,
    registry_path: Entity<BoardTextField>,
    registry: registry_actions::RegistryState,
    create_task: Option<Entity<crate::components::create_task::CreateTaskModal>>,
    /// 만드는 중에 닫은 모달. 결과를 받을 때까지 붙잡아 둔다
    background_create_tasks: Vec<Entity<crate::components::create_task::CreateTaskModal>>,
    create_task_subscriptions: Vec<Subscription>,
    _board_load: Task<()>,
    _load_delay_timer: Task<()>,
    polls: polls::BoardPolls,
    /// 열마다 카드 자리 기록(TS `useFlipReflow`). 칸반을 그리지 않는 동안에는 비운다
    column_reflows: HashMap<TaskStatus, flip_reflow::SharedColumnReflow>,
    /// 카드 변경 집계 숫자(파일 수·추가·삭제). 값이 바뀌면 굴려 보인다
    diff_counters: HashMap<String, [Entity<count_up::CountUpNumber>; 3]>,
    live_sessions: live_sessions::LiveSessionState,
    _subscriptions: Vec<Subscription>,
    _app_event_subscriptions: Vec<Subscription>,
    /// 작업 종류 필터를 기억할 창(첫 렌더에서 정한다)
    task_kind_window: Option<gpui_kit::WindowId>,
}

impl BoardView {
    pub fn new(screen: ScreenContext, cx: &mut Context<Self>) -> Self {
        let services = board_services(cx);
        let view_mode = services
            .as_ref()
            .map(|services| services.board_view_mode())
            .unwrap_or(BoardViewMode::Kanban);
        let filters = BoardFilters {
            task_kind: TaskKindFilter::parse(query_value(
                &screen.route.search,
                TaskKindFilter::QUERY_KEY,
            )),
            ..BoardFilters::default()
        };
        let (project_search, find_field, workspace_search) =
            Self::create_text_fields(screen.route.locale, cx);
        let (registry_search, registry_path) =
            Self::create_registry_fields(screen.route.locale, cx);
        let notification_center = {
            let locale = screen.route.locale;
            let navigator = screen.navigator.clone();
            cx.new(|cx| NotificationCenterButton::new(locale, navigator, cx))
        };
        let mut view = Self {
            screen,
            services,
            view_mode,
            load_status: RouteLoadStatus::default(),
            board: None,
            filters,
            deletion_states: HashMap::new(),
            move_states: HashMap::new(),
            deleting_task_ids: HashSet::new(),
            saving_move_task_ids: HashSet::new(),
            context_menu: None,
            pending_done_move: None,
            is_done_dont_ask_checked: false,
            open_popover: None,
            popover_highlight: 0,
            project_options_scroll: ScrollHandle::new(),
            is_popover_keyboard_navigated: false,
            popover_focus: cx.focus_handle(),
            project_search,
            menu_focus: cx.focus_handle(),
            dialog_focus: cx.focus_handle(),
            card_focus: HashMap::new(),
            card_bounds: Rc::default(),
            board_focus: cx.focus_handle(),
            page_scroll: ScrollHandle::new(),
            has_claimed_focus: false,
            initial_focus_task_id: None,
            pending_delete_sequence: None,
            find_bar: keyboard::FindBarState::default(),
            find_field,
            find_match: None,
            notification_center,
            workspace: Default::default(),
            workspace_search,
            workspace_scroll: ScrollHandle::new(),
            workspace_list_focus: cx.focus_handle(),
            workspace_focus_tracker: None,
            workspace_modifiers: gpui_kit::Modifiers::default(),
            workspace_terminal_focus: cx.focus_handle(),
            is_registry_open: false,
            registry_search,
            registry_path,
            registry: Default::default(),
            create_task: None,
            background_create_tasks: Vec::new(),
            create_task_subscriptions: Vec::new(),
            _board_load: Task::ready(()),
            _load_delay_timer: Task::ready(()),
            polls: polls::BoardPolls::default(),
            column_reflows: HashMap::new(),
            diff_counters: HashMap::new(),
            live_sessions: Default::default(),
            _subscriptions: Vec::new(),
            _app_event_subscriptions: Vec::new(),
            task_kind_window: None,
        };
        MotionPreference::ensure_loaded(cx);
        view.subscribe_to_app_events(cx);
        view.begin_first_load(cx);
        view
    }

    /// 새로 고침 신호(TS `useRefreshSignal(["board"])`)와 알림 변경을 구독한다
    fn subscribe_to_app_events(&mut self, cx: &mut Context<Self>) {
        let mut subscriptions: Vec<Subscription> =
            subscribe_to_refresh(&[RefreshScope::Board], cx, |this, cx| this.refresh(cx))
                .into_iter()
                .collect();
        if let Some(bus) = cx.try_global::<AppEvents>().map(|events| events.0.clone()) {
            subscriptions.push(cx.subscribe(&bus, |this, _, _: &NotificationsChanged, cx| {
                this.reload_notifications(cx)
            }));
        }
        self._app_event_subscriptions = subscriptions;
    }

    /// 런타임 알림 목록(`NotificationCenter`)을 카드 배지와 알림 센터에 반영한다
    fn reload_notifications(&mut self, cx: &mut Context<Self>) {
        let Some(notifications) = cx
            .try_global::<NotificationCenter>()
            .map(|center| center.notifications().to_vec())
        else {
            return;
        };
        if let Some(board) = self.board.as_mut() {
            board.notifications = notifications;
            cx.notify();
        }
    }

    /// 프로젝트 필터 검색칸과 찾기 바 입력칸
    fn create_text_fields(
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> (
        Entity<BoardTextField>,
        Entity<BoardTextField>,
        Entity<BoardTextField>,
    ) {
        let theme = *cx.global::<ThemeTokens>();
        let colors = [theme.text.primary, theme.text.muted, theme.text.primary];
        let project_placeholder = Translator::new(locale, "task").translate("projectSearch");
        let find_placeholder = Translator::new(locale, "board.pageFind").translate("placeholder");
        let project_search = cx.new(|cx| BoardTextField::new(project_placeholder, colors, cx));
        let find_field = cx.new(|cx| BoardTextField::new(find_placeholder, colors, cx));
        let workspace_placeholder =
            Translator::new(locale, "workspace").translate("searchPlaceholder");
        let workspace_search = cx.new(|cx| BoardTextField::new(workspace_placeholder, colors, cx));
        (project_search, find_field, workspace_search)
    }

    /// 돌아온 작업의 카드가 이번에 그려졌으면 그 카드에 포커스를 준다. 보이지 않는 작업이면 계속 기다린다
    fn apply_initial_card_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task_id) = self.initial_focus_task_id.as_deref() else {
            return;
        };
        let Some(handle) = self.card_focus.get(task_id).cloned() else {
            return;
        };
        self.initial_focus_task_id = None;
        handle.focus(window, cx);
    }

    /// 창 루트가 쥔 포커스를 보드로 가져온다. 그래야 앱 단축키 동작이 보드까지 내려온다.
    /// 보드가 사라질 때(다른 경로로 이동) 포커스가 허공에 남지 않도록 가져오기 전 포커스로 돌려준다
    fn claim_initial_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_claimed_focus {
            return;
        }
        self.has_claimed_focus = true;
        self.initial_focus_task_id =
            consume_board_focus_task(window.window_handle().window_id(), cx);
        let previous_focus = window.focused(cx);
        self.board_focus.focus(window, cx);
        let Some(previous_focus) = previous_focus else {
            return;
        };
        cx.on_release_in(window, move |this, window, cx| {
            let is_focus_inside_board = this.board_focus.contains_focused(window, cx)
                || this.board_focus.is_focused(window);
            if window.focused(cx).is_none() || is_focus_inside_board {
                previous_focus.focus(window, cx);
            }
        })
        .detach();
    }

    /// 프로젝트 등록 대화상자의 검색칸과 스캔 경로(기본 `~/`)
    fn create_registry_fields(
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> (Entity<BoardTextField>, Entity<BoardTextField>) {
        let theme = *cx.global::<ThemeTokens>();
        let colors = [theme.text.primary, theme.text.muted, theme.text.primary];
        let placeholder = Translator::new(locale, "settings").translate("projectSearchPlaceholder");
        let search = cx.new(|cx| BoardTextField::new(placeholder, colors, cx));
        let path = cx.new(|cx| {
            let mut field = BoardTextField::new("", colors, cx);
            field.set_text("~/", cx);
            field
        });
        (search, path)
    }

    /// 입력칸 이벤트는 창이 있어야 포커스를 옮길 수 있어 첫 렌더에서 구독한다
    fn subscribe_text_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self._subscriptions.is_empty() {
            return;
        }
        let project_search = self.project_search.clone();
        let find_field = self.find_field.clone();
        let registry_path = self.registry_path.clone();
        let registry_search = self.registry_search.clone();
        let workspace_search = self.workspace_search.clone();
        let subscriptions = vec![
            cx.subscribe_in(
                &workspace_search,
                window,
                |this: &mut Self, _, event: &TextFieldEvent, _, cx| {
                    if matches!(event, TextFieldEvent::Changed(_)) {
                        this.workspace.hidden_selection_id =
                            this.workspace.selected_task_id.clone();
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &project_search,
                window,
                |this: &mut Self, _, event: &TextFieldEvent, window, cx| match event {
                    TextFieldEvent::Changed(_) => {
                        this.popover_highlight = 0;
                        cx.notify();
                    }
                    TextFieldEvent::Submitted { .. } => {
                        this.handle_project_filter_key("enter", window, cx)
                    }
                    TextFieldEvent::Escaped => this.handle_project_filter_key("escape", window, cx),
                    TextFieldEvent::NavigationKey(key) => {
                        this.handle_project_filter_key(key, window, cx)
                    }
                },
            ),
            cx.subscribe_in(
                &registry_path,
                window,
                |this: &mut Self, _, event: &TextFieldEvent, window, cx| {
                    this.handle_registry_path_event(event, window, cx)
                },
            ),
            cx.subscribe(&registry_search, |_, _, _: &TextFieldEvent, cx| cx.notify()),
            cx.subscribe_in(
                &find_field,
                window,
                |this: &mut Self, _, event: &TextFieldEvent, window, cx| {
                    this.handle_find_field_event(event, window, cx)
                },
            ),
        ];
        self._subscriptions = subscriptions;
    }

    pub fn view_mode(&self) -> BoardViewMode {
        self.view_mode
    }

    fn locale(&self) -> Locale {
        self.screen.route.locale
    }

    /// 화면을 만들 때의 첫 조회. 조회는 백그라운드에서 돌고, 한 프레임 안에 끝나면 그 결과로 첫 프레임을 그린다.
    /// 첫 프레임을 빈 보드로 그린 뒤 다음 프레임에 카드를 다시 그리면 그 사이 프레임 하나만큼 첫 화면이 늦기 때문이다.
    /// 더 오래 걸리면(고장 주입 지연·멈춤 포함) 지금처럼 로딩 화면을 먼저 그리고 결과를 기다린다
    fn begin_first_load(&mut self, cx: &mut Context<Self>) {
        let load = self.start_snapshot_load(cx);
        match cx
            .foreground_executor()
            .block_with_timeout(FIRST_LOAD_WAIT, load)
        {
            Ok(result) => {
                mark_startup_phase("board_data_loaded");
                self.finish_load(result, cx)
            }
            Err(load) => {
                let load = async move {
                    let result = load.await;
                    mark_startup_phase("board_data_loaded");
                    result
                };
                self.await_load(load, cx)
            }
        }
    }

    /// 보드 데이터를 (다시) 읽는다. 이미 데이터가 있으면 화면은 그대로 두고 실패만 배너로 알린다
    fn begin_load(&mut self, cx: &mut Context<Self>) {
        let load = self.start_snapshot_load(cx);
        self.await_load(load, cx);
    }

    fn start_snapshot_load(
        &self,
        cx: &Context<Self>,
    ) -> Task<Result<BoardSnapshot, BoardServiceFailure>> {
        match &self.services {
            Some(services) => services.load_snapshot(cx),
            None => Task::ready(Ok(BoardSnapshot::empty())),
        }
    }

    /// 조회가 끝나면 화면에 반영하고, 오래 걸리면 지연 안내를 띄운다
    fn await_load(
        &mut self,
        load: impl Future<Output = Result<BoardSnapshot, BoardServiceFailure>> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.load_status = RouteLoadStatus::default();
        let delay_timer = cx.background_executor().timer(LOAD_DELAY_NOTICE_AFTER);
        self._load_delay_timer = cx.spawn(async move |this, cx| {
            delay_timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.board.is_none() && this.load_status.failure_reason.is_none() {
                    this.load_status.is_delayed = true;
                    cx.notify();
                }
            });
        });
        self._board_load = cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| this.finish_load(result, cx));
        });
    }

    fn finish_load(
        &mut self,
        result: Result<BoardSnapshot, BoardServiceFailure>,
        cx: &mut Context<Self>,
    ) {
        self._load_delay_timer = Task::ready(());
        match result {
            Ok(snapshot) => {
                self.load_status = RouteLoadStatus::default();
                match &mut self.board {
                    Some(board) => board.replace_with(snapshot),
                    None => {
                        self.board = Some(KanbanBoard::from_snapshot(snapshot));
                    }
                }
                self.sync_diff_counters(cx);
            }
            Err(failure) => {
                self.load_status = RouteLoadStatus {
                    is_delayed: false,
                    failure_reason: Some(failure.reason),
                };
            }
        }
        cx.notify();
    }

    /// 받은 집계를 카드 숫자에 넘긴다. 처음 보는 작업은 굴리지 않고 그 값으로 시작한다
    fn sync_diff_counters(&mut self, cx: &mut Context<Self>) {
        let Some(board) = self.board.as_ref() else {
            return;
        };
        for (task_id, stats) in &board.diff_stats {
            let values = [stats.file_count, stats.additions, stats.deletions];
            match self.diff_counters.get(task_id) {
                Some(counters) => {
                    for (counter, value) in counters.iter().zip(values) {
                        counter.update(cx, |counter, cx| counter.set_value(value, cx));
                    }
                }
                None => {
                    let test_ids = [
                        "task-diff-file-count",
                        "task-diff-additions",
                        "task-diff-deletions",
                    ];
                    let counters = [0, 1, 2].map(|index| {
                        cx.new(|_| count_up::CountUpNumber::new(values[index], test_ids[index]))
                    });
                    self.diff_counters.insert(task_id.clone(), counters);
                }
            }
        }
    }

    /// 보드 이벤트(다른 창·hook·sync의 변경)를 받으면 다시 읽는다
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.begin_load(cx);
    }

    fn retry_load(&mut self, cx: &mut Context<Self>) {
        self.begin_load(cx);
        cx.notify();
    }
}

impl BoardView {
    /// 화면을 덮는 층의 크기. 칸반은 페이지 스크롤바 홈을 덮지 않는다(Chromium의 `fixed inset-0`이 스크롤바를 빼는 것과 같다)
    pub(crate) fn overlay_viewport(&self, window: &Window) -> gpui_kit::Size<Pixels> {
        let viewport = window.viewport_size();
        if self.view_mode == BoardViewMode::Workspace || self.board.is_none() {
            return viewport;
        }
        gpui_kit::size(
            viewport.width - crate::components::scroll_area::PAGE_SCROLLBAR_WIDTH,
            viewport.height,
        )
    }

    pub(super) fn show_failure_toast(&mut self, message: String, cx: &mut Context<Self>) {
        self.push_toast(ToastKind::Failure, message, None, cx);
    }

    /// 작업 결과 알림은 창의 전역 덮개가 그린다(TS `showActionStatus`)
    pub(super) fn push_toast(
        &mut self,
        kind: ToastKind,
        message: String,
        open_task_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        match self.screen.navigator.locale_shell(cx) {
            Some(shell) => shell.update(cx, |shell, cx| {
                shell.show_action_status(kind, message, open_task_id, cx)
            }),
            None => eprintln!("[kanvibe] {message}"),
        }
    }

    /// 알림 센터(시험이 패널 상태를 읽는다)
    pub fn notification_center(&self) -> &Entity<NotificationCenterButton> {
        &self.notification_center
    }
}

/// `?a=1&b=2`에서 값 하나를 꺼낸다
fn query_value<'a>(search: &'a str, key: &str) -> Option<&'a str> {
    search
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

/// TS `InlineLoadError`: 실패 사유와 다시 시도
pub(crate) fn inline_load_error(
    theme: &ThemeTokens,
    locale: Locale,
    message: String,
    retry_id: &'static str,
    on_retry: impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
) -> gpui_kit::Div {
    let retry_label = Translator::new(locale, "common").translate("retry");
    let error_color = theme.status.error;
    div()
        .flex()
        .items_start()
        .justify_between()
        .gap(spacing(3.0))
        .rounded(radius::MD)
        .border_1()
        .border_color(crate::components::fade(error_color, 0.2))
        .bg(theme.background.page)
        .px(spacing(3.0))
        .py(spacing(2.0))
        .text_scale(text_scale::XS)
        .text_color(error_color)
        .relative()
        .child(crate::qa::targets::qa_target_layer(
            crate::qa::targets::QaTargetDescription {
                role: "alert",
                name: message.clone().into(),
                text: message.clone().into(),
            },
        ))
        .child(div().min_w_0().flex_1().child(message))
        .child(
            div()
                .id(retry_id)
                .relative()
                .flex_none()
                .rounded(radius::BASE)
                .px(px(6.0))
                .py(px(2.0))
                .css_weight(500)
                .cursor_pointer()
                .on_click(on_retry)
                .child(retry_label.clone())
                .child(crate::qa::targets::qa_target_layer(
                    crate::qa::targets::QaTargetDescription {
                        role: "button",
                        name: retry_label.clone().into(),
                        text: retry_label.into(),
                    },
                )),
        )
}

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.subscribe_text_fields(window, cx);
        self.claim_initial_focus(window, cx);
        self.restore_task_kind_session(window, cx);
        self.ensure_board_polls(window, cx);
        let theme = *cx.global::<ThemeTokens>();
        let viewport = window.viewport_size();
        let is_workspace = self.view_mode == BoardViewMode::Workspace;
        if is_workspace {
            self.column_reflows.clear();
        }
        let content: AnyElement = if self.board.is_some() && is_workspace {
            self.render_workspace(window, cx).into_any_element()
        } else if self.board.is_some() {
            self.render_kanban(window, cx).into_any_element()
        } else if let Some(reason) = self.load_status.failure_reason.clone() {
            self.render_initial_failure(&theme, reason, cx)
                .into_any_element()
        } else if is_workspace {
            self.render_workspace_loading(&theme).into_any_element()
        } else {
            kanban::render_board_skeleton(
                &theme,
                f32::from(viewport.height),
                self.load_status
                    .is_delayed
                    .then(|| Translator::new(self.locale(), "common").translate("loadDelayed")),
            )
            .into_any_element()
        };
        self.apply_initial_card_focus(window, cx);
        let root = div()
            .id("board-route")
            .key_context(keyboard::BOARD_KEY_CONTEXT)
            .track_focus(&self.board_focus)
            .size_full()
            .child(content);
        self.register_board_actions(root, cx)
    }
}

impl BoardView {
    /// 워크스페이스는 스켈레톤 없이 빈 화면에서 지연 안내만 띄운다
    fn render_workspace_loading(&self, theme: &ThemeTokens) -> impl IntoElement + use<> {
        let notice = self.load_status.is_delayed.then(|| {
            let text = Translator::new(self.locale(), "common").translate("loadDelayed");
            div()
                .relative()
                .text_scale(text_scale::XS)
                .text_color(theme.text.muted)
                .child(text.clone())
                .child(header::qa_text_layer("status", &text))
        });
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background.page)
            .px(spacing(4.0))
            .children(notice)
    }

    fn render_initial_failure(
        &self,
        theme: &ThemeTokens,
        reason: String,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let message = Translator::new(self.locale(), "common")
            .translate_with("loadFailed", &[("reason", &reason)]);
        let retry = cx.listener(|this, _, _, cx| this.retry_load(cx));
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background.page)
            .px(spacing(4.0))
            .child(div().w_full().max_w(px(448.0)).child(inline_load_error(
                theme,
                self.locale(),
                message,
                "board-load-retry",
                retry,
            )))
    }
}
