//! 원격 명령을 daemon 연결로 보내는 `ShellCommandRunner`(TS `execGit`의 `getRemoteHostConnection` 분기).
//!
//! 호스트에 daemon 연결이 있으면 그 연결로 실행하고, daemon을 쓸 수 없는 호스트(신원 미설정, 미지원 플랫폼)는
//! 감싼 실행기(시스템 ssh)로 보낸다. 로컬 명령은 늘 감싼 실행기가 맡는다. TS의 background 큐 제한은 옮기지 않았다.

use std::sync::Arc;

use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};

use crate::connection::HostExecuteOptions;
use crate::pool::RemoteHostConnectionPool;

pub struct DaemonRoutedCommandRunner {
    fallback: Arc<dyn ShellCommandRunner>,
    pool: Arc<RemoteHostConnectionPool>,
}

impl DaemonRoutedCommandRunner {
    pub fn new(fallback: Arc<dyn ShellCommandRunner>, pool: Arc<RemoteHostConnectionPool>) -> Self {
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
        let Some(ssh_host) = ssh_host else {
            return self.fallback.exec_git(command, None, options);
        };
        let Some(connection) = self.pool.connection(ssh_host).map_err(ExecGitError::new)? else {
            return self.fallback.exec_git(command, Some(ssh_host), options);
        };
        let execute_options = HostExecuteOptions {
            timeout: options.timeout,
            max_buffer_bytes: options.max_buffer_bytes,
            remote_stdin: options.remote_stdin.clone(),
            remote_stdin_responder: options.remote_stdin_responder.clone(),
        };
        connection.execute(command, &execute_options)
    }
}
