//! Herdr CLI 응답 해석, 로컬 API 소켓의 `tab.move`, 실행 중 에이전트 수집.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;
use std::time::Duration;

use kanvibe_ai::AiSessionProvider;
use kanvibe_ai::types::RunningAgentPane;
use kanvibe_process::exec::ExecGitError;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::fixtures::{fixture, result, session_list};
use crate::herdr::build_herdr_command;

/// 요청 한 줄을 읽어 넘기고, 응답 조각들을 차례로 보낸 뒤 연결을 닫는 소켓 서버
fn serve_once(socket_path: &std::path::Path, chunks: Vec<&'static str>) -> mpsc::Receiver<Value> {
    let listener = UnixListener::bind(socket_path).unwrap();
    let (request_sender, requests) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        request_sender
            .send(serde_json::from_str(&line).unwrap())
            .unwrap();
        for chunk in chunks {
            stream.write_all(chunk.as_bytes()).unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    requests
}

fn respond_with_local_session(
    runner: &crate::test_support::ScriptedShellRunner,
    socket_path: &std::path::Path,
) {
    let sessions =
        session_list(json!([{ "name": "feature", "running": true, "socket_path": socket_path }]));
    runner.respond(move |_, _| Some(Ok(sessions.clone())));
}

#[test]
fn cli_api_errors_are_not_mistaken_for_an_empty_list() {
    let herdr = fixture();
    herdr.runner.respond(|_, _| {
        Some(Ok(
            r#"{"error":{"code":"not_found","message":"missing session"}}"#.to_string(),
        ))
    });

    let error = herdr.service.read_herdr_tabs("feature", None).unwrap_err();

    assert_eq!(error.to_string(), "Herdr: missing session");
}

#[test]
fn tab_list_polling_asks_not_to_log_expected_failures() {
    let herdr = fixture();
    herdr
        .runner
        .respond(|_, _| Some(Ok(r#"{"result":{"tabs":[]}}"#.to_string())));

    herdr
        .service
        .read_herdr_tabs("feature", Some("remote"))
        .unwrap();

    let calls = herdr.runner.calls();
    assert_eq!(
        calls[0].command,
        build_herdr_command("feature", &["tab", "list"]).unwrap()
    );
    assert_eq!(calls[0].ssh_host.as_deref(), Some("remote"));
    assert!(calls[0].options.is_failure_expected);
}

#[test]
fn sends_one_exact_tab_move_request_even_when_the_reply_is_fragmented() {
    let herdr = fixture();
    let directory = TempDir::new().unwrap();
    let socket_path = directory.path().join("api");
    let requests = serve_once(
        &socket_path,
        vec![r#"{"id":"kanvibe","res"#, "ult\":{\"type\":\"ok\"}}\n"],
    );
    respond_with_local_session(&herdr.runner, &socket_path);

    herdr
        .service
        .move_herdr_tab("feature", "w1:t1", 3, None)
        .unwrap();

    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        request,
        json!({ "id": "kanvibe", "method": "tab.move", "params": { "tab_id": "w1:t1", "insert_index": 3 } })
    );
    assert!(requests.try_recv().is_err());
}

#[test]
fn socket_error_replies_reach_the_caller() {
    let herdr = fixture();
    let directory = TempDir::new().unwrap();
    let socket_path = directory.path().join("api");
    let _requests = serve_once(
        &socket_path,
        vec![
            "{\"id\":\"kanvibe\",\"error\":{\"code\":\"not_found\",\"message\":\"tab missing\"}}\n",
        ],
    );
    respond_with_local_session(&herdr.runner, &socket_path);

    let error = herdr
        .service
        .move_herdr_tab("feature", "w1:t1", 3, None)
        .unwrap_err();

    assert!(error.to_string().contains("tab missing"));
}

#[test]
fn never_sends_socket_requests_to_a_stopped_session() {
    let herdr = fixture();
    let sessions =
        session_list(json!([{ "name": "feature", "running": false, "socket_path": "/missing" }]));
    herdr.runner.respond(move |_, _| Some(Ok(sessions.clone())));

    let error = herdr
        .service
        .move_herdr_tab("feature", "w1:t1", 0, None)
        .unwrap_err();

    assert!(error.to_string().contains("세션을 찾을 수 없습니다"));
}

#[test]
fn collects_running_agents_even_when_another_session_fails() {
    let herdr = fixture();
    herdr.runner.respond(|command, _| {
        if command.contains("'session' 'list'") {
            return Some(Ok(session_list(json!([{ "name": "feature", "running": true }, { "name": "broken", "running": true }]))));
        }
        if command.contains("--session 'broken'") {
            return Some(Err(ExecGitError::new("offline")));
        }
        if command.contains("'agent' 'list'") {
            let agents = json!([
                { "agent": "codex", "tab_id": "w1:t1", "cwd": "/old", "foreground_cwd": "/work" },
                { "agent": null, "cwd": "/work" },
            ]);
            return Some(Ok(result(json!({ "agents": agents }))));
        }
        Some(Ok(result(json!({ "tabs": [{ "tab_id": "w1:t1", "label": "coding", "focused": true }] }))))
    });

    let panes = herdr.service.list_herdr_running_agent_panes(None);

    assert_eq!(
        panes,
        vec![RunningAgentPane {
            provider: AiSessionProvider::Codex,
            worktree_path: "/work".into(),
            session_name: "feature".into(),
            window_id: "w1:t1".into(),
            window_name: "coding".into(),
        }]
    );
}
