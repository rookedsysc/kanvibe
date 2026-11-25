//! 원격 계정 하나의 터널과 그 감시 스레드(TS `connectHookTunnel`, `scheduleHookTunnelReconnect`).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

use kanvibe_process::exec::ExecGitOptions;
use kanvibe_process::hooks::socket_path::build_remote_hook_socket_prepare_command;

use super::boundaries::{
    HookTunnelHostConnection, HookTunnelProcess, HookTunnelServing, HostHookRequest,
    HostHookResponse, SourceSshHosts,
};
use super::replay::replay_hook_request;
use super::ssh_args::{
    build_hook_tunnel_base_args, build_hook_tunnel_ssh_args, validate_remote_socket_path,
};
use super::{ServiceShared, TunnelTimings};

#[derive(Default)]
struct TunnelState {
    is_closed: bool,
    reconnect_delay: Duration,
    process: Option<Arc<dyn HookTunnelProcess>>,
    host_connection: Option<Arc<dyn HookTunnelHostConnection>>,
    is_host_connection_closed: bool,
    should_reconnect_now: bool,
}

pub(crate) struct HookTunnel {
    endpoint_key: String,
    /// 연결에 쓰는 별칭. 같은 원격 계정이라 어느 별칭으로 붙어도 소켓 위치는 같다
    connect_host: String,
    ssh_hosts: SourceSshHosts,
    local_socket_path: PathBuf,
    serving: Box<dyn HookTunnelServing>,
    state: Mutex<TunnelState>,
    state_changed: Condvar,
}

/// 한 번 이은 결과
enum TunnelConnection {
    Process(Arc<dyn HookTunnelProcess>),
    HostConnection(Arc<dyn HookTunnelHostConnection>),
}

impl HookTunnel {
    pub(super) fn new(
        endpoint_key: &str,
        connect_host: &str,
        ssh_hosts: SourceSshHosts,
        local_socket_path: PathBuf,
        serving: Box<dyn HookTunnelServing>,
        reconnect_min_delay: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            endpoint_key: endpoint_key.to_string(),
            connect_host: connect_host.to_string(),
            ssh_hosts,
            local_socket_path,
            serving,
            state: Mutex::new(TunnelState {
                reconnect_delay: reconnect_min_delay,
                ..TunnelState::default()
            }),
            state_changed: Condvar::new(),
        })
    }

    pub(super) fn endpoint_key(&self) -> &str {
        &self.endpoint_key
    }

    pub(super) fn serves_host(&self, ssh_host: &str) -> bool {
        self.ssh_hosts.read().unwrap().contains(ssh_host)
    }

    pub(super) fn add_host(&self, ssh_host: &str) {
        self.ssh_hosts.write().unwrap().insert(ssh_host.to_string());
    }

    /// 더는 쓰지 않는 별칭을 빼고, 남은 별칭이 있는지 돌려준다
    pub(super) fn retain_hosts(&self, active_hosts: &HashSet<String>) -> bool {
        let mut ssh_hosts = self.ssh_hosts.write().unwrap();
        ssh_hosts.retain(|ssh_host| active_hosts.contains(ssh_host));
        !ssh_hosts.is_empty()
    }

    fn is_closed(&self) -> bool {
        self.state.lock().unwrap().is_closed
    }

    /// 터널을 닫는다. 감시 스레드는 다시 잇지 않고 끝난다
    pub(super) fn close(&self) {
        let process = {
            let mut state = self.state.lock().unwrap();
            state.is_closed = true;
            state.process.clone()
        };
        self.state_changed.notify_all();
        if let Some(process) = process {
            process.kill();
        }
        self.serving.stop();
        let _ = std::fs::remove_file(&self.local_socket_path);
    }

    /// 대기를 처음 값으로 되돌리고 지금 연결을 끊어 곧바로 다시 잇게 한다
    pub(super) fn reconnect_now(&self, reconnect_min_delay: Duration) {
        let (host_connection, process) = {
            let mut state = self.state.lock().unwrap();
            state.reconnect_delay = reconnect_min_delay;
            let is_waiting = state.host_connection.is_none() && state.process.is_none();
            state.should_reconnect_now = is_waiting;
            (state.host_connection.clone(), state.process.clone())
        };
        self.state_changed.notify_all();
        match (host_connection, process) {
            (Some(host_connection), _) => host_connection.close(),
            (None, Some(process)) => process.kill(),
            (None, None) => {}
        }
    }

    /// 다음 연결 전까지 기다리고 대기를 두 배로 늘린다. 닫혔으면 false다
    fn wait_before_reconnect(&self, timings: &TunnelTimings) -> bool {
        let mut state = self.state.lock().unwrap();
        let delay = state.reconnect_delay;
        state.reconnect_delay = (delay * 2).min(timings.reconnect_max_delay);
        let (mut state, _) = self
            .state_changed
            .wait_timeout_while(state, delay, |state| {
                !state.is_closed && !state.should_reconnect_now
            })
            .unwrap();
        state.should_reconnect_now = false;
        !state.is_closed
    }

    fn is_current_connection(&self, host_connection: &Arc<dyn HookTunnelHostConnection>) -> bool {
        let state = self.state.lock().unwrap();
        !state.is_closed
            && state
                .host_connection
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, host_connection))
    }
}

pub(super) fn start_supervising(shared: &Arc<ServiceShared>, tunnel: &Arc<HookTunnel>) {
    let (shared, tunnel) = (Arc::clone(shared), Arc::clone(tunnel));
    std::thread::spawn(move || supervise(&shared, &tunnel));
}

/// 터널이 닫힐 때까지 잇고, 끊기면 기다렸다 다시 잇는다. daemon 연결이 끊기면 기다리지 않고 바로 다시 잇는다
fn supervise(shared: &Arc<ServiceShared>, tunnel: &Arc<HookTunnel>) {
    while !tunnel.is_closed() {
        match connect(shared, tunnel) {
            Ok(Some(TunnelConnection::HostConnection(host_connection))) => {
                watch_host_connection(tunnel, &host_connection);
                continue;
            }
            Ok(Some(TunnelConnection::Process(process))) => watch_process(shared, tunnel, &process),
            Ok(None) => return,
            Err(error) => eprintln!(
                "[kanvibe] Hook tunnel to {} failed: {error}",
                tunnel.connect_host
            ),
        }
        if !tunnel.wait_before_reconnect(&shared.timings) {
            return;
        }
    }
}

fn connect(
    shared: &Arc<ServiceShared>,
    tunnel: &Arc<HookTunnel>,
) -> Result<Option<TunnelConnection>, String> {
    let dependencies = &shared.dependencies;
    let config = dependencies
        .ssh_hosts
        .find_host(&tunnel.connect_host)
        .ok_or_else(|| format!("SSH 호스트를 찾을 수 없습니다: {}", tunnel.connect_host))?;
    let host_connection = dependencies
        .host_transport
        .connection(&tunnel.connect_host)?;
    if tunnel.is_closed() {
        return Ok(None);
    }
    if let Some(host_connection) = host_connection {
        attach_host_connection(shared, tunnel, &host_connection);
        return Ok(Some(TunnelConnection::HostConnection(host_connection)));
    }
    let client_id = dependencies.hook_client_ids.hook_client_id()?;
    let prepare_command =
        build_remote_hook_socket_prepare_command(&client_id).map_err(|error| error.to_string())?;
    let options = ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    };
    let remote_socket_path = dependencies
        .runner
        .exec_git(&prepare_command, Some(&tunnel.connect_host), &options)
        .map_err(|error| error.message)?;
    let remote_socket_path = remote_socket_path.trim();
    validate_remote_socket_path(remote_socket_path, &client_id)?;
    if tunnel.is_closed() {
        return Ok(None);
    }
    let local_socket_path = tunnel.local_socket_path.to_string_lossy();
    let arguments = build_hook_tunnel_ssh_args(
        &build_hook_tunnel_base_args(&config),
        remote_socket_path,
        &local_socket_path,
    );
    let process = dependencies.process_spawner.spawn(&arguments)?;
    Ok(Some(TunnelConnection::Process(process)))
}

fn watch_process(
    shared: &ServiceShared,
    tunnel: &HookTunnel,
    process: &Arc<dyn HookTunnelProcess>,
) {
    let started_at = Instant::now();
    {
        let mut state = tunnel.state.lock().unwrap();
        if state.is_closed {
            drop(state);
            process.kill();
            return;
        }
        state.process = Some(Arc::clone(process));
    }
    let exit = process.wait();
    let mut state = tunnel.state.lock().unwrap();
    state.process = None;
    if state.is_closed {
        return;
    }
    if started_at.elapsed() >= shared.timings.stable_after {
        state.reconnect_delay = shared.timings.reconnect_min_delay;
    }
    let exit_code = exit
        .exit_code
        .map_or("null".to_string(), |code| code.to_string());
    eprintln!(
        "[kanvibe] Hook tunnel to {} closed (exit {exit_code}): {}",
        tunnel.connect_host, exit.last_error_output
    );
}

/// daemon 연결로 들어온 hook 요청을 로컬 소켓에 다시 보내게 하고, 끊긴 사이 바뀐 task 상태를 다시 읽는다
fn attach_host_connection(
    shared: &Arc<ServiceShared>,
    tunnel: &Arc<HookTunnel>,
    host_connection: &Arc<dyn HookTunnelHostConnection>,
) {
    {
        let mut state = tunnel.state.lock().unwrap();
        state.host_connection = Some(Arc::clone(host_connection));
        state.is_host_connection_closed = false;
    }
    let replay_target = Arc::downgrade(tunnel);
    host_connection.set_hook_handler(Arc::new(move |request| {
        replay_through_tunnel(&replay_target, &request)
    }));
    let close_target = Arc::downgrade(tunnel);
    host_connection.on_close(Box::new(move || mark_host_connection_closed(&close_target)));
    let (refresher, tunnel, host_connection) = (
        Arc::clone(&shared.dependencies.status_refresher),
        Arc::clone(tunnel),
        Arc::clone(host_connection),
    );
    std::thread::spawn(move || {
        let ssh_hosts = tunnel.ssh_hosts.read().unwrap().clone();
        let is_current = || tunnel.is_current_connection(&host_connection);
        if let Err(error) = refresher.refresh_host_task_statuses(&ssh_hosts, &is_current) {
            eprintln!("[kanvibe] Host status refresh failed: {error}");
        }
    });
}

fn replay_through_tunnel(
    tunnel: &Weak<HookTunnel>,
    request: &HostHookRequest,
) -> Result<HostHookResponse, String> {
    let tunnel = tunnel.upgrade().filter(|tunnel| !tunnel.is_closed());
    let Some(tunnel) = tunnel.filter(|_| !request.path.is_empty() && !request.method.is_empty())
    else {
        return Err("Invalid hook request".into());
    };
    replay_hook_request(&tunnel.local_socket_path, request)
}

fn mark_host_connection_closed(tunnel: &Weak<HookTunnel>) {
    if let Some(tunnel) = tunnel.upgrade() {
        tunnel.state.lock().unwrap().is_host_connection_closed = true;
        tunnel.state_changed.notify_all();
    }
}

/// daemon 연결이 끊기거나 터널이 닫힐 때까지 기다린다
fn watch_host_connection(tunnel: &HookTunnel, host_connection: &Arc<dyn HookTunnelHostConnection>) {
    let state = tunnel.state.lock().unwrap();
    let mut state = tunnel
        .state_changed
        .wait_while(state, |state| {
            !state.is_closed && !state.is_host_connection_closed
        })
        .unwrap();
    let is_same = state
        .host_connection
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, host_connection));
    if is_same {
        state.host_connection = None;
    }
}
