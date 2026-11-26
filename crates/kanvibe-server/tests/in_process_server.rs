//! 앱과 QA 실행기가 함께 쓰는 프로세스 안 서버(`desktop::in_process`)가 런타임 조립 위에서 포트를 열고 닫는지 본다.
//! 실제 ssh는 쓰지 않는다. 시험 HOME에는 `~/.ssh/config`가 없어 hook 터널이 어떤 호스트에도 붙지 않는다.

#[path = "../../kanvibe-runtime/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kanvibe_runtime::DesktopRuntime;
use kanvibe_server::desktop::in_process::{InProcessServer, InProcessServerListen};
use kanvibe_server::desktop::remote::{RemoteHostWiring, RemoteHostWiringConfig};
use kanvibe_server::desktop::{DesktopServerConfig, DesktopServices};
use kanvibe_server::server::{MOBILE_TLS_PORT_OFFSET, RouteOptions};

use support::RuntimeFixture;

/// 평문 포트와 TLS 포트(+10)가 둘 다 비어 있는 포트
fn free_hook_port() -> u16 {
    loop {
        let probe = TcpListener::bind("127.0.0.1:0").expect("빈 포트");
        let port = probe.local_addr().unwrap().port();
        let is_tls_free = port
            .checked_add(MOBILE_TLS_PORT_OFFSET)
            .is_some_and(|tls_port| TcpListener::bind(("0.0.0.0", tls_port)).is_ok());
        if is_tls_free {
            return port;
        }
    }
}

fn open_services(fixture: &RuntimeFixture) -> (Arc<DesktopServices>, RemoteHostWiring) {
    let wiring = RemoteHostWiring::new(RemoteHostWiringConfig {
        home_directory: fixture.home(),
        base_environment: fixture.base_environment(),
        agent_binary_directory: Some(fixture.path("missing-agent-binaries")),
        namespace: "qa-in-process".to_string(),
    })
    .expect("원격 호스트 조립");
    let mut config = fixture.config();
    wiring.wire_runtime(&mut config);
    let runtime = Arc::new(DesktopRuntime::new(config).expect("런타임"));
    let services = Arc::new(DesktopServices::new(
        runtime,
        DesktopServerConfig {
            home_directory: PathBuf::from(fixture.home()),
            mobile_tls_port: 0,
            push_relay: None,
        },
    ));
    (services, wiring)
}

fn listen_on(fixture: &RuntimeFixture, hook_port: u16) -> InProcessServerListen {
    InProcessServerListen {
        hook_port,
        tls_directory: fixture.path("mobile-tls"),
        route_options: RouteOptions::default(),
    }
}

fn health_status(address: SocketAddr) -> Option<String> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(
            b"GET /api/hooks/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
        )
        .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    response.lines().next().map(str::to_string)
}

#[test]
fn mobile_services_use_the_runtime_pane_mirror() {
    let fixture = RuntimeFixture::new();
    let (services, _wiring) = open_services(&fixture);

    assert!(Arc::ptr_eq(
        &services.mirror,
        services.runtime.pane_mirror()
    ));
}

#[test]
fn the_server_answers_hooks_until_it_is_stopped() {
    let fixture = RuntimeFixture::new();
    let (services, wiring) = open_services(&fixture);
    let hook_port = free_hook_port();

    let server = InProcessServer::start(services, wiring, &listen_on(&fixture, hook_port))
        .expect("서버 시작");
    let address = server.plaintext_address();
    let tls_address = server.tls_address().expect("TLS 리스너");

    assert_eq!(address.port(), hook_port);
    assert_eq!(tls_address.port(), hook_port + MOBILE_TLS_PORT_OFFSET);
    assert_eq!(health_status(address).as_deref(), Some("HTTP/1.1 200 OK"));
    assert!(fixture.path("mobile-tls/certificate.pem").exists());

    server.stop();
    server.stop();

    assert_eq!(health_status(address), None);
    assert!(
        TcpListener::bind(address).is_ok(),
        "평문 포트가 풀려야 한다"
    );
}

#[test]
fn a_taken_hook_port_is_reported_instead_of_starting_a_second_server() {
    let first_fixture = RuntimeFixture::new();
    let (first_services, first_wiring) = open_services(&first_fixture);
    let hook_port = free_hook_port();
    let first = InProcessServer::start(
        first_services,
        first_wiring,
        &listen_on(&first_fixture, hook_port),
    )
    .expect("첫 서버");
    let second_fixture = RuntimeFixture::new();
    let (second_services, second_wiring) = open_services(&second_fixture);

    let second = InProcessServer::start(
        second_services,
        second_wiring,
        &listen_on(&second_fixture, hook_port),
    );

    let error = second
        .err()
        .expect("같은 포트의 두 번째 서버는 실패해야 한다");
    assert!(error.contains("Hook server failed"), "{error}");
    assert_eq!(
        health_status(first.plaintext_address()).as_deref(),
        Some("HTTP/1.1 200 OK")
    );
    first.stop();
}
