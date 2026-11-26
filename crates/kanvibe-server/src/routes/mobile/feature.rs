//! 기능 경로(TS `electron/mobileFeatureRoutes.js`). 기본 경로보다 먼저 본다.
//!
//! 표의 각 항목이 허용한 서비스만 부른다. 경로나 본문의 문자열을 메서드 이름으로 쓰지 않는다.

use http::Method;
use serde_json::Value;

use super::{MobileRequest, RouteError, RouteResult, SERVER_ERROR, ai, diagram, spread_success};
use crate::bridge::{BridgeError, FeatureCall};
use crate::http::body::{HttpResponse, MAX_BODY_BYTES, failure_response, json_response};
use crate::routes::js_semantics::{MobileUrl, decode_all, safe_integer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeatureKind {
    SearchTasks,
    DoneTasks,
    TaskDetails,
    BranchTask,
    RefreshPr,
    Hosts,
    ProjectColor,
    ProjectTasks,
    ProjectBranchTask,
    DeleteProject,
    DiffStats,
    TaskDiff,
    DiffFile,
    SaveDiffFile,
    RenameTab,
    MoveTab,
    PaneImage,
    PaneLayouts,
    SavePaneLayout,
    DeletePaneLayout,
    Settings,
    UpdateSettings,
    ChangeTargets,
    ChangeAll,
    RunSync,
    SyncStatus,
    StartSync,
    SyncReview,
    DeleteSyncReview,
    TaskNotificationsRead,
}

struct FeatureRoute {
    method: Method,
    pattern: &'static str,
    kind: FeatureKind,
    /// 본문을 읽는 경로의 한도. 본문이 없는 경로는 `None`
    body_limit: Option<usize>,
}

const fn route(
    method: Method,
    pattern: &'static str,
    kind: FeatureKind,
    body_limit: Option<usize>,
) -> FeatureRoute {
    FeatureRoute {
        method,
        pattern,
        kind,
        body_limit,
    }
}

const DEFAULT: Option<usize> = Some(MAX_BODY_BYTES);

/// TS `FEATURE_ROUTES`와 같은 순서
static FEATURE_ROUTES: [FeatureRoute; 30] = [
    route(
        Method::GET,
        "/api/mobile/tasks/search",
        FeatureKind::SearchTasks,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/tasks/done",
        FeatureKind::DoneTasks,
        None,
    ),
    route(
        Method::PATCH,
        "/api/mobile/tasks/*/details",
        FeatureKind::TaskDetails,
        DEFAULT,
    ),
    route(
        Method::POST,
        "/api/mobile/tasks/*/branch",
        FeatureKind::BranchTask,
        DEFAULT,
    ),
    route(
        Method::POST,
        "/api/mobile/tasks/*/pr/refresh",
        FeatureKind::RefreshPr,
        None,
    ),
    route(Method::GET, "/api/mobile/hosts", FeatureKind::Hosts, None),
    route(
        Method::PATCH,
        "/api/mobile/projects/*/color",
        FeatureKind::ProjectColor,
        DEFAULT,
    ),
    route(
        Method::GET,
        "/api/mobile/projects/*/tasks",
        FeatureKind::ProjectTasks,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/projects/*/task",
        FeatureKind::ProjectBranchTask,
        None,
    ),
    route(
        Method::DELETE,
        "/api/mobile/projects/*",
        FeatureKind::DeleteProject,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/diff-stats",
        FeatureKind::DiffStats,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/tasks/*/diff",
        FeatureKind::TaskDiff,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/tasks/*/files",
        FeatureKind::DiffFile,
        None,
    ),
    route(
        Method::PUT,
        "/api/mobile/tasks/*/files",
        FeatureKind::SaveDiffFile,
        Some(6 * 1024 * 1024 + 1024),
    ),
    route(
        Method::PATCH,
        "/api/mobile/tasks/*/tabs/*",
        FeatureKind::RenameTab,
        DEFAULT,
    ),
    route(
        Method::POST,
        "/api/mobile/tasks/*/tabs/*/move",
        FeatureKind::MoveTab,
        DEFAULT,
    ),
    route(
        Method::POST,
        "/api/mobile/tasks/*/panes/*/image",
        FeatureKind::PaneImage,
        Some(3 * 1024 * 1024),
    ),
    route(
        Method::GET,
        "/api/mobile/pane-layouts",
        FeatureKind::PaneLayouts,
        None,
    ),
    route(
        Method::PUT,
        "/api/mobile/pane-layouts",
        FeatureKind::SavePaneLayout,
        DEFAULT,
    ),
    route(
        Method::DELETE,
        "/api/mobile/pane-layouts/*",
        FeatureKind::DeletePaneLayout,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/settings",
        FeatureKind::Settings,
        None,
    ),
    route(
        Method::PATCH,
        "/api/mobile/settings",
        FeatureKind::UpdateSettings,
        DEFAULT,
    ),
    route(
        Method::GET,
        "/api/mobile/sessions/change-targets",
        FeatureKind::ChangeTargets,
        None,
    ),
    route(
        Method::POST,
        "/api/mobile/sessions/change-all",
        FeatureKind::ChangeAll,
        DEFAULT,
    ),
    route(
        Method::POST,
        "/api/mobile/sync/run",
        FeatureKind::RunSync,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/sync/status",
        FeatureKind::SyncStatus,
        None,
    ),
    route(
        Method::POST,
        "/api/mobile/sync/start",
        FeatureKind::StartSync,
        None,
    ),
    route(
        Method::GET,
        "/api/mobile/notifications/*/review",
        FeatureKind::SyncReview,
        None,
    ),
    route(
        Method::POST,
        "/api/mobile/notifications/*/review/delete",
        FeatureKind::DeleteSyncReview,
        DEFAULT,
    ),
    route(
        Method::POST,
        "/api/mobile/tasks/*/notifications/read",
        FeatureKind::TaskNotificationsRead,
        None,
    ),
];

pub async fn handle_feature_request(mobile: &mut MobileRequest<'_>) -> RouteResult {
    let matched = FEATURE_ROUTES.iter().find_map(|candidate| {
        let is_method = mobile.method_is(&candidate.method);
        is_method
            .then(|| mobile.url.capture(candidate.pattern))
            .flatten()
            .map(|ids| (candidate, ids))
    });
    let Some((route, raw_ids)) = matched else {
        if let Some(response) = diagram::handle_diagram_request(mobile).await? {
            return Ok(Some(response));
        }
        return ai::handle_ai_request(mobile).await;
    };
    let outcome = run_feature_route(mobile, route, &raw_ids).await;
    Ok(Some(match outcome {
        Ok(response) => response,
        Err(RouteError::Abort) => return Err(RouteError::Abort),
        Err(RouteError::Bridge(error)) => feature_failure_response(&error),
        Err(RouteError::Unexpected) => failure_response(500, SERVER_ERROR),
    }))
}

async fn run_feature_route(
    mobile: &mut MobileRequest<'_>,
    route: &FeatureRoute,
    raw_ids: &[String],
) -> Result<HttpResponse, RouteError> {
    let ids = decode_all(raw_ids).map_err(|_| invalid("잘못된 식별자입니다"))?;
    let body = match route.body_limit {
        Some(limit) => mobile
            .read_json(limit)
            .await?
            .ok_or_else(|| invalid("잘못된 요청입니다"))?,
        None => Value::Null,
    };
    let call = build_feature_call(route.kind, ids, &mobile.url, body)?;
    let result = mobile
        .bridge(move |bridge| bridge.run_mobile_feature(call))
        .await?;
    Ok(json_response(200, &spread_success(Value::Object(result))))
}

fn invalid(message: &str) -> RouteError {
    RouteError::Bridge(BridgeError::Feature {
        status: 400,
        message: message.to_string(),
    })
}

fn feature_failure_response(error: &BridgeError) -> HttpResponse {
    match error {
        BridgeError::Feature { status, message } if matches!(status, 400 | 404 | 409) => {
            failure_response(*status, message)
        }
        _ => failure_response(500, SERVER_ERROR),
    }
}

/// TS `pageInteger`
fn page_integer(
    value: Option<&str>,
    fallback: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, RouteError> {
    let Some(text) = value else {
        return Ok(fallback);
    };
    safe_integer(text)
        .filter(|number| (minimum..=maximum).contains(number))
        .ok_or_else(|| invalid("페이지 값이 올바르지 않습니다"))
}

fn build_feature_call(
    kind: FeatureKind,
    ids: Vec<String>,
    url: &MobileUrl,
    body: Value,
) -> Result<FeatureCall, RouteError> {
    let mut ids = ids.into_iter();
    let mut next_id = move || ids.next().unwrap_or_default();
    let param = |name: &str| url.param(name).map(str::to_string);
    Ok(match kind {
        FeatureKind::SearchTasks => FeatureCall::SearchTasks,
        FeatureKind::DoneTasks => FeatureCall::DoneTasks {
            offset: page_integer(url.param("offset"), 0, 0, 9_007_199_254_740_991)?,
            limit: page_integer(url.param("limit"), 20, 1, 100)?,
        },
        FeatureKind::TaskDetails => FeatureCall::UpdateTaskDetails {
            task_id: next_id(),
            body,
        },
        FeatureKind::BranchTask => FeatureCall::BranchTask {
            task_id: next_id(),
            body,
        },
        FeatureKind::RefreshPr => FeatureCall::RefreshTaskPr { task_id: next_id() },
        FeatureKind::Hosts => FeatureCall::Hosts,
        FeatureKind::ProjectColor => FeatureCall::UpdateProjectColor {
            project_id: next_id(),
            body,
        },
        FeatureKind::ProjectTasks => FeatureCall::ProjectTasks {
            project_id: next_id(),
        },
        FeatureKind::ProjectBranchTask => FeatureCall::ProjectBranchTask {
            project_id: next_id(),
            branch: param("branch"),
        },
        FeatureKind::DeleteProject => FeatureCall::DeleteProject {
            project_id: next_id(),
        },
        FeatureKind::DiffStats => FeatureCall::DiffStats,
        FeatureKind::TaskDiff => FeatureCall::TaskDiff { task_id: next_id() },
        FeatureKind::DiffFile => FeatureCall::TaskDiffFile {
            task_id: next_id(),
            path: param("path"),
        },
        FeatureKind::SaveDiffFile => FeatureCall::SaveTaskDiffFile {
            task_id: next_id(),
            path: param("path"),
            body,
        },
        FeatureKind::RenameTab => FeatureCall::RenameTab {
            task_id: next_id(),
            tab_id: next_id(),
            body,
        },
        FeatureKind::MoveTab => FeatureCall::MoveTab {
            task_id: next_id(),
            tab_id: next_id(),
            body,
        },
        FeatureKind::PaneImage => FeatureCall::PastePaneImage {
            task_id: next_id(),
            pane_id: next_id(),
            body,
        },
        FeatureKind::PaneLayouts => FeatureCall::PaneLayouts,
        FeatureKind::SavePaneLayout => FeatureCall::SavePaneLayout { body },
        FeatureKind::DeletePaneLayout => FeatureCall::DeletePaneLayout {
            layout_id: next_id(),
        },
        FeatureKind::Settings => FeatureCall::Settings,
        FeatureKind::UpdateSettings => FeatureCall::UpdateSettings { body },
        FeatureKind::ChangeTargets => FeatureCall::SessionChangeTargets {
            session_type: param("sessionType"),
        },
        FeatureKind::ChangeAll => FeatureCall::ChangeAllSessions { body },
        FeatureKind::RunSync => FeatureCall::RunSync,
        FeatureKind::SyncStatus => FeatureCall::SyncStatus,
        FeatureKind::StartSync => FeatureCall::StartSync,
        FeatureKind::SyncReview => FeatureCall::SyncReview {
            notification_id: next_id(),
        },
        FeatureKind::DeleteSyncReview => FeatureCall::DeleteSyncReviewTasks {
            notification_id: next_id(),
            body,
        },
        FeatureKind::TaskNotificationsRead => {
            FeatureCall::MarkTaskNotificationsRead { task_id: next_id() }
        }
    })
}
