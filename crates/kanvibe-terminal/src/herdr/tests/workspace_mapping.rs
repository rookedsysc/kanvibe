//! Space 연결 파일 읽기의 재사용·실패 처리와, 그 위에서 도는 task pane 조회·생존 확인.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kanvibe_ai::AiSessionProvider;
use kanvibe_process::exec::ExecGitError;
use serde_json::json;

use super::fixtures::{command_for, fixture, install_frozen_clock, result, session_list};
use crate::herdr::{HerdrAgentSession, HerdrTaskPaneTab};

fn is_mapping_read(command: &str) -> bool {
    command.starts_with("test -f ")
}

fn mapping_read_count(runner: &crate::test_support::ScriptedShellRunner) -> usize {
    runner
        .commands()
        .iter()
        .filter(|command| is_mapping_read(command))
        .count()
}

/// 공유 서버 세션 목록과 빈 탭 목록을 돌려주는 원격 서버
fn respond_as_shared_server(runner: &crate::test_support::ScriptedShellRunner) {
    let sessions = session_list(
        json!([{ "name": "kanvibe", "running": true, "session_dir": "/shared", "socket_path": "/shared/herdr.sock" }]),
    );
    runner.respond(move |command, _| {
        Some(Ok(match command.contains("'session' 'list'") {
            true => sessions.clone(),
            false => result(json!({ "tabs": [] })),
        }))
    });
}

/// 정상·대화 ID 불안전·복원 실패·목록 밖 탭 pane을 하나씩 담은 pane 목록과 탭 목록
fn task_pane_lists() -> (serde_json::Value, serde_json::Value) {
    let scroll =
        json!({ "max_offset_from_bottom": 0, "offset_from_bottom": 0, "viewport_rows": 38 });
    let panes = json!([
        { "pane_id": "w1:p1", "tab_id": "w1:t1", "agent": "claude", "agent_session_id": "exact-session", "agent_session_path": "/untrusted/never-read.jsonl", "scroll": scroll },
        { "pane_id": "w1:p2", "tab_id": "w1:t2", "agent": "codex", "agent_session_id": "../unsafe", "scroll": scroll },
        { "pane_id": "w1:p3", "tab_id": "w1:t2", "restore_error": "cwd is gone", "scroll": scroll },
        { "pane_id": "w1:p4", "tab_id": "w1:t9", "scroll": scroll },
    ]);
    let tabs = json!([
        { "tab_id": "w1:t1", "number": 1, "label": "work", "focused": true },
        { "tab_id": "w1:t2", "number": 2, "label": "logs", "focused": false },
    ]);
    (panes, tabs)
}

// parity: TS-lib-herdr-81c83d
#[test]
fn task_pane_tabs_read_only_the_pane_and_tab_lists() {
    let herdr = fixture();
    let (panes, tabs) = task_pane_lists();
    herdr.runner.respond(move |command, _| {
        if command == command_for(&["pane", "list"]) {
            return Some(Ok(result(json!({ "panes": panes }))));
        }
        if command == command_for(&["tab", "list"]) {
            return Some(Ok(result(json!({ "tabs": tabs }))));
        }
        Some(Err(ExecGitError::new(format!(
            "예상하지 않은 원격 명령: {command}"
        ))))
    });

    // 복원에 실패한 pane과 목록에 없는 탭의 pane은 task가 쓸 수 없으므로 제외한다.
    let pane_tabs = herdr
        .service
        .list_herdr_task_pane_tab_ids("feature", Some("remote"))
        .unwrap();

    assert_eq!(
        pane_tabs,
        vec![
            HerdrTaskPaneTab {
                pane_id: "w1:p1".into(),
                tab_id: "w1:t1".into(),
                agent_session: Some(HerdrAgentSession {
                    provider: AiSessionProvider::Claude,
                    session_id: "exact-session".into()
                }),
            },
            HerdrTaskPaneTab {
                pane_id: "w1:p2".into(),
                tab_id: "w1:t2".into(),
                agent_session: None
            },
        ]
    );
    assert_eq!(herdr.runner.calls().len(), 2);
}

// parity: TS-lib-herdr-81659c
#[test]
fn task_liveness_lists_spaces_without_waiting_for_the_mapping_read() {
    let herdr = fixture();
    let has_listed_workspaces = Arc::new(AtomicBool::new(false));
    let listed = Arc::clone(&has_listed_workspaces);
    let sessions = session_list(
        json!([{ "name": "kanvibe", "running": true, "socket_path": "/run/kanvibe/herdr.sock", "session_dir": "/run/kanvibe" }]),
    );
    herdr.runner.respond(move |command, _| {
        if command.contains("'session' 'list'") {
            return Some(Ok(sessions.clone()));
        }
        if !is_mapping_read(command) {
            listed.store(true, Ordering::SeqCst);
            return Some(Ok(result(
                json!({ "workspaces": [{ "workspace_id": "w7" }] }),
            )));
        }
        // Space 목록 조회가 함께 시작됐을 때만 연결 파일 읽기를 끝낸다.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !listed.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        match listed.load(Ordering::SeqCst) {
            true => Some(Ok(json!({ "task-parallel": "w7" }).to_string())),
            false => Some(Err(ExecGitError::new(
                "Space 목록 조회가 연결 파일 읽기를 기다렸다",
            ))),
        }
    });

    assert!(
        herdr
            .service
            .is_herdr_task_alive("kanvibe:task-parallel", Some("parallel-host"))
    );
    assert!(has_listed_workspaces.load(Ordering::SeqCst));
}

// parity: TS-lib-herdr-5c7d1f
#[test]
fn consecutive_commands_reuse_one_mapping_read_for_two_seconds() {
    let mut herdr = fixture();
    let clock_offset = install_frozen_clock(&mut herdr.service);
    respond_as_shared_server(&herdr.runner);
    herdr.runner.respond(|command, _| {
        is_mapping_read(command).then(|| Ok(json!({ "task-a": "w1" }).to_string()))
    });
    let host = Some("mapping-reuse-host");

    std::thread::scope(|scope| {
        let first = scope.spawn(|| herdr.service.read_herdr_tabs("kanvibe:task-a", host));
        let second = scope.spawn(|| herdr.service.read_herdr_tabs("kanvibe:task-a", host));
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    });
    clock_offset.store(1_999, Ordering::SeqCst);
    herdr
        .service
        .read_herdr_tabs("kanvibe:task-a", host)
        .unwrap();
    assert_eq!(mapping_read_count(&herdr.runner), 1);

    clock_offset.store(2_000, Ordering::SeqCst);
    herdr
        .service
        .read_herdr_tabs("kanvibe:task-a", host)
        .unwrap();
    assert_eq!(mapping_read_count(&herdr.runner), 2);
}

// parity: TS-lib-herdr-47a1df
#[test]
fn a_failed_mapping_read_is_not_handed_to_the_next_command() {
    let herdr = fixture();
    respond_as_shared_server(&herdr.runner);
    let read_count = AtomicUsize::new(0);
    herdr.runner.respond(move |command, _| {
        if !is_mapping_read(command) {
            return None;
        }
        Some(match read_count.fetch_add(1, Ordering::SeqCst) {
            0 => Err(ExecGitError::new("Connection reset by peer")),
            _ => Ok(json!({ "task-a": "w1" }).to_string()),
        })
    });
    let host = Some("mapping-failure-host");

    let error = herdr
        .service
        .read_herdr_tabs("kanvibe:task-a", host)
        .unwrap_err();
    assert!(error.to_string().contains("Connection reset by peer"));

    assert_eq!(
        herdr
            .service
            .read_herdr_tabs("kanvibe:task-a", host)
            .unwrap(),
        vec![]
    );
    assert_eq!(mapping_read_count(&herdr.runner), 2);
}
