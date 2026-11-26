//! hook 설치기와 그 설치기가 데스크톱에서 받아야 하는 값(TS `kanvibeHooksInstaller`가 직접 부르던 appSettings,
//! taskCliTokenService, hookEndpoint, remoteHostConnection, hookTunnelService).

use std::sync::{Arc, Mutex};

use kanvibe_db::Database;
use kanvibe_db::app_settings_service::AppSettingsService;
use kanvibe_domain::platform::ShortcutPlatform;
use kanvibe_process::hooks::client_runtime::{
    HookClientRuntime, HookInstallContext, HookInstallError,
};
use kanvibe_process::hooks::endpoint::{
    HookEndpointError, HookServerEndpoint, SystemHookNetworkRoutes,
};
use kanvibe_process::hooks::install_bundle::HookInstallScripts;
use kanvibe_process::hooks::installer::KanvibeHooksInstaller;
use kanvibe_process::hooks::open_code_registry::{
    OpenCodePluginRegistration, OpenCodePluginRegistry, SystemOpenCodeDebugConfigCommand,
};
use kanvibe_process::hooks::server_status::{HOOK_SERVER_CACHE_TTL, HookServerStatusCache};
use kanvibe_process::ssh_config::SshConfigReader;
use kanvibe_process::task_cli::token::{TaskCliTokenError, TaskCliTokenService};

use crate::config::{RemoteHookRouting, RuntimeConfig};
use crate::foundation::Foundation;

/// `HMAC(task_cli_secret, taskId)` task 토큰(TS `taskCliTokenService`). DB를 빌리는 kanvibe-process 서비스를
/// 호출마다 만들되, 비밀값을 처음 만드는 순간이 겹쳐 서로 다른 비밀값이 저장되지 않도록 발급을 한 줄로 세운다
pub struct TaskCliTokens {
    database: Arc<Database>,
    issuing_turn: Mutex<()>,
}

impl TaskCliTokens {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            database,
            issuing_turn: Mutex::new(()),
        }
    }

    pub fn build_task_cli_token(&self, task_id: &str) -> Result<String, TaskCliTokenError> {
        let _turn = self
            .issuing_turn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        TaskCliTokenService::new(&self.database).build_task_cli_token(task_id)
    }

    pub fn is_valid_task_cli_token(
        &self,
        task_id: &str,
        submitted_token: &str,
    ) -> Result<bool, TaskCliTokenError> {
        let _turn = self
            .issuing_turn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        TaskCliTokenService::new(&self.database).is_valid_task_cli_token(task_id, submitted_token)
    }
}

/// hook 스크립트가 통보할 이 데스크톱의 hook 서버 주소(TS `getHookServerUrl`, `getHookServerPort`)
pub struct HookServerAddress {
    endpoint: HookServerEndpoint,
    network: SystemHookNetworkRoutes,
}

impl HookServerAddress {
    pub fn new(port: u16, home_directory: &str) -> Self {
        Self {
            endpoint: HookServerEndpoint::new(port),
            network: SystemHookNetworkRoutes::new(SshConfigReader::new(home_directory)),
        }
    }

    pub fn port(&self) -> u16 {
        self.endpoint.port()
    }

    /// 로컬이면 `http://localhost:<port>`, 원격이면 그 호스트에서 닿는 로컬 IPv4 주소다
    pub fn url(&self, ssh_host: Option<&str>) -> Result<String, HookEndpointError> {
        self.endpoint.url(ssh_host, &self.network)
    }
}

/// 설치기가 쓰는 데스크톱 쪽 값. client ID와 토큰은 DB에서, 주소는 설정한 포트에서, 원격 경로는 주입한 seam에서 얻는다
pub struct DesktopHookClientRuntime {
    database: Arc<Database>,
    tokens: Arc<TaskCliTokens>,
    address: Arc<HookServerAddress>,
    routing: Arc<dyn RemoteHookRouting>,
    install_scripts: Arc<HookInstallScripts>,
}

fn install_error(error: impl ToString) -> HookInstallError {
    HookInstallError::Message(error.to_string())
}

impl HookClientRuntime for DesktopHookClientRuntime {
    fn hook_client_id(&self) -> Result<String, HookInstallError> {
        AppSettingsService::new(&self.database, ShortcutPlatform::current())
            .get_or_create_hook_client_id()
            .map_err(install_error)
    }

    fn task_cli_token(&self, task_id: &str) -> Result<String, HookInstallError> {
        self.tokens
            .build_task_cli_token(task_id)
            .map_err(install_error)
    }

    fn hook_server_url(&self, ssh_host: Option<&str>) -> Result<String, HookInstallError> {
        self.address.url(ssh_host).map_err(install_error)
    }

    fn hook_server_port(&self) -> u16 {
        self.address.port()
    }

    fn remote_host_namespace(&self, ssh_host: &str) -> Result<Option<String>, HookInstallError> {
        self.routing
            .remote_host_namespace(ssh_host)
            .map_err(install_error)
    }

    fn ensure_hook_tunnel(&self, ssh_host: &str) {
        self.routing.ensure_hook_tunnel(ssh_host);
    }

    fn install_scripts(&self) -> &HookInstallScripts {
        &self.install_scripts
    }
}

pub(crate) struct HookParts {
    pub tokens: Arc<TaskCliTokens>,
    pub address: Arc<HookServerAddress>,
    pub install_scripts: Arc<HookInstallScripts>,
    pub context: HookInstallContext,
    pub open_code_registration: Arc<dyn OpenCodePluginRegistration>,
    pub installer: Arc<KanvibeHooksInstaller>,
}

pub(crate) fn build_hooks(foundation: &Foundation, config: &RuntimeConfig) -> HookParts {
    let tokens = Arc::new(TaskCliTokens::new(Arc::clone(&foundation.database)));
    let address = Arc::new(HookServerAddress::new(
        config.hook_server_port,
        &config.home_directory,
    ));
    let install_scripts = Arc::new(HookInstallScripts::default());
    let runtime = Arc::new(DesktopHookClientRuntime {
        database: Arc::clone(&foundation.database),
        tokens: Arc::clone(&tokens),
        address: Arc::clone(&address),
        routing: Arc::clone(&config.remote_hook_routing),
        install_scripts: Arc::clone(&install_scripts),
    });
    let context = HookInstallContext {
        files: Arc::clone(&foundation.files),
        runtime,
        server_status: Arc::new(HookServerStatusCache::new(HOOK_SERVER_CACHE_TTL)),
    };
    let open_code_registration: Arc<dyn OpenCodePluginRegistration> =
        Arc::new(OpenCodePluginRegistry::new(Arc::new(
            SystemOpenCodeDebugConfigCommand::new(foundation.environment.shell_environment.clone()),
        )));
    let installer = Arc::new(KanvibeHooksInstaller::for_context(
        context.clone(),
        Arc::clone(&open_code_registration),
    ));
    HookParts {
        tokens,
        address,
        install_scripts,
        context,
        open_code_registration,
        installer,
    }
}
