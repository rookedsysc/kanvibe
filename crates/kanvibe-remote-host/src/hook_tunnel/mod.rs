//! 원격 호스트의 hook·CLI가 평문 포트 대신 SSH 안으로 데스크탑에 돌아오도록 호스트별 터널을 유지한다
//! (TS `desktop/main/services/hookTunnelService.ts`).
//!
//! 원격 계정마다 로컬 소켓을 따로 두어, 요청이 어느 호스트에서 왔는지를 소켓만으로 알 수 있게 한다.
//! 같은 원격 계정을 가리키는 SSH 별칭들은 원격 소켓 경로가 같아 터널 하나를 함께 쓴다. 별칭마다 터널을 열면
//! 나중에 연 터널이 앞 터널의 소켓을 지워, 앞 별칭의 요청이 다른 별칭의 요청으로 들어온다.
//!
//! 터널은 두 방식 가운데 하나로 이어진다. 원격 host daemon을 쓸 수 있으면 daemon 연결이 넘긴 `hook.request`를
//! 로컬 소켓에 다시 보내고, 쓸 수 없으면 예전처럼 `ssh -R <원격 소켓>:<로컬 소켓>`을 띄운다.
//! 끊기면 5초부터 두 배씩(최대 5분) 기다렸다 다시 잇고, 30초 넘게 살아 있었으면 대기를 처음 값으로 되돌린다.
//! 잇지 못한 사유는 로그와 앱 알림으로 남기되, 재시도에서 같은 사유가 이어지면 한 번만 남긴다.
//! 스레드 모델: 터널마다 감시 스레드 하나, 주기 정리 스레드 하나. 호스트를 터널에 묶는 판단은 잠금 하나로 줄 세운다.

mod boundaries;
mod replay;
mod ssh_args;
mod system;
#[cfg(test)]
mod tests;
mod tunnel;

use std::collections::{HashMap, HashSet};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kanvibe_process::exec::ShellCommandRunner;

pub use boundaries::{
    HookClientIdSource, HookTunnelHealthNotifier, HookTunnelHostConnection,
    HookTunnelHostTransport, HookTunnelProcess, HookTunnelProcessExit, HookTunnelProcessSpawner,
    HookTunnelRequestServer, HookTunnelServing, HostHookHandler, HostHookRequest, HostHookResponse,
    HostTaskStatusRefresher, RemoteHostsSource, SourceSshHosts, SshHostLookup,
};
pub use replay::replay_hook_request;
pub use ssh_args::{build_hook_tunnel_ssh_args, build_plain_endpoint_key};
pub use system::{
    DatabaseHookClientIds, DatabaseRemoteHosts, SystemHookTunnelProcessSpawner,
    TaskStatusFileRefresher,
};
use tunnel::HookTunnel;

/// Unix 소켓 경로 길이 제한(약 104자)을 넘지 않도록 시스템 임시 디렉터리 대신 짧은 경로를 쓴다
const LOCAL_SOCKET_DIRECTORY_PREFIX: &str = "/tmp/kvh-";

/// 재연결·정리 주기. 운영 값은 TS와 같고, 테스트만 짧은 값을 쓴다
#[derive(Debug, Clone, Copy)]
pub(crate) struct TunnelTimings {
    pub reconnect_min_delay: Duration,
    pub reconnect_max_delay: Duration,
    /// 이만큼 살아 있었으면 정상 연결로 보고 재연결 대기를 처음 값으로 되돌린다
    pub stable_after: Duration,
    pub reconcile_interval: Duration,
}

impl TunnelTimings {
    const PRODUCTION: Self = Self {
        reconnect_min_delay: Duration::from_millis(5_000),
        reconnect_max_delay: Duration::from_millis(5 * 60_000),
        stable_after: Duration::from_millis(30_000),
        reconcile_interval: Duration::from_millis(5 * 60_000),
    };
}

/// 서비스가 쓰는 협력자. 앱이 시작할 때 한 번 만든다
pub struct HookTunnelDependencies {
    pub request_server: Arc<dyn HookTunnelRequestServer>,
    pub host_transport: Arc<dyn HookTunnelHostTransport>,
    pub ssh_hosts: Arc<dyn SshHostLookup>,
    /// 원격 소켓 준비 명령을 실행한다(`exec_git`, 호스트 키 엄격 확인)
    pub runner: Arc<dyn ShellCommandRunner>,
    pub process_spawner: Arc<dyn HookTunnelProcessSpawner>,
    pub remote_hosts: Arc<dyn RemoteHostsSource>,
    pub hook_client_ids: Arc<dyn HookClientIdSource>,
    pub status_refresher: Arc<dyn HostTaskStatusRefresher>,
    pub health_notifier: Arc<dyn HookTunnelHealthNotifier>,
}

pub struct HookTunnelService {
    shared: Arc<ServiceShared>,
}

pub(crate) struct ServiceShared {
    dependencies: HookTunnelDependencies,
    timings: TunnelTimings,
    state: Mutex<ServiceState>,
    /// 정리 스레드를 깨운다(멈춤)
    state_changed: Condvar,
    /// 별칭을 터널에 묶는 판단이 겹치면 같은 원격 계정에 터널이 둘 생기므로 차례로 처리한다
    tunnel_operation: Mutex<()>,
}

#[derive(Default)]
struct ServiceState {
    is_started: bool,
    /// 시작·멈춤마다 늘어 지난 세대의 정리 스레드가 스스로 끝나게 한다
    generation: u64,
    local_socket_directory: Option<PathBuf>,
    tunnels_by_endpoint: HashMap<String, Arc<HookTunnel>>,
}

impl HookTunnelService {
    pub fn new(dependencies: HookTunnelDependencies) -> Self {
        Self::with_timings(dependencies, TunnelTimings::PRODUCTION)
    }

    pub(crate) fn with_timings(
        dependencies: HookTunnelDependencies,
        timings: TunnelTimings,
    ) -> Self {
        Self {
            shared: Arc::new(ServiceShared {
                dependencies,
                timings,
                state: Mutex::new(ServiceState::default()),
                state_changed: Condvar::new(),
                tunnel_operation: Mutex::new(()),
            }),
        }
    }

    /// 터널 관리를 시작한다(TS `startHookTunnels`). `namespace`가 있으면 daemon 연결을 먼저 설정한다.
    /// 이미 시작했으면 아무것도 하지 않는다
    pub fn start(&self, namespace: Option<&str>) -> Result<(), String> {
        let generation = {
            let mut state = self.shared.state.lock().unwrap();
            if state.is_started {
                return Ok(());
            }
            state.local_socket_directory = Some(create_local_socket_directory()?);
            state.is_started = true;
            state.generation += 1;
            state.generation
        };
        let shared = Arc::clone(&self.shared);
        let namespace = namespace.map(str::to_string);
        std::thread::spawn(move || {
            if let Some(namespace) = namespace {
                configure_transport(&shared, &namespace);
            }
            reconcile_until_stopped(&shared, generation);
        });
        Ok(())
    }

    /// 호스트의 터널을 연다. 이미 있으면 그대로 두므로 hook을 설치할 때마다 불러도 된다(TS `ensureHookTunnel`)
    pub fn ensure_hook_tunnel(&self, ssh_host: &str) {
        let shared = Arc::clone(&self.shared);
        let ssh_host = ssh_host.to_string();
        std::thread::spawn(move || {
            if let Err(error) = open_hook_tunnel(&shared, &ssh_host) {
                eprintln!("[kanvibe] Hook tunnel to {ssh_host} failed: {error}");
            }
        });
    }

    /// 잠자기에서 깨면 끊긴 연결을 기다리지 않고 곧바로 다시 잇는다(TS `reconnectHookTunnels`)
    pub fn reconnect_hook_tunnels(&self) {
        for tunnel in self.shared.tunnels() {
            tunnel.reconnect_now(self.shared.timings.reconnect_min_delay);
        }
    }

    /// 모든 터널을 닫고 daemon 연결도 멈춘다(TS `stopHookTunnels`)
    pub fn stop(&self) {
        let (tunnels, directory) = {
            let mut state = self.shared.state.lock().unwrap();
            state.is_started = false;
            state.generation += 1;
            let tunnels: Vec<Arc<HookTunnel>> = state
                .tunnels_by_endpoint
                .drain()
                .map(|(_, tunnel)| tunnel)
                .collect();
            (tunnels, state.local_socket_directory.take())
        };
        self.shared.state_changed.notify_all();
        for tunnel in tunnels {
            tunnel.close();
        }
        if let Some(directory) = directory {
            let _ = std::fs::remove_dir_all(directory);
        }
        self.shared.dependencies.host_transport.stop();
    }
}

impl Drop for HookTunnelService {
    fn drop(&mut self) {
        self.stop();
    }
}

impl ServiceShared {
    fn tunnels(&self) -> Vec<Arc<HookTunnel>> {
        let state = self.state.lock().unwrap();
        state.tunnels_by_endpoint.values().cloned().collect()
    }

    fn remove_tunnel(&self, tunnel: &Arc<HookTunnel>) {
        let mut state = self.state.lock().unwrap();
        let is_same = state
            .tunnels_by_endpoint
            .get(tunnel.endpoint_key())
            .is_some_and(|current| Arc::ptr_eq(current, tunnel));
        if is_same {
            state.tunnels_by_endpoint.remove(tunnel.endpoint_key());
        }
    }
}

/// mkdtemp처럼 추측할 수 없는 이름의 0700 디렉터리를 만들어 같은 머신의 다른 사용자가 끼어들지 못하게 한다
fn create_local_socket_directory() -> Result<PathBuf, String> {
    for _ in 0..8 {
        let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..6].to_string();
        let directory = PathBuf::from(format!("{LOCAL_SOCKET_DIRECTORY_PREFIX}{suffix}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("Hook tunnel socket directory could not be created".into())
}

fn configure_transport(shared: &ServiceShared, namespace: &str) {
    let configured = shared
        .dependencies
        .hook_client_ids
        .hook_client_id()
        .and_then(|client_id| {
            shared
                .dependencies
                .host_transport
                .configure(namespace, &client_id)
        });
    if let Err(error) = configured {
        eprintln!("[kanvibe] Host transport configuration failed: {error}");
    }
}

/// 시작한 세대가 끝날 때까지 주기마다 터널을 원격 task·프로젝트에 맞춘다
fn reconcile_until_stopped(shared: &Arc<ServiceShared>, generation: u64) {
    loop {
        if let Err(error) = reconcile_hook_tunnels(shared, generation) {
            eprintln!("[kanvibe] Hook tunnel reconcile failed: {error}");
        }
        let state = shared.state.lock().unwrap();
        let (state, _) = shared
            .state_changed
            .wait_timeout_while(state, shared.timings.reconcile_interval, |state| {
                state.generation == generation
            })
            .unwrap();
        if state.generation != generation {
            return;
        }
    }
}

/// 원격 task나 원격 프로젝트가 남아 있는 호스트의 터널을 열고, 아무도 쓰지 않는 별칭과 터널을 닫는다
fn reconcile_hook_tunnels(shared: &Arc<ServiceShared>, generation: u64) -> Result<(), String> {
    let active_hosts: HashSet<String> = shared
        .dependencies
        .remote_hosts
        .remote_ssh_hosts()?
        .into_iter()
        .collect();
    let mut ordered_hosts: Vec<&String> = active_hosts.iter().collect();
    ordered_hosts.sort();
    for ssh_host in ordered_hosts {
        if shared.state.lock().unwrap().generation != generation {
            return Ok(());
        }
        if let Err(error) = open_hook_tunnel(shared, ssh_host) {
            eprintln!("[kanvibe] Hook tunnel to {ssh_host} failed: {error}");
        }
    }
    for tunnel in shared.tunnels() {
        if tunnel.retain_hosts(&active_hosts) {
            continue;
        }
        shared.remove_tunnel(&tunnel);
        tunnel.close();
    }
    Ok(())
}

fn open_hook_tunnel(shared: &Arc<ServiceShared>, ssh_host: &str) -> Result<(), String> {
    let _operation = shared.tunnel_operation.lock().unwrap();
    if !can_open_tunnel_for(shared, ssh_host) {
        return Ok(());
    }
    let config = shared
        .dependencies
        .ssh_hosts
        .find_host(ssh_host)
        .ok_or_else(|| format!("SSH 호스트를 찾을 수 없습니다: {ssh_host}"))?;
    let transport = &shared.dependencies.host_transport;
    let endpoint_key = match transport.namespace() {
        Some(_) => transport.endpoint_key(&config)?,
        None => build_plain_endpoint_key(&config),
    };
    let mut state = shared.state.lock().unwrap();
    if let Some(existing) = state.tunnels_by_endpoint.get(&endpoint_key) {
        existing.add_host(ssh_host);
        return Ok(());
    }
    let Some(directory) = state
        .local_socket_directory
        .clone()
        .filter(|_| state.is_started)
    else {
        return Ok(());
    };
    let socket_path = local_socket_path(&directory, state.tunnels_by_endpoint.len());
    let tunnel = bind_tunnel(shared, &endpoint_key, ssh_host, &socket_path)?;
    state
        .tunnels_by_endpoint
        .insert(endpoint_key, Arc::clone(&tunnel));
    drop(state);
    tunnel::start_supervising(shared, &tunnel);
    Ok(())
}

fn can_open_tunnel_for(shared: &ServiceShared, ssh_host: &str) -> bool {
    let state = shared.state.lock().unwrap();
    let is_ready = state.is_started && state.local_socket_directory.is_some();
    is_ready
        && !state
            .tunnels_by_endpoint
            .values()
            .any(|tunnel| tunnel.serves_host(ssh_host))
}

fn local_socket_path(directory: &Path, tunnel_count: usize) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    directory.join(format!("{tunnel_count}-{millis}.sock"))
}

fn bind_tunnel(
    shared: &ServiceShared,
    endpoint_key: &str,
    ssh_host: &str,
    socket_path: &Path,
) -> Result<Arc<HookTunnel>, String> {
    let listener = UnixListener::bind(socket_path)
        .map_err(|error| format!("Hook tunnel socket for {ssh_host} failed: {error}"))?;
    let ssh_hosts: SourceSshHosts = Arc::new(RwLock::new(HashSet::from([ssh_host.to_string()])));
    let serving = shared
        .dependencies
        .request_server
        .serve(listener, Arc::clone(&ssh_hosts));
    Ok(HookTunnel::new(
        endpoint_key,
        ssh_host,
        ssh_hosts,
        socket_path.to_path_buf(),
        serving,
        shared.timings.reconnect_min_delay,
    ))
}
