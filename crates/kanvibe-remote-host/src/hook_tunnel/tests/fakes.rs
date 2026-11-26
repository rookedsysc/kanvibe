//! 터널 테스트의 가짜 경계와 fixture.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::notifications::{HookTunnelFailedPayload, HookTunnelRecoveredPayload};
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::host_file_access::HostFileAccess;
use kanvibe_process::ssh_config::SshHostConfig;
use kanvibe_process::task_state_service::BoardUpdateNotifier;

use super::super::*;
use super::{DatabaseFixture, REMOTE_SOCKET};
use crate::connection::RemoteHostConnectionError;

pub(super) const CLIENT_ID: &str = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";

/// TS 가짜 타이머 대신 쓰는 짧은 주기. 정상 연결 판정과 주기 정리는 테스트 안에서 일어나지 않게 길게 둔다
pub(super) const FAST_TIMINGS: TunnelTimings = TunnelTimings {
    reconnect_min_delay: Duration::from_millis(150),
    reconnect_max_delay: Duration::from_secs(10),
    stable_after: Duration::from_secs(60),
    reconcile_interval: Duration::from_secs(60),
};

pub(super) fn wait_until(description: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "{description} 시간 초과");
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(super) struct Fixture {
    pub service: HookTunnelService,
    pub spawner: Arc<FakeSpawner>,
    pub runner: Arc<FakeRunner>,
    pub server: Arc<FakeRequestServer>,
    pub transport: Arc<FakeTransport>,
    pub health: Arc<RecordingHealthNotifier>,
}

struct FixtureParts {
    remote_hosts: Arc<dyn RemoteHostsSource>,
    timings: TunnelTimings,
    transport: Arc<FakeTransport>,
    status_refresher: Arc<dyn HostTaskStatusRefresher>,
}

impl Fixture {
    pub fn new(hosts: &[&str]) -> Self {
        Self::with_remote_hosts(Arc::new(FakeRemoteHosts::new(hosts)), FAST_TIMINGS)
    }

    pub fn with_remote_hosts(
        remote_hosts: Arc<dyn RemoteHostsSource>,
        timings: TunnelTimings,
    ) -> Self {
        Self::build(FixtureParts {
            remote_hosts,
            timings,
            transport: Arc::new(FakeTransport::default()),
            status_refresher: Arc::new(NoopRefresher),
        })
    }

    /// daemon 연결을 쓰고 원격 상태 파일을 실제 DB에 반영하는 fixture. 원격 파일 읽기는 `remote_file`이 답한다
    pub fn with_daemon_status_sync(
        database: &DatabaseFixture,
        remote_file: Arc<FakeRunner>,
    ) -> Self {
        let files = Arc::new(HostFileAccess::new(remote_file, "/home/local"));
        let refresher =
            TaskStatusFileRefresher::new(database.shared(), files, Arc::new(NoopNotifier));
        Self::build(FixtureParts {
            remote_hosts: Arc::new(FakeRemoteHosts::new(&["host-a"])),
            timings: FAST_TIMINGS,
            transport: Arc::new(FakeTransport::daemon()),
            status_refresher: Arc::new(refresher),
        })
    }

    pub fn with_daemon_transport() -> Self {
        Self::build(FixtureParts {
            remote_hosts: Arc::new(FakeRemoteHosts::new(&["host-a"])),
            timings: FAST_TIMINGS,
            transport: Arc::new(FakeTransport::daemon()),
            status_refresher: Arc::new(NoopRefresher),
        })
    }

    fn build(parts: FixtureParts) -> Self {
        let spawner = Arc::new(FakeSpawner::default());
        let runner = Arc::new(FakeRunner::new(REMOTE_SOCKET));
        let server = Arc::new(FakeRequestServer::default());
        let health = Arc::new(RecordingHealthNotifier::default());
        let dependencies = HookTunnelDependencies {
            request_server: server.clone(),
            host_transport: parts.transport.clone(),
            ssh_hosts: Arc::new(FakeSshHosts),
            runner: runner.clone(),
            process_spawner: spawner.clone(),
            remote_hosts: parts.remote_hosts,
            hook_client_ids: Arc::new(FixedClientId),
            status_refresher: parts.status_refresher,
            health_notifier: health.clone(),
        };
        Self {
            service: HookTunnelService::with_timings(dependencies, parts.timings),
            spawner,
            runner,
            server,
            transport: parts.transport,
            health,
        }
    }

    pub fn start(&self) {
        self.service.start(None).unwrap();
    }
}

struct FakeSshHosts;

impl SshHostLookup for FakeSshHosts {
    fn find_host(&self, ssh_host: &str) -> Option<SshHostConfig> {
        let hostname = match ssh_host {
            "host-a" | "host-a-alias" => "10.0.0.7",
            "host-b" => "10.0.0.8",
            _ => return None,
        };
        Some(SshHostConfig {
            host: ssh_host.into(),
            hostname: hostname.into(),
            port: 22,
            username: "dev".into(),
            private_key_path: "/fixture-key".into(),
        })
    }
}

struct FixedClientId;

impl HookClientIdSource for FixedClientId {
    fn hook_client_id(&self) -> Result<String, String> {
        Ok(CLIENT_ID.into())
    }
}

/// 터널이 알린 실패와 복구를 차례대로 모은다
#[derive(Default)]
pub(super) struct RecordingHealthNotifier {
    failures: Mutex<Vec<HookTunnelFailedPayload>>,
    recoveries: Mutex<Vec<HookTunnelRecoveredPayload>>,
}

impl RecordingHealthNotifier {
    pub fn failures(&self) -> Vec<HookTunnelFailedPayload> {
        self.failures.lock().unwrap().clone()
    }

    pub fn recoveries(&self) -> Vec<HookTunnelRecoveredPayload> {
        self.recoveries.lock().unwrap().clone()
    }
}

impl HookTunnelHealthNotifier for RecordingHealthNotifier {
    fn broadcast_hook_tunnel_failed(&self, payload: HookTunnelFailedPayload) {
        self.failures.lock().unwrap().push(payload);
    }

    fn broadcast_hook_tunnel_recovered(&self, payload: HookTunnelRecoveredPayload) {
        self.recoveries.lock().unwrap().push(payload);
    }
}

struct NoopRefresher;

impl HostTaskStatusRefresher for NoopRefresher {
    fn refresh_host_task_statuses(
        &self,
        _: &HashSet<String>,
        _: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        Ok(())
    }
}

struct NoopNotifier;

impl BoardUpdateNotifier for NoopNotifier {
    fn broadcast_board_update(&self) {}
}

pub(super) struct FakeRemoteHosts {
    hosts: Mutex<Vec<String>>,
}

impl FakeRemoteHosts {
    pub fn new(hosts: &[&str]) -> Self {
        Self {
            hosts: Mutex::new(hosts.iter().map(|host| host.to_string()).collect()),
        }
    }

    pub fn replace(&self, hosts: &[&str]) {
        *self.hosts.lock().unwrap() = hosts.iter().map(|host| host.to_string()).collect();
    }
}

impl RemoteHostsSource for FakeRemoteHosts {
    fn remote_ssh_hosts(&self) -> Result<Vec<String>, String> {
        Ok(self.hosts.lock().unwrap().clone())
    }
}

/// 모든 명령에 같은 출력을 돌려주고, 한 번만 실패하게 할 수 있다
pub(super) struct FakeRunner {
    output: Mutex<String>,
    next_failure: Mutex<Option<String>>,
    calls: AtomicUsize,
}

impl FakeRunner {
    pub fn new(output: &str) -> Self {
        Self {
            output: Mutex::new(output.into()),
            next_failure: Mutex::new(None),
            calls: AtomicUsize::new(0),
        }
    }

    pub fn respond_with(&self, output: &str) {
        *self.output.lock().unwrap() = output.into();
    }

    pub fn fail_next(&self, message: &str) {
        *self.next_failure.lock().unwrap() = Some(message.into());
    }

    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ShellCommandRunner for FakeRunner {
    fn exec_git(
        &self,
        _: &str,
        _: Option<&str>,
        _: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(message) = self.next_failure.lock().unwrap().take() {
            return Err(ExecGitError::new(message));
        }
        Ok(self.output.lock().unwrap().clone())
    }
}

#[derive(Default)]
pub(super) struct FakeProcess {
    exit: Mutex<Option<Option<i32>>>,
    exited: Condvar,
    kills: AtomicUsize,
    error_output: Mutex<String>,
}

impl FakeProcess {
    /// ssh가 끝난 것처럼 만든다
    pub fn exit(&self, exit_code: Option<i32>) {
        self.exit.lock().unwrap().get_or_insert(exit_code);
        self.exited.notify_all();
    }

    /// stderr에 `error_output`을 남기고 끝난 ssh
    pub fn exit_with_error_output(&self, exit_code: i32, error_output: &str) {
        *self.error_output.lock().unwrap() = error_output.into();
        self.exit(Some(exit_code));
    }

    pub fn kill_count(&self) -> usize {
        self.kills.load(Ordering::SeqCst)
    }
}

impl HookTunnelProcess for FakeProcess {
    fn kill(&self) {
        self.kills.fetch_add(1, Ordering::SeqCst);
        self.exit(None);
    }

    fn wait(&self) -> HookTunnelProcessExit {
        let exit = self.exit.lock().unwrap();
        let exit = self.exited.wait_while(exit, |exit| exit.is_none()).unwrap();
        HookTunnelProcessExit {
            exit_code: exit.flatten(),
            last_error_output: self.error_output.lock().unwrap().clone(),
        }
    }
}

#[derive(Default)]
pub(super) struct FakeSpawner {
    spawned: Mutex<Vec<(Vec<String>, Arc<FakeProcess>)>>,
}

impl FakeSpawner {
    pub fn spawn_count(&self) -> usize {
        self.spawned.lock().unwrap().len()
    }

    pub fn wait_for_spawns(&self, count: usize) {
        wait_until(&format!("터널 {count}개"), || {
            self.spawn_count() >= count
        });
        assert_eq!(self.spawn_count(), count);
    }

    pub fn arguments(&self, index: usize) -> Vec<String> {
        self.spawned.lock().unwrap()[index].0.clone()
    }

    pub fn process(&self, index: usize) -> Arc<FakeProcess> {
        Arc::clone(&self.spawned.lock().unwrap()[index].1)
    }
}

impl HookTunnelProcessSpawner for FakeSpawner {
    fn spawn(&self, ssh_arguments: &[String]) -> Result<Arc<dyn HookTunnelProcess>, String> {
        let process = Arc::new(FakeProcess::default());
        self.spawned
            .lock()
            .unwrap()
            .push((ssh_arguments.to_vec(), Arc::clone(&process)));
        Ok(process)
    }
}

#[derive(Default)]
pub(super) struct FakeHostConnection {
    handler: Mutex<Option<HostHookHandler>>,
    close_callbacks: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    is_closed: AtomicBool,
    has_replaced_stale_peer: bool,
}

impl FakeHostConnection {
    pub fn handler(&self) -> Option<HostHookHandler> {
        self.handler.lock().unwrap().clone()
    }
}

impl HookTunnelHostConnection for FakeHostConnection {
    fn set_hook_handler(&self, handler: HostHookHandler) {
        *self.handler.lock().unwrap() = Some(handler);
    }

    fn close(&self) {
        self.is_closed.store(true, Ordering::SeqCst);
        let callbacks = std::mem::take(&mut *self.close_callbacks.lock().unwrap());
        for callback in callbacks {
            callback();
        }
    }

    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        if self.is_closed.load(Ordering::SeqCst) {
            callback();
            return;
        }
        self.close_callbacks.lock().unwrap().push(callback);
    }

    fn has_replaced_stale_peer(&self) -> bool {
        self.has_replaced_stale_peer
    }
}

#[derive(Default)]
pub(super) struct FakeTransport {
    is_daemon: bool,
    /// 다음에 여는 daemon 연결이 응답 없는 앞 연결을 교체한 것처럼 보이게 한다
    should_replace_stale_peer: AtomicBool,
    namespace: Mutex<Option<String>>,
    configured: Mutex<Vec<(String, String)>>,
    connections: Mutex<Vec<Arc<FakeHostConnection>>>,
    stops: AtomicUsize,
}

impl FakeTransport {
    fn daemon() -> Self {
        Self {
            is_daemon: true,
            ..Self::default()
        }
    }

    pub fn configured(&self) -> Vec<(String, String)> {
        self.configured.lock().unwrap().clone()
    }

    pub fn connection_count(&self) -> usize {
        self.connections.lock().unwrap().len()
    }

    pub fn connection_at(&self, index: usize) -> Arc<FakeHostConnection> {
        Arc::clone(&self.connections.lock().unwrap()[index])
    }

    pub fn stop_count(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }

    pub fn replace_stale_peer_on_next_connection(&self) {
        self.should_replace_stale_peer.store(true, Ordering::SeqCst);
    }
}

impl HookTunnelHostTransport for FakeTransport {
    fn configure(&self, namespace: &str, client_id: &str) -> Result<(), String> {
        self.configured
            .lock()
            .unwrap()
            .push((namespace.into(), client_id.into()));
        *self.namespace.lock().unwrap() = Some(namespace.into());
        Ok(())
    }

    fn namespace(&self) -> Option<String> {
        self.namespace.lock().unwrap().clone()
    }

    fn endpoint_key(&self, config: &SshHostConfig) -> Result<String, String> {
        Ok(format!("endpoint-{}", config.hostname))
    }

    fn connection(
        &self,
        _: &str,
    ) -> Result<Option<Arc<dyn HookTunnelHostConnection>>, RemoteHostConnectionError> {
        if !self.is_daemon || self.namespace().is_none() {
            return Ok(None);
        }
        let connection = Arc::new(FakeHostConnection {
            has_replaced_stale_peer: self.should_replace_stale_peer.swap(false, Ordering::SeqCst),
            ..FakeHostConnection::default()
        });
        self.connections
            .lock()
            .unwrap()
            .push(Arc::clone(&connection));
        Ok(Some(connection))
    }

    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
}

/// 터널 소켓에서 요청을 받아 기록하고 `202 ack`로 답하는 서버
#[derive(Default)]
pub(super) struct FakeRequestServer {
    host_sets: Mutex<Vec<SourceSshHosts>>,
    received: Arc<Mutex<Vec<String>>>,
}

impl FakeRequestServer {
    pub fn host_sets(&self) -> Vec<SourceSshHosts> {
        self.host_sets.lock().unwrap().clone()
    }

    pub fn received_requests(&self) -> Vec<String> {
        self.received.lock().unwrap().clone()
    }
}

impl HookTunnelRequestServer for FakeRequestServer {
    fn serve(
        &self,
        listener: UnixListener,
        source_ssh_hosts: SourceSshHosts,
    ) -> Box<dyn HookTunnelServing> {
        self.host_sets.lock().unwrap().push(source_ssh_hosts);
        let socket_path = listener
            .local_addr()
            .ok()
            .and_then(|address| address.as_pathname().map(PathBuf::from));
        let is_stopped = Arc::new(AtomicBool::new(false));
        let (stopped, received) = (Arc::clone(&is_stopped), Arc::clone(&self.received));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                answer_with_ack(stream, &received);
            }
        });
        Box::new(FakeServing {
            is_stopped,
            socket_path,
        })
    }
}

fn answer_with_ack(mut stream: UnixStream, received: &Mutex<Vec<String>>) {
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    while !is_complete_request(&request) {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(count) => request.extend_from_slice(&chunk[..count]),
        }
    }
    received
        .lock()
        .unwrap()
        .push(String::from_utf8_lossy(&request).into_owned());
    let _ = stream.write_all(b"HTTP/1.1 202 Accepted\r\ncontent-length: 3\r\n\r\nack");
}

fn is_complete_request(request: &[u8]) -> bool {
    let text = String::from_utf8_lossy(request);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    body.len() >= length
}

struct FakeServing {
    is_stopped: Arc<AtomicBool>,
    socket_path: Option<PathBuf>,
}

impl HookTunnelServing for FakeServing {
    fn stop(&self) {
        self.is_stopped.store(true, Ordering::SeqCst);
        // accept에서 기다리는 스레드를 깨워 끝내게 한다
        if let Some(socket_path) = &self.socket_path {
            let _ = UnixStream::connect(socket_path);
        }
    }
}
