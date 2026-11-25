//! 설치기 테스트용 가짜 협력자. TS 테스트가 `vi.mock`으로 바꾸던 readTextFiles·writeHookProviderFiles·
//! registerKanvibeHookTarget·provider setup/getStatus·hookEndpoint·appSettingsService·execGit 자리다.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::HookProviderFile;
use super::client_runtime::{HookClientRuntime, HookInstallContext, HookInstallError};
use super::install_bundle::HookInstallScripts;
use super::installer::{
    HookInstallOperations, HookInstallerParts, HookInstallerTimings, KanvibeHooksInstaller,
};
use super::installer_verification::{HookInstallLog, HookLogLevel, HookVerificationRecord};
use super::open_code_tests::FakeOpenCodeRegistration;
use super::provider::ShellHookScriptsStatus;
use super::providers::{
    HookFileSnapshot, KanvibeHookProvider, KanvibeHookProviderModule, KanvibeHookStatus,
    kanvibe_hook_provider_modules,
};
use super::server_status::HookServerStatusCache;
use crate::exec::testing::ScriptedCommandRunner;
use crate::exec::{ExecGitError, ShellCommandRunner};
use crate::host_file_access::{HostFileAccess, TextFileReadResult};

/// 테스트가 열어 줄 때까지 기다리게 하는 문. 테스트가 멈추지 않도록 10초 뒤에는 저절로 열린다
pub(super) struct Gate {
    is_open: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            is_open: Mutex::new(false),
            opened: Condvar::new(),
        })
    }

    pub(super) fn open(&self) {
        *self.is_open.lock().unwrap() = true;
        self.opened.notify_all();
    }

    pub(super) fn wait(&self) {
        let is_open = self.is_open.lock().unwrap();
        let _ = self
            .opened
            .wait_timeout_while(is_open, Duration::from_secs(10), |is_open| !*is_open);
    }
}

/// 조건이 참이 될 때까지 기다린다
pub(super) fn wait_until(condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "조건을 기다리다 시간이 지났습니다"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn message_error(message: &str) -> HookInstallError {
    HookInstallError::Message(message.to_string())
}

pub(super) fn snapshot(entries: &[(&str, &str)]) -> HookFileSnapshot {
    entries
        .iter()
        .map(|(path, content)| {
            (
                path.to_string(),
                TextFileReadResult {
                    exists: true,
                    content: content.to_string(),
                },
            )
        })
        .collect()
}

type ReadStep = Box<dyn FnOnce(&[String]) -> Result<HookFileSnapshot, HookInstallError> + Send>;
type ReadBehavior =
    Arc<dyn Fn(&[String]) -> Result<HookFileSnapshot, HookInstallError> + Send + Sync>;
type WriteStep = Box<dyn FnOnce(&[HookProviderFile]) -> Result<(), HookInstallError> + Send>;
type WriteBehavior = Arc<dyn Fn(&[HookProviderFile]) -> Result<(), HookInstallError> + Send + Sync>;
/// 등록·setup 호출 인자(repo, taskId, hook 서버 주소, sshHost)
pub(super) type InstallCall = (String, String, String, Option<String>);
type RegisterBehavior = Arc<dyn Fn() -> Result<(), HookInstallError> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReadCall {
    pub paths: Vec<String>,
    pub ssh_host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WriteCall {
    pub files: Vec<HookProviderFile>,
    pub ssh_host: Option<String>,
}

/// 파일 읽기·쓰기·등록 가짜. 한 번만 쓰는 동작을 쌓아 두고, 없으면 기본 동작을 쓴다
pub(super) struct FakeOperations {
    pub read_calls: Mutex<Vec<ReadCall>>,
    read_steps: Mutex<VecDeque<ReadStep>>,
    read_default: Mutex<ReadBehavior>,
    pub write_calls: Mutex<Vec<WriteCall>>,
    write_steps: Mutex<VecDeque<WriteStep>>,
    write_default: Mutex<WriteBehavior>,
    pub register_calls: Mutex<Vec<InstallCall>>,
    register_default: Mutex<RegisterBehavior>,
    /// 등록과 쓰기가 일어난 순서
    pub events: Mutex<Vec<&'static str>>,
}

impl FakeOperations {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            read_calls: Mutex::new(Vec::new()),
            read_steps: Mutex::new(VecDeque::new()),
            read_default: Mutex::new(Arc::new(|_| Ok(HookFileSnapshot::new()))),
            write_calls: Mutex::new(Vec::new()),
            write_steps: Mutex::new(VecDeque::new()),
            write_default: Mutex::new(Arc::new(|_| Ok(()))),
            register_calls: Mutex::new(Vec::new()),
            register_default: Mutex::new(Arc::new(|| Ok(()))),
            events: Mutex::new(Vec::new()),
        })
    }

    pub(super) fn read_once(
        &self,
        step: impl FnOnce(&[String]) -> Result<HookFileSnapshot, HookInstallError> + Send + 'static,
    ) {
        self.read_steps.lock().unwrap().push_back(Box::new(step));
    }

    pub(super) fn read_always(
        &self,
        behavior: impl Fn(&[String]) -> Result<HookFileSnapshot, HookInstallError>
        + Send
        + Sync
        + 'static,
    ) {
        *self.read_default.lock().unwrap() = Arc::new(behavior);
    }

    pub(super) fn write_once(
        &self,
        step: impl FnOnce(&[HookProviderFile]) -> Result<(), HookInstallError> + Send + 'static,
    ) {
        self.write_steps.lock().unwrap().push_back(Box::new(step));
    }

    pub(super) fn write_always(
        &self,
        behavior: impl Fn(&[HookProviderFile]) -> Result<(), HookInstallError> + Send + Sync + 'static,
    ) {
        *self.write_default.lock().unwrap() = Arc::new(behavior);
    }

    pub(super) fn register_always(
        &self,
        behavior: impl Fn() -> Result<(), HookInstallError> + Send + Sync + 'static,
    ) {
        *self.register_default.lock().unwrap() = Arc::new(behavior);
    }

    pub(super) fn read_count(&self) -> usize {
        self.read_calls.lock().unwrap().len()
    }

    pub(super) fn write_count(&self) -> usize {
        self.write_calls.lock().unwrap().len()
    }

    pub(super) fn register_count(&self) -> usize {
        self.register_calls.lock().unwrap().len()
    }

    /// 마지막 설치가 기록한 파일 전체(provider 배치 4개와 task CLI 배치 1개)
    pub(super) fn last_install_written_files(&self) -> Vec<HookProviderFile> {
        let calls = self.write_calls.lock().unwrap();
        let start = calls.len().saturating_sub(5);
        calls[start..]
            .iter()
            .flat_map(|call| call.files.clone())
            .collect()
    }

    pub(super) fn written_content(&self, file_path: &str) -> String {
        self.last_install_written_files()
            .into_iter()
            .find(|file| file.file_path == file_path)
            .unwrap_or_else(|| panic!("write payload not found for {file_path}"))
            .content
    }

    pub(super) fn clear_calls(&self) {
        self.read_calls.lock().unwrap().clear();
        self.write_calls.lock().unwrap().clear();
        self.register_calls.lock().unwrap().clear();
        self.events.lock().unwrap().clear();
    }
}

impl HookInstallOperations for FakeOperations {
    fn read_text_files(
        &self,
        paths: &[String],
        ssh_host: Option<&str>,
    ) -> Result<HookFileSnapshot, HookInstallError> {
        self.read_calls.lock().unwrap().push(ReadCall {
            paths: paths.to_vec(),
            ssh_host: ssh_host.map(str::to_string),
        });
        let step = self.read_steps.lock().unwrap().pop_front();
        match step {
            Some(step) => step(paths),
            None => {
                let behavior = Arc::clone(&self.read_default.lock().unwrap());
                behavior(paths)
            }
        }
    }

    fn write_hook_provider_files(
        &self,
        files: &[HookProviderFile],
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        self.write_calls.lock().unwrap().push(WriteCall {
            files: files.to_vec(),
            ssh_host: ssh_host.map(str::to_string),
        });
        self.events.lock().unwrap().push("write");
        let step = self.write_steps.lock().unwrap().pop_front();
        match step {
            Some(step) => step(files),
            None => {
                let behavior = Arc::clone(&self.write_default.lock().unwrap());
                behavior(files)
            }
        }
    }

    fn register_hook_target(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        let call = (
            repo_path.to_string(),
            task_id.to_string(),
            hook_server_url.to_string(),
            ssh_host.map(str::to_string),
        );
        self.register_calls.lock().unwrap().push(call);
        self.events.lock().unwrap().push("register");
        let behavior = Arc::clone(&self.register_default.lock().unwrap());
        behavior()
    }

    fn assert_state_paths_are_not_symlinks(
        &self,
        _repo_path: &str,
        _file_names: &[&str],
        _ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        Ok(())
    }
}

/// appSettingsService·hookEndpoint 가짜. client ID와 포트는 테스트 도중 바꿀 수 있다
pub(super) struct TestRuntime {
    pub client_id: Mutex<String>,
    pub client_id_calls: AtomicUsize,
    pub hook_server_url: Mutex<String>,
    pub hook_server_url_calls: Mutex<Vec<Option<String>>>,
    pub hook_server_port: AtomicU16,
    install_scripts: HookInstallScripts,
}

impl TestRuntime {
    pub(super) fn new(hook_server_url: &str) -> Arc<Self> {
        Arc::new(Self {
            client_id: Mutex::new("own-client".into()),
            client_id_calls: AtomicUsize::new(0),
            hook_server_url: Mutex::new(hook_server_url.into()),
            hook_server_url_calls: Mutex::new(Vec::new()),
            hook_server_port: AtomicU16::new(9736),
            install_scripts: HookInstallScripts::default(),
        })
    }
}

impl HookClientRuntime for TestRuntime {
    fn hook_client_id(&self) -> Result<String, HookInstallError> {
        self.client_id_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.client_id.lock().unwrap().clone())
    }

    fn task_cli_token(&self, _task_id: &str) -> Result<String, HookInstallError> {
        Ok("token".into())
    }

    fn hook_server_url(&self, ssh_host: Option<&str>) -> Result<String, HookInstallError> {
        self.hook_server_url_calls
            .lock()
            .unwrap()
            .push(ssh_host.map(str::to_string));
        Ok(self.hook_server_url.lock().unwrap().clone())
    }

    fn hook_server_port(&self) -> u16 {
        self.hook_server_port.load(Ordering::SeqCst)
    }

    fn remote_host_namespace(&self, _ssh_host: &str) -> Option<String> {
        None
    }

    fn ensure_hook_tunnel(&self, _ssh_host: &str) {}

    fn is_local_hook_server_healthy(&self, _health_check_url: &str) -> bool {
        true
    }

    fn install_scripts(&self) -> &HookInstallScripts {
        &self.install_scripts
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StatusCall {
    pub repo_path: String,
    pub task_id: Option<String>,
    pub ssh_host: Option<String>,
    pub files: Option<HookFileSnapshot>,
}

type StatusStep = Box<dyn FnOnce() -> Result<KanvibeHookStatus, HookInstallError> + Send>;

/// provider 가짜. 산출물 경로와 내용은 진짜 provider로 만들고, setup·getStatus만 바꿔 끼운다
pub(super) struct FakeProvider {
    real: Arc<dyn KanvibeHookProviderModule>,
    pub status_calls: Mutex<Vec<StatusCall>>,
    status_steps: Mutex<VecDeque<StatusStep>>,
    /// `None`이면 진짜 provider로 상태를 판정한다
    status_default: Mutex<Option<Result<KanvibeHookStatus, String>>>,
    pub install_calls: Mutex<Vec<InstallCall>>,
    install_result: Mutex<Result<(), String>>,
}

impl FakeProvider {
    fn new(real: Arc<dyn KanvibeHookProviderModule>) -> Arc<Self> {
        let installed = installed_status(real.provider());
        Arc::new(Self {
            real,
            status_calls: Mutex::new(Vec::new()),
            status_steps: Mutex::new(VecDeque::new()),
            status_default: Mutex::new(Some(Ok(installed))),
            install_calls: Mutex::new(Vec::new()),
            install_result: Mutex::new(Ok(())),
        })
    }

    pub(super) fn status_always(&self, status: Result<KanvibeHookStatus, &str>) {
        *self.status_default.lock().unwrap() = Some(status.map_err(str::to_string));
    }

    pub(super) fn use_real_status(&self) {
        *self.status_default.lock().unwrap() = None;
    }

    pub(super) fn status_once(
        &self,
        step: impl FnOnce() -> Result<KanvibeHookStatus, HookInstallError> + Send + 'static,
    ) {
        self.status_steps.lock().unwrap().push_back(Box::new(step));
    }

    pub(super) fn install_fails_with(&self, message: &str) {
        *self.install_result.lock().unwrap() = Err(message.to_string());
    }

    pub(super) fn status_count(&self) -> usize {
        self.status_calls.lock().unwrap().len()
    }

    pub(super) fn install_count(&self) -> usize {
        self.install_calls.lock().unwrap().len()
    }

    pub(super) fn clear_calls(&self) {
        self.status_calls.lock().unwrap().clear();
        self.install_calls.lock().unwrap().clear();
    }
}

impl KanvibeHookProviderModule for FakeProvider {
    fn provider(&self) -> KanvibeHookProvider {
        self.real.provider()
    }

    fn install_input_paths(&self, repo_path: &str) -> Vec<String> {
        self.real.install_input_paths(repo_path)
    }

    fn build_files(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        existing_files: &HookFileSnapshot,
    ) -> Result<Vec<HookProviderFile>, HookInstallError> {
        self.real
            .build_files(repo_path, task_id, hook_server_url, existing_files)
    }

    fn install(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        let call = (
            repo_path.to_string(),
            task_id.to_string(),
            hook_server_url.to_string(),
            ssh_host.map(str::to_string),
        );
        self.install_calls.lock().unwrap().push(call);
        self.install_result
            .lock()
            .unwrap()
            .clone()
            .map_err(HookInstallError::Message)
    }

    fn status(
        &self,
        repo_path: &str,
        task_id: Option<&str>,
        ssh_host: Option<&str>,
        existing_files: Option<HookFileSnapshot>,
    ) -> Result<KanvibeHookStatus, HookInstallError> {
        self.status_calls.lock().unwrap().push(StatusCall {
            repo_path: repo_path.to_string(),
            task_id: task_id.map(str::to_string),
            ssh_host: ssh_host.map(str::to_string),
            files: existing_files.clone(),
        });
        let step = self.status_steps.lock().unwrap().pop_front();
        if let Some(step) = step {
            return step();
        }
        let status_default = self.status_default.lock().unwrap().clone();
        match status_default {
            Some(status) => status.map_err(HookInstallError::Message),
            None => self
                .real
                .status(repo_path, task_id, ssh_host, existing_files),
        }
    }
}

/// 검증 로그와 그 밖의 경고를 모두 기록한다
#[derive(Default)]
pub(super) struct RecordingLog {
    pub verifications: Mutex<Vec<(HookLogLevel, HookVerificationRecord)>>,
    pub events: Mutex<Vec<(HookLogLevel, String, String)>>,
}

impl HookInstallLog for RecordingLog {
    fn verification(&self, level: HookLogLevel, record: &HookVerificationRecord) {
        self.verifications
            .lock()
            .unwrap()
            .push((level, record.clone()));
    }

    fn event(&self, level: HookLogLevel, message: &str, detail: &str) {
        self.events
            .lock()
            .unwrap()
            .push((level, message.to_string(), detail.to_string()));
    }
}

impl RecordingLog {
    pub(super) fn verification_for(
        &self,
        level: HookLogLevel,
        provider: &str,
    ) -> Option<HookVerificationRecord> {
        self.verifications
            .lock()
            .unwrap()
            .iter()
            .find(|(record_level, record)| *record_level == level && record.provider == provider)
            .map(|(_, record)| record.clone())
    }
}

/// 테스트가 직접 앞으로 돌리는 시계
#[derive(Clone)]
pub(super) struct ManualClock {
    now: Arc<Mutex<Instant>>,
}

impl ManualClock {
    fn new() -> Self {
        Self {
            now: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub(super) fn advance(&self, duration: Duration) {
        *self.now.lock().unwrap() += duration;
    }

    fn as_clock(&self) -> Arc<dyn Fn() -> Instant + Send + Sync> {
        let now = Arc::clone(&self.now);
        Arc::new(move || *now.lock().unwrap())
    }
}

type CommandResult = Result<String, ExecGitError>;

/// 설치기와 가짜 협력자 묶음
pub(super) struct InstallerFixture {
    pub installer: Arc<KanvibeHooksInstaller>,
    pub operations: Arc<FakeOperations>,
    pub claude: Arc<FakeProvider>,
    pub gemini: Arc<FakeProvider>,
    pub codex: Arc<FakeProvider>,
    pub open_code: Arc<FakeProvider>,
    pub runtime: Arc<TestRuntime>,
    pub runner: Arc<ScriptedCommandRunner>,
    pub command_result: Arc<Mutex<CommandResult>>,
    pub registration: Arc<FakeOpenCodeRegistration>,
    pub log: Arc<RecordingLog>,
    pub clock: ManualClock,
}

impl InstallerFixture {
    /// TS `beforeEach`의 기본값: hook 서버 주소, 빈 읽기, 성공하는 쓰기, 설치된 provider 상태
    pub(super) fn new(hook_server_url: &str) -> Self {
        let command_result: Arc<Mutex<CommandResult>> = Arc::new(Mutex::new(Ok(String::new())));
        let handler_result = Arc::clone(&command_result);
        let runner = Arc::new(ScriptedCommandRunner::new(move |_| {
            handler_result.lock().unwrap().clone()
        }));
        let runtime = TestRuntime::new(hook_server_url);
        let shared_runner: Arc<dyn ShellCommandRunner> = runner.clone();
        let context = HookInstallContext {
            files: Arc::new(HostFileAccess::new(shared_runner, "/home/test-user")),
            runtime: runtime.clone(),
            server_status: Arc::new(HookServerStatusCache::new(Duration::ZERO)),
        };
        let registration = Arc::new(FakeOpenCodeRegistration::new());
        let real_modules = kanvibe_hook_provider_modules(&context, registration.clone());
        let providers: Vec<Arc<FakeProvider>> =
            real_modules.into_iter().map(FakeProvider::new).collect();
        let operations = FakeOperations::new();
        let log = Arc::new(RecordingLog::default());
        let clock = ManualClock::new();
        let installer = Arc::new(KanvibeHooksInstaller::new(HookInstallerParts {
            runtime: runtime.clone(),
            operations: operations.clone(),
            providers: providers
                .iter()
                .map(|provider| Arc::clone(provider) as Arc<dyn KanvibeHookProviderModule>)
                .collect(),
            open_code_registration: registration.clone(),
            log: log.clone(),
            timings: HookInstallerTimings {
                retry_delay_step: Duration::ZERO,
                remote_status_reuse: Duration::from_secs(30),
            },
            clock: clock.as_clock(),
        }));
        Self {
            installer,
            operations,
            claude: Arc::clone(&providers[0]),
            gemini: Arc::clone(&providers[1]),
            codex: Arc::clone(&providers[2]),
            open_code: Arc::clone(&providers[3]),
            runtime,
            runner,
            command_result,
            registration,
            log,
            clock,
        }
    }

    pub(super) fn providers(&self) -> [&Arc<FakeProvider>; 4] {
        [&self.claude, &self.gemini, &self.codex, &self.open_code]
    }
}

fn passing_scripts() -> ShellHookScriptsStatus {
    ShellHookScriptsStatus {
        has_task_id_binding: true,
        has_status_mappings: true,
        has_status_json_persistence: true,
        has_target_fanout: true,
        has_bounded_notify_timeout: true,
        has_parallel_target_fanout: true,
        has_expected_hook_server_url: true,
        has_reachable_hook_server: true,
        bound_task_id: None,
        configured_hook_server_url: None,
        expected_hook_server_url: None,
        has_registered_hook_target: true,
        registered_hook_target_url: None,
    }
}

/// 모든 확인 항목이 참인 설치 상태(TS mock `{ installed: true }`)
pub(super) fn installed_status(provider: KanvibeHookProvider) -> KanvibeHookStatus {
    use super::claude::ClaudeHooksStatus;
    use super::codex::CodexHooksStatus;
    use super::gemini::GeminiHooksStatus;
    match provider {
        KanvibeHookProvider::Claude => KanvibeHookStatus::Claude(ClaudeHooksStatus {
            is_installed: true,
            has_prompt_hook: true,
            has_stop_hook: true,
            has_question_hook: true,
            has_settings_entry: true,
            scripts: passing_scripts(),
        }),
        KanvibeHookProvider::Gemini => KanvibeHookStatus::Gemini(GeminiHooksStatus {
            is_installed: true,
            has_prompt_hook: true,
            has_stop_hook: true,
            has_settings_entry: true,
            scripts: passing_scripts(),
        }),
        KanvibeHookProvider::Codex => KanvibeHookStatus::Codex(CodexHooksStatus {
            is_installed: true,
            has_prompt_hook: true,
            has_permission_hook: true,
            has_pre_tool_hook: true,
            has_stop_hook: true,
            has_hooks_file: true,
            has_hook_entries: true,
            has_config_entry: true,
            scripts: passing_scripts(),
        }),
        KanvibeHookProvider::OpenCode => KanvibeHookStatus::OpenCode(installed_open_code_status()),
    }
}

fn installed_open_code_status() -> super::open_code::OpenCodeHooksStatus {
    use super::open_code::{OpenCodeHooksStatus, OpenCodePluginRegistrationStatus};
    use super::target_registration::HookTargetRegistrationStatus;
    OpenCodeHooksStatus {
        is_installed: true,
        has_plugin: true,
        has_task_id_binding: true,
        has_status_endpoint: true,
        has_event_mappings: true,
        has_status_json_persistence: true,
        has_bounded_notify_timeout: true,
        has_target_fanout: true,
        has_parallel_target_fanout: true,
        has_main_session_guard: true,
        has_duplicate_progress_guard: true,
        has_pane_chat_binding: true,
        has_expected_hook_server_url: true,
        has_reachable_hook_server: true,
        bound_task_id: None,
        target_path: String::new(),
        plugin_path: String::new(),
        configured_hook_server_url: None,
        expected_hook_server_url: None,
        registration: OpenCodePluginRegistrationStatus {
            has_registered_plugin: Some(true),
            ..Default::default()
        },
        target_registration: HookTargetRegistrationStatus {
            has_registered_hook_target: true,
            registered_hook_target_url: None,
        },
    }
}

/// TS mock의 부분 상태. `failing`에 적은 항목만 거짓이고 나머지 항목은 참이다
pub(super) fn status_with(
    provider: KanvibeHookProvider,
    is_installed: bool,
    failing: &[&str],
) -> KanvibeHookStatus {
    let mut status = installed_status(provider);
    match &mut status {
        KanvibeHookStatus::Claude(status) => status.is_installed = is_installed,
        KanvibeHookStatus::Gemini(status) => status.is_installed = is_installed,
        KanvibeHookStatus::Codex(status) => {
            status.is_installed = is_installed;
            status.has_config_entry = !failing.contains(&"hasConfigEntry");
        }
        KanvibeHookStatus::OpenCode(status) => {
            status.is_installed = is_installed;
            status.has_plugin = !failing.contains(&"hasPlugin");
            status.has_task_id_binding = !failing.contains(&"hasTaskIdBinding");
            status.registration.has_registered_plugin =
                Some(!failing.contains(&"hasRegisteredPlugin"));
        }
    }
    status
}

/// OpenCode 등록 조회가 실패해 등록 여부(hasRegisteredPlugin)가 비어 있는 상태
pub(super) fn open_code_registration_failure(error: &str, failing: &[&str]) -> KanvibeHookStatus {
    let mut status = status_with(KanvibeHookProvider::OpenCode, false, failing);
    if let KanvibeHookStatus::OpenCode(status) = &mut status {
        status.registration.has_registered_plugin = None;
        status.registration.plugin_registration_error = Some(error.to_string());
    }
    status
}
