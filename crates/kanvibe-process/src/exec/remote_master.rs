//! 호스트별 공용 ControlMaster 연결. 명령 채널과 별개로 수명을 관리한다.
//! 마스터는 `ssh -N -f`로 따로 띄우고, ControlPersist 10분이 끝나기 전에 다시 확인하도록 준비 상태를 9분만 믿는다.
//! 준비는 별도 스레드에서 돌아, 기한이 짧은 호출자가 먼저 포기해도 같은 준비를 기다리는 다른 호출자는 계속 쓸 수 있다.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::ExecGitError;
use super::remote_command::{CommandFailure, attach_output, exit_error, timeout_error};
use super::ssh_process::{SshEventSink, SshProcessEvent, SshProcessLauncher, SshSpawnMode};
use crate::shell_environment::ShellEnvironment;

/// 10분 ControlPersist 유휴 창이 끝나기 전에 다시 확인한다
pub(crate) const REMOTE_SSH_MASTER_READY_TTL_MS: u64 = 9 * 60_000;
/// 마스터 준비의 상한. TS는 명령 기본 시간 초과(45초)를 그대로 쓴다
pub(crate) const MASTER_PREPARATION_TIMEOUT: Duration = Duration::from_millis(45_000);

enum PreparationState {
    Pending,
    Ready,
    Failed(CommandFailure),
}

pub(crate) struct MasterConnection {
    pub generation: u64,
    /// 마지막으로 준비를 확인한 시각(ms). 0이면 아직 한 번도 준비되지 않았다
    pub ready_at_ms: AtomicU64,
    pub has_failed: AtomicBool,
    preparation: Mutex<PreparationState>,
    prepared: Condvar,
}

impl MasterConnection {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            ready_at_ms: AtomicU64::new(0),
            has_failed: AtomicBool::new(false),
            preparation: Mutex::new(PreparationState::Pending),
            prepared: Condvar::new(),
        }
    }

    pub fn is_reusable(&self, now_ms: u64) -> bool {
        let ready_at_ms = self.ready_at_ms.load(Ordering::SeqCst);
        !self.has_failed.load(Ordering::SeqCst)
            && (ready_at_ms == 0
                || now_ms.saturating_sub(ready_at_ms) < REMOTE_SSH_MASTER_READY_TTL_MS)
    }

    /// 준비 결과를 기록하고 기다리는 호출자를 깨운다
    pub fn finish_preparation(&self, result: Result<u64, CommandFailure>) {
        let state = match result {
            Ok(ready_at_ms) => {
                self.ready_at_ms.store(ready_at_ms, Ordering::SeqCst);
                PreparationState::Ready
            }
            Err(failure) => {
                self.has_failed.store(true, Ordering::SeqCst);
                PreparationState::Failed(failure)
            }
        };
        *self.preparation.lock().unwrap() = state;
        self.prepared.notify_all();
    }

    /// 준비가 끝날 때까지 기다린다. 기한이 지나도 준비 자체는 취소하지 않는다
    pub fn wait_for_preparation(&self, deadline: Instant) -> Result<(), CommandFailure> {
        let wait_timeout = deadline.saturating_duration_since(Instant::now());
        let mut state = self.preparation.lock().unwrap();
        loop {
            match &*state {
                PreparationState::Ready => return Ok(()),
                PreparationState::Failed(failure) => return Err(failure.clone()),
                PreparationState::Pending => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                let mut failure = timeout_error(wait_timeout, "", "");
                failure.is_preparation_wait_timeout = true;
                return Err(failure);
            }
            state = self.prepared.wait_timeout(state, remaining).unwrap().0;
        }
    }
}

/// `-N -f` 마스터를 띄우고 준비될 때까지 기다린다.
/// `-f`는 stderr를 쥔 채 유휴 마스터를 분기하므로 파이프 닫힘이 아니라 부모 프로세스의 정상 종료로 준비를 확인한다
pub(crate) fn spawn_ssh_master(
    launcher: &dyn SshProcessLauncher,
    environment: &ShellEnvironment,
    args: &[String],
) -> Result<(), CommandFailure> {
    let (sender, receiver) = mpsc::channel();
    let sender = Mutex::new(sender);
    let sink: SshEventSink = Arc::new(move |event| {
        let _ = sender.lock().unwrap().send(event);
    });
    let mut process = launcher.spawn(args, SshSpawnMode::Master, environment, sink);
    let deadline = Instant::now() + MASTER_PREPARATION_TIMEOUT;
    let mut stderr = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
            Ok(SshProcessEvent::Stderr(chunk)) => stderr.push_str(&chunk),
            Ok(SshProcessEvent::Exit(Some(0))) => return Ok(()),
            Ok(SshProcessEvent::Exit(_) | SshProcessEvent::Stdout(_)) => {}
            Ok(SshProcessEvent::Close { code, signal }) => {
                return Err(exit_error(code, signal, "", &stderr));
            }
            Ok(SshProcessEvent::Error(error)) => return Err(attach_output(error, "", &stderr)),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                process.kill();
                return Err(timeout_error(MASTER_PREPARATION_TIMEOUT, "", &stderr));
            }
        }
    }
}

pub(crate) fn control_directory_failure(error: std::io::Error) -> CommandFailure {
    CommandFailure::plain(ExecGitError::new(error.to_string()))
}
