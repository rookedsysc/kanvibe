//! CLAUDE.md 런타임 환경 규칙: 서버 런타임 값(`PORT`, `HOST`, `NODE_ENV`, `KANVIBE_*`)은 task 터미널 PTY로 새지 않는다

use kanvibe_domain::task::SessionType;

use super::fakes::*;
use crate::pty::PtySpawnRequest;

fn assert_no_server_runtime_variables(request: &PtySpawnRequest) {
    for key in request.environment.keys() {
        let is_server_runtime_key =
            ["PORT", "HOST", "NODE_ENV"].contains(&key.as_str()) || key.starts_with("KANVIBE_");
        assert!(
            !is_server_runtime_key,
            "{key}가 {} PTY 환경에 들어갔다",
            request.program
        );
    }
}

#[test]
fn task_terminal_ptys_never_receive_server_runtime_variables() {
    // Given: 부모 환경에 서버 런타임 값이 있다
    let harness = Harness::with_parent_variables(&[
        ("KANVIBE_DEBUG_TERMINAL", "true"),
        ("KANVIBE_HOST", "0.0.0.0"),
    ]);

    // When: 세션 타입과 로컬·원격 경로마다 PTY를 띄운다
    let local_targets = [
        (
            "task-terminal",
            Some("tab-1"),
            SessionType::Terminal,
            "proj",
        ),
        ("task-tmux", None, SessionType::Tmux, "proj"),
        ("task-zellij", None, SessionType::Zellij, "proj"),
        ("task-herdr", None, SessionType::Herdr, "kanvibe:task-herdr"),
    ];
    for (task_id, tab_id, session_type, session_name) in local_targets {
        let request = local_request(task_id, tab_id, session_type, session_name, Some("/work"));
        harness.attach_local(request, &FakeClient::new()).unwrap();
    }
    let remote = remote_request(
        "task-remote",
        None,
        "remote-host",
        SessionType::Tmux,
        "proj",
        None,
    );
    harness.attach_remote(remote, &FakeClient::new()).unwrap();

    // Then
    assert_eq!(harness.ptys.count(), 5);
    for index in 0..harness.ptys.count() {
        let request = harness.ptys.request(index);
        assert_no_server_runtime_variables(&request);
        assert_eq!(
            request.environment.get("HOME").map(String::as_str),
            Some("/home/local-user")
        );
    }
    harness.registry.kill_all_terminal_sessions();
}
