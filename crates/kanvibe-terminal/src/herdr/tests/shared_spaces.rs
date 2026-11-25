//! 공유 서버 `kanvibe` 안의 task Space. 연결 파일은 임시 세션 디렉터리의 실제 파일이다.

use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use kanvibe_process::exec::ExecGitError;
use kanvibe_process::worktree::PaneLayoutSelection;
use serde_json::{Value, json};

use super::fixtures::{
    HerdrFixture, SharedSessionDirectory, count_containing, fixture, result, shared_command_for,
    wait_until,
};

struct SharedSpaces {
    herdr: HerdrFixture,
    shared: SharedSessionDirectory,
    spaces: Arc<Mutex<Vec<String>>>,
}

impl SharedSpaces {
    fn commands(&self) -> Vec<String> {
        self.herdr.runner.commands()
    }

    fn space_count(&self) -> usize {
        self.spaces.lock().unwrap().len()
    }
}

/// 공유 서버의 Space 목록을 흉내 낸다. 새 Space는 `w{개수+1}`로 만든다
fn shared_spaces(mapping: Value, spaces: &[&str]) -> SharedSpaces {
    let herdr = fixture();
    let shared = SharedSessionDirectory::new(mapping);
    let spaces = Arc::new(Mutex::new(
        spaces.iter().map(ToString::to_string).collect::<Vec<_>>(),
    ));
    let live_spaces = Arc::clone(&spaces);
    let session_list = shared.session_list();
    herdr.runner.respond(move |command, _| {
        let mut spaces = live_spaces.lock().unwrap();
        let output = if command.contains("'session' 'list'") {
            session_list.clone()
        } else if command.contains("'workspace' 'list'") {
            let workspaces: Vec<Value> = spaces.iter().map(|id| json!({ "workspace_id": id, "label": id })).collect();
            result(json!({ "workspaces": workspaces }))
        } else if command.contains("'pane' 'list'") {
            let panes: Vec<Value> = spaces.iter().map(|id| json!({ "pane_id": format!("{id}:p1") })).collect();
            result(json!({ "panes": panes }))
        } else if command.contains("'workspace' 'create'") {
            let id = format!("w{}", spaces.len() + 1);
            spaces.push(id.clone());
            result(json!({ "workspace": { "workspace_id": id }, "root_pane": { "pane_id": format!("{id}:p1") } }))
        } else if command.contains("'tab' 'list'") {
            result(json!({ "tabs": [{ "tab_id": "w1:t1", "number": 1, "label": "coding", "focused": true }] }))
        } else {
            result(json!({}))
        };
        Some(Ok(output))
    });
    SharedSpaces {
        herdr,
        shared,
        spaces,
    }
}

fn quad_layout(commands: &[&str]) -> PaneLayoutSelection {
    let panes = commands
        .iter()
        .enumerate()
        .map(|(position, command)| PaneCommand {
            position: position as i64,
            command: command.to_string(),
        })
        .collect();
    PaneLayoutSelection {
        layout_type: PaneLayoutType::Quad,
        panes,
    }
}

fn has_session_teardown(commands: &[String]) -> bool {
    commands.iter().any(|command| {
        command.contains("'session' 'stop'") || command.contains("'session' 'delete'")
    })
}

/// Node `os.hostname()`을 hex로 바꾼 값. 이 호스트의 잠금 소유자 이름에 들어간다
fn local_hostname_hex() -> String {
    let mut buffer = [0_u8; 256];
    // SAFETY: 버퍼 길이를 넘지 않게 쓰고, 끝의 NUL 앞까지만 읽는다
    unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    let length = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    hex::encode(&buffer[..length])
}

// parity: TS-lib-herdr-9cfa8a
#[test]
fn different_tasks_become_spaces_of_one_server_and_concurrent_reconnects_do_not_duplicate() {
    let spaces = shared_spaces(json!({}), &[]);
    let service = &spaces.herdr.service;

    std::thread::scope(|scope| {
        let connections = [
            ("kanvibe:task-a", "/work/a"),
            ("kanvibe:task-b", "/work/b"),
            ("kanvibe:task-a", "/work/a"),
        ];
        let handles: Vec<_> = connections
            .into_iter()
            .map(|(session_name, cwd)| {
                scope.spawn(move || service.ensure_herdr_session(session_name, cwd, None, None))
            })
            .collect();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }
    });

    assert_eq!(spaces.space_count(), 2);
    let mapping = spaces.shared.read_mapping();
    let mut assigned: Vec<&str> = ["task-a", "task-b"]
        .iter()
        .map(|key| mapping[key].as_str().unwrap())
        .collect();
    assigned.sort();
    assert_eq!(assigned, vec!["w1", "w2"]);
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'create'"),
        2
    );
    assert!(
        spaces
            .commands()
            .iter()
            .all(|command| !command.contains("--session 'kanvibe:"))
    );
}

// parity: TS-lib-herdr-39850c
#[test]
fn renamed_space_reconnects_by_stored_id_without_repeating_startup_commands() {
    let spaces = shared_spaces(json!({ "task-a": "w1" }), &["w1"]);

    let quad = quad_layout(&["once"]);
    spaces
        .herdr
        .service
        .ensure_herdr_session("kanvibe:task-a", "/work/a", Some(&quad), None)
        .unwrap();
    let commands = spaces.commands();
    assert!(
        !commands
            .iter()
            .any(|command| command.contains("'workspace' 'create'")
                || command.contains("'pane' 'send-text'"))
    );

    spaces
        .herdr
        .service
        .read_herdr_tabs("kanvibe:task-a", None)
        .unwrap();
    assert_eq!(
        spaces.commands().last().unwrap(),
        &shared_command_for(&["tab", "list", "--workspace", "w1"])
    );
}

// parity: TS-lib-herdr-c5a17a
#[test]
fn live_space_returns_the_checked_id_without_writing_the_mapping() {
    for ssh_host in [None, Some("remote")] {
        let spaces = shared_spaces(json!({ "task-a": "w1" }), &["w1"]);
        spaces.herdr.runner.respond(|command, _| {
            command
                .starts_with("test -f ")
                .then(|| Ok(json!({ "task-a": "w1" }).to_string()))
        });
        let inode_before = std::fs::metadata(spaces.shared.mapping_path())
            .unwrap()
            .ino();

        let workspace_id = spaces
            .herdr
            .service
            .ensure_herdr_session("kanvibe:task-a", "/work/a", None, ssh_host)
            .unwrap();

        assert_eq!(workspace_id.as_deref(), Some("w1"));
        let commands = spaces.commands();
        assert!(
            !commands
                .iter()
                .any(|command| command.contains("__KANVIBE_FILE_UPDATE")),
            "{ssh_host:?}"
        );
        assert_eq!(
            std::fs::metadata(spaces.shared.mapping_path())
                .unwrap()
                .ino(),
            inode_before
        );
        if ssh_host.is_some() {
            assert_eq!(
                commands
                    .iter()
                    .filter(|command| command.starts_with("test -f "))
                    .count(),
                1
            );
        }
        assert_eq!(count_containing(&commands, "'workspace' 'list'"), 1);
    }
}

// parity: TS-lib-herdr-f48e1c
#[test]
fn space_issued_by_another_connection_is_rechecked_inside_the_lock() {
    let spaces = shared_spaces(json!({}), &[]);
    // 다른 접속이 연결 파일 잠금을 쥐고 있는 상태로 시작한다.
    let lock_path = format!("{}.lock", spaces.shared.mapping_path().to_string_lossy());
    let owner_path = format!(
        "{lock_path}/owner-{}-other-connection-{}",
        std::process::id(),
        local_hostname_hex()
    );
    std::fs::create_dir_all(&owner_path).unwrap();

    let workspace_id = std::thread::scope(|scope| {
        let connecting = scope.spawn(|| {
            spaces
                .herdr
                .service
                .ensure_herdr_session("kanvibe:task-a", "/work/a", None, None)
        });
        wait_until(|| count_containing(&spaces.commands(), "'session' 'list'") == 1);
        std::thread::sleep(Duration::from_millis(200));
        // 잠금 밖에서 빈 연결을 읽은 뒤, 다른 접속이 먼저 Space를 발급하고 잠금을 푼다.
        spaces
            .shared
            .write_mapping(&json!({ "task-a": "wA", "task-b": "wB" }));
        *spaces.spaces.lock().unwrap() = vec!["wA".to_string(), "wB".to_string()];
        std::fs::remove_dir(&owner_path).unwrap();
        std::fs::remove_dir(&lock_path).unwrap();
        connecting.join().unwrap()
    });

    assert_eq!(workspace_id, Ok(Some("wA".to_string())));
    assert_eq!(
        spaces.shared.read_mapping(),
        json!({ "task-a": "wA", "task-b": "wB" })
    );
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'create'"),
        0
    );
}

// parity: TS-lib-herdr-4ef359
#[test]
fn one_task_tab_mutation_cannot_select_another_task() {
    let spaces = shared_spaces(json!({ "task-a": "w1" }), &["w1"]);

    let error = spaces
        .herdr
        .service
        .run_herdr_command("kanvibe:task-a", &["tab", "close", "w2:t1"], None)
        .unwrap_err();

    assert!(error.to_string().contains("다른 Space"));
    assert_eq!(count_containing(&spaces.commands(), "'tab' 'close'"), 0);
}

// parity: TS-lib-herdr-cb95b1
#[test]
fn numeric_space_id_task_reconnects_even_with_lettered_space_ids_present() {
    let spaces = shared_spaces(json!({ "task-a": "w1", "task-b": "wA" }), &["w1", "wA"]);

    spaces
        .herdr
        .service
        .ensure_herdr_session("kanvibe:task-a", "/work/a", None, None)
        .unwrap();
    spaces
        .herdr
        .service
        .read_herdr_tabs("kanvibe:task-a", None)
        .unwrap();

    assert_eq!(
        spaces.commands().last().unwrap(),
        &shared_command_for(&["tab", "list", "--workspace", "w1"])
    );
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'create'"),
        0
    );
    assert_eq!(
        spaces.shared.read_mapping(),
        json!({ "task-a": "w1", "task-b": "wA" })
    );
}

// parity: TS-lib-herdr-0ce331
#[test]
fn newly_issued_lettered_space_reconnects_and_closes_alone() {
    let spaces = shared_spaces(json!({ "task-a": "w1" }), &["w1"]);
    let live_spaces = Arc::clone(&spaces.spaces);
    spaces.herdr.runner.respond(move |command, _| {
        command.contains("'workspace' 'create'").then(|| {
            live_spaces.lock().unwrap().push("wA".to_string());
            Ok(result(json!({ "root_pane": { "pane_id": "wA:p1" } })))
        })
    });
    let service = &spaces.herdr.service;

    service
        .ensure_herdr_session("kanvibe:task-b", "/work/b", None, None)
        .unwrap();
    assert_eq!(
        service
            .get_herdr_workspace_id("kanvibe:task-b", None)
            .unwrap()
            .as_deref(),
        Some("wA")
    );
    service
        .ensure_herdr_session("kanvibe:task-b", "/work/b", None, None)
        .unwrap();
    service.read_herdr_tabs("kanvibe:task-b", None).unwrap();
    assert_eq!(
        spaces.commands().last().unwrap(),
        &shared_command_for(&["tab", "list", "--workspace", "wA"])
    );
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'create'"),
        1
    );

    service
        .close_herdr_task_workspace("kanvibe:task-b", None)
        .unwrap();
    assert_eq!(spaces.shared.read_mapping(), json!({ "task-a": "w1" }));
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'close' 'wA'"),
        1
    );
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'close' 'w1'"),
        0
    );
}

// parity: TS-lib-herdr-43227b
#[test]
fn lettered_ids_cannot_reach_another_space_either() {
    let cases: [&[&str]; 5] = [
        &["tab", "close", "wA:t1"],
        &["tab", "close", "w2:tA"],
        &["pane", "send-text", "wA:p1", "hello"],
        &["pane", "send-text", "w2:pA", "hello"],
        &["pane", "split", "--pane", "wA:pA"],
    ];
    for arguments in cases {
        let spaces = shared_spaces(json!({ "task-a": "w1" }), &["w1"]);

        let error = spaces
            .herdr
            .service
            .run_herdr_command("kanvibe:task-a", arguments, None)
            .unwrap_err();

        assert!(error.to_string().contains("다른 Space"), "{arguments:?}");
        // Space 연결 파일 경로를 찾는 세션 목록 말고는 아무 명령도 보내지 않는다.
        assert!(
            spaces
                .commands()
                .iter()
                .all(|command| command.contains("'session' 'list'")),
            "{arguments:?}"
        );
    }
}

// parity: TS-lib-herdr-50e48a
#[test]
fn initial_layout_failure_closes_only_the_new_space() {
    let spaces = shared_spaces(json!({}), &["w1"]);
    spaces.herdr.runner.respond(|command, _| {
        command
            .contains("'pane' 'split'")
            .then(|| Err(ExecGitError::new("split failed")))
    });

    let quad = quad_layout(&[]);
    let error = spaces
        .herdr
        .service
        .ensure_herdr_session("kanvibe:task-b", "/work/b", Some(&quad), None)
        .unwrap_err();

    assert!(error.to_string().contains("split failed"));
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'close' 'w2'"),
        1
    );
    assert!(!has_session_teardown(&spaces.commands()));
    assert_eq!(spaces.shared.read_mapping(), json!({}));
}

// parity: TS-lib-herdr-581c63
#[test]
fn closing_one_task_keeps_other_spaces_and_the_shared_server() {
    let spaces = shared_spaces(json!({ "task-a": "w1", "task-b": "w2" }), &["w1", "w2"]);
    let service = &spaces.herdr.service;

    service
        .close_herdr_task_workspace("kanvibe:task-a", None)
        .unwrap();

    assert_eq!(spaces.shared.read_mapping(), json!({ "task-b": "w2" }));
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'close' 'w1'"),
        1
    );
    assert!(!has_session_teardown(&spaces.commands()));
    assert!(!service.is_herdr_task_alive("kanvibe:task-a", None));
    assert!(service.is_herdr_task_alive("kanvibe:task-b", None));
}

// parity: TS-lib-herdr-5f3adc
#[test]
fn missing_task_space_counts_as_no_session_even_with_a_live_server() {
    let spaces = shared_spaces(json!({ "task-a": "w9" }), &[]);

    assert!(
        !spaces
            .herdr
            .service
            .is_herdr_task_alive("kanvibe:task-a", None)
    );
}

// parity: TS-lib-herdr-07e7c0
#[test]
fn space_closed_natively_is_recreated_and_its_id_updated() {
    let spaces = shared_spaces(json!({ "task-a": "w9" }), &[]);

    let workspace_id = spaces
        .herdr
        .service
        .ensure_herdr_session("kanvibe:task-a", "/work/a", None, None)
        .unwrap();

    assert_eq!(workspace_id.as_deref(), Some("w1"));
    assert_eq!(spaces.shared.read_mapping(), json!({ "task-a": "w1" }));
    assert_eq!(spaces.space_count(), 1);
}

// parity: TS-lib-herdr-91ba7f
#[test]
fn shared_server_mirror_returns_only_the_task_space_panes() {
    let spaces = shared_spaces(json!({ "task-a": "w1", "task-b": "w2" }), &["w1", "w2"]);
    spaces.herdr.runner.respond(|command, _| {
        let output = if command.contains("'pane' 'list'") {
            result(json!({ "panes": [
                { "pane_id": "w1:p1", "tab_id": "w1:t1", "scroll": { "viewport_rows": 30 } },
                { "pane_id": "w2:p1", "tab_id": "w2:t1", "scroll": { "viewport_rows": 30 } },
            ] }))
        } else if command.contains("'pane' 'layout'") {
            result(json!({ "layout": { "area": { "width": 80 }, "panes": [], "focused_pane_id": "w1:p1" } }))
        } else if command.contains("'pane' 'process-info'") {
            result(json!({ "process_info": { "foreground_processes": [{ "name": "zsh" }] } }))
        } else {
            return None;
        };
        Some(Ok(output))
    });

    let mirror_panes = spaces
        .herdr
        .service
        .list_herdr_mirror_panes(
            "kanvibe:task-a",
            None,
            &crate::herdr::HerdrMirrorPaneOptions::default(),
        )
        .unwrap();

    let ids: Vec<&str> = mirror_panes.iter().map(|pane| pane.id.as_str()).collect();
    assert_eq!(ids, vec!["w1:p1"]);
    assert_eq!(
        count_containing(&spaces.commands(), "'process-info' '--pane' 'w2:p1'"),
        0
    );
}

// parity: TS-lib-herdr-b55d8d
#[test]
fn shared_server_agents_report_their_task_session_reference() {
    let spaces = shared_spaces(json!({ "task-a": "w1", "task-b": "w2" }), &["w1", "w2"]);
    spaces.herdr.runner.respond(|command, _| {
        command.contains("'agent' 'list'").then(|| {
            Ok(result(json!({ "agents": [
                { "agent": "codex", "pane_id": "w1:p1", "tab_id": "w1:t1", "cwd": "/work/a", "agent_status": "working" },
                { "agent": "claude", "pane_id": "w2:p1", "tab_id": "w2:t1", "cwd": "/work/b", "agent_status": "working" },
            ] })))
        })
    });

    let agents = spaces.herdr.service.list_herdr_running_agent_panes(None);

    let session_names: Vec<&str> = agents
        .iter()
        .map(|agent| agent.session_name.as_str())
        .collect();
    assert_eq!(session_names, vec!["kanvibe:task-a", "kanvibe:task-b"]);
}

// parity: TS-lib-herdr-4f856a
#[test]
fn input_text_or_tab_names_are_not_mistaken_for_space_ids() {
    let spaces = shared_spaces(json!({ "task-a": "w1" }), &["w1"]);
    let service = &spaces.herdr.service;

    assert!(
        service
            .run_herdr_command(
                "kanvibe:task-a",
                &["pane", "send-text", "w1:p1", "w2:p1"],
                None
            )
            .is_ok()
    );
    assert!(
        service
            .run_herdr_command("kanvibe:task-a", &["tab", "rename", "w1:t1", "w2:t1"], None)
            .is_ok()
    );
}

// parity: TS-lib-herdr-6dea06
#[test]
fn corrupted_mapping_is_reported_without_being_overwritten() {
    let spaces = shared_spaces(json!({}), &[]);
    std::fs::write(spaces.shared.mapping_path(), "{broken").unwrap();

    assert!(
        spaces
            .herdr
            .service
            .ensure_herdr_session("kanvibe:task-a", "/work/a", None, None)
            .is_err()
    );

    assert_eq!(
        std::fs::read_to_string(spaces.shared.mapping_path()).unwrap(),
        "{broken"
    );
    assert_eq!(
        count_containing(&spaces.commands(), "'workspace' 'create'"),
        0
    );
}
