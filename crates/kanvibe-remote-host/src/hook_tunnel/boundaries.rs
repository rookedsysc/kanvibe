//! hook 터널 서비스가 바깥과 만나는 경계. 데스크탑 앱이 시작할 때 실제 구현을 끼우고, 테스트는 가짜를 끼운다.

use std::collections::HashSet;
use std::os::unix::net::UnixListener;
use std::sync::{Arc, RwLock};

use kanvibe_process::ssh_config::{SshConfigReader, SshHostConfig};

/// 터널 하나가 맡은 SSH 별칭. 같은 원격 계정을 가리키는 별칭이 나중에 더해지거나 빠져도 요청 처리기가 지금 값을 보도록
/// 서비스와 요청 처리기가 같은 값을 나눠 쥔다(TS `HookTunnel.sshHosts` Set을 그대로 넘기던 자리)
pub type SourceSshHosts = Arc<RwLock<HashSet<String>>>;

/// 원격 daemon이 넘긴 hook·task CLI 요청(`hook.request` 레코드의 내용)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHookRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// 데스크탑이 daemon에 돌려줄 응답(`hook.response` 레코드의 내용)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHookResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// daemon 연결이 받은 hook 요청을 처리한다. 오류면 연결이 503으로 답한다
pub type HostHookHandler =
    Arc<dyn Fn(HostHookRequest) -> Result<HostHookResponse, String> + Send + Sync>;

/// 원격 host daemon 연결 하나(TS `HostConnection` 가운데 터널이 쓰는 부분)
pub trait HookTunnelHostConnection: Send + Sync {
    fn set_hook_handler(&self, handler: HostHookHandler);
    fn close(&self);
    /// 연결이 닫히면 한 번 부른다. 이미 닫혔으면 바로 부른다
    fn on_close(&self, callback: Box<dyn FnOnce() + Send>);
}

/// 호스트별 daemon 연결 풀(TS `remoteHostConnection` 모듈 함수들)
pub trait HookTunnelHostTransport: Send + Sync {
    /// 이 데스크탑의 namespace와 hook client ID로 daemon 연결을 쓰게 한다(TS `configureRemoteHostTransport`)
    fn configure(&self, namespace: &str, client_id: &str) -> Result<(), String>;
    /// 설정된 namespace. 없으면 daemon을 쓰지 않는다(TS `getRemoteHostNamespace`)
    fn namespace(&self) -> Option<String>;
    /// `ssh -G`로 본 실제 접속 대상의 식별자(TS `getRemoteHostEndpointKey`)
    fn endpoint_key(&self, config: &SshHostConfig) -> Result<String, String>;
    /// daemon을 쓸 수 없는 호스트면 `None`이다(TS `getRemoteHostConnection`)
    fn connection(
        &self,
        ssh_host: &str,
    ) -> Result<Option<Arc<dyn HookTunnelHostConnection>>, String>;
    /// 모든 연결을 닫고 설정을 지운다(TS `stopRemoteHostConnections`)
    fn stop(&self);
}

/// 터널 로컬 소켓에서 hook·task CLI 요청을 처리하는 서버. 서버 crate가 구현한다.
/// 요청이 다룰 수 있는 task는 `source_ssh_hosts`로 `kanvibe_process::request_scope::is_task_reachable_from_tunnel`이 가린다
pub trait HookTunnelRequestServer: Send + Sync {
    fn serve(
        &self,
        listener: UnixListener,
        source_ssh_hosts: SourceSshHosts,
    ) -> Box<dyn HookTunnelServing>;
}

/// 서버가 소켓을 듣는 동안의 핸들. `stop`하면 더 받지 않는다
pub trait HookTunnelServing: Send + Sync {
    fn stop(&self);
}

/// `~/.ssh/config`의 호스트 정의 조회
pub trait SshHostLookup: Send + Sync {
    fn find_host(&self, ssh_host: &str) -> Option<SshHostConfig>;
}

impl SshHostLookup for SshConfigReader {
    fn find_host(&self, ssh_host: &str) -> Option<SshHostConfig> {
        self.parse_ssh_config()
            .into_iter()
            .find(|config| config.host == ssh_host)
    }
}

/// 원격 task나 원격 프로젝트가 가리키는 SSH 호스트(중복 포함). 완료한 task도 넣는다
pub trait RemoteHostsSource: Send + Sync {
    fn remote_ssh_hosts(&self) -> Result<Vec<String>, String>;
}

/// 이 설치의 hook client ID(TS `getOrCreateHookClientId`)
pub trait HookClientIdSource: Send + Sync {
    fn hook_client_id(&self) -> Result<String, String>;
}

/// daemon에 다시 붙은 호스트의 task 상태 파일을 DB에 반영한다(TS `refreshHostTaskStatuses`)
pub trait HostTaskStatusRefresher: Send + Sync {
    fn refresh_host_task_statuses(
        &self,
        ssh_hosts: &HashSet<String>,
        is_current: &dyn Fn() -> bool,
    ) -> Result<(), String>;
}

/// `ssh -R` 터널 프로세스 하나
pub trait HookTunnelProcess: Send + Sync {
    fn kill(&self);
    /// 끝날 때까지 기다린다. 종료 코드와 stderr 끝부분을 돌려준다
    fn wait(&self) -> HookTunnelProcessExit;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookTunnelProcessExit {
    pub exit_code: Option<i32>,
    pub last_error_output: String,
}

pub trait HookTunnelProcessSpawner: Send + Sync {
    fn spawn(&self, ssh_arguments: &[String]) -> Result<Arc<dyn HookTunnelProcess>, String>;
}
