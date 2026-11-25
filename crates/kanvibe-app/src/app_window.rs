//! 창 하나의 루트 뷰. 경로 히스토리를 들고 현재 경로의 화면을 그리며, 창 전역 단축키 동작을 받는다.
//! TS `App.tsx`의 `HashRouter` + `LocaleShell`에 해당한다.

use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement,
    IntoElement, KeyContext, ParentElement, Render, Styled, Task, Window, WindowBounds,
    WindowDecorations, WindowOptions, div, point, px, size,
};

use crate::navigation::NavigationHistory;
use crate::notifications::NotificationCenter;
use crate::qa::targets::QaTargetRegistry;
use crate::routes::AppRoute;
use crate::services::{AppServices, BoardHeaderSnapshot, ServiceResult};
use crate::shortcuts::{
    NavigateBack, NavigateForward, RunTerminalTabCommand, TASK_DETAIL_KEY_CONTEXT,
};

/// 창 루트 키 컨텍스트
pub const WINDOW_KEY_CONTEXT: &str = "KanvibeWindow";
use crate::fonts::sans_font;
use crate::theme::{CANVAS_BACKGROUND, ThemeTokens, text_scale};
use crate::views::board::{
    BoardHeaderModel, TaskKindFilter, render_board_skeleton, render_loaded_board,
};
use crate::views::not_found::render_not_found;
use crate::views::window_controls::window_controls_overlay;
use crate::windows::WindowRegistry;
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;

/// 보드 데이터 조회 상태(`useRouteLoadStatus`의 로딩·성공·실패)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardLoadState {
    Loading,
    Loaded(BoardHeaderSnapshot),
    Failed(String),
}

pub struct AppWindow {
    window_handle: AnyWindowHandle,
    history: NavigationHistory,
    focus_handle: FocusHandle,
    board: BoardLoadState,
    _board_load: Option<Task<()>>,
}

/// gpui-pre-linux 0.3.14 X11 `create_window`는 요청한 x에 2를 더해 창을 만든다(`x11/window.rs`).
/// QA 캡처는 창이 화면 원점에 정확히 놓여야 하므로 그만큼 미리 뺀다.
#[cfg(target_os = "linux")]
const QA_WINDOW_ORIGIN_X: f32 = -2.0;
#[cfg(not(target_os = "linux"))]
const QA_WINDOW_ORIGIN_X: f32 = 0.0;

/// 새 앱 창을 연다. QA 모드에서는 결정적 캡처를 위해 화면 원점(0,0)에 둔다.
pub fn open_app_window(
    initial_path: &str,
    window_size: (f32, f32),
    cx: &mut App,
) -> anyhow::Result<AnyWindowHandle> {
    let content_size = size(px(window_size.0), px(window_size.1));
    let bounds = if cx.has_global::<crate::qa::QaRuntime>() {
        Bounds::new(point(px(QA_WINDOW_ORIGIN_X), px(0.0)), content_size)
    } else {
        Bounds::centered(None, content_size, cx)
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        // 클라이언트 장식은 그림자·크기 조절용 여백을 창 안쪽에 두어 내용이 2px 밀린다. 장식은 창 관리자에 맡긴다
        window_decorations: Some(WindowDecorations::Server),
        app_id: Some("kanvibe".to_string()),
        ..Default::default()
    };
    let initial_path = initial_path.to_string();
    let (handle, _) = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| AppWindow::new(&initial_path, window, cx))
    })?;
    Ok(handle)
}

impl AppWindow {
    pub fn new(initial_path: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let window_id = window.window_handle().window_id();
        cx.on_release(move |_, cx| cx.default_global::<WindowRegistry>().forget(window_id))
            .detach();
        let mut app_window = Self {
            window_handle: window.window_handle(),
            history: NavigationHistory::new(initial_path),
            focus_handle,
            board: BoardLoadState::Loading,
            _board_load: None,
        };
        app_window.on_route_changed(cx);
        app_window
    }

    pub fn current_path(&self) -> &str {
        self.history.current_path()
    }

    /// 앱 안 경로로 이동한다(`router.push`)
    pub fn navigate(&mut self, href: &str, cx: &mut Context<Self>) {
        self.history.push(href);
        self.after_history_change(cx);
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        self.history.back();
        self.after_history_change(cx);
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        self.history.forward();
        self.after_history_change(cx);
    }

    fn after_history_change(&mut self, cx: &mut Context<Self>) {
        self.on_route_changed(cx);
        cx.notify();
    }

    fn on_route_changed(&mut self, cx: &mut Context<Self>) {
        let handle = self.window_handle;
        let view = cx.entity().downgrade();
        let current_path = self.history.current_path().to_string();
        cx.default_global::<WindowRegistry>()
            .record(handle, view, &current_path);
        if self.history.current_route().route == AppRoute::Board {
            self.start_board_load(cx);
        }
    }

    fn start_board_load(&mut self, cx: &mut Context<Self>) {
        self.board = BoardLoadState::Loading;
        let load = cx.global::<AppServices>().load_board_header(cx);
        self._board_load = Some(cx.spawn(async move |this, cx| {
            let result: ServiceResult<BoardHeaderSnapshot> = load.await;
            let _ = this.update(cx, |this, cx| {
                this.board = match result {
                    Ok(snapshot) => BoardLoadState::Loaded(snapshot),
                    Err(failure) => BoardLoadState::Failed(failure.reason),
                };
                cx.notify();
            });
        }));
    }

    fn handle_navigate_back(&mut self, _: &NavigateBack, _: &mut Window, cx: &mut Context<Self>) {
        self.go_back(cx);
    }

    fn handle_navigate_forward(
        &mut self,
        _: &NavigateForward,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.go_forward(cx);
    }

    /// 창 닫기는 터미널이 없는 화면에서도 동작해야 하므로 루트가 받는다. 나머지 탭 명령은 작업 상세가 받는다.
    fn handle_terminal_tab_command(
        &mut self,
        action: &RunTerminalTabCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.command == TerminalTabShortcutCommand::CloseWindow {
            window.remove_window();
        } else {
            cx.propagate();
        }
    }

    /// 작업 상세 경로면 `TaskDetail`을 더해, 포커스가 창 루트에 있어도 단축키가 작업 상세로 판정되게 한다
    fn key_context(&self) -> KeyContext {
        let mut context = KeyContext::default();
        context.add(WINDOW_KEY_CONTEXT);
        if matches!(
            self.history.current_route().route,
            AppRoute::TaskDetail { .. }
        ) {
            context.add(TASK_DETAIL_KEY_CONTEXT);
        }
        context
    }

    fn render_route(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.global::<ThemeTokens>();
        let route = self.history.current_route();
        let viewport = window.viewport_size();
        match route.route {
            AppRoute::Board => self.render_board(&theme, route.locale, viewport, cx),
            _ => {
                let back = cx.listener(|this, _, _, cx| this.go_back(cx));
                render_not_found(&theme, route.locale, back).into_any_element()
            }
        }
    }

    fn render_board(
        &self,
        theme: &ThemeTokens,
        locale: kanvibe_domain::i18n::Locale,
        viewport: gpui_kit::Size<gpui_kit::Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match &self.board {
            BoardLoadState::Loaded(snapshot) => {
                let model = BoardHeaderModel {
                    locale,
                    task_kind_filter: TaskKindFilter::All,
                    selected_project_names: Vec::new(),
                    sort_active_count: snapshot.sort_active_count,
                    unread_notification_count: cx
                        .try_global::<NotificationCenter>()
                        .map(NotificationCenter::unread_count)
                        .unwrap_or(0),
                };
                render_loaded_board(theme, &model, snapshot.status_counts, viewport)
                    .into_any_element()
            }
            BoardLoadState::Loading | BoardLoadState::Failed(_) => {
                render_board_skeleton(theme, f32::from(viewport.height)).into_any_element()
            }
        }
    }
}

/// TS 앱은 macOS에서 신호등 버튼(`hiddenInset`)을 쓰고 그 밖에서만 흰 조작 버튼을 겹쳤다
fn linux_window_controls() -> Option<impl IntoElement> {
    cfg!(not(target_os = "macos")).then(window_controls_overlay)
}

impl Render for AppWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_id = window.window_handle().window_id();
        if cx.has_global::<QaTargetRegistry>() {
            cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
        }
        let theme = *cx.global::<ThemeTokens>();
        let route_view = self.render_route(window, cx);
        div()
            .id("kanvibe-window")
            .key_context(self.key_context())
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::handle_navigate_back))
            .on_action(cx.listener(Self::handle_navigate_forward))
            .on_action(cx.listener(Self::handle_terminal_tab_command))
            .relative()
            .size_full()
            .bg(CANVAS_BACKGROUND)
            .font(sans_font(400))
            .text_size(text_scale::BODY.font_size)
            .line_height(text_scale::BODY.line_height)
            .text_color(theme.text.primary)
            .child(route_view)
            .children(linux_window_controls())
    }
}
