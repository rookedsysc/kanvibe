//! 창 하나의 루트 뷰. 경로 히스토리를 들고 현재 경로의 화면을 그리며, 창 전역 단축키 동작을 받는다.
//! TS `App.tsx`의 `HashRouter` + `LocaleShell`에 해당한다.

use gpui_kit::{
    AnyView, AnyWindowHandle, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement,
    IntoElement, KeyContext, ParentElement, Render, Styled, Window, WindowBounds,
    WindowDecorations, WindowOptions, deferred, div, point, px, size,
};

use crate::navigation::NavigationHistory;
use crate::qa::targets::QaTargetRegistry;
use crate::routes::AppRoute;
use crate::shortcuts::{
    NavigateBack, NavigateForward, OpenNewWindow, RunTerminalTabCommand, TASK_DETAIL_KEY_CONTEXT,
    end_shortcut_capture,
};

/// 창 루트 키 컨텍스트
pub const WINDOW_KEY_CONTEXT: &str = "KanvibeWindow";

use crate::components::dialog::MODAL_LAYER_PRIORITY;
use crate::fonts::sans_font;
use crate::theme::{CANVAS_BACKGROUND, ThemeTokens, text_scale};
use crate::views::window_controls::window_controls_overlay;
use crate::views::{ScreenContext, WindowNavigator, build_screen_view};
use crate::windows::WindowRegistry;
use kanvibe_domain::terminal_tabs::TerminalTabShortcutCommand;

pub struct AppWindow {
    window_handle: AnyWindowHandle,
    history: NavigationHistory,
    focus_handle: FocusHandle,
    /// 현재 경로의 화면 뷰(`views::build_screen_view`). 경로가 바뀔 때마다 새로 만든다
    screen: AnyView,
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
        cx.on_release(move |_, cx| {
            cx.default_global::<WindowRegistry>().forget(window_id);
            end_shortcut_capture(window_id, cx);
        })
        .detach();
        let history = NavigationHistory::new(initial_path);
        let screen = Self::build_screen(&history, cx);
        let app_window = Self {
            window_handle: window.window_handle(),
            history,
            focus_handle,
            screen,
        };
        app_window.record_current_path(cx);
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

    /// 화면을 다시 만들지 않고 지금 경로의 검색 문자열만 바꾼다(TS `navigate(..., { replace: true })`로 필터 쿼리를 고칠 때).
    /// 화면은 이미 그 상태를 그리고 있으므로 경로 기록과 창 목록의 경로만 맞춘다
    pub fn replace_search(&mut self, href: &str, cx: &mut Context<Self>) {
        self.history.replace(href);
        self.record_current_path(cx);
    }

    fn after_history_change(&mut self, cx: &mut Context<Self>) {
        // TS는 문서를 다시 불러오면(`did-navigate`) 그 창의 녹화 표시를 지웠다. 녹화하던 화면 뷰가 여기서 버려진다
        end_shortcut_capture(self.window_handle.window_id(), cx);
        self.screen = Self::build_screen(&self.history, cx);
        self.record_current_path(cx);
        self.reclaim_focus_after_navigation(cx);
        cx.notify();
    }

    /// 화면이 바뀌면 버려진 화면(터미널 등)이 쥐던 포커스가 남아 창 단축키가 막힌다. 루트가 포커스를 다시 잡고,
    /// 새 화면이 자기 요소에 포커스를 주려면 첫 프레임에서 다시 가져간다. 이동은 창을 갱신하는 도중에 불리므로
    /// 그 갱신이 끝난 뒤(`defer`)에 잡는다
    fn reclaim_focus_after_navigation(&self, cx: &mut Context<Self>) {
        let focus_handle = self.focus_handle.clone();
        let window_handle = self.window_handle;
        cx.defer(move |cx| {
            let _ = window_handle.update(cx, |_, window, cx| focus_handle.focus(window, cx));
        });
    }

    fn build_screen(history: &NavigationHistory, cx: &mut Context<Self>) -> AnyView {
        let screen = ScreenContext {
            route: history.current_route(),
            navigator: WindowNavigator::new(cx.entity().downgrade()),
        };
        build_screen_view(screen, cx)
    }

    fn record_current_path(&self, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        cx.default_global::<WindowRegistry>().record(
            self.window_handle,
            view,
            self.history.current_path(),
        );
    }

    /// 지금 경로로 새 창을 연다(TS main `createAppWindow(currentUrl)`)
    fn handle_open_new_window(
        &mut self,
        _: &OpenNewWindow,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_path = self.history.current_path().to_string();
        if let Err(error) =
            open_app_window(&current_path, crate::qa::options::DEFAULT_WINDOW_SIZE, cx)
        {
            eprintln!("[kanvibe] failed to open a new window: {error:#}");
        }
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
}

/// TS 앱은 macOS에서 신호등 버튼(`hiddenInset`)을 쓰고 그 밖에서만 흰 조작 버튼을 겹쳤다. Electron 조작 버튼은
/// 웹 문서 밖에 그려져 모달 배경보다도 위에 보였으므로, 모달보다 한 단계 위 층에 그린다
fn linux_window_controls() -> Option<impl IntoElement> {
    cfg!(not(target_os = "macos"))
        .then(|| deferred(window_controls_overlay()).with_priority(MODAL_LAYER_PRIORITY + 1))
}

impl Render for AppWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_id = window.window_handle().window_id();
        if cx.has_global::<QaTargetRegistry>() {
            cx.global_mut::<QaTargetRegistry>().begin_frame(window_id);
        }
        let theme = *cx.global::<ThemeTokens>();
        div()
            .id("kanvibe-window")
            .key_context(self.key_context())
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::handle_navigate_back))
            .on_action(cx.listener(Self::handle_navigate_forward))
            .on_action(cx.listener(Self::handle_terminal_tab_command))
            .on_action(cx.listener(Self::handle_open_new_window))
            .relative()
            .size_full()
            .bg(CANVAS_BACKGROUND)
            .font(sans_font(400))
            .text_size(text_scale::BODY.font_size)
            .line_height(text_scale::BODY.line_height)
            .text_color(theme.text.primary)
            .child(self.screen.clone())
            .children(linux_window_controls())
    }
}
