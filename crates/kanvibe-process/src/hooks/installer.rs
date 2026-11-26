//! 네 provider hook과 task CLI의 설치·검증·재설치 예약(TS `kanvibeHooksInstaller.ts`).
//! 같은 대상의 설치·검사·상태 조회는 진행 중인 작업을 함께 쓰고, 원격 상태는 30초 동안 다시 쓴다.
//! TS 모듈 전역 Map이던 작업·캐시 상태는 모두 `KanvibeHooksInstaller` 하나가 갖는다.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::HookProviderFile;
use super::client_runtime::{HookClientRuntime, HookInstallContext, HookInstallError};
use super::file_writer::write_hook_provider_files;
use super::installer_jobs::{SharedJobs, StatusReads};
use super::installer_verification::{
    HookInstallLog, HookLogLevel, HookVerificationFailure, StderrHookInstallLog,
    build_verification_record, fatal_hook_verification_failed_checks,
    format_hook_verification_failures, hook_verification_failed_checks,
    should_fail_hook_verification,
};
use super::open_code_registry::OpenCodePluginRegistration;
use super::providers::{
    HookFileSnapshot, KanvibeHookProvider, KanvibeHookProviderModule, KanvibeHookStatus,
    KanvibeHookStatuses, kanvibe_hook_provider_modules,
};
use super::socket_path::HOOK_TRANSPORT_VERSION_MARKER;
use super::target_registration::register_kanvibe_hook_target;
use crate::exec::{ExecGitOptions, run_with_background_ssh_commands};
use crate::kanvibe_project_state::{
    HookTargetTransport, assert_kanvibe_state_paths_are_not_symlinks, get_kanvibe_targets_path,
    parse_kanvibe_targets,
};
use crate::task_cli::script::{TASK_CLI_FILE_NAME, build_task_cli_file};

const HOOK_INSTALL_MAX_ATTEMPTS: u32 = 3;

thread_local! {
    /// 예약 설치처럼 기다리는 화면이 없는 작업. provider별 병렬 스레드에도 원격 명령 백그라운드 우선순위를 물려준다
    static IS_BACKGROUND_INSTALL: Cell<bool> = const { Cell::new(false) };
}

fn run_as_background<T>(operation: impl FnOnce() -> T) -> T {
    let previous = IS_BACKGROUND_INSTALL.with(|flag| flag.replace(true));
    let result = run_with_background_ssh_commands(operation);
    IS_BACKGROUND_INSTALL.with(|flag| flag.set(previous));
    result
}

/// 항목마다 스레드를 띄워 함께 실행하고 입력 순서대로 결과를 모은다(TS `Promise.all`·`allSettled`)
fn run_in_parallel<T: Sync, R: Send>(items: &[T], run: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let is_background = IS_BACKGROUND_INSTALL.with(Cell::get);
    let run = &run;
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .iter()
            .map(|item| {
                scope.spawn(move || match is_background {
                    true => run_as_background(|| run(item)),
                    false => run(item),
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("hook 설치 스레드가 멈췄다"))
            .collect()
    })
}

/// 설치기가 쓰는 파일 읽기·쓰기·등록. 앱은 `ContextHookInstallOperations`를 쓴다
pub trait HookInstallOperations: Send + Sync {
    /// 호스트 키를 검증하는 연결로 파일 묶음을 한 번에 읽는다
    fn read_text_files(
        &self,
        paths: &[String],
        ssh_host: Option<&str>,
    ) -> Result<HookFileSnapshot, HookInstallError>;
    fn write_hook_provider_files(
        &self,
        files: &[HookProviderFile],
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError>;
    fn register_hook_target(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError>;
    fn assert_state_paths_are_not_symlinks(
        &self,
        repo_path: &str,
        file_names: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError>;
}

pub struct ContextHookInstallOperations {
    pub context: HookInstallContext,
}

impl HookInstallOperations for ContextHookInstallOperations {
    fn read_text_files(
        &self,
        paths: &[String],
        ssh_host: Option<&str>,
    ) -> Result<HookFileSnapshot, HookInstallError> {
        let options = ExecGitOptions {
            should_check_host_key_strictly: true,
            ..ExecGitOptions::default()
        };
        Ok(self
            .context
            .files
            .read_text_files(paths, ssh_host, &options)?)
    }

    fn write_hook_provider_files(
        &self,
        files: &[HookProviderFile],
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        write_hook_provider_files(&self.context, files, ssh_host)
    }

    fn register_hook_target(
        &self,
        repo_path: &str,
        task_id: &str,
        hook_server_url: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        register_kanvibe_hook_target(&self.context, repo_path, task_id, hook_server_url, ssh_host)
    }

    fn assert_state_paths_are_not_symlinks(
        &self,
        repo_path: &str,
        file_names: &[&str],
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        Ok(assert_kanvibe_state_paths_are_not_symlinks(
            &self.context.files,
            repo_path,
            file_names,
            ssh_host,
        )?)
    }
}

/// 재시도 간격의 단위(시도 n번째 실패 뒤 n배)와 원격 상태 재사용 시간
#[derive(Debug, Clone, Copy)]
pub struct HookInstallerTimings {
    pub retry_delay_step: Duration,
    pub remote_status_reuse: Duration,
}

impl Default for HookInstallerTimings {
    fn default() -> Self {
        Self {
            retry_delay_step: Duration::from_millis(500),
            remote_status_reuse: Duration::from_secs(30),
        }
    }
}

/// 설치기를 만드는 재료
pub struct HookInstallerParts {
    pub runtime: Arc<dyn HookClientRuntime>,
    pub operations: Arc<dyn HookInstallOperations>,
    pub providers: Vec<Arc<dyn KanvibeHookProviderModule>>,
    pub open_code_registration: Arc<dyn OpenCodePluginRegistration>,
    pub log: Arc<dyn HookInstallLog>,
    pub timings: HookInstallerTimings,
    pub clock: Arc<dyn Fn() -> Instant + Send + Sync>,
}

/// 예약 설치·검증이 실패했을 때 부르는 callback
pub type HookInstallFailureCallback = Box<dyn FnOnce(&HookInstallError) + Send>;

/// 예약 설치·검증이 끝난 뒤 부르는 callback
#[derive(Default)]
pub struct HookInstallScheduleOptions {
    pub delay: Duration,
    pub on_success: Option<Box<dyn FnOnce() + Send>>,
    pub on_failure: Option<HookInstallFailureCallback>,
}

struct HookInstallCallbacks {
    on_success: Option<Box<dyn FnOnce() + Send>>,
    on_failure: Option<HookInstallFailureCallback>,
}

type ScheduledCallbacks = Arc<Mutex<Vec<HookInstallCallbacks>>>;

/// 설치 대상 하나(worktree 경로, task, 원격 호스트)
#[derive(Debug, Clone, Copy)]
struct HookTarget<'a> {
    path: &'a str,
    task_id: &'a str,
    ssh_host: Option<&'a str>,
}

impl HookTarget<'_> {
    /// TS `buildHookInstallKey`: `[작업, sshHost ?? "", 경로, taskId].join("\0")`
    fn key(&self, job: &str) -> String {
        [
            job,
            self.ssh_host.unwrap_or_default(),
            self.path,
            self.task_id,
        ]
        .join("\0")
    }
}

/// 검사 결과. 최신이 아니면 실패 항목을 담는다
struct HooksStateInspection {
    is_current: bool,
    failures: Vec<String>,
}

pub struct KanvibeHooksInstaller {
    runtime: Arc<dyn HookClientRuntime>,
    operations: Arc<dyn HookInstallOperations>,
    providers: Vec<Arc<dyn KanvibeHookProviderModule>>,
    open_code_registration: Arc<dyn OpenCodePluginRegistration>,
    log: Arc<dyn HookInstallLog>,
    timings: HookInstallerTimings,
    ensure_jobs: SharedJobs<bool>,
    install_jobs: SharedJobs<()>,
    file_install_jobs: SharedJobs<()>,
    status_reads: StatusReads<KanvibeHookStatuses>,
    verified_remote_hook_files: Mutex<HashMap<String, String>>,
    scheduled_installs: Mutex<HashMap<String, ScheduledCallbacks>>,
}

impl KanvibeHooksInstaller {
    pub fn new(parts: HookInstallerParts) -> Self {
        Self {
            status_reads: StatusReads::new(parts.timings.remote_status_reuse, parts.clock),
            runtime: parts.runtime,
            operations: parts.operations,
            providers: parts.providers,
            open_code_registration: parts.open_code_registration,
            log: parts.log,
            timings: parts.timings,
            ensure_jobs: SharedJobs::new(),
            install_jobs: SharedJobs::new(),
            file_install_jobs: SharedJobs::new(),
            verified_remote_hook_files: Mutex::new(HashMap::new()),
            scheduled_installs: Mutex::new(HashMap::new()),
        }
    }

    /// 앱이 쓰는 설치기. 실제 파일 접근과 네 provider, 표준 오류 로그를 쓴다
    pub fn for_context(
        context: HookInstallContext,
        open_code_registration: Arc<dyn OpenCodePluginRegistration>,
    ) -> Self {
        Self::new(HookInstallerParts {
            runtime: Arc::clone(&context.runtime),
            providers: kanvibe_hook_provider_modules(&context, Arc::clone(&open_code_registration)),
            operations: Arc::new(ContextHookInstallOperations { context }),
            open_code_registration,
            log: Arc::new(StderrHookInstallLog),
            timings: HookInstallerTimings::default(),
            clock: Arc::new(Instant::now),
        })
    }

    fn provider_module(
        &self,
        provider: KanvibeHookProvider,
    ) -> Result<&Arc<dyn KanvibeHookProviderModule>, HookInstallError> {
        self.providers
            .iter()
            .find(|module| module.provider() == provider)
            .ok_or_else(|| {
                HookInstallError::Message(format!("{} hook provider가 없습니다", provider.label()))
            })
    }
}

fn normalize_ssh_host(ssh_host: Option<&str>) -> Option<&str> {
    ssh_host.filter(|host| !host.is_empty())
}

fn deduplicate_paths(paths: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

/// 검증한 파일 묶음의 지문. 같은 client·포트·파일이면 같다
fn hook_files_fingerprint(
    files: &HookFileSnapshot,
    client_id: &str,
    hook_server_port: u16,
) -> String {
    let entries: Vec<(&String, bool, &String)> = files
        .iter()
        .map(|(path, file)| (path, file.exists, &file.content))
        .collect();
    let serialized =
        serde_json::to_string(&(client_id, hook_server_port, entries)).expect("직렬화할 수 있다");
    hex::encode(Sha256::digest(serialized.as_bytes()))
}

impl KanvibeHooksInstaller {
    /// 현재 설치를 확인하고 구형 파일이나 현재 client 등록을 복구했으면 `true`. 검사 실패는 다음에 재시도하도록 오류로 돌려준다
    pub fn ensure_hooks_current(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, HookInstallError> {
        let target = HookTarget {
            path: target_path,
            task_id,
            ssh_host: normalize_ssh_host(ssh_host),
        };
        self.ensure_jobs.run(&target.key("all"), || {
            if self.inspect_hooks_state(target)?.is_current {
                return Ok(false);
            }
            self.install_hooks(target.path, target.task_id, target.ssh_host)?;
            let verification = self.inspect_hooks_state(target)?;
            if !verification.is_current {
                let failures = verification.failures.join(", ");
                return Err(HookInstallError::Message(format!(
                    "hooks 검증 실패: {failures}"
                )));
            }
            Ok(true)
        })
    }

    /// 모든 provider의 hook 상태를 파일 읽기 한 번으로 조회한다. 겹친 조회는 진행 중인 것을 함께 쓴다
    pub fn hooks_statuses(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<KanvibeHookStatuses, HookInstallError> {
        let target = HookTarget {
            path: target_path,
            task_id,
            ssh_host: normalize_ssh_host(ssh_host),
        };
        self.status_reads
            .read(&target.key("status"), target.ssh_host.is_some(), || {
                self.read_hooks_statuses(target)
            })
    }

    /// 설치가 끝난 뒤의 조회가 설치 전에 읽었거나 읽기 시작한 상태를 받지 않게 한다
    fn forget_status_reads(&self, target: HookTarget<'_>) {
        self.status_reads.forget(&target.key("status"));
    }

    pub fn install_hooks(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        let target = HookTarget {
            path: target_path,
            task_id,
            ssh_host: normalize_ssh_host(ssh_host),
        };
        let result = self.install_jobs.run(&target.key("all"), || {
            self.run_with_retry("install", target, None, || {
                self.install_hook_files_once(target)?;
                self.verify_installation(target)
            })
        });
        self.forget_status_reads(target);
        result
    }

    /// provider 검증을 기다리지 않고 hook 파일과 task CLI만 기록한다
    pub fn install_hook_files(
        &self,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        let target = HookTarget {
            path: target_path,
            task_id,
            ssh_host: normalize_ssh_host(ssh_host),
        };
        let result = self.file_install_jobs.run(&target.key("all-files"), || {
            self.run_with_retry("file install", target, None, || {
                self.install_hook_files_once(target)
            })
        });
        self.forget_status_reads(target);
        result
    }

    /// 선택한 provider 하나만 설치하고 검증한다
    pub fn install_hook_provider(
        &self,
        target_path: &str,
        task_id: &str,
        provider: KanvibeHookProvider,
        ssh_host: Option<&str>,
    ) -> Result<(), HookInstallError> {
        let target = HookTarget {
            path: target_path,
            task_id,
            ssh_host: normalize_ssh_host(ssh_host),
        };
        let result = self.install_jobs.run(&target.key(provider.as_str()), || {
            let module = self.provider_module(provider)?;
            self.run_with_retry("provider install", target, Some(provider), || {
                self.install_hook_provider_once(module.as_ref(), target)
            })
        });
        self.forget_status_reads(target);
        result
    }
}

impl KanvibeHooksInstaller {
    /// 같은 대상의 예약이 남아 있으면 callback만 더하고 `None`을 돌려준다. 새 예약은 설치 스레드를 돌려준다
    pub fn schedule_hooks_install(
        self: &Arc<Self>,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        options: HookInstallScheduleOptions,
    ) -> Option<JoinHandle<()>> {
        let target = HookTarget {
            path: target_path,
            task_id,
            ssh_host: normalize_ssh_host(ssh_host),
        };
        let install_key = target.key("all");
        let callbacks = HookInstallCallbacks {
            on_success: options.on_success,
            on_failure: options.on_failure,
        };
        let job = {
            let mut scheduled = self.scheduled_installs.lock().unwrap();
            if let Some(job) = scheduled.get(&install_key) {
                job.lock().unwrap().push(callbacks);
                return None;
            }
            let job: ScheduledCallbacks = Arc::new(Mutex::new(vec![callbacks]));
            scheduled.insert(install_key.clone(), Arc::clone(&job));
            job
        };
        let installer = Arc::clone(self);
        let (path, task_id, ssh_host) = (
            target.path.to_string(),
            task_id.to_string(),
            target.ssh_host.map(str::to_string),
        );
        Some(std::thread::spawn(move || {
            std::thread::sleep(options.delay);
            // 예약된 설치는 기다리는 화면이 없으므로, 화면 요청이 원격 명령 슬롯을 먼저 쓰게 한다
            let result =
                run_as_background(|| installer.install_hooks(&path, &task_id, ssh_host.as_deref()));
            let callbacks = installer.finish_scheduled_install(&install_key, &job);
            notify_install_callbacks(callbacks, &result);
        }))
    }

    /// 예약을 목록에서 빼면서 callback을 꺼낸다. 같은 잠금 안에서 빼므로 뒤늦게 더해진 callback을 잃지 않는다
    fn finish_scheduled_install(
        &self,
        install_key: &str,
        job: &ScheduledCallbacks,
    ) -> Vec<HookInstallCallbacks> {
        let mut scheduled = self.scheduled_installs.lock().unwrap();
        if scheduled
            .get(install_key)
            .is_some_and(|current| Arc::ptr_eq(current, job))
        {
            scheduled.remove(install_key);
        }
        std::mem::take(&mut *job.lock().unwrap())
    }

    /// hook 파일을 다시 쓰지 않고 provider 상태만 확인한다
    pub fn schedule_hooks_verification(
        self: &Arc<Self>,
        target_path: &str,
        task_id: &str,
        ssh_host: Option<&str>,
        options: HookInstallScheduleOptions,
    ) -> JoinHandle<()> {
        let installer = Arc::clone(self);
        let (path, task_id) = (target_path.to_string(), task_id.to_string());
        let ssh_host = normalize_ssh_host(ssh_host).map(str::to_string);
        std::thread::spawn(move || {
            std::thread::sleep(options.delay);
            let target = HookTarget {
                path: &path,
                task_id: &task_id,
                ssh_host: ssh_host.as_deref(),
            };
            let result = run_as_background(|| installer.verify_installation(target));
            let callbacks = HookInstallCallbacks {
                on_success: options.on_success,
                on_failure: options.on_failure,
            };
            notify_install_callbacks(vec![callbacks], &result);
        })
    }
}

fn notify_install_callbacks(
    callbacks: Vec<HookInstallCallbacks>,
    result: &Result<(), HookInstallError>,
) {
    for callback in callbacks {
        match result {
            Ok(()) => callback
                .on_success
                .into_iter()
                .for_each(|on_success| on_success()),
            Err(error) => callback
                .on_failure
                .into_iter()
                .for_each(|on_failure| on_failure(error)),
        }
    }
}

impl KanvibeHooksInstaller {
    /// 최대 3번 시도하고, n번째 실패 뒤에는 재시도 간격 단위의 n배만큼 기다린다
    fn run_with_retry(
        &self,
        operation_label: &str,
        target: HookTarget<'_>,
        provider: Option<KanvibeHookProvider>,
        install: impl Fn() -> Result<(), HookInstallError>,
    ) -> Result<(), HookInstallError> {
        for attempt in 1..=HOOK_INSTALL_MAX_ATTEMPTS {
            let Err(error) = install() else {
                return Ok(());
            };
            if attempt == HOOK_INSTALL_MAX_ATTEMPTS {
                return Err(error);
            }
            let retry_delay = self.timings.retry_delay_step * attempt;
            let detail = format!(
                "{{ targetPath: {:?}, taskId: {:?}, sshHost: {:?}, provider: {:?}, attempt: {attempt}, maxAttempts: {HOOK_INSTALL_MAX_ATTEMPTS}, nextAttemptInMs: {}, error: {:?} }}",
                target.path,
                target.task_id,
                target.ssh_host,
                provider.map(KanvibeHookProvider::label),
                retry_delay.as_millis(),
                error.to_string(),
            );
            self.log.event(
                HookLogLevel::Warn,
                &format!("[hooks] {operation_label} failed; retrying"),
                &detail,
            );
            std::thread::sleep(retry_delay);
        }
        Err(HookInstallError::Message("hooks 설정 실패".into()))
    }

    /// 모든 provider의 hook 파일과 task CLI를 기록한다. 기존 설정 읽기는 한 번으로 묶고,
    /// 쓰기는 provider 단위로 격리해 한 provider가 실패해도 나머지는 설치되게 한다
    fn install_hook_files_once(&self, target: HookTarget<'_>) -> Result<(), HookInstallError> {
        let hook_server_url = self.runtime.hook_server_url(target.ssh_host)?;
        self.operations.register_hook_target(
            target.path,
            target.task_id,
            &hook_server_url,
            target.ssh_host,
        )?;
        let input_paths: Vec<String> = self
            .providers
            .iter()
            .flat_map(|module| module.install_input_paths(target.path))
            .collect();
        let existing_files = self
            .operations
            .read_text_files(&input_paths, target.ssh_host)?;
        let mut writes: Vec<Option<&Arc<dyn KanvibeHookProviderModule>>> =
            self.providers.iter().map(Some).collect();
        // task CLI는 provider와 무관하게 worktree마다 하나만 두고, 원격 왕복이 늘지 않도록 provider 쓰기와 함께 보낸다
        writes.push(None);
        let results = run_in_parallel(&writes, |write| match write {
            Some(module) => self.write_provider_files(
                module.as_ref(),
                target,
                &hook_server_url,
                &existing_files,
            ),
            None => self.write_task_cli(target),
        });
        let failures: Vec<String> = writes
            .iter()
            .zip(results)
            .filter_map(|(write, result)| {
                let label = write.map_or("task CLI", |module| module.provider().label());
                result.err().map(|error| format!("{label}({error})"))
            })
            .collect();
        match failures.is_empty() {
            true => Ok(()),
            false => Err(HookInstallError::Message(format!(
                "hooks 설치 실패: {}",
                failures.join(", ")
            ))),
        }
    }

    fn write_provider_files(
        &self,
        module: &dyn KanvibeHookProviderModule,
        target: HookTarget<'_>,
        hook_server_url: &str,
        existing_files: &HookFileSnapshot,
    ) -> Result<(), HookInstallError> {
        let result = module
            .build_files(target.path, target.task_id, hook_server_url, existing_files)
            .and_then(|files| {
                self.operations
                    .write_hook_provider_files(&files, target.ssh_host)
            });
        if let Err(error) = &result {
            let label = module.provider().label();
            let detail = format!(
                "{{ provider: {label:?}, targetPath: {:?}, taskId: {:?}, sshHost: {:?}, error: {:?} }}",
                target.path,
                target.task_id,
                target.ssh_host,
                error.to_string(),
            );
            self.log.event(
                HookLogLevel::Error,
                &format!("[hooks] {label} install failed"),
                &detail,
            );
        }
        if module.provider() == KanvibeHookProvider::OpenCode && target.ssh_host.is_none() {
            self.open_code_registration.invalidate(target.path);
        }
        result
    }

    fn write_task_cli(&self, target: HookTarget<'_>) -> Result<(), HookInstallError> {
        self.operations.assert_state_paths_are_not_symlinks(
            target.path,
            &[TASK_CLI_FILE_NAME],
            target.ssh_host,
        )?;
        let task_cli_file = build_task_cli_file(target.path, target.ssh_host);
        self.operations
            .write_hook_provider_files(std::slice::from_ref(&task_cli_file), target.ssh_host)
    }

    fn install_hook_provider_once(
        &self,
        module: &dyn KanvibeHookProviderModule,
        target: HookTarget<'_>,
    ) -> Result<(), HookInstallError> {
        let hook_server_url = self.runtime.hook_server_url(target.ssh_host)?;
        module.install(
            target.path,
            target.task_id,
            &hook_server_url,
            target.ssh_host,
        )?;
        let status = module.status(target.path, Some(target.task_id), target.ssh_host, None);
        let failure = self.evaluate_status_result(module.provider(), target, status);
        match failure {
            None => Ok(()),
            Some(failure) => Err(HookInstallError::Message(format!(
                "hooks 검증 실패: {}",
                format_hook_verification_failures(&[failure])
            ))),
        }
    }
}

impl KanvibeHooksInstaller {
    /// provider들이 상태 판정에 쓰는 파일(provider 산출물과 targets.json)을 한 번에 읽는다
    fn read_hook_status_files(
        &self,
        target: HookTarget<'_>,
    ) -> Result<HookFileSnapshot, HookInstallError> {
        let mut paths = self.provider_output_paths(target)?;
        paths.push(get_kanvibe_targets_path(target.path));
        self.operations
            .read_text_files(&deduplicate_paths(paths), target.ssh_host)
    }

    fn provider_output_paths(
        &self,
        target: HookTarget<'_>,
    ) -> Result<Vec<String>, HookInstallError> {
        let empty_files = HookFileSnapshot::new();
        let mut paths = Vec::new();
        for module in &self.providers {
            let files = module.build_files(target.path, target.task_id, "", &empty_files)?;
            paths.extend(files.into_iter().map(|file| file.file_path));
        }
        Ok(paths)
    }

    fn read_hooks_statuses(
        &self,
        target: HookTarget<'_>,
    ) -> Result<KanvibeHookStatuses, HookInstallError> {
        let files = self.read_hook_status_files(target)?;
        let mut statuses = run_in_parallel(&KanvibeHookProvider::ALL, |provider| {
            let module = self.provider_module(*provider)?;
            module.status(
                target.path,
                Some(target.task_id),
                target.ssh_host,
                Some(files.clone()),
            )
        })
        .into_iter()
        .collect::<Result<Vec<KanvibeHookStatus>, HookInstallError>>()?
        .into_iter();
        let mut next_status = || statuses.next().expect("provider 네 개의 상태");
        Ok(KanvibeHookStatuses {
            claude: next_status(),
            gemini: next_status(),
            codex: next_status(),
            open_code: next_status(),
        })
    }

    /// 원격에서는 provider마다 따로 읽으면 검증 접속이 provider 수만큼 생기므로 한 번 읽어 나눠 쓴다.
    /// 묶어 읽기가 실패하면 어느 provider가 원인인지 가릴 수 없어, 그때만 provider별로 다시 읽는다
    fn verify_installation(&self, target: HookTarget<'_>) -> Result<(), HookInstallError> {
        let shared_files = match target.ssh_host {
            Some(_) => self.read_hook_status_files(target).ok(),
            None => None,
        };
        let results = run_in_parallel(&self.providers, |module| {
            module.status(
                target.path,
                Some(target.task_id),
                target.ssh_host,
                shared_files.clone(),
            )
        });
        let failures: Vec<HookVerificationFailure> = self
            .providers
            .iter()
            .zip(results)
            .filter_map(|(module, status)| {
                self.evaluate_status_result(module.provider(), target, status)
            })
            .collect();
        match failures.is_empty() {
            true => Ok(()),
            false => Err(HookInstallError::Message(format!(
                "hooks 검증 실패: {}",
                format_hook_verification_failures(&failures)
            ))),
        }
    }

    /// 상태를 로그로 남기고 실패면 실패 항목을, 상태를 읽지 못했으면 오류를 돌려준다
    fn evaluate_status_result(
        &self,
        provider: KanvibeHookProvider,
        target: HookTarget<'_>,
        status: Result<KanvibeHookStatus, HookInstallError>,
    ) -> Option<HookVerificationFailure> {
        let label = provider.label();
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                let detail = format!(
                    "{{ provider: {label:?}, targetPath: {:?}, taskId: {:?}, sshHost: {:?}, error: {:?} }}",
                    target.path,
                    target.task_id,
                    target.ssh_host,
                    error.to_string(),
                );
                self.log.event(
                    HookLogLevel::Warn,
                    &format!("[hooks] {label} verification unavailable"),
                    &detail,
                );
                return Some(HookVerificationFailure::Unavailable {
                    provider: label,
                    error: error.to_string(),
                });
            }
        };
        let failed_checks = hook_verification_failed_checks(&status);
        let level = if status.is_installed() {
            HookLogLevel::Info
        } else {
            HookLogLevel::Warn
        };
        let record = build_verification_record(
            provider,
            &status,
            &failed_checks,
            (target.path, target.task_id, target.ssh_host),
        );
        self.log.verification(level, &record);
        let fatal_failed_checks =
            fatal_hook_verification_failed_checks(provider, &status, &failed_checks);
        should_fail_hook_verification(&status, &failed_checks, &fatal_failed_checks).then_some(
            HookVerificationFailure::Checks {
                provider: label,
                failed_checks: fatal_failed_checks,
            },
        )
    }
}

impl KanvibeHooksInstaller {
    /// 설치 파일과 현재 client 등록이 최신인지 검사한다. 원격은 같은 파일 묶음을 이미 검증했으면 provider 재검사를 생략한다
    fn inspect_hooks_state(
        &self,
        target: HookTarget<'_>,
    ) -> Result<HooksStateInspection, HookInstallError> {
        let cli_path = build_task_cli_file(target.path, target.ssh_host).file_path;
        let targets_path = get_kanvibe_targets_path(target.path);
        let files =
            self.read_verification_files(target, [cli_path.clone(), targets_path.clone()])?;
        let client_id = self.runtime.hook_client_id()?;
        let verification_key = target.key("all");
        let namespace = match target.ssh_host {
            Some(host) => self.runtime.remote_host_namespace(host)?,
            None => None,
        };
        let fingerprint = format!(
            "{}:{}",
            namespace.as_deref().unwrap_or("legacy"),
            hook_files_fingerprint(&files, &client_id, self.runtime.hook_server_port())
        );
        if target.ssh_host.is_some()
            && self.verified_fingerprint(&verification_key).as_deref() == Some(fingerprint.as_str())
        {
            return Ok(HooksStateInspection {
                is_current: true,
                failures: Vec::new(),
            });
        }
        self.verified_remote_hook_files
            .lock()
            .unwrap()
            .remove(&verification_key);
        let mut failures = self.inspect_provider_statuses(target, &files)?;
        failures.extend(inspect_task_cli_and_target(
            &files,
            (&cli_path, &targets_path),
            &client_id,
            target,
            namespace.as_deref(),
        ));
        let is_current = failures.is_empty();
        // provider가 같은 파일 묶음을 검증했을 때만 재사용해, 검사 도중 바뀐 파일을 정상으로 기억하지 않는다
        if target.ssh_host.is_some() && is_current {
            self.verified_remote_hook_files
                .lock()
                .unwrap()
                .insert(verification_key, fingerprint);
        }
        Ok(HooksStateInspection {
            is_current,
            failures,
        })
    }

    /// 원격은 provider 산출물까지, 로컬은 task CLI와 targets.json만 읽는다. 로컬 provider는 상태 판정 때 직접 읽는다
    fn read_verification_files(
        &self,
        target: HookTarget<'_>,
        state_paths: [String; 2],
    ) -> Result<HookFileSnapshot, HookInstallError> {
        let mut verification_paths = match target.ssh_host {
            Some(_) => self.provider_output_paths(target)?,
            None => Vec::new(),
        };
        verification_paths.extend(state_paths);
        self.operations
            .read_text_files(&deduplicate_paths(verification_paths), target.ssh_host)
    }

    fn verified_fingerprint(&self, verification_key: &str) -> Option<String> {
        self.verified_remote_hook_files
            .lock()
            .unwrap()
            .get(verification_key)
            .cloned()
    }

    /// provider별 파일 확인 실패. 원격 등록은 재접속 때 IP가 바뀔 수 있으므로 현재 client ID로 따로 확인한다
    fn inspect_provider_statuses(
        &self,
        target: HookTarget<'_>,
        files: &HookFileSnapshot,
    ) -> Result<Vec<String>, HookInstallError> {
        let shared_files = target.ssh_host.map(|_| files.clone());
        let statuses = run_in_parallel(&self.providers, |module| {
            module.status(
                target.path,
                Some(target.task_id),
                target.ssh_host,
                shared_files.clone(),
            )
        });
        let mut failures = Vec::new();
        for (module, status) in self.providers.iter().zip(statuses) {
            let label = module.provider().label();
            let status = status?;
            let failed_checks = hook_verification_failed_checks(&status);
            let file_checks: Vec<String> =
                fatal_hook_verification_failed_checks(module.provider(), &status, &failed_checks)
                    .into_iter()
                    .filter(|check| check != "hasRegisteredHookTarget" || target.ssh_host.is_none())
                    .collect();
            let has_unavailable_connection = status.check("hasReachableHookServer") == Some(false)
                || status.check("hasExpectedHookServerUrl") == Some(false);
            if !status.is_installed() && failed_checks.is_empty() && !has_unavailable_connection {
                return Err(HookInstallError::Message(format!(
                    "{label} hooks 검사 결과를 확인할 수 없습니다."
                )));
            }
            if !file_checks.is_empty() {
                failures.push(format!("{label}({})", file_checks.join(", ")));
            }
        }
        Ok(failures)
    }
}

/// task.sh가 최신 전송 파일이고, targets.json에 현재 client의 이 task 등록이 맞는 전송으로 남아 있는지
fn inspect_task_cli_and_target(
    files: &HookFileSnapshot,
    paths: (&str, &str),
    client_id: &str,
    target: HookTarget<'_>,
    namespace: Option<&str>,
) -> Vec<String> {
    let (cli_path, targets_path) = paths;
    let has_current_cli = files
        .get(cli_path)
        .is_some_and(|file| file.exists && file.content.contains(HOOK_TRANSPORT_VERSION_MARKER));
    let targets_content = files
        .get(targets_path)
        .map_or("", |file| file.content.as_str());
    let targets = parse_kanvibe_targets(targets_content).targets;
    let own_target = targets.iter().find(|entry| {
        entry.client_id.as_deref() == Some(client_id) && entry.task_id == target.task_id
    });
    let required_transport = match target.ssh_host {
        Some(_) => HookTargetTransport::Ssh,
        None => HookTargetTransport::Local,
    };
    let has_current_target = own_target.is_some_and(|entry| {
        entry
            .token
            .as_deref()
            .is_some_and(|token| !token.is_empty())
            && entry.transport == Some(required_transport)
            && entry.remote_host_namespace.as_deref() == namespace
    });
    let mut failures = Vec::new();
    if !has_current_cli {
        failures.push("task.sh(최신 전송 파일이 없습니다)".to_string());
    }
    if !has_current_target {
        failures.push("targets.json(현재 client 등록이 없습니다)".to_string());
    }
    failures
}
