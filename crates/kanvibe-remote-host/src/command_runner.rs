//! 원격 명령을 daemon 연결로 보내는 `ShellCommandRunner`(TS `execGit`의 `getRemoteHostConnection` 분기).
//!
//! 호스트에 daemon 연결이 있으면 그 연결로 실행하고, daemon을 쓸 수 없는 호스트(신원 미설정, 미지원 플랫폼)는
//! 감싼 실행기(시스템 ssh)로 보낸다. 로컬 명령은 늘 감싼 실행기가 맡는다. daemon으로 보내는 명령도 ssh 경로와 같은
//! 호스트 채널 줄(background 몫)을 서고, 시간 초과 문구도 같다(TS `runLimitedRemoteCommand`, `normalizeSSHExecError`).

use std::sync::Arc;

use kanvibe_process::exec::system::SystemShellCommandRunner;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};

use crate::connection::HostExecuteOptions;
use crate::pool::RemoteHostConnectionPool;

pub struct DaemonRoutedCommandRunner {
    /// daemon이 없는 호스트와 로컬 명령을 맡고, daemon 명령의 채널 줄도 이 실행기의 것을 함께 쓴다
    fallback: Arc<SystemShellCommandRunner>,
    pool: Arc<RemoteHostConnectionPool>,
}

impl DaemonRoutedCommandRunner {
    pub fn new(
        fallback: Arc<SystemShellCommandRunner>,
        pool: Arc<RemoteHostConnectionPool>,
    ) -> Self {
        Self { fallback, pool }
    }
}

impl ShellCommandRunner for DaemonRoutedCommandRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        let Some(ssh_host) = ssh_host.filter(|host| !host.is_empty()) else {
            return self.fallback.exec_git(command, ssh_host, options);
        };
        let Some(connection) = self.pool.connection(ssh_host).map_err(ExecGitError::new)? else {
            return self.fallback.exec_git(command, Some(ssh_host), options);
        };
        self.fallback
            .remote()
            .exec_through(ssh_host, options, |admitted| {
                let execute_options = HostExecuteOptions {
                    timeout: admitted.timeout,
                    max_buffer_bytes: admitted.max_buffer_bytes,
                    remote_stdin: admitted.remote_stdin.clone(),
                    remote_stdin_responder: admitted.remote_stdin_responder.clone(),
                };
                connection.execute(command, &execute_options)
            })
    }
}
