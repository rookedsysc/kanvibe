//! hook 설치·검증이 데스크탑 client에게서 받아야 하는 값과 동작.
//! TS 설치기는 appSettingsService, hookEndpoint, remoteHostConnection, hookTunnelService를 직접 불렀지만,
//! 여기서는 그 경계를 trait 하나로 좁혀 설치기가 서버·원격 daemon(M4) 구현에 묶이지 않게 한다.

use std::sync::Arc;

use super::install_bundle::HookInstallScripts;
use crate::exec::ExecGitError;
use crate::host_file_access::{HostFileAccess, HostFileError};

#[derive(Debug, thiserror::Error)]
pub enum HookInstallError {
    #[error(transparent)]
    Files(#[from] HostFileError),
    #[error(transparent)]
    Exec(#[from] ExecGitError),
    /// TS가 `new Error(...)`로 던지던 한국어 메시지
    #[error("{0}")]
    Message(String),
}

/// 설치기가 쓰는 데스크탑 client 쪽 값과 동작
pub trait HookClientRuntime: Send + Sync {
    /// 이 데스크탑 설치를 가리키는 hook client ID(app_settings `hook_client_id`)
    fn hook_client_id(&self) -> Result<String, HookInstallError>;
    /// `HMAC(task_cli_secret, taskId)` task CLI 토큰
    fn task_cli_token(&self, task_id: &str) -> Result<String, HookInstallError>;
    /// hook 스크립트가 통보할 서버 주소(TS `getHookServerUrl`). 원격 경로를 찾지 못하면 오류다
    fn hook_server_url(&self, ssh_host: Option<&str>) -> Result<String, HookInstallError>;
    /// 지금 열려 있는 hook 서버 포트(TS `getHookServerPort`)
    fn hook_server_port(&self) -> u16;
    /// 원격 host daemon이 붙어 있으면 그 namespace. daemon을 쓰지 않는 호스트면 `Ok(None)`이고, 연결 오류는 legacy로 돌리지 않고 돌려준다
    fn remote_host_namespace(&self, ssh_host: &str) -> Result<Option<String>, HookInstallError>;
    /// 원격 hook이 데스크탑으로 돌아올 터널을 연다(TS `ensureHookTunnel`). 결과를 기다리지 않는다
    fn ensure_hook_tunnel(&self, ssh_host: &str);
    /// 로컬 hook 서버 health 주소에 GET을 보내 2xx로 답하는지(TS `fetch(url).ok`)
    fn is_local_hook_server_healthy(&self, health_check_url: &str) -> bool {
        probe_local_hook_server_health(health_check_url)
    }
    /// 원격 설치 스크립트 보관소. hook 서버의 `/api/install/hooks.sh` 경로가 같은 보관소를 읽는다
    fn install_scripts(&self) -> &HookInstallScripts;
}

/// TS `fetch`처럼 응답 상태가 2xx일 때만 성공으로 본다. 연결 실패와 4xx·5xx는 모두 실패다
pub fn probe_local_hook_server_health(health_check_url: &str) -> bool {
    ureq::get(health_check_url)
        .call()
        .is_ok_and(|response| response.status().is_success())
}

/// 설치기가 함께 쓰는 파일 접근, client runtime, hook 서버 상태 캐시
#[derive(Clone)]
pub struct HookInstallContext {
    pub files: Arc<HostFileAccess>,
    pub runtime: Arc<dyn HookClientRuntime>,
    pub server_status: Arc<super::server_status::HookServerStatusCache>,
}
