//! 서버 기동, 레이아웃 적용, 동시 연결 합류, 복원 실패 pane 정리.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use kanvibe_process::exec::ExecGitError;
use kanvibe_process::worktree::PaneLayoutSelection;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::fixtures::{
    RESTORE_UNAVAILABLE, SharedSessionDirectory, command_for, fixture, fixture_with_environment,
    respond_as_healthy_server, respond_in_sequence, result, shared_command_for, wait_for_file,
    write_executable,
};
use crate::herdr::build_herdr_session_cleanup_command;

/// 자기 세션 ID, 프로세스 ID, 인자, 환경 변수 이름을 JSON으로 남기는 가짜 herdr
fn write_recording_herdr(directory: &Path) -> std::path::PathBuf {
    let result_path = directory.join("launch-result");
    let script = format!(
        "#!/usr/bin/env python3\nimport json, os, sys\nresult = {path:?}\nopen(result + '.partial', 'w').write(json.dumps({{'sid': os.getsid(0), 'pid': os.getpid(), 'args': sys.argv[1:], 'env': sorted(os.environ)}}))\nos.rename(result + '.partial', result)\n",
        path = result_path.to_string_lossy()
    );
    write_executable(&directory.join("herdr"), &script);
    result_path
}

fn layout(layout_type: PaneLayoutType, commands: &[&str]) -> PaneLayoutSelection {
    let panes = commands
        .iter()
        .enumerate()
        .map(|(position, command)| PaneCommand {
            position: position as i64,
            command: command.to_string(),
        })
        .collect();
    PaneLayoutSelection { layout_type, panes }
}

#[test]
fn local_server_runs_in_its_own_session_with_a_narrowed_environment() {
    let fake_bin = TempDir::new().unwrap();
    let result_path = write_recording_herdr(fake_bin.path());
    let path = format!("{}:/usr/bin:/bin", fake_bin.path().to_string_lossy());
    let herdr = fixture_with_environment(&[
        ("PATH", &path),
        ("PORT", "4884"),
        ("NODE_ENV", "production"),
        ("KANVIBE_HOOK_PORT", "4885"),
    ]);
    respond_as_healthy_server(&herdr.runner);
    respond_in_sequence(&herdr.runner, vec![Err("server not running")]);

    herdr
        .service
        .ensure_herdr_session("feature", "/work", None, None)
        .unwrap();

    let launch: Value = serde_json::from_str(&wait_for_file(&result_path)).unwrap();
    assert_eq!(
        launch["sid"], launch["pid"],
        "서버가 앱의 프로세스 그룹과 분리된 세션 리더여야 한다"
    );
    assert_eq!(launch["args"], json!(["--session", "feature", "server"]));
    let environment_keys: Vec<&str> = launch["env"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for key in ["PORT", "NODE_ENV", "KANVIBE_HOOK_PORT"] {
        assert!(!environment_keys.contains(&key), "{key}");
    }
    assert_eq!(
        herdr.runner.commands(),
        vec![
            command_for(&["workspace", "list"]),
            command_for(&["workspace", "list"]),
            command_for(&["pane", "list"])
        ]
    );
}

#[test]
fn local_server_spawn_failure_reaches_the_connection_request() {
    let mut herdr = fixture();
    herdr.service.posix_shell_path = "/nonexistent/kanvibe-test-sh".to_string();
    respond_in_sequence(&herdr.runner, vec![Err("server not running")]);

    let error = herdr
        .service
        .ensure_herdr_session("feature", "/work", None, None)
        .unwrap_err();

    assert!(!error.to_string().is_empty());
    assert_eq!(herdr.runner.calls().len(), 1);
}

#[test]
fn ssh_server_starts_on_the_remote_host_and_waits_for_readiness() {
    let mut herdr = fixture();
    // 로컬 서버를 띄우려 하면 실패하도록 셸 경로를 막아 둔다.
    herdr.service.posix_shell_path = "/nonexistent/kanvibe-test-sh".to_string();
    respond_as_healthy_server(&herdr.runner);
    respond_in_sequence(&herdr.runner, vec![Err("server not running"), Ok("")]);

    herdr
        .service
        .ensure_herdr_session("feature", "/work", None, Some("remote"))
        .unwrap();

    let calls = herdr.runner.calls();
    assert!(calls[1].command.contains("setsid"));
    assert_eq!(calls[1].ssh_host.as_deref(), Some("remote"));
}

#[test]
fn ssh_server_launch_command_makes_herdr_its_own_session_leader() {
    // Given: 원격 시작 명령을 그대로 받아 실행할 수 있도록, 자기 세션 정보를 기록하는 가짜 herdr를 PATH 맨 앞에 둔다
    let herdr = fixture();
    respond_as_healthy_server(&herdr.runner);
    respond_in_sequence(&herdr.runner, vec![Err("server not running"), Ok("")]);
    herdr
        .service
        .ensure_herdr_session("feature", "/work", None, Some("remote"))
        .unwrap();
    let launch_command = herdr.runner.commands()[1].clone();
    let fake_bin = TempDir::new().unwrap();
    let result_path = write_recording_herdr(fake_bin.path());

    // When: 원격 셸과 같은 POSIX sh에서 실행한다
    let path = format!(
        "{}:{}",
        fake_bin.path().to_string_lossy(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = std::process::Command::new("/bin/sh")
        .args(["-c", &launch_command])
        .env("PATH", path)
        .status()
        .unwrap();
    assert!(status.success());

    // Then: herdr 프로세스 자신이 세션 리더여야 Herdr가 detached daemon으로 인정한다
    let launch: Value = serde_json::from_str(&wait_for_file(&result_path)).unwrap();
    assert_eq!(launch["sid"], launch["pid"]);
    assert_eq!(launch["args"], json!(["--session", "feature", "server"]));
}

#[test]
fn reconnecting_to_an_existing_session_does_not_repeat_layout_or_commands() {
    let herdr = fixture();
    respond_as_healthy_server(&herdr.runner);

    let quad = layout(PaneLayoutType::Quad, &["do-not-repeat"]);
    herdr
        .service
        .ensure_herdr_session("feature", "/work", Some(&quad), None)
        .unwrap();

    assert_eq!(
        herdr.runner.commands(),
        vec![
            command_for(&["workspace", "list"]),
            command_for(&["pane", "list"])
        ]
    );
}

/// 새 workspace를 만들고 split마다 다음 pane ID를 주는 서버
fn respond_as_empty_server(runner: &crate::test_support::ScriptedShellRunner) {
    let next_pane = AtomicUsize::new(2);
    runner.respond(move |command, _| {
        let output = if command.contains("'workspace' 'list'") {
            result(json!({ "workspaces": [] }))
        } else if command.contains("'workspace' 'create'") {
            result(json!({ "root_pane": { "pane_id": "w1:p1" } }))
        } else if command.contains("'pane' 'split'") {
            let pane = next_pane.fetch_add(1, Ordering::SeqCst);
            result(json!({ "pane": { "pane_id": format!("w1:p{pane}") } }))
        } else {
            String::new()
        };
        Some(Ok(output))
    });
}

/// 레이아웃 종류, 기대하는 split(대상 pane, 방향), 위치 순서의 pane ID
type LayoutCase<'a> = (PaneLayoutType, &'a [(&'a str, &'a str)], &'a [&'a str]);

#[test]
fn layout_commands_reach_the_pane_at_each_position() {
    let cases: [LayoutCase; 6] = [
        (PaneLayoutType::Single, &[], &["w1:p1"]),
        (
            PaneLayoutType::Horizontal2,
            &[("w1:p1", "down")],
            &["w1:p1", "w1:p2"],
        ),
        (
            PaneLayoutType::Vertical2,
            &[("w1:p1", "right")],
            &["w1:p1", "w1:p2"],
        ),
        (
            PaneLayoutType::LeftRightTb,
            &[("w1:p1", "right"), ("w1:p2", "down")],
            &["w1:p1", "w1:p2", "w1:p3"],
        ),
        (
            PaneLayoutType::LeftTbRight,
            &[("w1:p1", "right"), ("w1:p1", "down")],
            &["w1:p1", "w1:p3", "w1:p2"],
        ),
        (
            PaneLayoutType::Quad,
            &[("w1:p1", "right"), ("w1:p1", "down"), ("w1:p2", "down")],
            &["w1:p1", "w1:p2", "w1:p3", "w1:p4"],
        ),
    ];
    for (layout_type, splits, pane_ids) in cases {
        assert_layout_commands(layout_type, splits, pane_ids);
    }
}

fn commands_containing(commands: &[String], needle: &str) -> Vec<String> {
    commands
        .iter()
        .filter(|command| command.contains(needle))
        .cloned()
        .collect()
}

fn split_command((pane_id, direction): &(&str, &str)) -> String {
    let arguments = [
        "pane",
        "split",
        pane_id,
        "--direction",
        direction,
        "--cwd",
        "/work tree",
    ];
    command_for(&arguments)
}

fn send_text_command((position, pane_id): (usize, &&str)) -> String {
    command_for(&[
        "pane",
        "send-text",
        pane_id,
        &format!("echo pane-{position}\r"),
    ])
}

/// 레이아웃 하나를 새 workspace에 적용하고 split 순서와 위치별 명령 대상 pane을 확인한다
fn assert_layout_commands(layout_type: PaneLayoutType, splits: &[(&str, &str)], pane_ids: &[&str]) {
    let herdr = fixture();
    respond_as_empty_server(&herdr.runner);
    let pane_commands: Vec<String> = (0..pane_ids.len())
        .map(|position| format!("echo pane-{position}"))
        .collect();
    let pane_command_refs: Vec<&str> = pane_commands.iter().map(String::as_str).collect();
    let selection = layout(layout_type, &pane_command_refs);

    herdr
        .service
        .ensure_herdr_session("feature", "/work tree", Some(&selection), Some("remote"))
        .unwrap();

    let commands = herdr.runner.commands();
    let create = ["workspace", "create", "--cwd", "/work tree", "--focus"];
    assert!(commands.contains(&command_for(&create)));
    let expected_splits: Vec<String> = splits.iter().map(split_command).collect();
    let actual_splits = commands_containing(&commands, "'pane' 'split'");
    assert_eq!(actual_splits, expected_splits, "{layout_type:?}");
    let expected_texts: Vec<String> = pane_ids.iter().enumerate().map(send_text_command).collect();
    let actual_texts = commands_containing(&commands, "'pane' 'send-text'");
    assert_eq!(actual_texts, expected_texts, "{layout_type:?}");
    let calls = herdr.runner.calls();
    assert!(
        calls
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote"))
    );
}

#[test]
fn concurrent_connection_requests_join_one_bootstrap() {
    let herdr = fixture();
    respond_as_healthy_server(&herdr.runner);
    let (started, started_signal) = mpsc::channel::<()>();
    let (release, release_signal) = mpsc::channel::<()>();
    let started = Mutex::new(Some(started));
    let release_signal = Mutex::new(release_signal);
    herdr.runner.respond(move |_, _| {
        let started = started.lock().unwrap().take()?;
        started.send(()).unwrap();
        release_signal.lock().unwrap().recv().unwrap();
        Some(Ok(result(
            json!({ "workspaces": [{ "workspace_id": "w1" }] }),
        )))
    });

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            herdr
                .service
                .ensure_herdr_session("feature", "/work", None, None)
        });
        started_signal.recv().unwrap();
        let second = scope.spawn(|| {
            herdr
                .service
                .ensure_herdr_session("feature", "/work", None, None)
        });
        std::thread::sleep(Duration::from_millis(100));
        release.send(()).unwrap();
        assert_eq!(first.join().unwrap(), Ok(None));
        assert_eq!(second.join().unwrap(), Ok(None));
    });

    assert_eq!(
        herdr.runner.commands(),
        vec![
            command_for(&["workspace", "list"]),
            command_for(&["pane", "list"])
        ]
    );
}

#[test]
fn ssh_connection_failures_never_start_a_remote_server() {
    let messages = [
        "Connection reset by peer",
        "Permission denied (publickey)",
        "Host key verification failed",
        "SSH 명령이 5초 안에 완료되지 않았습니다.",
    ];
    for message in messages {
        let herdr = fixture();
        let failure = message.to_string();
        herdr
            .runner
            .respond(move |_, _| Some(Err(ExecGitError::new(failure.clone()))));

        let error = herdr
            .service
            .ensure_herdr_session("feature", "/work", None, Some("remote"))
            .unwrap_err();

        assert!(error.to_string().contains(message));
        assert_eq!(herdr.runner.calls().len(), 1, "{message}");
    }
}

/// 공유 서버의 workspace·pane 목록을 흉내 낸다. pane을 닫으면 목록에서도 사라진다
fn respond_as_shared_server(
    runner: &crate::test_support::ScriptedShellRunner,
    shared: &SharedSessionDirectory,
    panes: Vec<Value>,
) {
    let live_panes = Arc::new(Mutex::new(panes));
    let session_list = shared.session_list();
    runner.respond(move |command, _| {
        let mut live_panes = live_panes.lock().unwrap();
        let workspace_of = |pane: &Value| pane["pane_id"].as_str().unwrap().split(':').next().unwrap().to_string();
        let output = if command.contains("'session' 'list'") {
            session_list.clone()
        } else if command.contains("'workspace' 'list'") {
            let mut workspace_ids: Vec<String> = live_panes.iter().map(workspace_of).collect();
            workspace_ids.dedup();
            result(json!({ "workspaces": workspace_ids.iter().map(|id| json!({ "workspace_id": id })).collect::<Vec<_>>() }))
        } else if command.contains("'pane' 'list'") {
            let listed: Vec<Value> = live_panes
                .iter()
                .map(|pane| {
                    let mut pane = pane.clone();
                    pane["tab_id"] = json!(format!("{}:t1", workspace_of(&pane)));
                    pane
                })
                .collect();
            result(json!({ "panes": listed }))
        } else if command.contains("'pane' 'close'") {
            live_panes.retain(|pane| !command.contains(&format!("'{}'", pane["pane_id"].as_str().unwrap())));
            result(json!({ "type": "ok" }))
        } else if command.contains("'workspace' 'create'") {
            live_panes.push(json!({ "pane_id": "w3:p1" }));
            result(json!({ "root_pane": { "pane_id": "w3:p1" } }))
        } else {
            String::new()
        };
        Some(Ok(output))
    });
}

#[test]
fn closes_only_the_task_space_unrestorable_panes_and_keeps_the_space() {
    let herdr = fixture();
    let shared = SharedSessionDirectory::new(json!({ "task-a": "w1", "task-b": "w2" }));
    respond_as_shared_server(
        &herdr.runner,
        &shared,
        vec![
            json!({ "pane_id": "w1:p1" }),
            json!({ "pane_id": "w1:p2", "restore_error": RESTORE_UNAVAILABLE }),
            json!({ "pane_id": "w2:p1", "restore_error": RESTORE_UNAVAILABLE }),
        ],
    );

    let workspace_id = herdr
        .service
        .ensure_herdr_session("kanvibe:task-a", "/work", None, None)
        .unwrap();

    assert_eq!(workspace_id.as_deref(), Some("w1"));
    let commands = herdr.runner.commands();
    let closes: Vec<&String> = commands
        .iter()
        .filter(|command| command.contains("'pane' 'close'"))
        .collect();
    assert_eq!(
        closes,
        vec![&shared_command_for(&["pane", "close", "w1:p2"])]
    );
    assert!(
        !commands
            .iter()
            .any(|command| command.contains("'workspace' 'create'"))
    );
}

#[test]
fn replaces_a_task_space_whose_panes_all_failed_to_restore() {
    let herdr = fixture();
    let shared = SharedSessionDirectory::new(json!({ "task-a": "w1", "task-b": "w2" }));
    respond_as_shared_server(
        &herdr.runner,
        &shared,
        vec![
            json!({ "pane_id": "w1:p1", "restore_error": RESTORE_UNAVAILABLE }),
            json!({ "pane_id": "w2:p1" }),
        ],
    );

    let workspace_id = herdr
        .service
        .ensure_herdr_session("kanvibe:task-a", "/work", None, None)
        .unwrap();

    assert_eq!(workspace_id.as_deref(), Some("w3"));
    let commands = herdr.runner.commands();
    assert!(commands.contains(&shared_command_for(&["pane", "close", "w1:p1"])));
    assert!(commands.contains(&shared_command_for(&[
        "workspace",
        "create",
        "--cwd",
        "/work",
        "--label",
        "task-a",
        "--no-focus"
    ])));
    assert_eq!(
        shared.read_mapping(),
        json!({ "task-a": "w3", "task-b": "w2" })
    );
}

#[test]
fn creates_a_new_workspace_when_every_dedicated_server_pane_failed_to_restore() {
    let herdr = fixture();
    let live_panes = Mutex::new(vec![
        json!({ "pane_id": "w1:p1", "restore_error": RESTORE_UNAVAILABLE }),
    ]);
    herdr.runner.respond(move |command, _| {
        let mut live_panes = live_panes.lock().unwrap();
        let output = if command.contains("'workspace' 'list'") {
            let workspaces = match live_panes.is_empty() {
                true => json!([]),
                false => json!([{ "workspace_id": "w1" }]),
            };
            result(json!({ "workspaces": workspaces }))
        } else if command.contains("'pane' 'list'") {
            result(json!({ "panes": *live_panes }))
        } else if command.contains("'pane' 'close'") {
            live_panes.clear();
            result(json!({ "type": "ok" }))
        } else if command.contains("'workspace' 'create'") {
            result(json!({ "root_pane": { "pane_id": "w2:p1" } }))
        } else {
            String::new()
        };
        Some(Ok(output))
    });

    herdr
        .service
        .ensure_herdr_session("feature", "/work", None, None)
        .unwrap();

    let commands = herdr.runner.commands();
    assert!(commands.contains(&command_for(&["pane", "close", "w1:p1"])));
    assert!(commands.contains(&command_for(&[
        "workspace",
        "create",
        "--cwd",
        "/work",
        "--focus"
    ])));
}

#[test]
fn initial_layout_failure_cleans_up_the_new_session() {
    let herdr = fixture();
    let empty = result(json!({ "workspaces": [] }));
    let root = result(json!({ "root_pane": { "pane_id": "w1:p1" } }));
    respond_in_sequence(
        &herdr.runner,
        vec![Ok(&empty), Ok(&root), Err("split failed"), Ok("")],
    );

    let quad = layout(PaneLayoutType::Quad, &[]);
    let error = herdr
        .service
        .ensure_herdr_session("feature", "/work", Some(&quad), None)
        .unwrap_err();

    assert!(error.to_string().contains("split failed"));
    assert_eq!(
        herdr.runner.commands().last().unwrap(),
        &build_herdr_session_cleanup_command("feature").unwrap()
    );
}
