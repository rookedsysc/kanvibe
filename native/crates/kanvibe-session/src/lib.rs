use std::collections::{BTreeMap, BTreeSet};

use kanvibe_core::SessionType;

pub const CRATE_PURPOSE: &str = "Persistent terminal session dependency policy";
pub const SESSION_DEPENDENCY_SUCCESS_CACHE_MS: u64 = 60_000;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SessionDependencyStatus {
    pub session_type: SessionType,
    pub tool_name: &'static str,
    pub ssh_host: Option<String>,
    pub is_remote: bool,
    pub available: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Default)]
pub struct SessionDependencyRegistry {
    blocked_targets: BTreeMap<String, String>,
    available_targets: BTreeSet<String>,
}

impl SessionDependencyRegistry {
    pub fn status(
        &self,
        session_type: SessionType,
        ssh_host: Option<&str>,
    ) -> SessionDependencyStatus {
        let tool_name = session_dependency_tool_name(session_type);
        let key = session_dependency_target_key(tool_name, ssh_host);
        let blocked_reason = self.blocked_targets.get(&key).cloned();

        SessionDependencyStatus {
            session_type,
            tool_name,
            ssh_host: ssh_host.map(ToOwned::to_owned),
            is_remote: ssh_host.is_some(),
            available: blocked_reason.is_none() && self.available_targets.contains(&key),
            blocked_reason,
        }
    }

    pub fn remember_available(&mut self, session_type: SessionType, ssh_host: Option<&str>) {
        let key =
            session_dependency_target_key(session_dependency_tool_name(session_type), ssh_host);
        self.blocked_targets.remove(&key);
        self.available_targets.insert(key);
    }

    pub fn remember_install_failure(
        &mut self,
        session_type: SessionType,
        ssh_host: Option<&str>,
        error: &str,
    ) -> Option<String> {
        let tool_name = session_dependency_tool_name(session_type);
        let key = session_dependency_target_key(tool_name, ssh_host);
        self.available_targets.remove(&key);

        ssh_host.map(|ssh_host| {
            let reason = format!(
                "{ssh_host} host could not complete {tool_name} installation, so remote access is blocked. {error}"
            );
            self.blocked_targets.insert(key, reason.clone());
            reason
        })
    }

    pub fn blocked_remote_host_reason(&self, ssh_host: &str) -> Option<&str> {
        self.blocked_targets
            .get(&session_dependency_target_key("tmux", Some(ssh_host)))
            .or_else(|| {
                self.blocked_targets
                    .get(&session_dependency_target_key("zellij", Some(ssh_host)))
            })
            .map(String::as_str)
    }
}

pub const fn session_dependency_tool_name(session_type: SessionType) -> &'static str {
    match session_type {
        SessionType::Tmux => "tmux",
        SessionType::Zellij => "zellij",
    }
}

pub fn session_dependency_target_key(tool_name: &str, ssh_host: Option<&str>) -> String {
    format!("{}:{tool_name}", ssh_host.unwrap_or("local"))
}

pub fn build_session_dependency_check_command(session_type: SessionType) -> String {
    let tool_name = session_dependency_tool_name(session_type);
    format!("command -v {tool_name} >/dev/null 2>&1")
}

pub fn build_session_dependency_install_command(session_type: SessionType) -> String {
    let tool_name = session_dependency_tool_name(session_type);
    let install_commands = if session_type == SessionType::Zellij {
        vec![
            "cargo install --locked zellij",
            "brew install zellij",
            "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y zellij",
            "dnf install -y zellij",
            "yum install -y zellij",
            "pacman -Sy --noconfirm zellij",
            "zypper --non-interactive install zellij",
            "apk add zellij",
        ]
    } else {
        vec![
            "brew install tmux",
            "apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y tmux",
            "dnf install -y tmux",
            "yum install -y tmux",
            "pacman -Sy --noconfirm tmux",
            "zypper --non-interactive install tmux",
            "apk add tmux",
        ]
    };
    let mut lines = vec![
        format!("if command -v {tool_name} >/dev/null 2>&1; then exit 0; fi"),
        "run_install() {".to_owned(),
        "  if [ \"$(id -u)\" -eq 0 ]; then".to_owned(),
        "    sh -lc \"$1\"".to_owned(),
        "    return $?".to_owned(),
        "  fi".to_owned(),
        "  if command -v sudo >/dev/null 2>&1; then".to_owned(),
        "    sudo -n sh -lc \"$1\"".to_owned(),
        "    return $?".to_owned(),
        "  fi".to_owned(),
        "  return 1".to_owned(),
        "}".to_owned(),
    ];
    lines.extend(install_commands.iter().map(|command| {
        let executable = command.split(' ').next().unwrap_or_default();
        match executable {
            "cargo" | "brew" => {
                format!("if command -v {executable} >/dev/null 2>&1; then {command}; fi")
            }
            _ => format!(
                "if command -v {executable} >/dev/null 2>&1; then run_install '{command}' && exit 0; fi"
            ),
        }
    }));
    lines.extend([
        format!("if ! command -v {tool_name} >/dev/null 2>&1; then"),
        format!("  echo \"{tool_name} installation failed.\" >&2"),
        "  exit 1".to_owned(),
        "fi".to_owned(),
    ]);
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_policy_matches_supported_tools() {
        assert!(
            build_session_dependency_install_command(SessionType::Zellij)
                .contains("cargo install --locked zellij")
        );
        assert_eq!(
            build_session_dependency_check_command(SessionType::Tmux),
            "command -v tmux >/dev/null 2>&1"
        );
    }
}
