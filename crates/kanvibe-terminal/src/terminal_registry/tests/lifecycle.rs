//! TS `terminalSessionLifecycle.test.ts`의 viewport 소유권, 세션 타입별 수명, 동시 열기, spawn, 프로세스 조회, attach 경쟁

use std::sync::Arc;
use std::sync::atomic::Ordering;

use kanvibe_domain::task::SessionType;

use super::fakes::*;
use crate::terminal_registry::{TerminalAttachment, build_terminal_key};

fn resize_message(columns: u16, rows: u16) -> String {
    format!(
        "\u{1}{}",
        serde_json::json!({ "type": "resize", "cols": columns, "rows": rows })
    )
}

fn attach_zellij_task(harness: &Harness, client: &Arc<FakeClient>) -> TerminalAttachment {
    let mut request = local_request(
        "zellij-task",
        None,
        SessionType::Zellij,
        "proj-main",
        Some("/work"),
    );
    request.columns = Some(120);
    request.rows = Some(30);
    harness.attach_local(request, client).unwrap().unwrap()
}

fn attach_terminal_tab(
    harness: &Harness,
    task_id: &str,
    tab_id: &str,
    client: &Arc<FakeClient>,
) -> TerminalAttachment {
    let request = local_request(
        task_id,
        Some(tab_id),
        SessionType::Terminal,
        "proj",
        Some("/work"),
    );
    harness.attach_local(request, client).unwrap().unwrap()
}

/// 만들던 쪽이 PTY를 띄우는 동안, 뒤에 온 요청이 그 생성을 기다리기 시작할 때까지 기다린다
fn wait_for_waiting_attach(harness: &Harness, terminal_key: &str) {
    wait_until("두 번째 attach가 생성을 기다린다", || {
        harness
            .registry
            .lock_state()
            .pending_creations
            .get(terminal_key)
            .is_some_and(|creation| creation.waiting_count() > 0)
    });
}

// parity: TS-lib-terminalSessionLifecycle-4ff57f
#[test]
fn mobile_viewport_wins_until_released_then_latest_desktop_size_returns() {
    // Given
    let harness = Harness::new();
    let desktop = FakeClient::new();
    let attachment = attach_zellij_task(&harness, &desktop);
    assert_eq!(
        harness.registry.task_terminal_size("zellij-task"),
        Some((120, 30))
    );

    // When
    harness
        .registry
        .resize_task_terminal_viewport("zellij-task", 88, 73)
        .unwrap();
    harness
        .registry
        .client_message(&attachment, &resize_message(100, 40));

    // Then
    assert_eq!(
        harness.registry.task_terminal_size("zellij-task"),
        Some((88, 73))
    );
    assert_eq!(harness.ptys.pty(0).resizes(), vec![(88, 73)]);
    harness
        .registry
        .release_task_terminal_viewport("zellij-task", 120, 30);
    assert_eq!(
        harness.registry.task_terminal_size("zellij-task"),
        Some((100, 40))
    );
    harness
        .registry
        .client_message(&attachment, &resize_message(110, 45));
    assert_eq!(
        harness.registry.task_terminal_size("zellij-task"),
        Some((110, 45))
    );
}

// parity: TS-lib-terminalSessionLifecycle-376ba3
#[test]
fn released_mobile_viewport_restores_the_original_size_without_desktop_changes() {
    // Given
    let harness = Harness::new();
    attach_zellij_task(&harness, &FakeClient::new());

    // When
    harness
        .registry
        .resize_task_terminal_viewport("zellij-task", 88, 73)
        .unwrap();
    harness
        .registry
        .release_task_terminal_viewport("zellij-task", 120, 30);

    // Then
    assert_eq!(
        harness.registry.task_terminal_size("zellij-task"),
        Some((120, 30))
    );
}

// parity: TS-lib-terminalSessionLifecycle-d22c06
#[test]
fn viewport_calls_leave_other_session_types_and_missing_ptys_alone() {
    // Given
    let harness = Harness::new();
    let request = local_request(
        "tmux-task",
        None,
        SessionType::Tmux,
        "proj-main",
        Some("/work"),
    );
    harness.attach_local(request, &FakeClient::new()).unwrap();

    for task_id in ["tmux-task", "missing-task"] {
        // When / Then
        assert_eq!(harness.registry.task_terminal_size(task_id), None);
        assert!(
            harness
                .registry
                .resize_task_terminal_viewport(task_id, 88, 73)
                .is_err()
        );
        harness
            .registry
            .release_task_terminal_viewport(task_id, 120, 30);
    }
    assert!(harness.ptys.pty(0).resizes().is_empty());
}

// parity: TS-lib-terminalSessionLifecycle-f057c2
#[test]
fn closed_zellij_pty_is_not_recreated_by_viewport_release() {
    // Given
    let harness = Harness::new();
    let mobile = FakeClient::new();
    let attachment = attach_zellij_task(&harness, &mobile);
    harness
        .registry
        .resize_task_terminal_viewport("zellij-task", 88, 73)
        .unwrap();
    harness.registry.client_closed(&attachment);

    // When
    harness
        .registry
        .release_task_terminal_viewport("zellij-task", 120, 30);

    // Then
    assert_eq!(harness.registry.task_terminal_size("zellij-task"), None);
    assert_eq!(harness.ptys.count(), 1);
    assert_eq!(harness.ptys.pty(0).resizes(), vec![(88, 73)]);
}

// parity: TS-lib-terminalSessionLifecycle-3068af
#[test]
fn terminal_session_survives_its_last_client_leaving() {
    // Given
    let harness = Harness::new();
    let attachment = attach_terminal_tab(&harness, "task-1", "task-1-1", &FakeClient::new());

    // When
    harness.registry.client_closed(&attachment);

    // Then
    assert_eq!(harness.ptys.pty(0).kill_count(), 0);
    assert_eq!(harness.registry.active_terminal_count(), 1);
}

// parity: TS-lib-terminalSessionLifecycle-c7ed6f
#[test]
fn tmux_pty_is_detached_when_its_last_client_leaves() {
    // Given
    let harness = Harness::new();
    let request = local_request(
        "task-1",
        None,
        SessionType::Tmux,
        "proj-main",
        Some("/work"),
    );
    let attachment = harness
        .attach_local(request, &FakeClient::new())
        .unwrap()
        .unwrap();

    // When
    harness.registry.client_closed(&attachment);

    // Then
    assert_eq!(harness.ptys.pty(0).kill_count(), 1);
    assert_eq!(harness.registry.active_terminal_count(), 0);
}

// parity: TS-lib-terminalSessionLifecycle-338537
#[test]
fn tmux_pty_stays_while_another_client_remains() {
    // Given
    let harness = Harness::new();
    let request = local_request(
        "task-1",
        None,
        SessionType::Tmux,
        "proj-main",
        Some("/work"),
    );
    let first = harness
        .attach_local(request.clone(), &FakeClient::new())
        .unwrap()
        .unwrap();
    harness
        .attach_local(request, &FakeClient::new())
        .unwrap()
        .unwrap();

    // When
    harness.registry.client_closed(&first);

    // Then
    assert_eq!(harness.ptys.count(), 1);
    assert_eq!(harness.ptys.pty(0).kill_count(), 0);
}

// parity: TS-lib-terminalSessionLifecycle-4ae2fd
#[test]
fn app_quit_kills_every_live_terminal_pty() {
    // Given
    let harness = Harness::new();
    attach_terminal_tab(&harness, "task-1", "task-1-1", &FakeClient::new());
    attach_terminal_tab(&harness, "task-1", "task-1-2", &FakeClient::new());

    // When
    harness.registry.kill_all_terminal_sessions();

    // Then
    assert_eq!(harness.ptys.count(), 2);
    assert!((0..2).all(|index| harness.ptys.pty(index).kill_count() > 0));
    assert_eq!(harness.registry.active_terminal_count(), 0);
}

// parity: TS-lib-terminalSessionLifecycle-2af573
#[test]
fn task_cleanup_kills_only_that_tasks_tab_ptys() {
    // Given
    let harness = Harness::new();
    attach_terminal_tab(&harness, "task-1", "task-1-1", &FakeClient::new());
    attach_terminal_tab(&harness, "task-1", "task-1-2", &FakeClient::new());
    attach_terminal_tab(&harness, "task-2", "task-2-1", &FakeClient::new());

    // When
    harness
        .registry
        .detach_session("task-1", Some("cleanup-task-resources"));

    // Then
    assert_eq!(harness.ptys.pty(0).kill_count(), 1);
    assert_eq!(harness.ptys.pty(1).kill_count(), 1);
    assert_eq!(harness.ptys.pty(2).kill_count(), 0);
    assert_eq!(harness.registry.active_terminal_count(), 1);
}

// parity: TS-lib-terminalSessionLifecycle-902fb7
#[test]
fn simultaneous_local_opens_share_one_pty_and_both_receive_output() {
    // Given
    let harness = Harness::new();
    let gate = harness.ptys.spawn_gate.install();
    let first = FakeClient::new();
    let second = FakeClient::new();
    let request = local_request(
        "task-1",
        Some("task-1-1"),
        SessionType::Terminal,
        "proj",
        Some("/work"),
    );
    let first_attach = harness.attach_in_background(AttachTarget::Local(request.clone()), &first);
    wait_until("첫 attach가 PTY를 띄우는 중이다", || {
        gate.entered_count() == 1
    });
    let second_attach = harness.attach_in_background(AttachTarget::Local(request), &second);
    wait_for_waiting_attach(&harness, "task-1#task-1-1");

    // When
    gate.open();
    first_attach.join().unwrap().unwrap();
    second_attach.join().unwrap().unwrap();
    harness.ptys.pty(0).emit_output("hello");

    // Then
    assert_eq!(harness.ptys.count(), 1);
    assert_eq!(harness.registry.active_terminal_count(), 1);
    assert_eq!(first.sent(), vec!["hello"]);
    assert_eq!(second.sent(), vec!["hello"]);
}

// parity: TS-lib-terminalSessionLifecycle-d58e6b
#[test]
fn simultaneous_remote_opens_share_one_ssh_pty() {
    // Given
    let harness = Harness::new();
    let gate = harness.ptys.spawn_gate.install();
    let first = FakeClient::new();
    let second = FakeClient::new();
    let mut request = remote_request(
        "task-1",
        None,
        "remote-host",
        SessionType::Tmux,
        "proj-main",
        None,
    );
    request.ssh_config = ssh_config("remote-host", "127.0.0.1", 1, "dev", "/key");
    let first_attach = harness.attach_in_background(AttachTarget::Remote(request.clone()), &first);
    wait_until("첫 attach가 PTY를 띄우는 중이다", || {
        gate.entered_count() == 1
    });
    let second_attach = harness.attach_in_background(AttachTarget::Remote(request), &second);
    wait_for_waiting_attach(&harness, "task-1");

    // When
    gate.open();
    first_attach.join().unwrap().unwrap();
    second_attach.join().unwrap().unwrap();
    harness.ptys.pty(0).emit_output("hello");

    // Then
    assert_eq!(harness.ptys.count(), 1);
    assert_eq!(harness.registry.active_terminal_count(), 1);
    assert_eq!(first.sent(), vec!["hello"]);
    assert_eq!(second.sent(), vec!["hello"]);
}

// parity: TS-lib-terminalSessionLifecycle-780a4e
#[test]
fn waiting_client_is_closed_and_rejected_when_the_first_spawn_fails() {
    // Given
    let harness = Harness::new();
    harness.ptys.should_fail_next.store(true, Ordering::SeqCst);
    let gate = harness.ptys.spawn_gate.install();
    let first = FakeClient::new();
    let second = FakeClient::new();
    let request = local_request(
        "task-1",
        Some("task-1-1"),
        SessionType::Terminal,
        "proj",
        Some("/work"),
    );
    let first_attach = harness.attach_in_background(AttachTarget::Local(request.clone()), &first);
    wait_until("첫 attach가 PTY를 띄우는 중이다", || {
        gate.entered_count() == 1
    });
    let second_attach = harness.attach_in_background(AttachTarget::Local(request), &second);
    wait_for_waiting_attach(&harness, "task-1#task-1-1");

    // When
    gate.open();
    let results = [first_attach.join().unwrap(), second_attach.join().unwrap()];

    // Then
    assert!(results.iter().all(Result::is_err));
    assert_eq!(
        first.closures(),
        vec![closure(1011, "터미널 프로세스 생성 실패")]
    );
    assert_eq!(second.closures(), vec![closure(1011, "터미널 연결 실패")]);
    assert_eq!(harness.registry.active_terminal_count(), 0);
}

// parity: TS-lib-terminalSessionLifecycle-6e68be
#[test]
fn reopening_after_a_failed_spawn_spawns_again() {
    // Given
    let harness = Harness::new();
    harness.ptys.should_fail_next.store(true, Ordering::SeqCst);
    let request = local_request(
        "task-1",
        Some("task-1-1"),
        SessionType::Terminal,
        "proj",
        Some("/work"),
    );

    // When
    let failed = harness.attach_local(request.clone(), &FakeClient::new());
    let reopened = harness.attach_local(request, &FakeClient::new());

    // Then
    assert!(failed.is_err());
    assert!(reopened.unwrap().is_some());
    assert_eq!(harness.ptys.count(), 1);
}

// parity: TS-lib-terminalSessionLifecycle-1cef62
#[test]
fn local_terminal_session_starts_the_login_shell_in_the_worktree() {
    // Given
    let harness = Harness::new();
    let request = local_request(
        "task-1",
        Some("task-1-1"),
        SessionType::Terminal,
        "proj",
        Some("/work/tree"),
    );

    // When
    harness.attach_local(request, &FakeClient::new()).unwrap();

    // Then
    let spawn_request = harness.ptys.request(0);
    assert_eq!(
        (
            spawn_request.program.as_str(),
            spawn_request.args.clone(),
            spawn_request.working_directory.as_str()
        ),
        ("/bin/zsh", vec!["-l".to_string()], "/work/tree")
    );
}

// parity: TS-lib-terminalSessionLifecycle-14f345
#[test]
fn remote_terminal_session_enters_the_worktree_then_execs_the_login_shell() {
    // Given
    let harness = Harness::new();
    let mut request = remote_request(
        "task-1",
        Some("task-1-1"),
        "remote-host",
        SessionType::Terminal,
        "proj",
        Some("/remote/worktree"),
    );
    request.ssh_config = ssh_config("remote-host", "10.0.0.1", 22, "dev", "/key");

    // When
    harness.attach_remote(request, &FakeClient::new()).unwrap();

    // Then
    let spawn_request = harness.ptys.request(0);
    let remote_command = extract_remote_shell_payload(spawn_request.args.last().unwrap());
    assert_eq!(spawn_request.program, "ssh");
    assert!(remote_command.contains("cd '/remote/worktree'"));
    assert!(remote_command.contains(r#"exec "${SHELL:-/bin/sh}" -l"#));
    assert!(!remote_command.contains("tmux"));
    assert!(!remote_command.contains("zellij"));
}

// parity: TS-lib-terminalSessionLifecycle-e80639
#[test]
fn tab_process_lookup_reads_the_tab_pty_and_returns_none_for_closed_tabs() {
    // Given
    let harness = Harness::new();
    let first = harness.registry.list_local_terminal_tabs("task-1")[0].clone();
    let second = harness.registry.create_local_terminal_tab("task-1");
    attach_terminal_tab(&harness, "task-1", &first.id, &FakeClient::new());
    attach_terminal_tab(&harness, "task-1", &second.id, &FakeClient::new());
    *harness.ptys.pty(0).process_name.lock().unwrap() = Some("claude".to_string());
    *harness.ptys.pty(1).process_name.lock().unwrap() = Some("node".to_string());

    // When / Then
    assert_eq!(
        harness
            .registry
            .local_terminal_tab_process("task-1", &first.id)
            .as_deref(),
        Some("claude")
    );
    assert_eq!(
        harness
            .registry
            .local_terminal_tab_process("task-1", &second.id)
            .as_deref(),
        Some("node")
    );
    harness
        .registry
        .close_local_terminal_tab("task-1", &first.id);
    assert_eq!(
        harness
            .registry
            .local_terminal_tab_process("task-1", &first.id),
        None
    );
}

// parity: TS-lib-terminalSessionLifecycle-90c40a
#[test]
fn simultaneous_attaches_share_one_pty_locally_and_remotely() {
    for is_remote in [false, true] {
        // Given
        let harness = Harness::new();
        let gate = harness.ptys.spawn_gate.install();
        let target = || match is_remote {
            true => {
                let mut request =
                    remote_request("race", None, "host", SessionType::Tmux, "session", None);
                request.ssh_config = ssh_config("host", "host", 22, "user", "");
                AttachTarget::Remote(request)
            }
            false => AttachTarget::Local(local_request(
                "race",
                None,
                SessionType::Tmux,
                "session",
                None,
            )),
        };
        let first = harness.attach_in_background(target(), &FakeClient::new());
        wait_until("첫 attach가 PTY를 띄우는 중이다", || {
            gate.entered_count() == 1
        });
        let second = harness.attach_in_background(target(), &FakeClient::new());
        wait_for_waiting_attach(&harness, "race");

        // When
        gate.open();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();

        // Then
        assert_eq!(harness.ptys.count(), 1, "remote={is_remote}");
        harness.registry.kill_all_terminal_sessions();
        assert_eq!(harness.ptys.pty(0).kill_count(), 1, "remote={is_remote}");
    }
}

// parity: TS-lib-terminalSessionLifecycle-b86451
#[test]
fn client_closed_during_initialization_gets_no_pty() {
    // Given: tmux 세션 확인 중에 창이 닫힌다
    let harness = Harness::new();
    let client = FakeClient::new();
    let closing_client = client.clone();
    harness.runner.respond(move |command, _| {
        command.contains("has-session").then(|| {
            closing_client.set_open(false);
            Ok(String::new())
        })
    });

    // When
    let attachment = harness.attach_local(
        local_request("cancel", None, SessionType::Tmux, "session", None),
        &client,
    );

    // Then
    assert_eq!(attachment, Ok(None));
    assert_eq!(harness.ptys.count(), 0);
}

// parity: TS-lib-terminalSessionLifecycle-a45a7e
#[test]
fn old_pty_exit_and_old_client_close_are_ignored_after_replacement() {
    // Given
    let harness = Harness::new();
    let request = local_request("replace", None, SessionType::Tmux, "session", None);
    let old_attachment = harness
        .attach_local(request.clone(), &FakeClient::new())
        .unwrap()
        .unwrap();
    harness.registry.client_closed(&old_attachment);
    harness
        .attach_local(request, &FakeClient::new())
        .unwrap()
        .unwrap();

    // When
    harness.ptys.pty(0).emit_exit(0, false);
    harness.registry.client_closed(&old_attachment);

    // Then
    assert_eq!(harness.ptys.pty(1).kill_count(), 0);
    assert_eq!(harness.registry.active_terminal_count(), 1);
    assert_eq!(
        build_terminal_key("replace", None),
        old_attachment.terminal_key
    );
    harness.registry.kill_all_terminal_sessions();
}
