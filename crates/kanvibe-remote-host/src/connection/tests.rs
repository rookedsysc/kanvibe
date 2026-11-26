mod endpoint;
mod fake_remote;
mod integration;
mod pty_attachment;

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use kanvibe_remote_agent::protocol::{HOST_CHUNK_BYTES, decode_host_bytes, encode_host_bytes};
use serde_json::json;

use super::*;
use fake_remote::FakeRemote;

fn connect_fake() -> (Arc<HostConnection>, FakeRemote) {
    let (remote, spawner, catalog) = FakeRemote::start();
    let connection = connect_through_loader(
        &spawner,
        &fake_remote::fixture_config(),
        &catalog,
        "qa-client",
        "client",
    )
    .unwrap()
    .expect("fake bridge");
    (connection, remote)
}

#[test]
fn socket_forward_keeps_sending_while_acks_are_pending_and_drains_before_end() {
    let (connection, remote) = connect_fake();
    let forward = connection.forward_socket("/remote/herdr.sock").unwrap();
    let mut local = UnixStream::connect(forward.path()).unwrap();
    let id = remote.wait_for("socket.open", 1)[0]["id"].clone();
    remote.send(json!({ "type": "socket.ready", "id": id }));

    let bytes: Vec<_> = (0..HOST_CHUNK_BYTES * 9)
        .map(|index| (index % 251) as u8)
        .collect();
    let written = bytes.clone();
    let writer = std::thread::spawn(move || {
        local.write_all(&written).unwrap();
        local.shutdown(Shutdown::Write).unwrap();
    });

    let mut sent_chunks = 8;
    let mut received: Vec<_> = remote
        .wait_for("socket.data", sent_chunks)
        .iter()
        .flat_map(|record| decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES).unwrap())
        .collect();
    assert_eq!(remote.records_of("socket.data").len(), sent_chunks);
    while received.len() < bytes.len() {
        remote.send(json!({ "type": "socket.data-ready", "id": id }));
        sent_chunks += 1;
        let records = remote.wait_for("socket.data", sent_chunks);
        received.extend(
            decode_host_bytes(records.last().unwrap().get("data"), HOST_CHUNK_BYTES).unwrap(),
        );
    }
    writer.join().unwrap();
    assert_eq!(received, bytes);
    assert!(remote.records_of("socket.end").is_empty());
    for _ in 0..8 {
        remote.send(json!({ "type": "socket.data-ready", "id": id }));
    }
    remote.wait_for("socket.end", 1);
    remote.send(json!({ "type": "socket.close", "id": id }));
    connection.close();
}

/// 로컬 연결을 열고 원격 소켓이 준비됐다고 알린다. 돌려준 id로 `socket.data`를 보낸다
#[test]
fn a_finished_local_client_is_dropped_from_the_open_streams() {
    let (connection, remote) = connect_fake();
    let forward = connection.forward_socket("/remote/herdr.sock").unwrap();
    let local = UnixStream::connect(forward.path()).unwrap();
    let id = remote.wait_for("socket.open", 1)[0]["id"].clone();
    remote.send(json!({ "type": "socket.ready", "id": id }));
    assert_eq!(forward.open_local_stream_count(), 1);

    drop(local);
    remote.wait_for("socket.end", 1);
    remote.send(json!({ "type": "socket.close", "id": id }));
    remote.wait_for("socket.close", 1);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while forward.open_local_stream_count() > 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "닫힌 로컬 연결이 남았다"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    connection.close();
}

fn open_ready_forward(
    connection: &Arc<HostConnection>,
    remote: &FakeRemote,
) -> (Arc<HostSocketForward>, UnixStream, serde_json::Value) {
    let forward = connection.forward_socket("/remote/herdr.sock").unwrap();
    let local = UnixStream::connect(forward.path()).unwrap();
    let id = remote.wait_for("socket.open", 1)[0]["id"].clone();
    remote.send(json!({ "type": "socket.ready", "id": id }));
    (forward, local, id)
}

/// 원격이 로컬 연결로 `chunks`개의 조각을 보낸 뒤 명령 하나를 끝낸다. 명령 결과가 돌아온 시점에는
/// 수신 스레드가 앞선 조각을 모두 처리한 것이다
fn flood_then_finish_command(
    connection: &Arc<HostConnection>,
    remote: FakeRemote,
    id: serde_json::Value,
    chunks: usize,
) -> (
    std::sync::mpsc::Receiver<String>,
    std::thread::JoinHandle<FakeRemote>,
) {
    let (finished, finished_receiver) = std::sync::mpsc::channel();
    let command = {
        let connection = Arc::clone(connection);
        std::thread::spawn(move || {
            let output = connection.execute("after-flood", &HostExecuteOptions::default());
            let _ = finished.send(output.unwrap());
        })
    };
    let command_id = remote.wait_for("exec.open", 1)[0]["id"].clone();
    let chunk = encode_host_bytes(&vec![b'x'; HOST_CHUNK_BYTES]);
    let sending = std::thread::spawn(move || {
        for _ in 0..chunks {
            remote.send(json!({ "type": "socket.data", "id": id, "data": chunk }));
        }
        remote.send(json!({ "type": "exec.data", "id": command_id, "stream": "stdout", "data": encode_host_bytes(b"done") }));
        remote.send(json!({ "type": "exec.exit", "id": command_id, "code": 0, "signal": null }));
        remote
    });
    drop(command);
    (finished_receiver, sending)
}

#[test]
fn a_local_client_that_stops_reading_does_not_block_other_records_of_the_host() {
    let (connection, remote) = connect_fake();
    let (_forward, _local, id) = open_ready_forward(&connection, &remote);

    let (finished, sending) = flood_then_finish_command(&connection, remote, id, 64);

    assert_eq!(
        finished
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap(),
        "done"
    );
    sending.join().unwrap();
    connection.close();
}

#[test]
fn a_local_client_that_falls_too_far_behind_is_disconnected_alone() {
    use std::io::Read;
    let (connection, remote) = connect_fake();
    let (_forward, mut local, id) = open_ready_forward(&connection, &remote);
    let chunks = kanvibe_remote_agent::protocol::HOST_BUFFER_BYTES / HOST_CHUNK_BYTES + 32;

    let (finished, sending) = flood_then_finish_command(&connection, remote, id, chunks);
    assert_eq!(
        finished
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap(),
        "done"
    );
    sending.join().unwrap();

    local
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let mut received = Vec::new();
    local.read_to_end(&mut received).unwrap();
    assert!(received.len() < chunks * HOST_CHUNK_BYTES);
    connection.close();
}

#[test]
fn concurrent_commands_are_correlated_and_unicode_split_across_chunks_is_kept() {
    let (connection, remote) = connect_fake();
    let first = {
        let connection = Arc::clone(&connection);
        std::thread::spawn(move || connection.execute("first", &HostExecuteOptions::default()))
    };
    let opens = remote.wait_for("exec.open", 1);
    let second = {
        let connection = Arc::clone(&connection);
        std::thread::spawn(move || connection.execute("second", &HostExecuteOptions::default()))
    };
    let opens = [opens, remote.wait_for("exec.open", 2)].concat();
    let first_id = opens
        .iter()
        .find(|record| record["command"] == "first")
        .unwrap()["id"]
        .clone();
    let second_id = opens
        .iter()
        .find(|record| record["command"] == "second")
        .unwrap()["id"]
        .clone();
    let bytes = "한글\u{0}".as_bytes();

    for part in [&bytes[..1], &bytes[1..]] {
        remote.send(json!({ "type": "exec.data", "id": first_id, "stream": "stdout", "data": encode_host_bytes(part) }));
    }
    remote.send(json!({ "type": "exec.exit", "id": second_id, "code": 0, "signal": null }));
    remote.send(json!({ "type": "exec.exit", "id": first_id, "code": 0, "signal": null }));

    assert_eq!(first.join().unwrap().unwrap(), "한글\u{0}");
    assert_eq!(second.join().unwrap().unwrap(), "");
}

#[test]
fn a_disconnect_rejects_an_unknown_command_outcome_without_replay() {
    let (connection, remote) = connect_fake();
    let result = {
        let connection = Arc::clone(&connection);
        std::thread::spawn(move || connection.execute("write-once", &HostExecuteOptions::default()))
    };
    remote.wait_for("exec.open", 1);

    remote.disconnect();

    let error = result.join().unwrap().unwrap_err();
    assert!(
        error.message.to_lowercase().contains("disconnected"),
        "{}",
        error.message
    );
    assert!(!error.has_remote_command_completed);
    assert_eq!(remote.records_of("exec.open").len(), 1);
}

#[test]
fn a_pty_open_refused_by_a_closed_connection_ends_its_delivery_thread() {
    let (connection, _remote) = connect_fake();
    connection.close();
    let (exit_sender, exits) = std::sync::mpsc::channel();
    let callbacks = kanvibe_terminal::pty::PtyEventCallbacks {
        on_output: Box::new(|_| {}),
        on_exit: Box::new(move |exit| {
            let _ = exit_sender.send(exit);
        }),
    };

    let result = connection.open_pty("key", "true", 80, 24, callbacks);

    assert!(result.is_err());
    let exit = exits
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("delivery thread finished");
    assert!(exit.is_detached);
}

struct FixtureHostOnly;

impl crate::hook_tunnel::SshHostLookup for FixtureHostOnly {
    fn find_host(&self, ssh_host: &str) -> Option<kanvibe_process::ssh_config::SshHostConfig> {
        (ssh_host == "fixture").then(fake_remote::fixture_config)
    }
}

struct FixedEffectiveConfig;

impl crate::ssh_transport::EffectiveSshConfig for FixedEffectiveConfig {
    fn resolve(&self, _: &[String]) -> Result<String, String> {
        Ok("user fixture\n".into())
    }
}

#[test]
fn a_runtime_command_for_a_host_with_a_daemon_goes_through_the_daemon_connection() {
    let (remote, spawner, catalog) = FakeRemote::start();
    let pool = crate::pool::RemoteHostConnectionPool::new(
        crate::pool::RemoteHostConnectionPoolDependencies {
            ssh_hosts: Arc::new(FixtureHostOnly),
            binaries: Arc::new(catalog),
            transport_spawner: Arc::new(spawner),
            effective_ssh_config: Arc::new(FixedEffectiveConfig),
        },
    );
    pool.configure_transport("qa-client", "client").unwrap();
    let home = tempfile::tempdir().unwrap();
    let system_runner = Arc::new(
        kanvibe_process::exec::system::SystemShellCommandRunner::new(
            kanvibe_process::shell_environment::ShellEnvironment::new(),
            home.path().to_str().unwrap(),
            kanvibe_process::exec::remote::RemoteSshSettings::default(),
        ),
    );
    let runner = crate::command_runner::DaemonRoutedCommandRunner::new(system_runner, pool);
    use kanvibe_process::exec::ShellCommandRunner;
    let runner = Arc::new(runner);

    let remote_result = {
        let runner = Arc::clone(&runner);
        std::thread::spawn(move || {
            runner.exec_git("git status", Some("fixture"), &Default::default())
        })
    };
    let open = remote.wait_for("exec.open", 1)[0].clone();
    assert_eq!(open["command"], "git status");
    remote.send(json!({ "type": "exec.data", "id": open["id"], "stream": "stdout", "data": encode_host_bytes(b"clean") }));
    remote.send(json!({ "type": "exec.exit", "id": open["id"], "code": 0, "signal": null }));

    assert_eq!(remote_result.join().unwrap().unwrap(), "clean");
    assert_eq!(
        runner
            .exec_git("printf local", None, &Default::default())
            .unwrap(),
        "local"
    );
    assert_eq!(remote.records_of("exec.open").len(), 1);
}

#[test]
fn completed_remote_errors_carry_stdout_and_stderr() {
    let (connection, remote) = connect_fake();
    let result = {
        let connection = Arc::clone(&connection);
        std::thread::spawn(move || connection.execute("false", &HostExecuteOptions::default()))
    };
    let id = remote.wait_for("exec.open", 1)[0]["id"].clone();

    remote.send(json!({ "type": "exec.data", "id": id, "stream": "stderr", "data": encode_host_bytes(b"denied") }));
    remote.send(json!({ "type": "exec.exit", "id": id, "code": 7, "signal": null }));

    let error = result.join().unwrap().unwrap_err();
    assert_eq!(error.exit_code, Some(7));
    assert_eq!(error.stderr, "denied");
    assert!(error.has_remote_command_completed);
    assert_eq!(remote.records_of("exec.open")[0]["command"], "false");
}

#[test]
fn the_daemons_liveness_probe_is_answered_with_the_same_request_id() {
    let (_connection, remote) = connect_fake();

    remote.send(json!({ "type": "ping", "id": "fixture-probe" }));

    assert_eq!(remote.wait_for("pong", 1)[0]["id"], "fixture-probe");
}

fn connect_with_handshake(
    handshake: impl FnOnce(&str) -> serde_json::Value,
) -> Result<Option<Arc<HostConnection>>, RemoteHostConnectionError> {
    let (_remote, spawner, catalog) = FakeRemote::start_with_handshake(handshake);
    connect_through_loader(
        &spawner,
        &fake_remote::fixture_config(),
        &catalog,
        "qa-client",
        "client",
    )
}

#[test]
fn the_daemons_exact_rejection_reason_is_returned() {
    let error = connect_with_handshake(|_| {
        json!({ "type": "error", "error": "A desktop with this client ID is already connected", "code": "client-already-connected" })
    })
    .err()
    .expect("daemon이 거절했다");

    assert_eq!(error.kind, RemoteHostConnectionErrorKind::Handshake);
    assert_eq!(error.code.as_deref(), Some("client-already-connected"));
    assert_eq!(
        error.message,
        "A desktop with this client ID is already connected"
    );
}

#[test]
fn a_mismatched_ready_field_is_named_in_the_error() {
    let mismatches = [
        ("version", json!(1)),
        ("namespace", json!("dev")),
        ("buildHash", json!("old-build")),
        ("capabilities", json!(["exec"])),
    ];

    for (field, value) in mismatches {
        let error = connect_with_handshake(|hash| {
            let mut ready = fake_remote::fixture_ready(hash);
            ready[field] = value;
            ready
        })
        .err()
        .expect("ready가 맞지 않는다");
        assert_eq!(
            error.kind,
            RemoteHostConnectionErrorKind::Handshake,
            "{field}"
        );
        assert!(error.message.contains(field), "{field}: {}", error.message);
    }
}

#[test]
fn an_ssh_exit_before_bootstrap_is_classified_and_keeps_its_stderr() {
    let (_binary_directory, catalog) = fake_remote::fixture_catalog();
    let spawner = fake_remote::ssh_exit_before_bootstrap(
        "ssh: connect to host example port 22: Operation timed out\n",
    );

    let error = connect_through_loader(
        &spawner,
        &fake_remote::fixture_config(),
        &catalog,
        "qa-client",
        "client",
    )
    .err()
    .expect("ssh가 끝났다");

    assert_eq!(error.kind, RemoteHostConnectionErrorKind::Ssh);
    assert!(
        error.message.contains("Operation timed out"),
        "{}",
        error.message
    );
}

#[test]
fn a_created_process_streams_stdin_and_stdout_through_the_daemon_records() {
    let (connection, remote) = connect_fake();
    let mut process = connection
        .create_process("sh -lc 'agent'", std::time::Duration::from_secs(60))
        .unwrap();
    let open = remote.wait_for("exec.open", 1)[0].clone();
    let id = open["id"].clone();
    assert_eq!(open["command"], "sh -lc 'agent'");
    assert_eq!(open["interactive"], true);

    let writer = std::thread::spawn(move || {
        process.stdin.write_all(b"bootstrap\n").unwrap();
        process
    });
    let stdin_record = remote.wait_for("exec.stdin", 1)[0].clone();
    assert_eq!(
        decode_host_bytes(stdin_record.get("data"), HOST_CHUNK_BYTES).unwrap(),
        b"bootstrap\n"
    );
    remote.send(json!({ "type": "exec.stdin-ready", "id": id }));
    let mut process = writer.join().unwrap();

    remote.send(json!({ "type": "exec.data", "id": id, "stream": "stderr", "data": encode_host_bytes(b"noise") }));
    remote.send(json!({ "type": "exec.data", "id": id, "stream": "stdout", "data": encode_host_bytes(b"{\"type\":\"ready\"}\n") }));
    let mut line = [0u8; 17];
    process.stdout.read_exact(&mut line).unwrap();
    assert_eq!(&line, b"{\"type\":\"ready\"}\n");

    drop(process.stdin);
    let end = remote.wait_for("exec.stdin", 2)[1].clone();
    assert_eq!(end["end"], true);
    remote.send(json!({ "type": "exec.exit", "id": id, "code": 0 }));
    let mut rest = Vec::new();
    process.stdout.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
    process.termination.terminate();
    assert!(remote.records_of("exec.cancel").is_empty());
    connection.close();
}

#[test]
fn terminating_a_running_process_cancels_it_and_ends_its_stdout() {
    let (connection, remote) = connect_fake();
    let mut process = connection
        .create_process("sleep 100", std::time::Duration::from_secs(60))
        .unwrap();
    let id = remote.wait_for("exec.open", 1)[0]["id"].clone();

    process.termination.terminate();

    assert_eq!(remote.wait_for("exec.cancel", 1)[0]["id"], id);
    let mut rest = Vec::new();
    process.stdout.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
    assert!(process.stdin.write_all(b"late").is_err());
    connection.close();
}
