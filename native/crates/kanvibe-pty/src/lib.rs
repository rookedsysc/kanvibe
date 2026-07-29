use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{Display, Formatter},
    io::{Read, Write},
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex},
};

use kanvibe_core::{PaneCommand, PaneLayoutType, SessionType};
pub use kanvibe_session::{
    SESSION_DEPENDENCY_SUCCESS_CACHE_MS, SessionDependencyRegistry, SessionDependencyStatus,
    build_session_dependency_check_command, build_session_dependency_install_command,
    session_dependency_target_key, session_dependency_tool_name,
};
use portable_pty::{Child, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system};

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
pub const ZELLIJ_LAYOUT_FILENAME: &str = ".zellij-layout.kdl";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PtyRuntimeError(String);

impl PtyRuntimeError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for PtyRuntimeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for PtyRuntimeError {}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PtyLaunchRequest {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub environment: BTreeMap<String, String>,
    pub rows: u16,
    pub cols: u16,
}

impl PtyLaunchRequest {
    pub fn local_shell(
        shell: impl Into<String>,
        cwd: impl Into<PathBuf>,
        environment: BTreeMap<String, String>,
    ) -> Self {
        Self {
            program: shell.into(),
            args: Vec::new(),
            cwd: Some(cwd.into()),
            environment,
            rows: 24,
            cols: 80,
        }
    }

    pub fn ssh(
        ssh_host: impl Into<String>,
        remote_command: impl Into<String>,
        environment: BTreeMap<String, String>,
    ) -> Self {
        Self {
            program: "ssh".to_owned(),
            args: vec!["-tt".to_owned(), ssh_host.into(), remote_command.into()],
            cwd: None,
            environment,
            rows: 24,
            cols: 80,
        }
    }
}

pub fn build_task_session_pty_request(
    shell: &str,
    worktree_path: &str,
    session_type: Option<SessionType>,
    session_name: Option<&str>,
    ssh_host: Option<&str>,
    environment: BTreeMap<String, String>,
) -> PtyLaunchRequest {
    let session_name = session_name.filter(|name| !name.trim().is_empty());
    if let Some(ssh_host) = ssh_host.filter(|host| !host.trim().is_empty()) {
        let cwd = quote_for_posix_shell(worktree_path);
        let remote_command = match (session_type, session_name) {
            (Some(SessionType::Tmux), Some(session_name)) => format!(
                "cd {cwd} && exec tmux new-session -A -s {} -c {cwd}",
                quote_for_posix_shell(session_name)
            ),
            (Some(SessionType::Zellij), Some(session_name)) => format!(
                "cd {cwd} && exec zellij attach --create {}",
                quote_for_posix_shell(session_name)
            ),
            _ => format!("cd {cwd} && exec \"${{SHELL:-/bin/sh}}\" -l"),
        };
        return PtyLaunchRequest::ssh(ssh_host, remote_command, environment);
    }

    match (session_type, session_name) {
        (Some(SessionType::Tmux), Some(session_name)) => PtyLaunchRequest {
            program: "tmux".to_owned(),
            args: vec![
                "new-session".to_owned(),
                "-A".to_owned(),
                "-s".to_owned(),
                session_name.to_owned(),
                "-c".to_owned(),
                worktree_path.to_owned(),
            ],
            cwd: Some(PathBuf::from(worktree_path)),
            environment,
            rows: 24,
            cols: 80,
        },
        (Some(SessionType::Zellij), Some(session_name)) => PtyLaunchRequest {
            program: "zellij".to_owned(),
            args: vec![
                "attach".to_owned(),
                "--create".to_owned(),
                session_name.to_owned(),
            ],
            cwd: Some(PathBuf::from(worktree_path)),
            environment,
            rows: 24,
            cols: 80,
        },
        _ => PtyLaunchRequest::local_shell(shell, worktree_path, environment),
    }
}

pub fn is_local_tmux_session_alive(session_name: &str) -> Result<bool, PtyRuntimeError> {
    is_tmux_session_alive_with_program("tmux", session_name)
}

fn is_tmux_session_alive_with_program(
    program: impl AsRef<std::ffi::OsStr>,
    session_name: &str,
) -> Result<bool, PtyRuntimeError> {
    if session_name.trim().is_empty() || session_name.contains('\0') {
        return Err(PtyRuntimeError::new("session name must not be empty"));
    }
    match Command::new(program)
        .args(["has-session", "-t", session_name])
        .output()
    {
        Ok(output) => Ok(output.status.success()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PtyRuntimeError::new(format!(
            "failed to inspect tmux session: {error}"
        ))),
    }
}

pub fn build_session_cleanup_command(
    session_type: SessionType,
    session_name: &str,
    is_remote: bool,
) -> Result<String, PtyRuntimeError> {
    if session_name.trim().is_empty() || session_name.contains('\0') {
        return Err(PtyRuntimeError::new("session name must not be empty"));
    }
    let target = quote_for_posix_shell(session_name);
    match session_type {
        SessionType::Tmux => {
            let tmux_commands = if is_remote {
                ["tmux -L kanvibe -f /dev/null", "tmux"].as_slice()
            } else {
                ["tmux"].as_slice()
            };
            let mut commands = vec!["command -v tmux >/dev/null 2>&1 || exit 1".to_owned()];
            commands.extend(tmux_commands.iter().map(|tmux| {
                format!("{tmux} kill-session -t {target} 2>/dev/null || true")
            }));
            commands.extend(tmux_commands.iter().map(|tmux| {
                format!(
                    "if {tmux} has-session -t {target} 2>/dev/null; then exit 1; fi"
                )
            }));
            Ok(commands.join("; "))
        }
        SessionType::Zellij => Ok([
            "command -v zellij >/dev/null 2>&1 || exit 1".to_owned(),
            format!("zellij kill-sessions {target} 2>/dev/null || true"),
            format!("zellij delete-session {target} 2>/dev/null || true"),
            format!(
                "if zellij list-sessions 2>/dev/null | awk '{{ if ($1 == \"EXITED:\") print $2; else print $1 }}' | grep -Fx -- {target} >/dev/null; then exit 1; fi"
            ),
        ]
        .join("; ")),
    }
}

pub fn remove_session_only(
    session_type: SessionType,
    session_name: &str,
    ssh_host: Option<&str>,
) -> Result<(), PtyRuntimeError> {
    let command = build_session_cleanup_command(session_type, session_name, ssh_host.is_some())?;
    let output = if let Some(ssh_host) = ssh_host.filter(|host| !host.trim().is_empty()) {
        Command::new("ssh")
            .arg(ssh_host)
            .arg(command)
            .output()
            .map_err(|error| {
                PtyRuntimeError::new(format!("failed to start remote session cleanup: {error}"))
            })?
    } else {
        Command::new("/bin/sh")
            .args(["-lc", &command])
            .output()
            .map_err(|error| {
                PtyRuntimeError::new(format!("failed to start local session cleanup: {error}"))
            })?
    };

    if output.status.success() {
        return Ok(());
    }
    Err(PtyRuntimeError::new(format!(
        "session cleanup failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PtyExit {
    pub code: u32,
    pub signal: Option<String>,
}

impl From<ExitStatus> for PtyExit {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            signal: status.signal().map(ToOwned::to_owned),
        }
    }
}

#[derive(Clone)]
pub struct PtyController {
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
}

impl PtyController {
    pub fn resize(&self, cols: usize, rows: usize) -> Result<(), PtyRuntimeError> {
        let size = checked_pty_size(cols, rows)?;
        self.master
            .lock()
            .map_err(|_| PtyRuntimeError::new("PTY resize lock was poisoned"))?
            .resize(size)
            .map_err(|error| PtyRuntimeError::new(format!("failed to resize PTY: {error}")))
    }

    pub fn try_wait(&self) -> Result<Option<PtyExit>, PtyRuntimeError> {
        self.child
            .lock()
            .map_err(|_| PtyRuntimeError::new("PTY child lock was poisoned"))?
            .try_wait()
            .map(|status| status.map(Into::into))
            .map_err(|error| PtyRuntimeError::new(format!("failed to poll PTY child: {error}")))
    }

    pub fn wait(&self) -> Result<PtyExit, PtyRuntimeError> {
        self.child
            .lock()
            .map_err(|_| PtyRuntimeError::new("PTY child lock was poisoned"))?
            .wait()
            .map(Into::into)
            .map_err(|error| PtyRuntimeError::new(format!("failed to wait for PTY child: {error}")))
    }

    pub fn terminate(&self) -> Result<(), PtyRuntimeError> {
        self.child
            .lock()
            .map_err(|_| PtyRuntimeError::new("PTY child lock was poisoned"))?
            .kill()
            .map_err(|error| {
                PtyRuntimeError::new(format!("failed to terminate PTY child: {error}"))
            })
    }

    pub fn process_id(&self) -> Result<Option<u32>, PtyRuntimeError> {
        Ok(self
            .child
            .lock()
            .map_err(|_| PtyRuntimeError::new("PTY child lock was poisoned"))?
            .process_id())
    }
}

pub struct SpawnedPty {
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub controller: PtyController,
}

pub fn spawn_pty(request: PtyLaunchRequest) -> Result<SpawnedPty, PtyRuntimeError> {
    let size = checked_pty_size(usize::from(request.cols), usize::from(request.rows))?;
    let pair = native_pty_system()
        .openpty(size)
        .map_err(|error| PtyRuntimeError::new(format!("failed to open PTY: {error}")))?;
    let mut command = CommandBuilder::new(&request.program);
    command.args(&request.args);
    command.env_clear();
    for (key, value) in request
        .environment
        .iter()
        .filter(|(key, _)| !should_strip_from_shell_env(key))
    {
        command.env(key, value);
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    if let Some(cwd) = &request.cwd {
        command.cwd(cwd);
    }

    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| PtyRuntimeError::new(format!("failed to spawn PTY command: {error}")))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| PtyRuntimeError::new(format!("failed to clone PTY reader: {error}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| PtyRuntimeError::new(format!("failed to take PTY writer: {error}")))?;
    let master = Arc::new(Mutex::new(pair.master));
    drop(pair.slave);

    Ok(SpawnedPty {
        reader,
        writer,
        controller: PtyController {
            master,
            child: Arc::new(Mutex::new(child)),
        },
    })
}

fn checked_pty_size(cols: usize, rows: usize) -> Result<PtySize, PtyRuntimeError> {
    let cols = u16::try_from(cols)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| PtyRuntimeError::new("PTY columns must be between 1 and 65535"))?;
    let rows = u16::try_from(rows)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| PtyRuntimeError::new("PTY rows must be between 1 and 65535"))?;
    Ok(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}

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

    let delimiter = ':';
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

        let local = build_task_session_pty_request(
            "/bin/zsh",
            "/tmp/worktree",
            Some(SessionType::Tmux),
            Some("kanvibe-feature"),
            None,
            BTreeMap::new(),
        );
        assert_eq!(local.program, "tmux");
        assert_eq!(
            local.args,
            [
                "new-session",
                "-A",
                "-s",
                "kanvibe-feature",
                "-c",
                "/tmp/worktree"
            ]
        );

        let remote = build_task_session_pty_request(
            "/bin/zsh",
            "/srv/worktree",
            Some(SessionType::Zellij),
            Some("remote-feature"),
            Some("qa-host"),
            BTreeMap::new(),
        );
        assert_eq!(remote.program, "ssh");
        assert_eq!(remote.args[0], "-tt");
        assert_eq!(remote.args[1], "qa-host");
        assert!(remote.args[2].contains("zellij attach --create 'remote-feature'"));
    }

    #[cfg(unix)]
    #[test]
    fn portable_pty_round_trips_shell_io_resize_and_exit() {
        let request = PtyLaunchRequest {
            program: "/bin/sh".to_owned(),
            args: vec![
                "-lc".to_owned(),
                "read value; printf 'pty-ready:%s\\n' \"$value\"".to_owned(),
            ],
            cwd: Some(std::env::temp_dir()),
            environment: BTreeMap::from([
                ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
                ("KANVIBE_SECRET".to_owned(), "must-not-leak".to_owned()),
            ]),
            rows: 24,
            cols: 80,
        };
        let SpawnedPty {
            mut reader,
            mut writer,
            controller,
        } = spawn_pty(request).expect("spawn shell in PTY");

        controller.resize(100, 30).expect("resize PTY");
        writer.write_all(b"hello\n").expect("write PTY input");
        writer.flush().expect("flush PTY input");
        let mut output = String::new();
        reader.read_to_string(&mut output).expect("read PTY output");
        let exit = controller.wait().expect("wait for PTY child");

        assert!(output.contains("pty-ready:hello"));
        assert_eq!(exit.code, 0);
        assert!(exit.signal.is_none());
        assert!(controller.process_id().expect("read process id").is_some());
    }

    #[test]
    fn session_cleanup_commands_verify_removal_and_quote_exact_names() {
        let tmux =
            build_session_cleanup_command(SessionType::Tmux, "project-feature/'quoted", true)
                .expect("tmux cleanup");
        assert!(tmux.contains("tmux -L kanvibe -f /dev/null"));
        assert!(tmux.contains("has-session"));
        assert!(tmux.contains("'project-feature/'\"'\"'quoted'"));

        let zellij = build_session_cleanup_command(SessionType::Zellij, "project-session", false)
            .expect("zellij cleanup");
        assert!(zellij.contains("kill-sessions"));
        assert!(zellij.contains("delete-session"));
        assert!(zellij.contains("grep -Fx"));
        assert!(build_session_cleanup_command(SessionType::Tmux, " ", false).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn tmux_liveness_probe_uses_exact_session_target_and_handles_missing_binary() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "kanvibe-tmux-probe-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).expect("create tmux probe fixture");
        let fake_tmux = root.join("tmux");
        std::fs::write(
            &fake_tmux,
            "#!/bin/sh\n[ \"$1\" = has-session ] && [ \"$2\" = -t ] && [ \"$3\" = exact-session ]\n",
        )
        .expect("write fake tmux");
        std::fs::set_permissions(&fake_tmux, std::fs::Permissions::from_mode(0o755))
            .expect("make fake tmux executable");

        assert!(
            is_tmux_session_alive_with_program(&fake_tmux, "exact-session")
                .expect("probe matching session")
        );
        assert!(
            !is_tmux_session_alive_with_program(&fake_tmux, "other-session")
                .expect("probe missing session")
        );
        assert!(is_tmux_session_alive_with_program(&fake_tmux, " ").is_err());
        assert!(
            !is_tmux_session_alive_with_program(root.join("missing-tmux"), "exact-session")
                .expect("missing tmux is an inactive session")
        );

        std::fs::remove_dir_all(root).expect("remove tmux probe fixture");
    }

    #[cfg(unix)]
    #[test]
    fn portable_pty_clears_parent_environment_and_sets_terminal_contract() {
        let request = PtyLaunchRequest {
            program: "/bin/sh".to_owned(),
            args: vec![
                "-lc".to_owned(),
                "printf '%s:%s:%s\\n' \"${KANVIBE_SECRET-unset}\" \"$TERM\" \"$COLORTERM\""
                    .to_owned(),
            ],
            cwd: Some(std::env::temp_dir()),
            environment: BTreeMap::from([(
                "KANVIBE_SECRET".to_owned(),
                "must-not-leak".to_owned(),
            )]),
            rows: 24,
            cols: 80,
        };
        let SpawnedPty {
            mut reader,
            writer: _writer,
            controller,
        } = spawn_pty(request).expect("spawn isolated PTY");
        let mut output = String::new();
        reader
            .read_to_string(&mut output)
            .expect("read isolated PTY");
        let exit = controller.wait().expect("wait for isolated child");

        assert!(output.contains("unset:xterm-256color:truecolor"));
        assert_eq!(exit.code, 0);
        assert!(checked_pty_size(0, 24).is_err());
        assert!(checked_pty_size(80, 0).is_err());
    }
}
