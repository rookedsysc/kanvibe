//! 터미널 계층이 쓰는 호스트 환경. 앱이 시작할 때 한 번 만들어 넘기고, 이 crate의 다른 모듈은 프로세스 환경을 직접 읽지 않는다.
//! 자식 프로세스·PTY에는 [`TerminalHostEnvironment::shell_environment`]만 넘긴다. 이 값은 `PORT`, `HOST`, `NODE_ENV`,
//! `KANVIBE_*`를 걷어 낸 로컬 셸 환경이다(CLAUDE.md 런타임 환경 규칙).

use kanvibe_process::shell_environment::{
    ShellEnvironment, ShellPlatform, create_local_shell_environment, current_user_home_directory,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalHostEnvironment {
    /// TS `process.env.HOME || "/"`에서 쓰는 홈. HOME이 없으면 OS 사용자 홈이다
    pub home_directory: String,
    /// TS `process.env.SHELL`. 로그인 셸과 기본 탭 이름을 정한다
    pub login_shell: Option<String>,
    /// 자식 프로세스에 그대로 넘길 좁힌 환경
    pub shell_environment: ShellEnvironment,
}

impl TerminalHostEnvironment {
    /// 부모 환경을 받아 만든다. 테스트는 임의의 부모 환경을 넘긴다
    pub fn from_parent_environment(
        parent_environment: &ShellEnvironment,
        platform: ShellPlatform,
        fallback_home_directory: &str,
    ) -> Self {
        let shell_environment =
            create_local_shell_environment(parent_environment, platform, fallback_home_directory);
        let home_directory = shell_environment
            .get("HOME")
            .cloned()
            .filter(|home| !home.is_empty())
            .unwrap_or_else(|| "/".to_string());
        let login_shell = parent_environment
            .get("SHELL")
            .cloned()
            .filter(|shell| !shell.is_empty());
        Self {
            home_directory,
            login_shell,
            shell_environment,
        }
    }

    /// 지금 프로세스의 환경을 읽기만 해서 만든다
    pub fn for_current_process() -> Self {
        let parent_environment: ShellEnvironment = std::env::vars().collect();
        Self::from_parent_environment(
            &parent_environment,
            ShellPlatform::current(),
            &current_user_home_directory(),
        )
    }

    /// 좁힌 환경의 값. 비어 있으면 없는 것으로 본다(JS `process.env.X || ...`)
    pub fn variable(&self, key: &str) -> Option<&str> {
        self.shell_environment
            .get(key)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    /// TS `process.env.SHELL || "/bin/sh"`
    pub fn resolve_login_shell(&self) -> String {
        self.login_shell
            .clone()
            .unwrap_or_else(|| "/bin/sh".to_string())
    }

    /// tmux가 window 이름에 실행 중인 명령을 쓰는 것과 같이 기본 탭 이름은 셸 이름이다. SHELL이 없으면 `shell`이다
    pub fn default_tab_name(&self) -> String {
        match &self.login_shell {
            Some(shell) => kanvibe_domain::project::posix_basename(shell).to_string(),
            None => "shell".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent(entries: &[(&str, &str)]) -> ShellEnvironment {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn server_runtime_variables_never_reach_the_terminal_environment() {
        let environment = TerminalHostEnvironment::from_parent_environment(
            &parent(&[
                ("HOME", "/home/user"),
                ("PATH", "/usr/bin"),
                ("PORT", "9736"),
                ("HOST", "0.0.0.0"),
                ("NODE_ENV", "production"),
                ("KANVIBE_APP_DATA_DIR", "/tmp/kanvibe"),
                ("KANVIBE_QA_MODE", "1"),
                ("SHELL", "/bin/zsh"),
            ]),
            ShellPlatform::Other,
            "/fallback",
        );

        for key in [
            "PORT",
            "HOST",
            "NODE_ENV",
            "KANVIBE_APP_DATA_DIR",
            "KANVIBE_QA_MODE",
        ] {
            assert!(!environment.shell_environment.contains_key(key), "{key}");
        }
        assert_eq!(environment.variable("HOME"), Some("/home/user"));
        assert_eq!(environment.default_tab_name(), "zsh");
        assert_eq!(environment.resolve_login_shell(), "/bin/zsh");
    }

    #[test]
    fn missing_shell_falls_back_to_sh_and_shell_tab_name() {
        let environment = TerminalHostEnvironment::from_parent_environment(
            &parent(&[]),
            ShellPlatform::Other,
            "/fallback",
        );

        assert_eq!(environment.home_directory, "/fallback");
        assert_eq!(environment.resolve_login_shell(), "/bin/sh");
        assert_eq!(environment.default_tab_name(), "shell");
    }
}
