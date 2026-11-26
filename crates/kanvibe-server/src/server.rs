//! 평문·TLS 리스너와 터널 소켓을 띄우고 모바일 소켓 이벤트를 잇는 서버 본체(TS `createHookServer`).
//!
//! 포트를 잡은 뒤에만 TLS를 연다. 인증서를 만들거나 읽지 못해도 hook 경로는 계속 동작해야 하므로
//! TLS 실패는 평문 서버를 막지 않고 기록만 남긴다.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::bridge::{MobileBridge, Subscription};
use crate::http::{ConnectionTimeouts, ListenerKind, PeerContext, serve_connection};
use crate::routes::hooks::{HOOK_HEALTH_PATH, HookBackend};
use crate::ws::DEVICE_UNPAIRED_CLOSE_CODE;
use crate::ws::socket::SocketHub;

/// TLS 포트는 hook 포트에서 이만큼 떨어져 있다(TS `MOBILE_TLS_PORT_OFFSET`)
pub const MOBILE_TLS_PORT_OFFSET: u16 = 10;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// LAN 평문 사용 기록(TS `lanPlaintextService.recordLanPlaintextResponse`)
pub trait LanPlaintextUsage: Send + Sync {
    fn record_lan_plaintext_response(&self, remote_address: IpAddr, url: &str, status: u16);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RouteOptions {
    /// 소스 개발 실행에서만 여는 `/api/mobile/dev/pairing-code`
    pub is_dev_pairing_route_enabled: bool,
}

pub struct ServerState {
    pub bridge: Arc<dyn MobileBridge>,
    pub hooks: Arc<dyn HookBackend>,
    pub lan_usage: Option<Arc<dyn LanPlaintextUsage>>,
    pub options: RouteOptions,
    pub sockets: SocketHub,
    pub connection_timeouts: ConnectionTimeouts,
}

impl ServerState {
    pub fn new(
        bridge: Arc<dyn MobileBridge>,
        hooks: Arc<dyn HookBackend>,
        options: RouteOptions,
    ) -> Self {
        Self {
            bridge,
            hooks,
            lan_usage: None,
            options,
            sockets: SocketHub::default(),
            connection_timeouts: ConnectionTimeouts::default(),
        }
    }

    /// 성공한 비루프백 평문 응답만 센다. health 확인은 사용으로 보지 않는다
    pub fn record_plaintext_response(
        &self,
        remote_address: Option<IpAddr>,
        url: &str,
        status: u16,
    ) {
        let Some(remote_address) = remote_address else {
            return;
        };
        let should_record = status < 400
            && url != HOOK_HEALTH_PATH
            && !crate::http::is_loopback_hook_address(remote_address);
        let Some(lan_usage) = self.lan_usage.clone().filter(|_| should_record) else {
            return;
        };
        let url = url.to_string();
        tokio::task::spawn_blocking(move || {
            lan_usage.record_lan_plaintext_response(remote_address, &url, status)
        });
    }
}

/// 서버를 띄울 주소와 인증서 위치
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub plaintext_address: SocketAddr,
    pub tls_address: SocketAddr,
    /// `<userData>/mobile-tls`
    pub tls_directory: PathBuf,
}

pub struct RunningServer {
    pub plaintext_address: SocketAddr,
    pub tls_address: Option<SocketAddr>,
    state: Arc<ServerState>,
    shutdown: watch::Sender<bool>,
    /// 기기 해제·알림·이벤트 구독. 종료할 때 풀어 서비스가 닫힌 서버에 신호를 보내지 않게 한다
    subscriptions: std::sync::Mutex<Vec<Subscription>>,
}

impl RunningServer {
    pub fn state(&self) -> &Arc<ServerState> {
        &self.state
    }

    /// 열린 모바일 소켓을 1001로 닫고 리스너를 멈춘다
    pub fn shutdown(&self) {
        let subscriptions = std::mem::take(
            &mut *self
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        drop(subscriptions);
        self.state.sockets.close_all(1001, "server-shutdown");
        let bridge = self.state.bridge.clone();
        tokio::task::spawn_blocking(move || bridge.close_all_mobile_ai_logins());
        let _ = self.shutdown.send(true);
    }

    /// 터널 소켓을 hook 경로로 서빙하는 쪽. hook 터널 서비스가 자기 스레드에서 부를 수 있게 런타임 핸들을 함께 쥔다
    pub fn tunnel_server(&self) -> TunnelServer {
        TunnelServer {
            state: self.state.clone(),
            shutdown: self.shutdown.subscribe(),
            runtime: tokio::runtime::Handle::current(),
        }
    }
}

#[derive(Clone)]
pub struct TunnelServer {
    state: Arc<ServerState>,
    shutdown: watch::Receiver<bool>,
    runtime: tokio::runtime::Handle,
}

impl TunnelServer {
    /// 터널 소켓 하나를 hook 경로로 서빙한다. 돌려준 송신자에 `true`를 보내면 멈춘다
    pub fn serve(
        &self,
        listener: std::os::unix::net::UnixListener,
        source_ssh_hosts: Arc<std::sync::RwLock<HashSet<String>>>,
    ) -> std::io::Result<watch::Sender<bool>> {
        listener.set_nonblocking(true)?;
        let _context = self.runtime.enter();
        let listener = tokio::net::UnixListener::from_std(listener)?;
        let (stop, stopped) = watch::channel(false);
        let kind = ListenerKind::Tunnel(source_ssh_hosts);
        self.runtime.spawn(accept_unix(
            listener,
            kind,
            self.state.clone(),
            stopped,
            self.shutdown.clone(),
        ));
        Ok(stop)
    }
}

/// 평문 리스너를 열고, TLS 인증서를 읽어 TLS 리스너를 연다
pub async fn start_server(
    config: &ServerConfig,
    state: ServerState,
) -> std::io::Result<RunningServer> {
    let state = Arc::new(state);
    let (shutdown, _) = watch::channel(false);
    let plaintext = TcpListener::bind(config.plaintext_address).await?;
    let plaintext_address = plaintext.local_addr()?;
    tokio::spawn(accept_tcp(
        plaintext,
        None,
        state.clone(),
        shutdown.subscribe(),
    ));
    let tls_address = start_tls_listener(config, &state, &shutdown).await;
    let subscriptions = connect_socket_events(&state);
    tokio::spawn(run_heartbeat(state.clone(), shutdown.subscribe()));
    Ok(RunningServer {
        plaintext_address,
        tls_address,
        state,
        shutdown,
        subscriptions: std::sync::Mutex::new(subscriptions),
    })
}

async fn start_tls_listener(
    config: &ServerConfig,
    state: &Arc<ServerState>,
    shutdown: &watch::Sender<bool>,
) -> Option<SocketAddr> {
    let acceptor = match crate::tls::load_or_create_tls_acceptor(&config.tls_directory) {
        Ok(acceptor) => acceptor,
        Err(error) => {
            eprintln!("[kanvibe] Mobile TLS certificate failed: {error}");
            return None;
        }
    };
    match TcpListener::bind(config.tls_address).await {
        Ok(listener) => {
            let address = listener.local_addr().ok();
            tokio::spawn(accept_tcp(
                listener,
                Some(acceptor),
                state.clone(),
                shutdown.subscribe(),
            ));
            address
        }
        Err(error) => {
            eprintln!("[kanvibe] Mobile TLS server failed: {error}");
            None
        }
    }
}

/// 기기 해제는 그 기기의 소켓을 4001로 닫고, 알림·이벤트는 열린 이벤트 소켓 모두에 보낸다
fn connect_socket_events(state: &Arc<ServerState>) -> Vec<Subscription> {
    let unpaired_state = state.clone();
    let unpaired = state
        .bridge
        .on_mobile_device_unpaired(Arc::new(move |device_id: String| {
            unpaired_state
                .bridge
                .close_mobile_ai_logins_for_device(&device_id);
            unpaired_state.sockets.close_device_sockets(
                &device_id,
                DEVICE_UNPAIRED_CLOSE_CODE,
                "device-unpaired",
            );
        }));
    let changed_state = state.clone();
    let notifications = state
        .bridge
        .on_mobile_notifications_changed(Arc::new(move || {
            changed_state
                .sockets
                .broadcast_event(&serde_json::json!({ "type": "notifications-changed" }));
        }));
    let event_state = state.clone();
    let events = state
        .bridge
        .subscribe_to_mobile_events(Arc::new(move |event| {
            event_state.sockets.broadcast_event(&event)
        }));
    vec![unpaired, notifications, events]
}

/// 죽은 Wi-Fi 연결은 close가 오지 않을 수 있어 pong이 없는 소켓을 거둔다
async fn run_heartbeat(state: Arc<ServerState>, mut shutdown: watch::Receiver<bool>) {
    let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
    interval.tick().await;
    loop {
        tokio::select! {
            _ = interval.tick() => state.sockets.heartbeat(),
            _ = shutdown.changed() => return,
        }
    }
}

async fn accept_tcp(
    listener: TcpListener,
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    state: Arc<ServerState>,
    mut shutdown: watch::Receiver<bool>,
) {
    let listener_kind = if acceptor.is_some() {
        ListenerKind::Tls
    } else {
        ListenerKind::Plain
    };
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _ = shutdown.changed() => return,
        };
        let Ok((stream, remote)) = accepted else {
            continue;
        };
        let _ = stream.set_nodelay(true);
        let peer = PeerContext {
            listener: listener_kind.clone(),
            remote_address: Some(remote.ip()),
        };
        let state = state.clone();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            match acceptor {
                None => serve_connection(stream, peer, state).await,
                Some(acceptor) => {
                    let handshake = tokio::time::timeout(
                        state.connection_timeouts.tls_handshake,
                        acceptor.accept(stream),
                    );
                    if let Ok(Ok(stream)) = handshake.await {
                        serve_connection(stream, peer, state).await;
                    }
                }
            }
        });
    }
}

async fn accept_unix(
    listener: tokio::net::UnixListener,
    kind: ListenerKind,
    state: Arc<ServerState>,
    mut stopped: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => accepted,
            _ = stopped.changed() => return,
            _ = shutdown.changed() => return,
        };
        let Ok((stream, _)) = accepted else {
            continue;
        };
        let peer = PeerContext {
            listener: kind.clone(),
            remote_address: None,
        };
        tokio::spawn(serve_connection(stream, peer, state.clone()));
    }
}
