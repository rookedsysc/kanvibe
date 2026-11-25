//! 열린 창과 각 창의 현재 경로. 작업 상세로 가는 모든 이동은 `navigate_to_task_detail`을 거쳐
//! 같은 작업을 이미 보여 주는 창이 있으면 그 창을 앞으로 가져온다(CLAUDE.md Task Navigation).

use std::collections::HashMap;

use gpui_kit::{AnyWindowHandle, App, Global, WeakEntity, WindowId};
use kanvibe_domain::i18n::Locale;

use crate::app_window::{AppWindow, open_app_window};
use crate::routes::task_detail_href;
use crate::window_open::{
    WindowOpenAction, resolve_existing_navigation_target_window, resolve_window_open_action,
};

struct RegisteredWindow {
    handle: AnyWindowHandle,
    view: WeakEntity<AppWindow>,
    current_path: String,
}

#[derive(Default)]
pub struct WindowRegistry {
    windows: HashMap<WindowId, RegisteredWindow>,
}

impl Global for WindowRegistry {}

impl WindowRegistry {
    pub fn record(
        &mut self,
        handle: AnyWindowHandle,
        view: WeakEntity<AppWindow>,
        current_path: &str,
    ) {
        self.windows.insert(
            handle.window_id(),
            RegisteredWindow {
                handle,
                view,
                current_path: current_path.to_string(),
            },
        );
    }

    pub fn forget(&mut self, window_id: WindowId) {
        self.windows.remove(&window_id);
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
    if request.should_open_new_window {
        return open_new_window(&href, cx);
    }
    let registry = cx.default_global::<WindowRegistry>();
    let existing_window =
        resolve_existing_navigation_target_window(&href, &registry.open_windows(), None)
            .and_then(|window_id| registry.handle_for(window_id));
    if let Some(handle) = existing_window {
        activate(handle, cx);
        return TaskNavigationOutcome::FocusedExistingWindow;
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
