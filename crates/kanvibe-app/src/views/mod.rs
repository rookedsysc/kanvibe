//! 화면. 화면 하나가 디렉터리 하나이고, 각 디렉터리는 `XxxView::new(ScreenContext, &mut Context<Self>)`로 만드는
//! 뷰 하나를 내놓는다. 경로가 바뀌면 `build_screen_view`가 새 화면 뷰를 만들어 창 루트가 그린다.
//!
//! M7 병렬 작업 규칙: 화면 작업자는 자기 디렉터리(`views/<screen>/`) 안의 파일만 고친다.
//! `routes.rs`·`app_window.rs`·이 파일·`components/`·다른 화면 디렉터리는 건드리지 않고, 필요한 공용 요소나
//! 라우터 변경은 조정자에게 요청한다. 화면 뷰의 생성자 모양은 바꾸지 않는다.

pub mod ai_accounts;
pub mod board;
pub mod diff;
pub mod not_found;
pub mod pane_layout;
pub mod settings;
pub mod shortcut_settings;
pub mod task_detail;
pub mod window_controls;

use gpui_kit::{AnyView, App, AppContext, Div, ParentElement, Styled, WeakEntity, div};

use crate::app_window::AppWindow;
pub use crate::components::TailwindText;
use crate::routes::{AppRoute, LocalizedRoute};
use crate::theme::ThemeTokens;

/// 화면 뷰가 받는 것: 그릴 경로(locale·경로 인자·쿼리)와 창 이동 수단
pub struct ScreenContext {
    pub route: LocalizedRoute,
    pub navigator: WindowNavigator,
}

/// 화면이 자기 창의 경로를 바꿀 때 쓴다. 작업 상세로 갈 때는 `windows::navigate_to_task_detail`을 쓴다(CLAUDE.md Task Navigation)
#[derive(Clone)]
pub struct WindowNavigator {
    app_window: WeakEntity<AppWindow>,
}

impl WindowNavigator {
    pub fn new(app_window: WeakEntity<AppWindow>) -> Self {
        Self { app_window }
    }

    /// 앱 안 경로로 이동한다(`router.push`)
    pub fn navigate(&self, href: &str, cx: &mut App) {
        let _ = self
            .app_window
            .update(cx, |app_window, cx| app_window.navigate(href, cx));
    }

    /// 화면을 다시 만들지 않고 지금 경로의 검색 문자열(필터 쿼리)만 바꾼다
    pub fn replace_search(&self, href: &str, cx: &mut App) {
        let _ = self
            .app_window
            .update(cx, |app_window, cx| app_window.replace_search(href, cx));
    }

    pub fn go_back(&self, cx: &mut App) {
        let _ = self
            .app_window
            .update(cx, |app_window, cx| app_window.go_back(cx));
    }
}

/// 경로에 맞는 화면 뷰를 만든다
pub fn build_screen_view(screen: ScreenContext, cx: &mut App) -> AnyView {
    match &screen.route.route {
        AppRoute::Board => cx.new(|cx| board::BoardView::new(screen, cx)).into(),
        AppRoute::TaskDetail { .. } => cx
            .new(|cx| task_detail::TaskDetailView::new(screen, cx))
            .into(),
        AppRoute::TaskDiff { .. } => cx.new(|cx| diff::TaskDiffView::new(screen, cx)).into(),
        AppRoute::Settings => cx.new(|cx| settings::SettingsView::new(screen, cx)).into(),
        AppRoute::ShortcutSettings => cx
            .new(|cx| shortcut_settings::ShortcutSettingsView::new(screen, cx))
            .into(),
        AppRoute::AiAccounts => cx
            .new(|cx| ai_accounts::AiAccountsView::new(screen, cx))
            .into(),
        AppRoute::PaneLayout => cx
            .new(|cx| pane_layout::PaneLayoutView::new(screen, cx))
            .into(),
        AppRoute::NotFound { .. } => cx.new(|cx| not_found::NotFoundView::new(screen, cx)).into(),
    }
}

/// TS `RouteLoadingFallback`(`min-h-screen flex items-center justify-center bg-bg-page text-text-muted`).
/// 아직 구현하지 않은 화면도 이 모양을 그린다(TS가 화면 코드를 불러오는 동안 보이던 모양)
pub fn route_loading_fallback(theme: &ThemeTokens) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.background.page)
        .text_color(theme.text.muted)
        .child("Loading...")
}
