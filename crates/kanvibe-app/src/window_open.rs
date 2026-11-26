//! 앱 안 링크를 어느 창에서 열지 정하는 규칙(TS main `windowOpen.ts`).
//!
//! TS는 창 주소(`file://…/index.html#/ko/…`, 개발 서버 `http://localhost:3000/#/ko/…`)에서 hash 경로를 꺼내 비교했다.
//! GPUI 창은 주소가 없고 경로(`/ko/task/t1`)만 가지므로, 앱 안 경로는 `/`·`#/`·`/#/`로 시작하는 문자열이고
//! 스킴이 붙은 주소(https·file 등)는 모두 앱 밖(기본 브라우저)으로 연다. 경로 비교는 쿼리를 뺀 경로끼리 한다.

use kanvibe_domain::notifications::AppNotificationAction;

/// 링크를 열 때 할 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowOpenAction<W> {
    /// 앱 경로가 아니다. 기본 브라우저로 연다
    External,
    /// 같은 경로 창이 없다. 그 경로로 새 창을 연다(GPUI 창은 연 창과 수명이 따로다)
    OpenInternal { route: String },
    /// 같은 경로를 보여 주는 창이 있다. 새 창 대신 그 창을 앞으로 가져온다
    FocusExisting { route: String, existing_window: W },
}

fn normalize_internal_route(route: &str) -> Option<String> {
    if route.is_empty() {
        return None;
    }
    if let Some(rest) = route.strip_prefix("/#/") {
        return Some(format!("/{rest}"));
    }
    if let Some(rest) = route.strip_prefix("#/") {
        return Some(format!("/{rest}"));
    }
    if route.starts_with('/') {
        return Some(route.to_string());
    }
    Some(format!("/{route}"))
}

/// 앱 안 경로면 `/<locale>/...` 모양으로 돌려준다
pub fn extract_internal_route(target: &str) -> Option<String> {
    if target.starts_with('/') || target.starts_with('#') {
        return normalize_internal_route(target);
    }
    None
}

fn without_search(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

/// `route`를 보여 주는 열린 창. `exclude`(링크를 연 창)는 빼고 찾는다
fn find_window_showing<W: Copy + PartialEq>(
    route: &str,
    open_windows: &[(W, String)],
    exclude: Option<W>,
) -> Option<W> {
    open_windows
        .iter()
        .filter(|(window, _)| Some(*window) != exclude)
        .find(|(_, window_path)| {
            extract_internal_route(window_path)
                .is_some_and(|window_route| without_search(&window_route) == without_search(route))
        })
        .map(|(window, _)| *window)
}

/// 앱 안 링크(`window.open`에 해당)를 어떻게 열지 정한다
pub fn resolve_window_open_action<W: Copy + PartialEq>(
    target: &str,
    open_windows: &[(W, String)],
    exclude: Option<W>,
) -> WindowOpenAction<W> {
    let Some(route) = extract_internal_route(target) else {
        return WindowOpenAction::External;
    };
    match find_window_showing(&route, open_windows, exclude) {
        Some(existing_window) => WindowOpenAction::FocusExisting {
            route,
            existing_window,
        },
        None => WindowOpenAction::OpenInternal { route },
    }
}

/// 같은 경로 창이 있으면 그 창, 없으면 `preferred`(지금 앞에 있는 창)로 이동한다
pub fn resolve_navigation_target_window<W: Copy + PartialEq>(
    preferred: Option<W>,
    target: &str,
    open_windows: &[(W, String)],
    exclude: Option<W>,
) -> Option<W> {
    resolve_existing_navigation_target_window(target, open_windows, exclude).or(preferred)
}

/// 같은 경로 창이 있을 때만 그 창을 고른다. 없으면 호출부가 새 창을 열거나 지금 창에서 이동한다
pub fn resolve_existing_navigation_target_window<W: Copy + PartialEq>(
    target: &str,
    open_windows: &[(W, String)],
    exclude: Option<W>,
) -> Option<W> {
    let route = extract_internal_route(target)?;
    find_window_showing(&route, open_windows, exclude)
}

/// 백그라운드 동기화 결과 알림은 지금 화면 위에 검토 대화상자를 띄우므로 경로를 바꾸지 않는다
pub fn should_keep_current_route_for_notification_activation(
    action: Option<&AppNotificationAction>,
) -> bool {
    matches!(
        action,
        Some(AppNotificationAction::BackgroundSyncReview { .. })
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kanvibe_domain::notifications::BackgroundSyncReviewPayload;

    fn windows(paths: &[(u32, &str)]) -> Vec<(u32, String)> {
        paths
            .iter()
            .map(|(id, path)| (*id, (*path).to_string()))
            .collect()
    }

    #[test]
    fn hash_route_without_an_open_window_opens_an_independent_internal_window() {
        // TS는 패키지된 앱의 file:// 주소였다. GPUI는 주소 없이 앱 경로만 쓰므로 같은 hash 경로로 확인한다
        assert_eq!(
            resolve_window_open_action::<u32>("#/ko/task/task-1", &[], None),
            WindowOpenAction::OpenInternal {
                route: "/ko/task/task-1".into()
            }
        );
    }

    #[test]
    fn an_open_window_on_the_same_route_is_focused_instead_of_opening_a_new_one() {
        let open_windows = windows(&[(2, "/ko/task/task-1")]);
        assert_eq!(
            resolve_window_open_action("/#/ko/task/task-1", &open_windows, None),
            WindowOpenAction::FocusExisting {
                route: "/ko/task/task-1".into(),
                existing_window: 2
            }
        );
    }

    #[test]
    fn the_opening_window_itself_is_excluded_and_a_new_window_opens() {
        let open_windows = windows(&[(1, "/ko/task/task-1")]);
        assert_eq!(
            resolve_window_open_action("/#/ko/task/task-1", &open_windows, Some(1)),
            WindowOpenAction::OpenInternal {
                route: "/ko/task/task-1".into()
            }
        );
    }

    #[test]
    fn external_urls_are_not_internal_windows() {
        assert_eq!(
            resolve_window_open_action::<u32>("https://example.com/docs", &[], None),
            WindowOpenAction::External
        );
        assert_eq!(
            resolve_window_open_action::<u32>("file:///tmp/report.html#/ko", &[], None),
            WindowOpenAction::External
        );
    }

    #[test]
    fn navigation_picks_the_window_already_showing_the_detail_route() {
        let open_windows = windows(&[(1, "/ko"), (2, "/ko/task/task-1")]);
        assert_eq!(
            resolve_navigation_target_window(Some(1), "/ko/task/task-1", &open_windows, None),
            Some(2)
        );
    }

    #[test]
    fn navigation_keeps_the_preferred_window_when_no_window_shows_the_route() {
        let open_windows = windows(&[(1, "/ko"), (2, "/ko/task/task-2")]);
        assert_eq!(
            resolve_navigation_target_window(Some(1), "/ko/task/task-1", &open_windows, None),
            Some(1)
        );
    }

    #[test]
    fn existing_navigation_target_is_the_window_showing_the_route() {
        let open_windows = windows(&[(1, "/ko"), (2, "/ko/task/task-1")]);
        assert_eq!(
            resolve_existing_navigation_target_window("/ko/task/task-1", &open_windows, None),
            Some(2)
        );
    }

    #[test]
    fn existing_navigation_target_is_none_instead_of_the_current_window() {
        let open_windows = windows(&[(1, "/ko"), (2, "/ko/task/task-2")]);
        assert_eq!(
            resolve_existing_navigation_target_window("/ko/task/task-1", &open_windows, None),
            None
        );
    }

    #[test]
    fn excluded_current_window_on_the_same_route_still_finds_another_detail_window() {
        let open_windows = windows(&[(1, "/ko/task/task-1"), (2, "/ko/task/task-1")]);
        assert_eq!(
            resolve_existing_navigation_target_window("/ko/task/task-1", &open_windows, Some(1)),
            Some(2)
        );
    }

    #[test]
    fn background_sync_review_notifications_keep_the_current_route() {
        let action = AppNotificationAction::BackgroundSyncReview {
            payload: BackgroundSyncReviewPayload {
                merged_pull_requests: Vec::new(),
                registered_worktrees: Vec::new(),
                pulled_tasks: None,
                failures: None,
            },
        };
        assert!(should_keep_current_route_for_notification_activation(Some(
            &action
        )));
    }

    #[test]
    fn task_notifications_follow_the_target_route() {
        assert!(!should_keep_current_route_for_notification_activation(None));
    }

    #[test]
    fn query_strings_do_not_split_windows_on_the_same_route() {
        let open_windows = windows(&[(3, "/ko/task/task-1?tab=usage")]);
        assert_eq!(
            resolve_existing_navigation_target_window("/ko/task/task-1", &open_windows, None),
            Some(3)
        );
    }
}
