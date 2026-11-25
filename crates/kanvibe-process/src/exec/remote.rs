//! 원격 실행(TS `execRemote`). 시스템 `ssh`와 프로세스 소유 ControlMaster로 같은 셸 문자열을 원격에서 실행한다.
//! 호스트별 채널 상한 8(백그라운드 2), 45초 기본 시간 초과, `is_retry_safe` 명령만 최대 3회 시도, 전송 실패 뒤 5~60초 쿨다운을 둔다.
//! 호스트 키를 엄격히 확인하는 명령은 공용 마스터 없이 독립 연결을 쓰고, 같은 주소의 인증은 한 번에 하나씩 통과시킨다.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::remote_admission::{
    AuthenticationQueues, AuthenticationTurn, HostSlots, REMOTE_SSH_HOST_MAX_CHANNELS,
    ResponderContext, ResponderScope, authentication_key, is_background_priority,
    is_nested_responder_command, remaining_timeout,
};
use super::remote_command::{
    CommandFailure, SshCommandRequest, build_remote_shell_command, run_ssh_command,
};
use super::remote_errors::{
    is_ssh_transport_error, normalize_ssh_exec_error, should_log_remote_command_failure,
};
use super::remote_master::{MasterConnection, control_directory_failure, spawn_ssh_master};
use super::ssh_process::{SshProcessLauncher, SystemSshLauncher};
use super::{ExecGitError, ExecGitOptions, RemoteStdinResponder};
use crate::shell_environment::ShellEnvironment;
use crate::ssh_config::{
    SshArgsOptions, SshConfigReader, SshControlDirectory, SshHostConfig, build_ssh_args,
    kanvibe_ssh_connection_health_options,
};

const DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS: u64 = 45_000;
const DEFAULT_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS: u64 = 5_000;
const MAX_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS: u64 = 60_000;
const REMOTE_SSH_COMMAND_MAX_ATTEMPTS: u32 = 3;
const REMOTE_COMMAND_MAX_BUFFER_BYTES: usize = 10 * 1024 * 1024;
/// 이보다 오래 걸린 원격 명령만 단계별 시간을 남긴다. 정상 명령까지 남기면 폴링 로그에 묻힌다
const REMOTE_SSH_SLOW_COMMAND_LOG_MS: u64 = 3_000;
const REMOTE_SSH_SLOW_COMMAND_LOG_COMMAND_LENGTH: usize = 120;

/// `KANVIBE_REMOTE_SSH_*` 조정값. 프로세스 환경을 읽기만 하고, 테스트는 값을 직접 넘긴다
#[derive(Debug, Clone)]
pub struct RemoteSshSettings {
    pub host_max_concurrency: usize,
    pub command_timeout: Duration,
    pub transport_failure_cooldown: Duration,
    /// 느린 명령 로그 기준. 운영값은 3초로 고정이고 테스트만 줄인다
    pub(crate) slow_command_log_threshold: Duration,
}

impl Default for RemoteSshSettings {
    fn default() -> Self {
        Self {
            host_max_concurrency: REMOTE_SSH_HOST_MAX_CHANNELS,
            command_timeout: Duration::from_millis(DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS),
            transport_failure_cooldown: Duration::from_millis(
                DEFAULT_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS,
            ),
            slow_command_log_threshold: Duration::from_millis(REMOTE_SSH_SLOW_COMMAND_LOG_MS),
        }
    }
}

/// Node `parseInt` 뒤 1보다 작거나 NaN이면 기본값을 쓰는 TS `readPositiveInteger`
fn read_positive_integer(value: Option<&String>, fallback: u64) -> u64 {
    let digits: String = value
        .map(|value| {
            value
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect()
        })
        .unwrap_or_default();
    match digits.parse::<u64>() {
        Ok(parsed) if parsed >= 1 => parsed,
        _ => fallback,
    }
}

impl RemoteSshSettings {
    /// 부모 프로세스 환경에서 조정값을 읽는다. 자식 셸 환경은 `KANVIBE_*`가 지워져 있으므로 넘기지 않는다
    pub fn from_environment(environment: &ShellEnvironment) -> Self {
        let concurrency = read_positive_integer(
            environment.get("KANVIBE_REMOTE_SSH_HOST_MAX_CONCURRENCY"),
            REMOTE_SSH_HOST_MAX_CHANNELS as u64,
        );
        let timeout_ms = read_positive_integer(
            environment.get("KANVIBE_REMOTE_SSH_COMMAND_TIMEOUT_MS"),
            DEFAULT_REMOTE_SSH_COMMAND_TIMEOUT_MS,
        );
        let cooldown_ms = read_positive_integer(
            environment.get("KANVIBE_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS"),
            DEFAULT_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS,
        );
        Self {
            host_max_concurrency: (concurrency as usize).min(REMOTE_SSH_HOST_MAX_CHANNELS),
            command_timeout: Duration::from_millis(timeout_ms),
            transport_failure_cooldown: Duration::from_millis(
                cooldown_ms.min(MAX_REMOTE_SSH_TRANSPORT_FAILURE_COOLDOWN_MS),
            ),
            ..Self::default()
        }
    }
}

/// `[remote-ssh] command failed` 로그 내용
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCommandFailureLog {
    pub ssh_host: String,
    pub command: String,
    pub ssh_args: Vec<String>,
    pub error_message: String,
}

/// `[remote-ssh] slow command` 로그 내용. 시간은 모두 ms다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowRemoteCommandLog {
    pub ssh_host: String,
    pub priority: &'static str,
    pub total_ms: u64,
    pub slot_wait_ms: u64,
    pub connection_wait_ms: u64,
    pub command_ms: u64,
    pub command: String,
}

pub trait RemoteCommandObserver: Send + Sync {
    fn command_failed(&self, log: &RemoteCommandFailureLog);
    fn slow_command(&self, log: &SlowRemoteCommandLog);
}

/// TS `console.error`·`console.warn`처럼 표준 오류로 남긴다
pub struct StderrRemoteCommandObserver;

impl RemoteCommandObserver for StderrRemoteCommandObserver {
    fn command_failed(&self, log: &RemoteCommandFailureLog) {
        eprintln!("[remote-ssh] command failed {log:?}");
    }

    fn slow_command(&self, log: &SlowRemoteCommandLog) {
        eprintln!("[remote-ssh] slow command {log:?}");
    }
}

/// 쿨다운 만료와 마스터 준비 시각을 재는 벽시계(ms)
pub type WallClock = Arc<dyn Fn() -> u64 + Send + Sync>;

fn retry_after(milliseconds: u64) -> Option<std::num::NonZeroU32> {
    std::num::NonZeroU32::new(u32::try_from(milliseconds).unwrap_or(u32::MAX))
}

fn system_wall_clock() -> WallClock {
    Arc::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() as u64)
    })
}

struct TransportCooldown {
    until_ms: u64,
    error_message: String,
}

/// 원격 명령 하나가 어디에서 시간을 썼는지. 재시도한 명령은 시도별 시간을 더한다
#[derive(Default)]
struct CommandTiming {
    slot_wait: Duration,
    connection_wait: Duration,
    command: Duration,
}

/// 실행기 부품. 테스트는 가짜 ssh 실행기·관찰자·시계를 넣는다
pub(crate) struct RemoteSshParts {
    pub launcher: Arc<dyn SshProcessLauncher>,
    pub observer: Arc<dyn RemoteCommandObserver>,
    pub wall_clock: WallClock,
}

struct RemoteSshShared {
    settings: RemoteSshSettings,
    environment: ShellEnvironment,
    parts: RemoteSshParts,
    config_reader: SshConfigReader,
    control_directory: SshControlDirectory,
    slots: Arc<HostSlots>,
    authentication: Arc<AuthenticationQueues>,
    masters: Mutex<HashMap<String, Arc<MasterConnection>>>,
    next_master_generation: AtomicU64,
    cooldowns: Mutex<HashMap<String, TransportCooldown>>,
}

/// 원격 명령 실행기. 호스트별 채널·마스터·쿨다운 상태를 실행기 하나가 소유한다
pub struct RemoteSshExecutor {
    shared: Arc<RemoteSshShared>,
}

/// 한 번의 시도에서 바뀌는 상태
struct AttemptState {
    master: Option<Arc<MasterConnection>>,
    ssh_args: Vec<String>,
    is_waiting_for_master: bool,
    started_at: Instant,
    command_started_at: Option<Instant>,
}

/// 명령 하나를 실행하는 동안 바뀌지 않는 값
struct CommandScope<'a> {
    ssh_host: &'a str,
    command: &'a str,
    options: &'a ExecGitOptions,
    deadline: Instant,
    timeout: Duration,
    host_config: SshHostConfig,
    master_key: String,
}

impl RemoteSshExecutor {
    /// `home_directory`는 `~/.ssh/config`와 `~/.kanvibe` 제어 소켓의 기준이고, `environment`는 ssh 자식 프로세스 환경이다
    pub fn new(
        home_directory: &str,
        environment: ShellEnvironment,
        settings: RemoteSshSettings,
    ) -> Self {
        let parts = RemoteSshParts {
            launcher: Arc::new(SystemSshLauncher),
            observer: Arc::new(StderrRemoteCommandObserver),
            wall_clock: system_wall_clock(),
        };
        Self::with_parts(home_directory, environment, settings, parts)
    }

    pub(crate) fn with_parts(
        home_directory: &str,
        environment: ShellEnvironment,
        settings: RemoteSshSettings,
        parts: RemoteSshParts,
    ) -> Self {
        let slots = Arc::new(HostSlots::new(settings.host_max_concurrency));
        Self {
            shared: Arc::new(RemoteSshShared {
                settings,
                environment,
                parts,
                config_reader: SshConfigReader::new(home_directory),
                control_directory: SshControlDirectory::new(home_directory),
                slots,
                authentication: Arc::new(AuthenticationQueues::default()),
                masters: Mutex::new(HashMap::new()),
                next_master_generation: AtomicU64::new(0),
                cooldowns: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn ssh_config_reader(&self) -> &SshConfigReader {
        &self.shared.config_reader
    }

    /// 사용자가 직접 원격 연결을 요청하면 백그라운드 실패로 걸린 쿨다운을 풀어 바로 다시 시도하게 한다.
    /// 쿨다운은 백그라운드 명령이 끊긴 연결로 몰리는 것을 막으려는 장치라서, 사용자 요청까지 막으면 터미널이 열리지 않는다.
    pub fn clear_transport_cooldown(&self, ssh_host: &str) {
        self.shared.cooldowns.lock().unwrap().remove(ssh_host);
    }

    #[cfg(test)]
    pub(crate) fn queued_count(&self, ssh_host: &str) -> usize {
        self.shared.slots.queued_count(ssh_host)
    }

    /// 지금 캐시에 있는 마스터의 세대와 실패 여부
    #[cfg(test)]
    pub(crate) fn cached_masters(&self) -> Vec<(u64, bool)> {
        let masters = self.shared.masters.lock().unwrap();
        masters
            .values()
            .map(|master| (master.generation, master.has_failed.load(Ordering::SeqCst)))
            .collect()
    }

    /// 이미 버려진 세대의 마스터를 기다리던 시도가 뒤늦게 실패를 처리하는 상황을 재현한다
    #[cfg(test)]
    pub(crate) fn judge_stale_master_failure(
        &self,
        ssh_host: &str,
        command: &str,
        failure: CommandFailure,
        stale_generation: u64,
    ) -> ExecGitError {
        let host_config = self
            .shared
            .config_reader
            .parse_ssh_config()
            .into_iter()
            .find(|config| config.host == ssh_host)
            .unwrap();
        let options = ExecGitOptions::default();
        let scope = CommandScope {
            ssh_host,
            command,
            options: &options,
            deadline: Instant::now() + Duration::from_secs(1),
            timeout: Duration::from_secs(45),
            master_key: host_config.identity_json(),
            host_config,
        };
        let state = AttemptState {
            master: Some(Arc::new(MasterConnection::new(stale_generation))),
            ssh_args: Vec::new(),
            is_waiting_for_master: false,
            started_at: Instant::now(),
            command_started_at: None,
        };
        match self.shared.judge_failure(
            &scope,
            &state,
            failure,
            None,
            REMOTE_SSH_COMMAND_MAX_ATTEMPTS,
        ) {
            FailureDecision::Fail(failure) => *failure.error,
            FailureDecision::Retry => panic!("마지막 시도는 다시 하지 않는다"),
        }
    }

    /// 원격에서 명령을 실행하고 앞뒤 공백을 뺀 stdout을 돌려준다
    pub fn exec(
        &self,
        command: &str,
        ssh_host: &str,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        let shared = &self.shared;
        let timeout = options.timeout.unwrap_or(shared.settings.command_timeout);
        let requested_at = Instant::now();
        let deadline = requested_at + timeout;
        let mut timing = CommandTiming::default();
        if is_nested_responder_command(ssh_host) {
            let result = shared.execute(ssh_host, command, options, deadline, timeout, &mut timing);
            shared.log_slow_command(ssh_host, command, requested_at.elapsed(), &timing);
            return result.map_err(|failure| *failure.error);
        }
        let mut has_acquired_slot = false;
        let result = shared.slots.acquire(ssh_host, deadline).and_then(|lease| {
            has_acquired_slot = true;
            timing.slot_wait = requested_at.elapsed();
            let result = shared.execute(ssh_host, command, options, deadline, timeout, &mut timing);
            drop(lease);
            result
        });
        let total = requested_at.elapsed();
        if !has_acquired_slot {
            // 슬롯을 끝내 얻지 못하고 시간 초과된 명령은 전체 시간이 슬롯 대기다
            timing.slot_wait = total;
        }
        shared.log_slow_command(ssh_host, command, total, &timing);
        result.map_err(|failure| match failure.is_timeout() {
            true => normalize_ssh_exec_error(&failure, ssh_host, timeout, true),
            false => *failure.error,
        })
    }
}

impl RemoteSshShared {
    fn now_ms(&self) -> u64 {
        (self.parts.wall_clock)()
    }

    fn log_slow_command(
        &self,
        ssh_host: &str,
        command: &str,
        total: Duration,
        timing: &CommandTiming,
    ) {
        if total < self.settings.slow_command_log_threshold {
            return;
        }
        let log = SlowRemoteCommandLog {
            ssh_host: ssh_host.to_string(),
            priority: if is_background_priority() {
                "background"
            } else {
                "foreground"
            },
            total_ms: total.as_millis() as u64,
            slot_wait_ms: timing.slot_wait.as_millis() as u64,
            connection_wait_ms: timing.connection_wait.as_millis() as u64,
            command_ms: timing.command.as_millis() as u64,
            command: command
                .chars()
                .take(REMOTE_SSH_SLOW_COMMAND_LOG_COMMAND_LENGTH)
                .collect(),
        };
        self.parts.observer.slow_command(&log);
    }

    fn throw_if_transport_cooldown_active(&self, ssh_host: &str) -> Result<(), CommandFailure> {
        let mut cooldowns = self.cooldowns.lock().unwrap();
        let Some(cooldown) = cooldowns.get(ssh_host) else {
            return Ok(());
        };
        let now_ms = self.now_ms();
        if now_ms >= cooldown.until_ms {
            cooldowns.remove(ssh_host);
            return Ok(());
        }
        let mut error = ExecGitError::new(format!(
            "{ssh_host} 원격 명령 실패: 최근 SSH transport 실패로 원격 명령을 잠시 건너뜁니다. 마지막 오류: {}",
            cooldown.error_message
        ));
        error.retry_after_ms = retry_after(cooldown.until_ms - now_ms);
        Err(CommandFailure::plain(error))
    }

    fn remember_transport_failure(&self, ssh_host: &str, error: &mut ExecGitError) {
        if !is_ssh_transport_error(error) {
            return;
        }
        let retry_after_ms = self.settings.transport_failure_cooldown.as_millis() as u64;
        error.retry_after_ms = retry_after(retry_after_ms);
        let cooldown = TransportCooldown {
            until_ms: self.now_ms() + retry_after_ms,
            error_message: error.message.clone(),
        };
        self.cooldowns
            .lock()
            .unwrap()
            .insert(ssh_host.to_string(), cooldown);
    }

    fn current_master_generation(&self, master_key: &str) -> Option<u64> {
        self.masters
            .lock()
            .unwrap()
            .get(master_key)
            .map(|master| master.generation)
    }

    fn execute(
        self: &Arc<Self>,
        ssh_host: &str,
        command: &str,
        options: &ExecGitOptions,
        deadline: Instant,
        timeout: Duration,
        timing: &mut CommandTiming,
    ) -> Result<String, CommandFailure> {
        remaining_timeout(deadline)?;
        self.throw_if_transport_cooldown_active(ssh_host)?;
        let host_config = self
            .config_reader
            .parse_ssh_config()
            .into_iter()
            .find(|config| config.host == ssh_host)
            .ok_or_else(|| {
                CommandFailure::message(format!("SSH 호스트를 찾을 수 없습니다: {ssh_host}"))
            })?;
        let master_key = host_config.identity_json();
        let scope = CommandScope {
            ssh_host,
            command,
            options,
            deadline,
            timeout,
            host_config,
            master_key,
        };
        self.execute_attempts(&scope, timing)
    }

    fn execute_attempts(
        self: &Arc<Self>,
        scope: &CommandScope<'_>,
        timing: &mut CommandTiming,
    ) -> Result<String, CommandFailure> {
        let is_independent = scope.options.should_check_host_key_strictly;
        let mut attempt = 1;
        loop {
            let independent_generation = self.current_master_generation(&scope.master_key);
            let mut state = AttemptState {
                master: None,
                ssh_args: Vec::new(),
                is_waiting_for_master: false,
                started_at: Instant::now(),
                command_started_at: None,
            };
            let result = if is_independent {
                self.run_independent_attempt(scope, &mut state, timing)
            } else {
                self.run_attempt(scope, &mut state, None, timing)
            };
            let failure = match result {
                Ok(output) => return Ok(output),
                Err(failure) => failure,
            };
            if state.command_started_at.is_none() {
                // 연결 준비나 인증 차례를 기다리다 끝난 시도는 명령을 띄우지 못했으므로 그 시간을 연결 대기로 센다
                timing.connection_wait += state.started_at.elapsed();
            }
            let decision =
                self.judge_failure(scope, &state, failure, independent_generation, attempt);
            match decision {
                FailureDecision::Retry => attempt += 1,
                FailureDecision::Fail(failure) => return Err(failure),
            }
        }
    }

    /// 독립 연결은 인증 차례를 기다린 뒤 실행하고, 명령이 끝나면(또는 응답기 marker를 받으면) 차례를 놓는다
    fn run_independent_attempt(
        self: &Arc<Self>,
        scope: &CommandScope<'_>,
        state: &mut AttemptState,
        timing: &mut CommandTiming,
    ) -> Result<String, CommandFailure> {
        let key = authentication_key(&scope.host_config.hostname, scope.host_config.port);
        let turn = self.authentication.acquire(&key, Some(scope.deadline))?;
        let result = self.run_attempt(scope, state, Some(Arc::clone(&turn)), timing);
        turn.release();
        result
    }

    fn run_attempt(
        self: &Arc<Self>,
        scope: &CommandScope<'_>,
        state: &mut AttemptState,
        turn: Option<Arc<AuthenticationTurn>>,
        timing: &mut CommandTiming,
    ) -> Result<String, CommandFailure> {
        self.throw_if_transport_cooldown_active(scope.ssh_host)?;
        if !scope.options.should_check_host_key_strictly {
            let master = self.ensure_master(&scope.host_config, &scope.master_key);
            state.master = Some(Arc::clone(&master));
            state.is_waiting_for_master = true;
            master.wait_for_preparation(scope.deadline)?;
            state.is_waiting_for_master = false;
            master.ready_at_ms.store(self.now_ms(), Ordering::SeqCst);
        }
        let command_started_at = Instant::now();
        state.command_started_at = Some(command_started_at);
        timing.connection_wait += command_started_at - state.started_at;
        let mut args =
            self.build_exec_ssh_args(&scope.host_config, state.master.as_deref(), scope.options);
        args.push(build_remote_shell_command(scope.command));
        state.ssh_args = args.clone();
        let is_active = Arc::new(AtomicBool::new(true));
        let responder = scope
            .options
            .remote_stdin_responder
            .as_ref()
            .map(|responder| {
                wrap_responder(responder, turn, scope.ssh_host, Arc::clone(&is_active))
            });
        let request = SshCommandRequest {
            args,
            timeout: remaining_timeout(scope.deadline)?,
            max_buffer_bytes: scope
                .options
                .max_buffer_bytes
                .unwrap_or(REMOTE_COMMAND_MAX_BUFFER_BYTES),
            remote_stdin: scope.options.remote_stdin.clone(),
            responder,
        };
        let result = run_ssh_command(self.parts.launcher.as_ref(), &self.environment, request);
        is_active.store(false, Ordering::SeqCst);
        timing.command += command_started_at.elapsed();
        result
    }

    fn build_exec_ssh_args(
        &self,
        host_config: &SshHostConfig,
        master: Option<&MasterConnection>,
        options: &ExecGitOptions,
    ) -> Vec<String> {
        let connection_reuse = master.map(|master| {
            self.control_directory
                .connection_reuse_options(Some(master.generation), Some(host_config))
        });
        let args_options = SshArgsOptions {
            is_tty_disabled: true,
            connection_health: Some(kanvibe_ssh_connection_health_options()),
            should_check_host_key_strictly: options.should_check_host_key_strictly,
            connection_reuse,
            ..SshArgsOptions::default()
        };
        let mut args = build_ssh_args(host_config, &args_options);
        if master.is_some() {
            if let Some(control_master) = args.iter_mut().find(|arg| *arg == "ControlMaster=auto") {
                *control_master = "ControlMaster=no".into();
            }
            // 죽은 mux 소켓은 무한정 새 인증을 시작하지 말고 실패해야 한다
            args.splice(0..0, ["-o".to_string(), "ProxyCommand=false".to_string()]);
        }
        args
    }

    fn ensure_master(
        self: &Arc<Self>,
        host_config: &SshHostConfig,
        master_key: &str,
    ) -> Arc<MasterConnection> {
        let mut masters = self.masters.lock().unwrap();
        if let Some(existing) = masters.get(master_key)
            && existing.is_reusable(self.now_ms())
        {
            return Arc::clone(existing);
        }
        let generation = self.next_master_generation.fetch_add(1, Ordering::SeqCst);
        let master = Arc::new(MasterConnection::new(generation));
        masters.insert(master_key.to_string(), Arc::clone(&master));
        drop(masters);
        let shared = Arc::clone(self);
        let host_config = host_config.clone();
        let preparing = Arc::clone(&master);
        std::thread::spawn(move || shared.prepare_master(&host_config, &preparing));
        master
    }

    fn prepare_master(&self, host_config: &SshHostConfig, master: &MasterConnection) {
        let key = authentication_key(&host_config.hostname, host_config.port);
        let turn = self.authentication.acquire(&key, None);
        let result = self
            .control_directory
            .ensure()
            .map_err(control_directory_failure)
            .and_then(|()| {
                let args_options = SshArgsOptions {
                    is_tty_disabled: true,
                    connection_reuse: Some(
                        self.control_directory
                            .connection_reuse_options(Some(master.generation), Some(host_config)),
                    ),
                    connection_health: Some(kanvibe_ssh_connection_health_options()),
                    ..SshArgsOptions::default()
                };
                let mut args = vec!["-N".to_string(), "-f".to_string()];
                args.extend(build_ssh_args(host_config, &args_options));
                spawn_ssh_master(self.parts.launcher.as_ref(), &self.environment, &args)
            });
        let ready_at = result.map(|()| self.now_ms());
        drop(turn);
        master.finish_preparation(ready_at);
    }

    /// 실패한 시도를 다시 할지, 어떤 오류로 끝낼지 정한다
    fn judge_failure(
        &self,
        scope: &CommandScope<'_>,
        state: &AttemptState,
        failure: CommandFailure,
        independent_generation: Option<u64>,
        attempt: u32,
    ) -> FailureDecision {
        let is_timeout = failure.is_timeout();
        let mut normalized =
            normalize_ssh_exec_error(&failure, scope.ssh_host, scope.timeout, is_timeout);
        let is_transient = is_ssh_transport_error(&normalized);
        let owns_current_generation =
            self.forget_failed_master(scope, state, &failure, is_transient, independent_generation);
        let should_retry = scope.options.is_retry_safe
            && scope.options.remote_stdin_responder.is_none()
            && !failure.error.has_remote_command_completed
            && (is_transient || is_timeout)
            && attempt < REMOTE_SSH_COMMAND_MAX_ATTEMPTS
            && Instant::now() < scope.deadline;
        if should_retry {
            return FailureDecision::Retry;
        }
        if !scope.options.is_failure_expected && should_log_remote_command_failure(scope.command) {
            let log = RemoteCommandFailureLog {
                ssh_host: scope.ssh_host.to_string(),
                command: scope.command.to_string(),
                ssh_args: state.ssh_args.clone(),
                error_message: failure.error.message.clone(),
            };
            self.parts.observer.command_failed(&log);
        }
        if is_transient && owns_current_generation {
            self.remember_transport_failure(scope.ssh_host, &mut normalized);
        }
        FailureDecision::Fail(CommandFailure::plain(normalized))
    }

    /// 이전 명령의 실패가 더 새로운 요청이 준비한 마스터를 버리지 않도록, 지금 마스터를 소유한 시도만 마스터를 지운다
    fn forget_failed_master(
        &self,
        scope: &CommandScope<'_>,
        state: &AttemptState,
        failure: &CommandFailure,
        is_transient: bool,
        independent_generation: Option<u64>,
    ) -> bool {
        let mut masters = self.masters.lock().unwrap();
        let current_generation = masters
            .get(&scope.master_key)
            .map(|master| master.generation);
        let owned_generation = match scope.options.should_check_host_key_strictly {
            true => independent_generation,
            false => state.master.as_ref().map(|master| master.generation),
        };
        let owns_current_generation = current_generation == owned_generation;
        let Some(master) = &state.master else {
            return owns_current_generation;
        };
        let is_abandoned_wait = state.is_waiting_for_master && failure.is_preparation_wait_timeout;
        let is_never_ready = master.ready_at_ms.load(Ordering::SeqCst) == 0;
        if owns_current_generation && !is_abandoned_wait && (is_transient || is_never_ready) {
            masters.remove(&scope.master_key);
        }
        owns_current_generation
    }
}

enum FailureDecision {
    Retry,
    Fail(CommandFailure),
}

/// 응답기 marker는 CLI가 주는 가장 이른 인증 완료 신호다. callback이 이 차례 뒤에 줄 선 마스터에 합류할 수 있도록 먼저 차례를 놓는다
fn wrap_responder(
    responder: &RemoteStdinResponder,
    turn: Option<Arc<AuthenticationTurn>>,
    ssh_host: &str,
    is_active: Arc<AtomicBool>,
) -> RemoteStdinResponder {
    let respond = Arc::clone(&responder.respond);
    let context = ResponderContext {
        ssh_host: ssh_host.to_string(),
        is_active,
    };
    let is_background = is_background_priority();
    RemoteStdinResponder {
        marker: responder.marker.clone(),
        respond: Arc::new(move |payload| {
            if let Some(turn) = &turn {
                turn.release();
            }
            let scope = ResponderScope::enter(context.clone(), is_background);
            let reply = respond(payload);
            context.is_active.store(false, Ordering::SeqCst);
            drop(scope);
            reply
        }),
    }
}
