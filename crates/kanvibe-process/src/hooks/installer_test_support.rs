//! hook 설치기 테스트용 가짜 client runtime. TS 테스트가 appSettingsService·hookEndpoint·remoteHostConnection을
//! `vi.mock`으로 바꾸던 자리다.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::client_runtime::{HookClientRuntime, HookInstallContext, HookInstallError};
use super::install_bundle::HookInstallScripts;
use super::server_status::HookServerStatusCache;
use crate::exec::ShellCommandRunner;
use crate::host_file_access::HostFileAccess;

/// 실제 데스크탑 설치마다 다른 client ID를 흉내 내도록 부를 때마다 다른 ID를 줄 수 있다
pub(crate) struct FakeHookClientRuntime {
    pub client_id_prefix: Option<String>,
    pub fixed_client_id: String,
    client_id_calls: AtomicUsize,
    pub hook_server_url: Result<String, String>,
    pub hook_server_port: u16,
    pub remote_host_namespace: Result<Option<String>, String>,
    pub is_local_server_healthy: bool,
    pub tunnel_hosts: Mutex<Vec<String>>,
    pub token_task_ids: Mutex<Vec<String>>,
    pub health_check_urls: Mutex<Vec<String>>,
    pub install_scripts: HookInstallScripts,
}

impl FakeHookClientRuntime {
    pub fn new() -> Self {
        Self {
            client_id_prefix: None,
            fixed_client_id: "client-a".into(),
            client_id_calls: AtomicUsize::new(0),
            hook_server_url: Ok("http://localhost:9736".into()),
            hook_server_port: 9736,
            remote_host_namespace: Ok(None),
            is_local_server_healthy: true,
            tunnel_hosts: Mutex::new(Vec::new()),
            token_task_ids: Mutex::new(Vec::new()),
            health_check_urls: Mutex::new(Vec::new()),
            install_scripts: HookInstallScripts::default(),
        }
    }
}

impl HookClientRuntime for FakeHookClientRuntime {
    fn hook_client_id(&self) -> Result<String, HookInstallError> {
        let call_number = self.client_id_calls.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(match &self.client_id_prefix {
            Some(prefix) => format!("{prefix}-{call_number}"),
            None => self.fixed_client_id.clone(),
        })
    }

    fn task_cli_token(&self, task_id: &str) -> Result<String, HookInstallError> {
        self.token_task_ids
            .lock()
            .unwrap()
            .push(task_id.to_string());
        Ok("signed-token".into())
    }

    fn hook_server_url(&self, _ssh_host: Option<&str>) -> Result<String, HookInstallError> {
        self.hook_server_url
            .clone()
            .map_err(HookInstallError::Message)
    }

    fn hook_server_port(&self) -> u16 {
        self.hook_server_port
    }

    fn remote_host_namespace(&self, _ssh_host: &str) -> Result<Option<String>, HookInstallError> {
        self.remote_host_namespace
            .clone()
            .map_err(HookInstallError::Message)
    }

    fn ensure_hook_tunnel(&self, ssh_host: &str) {
        self.tunnel_hosts.lock().unwrap().push(ssh_host.to_string());
    }

    fn is_local_hook_server_healthy(&self, health_check_url: &str) -> bool {
        self.health_check_urls
            .lock()
            .unwrap()
            .push(health_check_url.to_string());
        self.is_local_server_healthy
    }

    fn install_scripts(&self) -> &HookInstallScripts {
        &self.install_scripts
    }
}

/// TS 테스트처럼 캐시를 쓰지 않는(유지 시간 0) 설치 문맥
pub(crate) fn install_context(
    runner: Arc<dyn ShellCommandRunner>,
    runtime: Arc<FakeHookClientRuntime>,
    home_directory: &str,
) -> HookInstallContext {
    HookInstallContext {
        files: Arc::new(HostFileAccess::new(runner, home_directory)),
        runtime,
        server_status: Arc::new(HookServerStatusCache::new(Duration::ZERO)),
    }
}

/// 로컬 repo 설치 테스트용 문맥. 명령은 실제 로컬 셸로 실행하고, client ID는 설치마다 달라진다
pub(crate) fn local_install_context(
    repo_path: &str,
    configure: impl FnOnce(&mut FakeHookClientRuntime),
) -> (HookInstallContext, Arc<FakeHookClientRuntime>) {
    let mut runtime = FakeHookClientRuntime::new();
    runtime.client_id_prefix = Some("client".into());
    configure(&mut runtime);
    let runtime = Arc::new(runtime);
    let runner = Arc::new(crate::exec::testing::LoopbackShellRunner::new(repo_path));
    (install_context(runner, runtime.clone(), repo_path), runtime)
}

/// 파일 내용의 문자열 하나를 모두 바꿔 쓴다(TS 테스트의 legacy 파일 흉내)
pub(crate) fn replace_in_file(path: &std::path::Path, from: &str, to: &str) {
    let content = std::fs::read_to_string(path).unwrap();
    std::fs::write(path, content.replace(from, to)).unwrap();
}
