//! TS `hookTunnelService.test.ts`, `hostConnectionStatusSync.test.ts`의 Rust 판.
//! 프로세스 밖으로 나가는 경계(ssh, 원격 명령, daemon 연결, 요청 서버)만 가짜로 바꾸고 TS 가짜 타이머 대신 짧은 주기를 쓴다.

mod fakes;

use std::collections::HashSet;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kanvibe_db::{Database, DatabaseLocation};
use kanvibe_domain::project::Project;
use kanvibe_domain::task::{KanbanTask, TaskStatus};
use kanvibe_process::request_scope::is_task_reachable_from_tunnel;
use kanvibe_process::shell_environment::ShellEnvironment;

use self::fakes::*;
use super::*;

const REMOTE_SOCKET: &str = "/tmp/kv-1000/1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed.sock";

// parity: TS-main-hookTunnelService-a838cf
#[test]
fn tunnel_arguments_surface_forward_failures_and_end_with_the_app() {
    let base_args: Vec<String> = ["-p", "22", "host-a"].map(String::from).to_vec();

    let arguments = build_hook_tunnel_ssh_args(&base_args, "/remote.sock", "/local.sock");

    let expected = [
        "-p",
        "22",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "StreamLocalBindUnlink=yes",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "StrictHostKeyChecking=yes",
        "-R",
        "/remote.sock:/local.sock",
        "host-a",
        "cat >/dev/null",
    ];
    assert_eq!(arguments, expected);
}

// parity: TS-main-hookTunnelService-5116b7
#[test]
fn no_tunnel_opens_before_the_service_starts() {
    let fixture = Fixture::new(&["host-a"]);

    fixture.service.ensure_hook_tunnel("host-a");
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(fixture.spawner.spawn_count(), 0);
}

// parity: TS-main-hookTunnelService-ac5d09
#[test]
fn aliases_of_one_remote_account_share_a_tunnel_that_serves_both() {
    let fixture = Fixture::new(&["host-a", "host-a-alias"]);

    fixture.start();
    fixture.spawner.wait_for_spawns(1);
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(fixture.spawner.spawn_count(), 1);
    let mut hosts: Vec<String> = fixture.server.host_sets()[0]
        .read()
        .unwrap()
        .iter()
        .cloned()
        .collect();
    hosts.sort();
    assert_eq!(hosts, ["host-a", "host-a-alias"]);
}

// parity: TS-main-hookTunnelService-c3db40
#[test]
fn different_remote_accounts_open_separate_tunnels() {
    let fixture = Fixture::new(&["host-a", "host-b"]);

    fixture.start();

    fixture.spawner.wait_for_spawns(2);
}

// parity: TS-main-hookTunnelService-c3e97f
#[test]
fn a_host_with_only_done_tasks_keeps_its_tunnel_for_worktree_hooks() {
    let database = DatabaseFixture::new();
    database.execute("INSERT INTO kanban_tasks (id, title, status, ssh_host) VALUES ('done', 'done', 'done', 'host-a')");
    let fixture = Fixture::with_remote_hosts(
        Arc::new(DatabaseRemoteHosts::new(database.shared())),
        FAST_TIMINGS,
    );

    fixture.start();

    fixture.spawner.wait_for_spawns(1);
}

// parity: TS-main-hookTunnelService-71476e
#[test]
fn a_host_with_a_remote_project_but_no_tasks_gets_a_tunnel() {
    let database = DatabaseFixture::new();
    database.execute(
        "INSERT INTO projects (id, name, repo_path, ssh_host) VALUES ('p', 'p', '/repo', 'host-b')",
    );
    let fixture = Fixture::with_remote_hosts(
        Arc::new(DatabaseRemoteHosts::new(database.shared())),
        FAST_TIMINGS,
    );

    fixture.start();

    fixture.spawner.wait_for_spawns(1);
}

// parity: TS-main-hookTunnelService-134b99
#[test]
fn a_tunnel_opens_for_a_remote_task_and_closes_when_no_task_remains() {
    let timings = TunnelTimings {
        reconcile_interval: Duration::from_millis(200),
        ..FAST_TIMINGS
    };
    let remote_hosts = Arc::new(FakeRemoteHosts::new(&["host-a"]));
    let fixture = Fixture::with_remote_hosts(remote_hosts.clone(), timings);
    fixture.start();
    fixture.spawner.wait_for_spawns(1);
    assert!(fixture.spawner.arguments(0).contains(&"-R".to_string()));

    remote_hosts.replace(&[]);

    wait_until("터널이 닫힘", || {
        fixture.spawner.process(0).kill_count() > 0
    });
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(fixture.spawner.spawn_count(), 1);
}

// parity: TS-main-hookTunnelService-021d9c
#[test]
fn requesting_the_same_host_again_keeps_one_tunnel() {
    let fixture = Fixture::new(&["host-a"]);
    fixture.start();
    fixture.spawner.wait_for_spawns(1);

    fixture.service.ensure_hook_tunnel("host-a");
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(fixture.spawner.spawn_count(), 1);
}

// parity: TS-main-hookTunnelService-fed6bc
#[test]
fn a_dropped_tunnel_reconnects_and_backs_off_when_it_drops_again_quickly() {
    let fixture = Fixture::new(&["host-a"]);
    fixture.start();
    fixture.spawner.wait_for_spawns(1);

    let first_drop = Instant::now();
    fixture.spawner.process(0).exit(Some(255));
    fixture.spawner.wait_for_spawns(2);
    let first_wait = first_drop.elapsed();
    let second_drop = Instant::now();
    fixture.spawner.process(1).exit(Some(255));
    fixture.spawner.wait_for_spawns(3);
    let second_wait = second_drop.elapsed();

    let minimum = FAST_TIMINGS.reconnect_min_delay;
    assert!(
        first_wait >= minimum && first_wait < minimum * 2,
        "{first_wait:?}"
    );
    assert!(second_wait >= minimum * 2, "{second_wait:?}");
}

// parity: TS-main-hookTunnelService-638b48
#[test]
fn a_failed_prepare_command_does_not_stop_the_app_and_is_retried() {
    let fixture = Fixture::new(&["host-a"]);
    fixture.runner.fail_next("다른 사용자의 디렉터리");
    fixture.start();
    wait_until("준비 명령 실행", || fixture.runner.call_count() == 1);
    assert_eq!(fixture.spawner.spawn_count(), 0);

    fixture.spawner.wait_for_spawns(1);

    assert_eq!(fixture.runner.call_count(), 2);
}

// parity: TS-main-hookTunnelService-5dee43
#[test]
fn a_socket_path_of_another_client_does_not_open_a_tunnel() {
    let fixture = Fixture::new(&["host-a"]);
    fixture
        .runner
        .respond_with("/tmp/kv-1000/another-client.sock");

    fixture.start();
    wait_until("준비 명령 실행", || fixture.runner.call_count() >= 1);
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(fixture.spawner.spawn_count(), 0);
    let error =
        ssh_args::validate_remote_socket_path("/tmp/kv-1000/another-client.sock", CLIENT_ID)
            .unwrap_err();
    assert!(error.contains("소켓 경로"), "{error}");
}

// parity: TS-main-hookTunnelService-068df1
#[test]
fn waking_from_sleep_drops_a_live_looking_connection_and_reconnects() {
    let fixture = Fixture::new(&["host-a"]);
    fixture.start();
    fixture.spawner.wait_for_spawns(1);

    fixture.service.reconnect_hook_tunnels();

    assert_eq!(fixture.spawner.process(0).kill_count(), 1);
    fixture.spawner.wait_for_spawns(2);
}

// parity: TS-main-hookTunnelService-314ab8
#[test]
fn stopping_closes_tunnels_without_reconnecting() {
    let fixture = Fixture::new(&["host-a"]);
    fixture.start();
    fixture.spawner.wait_for_spawns(1);

    fixture.service.stop();
    std::thread::sleep(FAST_TIMINGS.reconnect_min_delay * 3);

    assert!(fixture.spawner.process(0).kill_count() > 0);
    assert_eq!(fixture.spawner.spawn_count(), 1);
    assert_eq!(fixture.transport.stop_count(), 1);
}

fn reachability_task(ssh_host: Option<&str>) -> KanbanTask {
    KanbanTask {
        id: "task".into(),
        title: "task".into(),
        description: None,
        status: TaskStatus::Todo,
        branch_name: None,
        worktree_path: None,
        session_type: None,
        session_name: None,
        ssh_host: ssh_host.map(String::from),
        agent_type: None,
        project_id: None,
        base_branch: None,
        pr_url: None,
        priority: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn reachability_project(ssh_host: &str) -> Project {
    Project {
        id: "project".into(),
        name: "project".into(),
        repo_path: "/repo".into(),
        default_branch: "main".into(),
        ssh_host: Some(ssh_host.into()),
        is_worktree: false,
        color: None,
        icon_data_url: None,
        created_at: String::new(),
    }
}

/// (설명, task, 프로젝트, 요청이 들어온 터널의 호스트, 기대값)
type ReachabilityCase<'case> = (
    &'case str,
    KanbanTask,
    Option<&'case Project>,
    Option<&'case HashSet<String>>,
    bool,
);

// parity: TS-main-hookTunnelService-cbe28a
#[test]
fn tunnel_requests_reach_only_tasks_of_the_tunnel_hosts() {
    let host_a = HashSet::from(["host-a".to_string()]);
    let project_a = reachability_project("host-a");
    let cases: [ReachabilityCase; 5] = [
        (
            "평문 포트의 요청은 호스트를 묻지 않는다",
            reachability_task(Some("host-b")),
            None,
            None,
            true,
        ),
        (
            "터널이 맡은 호스트의 task",
            reachability_task(Some("host-a")),
            None,
            Some(&host_a),
            true,
        ),
        (
            "다른 호스트의 task",
            reachability_task(Some("host-b")),
            None,
            Some(&host_a),
            false,
        ),
        (
            "sshHost가 비어 있는 옛 task는 프로젝트 호스트로 판정한다",
            reachability_task(None),
            Some(&project_a),
            Some(&host_a),
            true,
        ),
        (
            "호스트를 알 수 없는 task는 터널로 다루지 못한다",
            reachability_task(None),
            None,
            Some(&host_a),
            false,
        ),
    ];

    for (name, task, project, source_ssh_hosts, expected) in cases {
        assert_eq!(
            is_task_reachable_from_tunnel(&task, project, source_ssh_hosts),
            expected,
            "{name}"
        );
    }
}

fn read_stored_status(database: &DatabaseFixture) -> String {
    database
        .database
        .connection()
        .query_row(
            "SELECT status FROM kanban_tasks WHERE id = 'fixture'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

// parity: TS-main-hostConnectionStatusSync-aa8433
#[test]
fn connecting_reflects_the_remote_status_file_and_reconnecting_reflects_the_latest_value() {
    let database = DatabaseFixture::new();
    database.execute("INSERT INTO kanban_tasks (id, title, status, ssh_host, worktree_path) VALUES ('fixture', 'fixture', 'progress', 'host-a', '/fixture')");
    let remote_file = Arc::new(FakeRunner::new(r#"{"schemaVersion":1,"status":"review"}"#));
    let fixture = Fixture::with_daemon_status_sync(&database, Arc::clone(&remote_file));

    fixture.service.start(Some("qa-sync")).unwrap();

    wait_until("review 반영", || {
        read_stored_status(&database) == "review"
    });
    assert_eq!(
        fixture.transport.configured(),
        [("qa-sync".to_string(), CLIENT_ID.to_string())]
    );
    remote_file.respond_with(r#"{"schemaVersion":1,"status":"done"}"#);
    fixture.transport.connection_at(0).close();
    wait_until("done 반영", || read_stored_status(&database) == "done");
    assert!(fixture.transport.connection_count() >= 2);
}

#[test]
fn daemon_hook_requests_are_replayed_to_the_tunnel_socket() {
    let fixture = Fixture::with_daemon_transport();
    fixture.service.start(Some("qa-replay")).unwrap();
    wait_until("daemon 연결", || {
        fixture.transport.connection_count() == 1
    });
    let connection = fixture.transport.connection_at(0);
    wait_until("hook 처리기 설정", || connection.handler().is_some());

    let request = HostHookRequest {
        method: "POST".into(),
        path: "/api/hooks/status".into(),
        headers: vec![("content-type".into(), "application/json".into())],
        body: br#"{"taskId":"fixture"}"#.to_vec(),
    };
    let response = connection.handler().unwrap()(request).unwrap();

    assert_eq!(response.status, 202);
    assert_eq!(response.body, b"ack");
    let received = fixture.server.received_requests().join("\n");
    assert!(
        received.starts_with("POST /api/hooks/status HTTP/1.1"),
        "{received}"
    );
    assert!(received.ends_with(r#"{"taskId":"fixture"}"#), "{received}");
}

#[test]
fn replayed_responses_over_the_record_limit_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    let socket_path = directory.path().join("hook.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0u8; 1024];
        let _ = stream.read(&mut buffer);
        let body = vec![b'x'; 512 * 1024 + 1];
        let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n", body.len());
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
    });
    let request = HostHookRequest {
        method: "GET".into(),
        path: "/api/hooks/health".into(),
        headers: Vec::new(),
        body: Vec::new(),
    };

    let error = replay_hook_request(&socket_path, &request).unwrap_err();

    assert_eq!(error, "Hook response too large");
}

#[test]
fn tunnel_ssh_children_do_not_inherit_server_runtime_variables() {
    let parent: ShellEnvironment = [
        ("PORT", "4000"),
        ("HOST", "0.0.0.0"),
        ("NODE_ENV", "production"),
        ("KANVIBE_HOOK_TOKEN", "secret"),
        ("HOME", "/home/fixture"),
        ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect();

    let spawner = SystemHookTunnelProcessSpawner::from_parent_environment(&parent);

    for key in ["PORT", "HOST", "NODE_ENV", "KANVIBE_HOOK_TOKEN"] {
        assert!(!spawner.environment().contains_key(key), "{key} leaked");
    }
    assert_eq!(spawner.environment()["SSH_AUTH_SOCK"], "/tmp/agent.sock");
}

#[test]
fn an_empty_database_lists_no_remote_hosts() {
    let database = DatabaseFixture::new();
    database.execute(
        "INSERT INTO kanban_tasks (id, title, status) VALUES ('local', 'local', 'progress')",
    );

    let hosts = DatabaseRemoteHosts::new(database.shared())
        .remote_ssh_hosts()
        .unwrap();

    assert!(hosts.is_empty());
}

/// 실제 DB 파일을 임시 디렉터리에 만든다
struct DatabaseFixture {
    _directory: tempfile::TempDir,
    database: Arc<Database>,
}

impl DatabaseFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let location = DatabaseLocation::in_app_data_directory(directory.path(), None);
        let database = Arc::new(Database::open(&location).unwrap());
        Self {
            _directory: directory,
            database,
        }
    }

    fn shared(&self) -> Arc<Database> {
        Arc::clone(&self.database)
    }

    fn execute(&self, sql: &str) {
        self.database.connection().execute(sql, []).unwrap();
    }
}
