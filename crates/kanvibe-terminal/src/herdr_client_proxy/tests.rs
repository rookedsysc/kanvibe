//! 실제 Unix 소켓으로 가짜 Herdr 서버와 가짜 TUI를 세우고 프록시를 사이에 둔다.

use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;

/// 테스트가 영원히 멈추지 않도록 소켓 읽기에 거는 한도. 요청 시간 초과(5초)보다 길어야 한다
const SOCKET_READ_LIMIT: Duration = Duration::from_secs(15);

/// TS 테스트의 `setImmediate` 대기. 프록시가 방금 쓴 프레임을 읽어 처리할 시간을 준다
const PROXY_READ_GRACE: Duration = Duration::from_millis(150);

fn string_bytes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut encoded = if bytes.len() < 251 {
        vec![bytes.len() as u8]
    } else {
        vec![251, (bytes.len() & 255) as u8, (bytes.len() >> 8) as u8]
    };
    encoded.extend_from_slice(bytes);
    encoded
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut framed = (payload.len() as u32).to_le_bytes().to_vec();
    framed.extend_from_slice(payload);
    framed
}

fn pane_input(tag: u8, target: &str) -> Vec<u8> {
    let payload = [
        vec![tag],
        string_bytes(target),
        vec![1, 1],
        string_bytes("must not reach initial terminal"),
    ]
    .concat();
    frame(&payload)
}

fn snapshot_for(boot_id: &str, workspace_id: &str, tab_id: &str, pane_id: &str) -> Vec<u8> {
    let snapshot_json = json!({
        "boot_id": boot_id,
        "focused_workspace_id": workspace_id,
        "focused_tab_id": tab_id,
        "focused_pane_id": pane_id,
    })
    .to_string();
    frame(
        &[
            vec![20],
            string_bytes("shell.snapshot.v1"),
            string_bytes(&snapshot_json),
        ]
        .concat(),
    )
}

fn snapshot(boot_id: &str, workspace_id: &str, tab_id: &str) -> Vec<u8> {
    snapshot_for(boot_id, workspace_id, tab_id, &format!("{workspace_id}:p1"))
}

fn default_snapshot() -> Vec<u8> {
    snapshot("boot-1", "w1", "w1:t1")
}

fn read_test_string(payload: &[u8], position: usize) -> (String, usize) {
    let marker = payload[position];
    let mut position = position + 1;
    let length = if marker == 251 {
        let length = u16::from_le_bytes([payload[position], payload[position + 1]]) as usize;
        position += 2;
        length
    } else {
        marker as usize
    };
    let text = String::from_utf8(payload[position..position + length].to_vec()).unwrap();
    (text, position + length)
}

struct EndpointRequest {
    boot_id: String,
    request: Value,
}

impl EndpointRequest {
    fn id(&self) -> &str {
        self.request["id"].as_str().unwrap()
    }
}

fn endpoint_request(payload: &[u8]) -> EndpointRequest {
    assert_eq!(payload[0], 15);
    let (boot_id, position) = read_test_string(payload, 1);
    let (request_json, _) = read_test_string(payload, position);
    EndpointRequest {
        boot_id,
        request: serde_json::from_str(&request_json).unwrap(),
    }
}

fn response(boot_id: &str, id: &str, body: Value) -> Vec<u8> {
    let payload = [
        vec![18],
        string_bytes(boot_id),
        string_bytes(id),
        vec![1],
        string_bytes(&body.to_string()),
    ]
    .concat();
    frame(&payload)
}

fn ok_body() -> Value {
    json!({ "result": { "type": "ok" } })
}

/// 소켓 한쪽에서 프레임 본문을 하나씩 읽는다
struct FrameStream {
    stream: UnixStream,
}

impl FrameStream {
    fn new(stream: &UnixStream) -> Self {
        let stream = stream.try_clone().unwrap();
        stream.set_read_timeout(Some(SOCKET_READ_LIMIT)).unwrap();
        Self { stream }
    }

    fn next(&mut self) -> Vec<u8> {
        let mut header = [0_u8; 4];
        self.stream.read_exact(&mut header).unwrap();
        let mut payload = vec![0_u8; u32::from_le_bytes(header) as usize];
        self.stream.read_exact(&mut payload).unwrap();
        payload
    }

    /// 상대가 연결을 닫을 때까지 기다린다
    fn wait_until_closed(&mut self) {
        let mut buffer = [0_u8; 64];
        loop {
            match self.stream.read(&mut buffer) {
                Ok(0) => return,
                Ok(_) => continue,
                Err(error) if error.kind() == ErrorKind::ConnectionReset => return,
                Err(error) => panic!("연결이 닫히지 않았다: {error}"),
            }
        }
    }
}

struct Fixture {
    proxy: Arc<HerdrClientProxy>,
    client: UnixStream,
    upstream: UnixStream,
    received: FrameStream,
    sent: FrameStream,
    server: UnixListener,
    _upstream_directory: TempDir,
    proxy_temp_root: TempDir,
}

fn fixture() -> Fixture {
    let upstream_directory = tempfile::Builder::new().prefix("kvhct-").tempdir().unwrap();
    let upstream_path = upstream_directory.path().join("api");
    let server = UnixListener::bind(&upstream_path).unwrap();
    let proxy_temp_root = tempfile::tempdir().unwrap();
    let proxy = HerdrClientProxy::create(&upstream_path, "w1", proxy_temp_root.path()).unwrap();
    let client = UnixStream::connect(proxy.socket_path()).unwrap();
    let received = FrameStream::new(&client);
    let (upstream, _) = server.accept().unwrap();
    let sent = FrameStream::new(&upstream);
    Fixture {
        proxy: Arc::new(proxy),
        client,
        upstream,
        received,
        sent,
        server,
        _upstream_directory: upstream_directory,
        proxy_temp_root,
    }
}

fn initialize(setup: &mut Fixture) -> EndpointRequest {
    setup.upstream.write_all(&default_snapshot()).unwrap();
    let initial = endpoint_request(&setup.sent.next());
    finish_initialization(&mut setup.upstream, &mut setup.received, &initial);
    initial
}

fn finish_initialization(
    upstream: &mut UnixStream,
    received: &mut FrameStream,
    focus: &EndpointRequest,
) {
    let marker = response(&focus.boot_id, "native-ready", ok_body());
    upstream
        .write_all(&response(&focus.boot_id, focus.id(), ok_body()))
        .unwrap();
    upstream.write_all(&marker).unwrap();
    // snapshot은 focus 응답이 처리되기 전에 전달된다
    received.next();
    assert_eq!(frame(&received.next()), marker);
}

fn request_in_background(
    proxy: &Arc<HerdrClientProxy>,
    method: &'static str,
    params: Value,
) -> thread::JoinHandle<Result<Value, String>> {
    let proxy = Arc::clone(proxy);
    thread::spawn(move || proxy.request(method, params))
}

fn hello_frame() -> Vec<u8> {
    let hello_json = json!({ "generation": 1, "surface_size": { "cols": 134, "rows": 47 }, "surface_active": true });
    frame(
        &[
            vec![20],
            string_bytes("endpoint.hello.v1"),
            string_bytes(&hello_json.to_string()),
        ]
        .concat(),
    )
}

// parity: TS-lib-herdrClientProxy-6250dd
#[test]
fn tracks_surface_size_and_sidebar_changes_and_hides_inactive_surface() {
    let mut setup = fixture();
    initialize(&mut setup);
    setup.client.write_all(&hello_frame()).unwrap();
    setup.sent.next();
    assert_eq!(
        setup.proxy.surface_size(),
        Some(HerdrSurfaceSize {
            cols: 134,
            rows: 47
        })
    );
    setup
        .client
        .write_all(&frame(&[12, 8, 16, 156, 47, 0]))
        .unwrap();
    setup.sent.next();
    assert_eq!(
        setup.proxy.surface_size(),
        Some(HerdrSurfaceSize {
            cols: 156,
            rows: 47
        })
    );
    let hide_json = json!({ "id": "native:hide", "method": "client_shell.surface.set", "params": { "active": false } });
    let hide = frame(
        &[
            vec![15],
            string_bytes("boot-1"),
            string_bytes(&hide_json.to_string()),
        ]
        .concat(),
    );
    setup.client.write_all(&hide).unwrap();
    setup.sent.next();
    assert_eq!(setup.proxy.surface_size(), None);
}

// parity: TS-lib-herdrClientProxy-86f49c
#[test]
fn unknown_resize_frame_stops_projection_without_guessing_size() {
    let mut setup = fixture();
    initialize(&mut setup);
    setup.client.write_all(&hello_frame()).unwrap();
    setup.sent.next();
    assert_eq!(
        setup.proxy.surface_size(),
        Some(HerdrSurfaceSize {
            cols: 134,
            rows: 47
        })
    );
    setup.client.write_all(&frame(&[12, 8, 16, 251])).unwrap();
    setup.sent.next();
    assert_eq!(setup.proxy.surface_size(), None);
}

// parity: TS-lib-herdrClientProxy-aaf7a1
#[test]
fn keeps_native_initial_snapshot_and_surface_but_drops_stale_initial_input() {
    let mut setup = fixture();
    let original = snapshot("boot-1", "w2", "w2:t1");
    let surface = frame(&[13, 0]);
    setup.upstream.write_all(&original).unwrap();
    setup.upstream.write_all(&surface).unwrap();
    let initial = endpoint_request(&setup.sent.next());
    setup.client.write_all(&pane_input(13, "w2:p1")).unwrap();
    thread::sleep(PROXY_READ_GRACE);
    let selected = snapshot("boot-1", "w1", "w1:t1");
    setup
        .upstream
        .write_all(&response(&initial.boot_id, initial.id(), ok_body()))
        .unwrap();
    setup.upstream.write_all(&selected).unwrap();
    assert_eq!(frame(&setup.received.next()), original);
    assert_eq!(frame(&setup.received.next()), surface);
    assert_eq!(frame(&setup.received.next()), selected);
    let current_input = frame(&[18, 1]);
    setup.client.write_all(&current_input).unwrap();
    assert_eq!(frame(&setup.sent.next()), current_input);
}

// parity: TS-lib-herdrClientProxy-287897
#[test]
fn drops_pane_and_popup_input_until_initial_focus_response() {
    let mut setup = fixture();
    setup.upstream.write_all(&default_snapshot()).unwrap();
    let initial = endpoint_request(&setup.sent.next());
    setup.client.write_all(&pane_input(13, "w1:p1")).unwrap();
    thread::sleep(PROXY_READ_GRACE);
    setup
        .client
        .write_all(&pane_input(14, "popup-terminal"))
        .unwrap();
    thread::sleep(PROXY_READ_GRACE);
    setup
        .upstream
        .write_all(&response(&initial.boot_id, initial.id(), ok_body()))
        .unwrap();
    setup.received.next();
    let current_input = frame(&[18, 1]);
    setup.client.write_all(&current_input).unwrap();
    assert_eq!(frame(&setup.sent.next()), current_input);
}

// parity: TS-lib-herdrClientProxy-cef703
#[test]
fn first_snapshot_boot_id_selects_only_this_connection_workspace() {
    let mut setup = fixture();
    let initial = initialize(&mut setup);
    assert_eq!(initial.boot_id, "boot-1");
    assert_eq!(initial.request["method"], "workspace.focus");
    assert_eq!(initial.request["params"], json!({ "workspace_id": "w1" }));
    let changed = snapshot("boot-1", "w1", "w1:t3");
    setup.upstream.write_all(&changed).unwrap();
    assert_eq!(frame(&setup.received.next()), changed);
    assert_eq!(setup.proxy.active_tab_id().as_deref(), Some("w1:t3"));
}

// parity: TS-lib-herdrClientProxy-56baa4
#[test]
fn returns_only_focused_pane_inside_reported_workspace() {
    let mut setup = fixture();
    initialize(&mut setup);
    assert_eq!(setup.proxy.active_pane_id().as_deref(), Some("w1:p1"));
    setup
        .upstream
        .write_all(&snapshot_for("boot-1", "w1", "w1:t3", "w1:p7"))
        .unwrap();
    setup.received.next();
    assert_eq!(setup.proxy.active_pane_id().as_deref(), Some("w1:p7"));
    setup
        .upstream
        .write_all(&snapshot_for("boot-1", "w2", "w2:t1", "w2:p1"))
        .unwrap();
    setup.received.next();
    assert_eq!(setup.proxy.active_pane_id(), None);
}

// parity: TS-lib-herdrClientProxy-9bdcf1
#[test]
fn relays_fragmented_frames_and_native_responses_but_hides_own_responses() {
    let mut setup = fixture();
    let bytes = default_snapshot();
    for part in [&bytes[..2], &bytes[2..9], &bytes[9..]] {
        setup.upstream.write_all(part).unwrap();
        setup.upstream.flush().unwrap();
    }
    let initial = endpoint_request(&setup.sent.next());
    setup
        .upstream
        .write_all(&response(&initial.boot_id, initial.id(), ok_body()))
        .unwrap();
    assert_eq!(frame(&setup.received.next()), bytes);
    let native = response("boot-1", "native-request", ok_body());
    setup.upstream.write_all(&native).unwrap();
    assert_eq!(frame(&setup.received.next()), native);
    let input = frame(&[18, 1]);
    setup.client.write_all(&input).unwrap();
    assert_eq!(frame(&setup.sent.next()), input);
}

// parity: TS-lib-herdrClientProxy-f3eddd
#[test]
fn relays_large_split_frame_unchanged() {
    let mut setup = fixture();
    initialize(&mut setup);
    let payload = vec![7_u8; 1024 * 1024];
    let bytes = frame(&payload);
    // TUI 쪽 버퍼가 차면 프록시가 서버 읽기를 멈추므로, 보내는 쪽은 따로 돌려 읽기와 겹치게 한다
    let mut upstream = setup.upstream.try_clone().unwrap();
    let writer = thread::spawn(move || {
        for part in bytes.chunks(1024) {
            upstream.write_all(part).unwrap();
        }
    });
    assert_eq!(setup.received.next(), payload);
    writer.join().unwrap();
}

// parity: TS-lib-herdrClientProxy-0b1a2b
#[test]
fn preserves_boundaries_between_consecutive_frames_and_partial_header() {
    let mut setup = fixture();
    initialize(&mut setup);
    let first = frame(&[7, 1, 2]);
    let second = frame(&[8, 3, 4]);
    let third = frame(&[9, 5, 6]);
    setup
        .upstream
        .write_all(&[first.clone(), second.clone(), third[..2].to_vec()].concat())
        .unwrap();
    assert_eq!(frame(&setup.received.next()), first);
    assert_eq!(frame(&setup.received.next()), second);
    setup.upstream.write_all(&third[2..5]).unwrap();
    setup.upstream.write_all(&third[5..]).unwrap();
    assert_eq!(frame(&setup.received.next()), third);
}

// parity: TS-lib-herdrClientProxy-b67a64
#[test]
fn tab_focus_waits_for_success_and_forwards_server_error() {
    let mut setup = fixture();
    initialize(&mut setup);
    let selecting = request_in_background(&setup.proxy, "tab.focus", json!({ "tab_id": "w1:t2" }));
    let sent = endpoint_request(&setup.sent.next());
    assert_eq!(sent.request["method"], "tab.focus");
    assert_eq!(sent.request["params"], json!({ "tab_id": "w1:t2" }));
    setup
        .upstream
        .write_all(&response(&sent.boot_id, sent.id(), ok_body()))
        .unwrap();
    assert_eq!(selecting.join().unwrap(), Ok(json!({ "type": "ok" })));
    let missing =
        request_in_background(&setup.proxy, "tab.focus", json!({ "tab_id": "w1:missing" }));
    let missing_request = endpoint_request(&setup.sent.next());
    let error_body = json!({ "error": { "code": "not_found", "message": "tab missing" } });
    setup
        .upstream
        .write_all(&response(
            &missing_request.boot_id,
            missing_request.id(),
            error_body,
        ))
        .unwrap();
    assert_eq!(missing.join().unwrap(), Err("tab missing".to_string()));
}

// parity: TS-lib-herdrClientProxy-2bb695
#[test]
fn sends_handshake_time_requests_and_input_after_initial_space_selection() {
    let mut setup = fixture();
    let selecting = request_in_background(&setup.proxy, "tab.focus", json!({ "tab_id": "w1:t2" }));
    thread::sleep(PROXY_READ_GRACE);
    setup.upstream.write_all(&default_snapshot()).unwrap();
    let initial = endpoint_request(&setup.sent.next());
    let input = frame(&[18, 1]);
    setup.client.write_all(&input).unwrap();
    setup
        .upstream
        .write_all(&response(&initial.boot_id, initial.id(), ok_body()))
        .unwrap();
    setup.received.next();
    let forwarded = [setup.sent.next(), setup.sent.next()];
    let after_focus = endpoint_request(forwarded.iter().find(|payload| payload[0] == 15).unwrap());
    assert_eq!(after_focus.request["method"], "tab.focus");
    setup
        .upstream
        .write_all(&response(&after_focus.boot_id, after_focus.id(), ok_body()))
        .unwrap();
    assert!(selecting.join().unwrap().is_ok());
    assert_eq!(
        frame(forwarded.iter().find(|payload| payload[0] == 18).unwrap()),
        input
    );
}

// parity: TS-lib-herdrClientProxy-da5c62
#[test]
fn reselects_initial_space_when_server_boot_id_changes() {
    let mut setup = fixture();
    initialize(&mut setup);
    setup
        .upstream
        .write_all(&snapshot("boot-2", "w1", "w1:t1"))
        .unwrap();
    let initial = endpoint_request(&setup.sent.next());
    assert_eq!(initial.boot_id, "boot-2");
    assert_eq!(initial.request["method"], "workspace.focus");
    finish_initialization(&mut setup.upstream, &mut setup.received, &initial);
}

// parity: TS-lib-herdrClientProxy-1b970e
#[test]
fn completes_new_initialization_when_server_changes_during_initial_selection() {
    let mut setup = fixture();
    setup
        .upstream
        .write_all(&snapshot("boot-old", "w1", "w1:t1"))
        .unwrap();
    setup.sent.next();
    setup.received.next();
    setup
        .upstream
        .write_all(&snapshot("boot-new", "w1", "w1:t1"))
        .unwrap();
    let initial = endpoint_request(&setup.sent.next());
    assert_eq!(initial.boot_id, "boot-new");
    setup
        .upstream
        .write_all(&response(&initial.boot_id, initial.id(), ok_body()))
        .unwrap();
    setup.received.next();
    let selecting = request_in_background(&setup.proxy, "tab.focus", json!({ "tab_id": "w1:t2" }));
    let sent = endpoint_request(&setup.sent.next());
    setup
        .upstream
        .write_all(&response(&sent.boot_id, sent.id(), ok_body()))
        .unwrap();
    assert!(selecting.join().unwrap().is_ok());
}

// parity: TS-lib-herdrClientProxy-d10331
#[test]
fn does_not_replay_input_held_for_previous_boot() {
    let mut setup = fixture();
    setup
        .upstream
        .write_all(&snapshot("boot-old", "w2", "w2:t1"))
        .unwrap();
    setup.sent.next();
    setup.client.write_all(&frame(&[18, 1])).unwrap();
    thread::sleep(PROXY_READ_GRACE);
    setup
        .upstream
        .write_all(&snapshot("boot-new", "w2", "w2:t1"))
        .unwrap();
    let initial = endpoint_request(&setup.sent.next());
    setup
        .upstream
        .write_all(&response(&initial.boot_id, initial.id(), ok_body()))
        .unwrap();
    setup
        .upstream
        .write_all(&snapshot("boot-new", "w1", "w1:t1"))
        .unwrap();
    setup.received.next();
    let current_input = frame(&[18, 0]);
    setup.client.write_all(&current_input).unwrap();
    assert_eq!(frame(&setup.sent.next()), current_input);
}

// parity: TS-lib-herdrClientProxy-834e36
#[test]
fn fails_without_sending_when_endpoint_request_exceeds_size_limit() {
    let mut setup = fixture();
    initialize(&mut setup);
    let oversized = json!({ "tab_id": "x".repeat(1024 * 1024) });

    let result = setup.proxy.request("tab.focus", oversized);

    assert!(result.unwrap_err().contains("요청 크기 초과"));
}

// parity: TS-lib-herdrClientProxy-c745b0
#[test]
fn oversized_frame_closes_connection_and_fails_pending_request() {
    let mut setup = fixture();
    initialize(&mut setup);
    let pending = request_in_background(&setup.proxy, "tab.focus", json!({ "tab_id": "w1:t2" }));
    setup.sent.next();
    let prefix = (32 * 1024 * 1024 + 1_u32).to_le_bytes();
    setup.upstream.write_all(&prefix).unwrap();
    setup.received.wait_until_closed();
    assert_eq!(
        pending.join().unwrap(),
        Err("Herdr 프레임 크기가 올바르지 않습니다.".to_string())
    );
}

// parity: TS-lib-herdrClientProxy-3aaf76
#[test]
fn endpoint_request_without_response_times_out() {
    let mut setup = fixture();
    initialize(&mut setup);
    let pending = request_in_background(&setup.proxy, "tab.focus", json!({ "tab_id": "w1:t2" }));
    setup.sent.next();
    let error = pending.join().unwrap().unwrap_err();
    assert!(error.contains("시간 초과"), "{error}");
}

// parity: TS-lib-herdrClientProxy-d594b9
#[test]
fn reconnected_tui_gets_fresh_space_selection_on_same_server() {
    let mut setup = fixture();
    initialize(&mut setup);
    setup.client.shutdown(std::net::Shutdown::Both).unwrap();
    let client = UnixStream::connect(setup.proxy.socket_path()).unwrap();
    let mut received = FrameStream::new(&client);
    let (mut upstream, _) = setup.server.accept().unwrap();
    let mut sent = FrameStream::new(&upstream);
    upstream.write_all(&default_snapshot()).unwrap();
    let initial = endpoint_request(&sent.next());
    assert_eq!(initial.boot_id, "boot-1");
    assert_eq!(initial.request["method"], "workspace.focus");
    assert_eq!(initial.request["params"], json!({ "workspace_id": "w1" }));
    finish_initialization(&mut upstream, &mut received, &initial);
}

// parity: TS-lib-herdrClientProxy-6645cf
#[test]
fn close_removes_connection_and_socket_directory_and_allows_repeat() {
    let mut setup = fixture();
    initialize(&mut setup);
    let socket_path = setup.proxy.socket_path().to_path_buf();
    let socket_directory = socket_path.parent().unwrap().to_path_buf();
    assert_eq!(setup.proxy.active_pane_id().as_deref(), Some("w1:p1"));
    assert_eq!(
        fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&socket_directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        socket_directory.parent(),
        Some(setup.proxy_temp_root.path())
    );
    assert!(
        socket_directory
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("kvhc-")
    );

    setup.proxy.close();
    setup.proxy.close();

    assert_eq!(setup.proxy.active_pane_id(), None);
    assert!(!socket_directory.exists());
    assert!(
        setup
            .proxy
            .request("tab.focus", json!({ "tab_id": "w1:t2" }))
            .is_err()
    );
}

#[test]
fn factory_creates_relay_under_node_style_temp_directory() {
    let temp_root = tempfile::tempdir().unwrap();
    let parent_environment: kanvibe_process::shell_environment::ShellEnvironment = [(
        "TMPDIR".to_string(),
        format!("{}/", temp_root.path().display()),
    )]
    .into_iter()
    .collect();
    let environment = crate::environment::TerminalHostEnvironment::from_parent_environment(
        &parent_environment,
        kanvibe_process::shell_environment::ShellPlatform::Other,
        "/fallback",
    );
    let factory = NativeHerdrClientRelayFactory::from_host_environment(&environment);

    let relay = factory.create("/nonexistent/api", "w1").unwrap();

    let socket_path = PathBuf::from(relay.socket_path());
    assert_eq!(
        socket_path.parent().unwrap().parent(),
        Some(temp_root.path())
    );
    assert_eq!(relay.surface_size(), None);
    assert_eq!(relay.close(), Ok(()));
    assert!(!socket_path.parent().unwrap().exists());
}
