//! 원격 SSH 호스트 daemon 연결 풀, hook 터널(`ssh -R`과 daemon hook 재생), 원격 터미널 agent를 서버 조립에 잇는다
//! (TS `electron/main.js`·`electron/hookServer.js`의 `startHookTunnels`·`stopHookTunnels`, `remoteTerminalService`).
//!
//! 런타임은 서버보다 먼저 만들어지므로 연결 풀, hook 경로(namespace·터널), 원격 터미널 agent 권한은 런타임 앞에서
//! 시작 환경 스냅샷만으로 만들어 [`RemoteHostWiring::wire_runtime`]으로 `RuntimeConfig`에 넣고, 터널 서비스는
//! hook 포트를 잡은 뒤에 시작해 hook 경로에 늦게 묶는다. 포트를 얻지 못한 인스턴스가 터널을 열면 먼저 뜬 인스턴스의
//! 원격 소켓을 지우기 때문이다. 자식 ssh에는 `create_local_shell_environment`로 좁힌 환경만 넘긴다.

use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use kanvibe_domain::notifications::{HookTunnelFailedPayload, HookTunnelRecoveredPayload};
use kanvibe_pane_mirror::RemotePaneGrants;
use kanvibe_process::shell_environment::{
    ShellEnvironment, ShellPlatform, create_local_shell_environment,
};
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_remote_host::agent_binaries::AgentBinaryCatalog;
use kanvibe_remote_host::command_runner::DaemonRoutedCommandRunner;
use kanvibe_remote_host::hook_tunnel::{
    DatabaseHookClientIds, DatabaseRemoteHosts, HookTunnelDependencies, HookTunnelHealthNotifier,
    HookTunnelRequestServer, HookTunnelService, HookTunnelServing, SourceSshHosts,
    SystemHookTunnelProcessSpawner, TaskStatusFileRefresher,
};
use kanvibe_remote_host::pool::{RemoteHostConnectionPool, RemoteHostConnectionPoolDependencies};
use kanvibe_remote_host::registry_bindings::{RegistryHostConnections, TunnelHostTransport};
use kanvibe_remote_host::ssh_transport::{SystemEffectiveSshConfig, SystemHostTransportSpawner};
use kanvibe_remote_host::terminal_agent_seam::{
    RustRemoteTerminalAgentBundle, SelfSignedCertificateIssuer, UploadingRemoteTerminalAgentSpawner,
};
use kanvibe_runtime::{DesktopRuntime, RemoteHookRouting, RuntimeConfig};
use kanvibe_services::board_notifier::BoardNotifier;
use kanvibe_terminal::environment::TerminalHostEnvironment;
use kanvibe_terminal::remote_terminal_service::{
    RemoteTerminalService, RemoteTerminalServiceDependencies, SystemRemoteTerminalAgentSpawner,
};
use kanvibe_terminal::terminal_registry::RemoteHostConnections;
use tokio::sync::watch;

use crate::server::TunnelServer;

/// 원격 호스트 조립에 필요한 값. 모두 호출자가 시작할 때 정해 넘긴다
pub struct RemoteHostWiringConfig {
    /// 사용자 홈. `~/.ssh/config`의 기준이고 좁힌 자식 환경의 `HOME`이다
    pub home_directory: String,
    /// 시작할 때 읽어 둔 프로세스 환경. 여기서 좁혀 쓰고 그대로 자식에 넘기지 않는다
    pub base_environment: ShellEnvironment,
    /// `kanvibe-remote-agent-<OS>-<arch>` 바이너리가 있는 디렉터리. 없거나 비어 있으면 daemon을 올리지 못해
    /// 원격 호스트는 모두 순수 ssh 경로로 간다
    pub agent_binary_directory: Option<PathBuf>,
    /// 이 데스크탑의 원격 daemon·agent namespace(`prod`, `dev`, `qa-*`)
    pub namespace: String,
}

/// 런타임 앞에서 만들어 런타임·모바일 서비스·서버에 나눠 주는 원격 호스트 조립
pub struct RemoteHostWiring {
    namespace: String,
    environment: ShellEnvironment,
    pool: Arc<RemoteHostConnectionPool>,
    routing: Arc<LateBoundHookRouting>,
    terminal_agents: Arc<RemoteTerminalService>,
}

impl RemoteHostWiring {
    pub fn new(config: RemoteHostWiringConfig) -> Result<Self, String> {
        let binaries = Arc::new(match &config.agent_binary_directory {
            Some(directory) => AgentBinaryCatalog::from_directory(directory)?,
            None => AgentBinaryCatalog::default(),
        });
        let terminal_agents = build_terminal_agents(&config, &binaries);
        let environment = narrow_environment(config.base_environment, &config.home_directory);
        let pool = RemoteHostConnectionPool::new(RemoteHostConnectionPoolDependencies {
            ssh_hosts: Arc::new(SshConfigReader::new(&config.home_directory)),
            binaries: Arc::clone(&binaries),
            transport_spawner: Arc::new(SystemHostTransportSpawner::new(environment.clone())),
            effective_ssh_config: Arc::new(SystemEffectiveSshConfig::new(environment.clone())),
        });
        let routing = Arc::new(LateBoundHookRouting {
            pool: Arc::clone(&pool),
            tunnels: RwLock::new(None),
        });
        Ok(Self {
            namespace: config.namespace,
            environment,
            pool,
            routing,
            terminal_agents,
        })
    }

    /// 런타임을 만들기 전에 원격 daemon 연결, hook 경로, 원격 터미널 agent 권한을 설정에 넣는다.
    /// agent 권한은 런타임 pane 미러가 쓰고, 런타임 `shutdown()`이 미러를 멈출 때 agent도 함께 내린다
    pub fn wire_runtime(&self, config: &mut RuntimeConfig) {
        config.remote_host_connections = self.host_connections();
        config.remote_hook_routing = self.hook_routing();
        config.remote_pane_grants = self.remote_grants();
    }

    pub fn pool(&self) -> &Arc<RemoteHostConnectionPool> {
        &self.pool
    }

    /// `RuntimeConfig.remote_host_connections`에 넣는다. daemon을 쓸 수 없는 호스트는 `None`이라 순수 ssh로 간다
    pub fn host_connections(&self) -> Arc<dyn RemoteHostConnections> {
        Arc::new(RegistryHostConnections(Arc::clone(&self.pool)))
    }

    /// `RuntimeConfig.remote_hook_routing`에 넣는다. 터널을 시작하기 전에는 아무것도 하지 않는다
    pub fn hook_routing(&self) -> Arc<dyn RemoteHookRouting> {
        self.routing.clone()
    }

    /// `RuntimeConfig.remote_pane_grants`에 넣는 원격 터미널 서비스. agent는 Rust 바이너리 loader로 띄운다
    pub fn remote_grants(&self) -> Arc<dyn RemotePaneGrants> {
        self.terminal_agents.clone()
    }

    /// hook 포트를 잡은 뒤에 부른다(TS `logListenerLifecycle`의 listening 콜백). 돌려준 핸들을 멈추거나 놓으면 터널이 닫힌다
    pub fn start_hook_tunnels(
        &self,
        runtime: &DesktopRuntime,
        server: TunnelServer,
    ) -> Result<HookTunnelHandle, String> {
        let service = Arc::new(HookTunnelService::new(
            self.hook_tunnel_dependencies(runtime, server),
        ));
        service.start(Some(&self.namespace))?;
        *self.routing.tunnels.write().unwrap() = Some(Arc::clone(&service));
        Ok(HookTunnelHandle {
            service,
            routing: Arc::clone(&self.routing),
        })
    }

    /// 시작한 터널이 있으면 끊긴 연결을 곧바로 다시 잇는다(TS `reconnectHookTunnels`)
    pub fn reconnect_hook_tunnels(&self) {
        let tunnels = self.routing.tunnels.read().unwrap().clone();
        if let Some(tunnels) = tunnels {
            tunnels.reconnect_hook_tunnels();
        }
    }

    fn hook_tunnel_dependencies(
        &self,
        runtime: &DesktopRuntime,
        server: TunnelServer,
    ) -> HookTunnelDependencies {
        let database = runtime.database();
        // TS `execGit`처럼 daemon이 붙은 호스트의 원격 소켓 준비 명령은 daemon 연결로 보낸다
        let runner = DaemonRoutedCommandRunner::new(
            Arc::clone(runtime.shell_runner()),
            Arc::clone(&self.pool),
        );
        HookTunnelDependencies {
            request_server: Arc::new(server),
            host_transport: Arc::new(TunnelHostTransport(Arc::clone(&self.pool))),
            ssh_hosts: runtime.ssh_config().clone(),
            runner: Arc::new(runner),
            process_spawner: Arc::new(SystemHookTunnelProcessSpawner::from_parent_environment(
                &self.environment,
            )),
            remote_hosts: Arc::new(DatabaseRemoteHosts::new(Arc::clone(database))),
            hook_client_ids: Arc::new(DatabaseHookClientIds::new(Arc::clone(database))),
            status_refresher: Arc::new(TaskStatusFileRefresher::new(
                Arc::clone(database),
                Arc::clone(runtime.host_files()),
                runtime.board_notifier().clone(),
            )),
            health_notifier: Arc::new(BoardHookTunnelHealth(runtime.board_notifier().clone())),
        }
    }
}

/// 터널 실패·복구를 보드 이벤트로 보내 알림 서비스가 로그와 앱 알림함에 남기게 한다
struct BoardHookTunnelHealth(Arc<BoardNotifier>);

impl HookTunnelHealthNotifier for BoardHookTunnelHealth {
    fn broadcast_hook_tunnel_failed(&self, payload: HookTunnelFailedPayload) {
        self.0.broadcast_hook_tunnel_failed(payload);
    }

    fn broadcast_hook_tunnel_recovered(&self, payload: HookTunnelRecoveredPayload) {
        self.0.broadcast_hook_tunnel_recovered(payload);
    }
}

/// 런타임 터미널 환경과 같은 스냅샷·홈으로 agent용 ssh 환경과 `~/.ssh/config` 읽기를 만든다
fn build_terminal_agents(
    config: &RemoteHostWiringConfig,
    binaries: &Arc<AgentBinaryCatalog>,
) -> Arc<RemoteTerminalService> {
    let mut parent_environment = config.base_environment.clone();
    parent_environment.insert("HOME".into(), config.home_directory.clone());
    let terminal_environment = Arc::new(TerminalHostEnvironment::from_parent_environment(
        &parent_environment,
        ShellPlatform::current(),
        &config.home_directory,
    ));
    let ssh_spawner = Arc::new(SystemRemoteTerminalAgentSpawner::new(terminal_environment));
    Arc::new(RemoteTerminalService::new(
        RemoteTerminalServiceDependencies {
            ssh_hosts: Arc::new(SshConfigReader::new(&config.home_directory)),
            agent_bundle: Arc::new(RustRemoteTerminalAgentBundle::new(
                Arc::clone(binaries),
                &config.namespace,
            )),
            certificate_issuer: Arc::new(SelfSignedCertificateIssuer),
            agent_spawner: Arc::new(UploadingRemoteTerminalAgentSpawner::new(
                ssh_spawner,
                Arc::clone(binaries),
            )),
        },
    ))
}

/// 런타임과 같은 방식으로 좁힌다: `HOME`을 설정한 홈으로 맞춘 뒤 서버 런타임 값과 KanVibe 내부 값을 걷어 낸다
fn narrow_environment(
    base_environment: ShellEnvironment,
    home_directory: &str,
) -> ShellEnvironment {
    let mut parent_environment = base_environment;
    parent_environment.insert("HOME".into(), home_directory.to_string());
    create_local_shell_environment(
        &parent_environment,
        ShellPlatform::current(),
        home_directory,
    )
}

/// 런타임이 만들어질 때는 아직 없는 터널 서비스를 나중에 받아 쓰는 hook 경로
pub struct LateBoundHookRouting {
    pool: Arc<RemoteHostConnectionPool>,
    tunnels: RwLock<Option<Arc<HookTunnelService>>>,
}

impl RemoteHookRouting for LateBoundHookRouting {
    /// TS `await getRemoteHostConnection(sshHost) ? getRemoteHostNamespace() : undefined`
    fn remote_host_namespace(&self, ssh_host: &str) -> Option<String> {
        self.pool.connection(ssh_host).ok().flatten()?;
        self.pool.namespace()
    }

    fn ensure_hook_tunnel(&self, ssh_host: &str) {
        let tunnels = self.tunnels.read().unwrap().clone();
        if let Some(tunnels) = tunnels {
            tunnels.ensure_hook_tunnel(ssh_host);
        }
    }
}

/// 시작한 hook 터널. 멈추거나 놓으면 터널과 daemon 연결을 닫고 hook 경로에서 떼어 낸다(TS `stopHookTunnels`)
pub struct HookTunnelHandle {
    service: Arc<HookTunnelService>,
    routing: Arc<LateBoundHookRouting>,
}

impl HookTunnelHandle {
    pub fn stop(&self) {
        self.routing.tunnels.write().unwrap().take();
        self.service.stop();
    }
}

impl Drop for HookTunnelHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 터널 소켓 하나를 서빙하는 동안의 핸들. 송신자를 놓아도 서빙이 멈춘다
struct TunnelSocketServing {
    stop: Option<watch::Sender<bool>>,
}

impl HookTunnelServing for TunnelSocketServing {
    fn stop(&self) {
        if let Some(stop) = &self.stop {
            let _ = stop.send(true);
        }
    }
}

impl HookTunnelRequestServer for TunnelServer {
    /// 소켓을 서빙하지 못하면 기록만 남기고 빈 핸들을 돌려준다. 그 터널로 온 요청은 받지 못한다
    fn serve(
        &self,
        listener: UnixListener,
        source_ssh_hosts: SourceSshHosts,
    ) -> Box<dyn HookTunnelServing> {
        let stop = TunnelServer::serve(self, listener, source_ssh_hosts)
            .inspect_err(|error| eprintln!("[kanvibe] Hook tunnel socket failed: {error}"))
            .ok();
        Box::new(TunnelSocketServing { stop })
    }
}
