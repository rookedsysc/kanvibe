//! 모든 명령 실행이 거치는 `exec_git(command, ssh_host)` 계약.
//! ssh_host가 없으면 로컬 `sh -c`로, 있으면 시스템 `ssh`로 같은 셸 문자열을 실행하고 앞뒤 공백을 뺀 stdout을 돌려준다.
//! 다른 모듈은 이 trait만 받으므로 테스트에서는 `testing`의 가짜 실행기로 명령 문자열을 검사한다.

pub mod local;
pub mod remote;
mod remote_admission;
mod remote_command;
mod remote_errors;
mod remote_master;
#[cfg(test)]
mod remote_tests;
pub mod ssh_process;
pub mod system;

pub use remote_admission::run_with_background_ssh_commands;
pub use remote_errors::{is_ssh_connection_error, is_ssh_transport_error};
#[cfg(test)]
pub(crate) mod testing;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// 원격 셸이 `marker\t<payload>` 한 줄을 내보내면 한 번 부르는 응답기. 반환값을 원격 stdin으로 보낸다
pub type RemoteStdinRespond = dyn Fn(&str) -> Result<String, ExecGitError> + Send + Sync;

#[derive(Clone)]
pub struct RemoteStdinResponder {
    pub marker: String,
    pub respond: Arc<RemoteStdinRespond>,
}

impl fmt::Debug for RemoteStdinResponder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteStdinResponder")
            .field("marker", &self.marker)
            .finish_non_exhaustive()
    }
}

/// TS `ExecGitOptions`와 같은 선택지. 기본값은 TS에서 옵션을 주지 않은 경우와 같다
#[derive(Debug, Clone, Default)]
pub struct ExecGitOptions {
    pub timeout: Option<Duration>,
    pub max_buffer_bytes: Option<usize>,
    /// 원격 host daemon처럼 공유 ControlMaster 없이 호스트 키를 엄격히 확인하는 분리 연결을 쓴다
    pub should_check_host_key_strictly: bool,
    /// 다시 실행해도 원격 부작용이 겹치지 않는 명령만 켠다
    pub is_retry_safe: bool,
    /// 원격 명령의 표준 입력. 자격 증명처럼 프로세스 인자에 남으면 안 되는 값을 싣는다
    pub remote_stdin: Option<String>,
    /// 세션이 꺼져 있으면 실패하는 폴링처럼 실패가 예상돼 오류 로그를 남기지 않는다(TS `shouldLogFailure: false`)
    pub is_failure_expected: bool,
    pub remote_stdin_responder: Option<RemoteStdinResponder>,
}

/// Node `child_process` 오류처럼 메시지와 출력, 종료 정보를 함께 담는다
#[derive(Debug, Clone, Default, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ExecGitError {
    pub message: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// `ETIMEDOUT`, `E2BIG`처럼 숫자 종료 코드가 아닌 Node 오류 코드
    pub error_code: Option<&'static str>,
    /// 시간 초과나 출력 상한으로 프로세스를 죽였다
    pub was_killed: bool,
    pub signal: Option<&'static str>,
    /// 원격 명령이 끝났다는 marker를 받았다. 전송 실패가 아니라 명령 자체의 실패다
    pub has_remote_command_completed: bool,
    /// 전송 실패 쿨다운이 풀리기까지 남은 시간(ms)
    pub retry_after_ms: Option<std::num::NonZeroU32>,
}

impl ExecGitError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            ..Self::default()
        }
    }
}

/// `execGit`. 구현은 로컬·원격 실행을 모두 맡는 `system::SystemShellCommandRunner`다
pub trait ShellCommandRunner: Send + Sync {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError>;
}
