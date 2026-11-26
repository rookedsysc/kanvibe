use std::io::{Read, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::protocol::{
    HOST_CHUNK_BYTES, HostRecordDecoder, decode_host_bytes, encode_host_bytes, encode_host_record,
    record_text,
};

const NAMESPACE: &str = "qa-provider";

fn build_hash() -> String {
    "a".repeat(64)
}

struct ServerFixture {
    root: tempfile::TempDir,
    server: HostServer,
}

fn temp_root() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("kvhp-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    root
}

fn start_in(root: &Path) -> Result<HostServer, String> {
    HostServer::start(HostServerOptions {
        namespace: NAMESPACE.into(),
        build_hash: build_hash(),
        root_directory: Some(root.to_path_buf()),
        pty_launcher: Arc::new(pty_launcher::NativePtyLauncher),
    })
}

fn start() -> ServerFixture {
    let root = temp_root();
    let server = start_in(root.path()).unwrap();
    ServerFixture { root, server }
}

/// rpc 소켓에 붙은 테스트용 데스크탑
struct WireClient {
    stream: UnixStream,
    records: Receiver<HostRecord>,
    held: Vec<HostRecord>,
}

impl Drop for WireClient {
    /// 읽기 스레드가 소켓 사본을 쥐고 있으므로 연결을 직접 끊는다
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

impl WireClient {
    fn connect(server: &HostServer, client_id: &str, hello: Value) -> Self {
        let stream = UnixStream::connect(&server.paths.rpc_socket_path).unwrap();
        let (sender, records) = channel();
        let mut reader = stream.try_clone().unwrap();
        std::thread::spawn(move || {
            let mut decoder = HostRecordDecoder::new();
            let mut buffer = vec![0; 64 * 1024];
            while let Ok(count) = reader.read(&mut buffer) {
                let Ok(decoded) = decoder
                    .push(&buffer[..count])
                    .map_err(|_| ())
                    .and_then(|records| (count > 0).then_some(records).ok_or(()))
                else {
                    return;
                };
                decoded
                    .into_iter()
                    .for_each(|record| drop(sender.send(record)));
            }
        });
        let mut client = Self {
            stream,
            records,
            held: Vec::new(),
        };
        let mut greeting = json!({ "type": "hello", "version": 2, "namespace": NAMESPACE, "clientId": client_id, "role": "desktop", "buildHash": build_hash() });
        for (key, value) in hello.as_object().cloned().unwrap_or_default() {
            greeting[key] = value;
        }
        client.send(greeting);
        client
    }

    fn ready(server: &HostServer, client_id: &str) -> (Self, HostRecord) {
        let mut client = Self::connect(server, client_id, json!({}));
        let ready = client.next("ready");
        (client, ready)
    }

    fn send(&mut self, record: Value) {
        self.stream
            .write_all(&encode_host_record(&record_from(record)).unwrap())
            .unwrap();
    }

    fn next(&mut self, kind: &str) -> HostRecord {
        if let Some(index) = self
            .held
            .iter()
            .position(|record| record_text(record, "type") == Some(kind))
        {
            return self.held.remove(index);
        }
        loop {
            let record = self
                .records
                .recv_timeout(Duration::from_secs(10))
                .unwrap_or_else(|_| panic!("{kind} 레코드가 오지 않았다"));
            if record_text(&record, "type") == Some(kind) {
                return record;
            }
            self.held.push(record);
        }
    }

    /// 두 종류 가운데 먼저 온 레코드
    fn next_of(&mut self, kinds: &[&str]) -> HostRecord {
        if let Some(index) = self
            .held
            .iter()
            .position(|record| kinds.contains(&record_text(record, "type").unwrap_or_default()))
        {
            return self.held.remove(index);
        }
        loop {
            let record = self
                .records
                .recv_timeout(Duration::from_secs(10))
                .expect("레코드가 오지 않았다");
            if kinds.contains(&record_text(&record, "type").unwrap_or_default()) {
                return record;
            }
            self.held.push(record);
        }
    }

    /// daemon이 이 연결을 닫을 때까지 기다린다. 남은 레코드는 버린다
    fn wait_until_closed(&mut self) {
        loop {
            match self.records.recv_timeout(Duration::from_secs(10)) {
                Ok(_) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!("daemon이 연결을 닫지 않았다")
                }
            }
        }
    }
}

fn text_of(record: &HostRecord) -> String {
    String::from_utf8(decode_host_bytes(record.get("data"), HOST_CHUNK_BYTES).unwrap()).unwrap()
}

/// hook 소켓에 HTTP 요청을 보내고 (상태, 본문)을 받는다
fn post(
    server: &HostServer,
    client_id: &str,
    body: &str,
    headers: &[(&str, &str)],
    path: &str,
) -> (u16, String) {
    post_to(&server.paths, client_id, body, headers, path)
}

// parity: TS-lib-remoteHostServer-983d0e
#[test]
fn private_sockets_keep_the_daemon_identity_across_attachment_disconnects() {
    let fixture = start();
    let (one, ready) = WireClient::ready(&fixture.server, "client-one");
    drop(one);

    let (_two, again) = WireClient::ready(&fixture.server, "client-two");

    assert_eq!(again["instanceId"], ready["instanceId"]);
    assert_eq!(again["pid"], json!(std::process::id()));
    assert_eq!(again["version"], json!(2));
    let mode = fs::metadata(&fixture.server.paths.rpc_socket_path)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

// parity: TS-lib-remoteHostServer-668fac
#[test]
fn a_competing_server_is_rejected_without_unlinking_the_live_socket() {
    let fixture = start();

    assert!(start_in(fixture.root.path()).is_err());

    let (_client, ready) = WireClient::ready(&fixture.server, "client-one");
    assert_eq!(
        record_text(&ready, "instanceId"),
        Some(fixture.server.instance_id.as_str())
    );
}

// parity: TS-lib-remoteHostServer-ec3b4a
#[test]
fn symlinked_and_nonprivate_roots_are_rejected() {
    let root = temp_root();
    let link = PathBuf::from(format!("{}-link", root.path().display()));
    symlink(root.path(), &link).unwrap();

    let linked = start_in(&link).err().unwrap();
    fs::remove_file(&link).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let shared = start_in(root.path()).err().unwrap();

    assert!(linked.contains("Untrusted"), "{linked}");
    assert!(shared.contains("private"), "{shared}");
}

// parity: TS-lib-remoteHostServer-5ae4b1
#[test]
fn mismatched_hellos_are_rejected() {
    let fixture = start();
    let mismatches = [
        json!({ "role": "hook" }),
        json!({ "namespace": "prod" }),
        json!({ "version": 1 }),
        json!({ "buildHash": "b".repeat(64) }),
    ];

    for hello in mismatches {
        let mut client = WireClient::connect(&fixture.server, "bad-client", hello.clone());
        let error = client.next("error");
        assert!(
            record_text(&error, "error").is_some_and(|error| !error.is_empty()),
            "{hello}"
        );
    }
}

// parity: TS-lib-remoteHostServer-69ed01
#[test]
fn commands_stream_stdout_and_stderr_with_exit_code_and_take_interactive_stdin_once() {
    let fixture = start();
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");

    peer.send(json!({ "type": "exec.open", "id": "interactive", "command": "read line; printf 'got:%s' \"$line\"; printf problem >&2; exit 7", "timeoutMs": 3000, "maxBufferBytes": 4096, "interactive": true }));
    peer.send(json!({ "type": "exec.stdin", "id": "interactive", "data": encode_host_bytes(b"hello\n"), "end": true }));

    let mut outputs: Vec<HostRecord> = vec![peer.next("exec.data"), peer.next("exec.data")];
    outputs.sort_by_key(|record| record_text(record, "stream").map(str::to_string));
    assert_eq!(record_text(&outputs[1], "stream"), Some("stdout"));
    assert_eq!(text_of(&outputs[1]), "got:hello");
    assert_eq!(record_text(&outputs[0], "stream"), Some("stderr"));
    assert_eq!(peer.next("exec.exit")["code"], json!(7));
}

// parity: TS-lib-remoteHostServer-4e98d0
#[test]
fn cancelling_one_request_leaves_another_command_usable() {
    let fixture = start();
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");

    peer.send(json!({ "type": "exec.open", "id": "slow", "command": "sleep 30", "timeoutMs": 35000, "maxBufferBytes": 4096 }));
    peer.send(json!({ "type": "exec.cancel", "id": "slow" }));
    assert_eq!(record_text(&peer.next("exec.exit"), "id"), Some("slow"));
    peer.send(json!({ "type": "exec.open", "id": "alive", "command": "printf alive", "timeoutMs": 3000, "maxBufferBytes": 4096 }));

    assert_eq!(text_of(&peer.next("exec.data")), "alive");
    assert_eq!(peer.next("exec.exit")["code"], json!(0));
}

fn start_echo_server(path: &Path) -> UnixListener {
    let listener = UnixListener::bind(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    let acceptor = listener.try_clone().unwrap();
    std::thread::spawn(move || {
        for stream in acceptor.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = stream.try_clone().unwrap();
                let mut writer = stream;
                let _ = std::io::copy(&mut reader, &mut writer);
            });
        }
    });
    listener
}

// parity: TS-lib-remoteHostServer-372a4e
#[test]
fn a_real_unix_socket_is_forwarded_without_owning_the_upstream_lifetime() {
    let fixture = start();
    let upstream_path = fixture.root.path().join("herdr.sock");
    let _upstream = start_echo_server(&upstream_path);
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");

    peer.send(json!({ "type": "socket.open", "id": "native", "path": upstream_path }));
    peer.next("socket.ready");
    peer.send(json!({ "type": "socket.data", "id": "native", "data": encode_host_bytes(b"native bytes") }));

    assert_eq!(
        record_text(&peer.next("socket.data-ready"), "id"),
        Some("native")
    );
    assert_eq!(text_of(&peer.next("socket.data")), "native bytes");
    peer.send(json!({ "type": "socket.close", "id": "native" }));
    assert_eq!(
        record_text(&peer.next("socket.close"), "id"),
        Some("native")
    );
    assert!(UnixStream::connect(&upstream_path).is_ok());
}

// parity: TS-lib-remoteHostServer-b796e2
#[test]
fn hooks_go_to_one_desktop_and_offline_requests_get_503_without_being_kept() {
    let fixture = start();
    let offline = (503, "Desktop offline; request not replayed".to_string());
    assert_eq!(
        post(
            &fixture.server,
            "offline",
            r#"{"taskId":"fixture","status":"done"}"#,
            &[],
            "/api/hooks/status"
        ),
        offline
    );
    assert_eq!(
        post(&fixture.server, "offline", "{}", &[], "/api/cli/tasks"),
        offline
    );
    assert_eq!(
        post(
            &fixture.server,
            "bad/client",
            "{}",
            &[],
            "/api/hooks/status"
        ),
        (400, "Invalid hook client".into())
    );
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");

    let server_paths = fixture.server.paths.clone();
    let pending = std::thread::spawn(move || {
        post_to(
            &server_paths,
            "client-one",
            r#"{"taskId":"fixture","status":"done"}"#,
            &[],
            "/api/hooks/status",
        )
    });
    let hook = peer.next("hook.request");
    assert_eq!(record_text(&hook, "path"), Some("/api/hooks/status"));
    assert_eq!(
        decode_host_bytes(hook.get("body"), 1 << 20).unwrap(),
        br#"{"taskId":"fixture","status":"done"}"#
    );
    peer.send(json!({ "type": "hook.response", "id": hook["id"], "status": 202, "headers": { "content-type": "application/json" }, "body": encode_host_bytes(br#"{"ok":true}"#) }));

    assert_eq!(pending.join().unwrap(), (202, r#"{"ok":true}"#.to_string()));
    drop(peer);
    wait_until(|| {
        post(
            &fixture.server,
            "client-one",
            "{}",
            &[],
            "/api/hooks/status",
        ) == offline
    });
}

fn post_to(
    paths: &HostSocketPaths,
    client_id: &str,
    body: &str,
    headers: &[(&str, &str)],
    path: &str,
) -> (u16, String) {
    let mut stream = UnixStream::connect(&paths.hook_socket_path).unwrap();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nx-kanvibe-client-id: {client_id}\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (key, value) in headers {
        request.push_str(&format!("{key}: {value}\r\n"));
    }
    stream
        .write_all(format!("{request}\r\n{body}").as_bytes())
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (response[9..12].parse().unwrap(), body)
}

fn wait_until(condition: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "조건이 시간 안에 맞지 않았다"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

// parity: TS-lib-remoteHostServer-3e6bfa
#[test]
fn task_cli_requests_and_tokens_reach_the_connected_desktop() {
    let fixture = start();
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");

    for request_path in ["/api/cli/tasks/list", "/api/cli/tasks/session"] {
        let paths = fixture.server.paths.clone();
        let pending = std::thread::spawn(move || {
            post_to(
                &paths,
                "client-one",
                r#"{"contextTaskId":"fixture"}"#,
                &[("authorization", "Bearer task-token")],
                request_path,
            )
        });
        let hook = peer.next("hook.request");
        assert_eq!(record_text(&hook, "path"), Some(request_path));
        assert_eq!(hook["headers"]["authorization"], json!("Bearer task-token"));
        peer.send(json!({ "type": "hook.response", "id": hook["id"], "status": 200, "headers": { "content-type": "application/json" }, "body": encode_host_bytes(b"[]") }));
        assert_eq!(pending.join().unwrap(), (200, "[]".to_string()));
    }
}

// parity: TS-lib-remoteHostServer-7f2626
#[test]
fn a_duplicate_live_client_id_is_rejected_without_replacing_the_first_desktop() {
    let fixture = start();
    let (mut first, original) = WireClient::ready(&fixture.server, "client-one");

    let mut second = WireClient::connect(&fixture.server, "client-one", json!({}));
    // daemon이 첫 데스크탑에 먼저 물어보지 않고 거절했다면 ping은 오지 않는다
    let probe = first.next("ping");
    first.send(json!({ "type": "pong", "id": probe["id"] }));
    let rejection = second.next("error");
    first.send(json!({ "type": "status", "id": "identity" }));

    assert_eq!(rejection["code"], json!("client-already-connected"));
    assert!(
        record_text(&rejection, "error").is_some_and(|error| error.contains("already connected")),
        "{rejection:?}"
    );
    assert_eq!(first.next("status")["instanceId"], original["instanceId"]);
}

// parity: TS-lib-remoteHostServer-6a8ce9
#[test]
fn an_unresponsive_desktop_is_replaced_once_when_several_connections_share_its_client_id() {
    let fixture = start();
    let (mut original, ready) = WireClient::ready(&fixture.server, "client-one");
    let server_paths = fixture.server.paths.clone();
    let pending_hook = std::thread::spawn(move || {
        post_to(&server_paths, "client-one", "{}", &[], "/api/hooks/status")
    });
    original.next("hook.request");

    let mut replacements = [
        WireClient::connect(&fixture.server, "client-one", json!({})),
        WireClient::connect(&fixture.server, "client-one", json!({})),
    ];
    let results: Vec<HostRecord> = replacements
        .iter_mut()
        .map(|replacement| replacement.next_of(&["ready", "error"]))
        .collect();

    let ready_index = results
        .iter()
        .position(|record| record_text(record, "type") == Some("ready"))
        .expect("교체한 연결 하나는 ready를 받는다");
    let rejected = &results[1 - ready_index];
    assert_eq!(results[ready_index]["instanceId"], ready["instanceId"]);
    assert_eq!(results[ready_index]["replacedStalePeer"], json!(true));
    assert_eq!(record_text(rejected, "type"), Some("error"));
    assert_eq!(rejected["code"], json!("client-already-connected"));
    original.wait_until_closed();
    assert_eq!(
        pending_hook.join().unwrap(),
        (
            503,
            "Desktop disconnected; request not replayed".to_string()
        )
    );
    let server_paths = fixture.server.paths.clone();
    let new_hook = std::thread::spawn(move || {
        post_to(&server_paths, "client-one", "{}", &[], "/api/hooks/status")
    });
    let active = &mut replacements[ready_index];
    let request = active.next("hook.request");
    active.send(json!({ "type": "hook.response", "id": request["id"], "status": 202, "headers": {}, "body": "" }));
    assert_eq!(new_hook.join().unwrap(), (202, String::new()));
}

// parity: TS-lib-remoteHostServer-0100d6
#[test]
fn the_original_desktop_stays_when_the_waiting_replacement_disconnects() {
    let fixture = start();
    let (mut original, _) = WireClient::ready(&fixture.server, "client-one");
    let cancelled = WireClient::connect(&fixture.server, "client-one", json!({}));
    original.next("ping");

    drop(cancelled);
    std::thread::sleep(Duration::from_millis(2_100));

    original.send(json!({ "type": "status", "id": "original-status" }));
    assert_eq!(original.next("status")["id"], json!("original-status"));
}

// parity: TS-lib-remoteHostServer-32c20b
#[test]
fn consumed_stdin_chunks_are_acknowledged_so_a_bulk_upload_stays_bounded() {
    let fixture = start();
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");
    peer.send(json!({ "type": "exec.open", "id": "upload", "command": "wc -c", "timeoutMs": 3000, "maxBufferBytes": 4096, "interactive": true }));
    let chunk = encode_host_bytes(&vec![b'x'; HOST_CHUNK_BYTES]);

    for _ in 0..257 {
        peer.send(json!({ "type": "exec.stdin", "id": "upload", "data": chunk }));
        assert_eq!(
            record_text(&peer.next("exec.stdin-ready"), "id"),
            Some("upload")
        );
    }
    peer.send(json!({ "type": "exec.stdin", "id": "upload", "data": "", "end": true }));

    assert_eq!(
        text_of(&peer.next("exec.data")).trim(),
        (HOST_CHUNK_BYTES * 257).to_string()
    );
    assert_eq!(peer.next("exec.exit")["code"], json!(0));
}

// parity: TS-lib-remoteHostServer-fd68dd
#[test]
fn unbounded_streaming_output_stays_alive_while_bounded_commands_enforce_their_limit() {
    let fixture = start();
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");
    let command = "printf '%65536s' x";

    peer.send(json!({ "type": "exec.open", "id": "bounded", "command": command, "timeoutMs": 3000, "maxBufferBytes": 1024 }));
    let bounded = peer.next("exec.exit");
    peer.send(json!({ "type": "exec.open", "id": "streaming", "command": command, "timeoutMs": 3000, "maxBufferBytes": 0 }));
    let streaming = peer.next_of(&["exec.exit", "error"]);

    assert!(record_text(&bounded, "error").is_some_and(|error| error.contains("maxBuffer")));
    assert_eq!(record_text(&streaming, "type"), Some("exec.exit"));
    assert_eq!(streaming["code"], json!(0));
}

// parity: TS-lib-remoteHostServer-1e9ffc
#[test]
fn a_browser_origin_is_rejected_at_hook_ingress() {
    let fixture = start();

    let (status, body) = post(
        &fixture.server,
        "offline",
        "{}",
        &[("Origin", "https://untrusted.example")],
        "/api/hooks/status",
    );

    assert_eq!(status, 403);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({ "success": false, "error": "브라우저 요청은 허용하지 않습니다" })
    );
}

// parity: TS-lib-remoteHostServer-ef2ee8
#[test]
fn a_forty_kib_file_command_is_not_limited_by_the_chunk_size() {
    let fixture = start();
    let (mut peer, _) = WireClient::ready(&fixture.server, "client-one");
    let contents = encode_host_bytes(&vec![b'x'; 40 * 1024]);
    let file_path = fixture.root.path().join("large-file.txt");
    let command = format!(
        "printf '%s' '{contents}' | base64 -d > '{}'; wc -c < '{}'",
        file_path.display(),
        file_path.display()
    );

    peer.send(json!({ "type": "exec.open", "id": "large-file", "command": command, "timeoutMs": 3000, "maxBufferBytes": 4096 }));
    let result = peer.next_of(&["exec.data", "error"]);

    assert_eq!(record_text(&result, "type"), Some("exec.data"));
    assert_eq!(text_of(&result).trim(), (40 * 1024).to_string());
    assert_eq!(peer.next("exec.exit")["code"], json!(0));
}

// parity: TS-lib-remoteHostPtyWire-dd6861
#[test]
fn a_daemon_pty_reattaches_to_the_same_pid_after_the_ssh_attachment_is_lost() {
    let fixture = start();
    let open = |id: &str| json!({ "type": "pty.open", "id": id, "key": "stable-tab", "command": "exec /bin/sh", "cols": 80, "rows": 24 });
    let (mut first_peer, _) = WireClient::ready(&fixture.server, "desktop-one");
    first_peer.send(open("first"));
    let first = first_peer.next_of(&["pty.ready", "pty.error"]);
    assert_eq!(record_text(&first, "type"), Some("pty.ready"));
    drop(first_peer);

    let mut second_peer = connect_when_released(&fixture.server, "desktop-one");
    second_peer.send(open("second"));
    let second = second_peer.next("pty.ready");

    assert_eq!(second["pid"], first["pid"]);
    assert_eq!(second["sessionId"], first["sessionId"]);
    second_peer.send(json!({ "type": "pty.input", "id": "second", "data": encode_host_bytes(b"printf '\\127\\111\\122\\105\\137\\117\\113\\n'\n") }));
    second_peer.next("pty.input-ready");
    let mut output = String::new();
    while !output.contains("WIRE_OK") {
        output.push_str(&text_of(&second_peer.next("pty.data")));
    }
    second_peer.send(json!({ "type": "pty.close", "id": "second" }));
    assert_eq!(
        second_peer.next("pty.exit")["sessionId"],
        first["sessionId"]
    );
}

/// 끊긴 연결의 client id가 daemon에서 풀릴 때까지 다시 붙는다
fn connect_when_released(server: &HostServer, client_id: &str) -> WireClient {
    for _ in 0..100 {
        let mut client = WireClient::connect(server, client_id, json!({}));
        let first = client.next_of(&["ready", "error"]);
        if record_text(&first, "type") == Some("ready") {
            return client;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("client id가 풀리지 않았다");
}
