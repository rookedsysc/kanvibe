//! 태스크 pane 조회, 세션이 아직 없는 태스크

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kanvibe_domain::task::SessionType;
use serde_json::json;

use super::fakes::*;
use super::*;
use crate::error::{PaneMirrorError, SurfaceUnavailableReason};
use crate::surfaces::SurfaceQuery;

fn surfaces(harness: &Harness) -> crate::error::MirrorResult<crate::surfaces::TaskSurfaces> {
    harness
        .mirror
        .get_task_surfaces("task-1", &SurfaceQuery::default())
}

fn unavailable(reason: SurfaceUnavailableReason) -> PaneMirrorError {
    PaneMirrorError::SurfaceUnavailable(reason)
}

#[test]
fn panes_are_grouped_by_tab() {
    let harness = Harness::new();
    harness.set_task(tmux_task());
    harness.runner.replace(|_, _| {
        Some(Ok([
            "%19\t@10\tbash\t0\t0\t50\t30\tfirst",
            "%20\t@10\tclaude\t51\t0\t49\t30\tfirst",
            "%21\t@11\tbash\t0\t0\t100\t30\tsecond",
        ]
        .join("\n")))
    });

    let surfaces = surfaces(&harness).unwrap();

    let tabs: Vec<(String, usize)> = surfaces
        .tabs
        .iter()
        .map(|tab| (tab.id.clone(), tab.panes.len()))
        .collect();
    assert_eq!(tabs, vec![("@10".to_string(), 2), ("@11".to_string(), 1)]);
}

#[test]
fn pane_coordinates_are_sent_as_is_so_a_tablet_can_rebuild_the_layout() {
    let harness = Harness::new();
    harness.set_task(tmux_task());
    harness
        .runner
        .replace(|_, _| Some(Ok("%20\t@10\tclaude\t51\t0\t49\t30\tfirst".to_string())));

    let surfaces = surfaces(&harness).unwrap();

    assert_eq!(
        serde_json::to_value(&surfaces.tabs[0].panes[0]).unwrap(),
        json!({ "id": "%20", "tabId": "@10", "tabName": "first", "command": "claude", "left": 51, "top": 0, "width": 49, "height": 30 })
    );
    let serialized = serde_json::to_value(&surfaces).unwrap();
    assert_eq!(serialized["taskId"], "task-1");
    assert_eq!(serialized["sessionType"], "tmux");
    assert_eq!(serialized["canResize"], true);
    assert_eq!(serialized["canRefreshHistory"], false);
    assert_eq!(serialized["canPrepareDirectConnection"], false);
    assert!(serialized.get("geometryTabId").is_none());
}

fn session_less_project_task() -> kanvibe_domain::task::KanbanTask {
    let mut task = task("task-1", None, None);
    task.project_id = Some("p1".to_string());
    task
}

fn stub_mirror_after_connect(harness: &Harness) {
    harness.runner.replace(|command, _| {
        Some(Ok(match command.contains("list-panes") {
            true => MIRROR_PANE_LINE.to_string(),
            false => "kanvibe-task".to_string(),
        }))
    });
}

#[test]
fn concurrent_retries_for_a_session_less_project_task_share_one_connection() {
    let harness = Arc::new(Harness::new());
    let mut task = session_less_project_task();
    task.branch_name = Some("main".to_string());
    harness.set_task(task);
    harness
        .tasks
        .connect_results
        .lock()
        .unwrap()
        .push_back(Some(tmux_task()));
    let gate = Gate::new();
    *harness.tasks.connect_gate.lock().unwrap() = Some(Arc::clone(&gate));
    stub_mirror_after_connect(&harness);

    let requests: Vec<_> = (0..2)
        .map(|_| {
            let harness = Arc::clone(&harness);
            std::thread::spawn(move || surfaces(&harness))
        })
        .collect();
    wait_until("첫 연결이 시작된다", || gate.entered_count() == 1);
    std::thread::sleep(std::time::Duration::from_millis(50));
    gate.open();
    let results: Vec<_> = requests
        .into_iter()
        .map(|request| request.join().unwrap().unwrap())
        .collect();

    assert!(
        results
            .iter()
            .all(|result| result.tabs[0].panes[0].id == "%19")
    );
    assert_eq!(
        harness.tasks.connect_calls(),
        vec![("task-1".to_string(), SessionType::Tmux)]
    );
}

#[test]
fn retry_after_a_failed_connection_releases_the_reservation_and_recovers() {
    let harness = Harness::new();
    let mut task = session_less_project_task();
    task.base_branch = Some("main".to_string());
    task.session_type = Some(SessionType::Zellij);
    harness.set_task(task);
    harness
        .tasks
        .connect_results
        .lock()
        .unwrap()
        .extend([None, Some(tmux_task())]);
    stub_mirror_after_connect(&harness);

    assert_eq!(
        surfaces(&harness).unwrap_err(),
        unavailable(SurfaceUnavailableReason::SessionNotRunning)
    );
    assert_eq!(surfaces(&harness).unwrap().tabs[0].panes[0].id, "%19");
    assert!(
        harness
            .tasks
            .connect_calls()
            .contains(&("task-1".to_string(), SessionType::Zellij))
    );
}

#[test]
fn task_without_a_session_reports_the_reason() {
    let harness = Harness::new();
    harness.set_task(task("task-1", None, None));

    assert_eq!(
        surfaces(&harness).unwrap_err(),
        unavailable(SurfaceUnavailableReason::NoSession)
    );
}

#[test]
fn stopped_session_reports_the_reason_instead_of_an_empty_screen() {
    let harness = Harness::new();
    harness.set_task(tmux_task());

    assert_eq!(
        surfaces(&harness).unwrap_err(),
        unavailable(SurfaceUnavailableReason::SessionNotRunning)
    );
}

#[test]
fn missing_task_is_also_a_reason() {
    let harness = Harness::new();
    harness.tasks.set(None);

    assert_eq!(
        surfaces(&harness).unwrap_err(),
        unavailable(SurfaceUnavailableReason::NoSession)
    );
}

#[test]
fn zellij_task_uses_the_json_listing() {
    let harness = Harness::new();
    harness.set_task(task(
        "task-zj",
        Some(SessionType::Zellij),
        Some("kanvibe-zj"),
    ));
    harness.runner.replace(|command, _| {
        Some(Ok(if command.contains("--version") {
            "zellij 0.44.3".to_string()
        } else if command.contains("list-sessions") {
            "kanvibe-zj [Created 1m ago]".to_string()
        } else {
            json!([{ "id": 1, "is_plugin": false, "tab_id": 0, "tab_name": "Tab #1", "title": "Pane #2",
                "terminal_command": "claude", "pane_x": 40, "pane_y": 0, "pane_rows": 24, "pane_columns": 40 }])
            .to_string()
        }))
    });

    let surfaces = harness
        .mirror
        .get_task_surfaces("task-zj", &SurfaceQuery::default())
        .unwrap();

    assert_eq!(surfaces.tabs[0].panes[0].id, "terminal_1");
    assert!(
        harness
            .commands()
            .iter()
            .any(|command| command.contains("list-panes --json"))
    );
}

/// tmux 세션이 시작되기 전까지는 `has-session`이 실패하고, 시작된 뒤에는 pane 하나를 답한다
fn stub_tmux_session_started_by(harness: &Harness, attach_result: Result<(), String>) {
    let is_started = Arc::new(AtomicBool::new(false));
    harness.set_task(tmux_task());
    let started = Arc::clone(&is_started);
    harness.runner.replace(move |command, _| {
        if command.contains("has-session") {
            return Some(match started.load(Ordering::SeqCst) {
                true => Ok(String::new()),
                false => Err("can't find session".to_string()),
            });
        }
        Some(Ok(MIRROR_PANE_LINE.to_string()))
    });
    harness.attacher.set_override(move |_, _, _| {
        attach_result.clone()?;
        is_started.store(true, Ordering::SeqCst);
        Ok(None)
    });
}

fn startup_client_is_open(harness: &Harness) -> bool {
    harness.attacher.calls.lock().unwrap()[0].2.is_open()
}

#[test]
fn missing_session_is_started_the_desktop_way_and_the_startup_client_detaches() {
    let harness = Harness::new();
    stub_tmux_session_started_by(&harness, Ok(()));

    let surfaces = surfaces(&harness).unwrap();

    assert_eq!(surfaces.tabs[0].panes[0].id, "%19");
    assert_eq!(harness.attacher.call_count(), 1);
    let (task_id, tab_id, ..) = harness.attacher.calls.lock().unwrap()[0].clone();
    assert_eq!((task_id.as_str(), tab_id), ("task-1", None));
    assert!(!startup_client_is_open(&harness));
}

#[test]
fn session_start_failure_reports_a_stopped_session_instead_of_a_server_error() {
    let harness = Harness::new();
    stub_tmux_session_started_by(&harness, Err("tmux 세션 생성에 실패했습니다.".to_string()));

    assert_eq!(
        surfaces(&harness).unwrap_err(),
        unavailable(SurfaceUnavailableReason::SessionNotRunning)
    );
    assert!(
        !harness
            .commands()
            .iter()
            .any(|command| command.contains("list-panes"))
    );
    assert!(!startup_client_is_open(&harness));
}

#[test]
fn unreachable_remote_host_is_reported_separately_without_starting_a_session() {
    let harness = Harness::new();
    let mut task = tmux_task();
    task.ssh_host = Some("devbox".to_string());
    harness.set_task(task);
    harness.runner.replace(|_, _| {
        Some(Err(
            "ssh: connect to host devbox port 22: Operation timed out".to_string(),
        ))
    });

    assert_eq!(
        surfaces(&harness).unwrap_err(),
        unavailable(SurfaceUnavailableReason::RemoteUnreachable)
    );
    assert_eq!(harness.attacher.call_count(), 0);
}

#[test]
fn reachable_remote_host_starts_a_missing_session_the_same_way() {
    let harness = Harness::new();
    stub_tmux_session_started_by(&harness, Ok(()));
    let mut task = tmux_task();
    task.ssh_host = Some("devbox".to_string());
    harness.set_task(task);

    let surfaces = surfaces(&harness).unwrap();

    assert_eq!(surfaces.tabs[0].panes[0].id, "%19");
    assert!(
        harness
            .runner
            .calls()
            .contains(&("true".to_string(), Some("devbox".to_string())))
    );
    assert_eq!(harness.attacher.call_count(), 1);
}

#[test]
fn live_session_is_not_started_again() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    surfaces(&harness).unwrap();

    assert_eq!(harness.attacher.call_count(), 0);
}
