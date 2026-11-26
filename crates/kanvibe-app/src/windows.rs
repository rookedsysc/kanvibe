//! 열린 창과 각 창의 현재 경로. 작업 상세로 가는 모든 이동은 `navigate_to_task_detail`을 거쳐
//! 같은 작업을 이미 보여 주는 창이 있으면 그 창을 앞으로 가져온다(CLAUDE.md Task Navigation).

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use gpui_kit::{AnyWindowHandle, App, Global, WeakEntity, WindowId};
use kanvibe_domain::i18n::Locale;
use kanvibe_domain::task_notifications::notification_locale;

use crate::app_window::{AppWindow, open_app_window};
use crate::routes::task_detail_href;
use crate::window_open::{
    WindowOpenAction, resolve_existing_navigation_target_window, resolve_window_open_action,
};

struct RegisteredWindow {
    handle: AnyWindowHandle,
    view: WeakEntity<AppWindow>,
    current_path: String,
    /// 창을 연 순서. 가장 먼저 연 창이 TS `mainWindow`다
    opened_order: u64,
}

/// 알림 문구 locale을 고를 창 경로의 locale(TS `getDesktopNotificationLocale`). 런타임 스레드가 보드 이벤트마다 읽는다.
/// 창이 없거나 경로에 locale 자리가 없으면 `None`이라 런타임이 시스템 locale(TS `app.getLocale()`)을 쓴다
#[derive(Clone, Default)]
pub struct WindowNotificationLocale(Arc<RwLock<Option<String>>>);

impl WindowNotificationLocale {
    pub fn current(&self) -> Option<String> {
        self.0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn replace(&self, locale: Option<String>) {
        *self.0.write().unwrap_or_else(PoisonError::into_inner) = locale;
    }
}

/// `#/<locale>/...`의 첫 자리를 TS `normalizeNotificationLocale`로 줄인다
pub fn notification_locale_for_path(path: &str) -> Option<String> {
    let segment = path
        .trim_start_matches('/')
        .split(['/', '?', '#'])
        .next()
        .filter(|segment| !segment.is_empty())?;
    Some(notification_locale(segment).as_str().to_string())
}

#[derive(Default)]
pub struct WindowRegistry {
    windows: HashMap<WindowId, RegisteredWindow>,
    next_opened_order: u64,
    notification_locale: WindowNotificationLocale,
}

impl Global for WindowRegistry {}

impl WindowRegistry {
    /// 런타임 알림 locale과 같은 칸을 쓰는 목록
    pub fn with_notification_locale(notification_locale: WindowNotificationLocale) -> Self {
        Self {
            notification_locale,
            ..Self::default()
        }
    }

    pub fn record(
        &mut self,
        handle: AnyWindowHandle,
        view: WeakEntity<AppWindow>,
        current_path: &str,
    ) {
        let opened_order = match self.windows.get(&handle.window_id()) {
            Some(window) => window.opened_order,
            None => {
                self.next_opened_order += 1;
                self.next_opened_order
            }
        };
        self.windows.insert(
            handle.window_id(),
            RegisteredWindow {
                handle,
                view,
                current_path: current_path.to_string(),
                opened_order,
            },
        );
        self.refresh_notification_locale();
    }

    pub fn forget(&mut self, window_id: WindowId) {
        self.windows.remove(&window_id);
        self.refresh_notification_locale();
    }

    /// 가장 먼저 연 창(TS `mainWindow`, 닫혔으면 남은 창 가운데 가장 먼저 연 창)의 경로 locale
    fn refresh_notification_locale(&self) {
        let main_window_path = self
            .windows
            .values()
            .min_by_key(|window| window.opened_order)
            .map(|window| window.current_path.as_str());
        self.notification_locale
            .replace(main_window_path.and_then(notification_locale_for_path));
    }

    pub fn current_path(&self, window_id: WindowId) -> Option<&str> {
        self.windows
            .get(&window_id)
            .map(|window| window.current_path.as_str())
    }

    /// 열린 창과 각 창의 현재 경로(`window_open` 규칙의 입력)
    fn open_windows(&self) -> Vec<(WindowId, String)> {
        self.windows
            .iter()
            .map(|(window_id, window)| (*window_id, window.current_path.clone()))
            .collect()
    }

    fn handle_for(&self, window_id: WindowId) -> Option<AnyWindowHandle> {
        self.windows.get(&window_id).map(|window| window.handle)
    }

    pub fn view_for(&self, window_id: WindowId) -> Option<WeakEntity<AppWindow>> {
        self.windows
            .get(&window_id)
            .map(|window| window.view.clone())
    }
}

/// 작업 상세 이동이 어떻게 끝났는지(호출부 테스트·로그용)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNavigationOutcome {
    FocusedExistingWindow,
    NavigatedInWindow,
    OpenedNewWindow,
}

/// 어느 창에서 출발했는지와 새 창 요청 여부
#[derive(Debug, Clone, Copy, Default)]
pub struct TaskNavigationRequest {
    pub origin_window: Option<WindowId>,
    pub should_open_new_window: bool,
}

/// 작업 상세로 간다. 새 창 요청이 아니면 같은 작업 창을 먼저 찾아 앞으로 가져오고, 없을 때만 출발 창에서 이동한다.
pub fn navigate_to_task_detail(
    task_id: &str,
    locale: Locale,
    request: TaskNavigationRequest,
    cx: &mut App,
) -> TaskNavigationOutcome {
    let href = task_detail_href(task_id, Some(locale.as_str()));
    let registry = cx.default_global::<WindowRegistry>();
    let existing_window = find_task_detail_window_to_focus(
        &href,
        &registry.open_windows(),
        request.origin_window,
        request.should_open_new_window,
    )
    .and_then(|window_id| registry.handle_for(window_id));
    if let Some(handle) = existing_window {
        activate(handle, cx);
        return TaskNavigationOutcome::FocusedExistingWindow;
    }
    if request.should_open_new_window {
        return open_new_window(&href, cx);
    }
    let target_view = request
        .origin_window
        .or_else(|| cx.active_window().map(|handle| handle.window_id()))
        .and_then(|window_id| cx.global::<WindowRegistry>().view_for(window_id))
        .and_then(|view| view.upgrade());
    match target_view {
        Some(view) => {
            view.update(cx, |app_window, cx| app_window.navigate(&href, cx));
            TaskNavigationOutcome::NavigatedInWindow
        }
        None => open_new_window(&href, cx),
    }
}

/// 같은 작업 상세를 보여 주는 창. 새 창 요청은 TS `excludeWindow`처럼 출발 창을 빼고 찾는다
fn find_task_detail_window_to_focus<W: Copy + PartialEq>(
    href: &str,
    open_windows: &[(W, String)],
    origin_window: Option<W>,
    should_open_new_window: bool,
) -> Option<W> {
    let excluded_window = if should_open_new_window {
        origin_window
    } else {
        None
    };
    resolve_existing_navigation_target_window(href, open_windows, excluded_window)
}

/// 화면 안 링크를 연다(TS `setWindowOpenHandler`). 앱 경로면 같은 경로 창을 앞으로 가져오거나 새 창을 열고,
/// 앱 경로가 아니면 기본 브라우저로 연다. `origin_window`는 링크를 누른 창이고 같은 경로여도 다시 고르지 않는다
pub fn open_href(
    href: &str,
    origin_window: Option<WindowId>,
    cx: &mut App,
) -> WindowOpenAction<WindowId> {
    let registry = cx.default_global::<WindowRegistry>();
    let action = resolve_window_open_action(href, &registry.open_windows(), origin_window);
    match &action {
        WindowOpenAction::External => cx.open_url(href),
        WindowOpenAction::FocusExisting {
            existing_window, ..
        } => {
            if let Some(handle) = cx.global::<WindowRegistry>().handle_for(*existing_window) {
                activate(handle, cx);
            }
        }
        WindowOpenAction::OpenInternal { route } => {
            open_new_window(route, cx);
        }
    }
    action
}

fn activate(handle: AnyWindowHandle, cx: &mut App) {
    let _ = handle.update(cx, |_, window, _| window.activate_window());
}

fn open_new_window(href: &str, cx: &mut App) -> TaskNavigationOutcome {
    if let Err(error) = open_app_window(href, crate::qa::options::DEFAULT_WINDOW_SIZE, cx) {
        eprintln!("[kanvibe] failed to open task window: {error:#}");
    }
    TaskNavigationOutcome::OpenedNewWindow
}

#[cfg(test)]
mod tests {
    use super::{find_task_detail_window_to_focus, notification_locale_for_path};

    const TASK_DETAIL_HREF: &str = "/ko/task/t1";

    #[test]
    fn new_window_request_focuses_another_window_showing_the_same_task() {
        let open_windows = [
            (1_u32, "/ko".to_string()),
            (2, TASK_DETAIL_HREF.to_string()),
        ];

        let focused_window =
            find_task_detail_window_to_focus(TASK_DETAIL_HREF, &open_windows, Some(1), true);

        assert_eq!(focused_window, Some(2));
    }

    #[test]
    fn new_window_request_skips_the_origin_window_showing_the_same_task() {
        let open_windows = [
            (1_u32, TASK_DETAIL_HREF.to_string()),
            (2, "/ko".to_string()),
        ];

        let focused_window =
            find_task_detail_window_to_focus(TASK_DETAIL_HREF, &open_windows, Some(1), true);

        assert_eq!(focused_window, None);
    }

    #[test]
    fn same_window_request_may_focus_the_origin_window_showing_the_same_task() {
        let open_windows = [(1_u32, TASK_DETAIL_HREF.to_string())];

        let focused_window =
            find_task_detail_window_to_focus(TASK_DETAIL_HREF, &open_windows, Some(1), false);

        assert_eq!(focused_window, Some(1));
    }

    #[test]
    fn notification_locale_comes_from_the_first_route_segment() {
        assert_eq!(
            notification_locale_for_path("/en/task/t1").as_deref(),
            Some("en")
        );
        assert_eq!(
            notification_locale_for_path("/zh?x=1").as_deref(),
            Some("zh")
        );
        assert_eq!(
            notification_locale_for_path("/fr/settings").as_deref(),
            Some("ko")
        );
        assert_eq!(notification_locale_for_path("/"), None);
    }
}
