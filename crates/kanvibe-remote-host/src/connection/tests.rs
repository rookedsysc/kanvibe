mod endpoint;
mod fake_remote;
mod integration;
mod pty_attachment;

use std::sync::Arc;

use kanvibe_remote_agent::protocol::encode_host_bytes;
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

// parity: TS-lib-remoteHostConnection-72109c
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

// parity: TS-lib-remoteHostConnection-1654c7
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

// parity: TS-lib-remoteHostConnection-c8536b
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
