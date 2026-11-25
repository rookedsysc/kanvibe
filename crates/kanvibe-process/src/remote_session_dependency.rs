//! 로컬·원격에 tmux, zellij, herdr가 있는지 확인하고 설치한다(TS `remoteSessionDependency.ts`, `sessionDependencyService.ts`).
//! 확인 성공은 1분 캐시하고, 원격 설치가 실패하면 그 호스트를 차단한다. 접속 오류는 의존성 부재로 판단하지 않는다.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::task::SessionType;
use serde::Serialize;

use crate::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner, is_ssh_connection_error};

const SESSION_DEPENDENCY_SUCCESS_CACHE: Duration = Duration::from_millis(60_000);
const SESSION_TOOL_NAMES: [&str; 3] = ["tmux", "zellij", "herdr"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDependencyStatus {
    pub session_type: SessionType,
    /// 설치가 필요 없는 세션 종류는 `None`이다
    pub tool_name: Option<&'static str>,
    pub ssh_host: Option<String>,
    pub is_remote: bool,
    #[serde(rename = "available")]
    pub is_available: bool,
    pub blocked_reason: Option<String>,
}

/// terminal 세션은 KanVibe가 셸을 직접 띄우므로 설치할 도구가 없다
fn tool_name(session_type: SessionType) -> Option<&'static str> {
    match session_type {
        SessionType::Tmux => Some("tmux"),
        SessionType::Zellij => Some("zellij"),
        SessionType::Herdr => Some("herdr"),
        SessionType::Terminal => None,
    }
}

fn remote_host(ssh_host: Option<&str>) -> Option<&str> {
    ssh_host.filter(|host| !host.is_empty())
}

fn blocked_target_key(tool_name: &str, ssh_host: Option<&str>) -> String {
    format!("{}:{tool_name}", remote_host(ssh_host).unwrap_or("local"))
}

#[derive(Default)]
struct DependencyState {
    blocked_targets: HashMap<String, String>,
    /// 키에서 캐시 만료 시각으로
    available_targets: HashMap<String, Instant>,
}

pub struct SessionDependencies {
    runner: Arc<dyn ShellCommandRunner>,
    state: Mutex<DependencyState>,
}

impl SessionDependencies {
    pub fn new(runner: Arc<dyn ShellCommandRunner>) -> Self {
        Self {
            runner,
            state: Mutex::new(DependencyState::default()),
        }
    }

    fn is_cached_available(&self, key: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        match state.available_targets.get(key) {
            Some(expires_at) if Instant::now() < *expires_at => true,
            Some(_) => {
                state.available_targets.remove(key);
                false
            }
            None => false,
        }
    }

    fn remember_available(&self, key: String) {
        let expires_at = Instant::now() + SESSION_DEPENDENCY_SUCCESS_CACHE;
        self.state
            .lock()
            .unwrap()
            .available_targets
            .insert(key, expires_at);
    }

    /// 원격 호스트가 어느 도구 설치 실패로 차단됐는지
    pub fn blocked_remote_host_reason(&self, ssh_host: &str) -> Option<String> {
        let state = self.state.lock().unwrap();
        SESSION_TOOL_NAMES.iter().find_map(|tool_name| {
            state
                .blocked_targets
                .get(&blocked_target_key(tool_name, Some(ssh_host)))
                .cloned()
        })
    }

    fn has_session_dependency(
        &self,
        tool_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<bool, ExecGitError> {
        let key = blocked_target_key(tool_name, ssh_host);
        if self.is_cached_available(&key) {
            return Ok(true);
        }
        match self.runner.exec_git(
            &build_dependency_check_command(tool_name),
            ssh_host,
            &ExecGitOptions::default(),
        ) {
            Ok(_) => {
                self.remember_available(key);
                Ok(true)
            }
            Err(error) => {
                self.state.lock().unwrap().available_targets.remove(&key);
                if remote_host(ssh_host).is_some() && is_ssh_connection_error(&error) {
                    return Err(error);
                }
                Ok(false)
            }
        }
    }

    pub fn status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> Result<SessionDependencyStatus, ExecGitError> {
        let tool_name = tool_name(session_type);
        let mut status = SessionDependencyStatus {
            session_type,
            tool_name,
            ssh_host: ssh_host.map(String::from),
            is_remote: remote_host(ssh_host).is_some(),
            is_available: true,
            blocked_reason: None,
        };
        let Some(tool_name) = tool_name else {
            return Ok(status);
        };
        let blocked_reason = remote_host(ssh_host).and_then(|_| {
            let state = self.state.lock().unwrap();
            state
                .blocked_targets
                .get(&blocked_target_key(tool_name, ssh_host))
                .cloned()
        });
        status.is_available = match blocked_reason {
            Some(_) => false,
            None => self.has_session_dependency(tool_name, ssh_host)?,
        };
        status.blocked_reason = blocked_reason;
        Ok(status)
    }

    pub fn install(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> Result<(), ExecGitError> {
        let Some(tool_name) = tool_name(session_type) else {
            return Ok(());
        };
        let key = blocked_target_key(tool_name, ssh_host);
        {
            let mut state = self.state.lock().unwrap();
            state.blocked_targets.remove(&key);
            state.available_targets.remove(&key);
        }
        let options = ExecGitOptions::default();
        let installed = self
            .runner
            .exec_git(&build_install_command(tool_name), ssh_host, &options)
            .and_then(|_| {
                self.runner.exec_git(
                    &build_dependency_check_command(tool_name),
                    ssh_host,
                    &options,
                )
            });
        let error = match installed {
            Ok(_) => {
                self.remember_available(key);
                return Ok(());
            }
            Err(error) => error,
        };
        Err(self.record_install_failure(tool_name, ssh_host, key, error))
    }

    /// 원격 설치 실패는 호스트를 차단하고, 접속 오류는 차단하지 않고 그대로 돌려준다
    fn record_install_failure(
        &self,
        tool_name: &str,
        ssh_host: Option<&str>,
        key: String,
        error: ExecGitError,
    ) -> ExecGitError {
        self.state.lock().unwrap().available_targets.remove(&key);
        let Some(host) = remote_host(ssh_host) else {
            return ExecGitError::new(format!(
                "로컬 환경에서 {tool_name} 설치를 완료하지 못했습니다. {}",
                error.message
            ));
        };
        if is_ssh_connection_error(&error) {
            return error;
        }
        let reason = format!(
            "{host} 호스트에서 {tool_name} 설치를 완료하지 못해 원격 접근을 차단했습니다. {}",
            error.message
        );
        self.state
            .lock()
            .unwrap()
            .blocked_targets
            .insert(key, reason.clone());
        ExecGitError::new(reason)
    }

    pub fn ensure(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> Result<(), ExecGitError> {
        let status = self.status(session_type, ssh_host)?;
        if status.is_available {
            return Ok(());
        }
        if let Some(blocked_reason) = status.blocked_reason {
            return Err(ExecGitError::new(blocked_reason));
        }
        self.install(session_type, ssh_host)
    }

    /// 원격 세션일 때만 의존성을 확인하고 없으면 설치한다
    pub fn ensure_remote(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> Result<(), ExecGitError> {
        if remote_host(ssh_host).is_none() {
            return Ok(());
        }
        self.ensure(session_type, ssh_host)
    }

    /// 설치한 뒤 다시 상태를 읽는다(TS `sessionDependencyService.installSessionDependency`)
    pub fn install_and_read_status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> Result<SessionDependencyStatus, ExecGitError> {
        self.install(session_type, ssh_host)?;
        self.status(session_type, ssh_host)
    }
}

/// 원격 Herdr 서버 교체 확인은 앱이 실행되는 동안 서버마다 한 번만 묻는다. 창이 여러 개여도 처음 요청한 창 하나만 묻는다
#[derive(Default)]
pub struct HerdrServerHandoffPrompts {
    claimed: Mutex<HashSet<String>>,
}

impl HerdrServerHandoffPrompts {
    pub fn claim(&self, ssh_host: &str, herdr_session: &str) -> bool {
        let prompt_key = serde_json::json!([ssh_host, herdr_session]).to_string();
        self.claimed.lock().unwrap().insert(prompt_key)
    }
}

fn build_dependency_check_command(tool_name: &str) -> String {
    if tool_name == "herdr" {
        return "command -v herdr >/dev/null 2>&1 || test -x \"$HOME/.local/bin/herdr\""
            .to_string();
    }
    format!("command -v {tool_name} >/dev/null 2>&1")
}

fn build_herdr_install_command() -> String {
    let check_command = build_dependency_check_command("herdr");
    [
        format!("{check_command} && exit 0"),
        "kanvibe_herdr_installer=$(mktemp) || exit 1".to_string(),
        "trap 'rm -f \"$kanvibe_herdr_installer\"' EXIT".to_string(),
        "curl -fsSL https://herdr.dev/install.sh -o \"$kanvibe_herdr_installer\" || exit 1"
            .to_string(),
        "HERDR_INSTALL_DIR=\"$HOME/.local/bin\" sh \"$kanvibe_herdr_installer\" || exit 1"
            .to_string(),
        check_command,
    ]
    .join("\n")
}

fn package_install_commands(tool_name: &str) -> Vec<&'static str> {
    if tool_name == "zellij" {
        return vec![
            "cargo install --locked zellij",
            "brew install zellij",
            "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y zellij",
            "dnf install -y zellij",
            "yum install -y zellij",
            "pacman -Sy --noconfirm zellij",
            "zypper --non-interactive install zellij",
            "apk add zellij",
        ];
    }
    vec![
        "brew install tmux",
        "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y tmux",
        "dnf install -y tmux",
        "yum install -y tmux",
        "pacman -Sy --noconfirm tmux",
        "zypper --non-interactive install tmux",
        "apk add tmux",
    ]
}

fn build_package_manager_branch(command: &str) -> String {
    let executable = command.split(' ').next().unwrap_or_default();
    match executable {
        "cargo" | "brew" => {
            format!("if command -v {executable} >/dev/null 2>&1; then {command}; fi")
        }
        _ => format!(
            "if command -v {executable} >/dev/null 2>&1; then run_install '{command}' && exit 0; fi"
        ),
    }
}

fn build_install_command(tool_name: &str) -> String {
    if tool_name == "herdr" {
        return build_herdr_install_command();
    }
    let mut lines = vec![
        format!("if command -v {tool_name} >/dev/null 2>&1; then exit 0; fi"),
        "run_install() {".to_string(),
        "  if [ \"$(id -u)\" -eq 0 ]; then".to_string(),
        "    sh -lc \"$1\"".to_string(),
        "    return $? ".to_string(),
        "  fi".to_string(),
        "  if command -v sudo >/dev/null 2>&1; then".to_string(),
        "    sudo -n sh -lc \"$1\"".to_string(),
        "    return $? ".to_string(),
        "  fi".to_string(),
        "  return 1".to_string(),
        "}".to_string(),
    ];
    lines.extend(
        package_install_commands(tool_name)
            .into_iter()
            .map(build_package_manager_branch),
    );
    lines.extend([
        format!("if ! command -v {tool_name} >/dev/null 2>&1; then"),
        format!("  echo \"{tool_name} 설치에 실패했습니다.\" >&2"),
        "  exit 1".to_string(),
        "fi".to_string(),
    ]);
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::exec::testing::ScriptedCommandRunner;

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

    fn missing() -> Result<String, ExecGitError> {
        Err(ExecGitError::new("missing"))
    }

    // parity: TS-lib-remoteSessionDependency-a8595c
    #[test]
    fn local_sessions_skip_remote_install_checks() {
        let runner = sequenced_runner(vec![]);

        SessionDependencies::new(runner.clone())
            .ensure_remote(SessionType::Tmux, None)
            .unwrap();

        assert!(runner.calls().is_empty());
    }

    // parity: TS-lib-remoteSessionDependency-1c1dbe
    #[test]
    fn local_dependency_status_is_checked_too() {
        let runner = sequenced_runner(vec![]);

        let status = SessionDependencies::new(runner.clone())
            .status(SessionType::Tmux, None)
            .unwrap();

        assert!(status.is_available);
        assert!(!status.is_remote);
        assert_eq!(status.tool_name, Some("tmux"));
        assert_eq!(runner.commands(), ["command -v tmux >/dev/null 2>&1"]);
        assert_eq!(runner.calls()[0].ssh_host, None);
    }

    // parity: TS-lib-remoteSessionDependency-607f4a
    #[test]
    fn local_environment_can_install_automatically() {
        let runner = sequenced_runner(vec![missing()]);

        SessionDependencies::new(runner.clone())
            .ensure(SessionType::Zellij, None)
            .unwrap();

        assert!(runner.commands()[1].contains("cargo install --locked zellij"));
    }

    // parity: TS-lib-remoteSessionDependency-ab43eb
    #[test]
    fn existing_remote_tool_is_not_reinstalled() {
        let runner = sequenced_runner(vec![]);

        SessionDependencies::new(runner.clone())
            .ensure_remote(SessionType::Tmux, Some("remote-a"))
            .unwrap();

        assert_eq!(runner.commands(), ["command -v tmux >/dev/null 2>&1"]);
        assert_eq!(runner.calls()[0].ssh_host.as_deref(), Some("remote-a"));
    }

    // parity: TS-lib-remoteSessionDependency-ae6934
    #[test]
    fn successful_remote_checks_are_reused_for_the_same_host_and_tool() {
        let runner = sequenced_runner(vec![]);
        let dependencies = SessionDependencies::new(runner.clone());

        dependencies
            .status(SessionType::Tmux, Some("remote-a"))
            .unwrap();
        dependencies
            .ensure_remote(SessionType::Tmux, Some("remote-a"))
            .unwrap();

        assert_eq!(runner.calls().len(), 1);
    }

    // parity: TS-lib-remoteSessionDependency-15075a
    #[test]
    fn connection_errors_neither_mean_missing_dependency_nor_block_the_host() {
        for message in [
            "Connection reset by 100.73.171.123 port 22",
            "Permission denied (publickey)",
            "Host key verification failed",
        ] {
            let failure = Err(ExecGitError::new(format!(
                "remote-a 원격 명령 실패: {message}"
            )));
            let runner = sequenced_runner(vec![failure]);
            let dependencies = SessionDependencies::new(runner.clone());

            let error = dependencies
                .ensure_remote(SessionType::Tmux, Some("remote-a"))
                .unwrap_err();
            assert!(error.message.contains(message));
            assert_eq!(runner.commands(), ["command -v tmux >/dev/null 2>&1"]);

            dependencies
                .ensure_remote(SessionType::Tmux, Some("remote-a"))
                .unwrap();
            assert_eq!(runner.calls().len(), 2);
        }
    }

    // parity: TS-lib-remoteSessionDependency-f22e35
    #[test]
    fn missing_remote_tool_is_installed_and_checked_again() {
        let runner = sequenced_runner(vec![missing()]);

        SessionDependencies::new(runner.clone())
            .ensure_remote(SessionType::Zellij, Some("remote-b"))
            .unwrap();

        let commands = runner.commands();
        assert_eq!(commands.len(), 3);
        assert!(commands[1].contains("cargo install --locked zellij"));
        assert!(commands[1].contains("run_install() {"));
        assert!(!commands[1].contains("run_install() {;"));
        assert_eq!(commands[2], "command -v zellij >/dev/null 2>&1");
        assert_eq!(runner.calls()[2].ssh_host.as_deref(), Some("remote-b"));
    }

    // parity: TS-lib-remoteSessionDependency-cc9fcc
    #[test]
    fn ssh_failures_during_install_do_not_block_the_host() {
        let closed = Err(ExecGitError::new(
            "remote-b 원격 명령 실패: Connection closed by 100.73.171.123 port 22",
        ));
        let runner = sequenced_runner(vec![missing(), closed]);
        let dependencies = SessionDependencies::new(runner.clone());

        let error = dependencies
            .ensure_remote(SessionType::Tmux, Some("remote-b"))
            .unwrap_err();
        assert!(error.message.contains("Connection closed"));

        dependencies
            .ensure_remote(SessionType::Tmux, Some("remote-b"))
            .unwrap();
        assert_eq!(runner.commands()[2], "command -v tmux >/dev/null 2>&1");
    }

    // parity: TS-lib-remoteSessionDependency-e5890c
    #[test]
    fn failed_install_blocks_the_host_for_following_requests() {
        let runner = sequenced_runner(vec![missing(), Err(ExecGitError::new("sudo unavailable"))]);
        let dependencies = SessionDependencies::new(runner.clone());

        let first = dependencies
            .ensure_remote(SessionType::Tmux, Some("remote-c"))
            .unwrap_err();
        assert!(
            regex::Regex::new("remote-c 호스트.*tmux 설치")
                .unwrap()
                .is_match(&first.message)
        );

        let second = dependencies
            .ensure_remote(SessionType::Tmux, Some("remote-c"))
            .unwrap_err();
        assert!(second.message.contains("원격 접근을 차단"));
        assert_eq!(runner.calls().len(), 2);
        assert!(
            dependencies
                .blocked_remote_host_reason("remote-c")
                .is_some()
        );
    }

    // parity: TS-lib-remoteSessionDependency-0e33f5
    #[test]
    fn missing_herdr_is_detected_and_installed_with_the_official_installer() {
        for ssh_host in [None, Some("remote-herdr")] {
            let runner = sequenced_runner(vec![missing()]);
            let dependencies = SessionDependencies::new(runner.clone());

            let status = dependencies.status(SessionType::Herdr, ssh_host).unwrap();
            assert_eq!(status.tool_name, Some("herdr"));
            assert!(!status.is_available);
            dependencies.install(SessionType::Herdr, ssh_host).unwrap();

            let commands = runner.commands();
            assert!(commands[1].contains("https://herdr.dev/install.sh"));
            assert!(commands[2].contains(".local/bin/herdr"));
        }
    }

    // parity: TS-main-sessionDependencyService-ec5e94
    #[test]
    fn herdr_server_handoff_is_asked_once_per_server() {
        let prompts = HerdrServerHandoffPrompts::default();

        assert!(prompts.claim("dev", "kanvibe"));
        assert!(!prompts.claim("dev", "kanvibe"));
        assert!(prompts.claim("dev", "feature"));
        assert!(prompts.claim("other", "kanvibe"));
    }

    #[test]
    fn terminal_sessions_need_no_tool() {
        let runner = sequenced_runner(vec![]);

        let status = SessionDependencies::new(runner.clone())
            .status(SessionType::Terminal, Some("remote"))
            .unwrap();

        assert_eq!(status.tool_name, None);
        assert!(status.is_available);
        assert!(runner.calls().is_empty());
    }
}
