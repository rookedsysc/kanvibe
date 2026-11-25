//! `gh` 설치 여부 확인과 설치(TS `githubCliDependency.ts`, `githubCliDependencyService.ts`).
//! 확인 성공은 1분 캐시하고, 같은 대상의 동시 확인은 하나로 합친다.
//! 원격 설치가 실패하면 그 호스트를 차단해 다음 요청을 바로 실패시키고, 접속 오류는 미설치로 판단하지 않는다.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner, is_ssh_connection_error};

const GITHUB_CLI_SUCCESS_CACHE: Duration = Duration::from_millis(60_000);
const GITHUB_CLI_CHECK_COMMAND: &str = "command -v gh >/dev/null 2>&1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCliStatus {
    pub tool_name: &'static str,
    pub ssh_host: Option<String>,
    pub is_remote: bool,
    #[serde(rename = "available")]
    pub is_available: bool,
    pub blocked_reason: Option<String>,
}

/// 진행 중인 확인 하나. 먼저 시작한 호출의 결과를 뒤에 온 호출이 함께 받는다
#[derive(Default)]
struct SharedCheck {
    result: Mutex<Option<Result<bool, ExecGitError>>>,
    finished: Condvar,
}

impl SharedCheck {
    fn wait(&self) -> Result<bool, ExecGitError> {
        let mut result = self.result.lock().unwrap();
        loop {
            if let Some(result) = result.as_ref() {
                return result.clone();
            }
            result = self.finished.wait(result).unwrap();
        }
    }

    fn finish(&self, outcome: Result<bool, ExecGitError>) {
        *self.result.lock().unwrap() = Some(outcome);
        self.finished.notify_all();
    }
}

#[derive(Default)]
struct DependencyState {
    blocked_targets: HashMap<String, String>,
    available_targets: HashMap<String, Instant>,
    in_flight_checks: HashMap<String, Arc<SharedCheck>>,
}

pub struct GitHubCliDependency {
    runner: Arc<dyn ShellCommandRunner>,
    state: Mutex<DependencyState>,
}

fn remote_host(ssh_host: Option<&str>) -> Option<&str> {
    ssh_host.filter(|host| !host.is_empty())
}

fn blocked_target_key(ssh_host: Option<&str>) -> String {
    format!("{}:gh", remote_host(ssh_host).unwrap_or("local"))
}

impl GitHubCliDependency {
    pub fn new(runner: Arc<dyn ShellCommandRunner>) -> Self {
        Self {
            runner,
            state: Mutex::new(DependencyState::default()),
        }
    }

    fn is_cached_available(&self, key: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        match state.available_targets.get(key) {
            Some(checked_at) if checked_at.elapsed() <= GITHUB_CLI_SUCCESS_CACHE => true,
            Some(_) => {
                state.available_targets.remove(key);
                false
            }
            None => false,
        }
    }

    fn check_installed(&self, ssh_host: Option<&str>) -> Result<bool, ExecGitError> {
        match self.runner.exec_git(
            GITHUB_CLI_CHECK_COMMAND,
            ssh_host,
            &ExecGitOptions::default(),
        ) {
            Ok(_) => {
                let key = blocked_target_key(ssh_host);
                self.state
                    .lock()
                    .unwrap()
                    .available_targets
                    .insert(key, Instant::now());
                Ok(true)
            }
            Err(error) if remote_host(ssh_host).is_some() && is_ssh_connection_error(&error) => {
                Err(error)
            }
            Err(_) => Ok(false),
        }
    }

    fn has_github_cli(&self, ssh_host: Option<&str>) -> Result<bool, ExecGitError> {
        let key = blocked_target_key(ssh_host);
        if self.is_cached_available(&key) {
            return Ok(true);
        }
        let (check, is_owner) = {
            let mut state = self.state.lock().unwrap();
            match state.in_flight_checks.get(&key) {
                Some(check) => (Arc::clone(check), false),
                None => {
                    let check = Arc::new(SharedCheck::default());
                    state
                        .in_flight_checks
                        .insert(key.clone(), Arc::clone(&check));
                    (check, true)
                }
            }
        };
        if !is_owner {
            return check.wait();
        }
        let outcome = self.check_installed(ssh_host);
        self.state.lock().unwrap().in_flight_checks.remove(&key);
        check.finish(outcome.clone());
        outcome
    }

    pub fn status(&self, ssh_host: Option<&str>) -> Result<GitHubCliStatus, ExecGitError> {
        let blocked_reason = self
            .state
            .lock()
            .unwrap()
            .blocked_targets
            .get(&blocked_target_key(ssh_host))
            .cloned();
        let is_available = match blocked_reason {
            Some(_) => false,
            None => self.has_github_cli(ssh_host)?,
        };
        Ok(GitHubCliStatus {
            tool_name: "gh",
            ssh_host: ssh_host.map(String::from),
            is_remote: remote_host(ssh_host).is_some(),
            is_available,
            blocked_reason,
        })
    }

    pub fn install(&self, ssh_host: Option<&str>) -> Result<(), ExecGitError> {
        let key = blocked_target_key(ssh_host);
        {
            let mut state = self.state.lock().unwrap();
            state.blocked_targets.remove(&key);
            state.available_targets.remove(&key);
            state.in_flight_checks.remove(&key);
        }
        let options = ExecGitOptions::default();
        let installed = self
            .runner
            .exec_git(&build_install_command(), ssh_host, &options)
            .and_then(|_| {
                self.runner
                    .exec_git(GITHUB_CLI_CHECK_COMMAND, ssh_host, &options)
            });
        let error = match installed {
            Ok(_) => {
                self.state
                    .lock()
                    .unwrap()
                    .available_targets
                    .insert(key, Instant::now());
                return Ok(());
            }
            Err(error) => error,
        };
        let Some(host) = remote_host(ssh_host) else {
            return Err(ExecGitError::new(format!(
                "로컬 환경에서 gh 설치를 완료하지 못했습니다. {}",
                error.message
            )));
        };
        if is_ssh_connection_error(&error) {
            return Err(error);
        }
        let reason = format!(
            "{host} 호스트에서 gh 설치를 완료하지 못해 원격 접근을 차단했습니다. {}",
            error.message
        );
        self.state
            .lock()
            .unwrap()
            .blocked_targets
            .insert(key, reason.clone());
        Err(ExecGitError::new(reason))
    }

    /// 설치한 뒤 다시 상태를 읽는다(TS `githubCliDependencyService.installGitHubCli`)
    pub fn install_and_read_status(
        &self,
        ssh_host: Option<&str>,
    ) -> Result<GitHubCliStatus, ExecGitError> {
        self.install(ssh_host)?;
        self.status(ssh_host)
    }
}

fn build_install_command() -> String {
    [
        "if command -v gh >/dev/null 2>&1; then exit 0; fi",
        "run_install() {",
        "  if [ \"$(id -u)\" -eq 0 ]; then",
        "    sh -lc \"$1\"",
        "    return $?",
        "  fi",
        "  if command -v sudo >/dev/null 2>&1; then",
        "    sudo -n sh -lc \"$1\"",
        "    return $?",
        "  fi",
        "  return 1",
        "}",
        "if command -v brew >/dev/null 2>&1; then brew install gh && exit 0; fi",
        r#"if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then run_install "type -p wget >/dev/null || (apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y wget) && mkdir -p -m 755 /etc/apt/keyrings && out=\$(mktemp) && wget -nv -O\$out https://cli.github.com/packages/githubcli-archive-keyring.gpg && cat \$out > /etc/apt/keyrings/githubcli-archive-keyring.gpg && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg && mkdir -p -m 755 /etc/apt/sources.list.d && echo \"deb [arch=\$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" > /etc/apt/sources.list.d/github-cli.list && apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y gh" && exit 0; fi"#,
        r#"if command -v dnf >/dev/null 2>&1; then run_install "dnf install -y dnf5-plugins || dnf install -y \"dnf-command(config-manager)\" || true; dnf config-manager addrepo --from-repofile=https://cli.github.com/packages/rpm/gh-cli.repo || dnf config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo; dnf install -y gh --repo gh-cli || dnf install -y gh" && exit 0; fi"#,
        r#"if command -v yum >/dev/null 2>&1; then run_install "type -p yum-config-manager >/dev/null || yum install -y yum-utils; yum-config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo && yum install -y gh" && exit 0; fi"#,
        r#"if command -v zypper >/dev/null 2>&1; then run_install "zypper --non-interactive addrepo https://cli.github.com/packages/rpm/gh-cli.repo || true; zypper --non-interactive ref && zypper --non-interactive install gh" && exit 0; fi"#,
        r#"if command -v pacman >/dev/null 2>&1; then run_install "pacman -Sy --noconfirm github-cli" && exit 0; fi"#,
        r#"if command -v apk >/dev/null 2>&1; then run_install "apk add github-cli" && exit 0; fi"#,
        "echo \"gh 설치에 실패했습니다.\" >&2",
        "exit 1",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::exec::testing::ScriptedCommandRunner;

    /// 호출마다 차례로 결과를 돌려주고, 다 쓰면 빈 출력으로 성공한다
    fn sequenced_runner(results: Vec<Result<String, ExecGitError>>) -> Arc<ScriptedCommandRunner> {
        let queue = Mutex::new(VecDeque::from(results));
        Arc::new(ScriptedCommandRunner::new(move |_| {
            queue
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(String::new()))
        }))
    }

    // parity: TS-lib-githubCliDependency-82e1af
    #[test]
    fn connection_errors_are_not_treated_as_missing_gh() {
        for message in [
            "Permission denied (publickey)",
            "Host key verification failed",
        ] {
            let runner = sequenced_runner(vec![Err(ExecGitError::new(message)), Ok(String::new())]);
            let dependency = GitHubCliDependency::new(runner.clone());

            let failure = dependency.status(Some("remote-a")).unwrap_err();
            assert!(failure.message.contains(message));
            assert_eq!(runner.calls().len(), 1);

            let status = dependency.status(Some("remote-a")).unwrap();
            assert!(status.is_available);
            assert_eq!(status.blocked_reason, None);
        }
    }

    // parity: TS-lib-githubCliDependency-56c874
    #[test]
    fn local_environment_status_is_checked_too() {
        let runner = sequenced_runner(vec![]);
        let dependency = GitHubCliDependency::new(runner.clone());

        let status = dependency.status(None).unwrap();

        assert!(status.is_available);
        assert!(!status.is_remote);
        assert_eq!(status.tool_name, "gh");
        assert_eq!(runner.commands(), [GITHUB_CLI_CHECK_COMMAND]);
        assert_eq!(runner.calls()[0].ssh_host, None);
    }

    // parity: TS-lib-githubCliDependency-e81726
    #[test]
    fn remote_install_verifies_gh_again() {
        let runner = sequenced_runner(vec![]);
        let dependency = GitHubCliDependency::new(runner.clone());

        dependency.install(Some("remote-a")).unwrap();

        let commands = runner.commands();
        assert_eq!(commands.len(), 2);
        assert!(commands[0].contains("brew install gh"));
        assert!(commands[0].contains("github-cli.list"));
        assert!(commands[0].contains("pacman -Sy --noconfirm github-cli"));
        assert_eq!(commands[1], GITHUB_CLI_CHECK_COMMAND);
        assert_eq!(runner.calls()[1].ssh_host.as_deref(), Some("remote-a"));
    }

    // parity: TS-lib-githubCliDependency-f5c36e
    #[test]
    fn failed_remote_install_blocks_the_host_for_following_requests() {
        let runner = sequenced_runner(vec![Err(ExecGitError::new("sudo unavailable"))]);
        let dependency = GitHubCliDependency::new(runner.clone());

        let failure = dependency.install(Some("remote-b")).unwrap_err();
        assert!(
            regex::Regex::new("remote-b 호스트.*gh 설치")
                .unwrap()
                .is_match(&failure.message)
        );

        let status = dependency.status(Some("remote-b")).unwrap();
        assert!(!status.is_available);
        assert!(status.blocked_reason.unwrap().contains("원격 접근을 차단"));
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn install_command_matches_the_ts_apt_branch() {
        let command = build_install_command();

        assert!(command.contains("out=\\$(mktemp) && wget -nv -O\\$out https://"));
        assert!(command.contains("echo \\\"deb [arch=\\$(dpkg --print-architecture)"));
    }
}
