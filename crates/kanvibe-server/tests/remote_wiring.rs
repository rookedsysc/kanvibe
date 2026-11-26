//! 원격 호스트 조립(`desktop::remote`)이 서버의 터널 소켓과 런타임에 맞게 이어지는지 본다.
//! 실제 ssh·sshd는 쓰지 않는다. 시험 HOME에는 `~/.ssh/config`가 없어 어떤 호스트도 찾지 못한다.

#[path = "routes/harness/mod.rs"]
#[allow(dead_code, unused_imports)]
mod harness;

#[path = "../../kanvibe-runtime/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::collections::HashSet;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use kanvibe_remote_host::hook_tunnel::{HookTunnelRequestServer, HookTunnelServing};
use kanvibe_runtime::DesktopRuntime;
use kanvibe_server::desktop::hook_backend::DesktopHookBackend;
use kanvibe_server::desktop::remote::{RemoteHostWiring, RemoteHostWiringConfig};
use kanvibe_server::server::{RouteOptions, ServerConfig, ServerState, start_server};
use serde_json::{Value, json};

use harness::http_client::exchange;
use harness::{FakeBridge, GOOD, HttpCall, HttpReply, TestServer, failure, wait_until};
use support::RuntimeFixture;

const NAMESPACE: &str = "qa-remote-wiring";

struct TunnelSocket {
    path: PathBuf,
    hosts: Arc<RwLock<HashSet<String>>>,
    serving: Box<dyn HookTunnelServing>,
    _directory: tempfile::TempDir,
}

/// 터널 서비스가 하듯 소켓을 묶고 서버에 서빙을 맡긴다
fn serve_tunnel_socket(server: &dyn HookTunnelRequestServer, host: &str) -> TunnelSocket {
    let directory = tempfile::tempdir().expect("임시 폴더");
    let path = directory.path().join("tunnel.sock");
    let listener = UnixListener::bind(&path).expect("터널 소켓");
    let hosts = Arc::new(RwLock::new(HashSet::from([host.to_string()])));
    let serving = server.serve(listener, Arc::clone(&hosts));
    TunnelSocket {
        path,
        hosts,
        serving,
        _directory: directory,
    }
}

async fn request(socket: &Path, call: HttpCall) -> HttpReply {
    let stream = tokio::net::UnixStream::connect(socket)
        .await
        .expect("터널 소켓 연결");
    exchange(stream, &call).await.reply()
}

fn sorted_hosts(value: &Value) -> Vec<String> {
    let mut hosts: Vec<String> =
        serde_json::from_value(value["hosts"].clone()).expect("호스트 목록");
    hosts.sort();
    hosts
}

#[tokio::test]
async fn tunnel_socket_serves_only_hook_routes() {
    let server = TestServer::start().await;
    let tunnel = serve_tunnel_socket(&server.running.tunnel_server(), "alpha");

    let health = request(&tunnel.path, HttpCall::get("/api/hooks/health")).await;
    assert_eq!(health.pair(), (200, json!({ "success": true })));

    let mobile = request(&tunnel.path, HttpCall::get("/api/mobile/board").auth(GOOD)).await;
    assert_eq!(mobile.pair(), (404, failure("Not found")));

    let browser = HttpCall::get("/api/hooks/health").header("Origin", "http://evil.example");
    assert_eq!(request(&tunnel.path, browser).await.status, 403);
}

#[tokio::test]
async fn tunnel_requests_see_the_live_source_host_set() {
    let server = TestServer::start().await;
    let tunnel = serve_tunnel_socket(&server.running.tunnel_server(), "alpha");
    let list_tasks = || {
        HttpCall::new("POST", "/api/cli/tasks/list")
            .auth("Bearer task-token")
            .json(&json!({}))
    };

    request(&tunnel.path, list_tasks()).await;
    tunnel
        .hosts
        .write()
        .unwrap()
        .insert("alpha-alias".to_string());
    request(&tunnel.path, list_tasks()).await;

    let calls = server.fake.calls("task_cli.List");
    assert_eq!(calls.len(), 2);
    assert_eq!(sorted_hosts(&calls[0]), ["alpha"]);
    assert_eq!(sorted_hosts(&calls[1]), ["alpha", "alpha-alias"]);
}

#[tokio::test]
async fn stopping_the_serving_handle_refuses_new_connections() {
    let server = TestServer::start().await;
    let tunnel = serve_tunnel_socket(&server.running.tunnel_server(), "alpha");
    assert_eq!(
        request(&tunnel.path, HttpCall::get("/api/hooks/health"))
            .await
            .status,
        200
    );

    tunnel.serving.stop();

    wait_until(|| std::os::unix::net::UnixStream::connect(&tunnel.path).is_err()).await;
}

fn open_wiring(fixture: &RuntimeFixture) -> RemoteHostWiring {
    RemoteHostWiring::new(RemoteHostWiringConfig {
        home_directory: fixture.home(),
        base_environment: fixture.base_environment(),
        // 없는 디렉터리면 빈 목록이라 daemon을 올리지 않고 순수 ssh 경로로 간다
        agent_binary_directory: Some(fixture.path("missing-agent-binaries")),
        namespace: NAMESPACE.to_string(),
    })
    .expect("원격 호스트 조립")
}

fn open_runtime(fixture: &RuntimeFixture, wiring: &RemoteHostWiring) -> Arc<DesktopRuntime> {
    let mut config = fixture.config();
    config.remote_host_connections = wiring.host_connections();
    config.remote_hook_routing = wiring.hook_routing();
    Arc::new(DesktopRuntime::new(config).expect("런타임"))
}

/// 실제 hook 처리기(`DesktopHookBackend`)를 단 서버. 모바일 경계는 쓰지 않으므로 가짜를 둔다
async fn start_desktop_hook_server(
    runtime: &Arc<DesktopRuntime>,
    tls_directory: PathBuf,
) -> kanvibe_server::server::RunningServer {
    harness::tls_client::write_test_identity(&tls_directory);
    let state = ServerState::new(
        FakeBridge::new(),
        Arc::new(DesktopHookBackend::new(Arc::clone(runtime))),
        RouteOptions::default(),
    );
    let loopback = "127.0.0.1:0".parse().expect("주소");
    let config = ServerConfig {
        plaintext_address: loopback,
        tls_address: loopback,
        tls_directory,
    };
    start_server(&config, state).await.expect("서버 시작")
}

#[test]
fn late_bound_hook_routing_does_nothing_before_tunnels_start() {
    let fixture = RuntimeFixture::new();
    let wiring = open_wiring(&fixture);
    let routing = wiring.hook_routing();

    routing.ensure_hook_tunnel("remote-a");

    assert_eq!(routing.remote_host_namespace("remote-a"), Ok(None));
    assert_eq!(wiring.pool().namespace(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn hook_tunnels_bind_to_the_runtime_and_scope_tunnel_requests() {
    let fixture = RuntimeFixture::new();
    let wiring = open_wiring(&fixture);
    let runtime = open_runtime(&fixture, &wiring);
    let running = start_desktop_hook_server(&runtime, fixture.path("mobile-tls")).await;

    let tunnels = wiring
        .start_hook_tunnels(&runtime, running.tunnel_server())
        .expect("터널 시작");
    wait_until(|| wiring.pool().namespace().as_deref() == Some(NAMESPACE)).await;
    // ssh 설정에 없는 호스트는 legacy namespace로 돌리지 않고 연결 오류를 돌려준다(TS `getRemoteHostConnection`의 throw)
    assert_eq!(
        wiring.hook_routing().remote_host_namespace("remote-a"),
        Err("SSH 호스트를 찾을 수 없습니다: remote-a".to_string())
    );

    let tunnel = serve_tunnel_socket(&running.tunnel_server(), "remote-a");
    let status = HttpCall::new("POST", "/api/hooks/status")
        .auth("Bearer bogus")
        .json(&json!({ "taskId": "missing-task", "status": "progress" }));
    assert_eq!(request(&tunnel.path, status).await.status, 401);

    tunnels.stop();
    assert_eq!(wiring.pool().namespace(), None);
    running.shutdown();
}
