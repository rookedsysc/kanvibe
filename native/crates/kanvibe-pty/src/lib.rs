use std::collections::{BTreeMap, BTreeSet};

use kanvibe_core::{PaneCommand, PaneLayoutType, SessionType};

pub const STRIPPED_RUNTIME_ENV_KEYS: &[&str] = &["PORT", "HOST", "NODE_ENV"];
pub const INTERNAL_ENV_PREFIX: &str = "KANVIBE_";
pub const MAC_LOCAL_COMMAND_PATHS: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/opt/local/bin",
    "/opt/local/sbin",
];
pub const SESSION_DEPENDENCY_SUCCESS_CACHE_MS: u64 = 60_000;
pub const ZELLIJ_LAYOUT_FILENAME: &str = ".zellij-layout.kdl";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ShellPlatform {
    Mac,
    Linux,
}

pub fn should_strip_from_shell_env(key: &str) -> bool {
    STRIPPED_RUNTIME_ENV_KEYS.contains(&key) || key.starts_with(INTERNAL_ENV_PREFIX)
}

pub fn merge_shell_path_entries(path_values: &[&str], delimiter: char) -> String {
    let mut entries = Vec::new();

    for path_value in path_values {
        for entry in path_value
            .split(delimiter)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        }
    }

    entries.join(&delimiter.to_string())
}

pub fn create_local_shell_environment(
    input: impl IntoIterator<Item = (String, String)>,
    home_directory: &str,
    platform: ShellPlatform,
) -> BTreeMap<String, String> {
    let mut environment = BTreeMap::new();

    for (key, value) in input {
        if !should_strip_from_shell_env(&key) {
            environment.insert(key, value);
        }
    }

    let delimiter = if platform == ShellPlatform::Mac {
        ':'
    } else {
        ':'
    };
    let current_path = environment.get("PATH").cloned().unwrap_or_default();
    let extra_path = match platform {
        ShellPlatform::Mac => {
            let user_paths = [
                format!("{home_directory}/.local/bin"),
                format!("{home_directory}/.cargo/bin"),
                format!("{home_directory}/.opencode/bin"),
                format!("{home_directory}/Library/pnpm"),
                format!("{home_directory}/.bun/bin"),
            ];
            let extra_paths = MAC_LOCAL_COMMAND_PATHS
                .iter()
                .copied()
                .chain(user_paths.iter().map(String::as_str))
                .collect::<Vec<_>>();

            extra_paths.join(":")
        }
        ShellPlatform::Linux => String::new(),
    };

    environment.insert("HOME".to_owned(), home_directory.to_owned());
    environment.insert(
        "PATH".to_owned(),
        merge_shell_path_entries(&[current_path.as_str(), extra_path.as_str()], delimiter),
    );
    environment
        .entry("LANG".to_owned())
        .or_insert_with(|| "en_US.UTF-8".to_owned());
    environment
        .entry("LC_ALL".to_owned())
        .or_insert_with(|| "en_US.UTF-8".to_owned());

    environment
}

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
        let tool_name = session_dependency_tool_name(session_type);
        let key = session_dependency_target_key(tool_name, ssh_host);

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

        if let Some(ssh_host) = ssh_host {
            let reason = format!(
                "{ssh_host} host could not complete {tool_name} installation, so remote access is blocked. {error}"
            );
            self.blocked_targets.insert(key, reason.clone());
            Some(reason)
        } else {
            None
        }
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
            "cargo" => format!("if command -v cargo >/dev/null 2>&1; then {command}; fi"),
            "brew" => format!("if command -v brew >/dev/null 2>&1; then {command}; fi"),
            _ => {
                format!("if command -v {executable} >/dev/null 2>&1; then run_install '{command}' && exit 0; fi")
            }
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

pub fn quote_for_posix_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn build_tmux_pane_layout_commands(
    session_name: &str,
    layout_type: PaneLayoutType,
    panes: &[PaneCommand],
    worktree_path: &str,
) -> Vec<String> {
    let target = quote_for_posix_shell(&format!("{session_name}:0"));
    let worktree_path = quote_for_posix_shell(worktree_path);
    let pane_target =
        |position: u32| quote_for_posix_shell(&format!("{session_name}:0.{position}"));
    let mut commands = match layout_type {
        PaneLayoutType::Single => Vec::new(),
        PaneLayoutType::Horizontal2 => {
            vec![format!(
                "tmux split-window -v -t {target} -c {worktree_path}"
            )]
        }
        PaneLayoutType::Vertical2 => {
            vec![format!(
                "tmux split-window -h -t {target} -c {worktree_path}"
            )]
        }
        PaneLayoutType::LeftRightTb => vec![
            format!("tmux split-window -h -t {target} -c {worktree_path}"),
            format!(
                "tmux split-window -v -t {} -c {worktree_path}",
                pane_target(1)
            ),
        ],
        PaneLayoutType::LeftTbRight => vec![
            format!("tmux split-window -h -t {target} -c {worktree_path}"),
            format!(
                "tmux split-window -v -t {} -c {worktree_path}",
                pane_target(0)
            ),
        ],
        PaneLayoutType::Quad => vec![
            format!("tmux split-window -h -t {target} -c {worktree_path}"),
            format!(
                "tmux split-window -v -t {} -c {worktree_path}",
                pane_target(0)
            ),
            format!(
                "tmux split-window -v -t {} -c {worktree_path}",
                pane_target(2)
            ),
        ],
    };

    commands.extend(panes.iter().filter_map(|pane| {
        let command = pane.command.trim();
        (!command.is_empty()).then(|| {
            format!(
                "tmux send-keys -t {} -- {} Enter",
                pane_target(pane.position),
                quote_for_posix_shell(command)
            )
        })
    }));
    commands
}

pub fn generate_zellij_layout_kdl(
    layout_type: PaneLayoutType,
    panes: &[PaneCommand],
    worktree_path: &str,
) -> String {
    let pane_map = panes
        .iter()
        .map(|pane| (pane.position, pane.command.as_str()))
        .collect::<BTreeMap<_, _>>();
    let cwd = escape_kdl(worktree_path);
    let render_pane = |position: u32, indent: &str| -> String {
        let Some(command) = pane_map.get(&position).map(|command| command.trim()) else {
            return format!("{indent}pane cwd=\"{cwd}\"");
        };
        if command.is_empty() {
            return format!("{indent}pane cwd=\"{cwd}\"");
        }
        format!(
            "{indent}pane command=\"bash\" {{\n{indent}    args \"-c\" \"{}\"\n{indent}    cwd \"{cwd}\"\n{indent}}}",
            escape_kdl(command)
        )
    };

    match layout_type {
        PaneLayoutType::Single => [
            "layout {".to_owned(),
            render_pane(0, "    "),
            "}".to_owned(),
        ]
        .join("\n"),
        PaneLayoutType::Horizontal2 => [
            "layout {".to_owned(),
            render_pane(0, "    "),
            render_pane(1, "    "),
            "}".to_owned(),
        ]
        .join("\n"),
        PaneLayoutType::Vertical2 => [
            "layout {".to_owned(),
            "    pane split_direction=\"vertical\" {".to_owned(),
            render_pane(0, "        "),
            render_pane(1, "        "),
            "    }".to_owned(),
            "}".to_owned(),
        ]
        .join("\n"),
        PaneLayoutType::LeftRightTb => [
            "layout {".to_owned(),
            "    pane split_direction=\"vertical\" {".to_owned(),
            render_pane(0, "        "),
            "        pane {".to_owned(),
            render_pane(1, "            "),
            render_pane(2, "            "),
            "        }".to_owned(),
            "    }".to_owned(),
            "}".to_owned(),
        ]
        .join("\n"),
        PaneLayoutType::LeftTbRight => [
            "layout {".to_owned(),
            "    pane split_direction=\"vertical\" {".to_owned(),
            "        pane {".to_owned(),
            render_pane(0, "            "),
            render_pane(1, "            "),
            "        }".to_owned(),
            render_pane(2, "        "),
            "    }".to_owned(),
            "}".to_owned(),
        ]
        .join("\n"),
        PaneLayoutType::Quad => [
            "layout {".to_owned(),
            "    pane split_direction=\"vertical\" {".to_owned(),
            "        pane {".to_owned(),
            render_pane(0, "            "),
            render_pane(2, "            "),
            "        }".to_owned(),
            "        pane {".to_owned(),
            render_pane(1, "            "),
            render_pane(3, "            "),
            "        }".to_owned(),
            "    }".to_owned(),
            "}".to_owned(),
        ]
        .join("\n"),
    }
}

fn escape_kdl(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_env_policy_strips_server_and_internal_values() {
        assert!(should_strip_from_shell_env("PORT"));
        assert!(should_strip_from_shell_env("HOST"));
        assert!(should_strip_from_shell_env("NODE_ENV"));
        assert!(should_strip_from_shell_env("KANVIBE_QA_SOCKET"));
        assert!(!should_strip_from_shell_env("PATH"));
        assert!(!should_strip_from_shell_env("HOME"));
    }

    #[test]
    fn local_shell_environment_sanitizes_and_preserves_shell_basics() {
        let environment = create_local_shell_environment(
            [
                ("PORT".to_owned(), "3000".to_owned()),
                ("HOST".to_owned(), "127.0.0.1".to_owned()),
                ("NODE_ENV".to_owned(), "production".to_owned()),
                ("KANVIBE_QA_SOCKET".to_owned(), "/tmp/socket".to_owned()),
                ("PATH".to_owned(), "/usr/bin:/usr/bin:/bin".to_owned()),
                ("CUSTOM".to_owned(), "kept".to_owned()),
            ],
            "/Users/tester",
            ShellPlatform::Mac,
        );

        assert!(!environment.contains_key("PORT"));
        assert!(!environment.contains_key("HOST"));
        assert!(!environment.contains_key("NODE_ENV"));
        assert!(!environment.contains_key("KANVIBE_QA_SOCKET"));
        assert_eq!(environment.get("CUSTOM").map(String::as_str), Some("kept"));
        assert_eq!(
            environment.get("HOME").map(String::as_str),
            Some("/Users/tester")
        );
        assert_eq!(
            environment.get("LANG").map(String::as_str),
            Some("en_US.UTF-8")
        );
        assert!(
            environment
                .get("PATH")
                .is_some_and(|path| path.contains("/opt/homebrew/bin"))
        );
    }

    #[test]
    fn remote_session_dependency_status_tracks_available_and_blocked_targets() {
        let mut registry = SessionDependencyRegistry::default();

        let initial = registry.status(SessionType::Zellij, Some("qa-remote"));
        assert_eq!(initial.tool_name, "zellij");
        assert!(initial.is_remote);
        assert!(!initial.available);
        assert!(initial.blocked_reason.is_none());

        registry.remember_available(SessionType::Zellij, Some("qa-remote"));
        assert!(
            registry
                .status(SessionType::Zellij, Some("qa-remote"))
                .available
        );

        registry.remember_install_failure(SessionType::Zellij, Some("qa-remote"), "fixture");
        let blocked = registry.status(SessionType::Zellij, Some("qa-remote"));
        assert!(!blocked.available);
        assert!(
            blocked
                .blocked_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("qa-remote"))
        );
        assert!(
            registry
                .blocked_remote_host_reason("qa-remote")
                .is_some_and(|reason| reason.contains("zellij"))
        );
    }

    #[test]
    fn session_dependency_install_commands_match_supported_tools() {
        let zellij = build_session_dependency_install_command(SessionType::Zellij);
        assert!(zellij.contains("cargo install --locked zellij"));
        assert!(zellij.contains("brew install zellij"));
        assert_eq!(
            build_session_dependency_check_command(SessionType::Tmux),
            "command -v tmux >/dev/null 2>&1"
        );
    }

    #[test]
    fn tmux_and_zellij_layout_builders_match_electron_shapes() {
        let panes = vec![
            PaneCommand {
                position: 0,
                command: "pnpm test".to_owned(),
            },
            PaneCommand {
                position: 1,
                command: "git status --short".to_owned(),
            },
        ];
        let tmux_commands = build_tmux_pane_layout_commands(
            "kanvibe-feature",
            PaneLayoutType::Vertical2,
            &panes,
            "/tmp/worktree",
        );

        assert_eq!(
            tmux_commands[0],
            "tmux split-window -h -t 'kanvibe-feature:0' -c '/tmp/worktree'"
        );
        assert!(tmux_commands[1].contains("tmux send-keys -t 'kanvibe-feature:0.0'"));

        let kdl = generate_zellij_layout_kdl(PaneLayoutType::Vertical2, &panes, "/tmp/worktree");
        assert!(kdl.contains("split_direction=\"vertical\""));
        assert!(kdl.contains("args \"-c\" \"pnpm test\""));
    }
}
