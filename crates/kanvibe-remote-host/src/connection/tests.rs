mod endpoint;
mod fake_remote;
mod integration;
mod pty_attachment;

use std::io::Write;
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
