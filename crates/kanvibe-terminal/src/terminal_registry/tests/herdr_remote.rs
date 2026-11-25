//! TS `terminalSessionLifecycle.test.ts` "Herdr 원격 machine 연결", "Herdr PTY 수명"

use std::sync::Arc;

use kanvibe_domain::task::SessionType;
use kanvibe_domain::terminal_tabs::HERDR_SERVER_REPLACED_CLOSE_REASON;

use super::fakes::*;
use crate::terminal_registry::{HerdrSessionError, RemoteAttachRequest};

fn herdr_remote_request(task_id: &str) -> RemoteAttachRequest {
    let mut request = remote_request(
        task_id,
        None,
        "host",
        SessionType::Herdr,
        "project-task",
        Some("/work"),
    );
    request.ssh_config = ssh_config("host", "host", 22, "user", "/key");
    request
}

fn herdr_target(is_remote: bool) -> AttachTarget {
    match is_remote {
        true => AttachTarget::Remote(herdr_remote_request("herdr-task")),
        false => AttachTarget::Local(local_request(
            "herdr-task",
            None,
            SessionType::Herdr,
            "project-task",
            Some("/work"),
        )),
    }
}

fn command_failure(message: &str, stderr: &str) -> HerdrSessionError {
    HerdrSessionError::Command {
        message: message.to_string(),
        stderr: stderr.to_string(),
    }
}

// parity: TS-lib-terminalSessionLifecycle-3bc991
#[test]
fn remote_herdr_attach_connects_ssh_while_machine_registration_runs_in_background() {
    // Given: machine 등록이 끝나지 않고 붙잡혀 있다
    let harness = Harness::new();
    let registration_gate = harness.herdr.remote_machine_gate.install();

    // When
    harness
        .attach_remote(herdr_remote_request("machine-task"), &FakeClient::new())
        .unwrap();
    wait_until("machine 등록이 시작된다", || {
        registration_gate.entered_count() == 1
    });

    // Then
    assert_eq!(harness.ptys.count(), 1);
    let spawn_request = harness.ptys.request(0);
    assert_eq!(spawn_request.program, "ssh");
    assert!(spawn_request.args.last().unwrap().contains("--session"));
    assert_eq!(
        harness.herdr.remote_machine_calls(),
        vec![("project-task".to_string(), "host".to_string())]
    );
    let ensure_call = harness.herdr.ensure_session_calls.lock().unwrap()[0].clone();
    assert_eq!(ensure_call.0, "project-task");
    assert_eq!(ensure_call.1, "/work");
    assert_eq!(ensure_call.3.as_deref(), Some("host"));
    registration_gate.open();
}

// parity: TS-lib-terminalSessionLifecycle-b641f1
#[test]
fn failed_machine_registration_keeps_the_remote_pty_and_warns() {
    // Given: `machine list`와 `machine add` 어느 쪽이 실패해도 같은 실패로 올라온다
    for failed_command in ["list", "add"] {
        let harness = Harness::new();
        harness
            .herdr
            .remote_machine_results
            .lock()
            .unwrap()
            .push_back(Err(command_failure("machine unavailable", "")));
        let client = FakeClient::new();

        // When
        harness
            .attach_remote(herdr_remote_request("machine-task"), &client)
            .unwrap();
        wait_until("등록 실패가 알려진다", || {
            !harness.events.registration_failed().is_empty()
        });

        // Then
        assert_eq!(
            harness.events.registration_failed(),
            vec![(
                "host".to_string(),
                "project-task".to_string(),
                "machine unavailable".to_string()
            )],
            "{failed_command}"
        );
        assert_eq!(harness.ptys.request(0).program, "ssh");
        assert_eq!(harness.registry.active_terminal_count(), 1);
        assert!(client.closures().is_empty());
    }
}

// parity: TS-lib-terminalSessionLifecycle-571be0
#[test]
fn rejected_machine_registration_reports_the_last_stderr_line() {
    // Given: Herdr CLI는 안내 문구 뒤 마지막 줄에 거부 이유를 쓰고 실패한다
    let harness = Harness::new();
    let stderr = "remote herdr server on host (session project-task) is still running v0.9.5.\nerror: remote server is not ready for saved machines; machine was not saved\n";
    harness
        .herdr
        .remote_machine_results
        .lock()
        .unwrap()
        .push_back(Err(command_failure(
            "Command failed: herdr machine add",
            stderr,
        )));

    // When
    harness
        .attach_remote(herdr_remote_request("machine-task"), &FakeClient::new())
        .unwrap();
    wait_until("등록 실패가 알려진다", || {
        !harness.events.registration_failed().is_empty()
    });

    // Then
    assert_eq!(
        harness.events.registration_failed(),
        vec![(
            "host".to_string(),
            "project-task".to_string(),
            "error: remote server is not ready for saved machines; machine was not saved"
                .to_string()
        )]
    );
}

// parity: TS-lib-terminalSessionLifecycle-c63e32
#[test]
fn legacy_remote_server_asks_for_replacement_instead_of_registering() {
    // Given: 세션 리더가 아니지만 live handoff를 지원하는 원격 서버
    let harness = Harness::new();
    harness
        .herdr
        .remote_machine_results
        .lock()
        .unwrap()
        .push_back(Err(HerdrSessionError::ServerHandoffRequired {
            message: "교체 필요".to_string(),
        }));

    // When
    harness
        .attach_remote(herdr_remote_request("machine-task"), &FakeClient::new())
        .unwrap();
    wait_until("교체 확인이 요청된다", || {
        !harness.events.handoff_required().is_empty()
    });

    // Then
    assert_eq!(
        harness.events.handoff_required(),
        vec![("host".to_string(), "project-task".to_string())]
    );
    assert!(harness.events.registration_failed().is_empty());
}

fn is_waiting_for_handoff(harness: &Harness) -> bool {
    let state = harness.registry.lock_state();
    let mut handoffs = state.herdr_server_handoffs.values();
    handoffs.any(|handoff| handoff.waiting_count() > 0)
}

// parity: TS-lib-terminalSessionLifecycle-4ef4cf
#[test]
fn approved_replacement_closes_attached_screens_and_holds_new_ones_until_done() {
    // Given: 이전 방식 서버에 붙은 원격 Herdr 화면
    let harness = Harness::new();
    let attached = FakeClient::new();
    harness
        .attach_remote(herdr_remote_request("machine-task"), &attached)
        .unwrap();
    wait_until("첫 machine 등록이 끝난다", || {
        harness.herdr.remote_machine_calls().len() == 1
    });
    let handoff_gate = harness.herdr.handoff_gate.install();

    // When: 교체를 시작하고, 끝나기 전에 같은 서버로 새 화면을 연다
    let registry = Arc::clone(&harness.registry);
    let replacing =
        std::thread::spawn(move || registry.replace_herdr_remote_server("host", "project-task"));
    wait_until("교체가 시작된다", || {
        handoff_gate.entered_count() == 1
    });
    let reconnecting = harness.attach_in_background(
        AttachTarget::Remote(herdr_remote_request("other-task")),
        &FakeClient::new(),
    );
    wait_until("새 화면이 교체를 기다린다", || {
        is_waiting_for_handoff(&harness)
    });
    let spawn_count_during_handoff = harness.ptys.count();
    handoff_gate.open();
    replacing.join().unwrap();
    reconnecting.join().unwrap().unwrap();

    // Then: 이전 화면은 재연결 사유로 닫히고, 새 화면은 교체가 끝난 뒤에야 PTY를 얻고, machine 등록이 이어진다
    assert_eq!(
        attached.closures(),
        vec![closure(1012, HERDR_SERVER_REPLACED_CLOSE_REASON)]
    );
    assert_eq!(harness.ptys.pty(0).kill_count(), 1);
    assert_eq!(spawn_count_during_handoff, 1);
    assert_eq!(harness.ptys.count(), 2);
    assert_eq!(
        harness.herdr.handoff_calls.lock().unwrap().clone(),
        vec![("project-task".to_string(), "host".to_string())]
    );
    wait_until("교체 뒤 machine 등록이 이어진다", || {
        harness.herdr.remote_machine_calls().len() >= 2
    });
}

// parity: TS-lib-terminalSessionLifecycle-bc26b7
#[test]
fn failed_replacement_reports_the_herdr_reason_without_registering() {
    // Given
    let harness = Harness::new();
    *harness.herdr.handoff_result.lock().unwrap() = Err(command_failure(
        "Command failed",
        "error: live handoff failed\n",
    ));

    // When
    harness
        .registry
        .replace_herdr_remote_server("host", "project-task");

    // Then
    assert_eq!(
        harness.events.registration_failed(),
        vec![(
            "host".to_string(),
            "project-task".to_string(),
            "error: live handoff failed".to_string()
        )]
    );
    assert!(harness.herdr.remote_machine_calls().is_empty());
}

// parity: TS-lib-terminalSessionLifecycle-47efa4
#[test]
fn local_herdr_attach_never_touches_remote_machines() {
    // Given
    let harness = Harness::new();

    // When
    let request = local_request(
        "local-machine-task",
        None,
        SessionType::Herdr,
        "project-task",
        Some("/work"),
    );
    harness.attach_local(request, &FakeClient::new()).unwrap();

    // Then
    assert_eq!(harness.ptys.count(), 1);
    assert!(harness.herdr.remote_machine_calls().is_empty());
}

// parity: TS-lib-terminalSessionLifecycle-731e62
#[test]
fn client_closed_while_preparing_herdr_gets_no_pty() {
    for is_remote in [false, true] {
        // Given
        let harness = Harness::new();
        let bootstrap_gate = harness.herdr.ensure_session_gate.install();
        let client = FakeClient::new();
        let attaching = harness.attach_in_background(herdr_target(is_remote), &client);
        wait_until("Space 준비가 시작된다", || {
            bootstrap_gate.entered_count() == 1
        });

        // When
        client.set_open(false);
        bootstrap_gate.open();
        let attachment = attaching.join().unwrap();

        // Then
        assert_eq!(attachment, Ok(None), "remote={is_remote}");
        assert_eq!(harness.ptys.count(), 0);
        assert_eq!(harness.registry.active_terminal_count(), 0);
    }
}

// parity: TS-lib-terminalSessionLifecycle-21f0ca
#[test]
fn cancelled_client_during_concurrent_preparation_is_not_kept() {
    for (is_remote, is_cancelled_first) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        // Given
        let harness = Harness::new();
        let bootstrap_gate = harness.herdr.ensure_session_gate.install();
        let cancelled = FakeClient::new();
        let live = FakeClient::new();
        let (first, second) = if is_cancelled_first {
            (&cancelled, &live)
        } else {
            (&live, &cancelled)
        };
        let first_attach = harness.attach_in_background(herdr_target(is_remote), first);
        wait_until("Space 준비가 시작된다", || {
            bootstrap_gate.entered_count() == 1
        });
        let second_attach = harness.attach_in_background(herdr_target(is_remote), second);
        wait_until("두 번째 attach가 기다린다", || {
            harness
                .registry
                .lock_state()
                .pending_creations
                .values()
                .any(|creation| creation.waiting_count() > 0)
        });

        // When
        cancelled.set_open(false);
        bootstrap_gate.open();
        let results = [first_attach.join().unwrap(), second_attach.join().unwrap()];

        // Then
        let case = format!("remote={is_remote}, cancelledFirst={is_cancelled_first}");
        assert!(results.iter().all(Result::is_ok), "{case}");
        assert_eq!(harness.ptys.count(), 1, "{case}");
        assert_eq!(harness.registry.active_terminal_count(), 1, "{case}");
        assert_eq!(harness.ptys.pty(0).kill_count(), 0, "{case}");
        let live_attachment = results
            .into_iter()
            .flatten()
            .flatten()
            .next()
            .expect("살아 있는 창은 붙는다");
        harness.registry.client_closed(&live_attachment);
        assert_eq!(harness.ptys.pty(0).kill_count(), 1, "{case}");
        assert_eq!(harness.registry.active_terminal_count(), 0, "{case}");
    }
}

// parity: TS-lib-terminalSessionLifecycle-f80637
#[test]
fn concurrent_herdr_attaches_share_one_pty_and_only_the_last_window_detaches() {
    for is_remote in [false, true] {
        // Given
        let harness = Harness::new();
        let bootstrap_gate = harness.herdr.ensure_session_gate.install();
        let first_attach =
            harness.attach_in_background(herdr_target(is_remote), &FakeClient::new());
        wait_until("Space 준비가 시작된다", || {
            bootstrap_gate.entered_count() == 1
        });
        let second_attach =
            harness.attach_in_background(herdr_target(is_remote), &FakeClient::new());
        wait_until("두 번째 attach가 기다린다", || {
            harness
                .registry
                .lock_state()
                .pending_creations
                .values()
                .any(|creation| creation.waiting_count() > 0)
        });

        // When
        bootstrap_gate.open();
        let first = first_attach.join().unwrap().unwrap().unwrap();
        let second = second_attach.join().unwrap().unwrap().unwrap();

        // Then
        assert_eq!(harness.ptys.count(), 1, "remote={is_remote}");
        let launched_command = harness.ptys.request(0).args.join(" ");
        assert!(launched_command.contains("--session"));
        assert!(!launched_command.contains("'session' 'attach'"));
        assert_eq!(harness.registry.active_terminal_count(), 1);
        harness.registry.client_closed(&first);
        assert_eq!(harness.ptys.pty(0).kill_count(), 0);
        harness.registry.client_closed(&second);
        assert_eq!(harness.ptys.pty(0).kill_count(), 1);
        assert_eq!(harness.registry.active_terminal_count(), 0);
    }
}
