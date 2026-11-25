//! Herdr 세션 미러 > B 지속 연결 권한

use std::sync::Arc;

use kanvibe_domain::task::{KanbanTask, SessionType};
use kanvibe_terminal::remote_terminal_protocol::{
    RemoteTerminalDirectRoute, RemoteTerminalGrant, RemoteTerminalGrantInput,
};

use super::fakes::*;
use super::*;
use crate::error::{PaneMirrorError, SurfaceUnavailableReason};

fn herdr_task(ssh_host: Option<&str>) -> KanbanTask {
    let mut task = task(
        "herdr-task",
        Some(SessionType::Herdr),
        Some("herdr-session"),
    );
    task.ssh_host = ssh_host.map(str::to_string);
    task
}

fn descriptor() -> RemoteTerminalGrant {
    RemoteTerminalGrant {
        version: 1,
        grant_id: "scoped-grant".to_string(),
        expires_at: 1_900_000_000_000,
        direct: RemoteTerminalDirectRoute {
            host: "b.example".to_string(),
            port: 19443,
            path: "/stream/scoped-grant".to_string(),
            token: "capability".to_string(),
            certificate_fingerprint: "b".repeat(64),
        },
    }
}

fn grant_harness(ssh_host: Option<&str>) -> Arc<Harness> {
    let harness = Harness::new();
    harness.set_task(herdr_task(ssh_host));
    *harness.grants.prepare_result.lock().unwrap() = Some(descriptor());
    Arc::new(harness)
}

fn prepare(
    harness: &Harness,
    pane_id: &str,
    device_id: &str,
) -> crate::error::MirrorResult<Option<RemoteTerminalGrant>> {
    harness
        .mirror
        .prepare_mobile_pane_connection("herdr-task", pane_id, device_id)
}

fn prepare_in_background(
    harness: &Arc<Harness>,
    device_id: &'static str,
) -> std::thread::JoinHandle<crate::error::MirrorResult<Option<RemoteTerminalGrant>>> {
    let harness = Arc::clone(harness);
    std::thread::spawn(move || prepare(&harness, "w1:p1", device_id))
}

fn session_not_running() -> PaneMirrorError {
    PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::SessionNotRunning)
}

// parity: TS-main-mobileBridgeService-ef72c5
#[test]
fn only_a_verified_remote_herdr_pane_gets_a_grant_for_that_device() {
    let harness = grant_harness(Some("remote-b"));

    assert_eq!(
        prepare(&harness, "w1:p1", "device-one").unwrap(),
        Some(descriptor())
    );

    assert_eq!(
        *harness.grants.prepared.lock().unwrap(),
        vec![RemoteTerminalGrantInput {
            ssh_host: "remote-b".to_string(),
            device_id: "device-one".to_string(),
            task_id: "herdr-task".to_string(),
            session_name: "herdr-session".to_string(),
            pane_id: "w1:p1".to_string(),
            cols: Some(120),
            rows: Some(38),
        }]
    );
    assert_eq!(*harness.herdr.size_calls.lock().unwrap(), vec!["w1:p1"]);
    assert!(harness.commands().is_empty());
}

// parity: TS-main-mobileBridgeService-bd237c
#[test]
fn other_panes_and_local_panes_get_no_grant() {
    let harness = grant_harness(None);

    assert_eq!(prepare(&harness, "w1:p1", "device-one").unwrap(), None);
    harness.set_task(herdr_task(Some("remote-b")));
    assert_eq!(
        prepare(&harness, "w9:p9", "device-one").unwrap_err(),
        session_not_running()
    );
    assert!(harness.grants.prepared.lock().unwrap().is_empty());
}

// parity: TS-main-mobileBridgeService-9a1112
#[test]
fn existing_legacy_subscription_prevents_a_second_controller() {
    let harness = grant_harness(None);
    let _subscriber = harness.subscribe("herdr-task", "w1:p1");
    harness.set_task(herdr_task(Some("remote-b")));

    assert_eq!(prepare(&harness, "w1:p1", "device-one").unwrap(), None);
    assert!(harness.grants.prepared.lock().unwrap().is_empty());
}

// parity: TS-main-mobileBridgeService-921240
#[test]
fn grant_arriving_during_a_session_change_is_discarded() {
    let harness = grant_harness(Some("remote-b"));
    let gate = Gate::new();
    *harness.grants.prepare_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let pending = prepare_in_background(&harness, "device-one");
    wait_until("권한 준비가 시작된다", || {
        gate.entered_count() == 1
    });

    harness.board.broadcast_task_session_changed("herdr-task");
    gate.open();

    assert_eq!(pending.join().unwrap().unwrap_err(), session_not_running());
    assert!(
        harness
            .grants
            .revoked_tasks
            .lock()
            .unwrap()
            .contains(&"herdr-task".to_string())
    );
    assert_eq!(
        *harness.grants.revoked_grants.lock().unwrap(),
        vec!["scoped-grant"]
    );
}

// parity: TS-main-mobileBridgeService-bb5910
#[test]
fn unpairing_a_device_also_revokes_its_direct_grants() {
    let harness = grant_harness(Some("remote-b"));

    harness.mirror.handle_mobile_device_unpaired("device-one");

    assert_eq!(
        *harness.grants.revoked_devices.lock().unwrap(),
        vec!["device-one"]
    );
}

// parity: TS-main-mobileBridgeService-0bc3d5
#[test]
fn device_unpaired_while_the_task_is_read_gets_no_new_grant() {
    let harness = grant_harness(Some("remote-b"));
    let gate = Gate::new();
    *harness.tasks.find_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let pending = prepare_in_background(&harness, "device-two");
    wait_until("태스크 조회가 시작된다", || {
        gate.entered_count() == 1
    });

    harness.mirror.handle_mobile_device_unpaired("device-two");
    gate.open();

    assert_eq!(pending.join().unwrap().unwrap_err(), session_not_running());
    assert!(harness.grants.prepared.lock().unwrap().is_empty());
}

// parity: TS-main-mobileBridgeService-75367d
#[test]
fn late_legacy_ownership_revokes_only_that_grant_and_not_the_whole_task() {
    let harness = grant_harness(Some("remote-b"));
    let gate = Gate::new();
    *harness.grants.prepare_gate.lock().unwrap() = Some(Arc::clone(&gate));
    let pending = prepare_in_background(&harness, "device-one");
    wait_until("권한 준비가 시작된다", || {
        gate.entered_count() == 1
    });

    harness.set_task(herdr_task(None));
    let _subscriber = harness.subscribe("herdr-task", "w1:p1");
    gate.open();

    assert_eq!(pending.join().unwrap().unwrap(), None);
    assert_eq!(
        *harness.grants.revoked_grants.lock().unwrap(),
        vec!["scoped-grant"]
    );
    assert!(harness.grants.revoked_tasks.lock().unwrap().is_empty());
}

// parity: TS-main-mobileBridgeService-4f9c6a
#[test]
fn relay_keeps_the_grant_and_device_task_pane_scope_unchanged() {
    let harness = grant_harness(None);

    let relay = harness
        .mirror
        .open_remote_mobile_pane_stream(
            "herdr-task",
            "w1:p1",
            "device-one",
            Some("scoped-grant"),
            Arc::new(|_| {}),
            Arc::new(|_, _| {}),
        )
        .unwrap();

    assert!(relay.is_some());
    assert_eq!(
        *harness.grants.attached.lock().unwrap(),
        vec![(
            "scoped-grant".to_string(),
            "device-one".to_string(),
            "herdr-task".to_string(),
            "w1:p1".to_string(),
        )]
    );
    assert!(harness.commands().is_empty());
}
