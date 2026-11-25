//! 원격 machine 등록과 서버 live handoff.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::fixtures::{count_containing, fixture, respond_in_sequence};
use crate::herdr::{HerdrError, build_herdr_command};

/// 현재 KanVibe가 띄운 서버. 세션 리더라 교체 없이 바로 machine으로 등록할 수 있다
const DETACHED_SERVER_STATUS: &str =
    r#"{"running":true,"capabilities":{"detached_server_daemon":true,"live_handoff":true}}"#;

fn machine_list(server_name: &str) -> String {
    build_herdr_command(server_name, &["machine", "list", "--json"]).unwrap()
}

/// 첫 호출을 붙잡아 두는 응답기를 건다. 첫 호출이 들어오면 알리고, 풀어 줄 때 `[]`로 답한다
fn hold_first_call(
    runner: &crate::test_support::ScriptedShellRunner,
) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (started, started_signal) = mpsc::channel::<()>();
    let (release, release_signal) = mpsc::channel::<()>();
    let started = Mutex::new(Some(started));
    let release_signal = Mutex::new(release_signal);
    runner.respond(move |_, _| {
        let started = started.lock().unwrap().take()?;
        started.send(()).unwrap();
        release_signal.lock().unwrap().recv().unwrap();
        Some(Ok("[]".to_string()))
    });
    (started_signal, release)
}

fn machine_add(server_name: &str, ssh_host: &str) -> String {
    let arguments = ["machine", "add", ssh_host, "--remote-session", server_name];
    format!(
        "{} </dev/null",
        build_herdr_command(server_name, &arguments).unwrap()
    )
}

// parity: TS-lib-herdr-c99f3c
#[test]
fn registers_the_real_server_session_as_a_local_machine() {
    for (session_name, server_name) in [("kanvibe:task-a", "kanvibe"), ("feature", "feature")] {
        let herdr = fixture();
        respond_in_sequence(
            &herdr.runner,
            vec![
                Ok("[]"),
                Ok(DETACHED_SERVER_STATUS),
                Ok("Saved SSH machine profile. Remote server is ready."),
            ],
        );

        herdr
            .service
            .ensure_herdr_remote_machine(session_name, "remote")
            .unwrap();

        let summary: Vec<(String, Option<String>, Option<Duration>, bool)> = herdr
            .runner
            .calls()
            .into_iter()
            .map(|call| {
                (
                    call.command,
                    call.ssh_host,
                    call.options.timeout,
                    call.options.is_retry_safe,
                )
            })
            .collect();
        let status = build_herdr_command(server_name, &["status", "server", "--json"]).unwrap();
        let five_seconds = Some(Duration::from_secs(5));
        let expected = vec![
            (machine_list(server_name), None, five_seconds, false),
            (status, Some("remote".to_string()), five_seconds, false),
            (
                machine_add(server_name, "remote"),
                None,
                Some(Duration::from_secs(30)),
                false,
            ),
        ];
        assert_eq!(summary, expected);
    }
}

// parity: TS-lib-herdr-3a2576
#[test]
fn keeps_an_existing_profile_name_and_enabled_state() {
    for enabled in [true, false] {
        let herdr = fixture();
        let machines = json!([{ "id": "opaque-id", "label": "사용자 이름", "target": "remote", "session": "kanvibe", "enabled": enabled }]);
        respond_in_sequence(&herdr.runner, vec![Ok(&machines.to_string())]);

        herdr
            .service
            .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
            .unwrap();

        assert_eq!(herdr.runner.calls().len(), 1);
    }
}

// parity: TS-lib-herdr-b6155f
#[test]
fn distinguishes_profiles_of_other_hosts_or_sessions() {
    for existing in [
        json!({ "target": "remote", "session": "default" }),
        json!({ "target": "other-remote", "session": "kanvibe" }),
    ] {
        let herdr = fixture();
        let machines = json!([existing]).to_string();
        respond_in_sequence(
            &herdr.runner,
            vec![Ok(&machines), Ok(DETACHED_SERVER_STATUS), Ok("")],
        );

        herdr
            .service
            .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
            .unwrap();

        assert!(
            herdr.runner.commands()[2]
                .contains("'machine' 'add' 'remote' '--remote-session' 'kanvibe'")
        );
    }
}

// parity: TS-lib-herdr-a21aa4
#[test]
fn concurrent_and_repeated_connections_register_once() {
    let herdr = fixture();
    let machines = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let catalog = Arc::clone(&machines);
    herdr.runner.respond(move |command, _| {
        if command.contains("'machine' 'list'") {
            return Some(Ok(json!(*catalog.lock().unwrap()).to_string()));
        }
        if command.contains("'status' 'server'") {
            return Some(Ok(DETACHED_SERVER_STATUS.to_string()));
        }
        catalog
            .lock()
            .unwrap()
            .push(json!({ "target": "remote", "session": "kanvibe" }));
        Some(Ok(String::new()))
    });

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            herdr
                .service
                .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
        });
        let second = scope.spawn(|| {
            herdr
                .service
                .ensure_herdr_remote_machine("kanvibe:task-b", "remote")
        });
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    });
    herdr
        .service
        .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
        .unwrap();

    assert_eq!(
        *machines.lock().unwrap(),
        vec![json!({ "target": "remote", "session": "kanvibe" })]
    );
    assert_eq!(
        count_containing(&herdr.runner.commands(), "'machine' 'add'"),
        1
    );
}

// parity: TS-lib-herdr-a8632e
#[test]
fn serializes_catalog_writes_across_hosts() {
    let herdr = fixture();
    respond_in_sequence(
        &herdr.runner,
        vec![
            Ok(DETACHED_SERVER_STATUS),
            Ok(""),
            Ok(r#"[{"target":"remote-a","session":"kanvibe"}]"#),
            Ok(DETACHED_SERVER_STATUS),
            Ok(""),
        ],
    );
    let (list_started_signal, release_list) = hold_first_call(&herdr.runner);

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            herdr
                .service
                .ensure_herdr_remote_machine("kanvibe:task-a", "remote-a")
        });
        list_started_signal.recv().unwrap();
        let second = scope.spawn(|| {
            herdr
                .service
                .ensure_herdr_remote_machine("kanvibe:task-b", "remote-b")
        });
        std::thread::sleep(Duration::from_millis(50));
        release_list.send(()).unwrap();
        first.join().unwrap().unwrap();
        second.join().unwrap().unwrap();
    });

    assert_eq!(
        herdr.runner.commands(),
        vec![
            machine_list("kanvibe"),
            build_herdr_command("kanvibe", &["status", "server", "--json"]).unwrap(),
            machine_add("kanvibe", "remote-a"),
            machine_list("kanvibe"),
            build_herdr_command("kanvibe", &["status", "server", "--json"]).unwrap(),
            machine_add("kanvibe", "remote-b"),
        ]
    );
}

// parity: TS-lib-herdr-a98099
#[test]
fn retries_on_the_next_request_after_a_lookup_or_add_failure() {
    for failure in ["lookup", "add"] {
        let herdr = fixture();
        let mut outputs = match failure {
            "add" => vec![Ok("[]"), Ok(DETACHED_SERVER_STATUS)],
            _ => Vec::new(),
        };
        outputs.extend([
            Err("machine failed"),
            Ok("[]"),
            Ok(DETACHED_SERVER_STATUS),
            Ok(""),
        ]);
        respond_in_sequence(&herdr.runner, outputs);

        let error = herdr
            .service
            .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
            .unwrap_err();
        assert!(error.to_string().contains("machine failed"));

        herdr
            .service
            .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
            .unwrap();
        assert!(
            herdr
                .runner
                .commands()
                .last()
                .unwrap()
                .contains("'machine' 'add' 'remote'")
        );
    }
}

// parity: TS-lib-herdr-29cf0d
#[test]
fn invalid_machine_list_output_never_writes_a_profile() {
    for output in ["invalid json", "{}"] {
        let herdr = fixture();
        respond_in_sequence(&herdr.runner, vec![Ok(output)]);

        assert!(
            herdr
                .service
                .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
                .is_err()
        );

        assert_eq!(herdr.runner.calls().len(), 1);
    }
}

// parity: TS-lib-herdr-e877ee
#[test]
fn re_registers_a_profile_the_user_deleted() {
    let herdr = fixture();
    respond_in_sequence(
        &herdr.runner,
        vec![
            Ok(r#"[{"target":"remote","session":"kanvibe"}]"#),
            Ok("[]"),
            Ok(DETACHED_SERVER_STATUS),
            Ok(""),
        ],
    );
    herdr
        .service
        .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
        .unwrap();

    herdr
        .service
        .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
        .unwrap();

    assert!(
        herdr
            .runner
            .commands()
            .last()
            .unwrap()
            .contains("'machine' 'add' 'remote'")
    );
}

// parity: TS-lib-herdr-788fe7
#[test]
fn a_server_started_by_an_older_kanvibe_requires_handoff_instead_of_registration() {
    // Given: 세션 리더가 아니지만 live handoff는 지원하는 원격 서버
    let herdr = fixture();
    let status =
        r#"{"running":true,"capabilities":{"detached_server_daemon":false,"live_handoff":true}}"#;
    respond_in_sequence(&herdr.runner, vec![Ok("[]"), Ok(status)]);

    // When
    let error = herdr
        .service
        .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
        .unwrap_err();

    // Then: Herdr가 거부할 machine add는 실행하지 않는다
    assert_eq!(
        error,
        HerdrError::ServerHandoffRequired {
            ssh_host: "remote".into(),
            herdr_session: "kanvibe".into()
        }
    );
    assert_eq!(
        count_containing(&herdr.runner.commands(), "'machine' 'add'"),
        0
    );
}

// parity: TS-lib-herdr-dbfffe
#[test]
fn servers_that_cannot_be_handed_off_still_try_registration() {
    let statuses = [
        r#"{"running":true,"capabilities":{"detached_server_daemon":false,"live_handoff":false}}"#,
        r#"{"running":false,"capabilities":null}"#,
    ];
    for status in statuses {
        let herdr = fixture();
        respond_in_sequence(&herdr.runner, vec![Ok("[]"), Ok(status), Ok("")]);

        herdr
            .service
            .ensure_herdr_remote_machine("kanvibe:task-a", "remote")
            .unwrap();

        assert!(
            herdr
                .runner
                .commands()
                .last()
                .unwrap()
                .contains("'machine' 'add' 'remote'")
        );
    }
}

// parity: TS-lib-herdr-b18326
#[test]
fn server_handoff_runs_herdr_live_handoff_on_the_remote() {
    let herdr = fixture();
    respond_in_sequence(&herdr.runner, vec![Ok("live handoff complete")]);

    herdr
        .service
        .handoff_herdr_remote_server("kanvibe", "remote")
        .unwrap();

    let calls = herdr.runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].command,
        build_herdr_command("kanvibe", &["server", "live-handoff"]).unwrap()
    );
    assert_eq!(calls[0].ssh_host.as_deref(), Some("remote"));
    assert_eq!(calls[0].options.timeout, Some(Duration::from_secs(30)));
}
