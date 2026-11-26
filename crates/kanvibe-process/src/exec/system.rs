//! 실제 프로세스를 띄우는 `ShellCommandRunner`. 로컬은 좁힌 셸 환경으로 `sh -c`를, 원격은 시스템 `ssh`를 쓴다.
//! 원격 host daemon(TS `getRemoteHostConnection`) 경로는 원격 daemon을 맡는 단계에서 붙이고, 지금은 순수 ssh 경로만 쓴다.

use super::local::exec_local;
use super::remote::{RemoteSshExecutor, RemoteSshSettings};
use super::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use crate::shell_environment::{
    ShellEnvironment, ShellPlatform, create_local_shell_environment, current_user_home_directory,
};

pub struct SystemShellCommandRunner {
    local_environment: ShellEnvironment,
    remote: RemoteSshExecutor,
}

impl SystemShellCommandRunner {
    /// `local_environment`는 `create_local_shell_environment`로 만든 값이어야 한다.
    /// `home_directory`는 `~/.ssh/config`와 `~/.kanvibe` 제어 소켓의 기준이다
    pub fn new(
        local_environment: ShellEnvironment,
        home_directory: &str,
        settings: RemoteSshSettings,
    ) -> Self {
        let remote = RemoteSshExecutor::new(home_directory, local_environment.clone(), settings);
        Self {
            local_environment,
            remote,
        }
    }

    /// 지금 프로세스의 환경을 한 번 읽어 만든다. 환경은 읽기만 하고 바꾸지 않는다
    pub fn for_current_process() -> Self {
        let parent_environment: ShellEnvironment = std::env::vars().collect();
        let home_directory = current_user_home_directory();
        let local_environment = create_local_shell_environment(
            &parent_environment,
            ShellPlatform::current(),
            &home_directory,
        );
        let settings = RemoteSshSettings::from_environment(&parent_environment);
        Self::new(local_environment, &home_directory, settings)
    }

    pub fn remote(&self) -> &RemoteSshExecutor {
        &self.remote
    }
}

impl ShellCommandRunner for SystemShellCommandRunner {
    fn exec_git(
        &self,
        command: &str,
        ssh_host: Option<&str>,
        options: &ExecGitOptions,
    ) -> Result<String, ExecGitError> {
        match ssh_host.filter(|host| !host.is_empty()) {
            None => exec_local(command, &self.local_environment, options),
            Some(host) => self.remote.exec(command, host, options),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_commands_include_cli_paths_missing_from_macos_app_environments() {
        let parent_environment: ShellEnvironment = [
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("HOME".to_string(), "/home/local-user".to_string()),
        ]
        .into();
        let local_environment =
            create_local_shell_environment(&parent_environment, ShellPlatform::Mac, "/unused");
        let runner = SystemShellCommandRunner::new(
            local_environment,
            "/home/local-user",
            RemoteSshSettings::default(),
        );

        let path = runner
            .exec_git("printf '%s' \"$PATH\"", None, &ExecGitOptions::default())
            .unwrap();

        assert!(path.contains("/opt/homebrew/bin"));
        assert!(path.contains(".opencode/bin"));
    }
}
