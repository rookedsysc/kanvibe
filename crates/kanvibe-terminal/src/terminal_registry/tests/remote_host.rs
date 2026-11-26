//! TS `terminalRemoteHost.test.ts` "ordinary remote terminals consume daemon PTYs"

use kanvibe_domain::task::SessionType;

use super::fakes::*;
use crate::terminal_registry::{RemoteAttachRequest, RemoteHostPtySession};

fn daemon_request(
    tab_id: Option<&str>,
    session_type: SessionType,
    session_name: &str,
    worktree_path: Option<&str>,
) -> RemoteAttachRequest {
    let mut request = remote_request(
        "task",
        tab_id,
        "fixture",
        session_type,
        session_name,
        worktree_path,
    );
    request.ssh_config = ssh_config("fixture", "fixture", 22, "fixture", "/unused");
    request.columns = None;
    request.rows = None;
    request
}

fn session(key: serde_json::Value, session_id: &str) -> RemoteHostPtySession {
    RemoteHostPtySession {
        key: key.to_string(),
        session_id: session_id.to_string(),
    }
}

fn tab_ids(harness: &Harness) -> Vec<String> {
    harness
        .registry
        .list_local_terminal_tabs("task")
        .into_iter()
        .map(|tab| tab.id)
        .collect()
}

#[test]
fn failed_daemon_close_acknowledgement_keeps_the_tab() {
    // Given
    let harness = Harness::new();
    let connection = harness.with_daemon_connection();
    let remote = connection.queue_pty();
    let tab_id = harness.registry.list_local_terminal_tabs("task")[0]
        .id
        .clone();
    harness
        .attach_remote(
            daemon_request(Some(&tab_id), SessionType::Terminal, "shell", None),
            &FakeClient::new(),
        )
        .unwrap();
    *connection.close_failure.lock().unwrap() = Some("transport write failed".to_string());

    // When
    let result = harness
        .registry
        .close_remote_terminal_tab("task", &tab_id, "fixture", "shell");

    // Then
    assert_eq!(result, Err("transport write failed".to_string()));
    assert!(tab_ids(&harness).contains(&tab_id));
    assert_eq!(remote.kill_count(), 0);
    harness.registry.kill_all_terminal_sessions();
}

#[test]
fn daemon_connection_errors_fail_the_close_and_keep_the_tab_instead_of_closing_only_locally() {
    // Given
    let harness = Harness::new();
    let tab_id = harness.registry.list_local_terminal_tabs("task")[0]
        .id
        .clone();
    *harness.remote_hosts.connection_error.lock().unwrap() =
        Some("Host preparation cancelled".to_string());

    // When
    let closing = harness
        .registry
        .close_remote_terminal_tab("task", &tab_id, "fixture", "shell");

    // Then
    assert_eq!(closing, Err("Host preparation cancelled".to_string()));
    assert!(tab_ids(&harness).contains(&tab_id));
}

#[test]
fn daemon_connection_errors_fail_remote_tab_restoration() {
    let harness = Harness::new();
    *harness.remote_hosts.connection_error.lock().unwrap() =
        Some("SSH 호스트를 찾을 수 없습니다: fixture".to_string());

    let restored = harness
        .registry
        .list_remote_terminal_tabs("task", "fixture", "shell");

    assert_eq!(
        restored,
        Err("SSH 호스트를 찾을 수 없습니다: fixture".to_string())
    );
}

/// daemon 세션 목록으로 되살린 탭 ID
fn restored_tab_ids(harness: &Harness) -> Vec<String> {
    let restored = harness
        .registry
        .list_remote_terminal_tabs("task", "fixture", "shell");
    restored.unwrap().into_iter().map(|tab| tab.id).collect()
}

fn terminal_session(terminal_key: &str, session_id: &str) -> RemoteHostPtySession {
    session(
        serde_json::json!([terminal_key, "terminal", "shell"]),
        session_id,
    )
}

#[test]
fn daemon_owned_tabs_are_restored_after_app_detach_and_closed_by_session_id() {
    // Given
    let harness = Harness::new();
    let connection = harness.with_daemon_connection();
    *connection.sessions.lock().unwrap() = vec![
        terminal_session("task#task-1", "first"),
        terminal_session("task#task-3", "third"),
        terminal_session("other#other-1", "other"),
    ];

    // When
    let restored = restored_tab_ids(&harness);

    // Then
    assert_eq!(restored, vec!["task-1", "task-3"]);
    assert_eq!(
        harness.registry.create_local_terminal_tab("task").id,
        "task-4"
    );
    let closing = harness
        .registry
        .close_remote_terminal_tab("task", "task-3", "fixture", "shell");
    assert_eq!(closing, Ok(2));
    assert_eq!(connection.closed_session_ids(), vec!["third"]);
    assert_eq!(tab_ids(&harness), vec!["task-1", "task-4"]);
    harness.registry.kill_all_terminal_sessions();
    assert_eq!(restored_tab_ids(&harness), vec!["task-1", "task-3"]);
}

#[test]
fn one_daemon_attachment_is_shared_kept_on_detach_and_closed_only_explicitly() {
    // Given
    let harness = Harness::new();
    let connection = harness.with_daemon_connection();
    let remote = connection.queue_pty();
    let tab_id = harness.registry.list_local_terminal_tabs("task")[0]
        .id
        .clone();
    let request = daemon_request(Some(&tab_id), SessionType::Terminal, "shell", Some("/work"));
    let first = harness
        .attach_remote(request.clone(), &FakeClient::new())
        .unwrap()
        .unwrap();
    let second = harness
        .attach_remote(request, &FakeClient::new())
        .unwrap()
        .unwrap();
    assert_eq!(connection.open_pty_calls().len(), 1);
    assert_eq!(harness.ptys.count(), 0);
    let pty_key = connection.open_pty_calls()[0].0.clone();
    assert_eq!(
        pty_key,
        serde_json::json!([format!("task#{tab_id}"), "terminal", "shell"]).to_string()
    );

    // When: 창이 모두 닫혀도 daemon 세션은 남고, 앱 종료는 연결만 끊는다
    harness.registry.client_closed(&first);
    harness.registry.client_closed(&second);
    assert_eq!(remote.kill_count(), 0);
    harness.registry.kill_all_terminal_sessions();

    // Then
    assert_eq!(remote.kill_count(), 1);
    assert_eq!(remote.close_session_count(), 0);
    let again = connection.queue_pty();
    harness
        .attach_remote(
            daemon_request(Some(&tab_id), SessionType::Terminal, "shell", None),
            &FakeClient::new(),
        )
        .unwrap();
    assert_eq!(connection.open_pty_calls()[1].0, pty_key);
    harness.registry.close_local_terminal_tab("task", &tab_id);
    assert_eq!(again.close_session_count(), 1);
}

#[test]
fn transport_loss_keeps_the_tab_and_stalled_clients_are_closed_by_backpressure() {
    // Given
    let harness = Harness::new();
    let connection = harness.with_daemon_connection();
    let remote = connection.queue_pty();
    let tab_id = harness.registry.list_local_terminal_tabs("task")[0]
        .id
        .clone();
    let client = FakeClient::new();
    harness
        .attach_remote(
            daemon_request(Some(&tab_id), SessionType::Terminal, "shell", None),
            &client,
        )
        .unwrap();

    // When
    client.set_buffered_amount(8 * 1024 * 1024 + 1);
    remote.emit_output("output");
    remote.emit_exit(255, true);

    // Then
    // daemon 연결이 끊긴 PTY는 정리되며 남은 클라이언트도 닫는다. 첫 닫힘이 출력 적체 때문이다
    assert_eq!(
        client.closures()[0],
        closure(1013, "Terminal output backpressure limit")
    );
    assert!(client.sent().is_empty());
    assert!(tab_ids(&harness).contains(&tab_id));
    assert_eq!(remote.close_session_count(), 0);
}

#[test]
fn output_sent_while_the_daemon_pty_opens_reaches_the_first_client() {
    for session_type in [SessionType::Tmux, SessionType::Zellij] {
        // Given
        let harness = Harness::new();
        let connection = harness.with_daemon_connection();
        connection.queue_pty();
        *connection.output_on_open.lock().unwrap() = Some("first-screen".to_string());
        let client = FakeClient::new();

        // When
        let request = daemon_request(None, session_type, "existing-session", Some("/work"));
        harness.attach_remote(request, &client).unwrap().unwrap();

        // Then
        assert_eq!(client.sent(), ["first-screen"]);
    }
}

#[test]
fn existing_multiplexer_attach_commands_run_inside_the_daemon() {
    for session_type in [SessionType::Tmux, SessionType::Zellij] {
        // Given
        let harness = Harness::new();
        let connection = harness.with_daemon_connection();
        let remote = connection.queue_pty();

        // When
        let request = daemon_request(None, session_type, "existing-session", Some("/work"));
        let attachment = harness
            .attach_remote(request, &FakeClient::new())
            .unwrap()
            .unwrap();

        // Then
        let (_, command, columns, rows) = connection.open_pty_calls()[0].clone();
        let expected_attach = if session_type == SessionType::Tmux {
            "tmux attach-session"
        } else {
            "zellij attach"
        };
        assert!(command.contains(expected_attach));
        assert!(command.contains("existing-session"));
        assert_eq!((columns, rows), (120, 30));
        assert_eq!(harness.ptys.count(), 0);
        harness.registry.client_closed(&attachment);
        assert_eq!(remote.kill_count(), 1);
        assert_eq!(remote.close_session_count(), 0);
    }
}
