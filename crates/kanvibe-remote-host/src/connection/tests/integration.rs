//! 같은 프로세스에 띄운 실제 daemon(`kanvibe_remote_agent::daemon`)에 데스크탑 연결로 붙는다(TS `remoteHostIntegration`).

use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_process::exec::RemoteStdinResponder;
use kanvibe_remote_agent::daemon::pty_launcher::NativePtyLauncher;
use kanvibe_remote_agent::daemon::{HostServer, HostServerOptions};
use kanvibe_remote_agent::protocol::HostSocketPaths;
use kanvibe_terminal::terminal_registry::RemoteHostSocketForward;

use crate::connection::{HostConnection, HostExecuteOptions, HostIdentity, HostTransport};
use crate::hook_tunnel::{HostHookRequest, HostHookResponse};

const NAMESPACE: &str = "qa-integration";

struct DaemonFixture {
    root: tempfile::TempDir,
    server: Option<HostServer>,
}

fn start_server(root: &Path) -> HostServer {
    HostServer::start(HostServerOptions {
        namespace: NAMESPACE.into(),
        build_hash: "a".repeat(64),
        root_directory: Some(root.to_path_buf()),
        pty_launcher: Arc::new(NativePtyLauncher),
    })
    .unwrap()
}

impl DaemonFixture {
    fn start() -> Self {
        let root = tempfile::Builder::new()
            .prefix("kvhi-")
            .tempdir_in("/tmp")
            .unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let server = start_server(root.path());
        Self {
            root,
            server: Some(server),
        }
    }

    fn server(&self) -> &HostServer {
        self.server.as_ref().unwrap()
    }

    fn paths(&self) -> HostSocketPaths {
        self.server().paths.clone()
    }

    fn attach(&self, client_id: &str) -> Arc<HostConnection> {
        let stream = UnixStream::connect(&self.server().paths.rpc_socket_path).unwrap();
        let closer = stream.try_clone().unwrap();
        let transport = HostTransport {
            writer: Box::new(stream.try_clone().unwrap()),
            reader: Box::new(stream),
            terminate: Box::new(move || drop(closer.shutdown(Shutdown::Both))),
            stderr_tail: Arc::new(Mutex::new(String::new())),
        };
        let identity = HostIdentity {
            namespace: NAMESPACE.into(),
            client_id: client_id.into(),
            build_hash: "a".repeat(64),
        };
        HostConnection::attach(transport, &identity).unwrap()
    }
}

/// hook 소켓에 상태 hook을 보낸다
fn hook(paths: &HostSocketPaths, client_id: &str, body: &str) -> (u16, String) {
    let mut stream = UnixStream::connect(&paths.hook_socket_path).unwrap();
    let request = format!(
        "POST /api/hooks/status HTTP/1.1\r\nx-kanvibe-client-id: {client_id}\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (response[9..12].parse().unwrap(), body)
}

fn ok_response(body: &[u8]) -> HostHookResponse {
    HostHookResponse {
        status: 200,
        headers: vec![("content-type".into(), "text/plain".into())],
        body: body.to_vec(),
    }
}

fn body_text(request: &HostHookRequest) -> String {
    String::from_utf8(request.body.clone()).unwrap()
}

#[test]
fn an_eight_mib_transaction_response_is_delivered_once_without_truncation() {
    let fixture = DaemonFixture::start();
    let connection = fixture.attach("client-one");
    let response = format!("{}\n", "a".repeat(8 * 1024 * 1024));
    let response_length = response.len();
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let responder = RemoteStdinResponder {
        marker: "READY".into(),
        respond: Arc::new(move |payload| {
            assert_eq!(payload, "payload");
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(response.clone())
        }),
    };
    let options = HostExecuteOptions {
        timeout: Some(Duration::from_secs(15)),
        remote_stdin_responder: Some(responder),
        ..HostExecuteOptions::default()
    };

    let result = connection
        .execute("printf 'READY\\tpayload\\n'; wc -c", &options)
        .unwrap();

    assert_eq!(result.parse::<usize>().unwrap(), response_length);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn hooks_and_another_client_keep_progressing_during_twenty_bulk_results() {
    let fixture = DaemonFixture::start();
    let (first, second) = (fixture.attach("client-one"), fixture.attach("client-two"));
    let received = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&received);
    second.set_hook_handler(Arc::new(move |request| {
        recorded.lock().unwrap().push(body_text(&request));
        Ok(ok_response(b"ack"))
    }));

    let bulk: Vec<_> = (0..20)
        .map(|_| {
            let first = Arc::clone(&first);
            std::thread::spawn(move || {
                first.execute(
                    "head -c 131072 /dev/zero",
                    &HostExecuteOptions {
                        timeout: Some(Duration::from_secs(10)),
                        ..HostExecuteOptions::default()
                    },
                )
            })
        })
        .collect();
    let hooked = hook(
        &fixture.paths(),
        "client-two",
        r#"{"taskId":"fixture","status":"review"}"#,
    );
    let interactive = second
        .execute("printf interactive", &HostExecuteOptions::default())
        .unwrap();

    assert_eq!(hooked, (200, "ack".to_string()));
    assert_eq!(interactive, "interactive");
    for output in bulk {
        assert_eq!(output.join().unwrap().unwrap().len(), 131072);
    }
    assert_eq!(
        *received.lock().unwrap(),
        vec![r#"{"taskId":"fixture","status":"review"}"#.to_string()]
    );
}

fn start_echo(path: &Path) -> UnixListener {
    let listener = UnixListener::bind(path).unwrap();
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

#[test]
fn binary_native_bytes_are_forwarded_and_detaching_keeps_the_upstream_server() {
    let fixture = DaemonFixture::start();
    let connection = fixture.attach("client-one");
    let upstream_path = fixture.root.path().join("native.sock");
    let _upstream = start_echo(&upstream_path);
    let forward = connection
        .forward_socket(upstream_path.to_str().unwrap())
        .unwrap();
    let mut bytes = vec![0xffu8; 96 * 1024];
    bytes[0] = 0;

    let mut local = UnixStream::connect(forward.path()).unwrap();
    let mut reader = local.try_clone().unwrap();
    let expected = bytes.len();
    let echoed = std::thread::spawn(move || {
        let mut actual = vec![0u8; expected];
        reader.read_exact(&mut actual).unwrap();
        actual
    });
    local.write_all(&bytes).unwrap();
    let actual = echoed.join().unwrap();
    local.shutdown(Shutdown::Write).unwrap();
    forward.close().unwrap();
    connection.close();

    assert_eq!(actual, bytes);
    assert!(UnixStream::connect(&upstream_path).is_ok());
    let next = fixture.attach("client-new");
    assert_eq!(next.instance_id, fixture.server().instance_id);
}

fn poll_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "조건이 시간 안에 맞지 않았다");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_offline_hook_fails_and_is_not_replayed_after_reconnect() {
    let fixture = DaemonFixture::start();
    let first = fixture.attach("client-one");
    first.close();
    let offline = (503, "Desktop offline; request not replayed".to_string());
    poll_until(|| {
        hook(
            &fixture.paths(),
            "client-one",
            r#"{"taskId":"fixture","status":"review"}"#,
        ) == offline
    });

    let second = fixture.attach("client-one");
    let events = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&events);
    second.set_hook_handler(Arc::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(ok_response(b""))
    }));

    assert_eq!(
        second
            .execute("printf snapshot", &HostExecuteOptions::default())
            .unwrap(),
        "snapshot"
    );
    assert_eq!(events.load(Ordering::SeqCst), 0);
}

#[test]
fn a_later_task_is_answered_while_earlier_tasks_of_the_same_client_hang_or_fail() {
    let fixture = DaemonFixture::start();
    let connection = fixture.attach("client-one");
    let received = Arc::new(Mutex::new(Vec::new()));
    let (hanging_started, started) = channel();
    let (release, released) = channel::<()>();
    let (recorded, hanging_started, released) = (
        Arc::clone(&received),
        Mutex::new(hanging_started),
        Mutex::new(released),
    );
    connection.set_hook_handler(Arc::new(move |request| {
        let body = body_text(&request);
        recorded.lock().unwrap().push(body.clone());
        let task_id = serde_json::from_str::<serde_json::Value>(&body).unwrap()["taskId"]
            .as_str()
            .unwrap()
            .to_string();
        if task_id == "hanging" {
            hanging_started.lock().unwrap().send(()).unwrap();
            let _ = released.lock().unwrap().recv();
        }
        let status = if task_id == "healthy" { 200 } else { 503 };
        Ok(HostHookResponse {
            status,
            headers: Vec::new(),
            body: task_id.into_bytes(),
        })
    }));
    let paths = fixture.paths();
    let send_hook = |body: &'static str| {
        let paths = paths.clone();
        std::thread::spawn(move || hook(&paths, "client-one", body))
    };

    let hanging = send_hook(r#"{"taskId":"hanging","status":"review"}"#);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let failing = send_hook(r#"{"taskId":"failing","status":"review"}"#);
    let healthy_body = r#"{"taskId":"healthy","status":"done","agentSession":{"provider":"codex","hookPid":1234}}"#;
    let healthy = send_hook(healthy_body);

    assert_eq!(healthy.join().unwrap(), (200, "healthy".to_string()));
    assert_eq!(failing.join().unwrap(), (503, "failing".to_string()));
    assert!(received.lock().unwrap().contains(&healthy_body.to_string()));
    release.send(()).unwrap();
    assert_eq!(hanging.join().unwrap(), (503, "hanging".to_string()));
    assert_eq!(received.lock().unwrap().len(), 3);
}

#[test]
fn an_earlier_builds_journal_file_is_neither_replayed_nor_rewritten() {
    let pending_event = serde_json::json!({ "version": 1, "nextSequence": 2, "expired": 0, "rejected": 0, "ignored": 0, "events": [{
        "id": "11111111-1111-4111-8111-111111111111", "sequence": 1, "receivedAt": 0, "clientId": "client-one", "method": "POST",
        "path": "/api/hooks/status", "headers": { "x-kanvibe-client-id": "client-one" }, "body": "eyJ0YXNrSWQiOiJmaXh0dXJlIn0=",
    }] })
    .to_string();
    for content in [pending_event.as_str(), "{corrupt"] {
        let mut fixture = DaemonFixture::start();
        fixture.server.take().unwrap().close();
        let journal_directory = fixture.root.path().join("state").join(NAMESPACE);
        std::fs::create_dir_all(&journal_directory).unwrap();
        let journal_path = journal_directory.join("hook-journal-v1.json");
        std::fs::write(&journal_path, content).unwrap();
        fixture.server = Some(start_server(fixture.root.path()));
        let connection = fixture.attach("client-one");
        let received = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&received);
        connection.set_hook_handler(Arc::new(move |request| {
            recorded.lock().unwrap().push(body_text(&request));
            Ok(HostHookResponse {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
            })
        }));

        let live = r#"{"taskId":"live","status":"review"}"#;
        assert_eq!(
            hook(&fixture.paths(), "client-one", live),
            (200, String::new())
        );
        assert_eq!(*received.lock().unwrap(), vec![live.to_string()]);
        assert_eq!(std::fs::read_to_string(&journal_path).unwrap(), content);
    }
}
