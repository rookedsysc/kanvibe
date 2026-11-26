//! 앱 프로세스 안에서 도는 hook·task CLI·모바일 HTTP/WebSocket/TLS 서버(TS `electron/main.js` `startHookServer`와
//! `before-quit`의 `hookServer.close()`).
//!
//! 서버는 자기 tokio 런타임을 갖고, GPUI 앱과 QA 실행기가 같은 순서로 띄우고 내린다. 평문 포트를 잡은 뒤에만 hook 터널을
//! 연다. 포트를 잡지 못한 인스턴스가 터널을 열면 먼저 뜬 인스턴스의 원격 소켓을 지우기 때문이다(TS `logListenerLifecycle`).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::hook_backend::DesktopHookBackend;
use super::remote::{HookTunnelHandle, RemoteHostWiring};
use super::{DesktopServices, LanPlaintextRecorder};
use crate::server::{
    MOBILE_TLS_PORT_OFFSET, RouteOptions, RunningServer, ServerConfig, ServerState, start_server,
};

/// 종료할 때 열린 소켓·작업을 기다리는 상한
const RUNTIME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// 서버를 어디에 띄울지
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InProcessServerListen {
    /// hook 평문 포트. TLS는 이 포트 + 10이다
    pub hook_port: u16,
    /// `<userData>/mobile-tls`
    pub tls_directory: PathBuf,
    pub route_options: RouteOptions,
}

pub struct InProcessServer {
    tokio_runtime: Mutex<Option<tokio::runtime::Runtime>>,
    services: Arc<DesktopServices>,
    remote: RemoteHostWiring,
    server: RunningServer,
    hook_tunnels: Mutex<Option<HookTunnelHandle>>,
}

impl InProcessServer {
    /// 평문·TLS 리스너를 연 뒤 푸시·이벤트 전달을 켜고 hook 터널을 시작한다.
    /// 평문 포트를 잡지 못하면 오류를 돌려주고 아무것도 남기지 않는다. TLS 실패는 기록만 남긴다
    pub fn start(
        services: Arc<DesktopServices>,
        remote: RemoteHostWiring,
        listen: &InProcessServerListen,
    ) -> Result<Self, String> {
        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_name("kanvibe-server")
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        let config = server_config(&services, listen);
        let state = server_state(&services, listen.route_options);
        let server = tokio_runtime
            .block_on(start_server(&config, state))
            .map_err(|error| {
                format!(
                    "Hook server failed on {}: {error}",
                    config.plaintext_address
                )
            })?;
        eprintln!(
            "[kanvibe] Hook server listening on {} (mobile TLS {:?})",
            server.plaintext_address, server.tls_address
        );
        services.start();
        let tunnel_server = {
            let _context = tokio_runtime.enter();
            server.tunnel_server()
        };
        let hook_tunnels = remote
            .start_hook_tunnels(&services.runtime, tunnel_server)
            .inspect_err(|error| eprintln!("[kanvibe] Hook tunnels failed: {error}"))
            .ok();
        Ok(Self {
            tokio_runtime: Mutex::new(Some(tokio_runtime)),
            services,
            remote,
            server,
            hook_tunnels: Mutex::new(hook_tunnels),
        })
    }

    pub fn services(&self) -> &Arc<DesktopServices> {
        &self.services
    }

    pub fn plaintext_address(&self) -> SocketAddr {
        self.server.plaintext_address
    }

    pub fn tls_address(&self) -> Option<SocketAddr> {
        self.server.tls_address
    }

    /// 잠자기에서 깨어났을 때 끊긴 hook 터널을 곧바로 다시 잇는다(TS `powerMonitor.on("resume", reconnectHookTunnels)`)
    pub fn reconnect_hook_tunnels(&self) {
        self.remote.reconnect_hook_tunnels();
    }

    /// 이 서버가 연 것을 TS `before-quit` 순서로 닫는다: hook 터널과 리스너(열린 모바일 소켓은 1001로 닫는다),
    /// 푸시·이벤트 전달, 그다음 런타임(동기화 예약, pane 미러와 원격 terminal agent, 로컬 PTY), daemon 연결.
    /// 두 번째 호출은 아무것도 하지 않는다
    pub fn stop(&self) {
        let Some(tokio_runtime) = lock(&self.tokio_runtime).take() else {
            return;
        };
        if let Some(hook_tunnels) = lock(&self.hook_tunnels).take() {
            hook_tunnels.stop();
        }
        {
            let _context = tokio_runtime.enter();
            self.server.shutdown();
        }
        self.services.stop();
        self.services.runtime.shutdown();
        self.remote.pool().stop_connections();
        tokio_runtime.shutdown_timeout(RUNTIME_SHUTDOWN_TIMEOUT);
    }

    /// SIGTERM·SIGINT를 받으면 `on_signal`을 한 번 부른다. 앱은 여기서 정상 종료 경로(`cx.quit()`)로 보낸다
    /// (Electron은 Chromium `ShutdownDetector`가 같은 신호를 받아 `before-quit` 경로로 끝냈다)
    pub fn on_termination_signal(&self, on_signal: impl FnOnce() + Send + 'static) {
        if let Some(tokio_runtime) = lock(&self.tokio_runtime).as_ref() {
            tokio_runtime.spawn(async move {
                wait_for_termination_signal().await;
                on_signal();
            });
        }
    }

    /// 서버 tokio 런타임에서 future 하나를 끝까지 돌린다(QA 실행기가 종료 신호를 기다릴 때 쓴다). 멈춘 뒤면 `None`이다
    pub fn block_on<F: std::future::Future>(&self, future: F) -> Option<F::Output> {
        let handle = lock(&self.tokio_runtime).as_ref()?.handle().clone();
        Some(handle.block_on(future))
    }
}

impl Drop for InProcessServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// SIGTERM이나 SIGINT 가운데 먼저 오는 것을 기다린다
pub async fn wait_for_termination_signal() {
    let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 평문은 LAN 설정에 따른 바인드 주소(기본 loopback), TLS는 모든 주소다(TS `MOBILE_TLS_BIND_HOST`)
fn server_config(services: &DesktopServices, listen: &InProcessServerListen) -> ServerConfig {
    let now_ms = kanvibe_ai::javascript::now_ms() as i64;
    let bind_host: IpAddr = services
        .lan_usage
        .resolve_hook_server_bind_host(now_ms)
        .parse()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    ServerConfig {
        plaintext_address: SocketAddr::new(bind_host, listen.hook_port),
        tls_address: SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            listen.hook_port + MOBILE_TLS_PORT_OFFSET,
        ),
        tls_directory: listen.tls_directory.clone(),
    }
}

fn server_state(services: &Arc<DesktopServices>, route_options: RouteOptions) -> ServerState {
    let mut state = ServerState::new(
        services.clone(),
        Arc::new(DesktopHookBackend::new(services.runtime.clone())),
        route_options,
    );
    state.lan_usage = Some(Arc::new(LanPlaintextRecorder(services.lan_usage.clone())));
    state
}
