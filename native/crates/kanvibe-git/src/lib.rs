use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    ffi::OsString,
    fmt::{Display, Formatter},
    fs,
    io::Read,
    net::IpAddr,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use kanvibe_core::SessionType;
use kanvibe_session::{
    build_session_dependency_check_command, build_session_dependency_install_command,
    session_dependency_tool_name,
};
use serde::Deserialize;

pub const CRATE_PURPOSE: &str = "Git, branch, worktree, and pull request command contracts";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BranchRef {
    name: String,
}

impl BranchRef {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GitError {
    kind: GitErrorKind,
    message: String,
}

impl GitError {
    fn new(message: impl Into<String>) -> Self {
        Self::with_kind(GitErrorKind::Command, message)
    }

    fn with_kind(kind: GitErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub const fn kind(&self) -> GitErrorKind {
        self.kind
    }
}

impl Display for GitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for GitError {}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GitErrorKind {
    InvalidInput,
    Spawn,
    Timeout,
    Transport,
    Command,
    OutputLimit,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DiffFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl DiffFileStatus {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DiffFile {
    pub path: String,
    pub status: DiffFileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub is_binary: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WorktreeSession {
    pub worktree_path: PathBuf,
    pub session_name: String,
    pub session_type: SessionType,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RegisteredWorktree {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_bare: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PullRequestInfo {
    pub url: String,
    pub state: String,
    pub merged_at: Option<String>,
    pub updated_at: Option<String>,
}

const REMOTE_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const REMOTE_COMMAND_MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const REMOTE_CONTROL_PERSIST: &str = "10m";
const REMOTE_COMMAND_MAX_ATTEMPTS: usize = 2;
const REMOTE_CONTROL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const REMOTE_EDIT_MAX_BYTES: usize = 64 * 1024;
const GITHUB_CLI_TIMEOUT: Duration = Duration::from_secs(10);
const GITHUB_CLI_MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const GITHUB_CLI_INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const SESSION_DEPENDENCY_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const SESSION_DEPENDENCY_INSTALL_TIMEOUT: Duration = Duration::from_secs(300);
const SESSION_DEPENDENCY_MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const HOOK_HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
const HOOK_HEALTH_MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct RemoteGitClient {
    ssh_host: String,
    control_directory: PathBuf,
    ssh_binary: PathBuf,
    command_timeout: Duration,
    max_output_bytes: usize,
}

impl RemoteGitClient {
    pub fn new(
        ssh_host: impl Into<String>,
        control_directory: impl Into<PathBuf>,
    ) -> Result<Self, GitError> {
        let ssh_host = ssh_host.into();
        if ssh_host.is_empty()
            || ssh_host.starts_with('-')
            || ssh_host.chars().any(char::is_whitespace)
            || ssh_host.chars().any(char::is_control)
        {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "SSH host must be one non-option destination without whitespace",
            ));
        }
        let control_directory = control_directory.into();
        if control_directory.as_os_str().is_empty() {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "SSH control directory is required",
            ));
        }
        Ok(Self {
            ssh_host,
            control_directory,
            ssh_binary: PathBuf::from("ssh"),
            command_timeout: REMOTE_COMMAND_TIMEOUT,
            max_output_bytes: REMOTE_COMMAND_MAX_OUTPUT_BYTES,
        })
    }

    pub fn validate_repo(&self, repo_path: &str) -> Result<bool, GitError> {
        Ok(self.exec_git(repo_path, &["rev-parse", "--is-inside-work-tree"])? == "true")
    }

    pub fn default_branch(&self, repo_path: &str) -> Result<String, GitError> {
        if let Ok(reference) = self.exec_git(
            repo_path,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        ) && let Some((_, branch)) = reference.split_once('/')
            && !branch.trim().is_empty()
        {
            return Ok(branch.to_owned());
        }
        let branch = self.exec_git(repo_path, &["branch", "--show-current"])?;
        if branch.is_empty() {
            return Err(GitError::new(
                "remote repository has no current branch or origin default branch",
            ));
        }
        Ok(branch)
    }

    pub fn list_branches(&self, repo_path: &str) -> Result<Vec<String>, GitError> {
        let output = self.exec_git(repo_path, &["branch", "--format=%(refname:short)"])?;
        Ok(output
            .lines()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    pub fn list_worktrees(&self, repo_path: &str) -> Result<Vec<RegisteredWorktree>, GitError> {
        let output = self.exec_git(repo_path, &["worktree", "list", "--porcelain"])?;
        Ok(parse_worktree_porcelain(&output))
    }

    pub fn pull_request_for_branch(
        &self,
        repo_path: &str,
        branch_name: &str,
    ) -> Result<Option<PullRequestInfo>, GitError> {
        validate_remote_ref(branch_name)?;
        if repo_path.is_empty() || repo_path.chars().any(char::is_control) {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "remote repository path must be non-empty and contain no control characters",
            ));
        }
        let command = std::iter::once("gh")
            .chain([
                "pr",
                "list",
                "--head",
                branch_name,
                "--state",
                "all",
                "--json",
                "url,state,mergedAt,updatedAt",
            ])
            .map(quote_for_posix_shell)
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!(
            "cd {repo} && {command}",
            repo = quote_for_posix_shell(repo_path)
        );
        match self.execute_remote_shell_with_options(
            &command,
            GITHUB_CLI_TIMEOUT,
            GITHUB_CLI_MAX_OUTPUT_BYTES,
        ) {
            Ok(output) => parse_pull_request_list(&output),
            Err(error) if is_missing_github_cli_message(&error.to_string()) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn github_cli_available(&self) -> Result<bool, GitError> {
        match self.execute_remote_shell("command -v gh >/dev/null 2>&1") {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == GitErrorKind::Command => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn install_github_cli(&self) -> Result<(), GitError> {
        self.execute_remote_shell_with_options(
            GITHUB_CLI_INSTALL_SCRIPT,
            GITHUB_CLI_INSTALL_TIMEOUT,
            GITHUB_CLI_MAX_OUTPUT_BYTES,
        )?;
        self.github_cli_available()?.then_some(()).ok_or_else(|| {
            GitError::with_kind(
                GitErrorKind::Command,
                "GitHub CLI installation completed without making gh available",
            )
        })
    }

    pub fn session_dependency_available(
        &self,
        session_type: SessionType,
    ) -> Result<bool, GitError> {
        match self.execute_remote_shell_with_options(
            &build_session_dependency_check_command(session_type),
            SESSION_DEPENDENCY_CHECK_TIMEOUT.min(self.command_timeout),
            SESSION_DEPENDENCY_MAX_OUTPUT_BYTES.min(self.max_output_bytes),
        ) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == GitErrorKind::Command => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn install_session_dependency(&self, session_type: SessionType) -> Result<(), GitError> {
        self.execute_remote_shell_with_options(
            &build_session_dependency_install_command(session_type),
            SESSION_DEPENDENCY_INSTALL_TIMEOUT,
            SESSION_DEPENDENCY_MAX_OUTPUT_BYTES,
        )?;
        self.session_dependency_available(session_type)?
            .then_some(())
            .ok_or_else(|| {
                GitError::with_kind(
                    GitErrorKind::Command,
                    format!(
                        "{} installation completed without making it available",
                        session_dependency_tool_name(session_type)
                    ),
                )
            })
    }

    pub fn remote_branch_exists(
        &self,
        repo_path: &str,
        branch_name: &str,
    ) -> Result<bool, GitError> {
        validate_remote_ref(branch_name)?;
        let remote_ref = format!("refs/heads/{branch_name}");
        let command = format!(
            "{git} >/dev/null; status=$?; \
             if test \"$status\" -eq 0; then printf exists; \
             elif test \"$status\" -eq 2; then printf missing; else exit \"$status\"; fi",
            git = build_remote_git_command(
                repo_path,
                &["ls-remote", "--exit-code", "--heads", "origin", &remote_ref],
            ),
        );
        Ok(self.execute_remote_shell(&command)?.trim() == "exists")
    }

    pub fn pull_current_branch(&self, repo_path: &str) -> Result<String, GitError> {
        self.exec_git_raw(repo_path, &["pull", "--ff-only"])
    }

    pub fn create_worktree(
        &self,
        project_path: &str,
        branch_name: &str,
        base_branch: &str,
    ) -> Result<PathBuf, GitError> {
        let worktree_path = build_managed_worktree_path(Path::new(project_path), branch_name);
        let worktree_path_string = worktree_path.to_string_lossy().into_owned();
        self.exec_git(
            project_path,
            &[
                "worktree",
                "add",
                &worktree_path_string,
                "-b",
                branch_name,
                base_branch,
            ],
        )?;
        Ok(worktree_path)
    }

    /// Remove an exact Git-registered linked worktree and then its local branch.
    ///
    /// The first worktree entry is treated as the project root and is never removed.
    /// `expected_worktree_path` is only a consistency guard; it is never used as an
    /// independent deletion target.
    pub fn remove_worktree_and_branch(
        &self,
        project_path: &str,
        branch_name: &str,
        expected_worktree_path: Option<&Path>,
    ) -> Result<(), GitError> {
        if branch_name.is_empty()
            || branch_name.starts_with('-')
            || branch_name.chars().any(char::is_control)
        {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "remote branch name must be non-empty, non-option, and contain no control characters",
            ));
        }
        let expected_worktree_path = expected_worktree_path
            .map(|path| {
                path.to_str().ok_or_else(|| {
                    GitError::with_kind(
                        GitErrorKind::InvalidInput,
                        "remote worktree path must be valid UTF-8",
                    )
                })
            })
            .transpose()?;
        let worktrees = self.list_worktrees(project_path)?;
        let root = worktrees
            .first()
            .map(|worktree| worktree.path.as_path())
            .unwrap_or_else(|| Path::new(project_path));
        let matching = worktrees.iter().filter(|worktree| {
            !worktree.is_bare && worktree.branch.as_deref() == Some(branch_name)
        });
        let root_has_branch = matching.clone().any(|worktree| worktree.path == root);
        let linked = matching
            .filter(|worktree| worktree.path != root)
            .find(|worktree| {
                expected_worktree_path.is_none_or(|expected| worktree.path == Path::new(expected))
            });

        if let Some(expected) = expected_worktree_path {
            let expected_is_root = Path::new(expected) == root;
            if linked.is_none() && !(root_has_branch && expected_is_root) {
                return Err(GitError::with_kind(
                    GitErrorKind::InvalidInput,
                    "expected remote worktree is not registered for the requested branch",
                ));
            }
        }

        if let Some(linked) = linked {
            let path = linked.path.to_str().ok_or_else(|| {
                GitError::with_kind(
                    GitErrorKind::InvalidInput,
                    "registered remote worktree path must be valid UTF-8",
                )
            })?;
            self.exec_git(project_path, &["worktree", "remove", path, "--force"])?;
        }

        if root_has_branch {
            return Ok(());
        }

        let branch_ref = format!("refs/heads/{branch_name}");
        match self.exec_git(
            project_path,
            &["show-ref", "--verify", "--quiet", &branch_ref],
        ) {
            Ok(_) => {
                self.exec_git(project_path, &["branch", "-D", "--", branch_name])?;
            }
            Err(error) if error.kind() == GitErrorKind::Command => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }

    pub fn changed_files(
        &self,
        worktree_path: &str,
        base_branch: &str,
        branch_name: &str,
    ) -> Result<Vec<DiffFile>, GitError> {
        validate_remote_ref(base_branch)?;
        validate_remote_ref(branch_name)?;
        let range = format!("{base_branch}...{branch_name}");
        let name_status = self
            .exec_git(worktree_path, &["diff", &range, "--name-status"])
            .or_else(|error| {
                (error.kind() == GitErrorKind::Command)
                    .then(String::new)
                    .ok_or(error)
            })?;
        let numstat = self
            .exec_git(worktree_path, &["diff", &range, "--numstat"])
            .or_else(|error| {
                (error.kind() == GitErrorKind::Command)
                    .then(String::new)
                    .ok_or(error)
            })?;
        let status = self.exec_git_raw(
            worktree_path,
            &["status", "--porcelain", "--untracked-files=all"],
        )?;
        Ok(parse_changed_files(&name_status, &numstat, &status))
    }

    pub fn original_file_content(
        &self,
        worktree_path: &str,
        base_branch: &str,
        file_path: &str,
    ) -> Result<String, GitError> {
        validate_remote_ref(base_branch)?;
        validate_remote_relative_path(file_path)?;
        self.exec_git_raw(
            worktree_path,
            &["show", &format!("{base_branch}:{file_path}")],
        )
        .or_else(|error| {
            (error.kind() == GitErrorKind::Command)
                .then(String::new)
                .ok_or(error)
        })
    }

    pub fn file_content(&self, worktree_path: &str, file_path: &str) -> Result<String, GitError> {
        self.read_optional_file(worktree_path, file_path)?
            .ok_or_else(|| GitError::with_kind(GitErrorKind::Command, "remote file does not exist"))
    }

    pub fn save_file_content_if_unchanged(
        &self,
        worktree_path: &str,
        file_path: &str,
        expected_current: &str,
        content: &str,
    ) -> Result<(), GitError> {
        validate_remote_relative_path(file_path)?;
        if expected_current.len() > REMOTE_EDIT_MAX_BYTES || content.len() > REMOTE_EDIT_MAX_BYTES {
            return Err(GitError::with_kind(
                GitErrorKind::OutputLimit,
                format!("remote editor content exceeds {REMOTE_EDIT_MAX_BYTES} bytes"),
            ));
        }
        let target_path = format!("{}/{}", worktree_path.trim_end_matches('/'), file_path);
        let parent_path = target_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .ok_or_else(|| {
                GitError::with_kind(GitErrorKind::InvalidInput, "remote file path has no parent")
            })?;
        let command = format!(
            "KANVIBE_EXPECTED_CONTENT={expected}; KANVIBE_NEW_CONTENT={content}; \
             KANVIBE_WORKTREE={worktree}; KANVIBE_PARENT={parent}; KANVIBE_TARGET={target}; \
             root=$(cd \"$KANVIBE_WORKTREE\" && pwd -P) || exit 70; \
             parent_real=$(cd \"$KANVIBE_PARENT\" && pwd -P) || exit 71; \
             case \"$parent_real/\" in \"$root/\"*) ;; *) printf '%s' 'remote file parent escapes worktree' >&2; exit 72;; esac; \
             test ! -L \"$KANVIBE_TARGET\" || {{ printf '%s' 'remote symbolic-link edits are not allowed' >&2; exit 73; }}; \
             test -f \"$KANVIBE_TARGET\" || {{ printf '%s' 'remote file does not exist' >&2; exit 74; }}; \
             expected_path=$(mktemp \"${{KANVIBE_TARGET}}.kanvibe-expected.XXXXXX\") || exit 76; \
             content_path=$(mktemp \"${{KANVIBE_TARGET}}.kanvibe-content.XXXXXX\") || {{ rm -f \"$expected_path\"; exit 76; }}; \
             trap 'rm -f \"$expected_path\" \"$content_path\"' EXIT HUP INT TERM; \
             printf '%s' \"$KANVIBE_EXPECTED_CONTENT\" | (base64 -d 2>/dev/null || base64 -D) > \"$expected_path\"; \
             if ! cmp -s \"$KANVIBE_TARGET\" \"$expected_path\"; then printf '%s' 'KANVIBE_CONFLICT' >&2; exit 75; fi; \
             printf '%s' \"$KANVIBE_NEW_CONTENT\" | (base64 -d 2>/dev/null || base64 -D) > \"$content_path\"; \
             cat \"$content_path\" > \"$KANVIBE_TARGET\"",
            expected = quote_for_posix_shell(&encode_base64(expected_current.as_bytes())),
            content = quote_for_posix_shell(&encode_base64(content.as_bytes())),
            worktree = quote_for_posix_shell(worktree_path),
            parent = quote_for_posix_shell(parent_path),
            target = quote_for_posix_shell(&target_path),
        );
        match self.execute_remote_shell(&command) {
            Ok(_) => Ok(()),
            Err(error) if error.to_string().contains("KANVIBE_CONFLICT") => {
                Err(GitError::with_kind(
                    GitErrorKind::Command,
                    "file changed on remote host after it was loaded; reload before saving",
                ))
            }
            Err(error) => Err(error),
        }
    }

    pub fn ssh_client_address(&self) -> Result<String, GitError> {
        let connection = self.execute_remote_shell("printf '%s' \"$SSH_CONNECTION\"")?;
        let address = connection.split_whitespace().next().ok_or_else(|| {
            GitError::with_kind(
                GitErrorKind::Transport,
                "remote SSH_CONNECTION did not contain a client address",
            )
        })?;
        address.parse::<IpAddr>().map_err(|_| {
            GitError::with_kind(
                GitErrorKind::Transport,
                "remote SSH_CONNECTION client address was invalid",
            )
        })?;
        Ok(address.to_owned())
    }

    pub fn native_hook_server_reachable(
        &self,
        callback_host: IpAddr,
        port: u16,
    ) -> Result<bool, GitError> {
        let host = match callback_host {
            IpAddr::V4(address) => address.to_string(),
            IpAddr::V6(address) => format!("[{address}]"),
        };
        let url = quote_for_posix_shell(&format!("http://{host}:{port}/api/hooks/health"));
        let command = format!(
            "if command -v curl >/dev/null 2>&1; then \
               curl -fsS --max-time 3 -- {url} >/dev/null; \
             elif command -v wget >/dev/null 2>&1; then \
               wget -q -T 3 -O /dev/null -- {url}; \
             else exit 69; fi"
        );
        match self.execute_remote_shell_with_options(
            &command,
            HOOK_HEALTH_TIMEOUT,
            HOOK_HEALTH_MAX_OUTPUT_BYTES,
        ) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == GitErrorKind::Command => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn home_directory(&self) -> Result<String, GitError> {
        let home = self.execute_remote_shell("cd && pwd -P")?;
        let home = home.trim();
        if !home.starts_with('/') || home.chars().any(char::is_control) {
            return Err(GitError::with_kind(
                GitErrorKind::Transport,
                "remote HOME directory was invalid",
            ));
        }
        Ok(home.to_owned())
    }

    pub fn home_path_exists(&self, path: &str) -> Result<bool, GitError> {
        validate_remote_absolute_path(path)?;
        let command = format!(
            "{guard} if test -e \"$KANVIBE_PATH\"; then printf 1; else printf 0; fi",
            guard = remote_home_path_guard(path, false),
        );
        match self.execute_remote_shell(&command)?.trim() {
            "1" => Ok(true),
            "0" => Ok(false),
            _ => Err(GitError::with_kind(
                GitErrorKind::Command,
                "remote path existence response was invalid",
            )),
        }
    }

    pub fn list_home_files(
        &self,
        root: &str,
        suffix: &str,
        recursive: bool,
    ) -> Result<Vec<String>, GitError> {
        validate_remote_absolute_path(root)?;
        if suffix.is_empty() || suffix.contains('/') || suffix.chars().any(char::is_control) {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "remote file suffix must be one non-empty path-free value",
            ));
        }
        let depth = if recursive { "" } else { "-maxdepth 1" };
        let command = format!(
            "{initial_guard} if ! test -e \"$KANVIBE_PATH\"; then exit 0; fi; {guard} \
             test -d \"$KANVIBE_PATH\" || {{ printf '%s' 'remote session root is not a directory' >&2; exit 74; }}; \
             find -P \"$KANVIBE_PATH\" {depth} -type f -name {pattern} -print",
            initial_guard = remote_home_path_guard(root, false),
            guard = remote_home_path_guard(root, true),
            pattern = quote_for_posix_shell(&format!("*{suffix}")),
        );
        Ok(self
            .execute_remote_shell(&command)?
            .lines()
            .filter(|path| !path.is_empty())
            .map(ToOwned::to_owned)
            .collect())
    }

    pub fn read_home_text(&self, path: &str) -> Result<String, GitError> {
        validate_remote_absolute_path(path)?;
        let command = format!(
            "{guard} test -f \"$KANVIBE_PATH\" || {{ printf '%s' 'remote session file is not a regular file' >&2; exit 74; }}; \
             test ! -L \"$KANVIBE_PATH\" || {{ printf '%s' 'remote session file symlinks are not allowed' >&2; exit 73; }}; \
             cat \"$KANVIBE_PATH\"",
            guard = remote_home_path_guard(path, true),
        );
        self.execute_remote_shell(&command)
    }

    pub fn query_open_code_history(
        &self,
        database_path: &str,
        session_id: Option<&str>,
    ) -> Result<String, GitError> {
        validate_remote_absolute_path(database_path)?;
        if session_id.is_some_and(|value| value.chars().any(char::is_control)) {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "OpenCode session id contains control characters",
            ));
        }
        let query_kind = if session_id.is_some() {
            "messages"
        } else {
            "sessions"
        };
        let python = r#"import json, sqlite3, sys
db_path, kind, session_id = sys.argv[1:4]
connection = sqlite3.connect("file:" + db_path + "?mode=ro", uri=True)
connection.row_factory = sqlite3.Row
if kind == "sessions":
    sql = """SELECT s.id, s.directory, s.title, s.time_created, s.time_updated,
      (SELECT COUNT(*) FROM part p WHERE p.session_id = s.id) AS part_count,
      (SELECT p.data FROM part p JOIN message m ON m.id = p.message_id
       WHERE p.session_id = s.id AND json_extract(m.data, '$.role') = 'user'
       AND json_extract(p.data, '$.type') = 'text'
       ORDER BY p.time_created ASC LIMIT 1) AS first_user_part
      FROM session s ORDER BY s.time_updated DESC LIMIT 120"""
    rows = connection.execute(sql).fetchall()
else:
    sql = """SELECT s.id AS session_id, s.directory, s.title, p.data AS part_data,
      p.time_created, m.data AS message_data
      FROM session s JOIN part p ON p.session_id = s.id
      JOIN message m ON m.id = p.message_id
      WHERE s.id = ? ORDER BY p.time_created ASC"""
    rows = connection.execute(sql, (session_id,)).fetchall()
print(json.dumps([dict(row) for row in rows], ensure_ascii=False))
"#;
        let command = format!(
            "{initial_guard} if ! test -e \"$KANVIBE_PATH\"; then printf '%s' '__KANVIBE_DB_MISSING__'; exit 0; fi; {guard} \
             test -f \"$KANVIBE_PATH\" || {{ printf '%s' 'remote OpenCode database is not a regular file' >&2; exit 74; }}; \
             python3 -c {python} \"$KANVIBE_PATH\" {kind} {session}",
            initial_guard = remote_home_path_guard(database_path, false),
            guard = remote_home_path_guard(database_path, true),
            python = quote_for_posix_shell(python),
            kind = quote_for_posix_shell(query_kind),
            session = quote_for_posix_shell(session_id.unwrap_or("")),
        );
        self.execute_remote_shell(&command)
    }

    pub fn read_optional_file(
        &self,
        worktree_path: &str,
        file_path: &str,
    ) -> Result<Option<String>, GitError> {
        validate_remote_relative_path(file_path)?;
        let path = Path::new(file_path);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                GitError::with_kind(GitErrorKind::InvalidInput, "remote file name is invalid")
            })?;
        let mut parent_steps = String::new();
        if let Some(parent) = path.parent() {
            for component in parent.components() {
                let Component::Normal(component) = component else {
                    return Err(GitError::with_kind(
                        GitErrorKind::InvalidInput,
                        "remote file parent contains an invalid component",
                    ));
                };
                let component = component.to_str().ok_or_else(|| {
                    GitError::with_kind(
                        GitErrorKind::InvalidInput,
                        "remote file parent must be valid UTF-8",
                    )
                })?;
                parent_steps.push_str(&format!(
                    "candidate=\"$current\"/{component}; \
                     if ! test -e \"$candidate\"; then printf '%s' '__KANVIBE_FILE_MISSING__'; exit 0; fi; \
                     test ! -L \"$candidate\" || {{ printf '%s' 'remote symbolic-link parent is not allowed' >&2; exit 73; }}; \
                     test -d \"$candidate\" || {{ printf '%s' 'remote file parent is not a directory' >&2; exit 74; }}; \
                     current=$(cd \"$candidate\" && pwd -P) || exit 71; \
                     case \"$current/\" in \"$root/\"*) ;; *) printf '%s' 'remote file parent escapes worktree' >&2; exit 72;; esac; ",
                    component = quote_for_posix_shell(component),
                ));
            }
        }
        let command = format!(
            "KANVIBE_WORKTREE={worktree}; root=$(cd \"$KANVIBE_WORKTREE\" && pwd -P) || exit 70; current=\"$root\"; \
             {parent_steps} target=\"$current\"/{file_name}; \
             if ! test -e \"$target\"; then printf '%s' '__KANVIBE_FILE_MISSING__'; exit 0; fi; \
             test ! -L \"$target\" || {{ printf '%s' 'remote symbolic-link target is not allowed' >&2; exit 73; }}; \
             test -f \"$target\" || {{ printf '%s' 'remote target is not a file' >&2; exit 74; }}; \
             printf '%s' '__KANVIBE_FILE_PRESENT__'; cat \"$target\"",
            worktree = quote_for_posix_shell(worktree_path),
            file_name = quote_for_posix_shell(file_name),
        );
        let output = self.execute_remote_shell(&command)?;
        if output == "__KANVIBE_FILE_MISSING__" {
            return Ok(None);
        }
        output
            .strip_prefix("__KANVIBE_FILE_PRESENT__")
            .map(|content| Some(content.to_owned()))
            .ok_or_else(|| {
                GitError::with_kind(
                    GitErrorKind::Command,
                    "remote file response marker was missing",
                )
            })
    }

    pub fn write_file(
        &self,
        worktree_path: &str,
        file_path: &str,
        content: &str,
        executable: bool,
    ) -> Result<(), GitError> {
        validate_remote_relative_path(file_path)?;
        if content.len() > REMOTE_EDIT_MAX_BYTES {
            return Err(GitError::with_kind(
                GitErrorKind::OutputLimit,
                format!("remote file content exceeds {REMOTE_EDIT_MAX_BYTES} bytes"),
            ));
        }
        let path = Path::new(file_path);
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                GitError::with_kind(GitErrorKind::InvalidInput, "remote file name is invalid")
            })?;
        let mut parent_steps = String::new();
        if let Some(parent) = path.parent() {
            for component in parent.components() {
                let Component::Normal(component) = component else {
                    return Err(GitError::with_kind(
                        GitErrorKind::InvalidInput,
                        "remote file parent contains an invalid component",
                    ));
                };
                let component = component.to_str().ok_or_else(|| {
                    GitError::with_kind(
                        GitErrorKind::InvalidInput,
                        "remote file parent must be valid UTF-8",
                    )
                })?;
                parent_steps.push_str(&format!(
                    "candidate=\"$current\"/{component}; \
                     test ! -L \"$candidate\" || {{ printf '%s' 'remote symbolic-link parent is not allowed' >&2; exit 73; }}; \
                     if ! test -e \"$candidate\"; then mkdir \"$candidate\" || exit 76; fi; \
                     test -d \"$candidate\" || {{ printf '%s' 'remote file parent is not a directory' >&2; exit 74; }}; \
                     current=$(cd \"$candidate\" && pwd -P) || exit 71; \
                     case \"$current/\" in \"$root/\"*) ;; *) printf '%s' 'remote file parent escapes worktree' >&2; exit 72;; esac; ",
                    component = quote_for_posix_shell(component),
                ));
            }
        }
        let mode = if executable { "755" } else { "644" };
        let command = format!(
            "KANVIBE_WORKTREE={worktree}; KANVIBE_CONTENT={content}; \
             root=$(cd \"$KANVIBE_WORKTREE\" && pwd -P) || exit 70; current=\"$root\"; \
             {parent_steps} \
             target=\"$current\"/{file_name}; \
             test ! -L \"$target\" || {{ printf '%s' 'remote symbolic-link target is not allowed' >&2; exit 73; }}; \
             temp_path=$(mktemp \"${{target}}.kanvibe.XXXXXX\") || exit 76; \
             trap 'rm -f \"$temp_path\"' EXIT HUP INT TERM; \
             printf '%s' \"$KANVIBE_CONTENT\" | (base64 -d 2>/dev/null || base64 -D) > \"$temp_path\"; \
             chmod {mode} \"$temp_path\" || exit 76; mv \"$temp_path\" \"$target\"",
            worktree = quote_for_posix_shell(worktree_path),
            content = quote_for_posix_shell(&encode_base64(content.as_bytes())),
            file_name = quote_for_posix_shell(file_name),
        );
        self.execute_remote_shell(&command).map(|_| ())
    }

    pub fn ensure_git_exclude_lines(
        &self,
        worktree_path: &str,
        lines: &[&str],
    ) -> Result<(), GitError> {
        if lines.is_empty()
            || lines
                .iter()
                .any(|line| line.is_empty() || line.chars().any(char::is_control))
        {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "remote Git exclude lines must be non-empty single lines",
            ));
        }
        let resolve_exclude = build_remote_git_command(
            worktree_path,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "info/exclude",
            ],
        );
        let mut command = format!(
            "exclude_path=$({resolve_exclude}) || exit 77; \
             mkdir -p \"$(dirname \"$exclude_path\")\" || exit 76; \
             touch \"$exclude_path\" || exit 76; "
        );
        for line in lines {
            command.push_str(&format!(
                "grep -Fqx -- {line} \"$exclude_path\" 2>/dev/null || printf '%s\\n' {line} >> \"$exclude_path\"; ",
                line = quote_for_posix_shell(line),
            ));
        }
        self.execute_remote_shell(&command).map(|_| ())
    }

    fn exec_git(&self, repo_path: &str, args: &[&str]) -> Result<String, GitError> {
        Ok(self.exec_git_raw(repo_path, args)?.trim().to_owned())
    }

    fn exec_git_raw(&self, repo_path: &str, args: &[&str]) -> Result<String, GitError> {
        if repo_path.is_empty() || repo_path.chars().any(char::is_control) {
            return Err(GitError::with_kind(
                GitErrorKind::InvalidInput,
                "remote repository path must be non-empty and contain no control characters",
            ));
        }
        let command = build_remote_git_command(repo_path, args);
        self.execute_remote_shell(&command)
    }

    fn execute_remote_shell(&self, command: &str) -> Result<String, GitError> {
        self.execute_remote_shell_with_options(command, self.command_timeout, self.max_output_bytes)
    }

    fn execute_remote_shell_with_options(
        &self,
        command: &str,
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Result<String, GitError> {
        create_control_directory(&self.control_directory)?;
        let control_path = self.control_directory.join("ssh-%C-0");
        let ssh_args = [
            "-T".to_owned(),
            "-o".to_owned(),
            "ControlMaster=auto".to_owned(),
            "-o".to_owned(),
            format!("ControlPersist={REMOTE_CONTROL_PERSIST}"),
            "-o".to_owned(),
            format!("ControlPath={}", control_path.to_string_lossy()),
            "-o".to_owned(),
            "ConnectTimeout=10".to_owned(),
            "-o".to_owned(),
            "ServerAliveInterval=15".to_owned(),
            "-o".to_owned(),
            "ServerAliveCountMax=2".to_owned(),
            self.ssh_host.clone(),
            format!("sh -lc {}", quote_for_posix_shell(command)),
        ];
        let mut last_error = None;
        for attempt in 1..=REMOTE_COMMAND_MAX_ATTEMPTS {
            match run_bounded_command(&self.ssh_binary, &ssh_args, timeout, max_output_bytes) {
                Ok(output) => return Ok(output),
                Err(error)
                    if attempt < REMOTE_COMMAND_MAX_ATTEMPTS
                        && matches!(
                            error.kind(),
                            GitErrorKind::Timeout | GitErrorKind::Transport
                        ) =>
                {
                    last_error = Some(error);
                    self.close_control_master(&control_path);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| GitError::new("remote command failed")))
    }

    fn close_control_master(&self, control_path: &Path) {
        let args = [
            "-T".to_owned(),
            "-O".to_owned(),
            "exit".to_owned(),
            "-S".to_owned(),
            control_path.to_string_lossy().into_owned(),
            self.ssh_host.clone(),
        ];
        let _ = run_bounded_command(
            &self.ssh_binary,
            &args,
            self.command_timeout.min(REMOTE_CONTROL_SHUTDOWN_TIMEOUT),
            self.max_output_bytes,
        );
    }

    #[cfg(test)]
    fn with_test_process_options(
        mut self,
        ssh_binary: impl Into<PathBuf>,
        command_timeout: Duration,
        max_output_bytes: usize,
    ) -> Self {
        self.ssh_binary = ssh_binary.into();
        self.command_timeout = command_timeout;
        self.max_output_bytes = max_output_bytes;
        self
    }
}

pub fn pull_request_for_branch(
    repo_path: impl AsRef<Path>,
    branch_name: &str,
) -> Result<Option<PullRequestInfo>, GitError> {
    validate_remote_ref(branch_name)?;
    pull_request_for_branch_with_binary(repo_path.as_ref(), branch_name, Path::new("gh"))
}

pub fn github_cli_available() -> Result<bool, GitError> {
    let args = ["-lc".to_owned(), "command -v gh >/dev/null 2>&1".to_owned()];
    match run_bounded_command_at(
        Path::new("sh"),
        &args,
        None,
        GITHUB_CLI_TIMEOUT,
        GITHUB_CLI_MAX_OUTPUT_BYTES,
        "GitHub CLI check",
    ) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == GitErrorKind::Command => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn install_github_cli() -> Result<(), GitError> {
    let args = ["-lc".to_owned(), GITHUB_CLI_INSTALL_SCRIPT.to_owned()];
    run_bounded_command_at(
        Path::new("sh"),
        &args,
        None,
        GITHUB_CLI_INSTALL_TIMEOUT,
        GITHUB_CLI_MAX_OUTPUT_BYTES,
        "GitHub CLI installer",
    )?;
    github_cli_available()?.then_some(()).ok_or_else(|| {
        GitError::with_kind(
            GitErrorKind::Command,
            "GitHub CLI installation completed without making gh available",
        )
    })
}

pub fn session_dependency_available(session_type: SessionType) -> Result<bool, GitError> {
    let args = [
        "-lc".to_owned(),
        build_session_dependency_check_command(session_type),
    ];
    match run_bounded_command_at(
        Path::new("sh"),
        &args,
        None,
        SESSION_DEPENDENCY_CHECK_TIMEOUT,
        SESSION_DEPENDENCY_MAX_OUTPUT_BYTES,
        "session dependency check",
    ) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == GitErrorKind::Command => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn install_session_dependency(session_type: SessionType) -> Result<(), GitError> {
    let args = [
        "-lc".to_owned(),
        build_session_dependency_install_command(session_type),
    ];
    run_bounded_command_at(
        Path::new("sh"),
        &args,
        None,
        SESSION_DEPENDENCY_INSTALL_TIMEOUT,
        SESSION_DEPENDENCY_MAX_OUTPUT_BYTES,
        "session dependency installer",
    )?;
    session_dependency_available(session_type)?
        .then_some(())
        .ok_or_else(|| {
            GitError::with_kind(
                GitErrorKind::Command,
                format!(
                    "{} installation completed without making it available",
                    session_dependency_tool_name(session_type)
                ),
            )
        })
}

pub fn remote_branch_exists(
    repo_path: impl AsRef<Path>,
    branch_name: &str,
) -> Result<bool, GitError> {
    validate_remote_ref(branch_name)?;
    let repo_path = repo_path.as_ref().to_str().ok_or_else(|| {
        GitError::with_kind(
            GitErrorKind::InvalidInput,
            "repository path must be valid UTF-8",
        )
    })?;
    let remote_ref = format!("refs/heads/{branch_name}");
    let command = format!(
        "{git} >/dev/null; status=$?; \
         if test \"$status\" -eq 0; then printf exists; \
         elif test \"$status\" -eq 2; then printf missing; else exit \"$status\"; fi",
        git = build_remote_git_command(
            repo_path,
            &["ls-remote", "--exit-code", "--heads", "origin", &remote_ref],
        ),
    );
    let args = ["-lc".to_owned(), command];
    Ok(run_bounded_command_at(
        Path::new("sh"),
        &args,
        None,
        GITHUB_CLI_TIMEOUT,
        GITHUB_CLI_MAX_OUTPUT_BYTES,
        "Git remote branch check",
    )?
    .trim()
        == "exists")
}

pub fn pull_current_branch(repo_path: impl AsRef<Path>) -> Result<String, GitError> {
    let repo_path = repo_path.as_ref().to_str().ok_or_else(|| {
        GitError::with_kind(
            GitErrorKind::InvalidInput,
            "repository path must be valid UTF-8",
        )
    })?;
    let command = build_remote_git_command(repo_path, &["pull", "--ff-only"]);
    let args = ["-lc".to_owned(), command];
    run_bounded_command_at(
        Path::new("sh"),
        &args,
        None,
        GITHUB_CLI_TIMEOUT,
        GITHUB_CLI_MAX_OUTPUT_BYTES,
        "Git pull",
    )
}

const GITHUB_CLI_INSTALL_SCRIPT: &str = r#"if command -v gh >/dev/null 2>&1; then exit 0; fi
run_install() {
  if [ "$(id -u)" -eq 0 ]; then sh -lc "$1"; return $?; fi
  if command -v sudo >/dev/null 2>&1; then sudo -n sh -lc "$1"; return $?; fi
  return 1
}
if command -v brew >/dev/null 2>&1; then brew install gh && exit 0; fi
if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then run_install "type -p wget >/dev/null || (apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y wget) && mkdir -p -m 755 /etc/apt/keyrings && out=\$(mktemp) && wget -nv -O\$out https://cli.github.com/packages/githubcli-archive-keyring.gpg && cat \$out > /etc/apt/keyrings/githubcli-archive-keyring.gpg && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg && mkdir -p -m 755 /etc/apt/sources.list.d && echo \"deb [arch=\$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main\" > /etc/apt/sources.list.d/github-cli.list && apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y gh" && exit 0; fi
if command -v dnf >/dev/null 2>&1; then run_install "dnf install -y dnf5-plugins || dnf install -y \"dnf-command(config-manager)\" || true; dnf config-manager addrepo --from-repofile=https://cli.github.com/packages/rpm/gh-cli.repo || dnf config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo; dnf install -y gh --repo gh-cli || dnf install -y gh" && exit 0; fi
if command -v yum >/dev/null 2>&1; then run_install "type -p yum-config-manager >/dev/null || yum install -y yum-utils; yum-config-manager --add-repo https://cli.github.com/packages/rpm/gh-cli.repo && yum install -y gh" && exit 0; fi
if command -v zypper >/dev/null 2>&1; then run_install "zypper --non-interactive addrepo https://cli.github.com/packages/rpm/gh-cli.repo || true; zypper --non-interactive ref && zypper --non-interactive install gh" && exit 0; fi
if command -v pacman >/dev/null 2>&1; then run_install "pacman -Sy --noconfirm github-cli" && exit 0; fi
if command -v apk >/dev/null 2>&1; then run_install "apk add github-cli" && exit 0; fi
printf '%s\n' 'gh installation failed' >&2
exit 1"#;

fn pull_request_for_branch_with_binary(
    repo_path: &Path,
    branch_name: &str,
    binary: &Path,
) -> Result<Option<PullRequestInfo>, GitError> {
    let args = [
        "pr",
        "list",
        "--head",
        branch_name,
        "--state",
        "all",
        "--json",
        "url,state,mergedAt,updatedAt",
    ]
    .map(ToOwned::to_owned);
    match run_bounded_command_at(
        binary,
        &args,
        Some(repo_path),
        GITHUB_CLI_TIMEOUT,
        GITHUB_CLI_MAX_OUTPUT_BYTES,
        "GitHub CLI",
    ) {
        Ok(output) => parse_pull_request_list(&output),
        Err(error) if error.kind() == GitErrorKind::Spawn && is_missing_binary_error(&error) => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubPullRequestPayload {
    url: Option<String>,
    state: Option<String>,
    merged_at: Option<String>,
    updated_at: Option<String>,
}

fn parse_pull_request_list(output: &str) -> Result<Option<PullRequestInfo>, GitError> {
    if output.trim().is_empty() {
        return Ok(None);
    }
    let mut pull_requests =
        serde_json::from_str::<Vec<GitHubPullRequestPayload>>(output).map_err(|error| {
            GitError::with_kind(
                GitErrorKind::Command,
                format!("GitHub CLI returned invalid pull request JSON: {error}"),
            )
        })?;
    pull_requests.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    let Some(latest) = pull_requests.into_iter().next() else {
        return Ok(None);
    };
    let Some(url) = latest.url.filter(|url| !url.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(Some(PullRequestInfo {
        url,
        state: latest.state.unwrap_or_default(),
        merged_at: latest.merged_at,
        updated_at: latest.updated_at,
    }))
}

fn is_missing_github_cli_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    (lower.contains("gh:") && lower.contains("not found"))
        || lower.contains("gh: command not found")
        || lower.contains("command not found: gh")
}

fn is_missing_binary_error(error: &GitError) -> bool {
    error.to_string().contains("No such file or directory")
}

fn create_control_directory(path: &Path) -> Result<(), GitError> {
    fs::create_dir_all(path).map_err(|error| {
        GitError::with_kind(
            GitErrorKind::Spawn,
            format!("failed to create SSH control directory: {error}"),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            GitError::with_kind(
                GitErrorKind::Spawn,
                format!("failed to protect SSH control directory: {error}"),
            )
        })?;
    }
    Ok(())
}

fn run_bounded_command(
    binary: &Path,
    args: &[String],
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<String, GitError> {
    run_bounded_command_at(binary, args, None, timeout, max_output_bytes, "SSH")
}

fn run_bounded_command_at(
    binary: &Path,
    args: &[String],
    working_directory: Option<&Path>,
    timeout: Duration,
    max_output_bytes: usize,
    process_name: &str,
) -> Result<String, GitError> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env_clear()
        .envs(filter_ssh_child_environment(std::env::vars_os()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(working_directory) = working_directory {
        command.current_dir(working_directory);
    }
    let mut child = {
        let mut last_error = None;
        let mut child = None;
        for attempt in 0..3 {
            match command.spawn() {
                Ok(spawned) => {
                    child = Some(spawned);
                    break;
                }
                Err(error) if error.raw_os_error() == Some(26) && attempt < 2 => {
                    last_error = Some(error);
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    return Err(GitError::with_kind(
                        GitErrorKind::Spawn,
                        format!("failed to spawn {process_name}: {error}"),
                    ));
                }
            }
        }
        child.ok_or_else(|| {
            GitError::with_kind(
                GitErrorKind::Spawn,
                format!(
                    "failed to spawn {process_name}: {}",
                    last_error
                        .map(|error| error.to_string())
                        .unwrap_or_else(|| "unknown spawn error".to_owned())
                ),
            )
        })?
    };
    let stdout = child.stdout.take().ok_or_else(|| {
        GitError::with_kind(
            GitErrorKind::Spawn,
            format!("{process_name} stdout is unavailable"),
        )
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        GitError::with_kind(
            GitErrorKind::Spawn,
            format!("{process_name} stderr is unavailable"),
        )
    })?;
    let stdout_reader = thread::spawn(move || read_process_output(stdout, max_output_bytes));
    let stderr_reader = thread::spawn(move || read_process_output(stderr, max_output_bytes));
    let started_at = Instant::now();

    let status = loop {
        if started_at.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(GitError::with_kind(
                GitErrorKind::Timeout,
                format!(
                    "{process_name} command timed out after {} ms",
                    timeout.as_millis()
                ),
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(GitError::with_kind(
                    GitErrorKind::Spawn,
                    format!("failed to wait for {process_name}: {error}"),
                ));
            }
        }
    };
    let (stdout, stdout_exceeded) = stdout_reader
        .join()
        .map_err(|_| {
            GitError::with_kind(
                GitErrorKind::Spawn,
                format!("{process_name} stdout reader stopped unexpectedly"),
            )
        })?
        .map_err(|error| {
            GitError::with_kind(
                GitErrorKind::Spawn,
                format!("failed to read {process_name} stdout: {error}"),
            )
        })?;
    let (stderr, stderr_exceeded) = stderr_reader
        .join()
        .map_err(|_| {
            GitError::with_kind(
                GitErrorKind::Spawn,
                format!("{process_name} stderr reader stopped unexpectedly"),
            )
        })?
        .map_err(|error| {
            GitError::with_kind(
                GitErrorKind::Spawn,
                format!("failed to read {process_name} stderr: {error}"),
            )
        })?;
    if stdout_exceeded
        || stderr_exceeded
        || stdout.len().saturating_add(stderr.len()) > max_output_bytes
    {
        return Err(GitError::with_kind(
            GitErrorKind::OutputLimit,
            format!("{process_name} command output exceeded {max_output_bytes} bytes"),
        ));
    }
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr = String::from_utf8_lossy(&stderr).trim().to_owned();
    if status.success() {
        return Ok(stdout);
    }
    let kind = if status.code() == Some(255) {
        GitErrorKind::Transport
    } else {
        GitErrorKind::Command
    };
    Err(GitError::with_kind(
        kind,
        format!(
            "{process_name} command failed with status {}: {stderr}",
            status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string())
        ),
    ))
}

fn read_process_output(
    mut stream: impl Read,
    max_output_bytes: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut stored = Vec::new();
    let mut exceeded = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = max_output_bytes.saturating_sub(stored.len());
        stored.extend_from_slice(&buffer[..read.min(remaining)]);
        exceeded |= read > remaining;
    }
    Ok((stored, exceeded))
}

fn filter_ssh_child_environment(
    environment: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    const SAFE_KEYS: &[&str] = &[
        "HOME",
        "GH_CONFIG_DIR",
        "GH_ENTERPRISE_TOKEN",
        "GH_HOST",
        "GH_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GITHUB_TOKEN",
        "LANG",
        "LOGNAME",
        "NO_COLOR",
        "PATH",
        "SSH_AGENT_PID",
        "SSH_AUTH_SOCK",
        "TMPDIR",
        "USER",
    ];
    environment
        .into_iter()
        .filter(|(key, _)| {
            key.to_str()
                .is_some_and(|key| SAFE_KEYS.contains(&key) || key.starts_with("LC_"))
        })
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect()
}

fn quote_for_posix_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn build_remote_git_command(repo_path: &str, args: &[&str]) -> String {
    std::iter::once("git")
        .chain(std::iter::once("-C"))
        .chain(std::iter::once(repo_path))
        .chain(args.iter().copied())
        .map(quote_for_posix_shell)
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_remote_ref(reference: &str) -> Result<(), GitError> {
    if reference.is_empty() || reference.starts_with('-') || reference.chars().any(char::is_control)
    {
        return Err(GitError::with_kind(
            GitErrorKind::InvalidInput,
            "remote Git reference must be non-empty, non-option, and contain no control characters",
        ));
    }
    Ok(())
}

fn validate_remote_relative_path(file_path: &str) -> Result<(), GitError> {
    let path = Path::new(file_path);
    if file_path.is_empty()
        || path.is_absolute()
        || file_path.chars().any(char::is_control)
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(GitError::with_kind(
            GitErrorKind::InvalidInput,
            "remote file path must contain only relative normal components",
        ));
    }
    Ok(())
}

fn validate_remote_absolute_path(path: &str) -> Result<(), GitError> {
    if path.is_empty()
        || !path.starts_with('/')
        || path.chars().any(char::is_control)
        || Path::new(path)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(GitError::with_kind(
            GitErrorKind::InvalidInput,
            "remote HOME path must be absolute and contain no parent/control components",
        ));
    }
    Ok(())
}

fn remote_home_path_guard(path: &str, require_existing: bool) -> String {
    let existence_guard = if require_existing {
        "test -e \"$KANVIBE_PATH\" || { printf '%s' 'remote HOME path does not exist' >&2; exit 74; }; \
         test ! -L \"$KANVIBE_PATH\" || { printf '%s' 'remote HOME symlink targets are not allowed' >&2; exit 73; }; \
         if test -d \"$KANVIBE_PATH\"; then KANVIBE_REAL=$(cd \"$KANVIBE_PATH\" && pwd -P) || exit 71; \
         else KANVIBE_PARENT=$(cd \"$(dirname \"$KANVIBE_PATH\")\" && pwd -P) || exit 71; \
         KANVIBE_REAL=\"$KANVIBE_PARENT/$(basename \"$KANVIBE_PATH\")\"; fi; \
         case \"$KANVIBE_REAL/\" in \"$KANVIBE_HOME/\"*) ;; *) printf '%s' 'remote path resolves outside HOME' >&2; exit 72;; esac; "
    } else {
        ""
    };
    format!(
        "KANVIBE_PATH={path}; KANVIBE_HOME=$(cd && pwd -P) || exit 70; \
         case \"$KANVIBE_PATH/\" in \"$KANVIBE_HOME/\"*) ;; *) printf '%s' 'remote path escapes HOME' >&2; exit 72;; esac; \
         {existence_guard}",
        path = quote_for_posix_shell(path),
    )
}

fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(ALPHABET[(first >> 2) as usize] as char);
        encoded.push(ALPHABET[(((first & 0b11) << 4) | (second >> 4)) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[(((second & 0b1111) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[(third & 0b11_1111) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

pub fn exec_git(repo_path: impl AsRef<Path>, args: &[&str]) -> Result<String, GitError> {
    Ok(exec_git_raw(repo_path, args)?.trim().to_owned())
}

fn exec_git_raw(repo_path: impl AsRef<Path>, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path.as_ref())
        .args(args)
        .output()
        .map_err(|error| {
            GitError::with_kind(GitErrorKind::Spawn, format!("failed to spawn git: {error}"))
        })?;

    if !output.status.success() {
        return Err(GitError::new(format!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn validate_git_repo(repo_path: impl AsRef<Path>) -> Result<bool, GitError> {
    Ok(exec_git(repo_path, &["rev-parse", "--is-inside-work-tree"])?.trim() == "true")
}

pub fn scan_git_repositories(root_path: impl AsRef<Path>) -> Result<Vec<PathBuf>, GitError> {
    const MAX_DEPTH: usize = 4;

    let root_path = fs::canonicalize(root_path.as_ref())
        .map_err(|error| GitError::new(format!("invalid scan root: {error}")))?;
    if !root_path.is_dir() {
        return Err(GitError::new("scan root must be a directory"));
    }

    let mut pending = vec![(root_path, 0_usize)];
    let mut repositories = BTreeSet::new();
    while let Some((directory, depth)) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if entry.file_name() == ".git" && (file_type.is_dir() || file_type.is_file()) {
                if let Some(repository) = path.parent() {
                    repositories.insert(repository.to_path_buf());
                }
                continue;
            }
            if depth + 1 < MAX_DEPTH && file_type.is_dir() && !file_type.is_symlink() {
                pending.push((path, depth + 1));
            }
        }
    }

    Ok(repositories.into_iter().collect())
}

pub fn is_submodule_repository(repo_path: impl AsRef<Path>) -> Result<bool, GitError> {
    Ok(!exec_git(
        repo_path,
        &["rev-parse", "--show-superproject-working-tree"],
    )?
    .is_empty())
}

pub fn current_branch(repo_path: impl AsRef<Path>) -> Result<String, GitError> {
    exec_git(repo_path, &["branch", "--show-current"])
}

pub fn repository_root(repo_path: impl AsRef<Path>) -> Result<PathBuf, GitError> {
    let root = exec_git(repo_path, &["rev-parse", "--show-toplevel"])?;
    fs::canonicalize(&root)
        .map_err(|error| GitError::new(format!("invalid repository root: {error}")))
}

pub fn common_repository_root(repo_path: impl AsRef<Path>) -> Result<PathBuf, GitError> {
    let repo_path = repo_path.as_ref();
    let common_dir = exec_git(
        repo_path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common_dir = fs::canonicalize(&common_dir)
        .map_err(|error| GitError::new(format!("invalid Git common directory: {error}")))?;
    if common_dir.file_name().and_then(|name| name.to_str()) == Some(".git") {
        return common_dir
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| GitError::new("Git common directory has no repository parent"));
    }
    repository_root(repo_path)
}

pub fn default_branch(repo_path: impl AsRef<Path>) -> Result<String, GitError> {
    let repo_path = repo_path.as_ref();
    if let Ok(reference) = exec_git(
        repo_path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) && let Some((_, branch)) = reference.split_once('/')
        && !branch.trim().is_empty()
    {
        return Ok(branch.to_owned());
    }
    let branch = current_branch(repo_path)?;
    if branch.is_empty() {
        return Err(GitError::new(
            "repository has no current branch or origin default branch",
        ));
    }
    Ok(branch)
}

pub fn ensure_git_exclude_pattern(
    repo_path: impl AsRef<Path>,
    pattern: &str,
) -> Result<(), GitError> {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.contains('\n') || pattern.contains('\r') {
        return Err(GitError::new(
            "Git exclude pattern must be one non-empty line",
        ));
    }
    let exclude_path = PathBuf::from(exec_git(
        repo_path.as_ref(),
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/exclude",
        ],
    )?);
    let current = match fs::read_to_string(&exclude_path) {
        Ok(current) => current,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(GitError::new(format!(
                "failed to read Git exclude file: {error}"
            )));
        }
    };
    if current.lines().any(|line| line.trim() == pattern) {
        return Ok(());
    }
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            GitError::new(format!("failed to create Git info directory: {error}"))
        })?;
    }
    let separator = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs::write(&exclude_path, format!("{current}{separator}{pattern}\n"))
        .map_err(|error| GitError::new(format!("failed to update Git exclude file: {error}")))
}

pub fn list_branches(repo_path: impl AsRef<Path>) -> Result<Vec<String>, GitError> {
    let output = exec_git(repo_path, &["branch", "--format=%(refname:short)"])?;

    Ok(output
        .lines()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

pub fn format_session_name(project_name: &str, branch_name: &str) -> String {
    format!("{project_name}-{branch_name}").replace('/', "-")
}

pub fn build_managed_worktree_path(project_path: impl AsRef<Path>, branch_name: &str) -> PathBuf {
    let project_path = project_path.as_ref();
    let project_name = project_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let worktree_base = project_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{project_name}__worktrees"));

    worktree_base.join(branch_name.replace('/', "-"))
}

pub fn create_worktree_with_session(
    project_path: impl AsRef<Path>,
    branch_name: &str,
    base_branch: &str,
    session_type: SessionType,
) -> Result<WorktreeSession, GitError> {
    let project_path = project_path.as_ref();
    let worktree_path = build_managed_worktree_path(project_path, branch_name);
    let project_name = project_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project");
    let session_name = format_session_name(project_name, branch_name);
    let worktree_path_arg = worktree_path.to_string_lossy().to_string();

    exec_git(
        project_path,
        &[
            "worktree",
            "add",
            &worktree_path_arg,
            "-b",
            branch_name,
            base_branch,
        ],
    )?;

    Ok(WorktreeSession {
        worktree_path,
        session_name,
        session_type,
    })
}

pub fn list_worktrees(project_path: impl AsRef<Path>) -> Result<Vec<RegisteredWorktree>, GitError> {
    let output = exec_git(project_path, &["worktree", "list", "--porcelain"])?;
    Ok(parse_worktree_porcelain(&output))
}

/// Remove only a Git-registered linked worktree for `branch_name`, then remove the branch.
///
/// The repository root checkout is never removed. `expected_worktree_path` is a consistency
/// check, not an alternate deletion target, so an arbitrary or stale path cannot be deleted.
pub fn remove_worktree_and_branch(
    project_path: impl AsRef<Path>,
    branch_name: &str,
    expected_worktree_path: Option<&Path>,
) -> Result<(), GitError> {
    let project_path = fs::canonicalize(project_path.as_ref())
        .map_err(|error| GitError::new(format!("invalid project path: {error}")))?;
    let worktrees = list_worktrees(&project_path)?;
    let root = worktrees
        .first()
        .map(|worktree| worktree.path.as_path())
        .unwrap_or(project_path.as_path());
    let matching = worktrees
        .iter()
        .filter(|worktree| !worktree.is_bare && worktree.branch.as_deref() == Some(branch_name));
    let root_has_branch = matching
        .clone()
        .any(|worktree| paths_refer_to_same_location(&worktree.path, root));
    let linked = matching
        .filter(|worktree| !paths_refer_to_same_location(&worktree.path, root))
        .find(|worktree| {
            expected_worktree_path
                .is_none_or(|expected| paths_refer_to_same_location(&worktree.path, expected))
        });

    if expected_worktree_path.is_some() && linked.is_none() && !root_has_branch {
        return Err(GitError::new(
            "expected worktree is not registered for the requested branch",
        ));
    }

    if let Some(linked) = linked {
        let path = linked.path.to_string_lossy().into_owned();
        exec_git(&project_path, &["worktree", "remove", &path, "--force"])?;
    }

    if root_has_branch {
        return Ok(());
    }

    let branch_ref = format!("refs/heads/{branch_name}");
    let branch_exists = Command::new("git")
        .arg("-C")
        .arg(&project_path)
        .args(["show-ref", "--verify", "--quiet", &branch_ref])
        .status()
        .map_err(|error| GitError::new(format!("failed to inspect branch: {error}")))?
        .success();
    if branch_exists {
        exec_git(&project_path, &["branch", "-D", branch_name])?;
    }
    Ok(())
}

pub fn changed_files(
    worktree_path: impl AsRef<Path>,
    base_branch: &str,
    branch_name: &str,
) -> Result<Vec<DiffFile>, GitError> {
    let worktree_path = worktree_path.as_ref();
    let range = format!("{base_branch}...{branch_name}");
    let name_status = exec_git(worktree_path, &["diff", &range, "--name-status"])
        .or_else(|_| Ok::<String, GitError>(String::new()))?;
    let numstat = exec_git(worktree_path, &["diff", &range, "--numstat"])
        .or_else(|_| Ok::<String, GitError>(String::new()))?;
    let status = exec_git_raw(
        worktree_path,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    Ok(parse_changed_files(&name_status, &numstat, &status))
}

pub fn original_file_content(
    worktree_path: impl AsRef<Path>,
    base_branch: &str,
    file_path: &str,
) -> Result<String, GitError> {
    validate_relative_path(worktree_path.as_ref(), file_path)?;
    exec_git_raw(
        worktree_path,
        &["show", &format!("{base_branch}:{file_path}")],
    )
    .or_else(|_| Ok(String::new()))
}

pub fn file_content(worktree_path: impl AsRef<Path>, file_path: &str) -> Result<String, GitError> {
    let resolved_path = validate_relative_path(worktree_path.as_ref(), file_path)?;
    fs::read_to_string(&resolved_path).map_err(|error| {
        GitError::new(format!(
            "failed to read {}: {error}",
            resolved_path.display()
        ))
    })
}

pub fn save_file_content(
    worktree_path: impl AsRef<Path>,
    file_path: &str,
    content: &str,
) -> Result<(), GitError> {
    let resolved_path = validate_relative_path(worktree_path.as_ref(), file_path)?;

    fs::write(&resolved_path, content).map_err(|error| {
        GitError::new(format!(
            "failed to write {}: {error}",
            resolved_path.display()
        ))
    })
}

pub fn save_file_content_if_unchanged(
    worktree_path: impl AsRef<Path>,
    file_path: &str,
    expected_current: &str,
    content: &str,
) -> Result<(), GitError> {
    let worktree_path = worktree_path.as_ref();
    let current = file_content(worktree_path, file_path)?;
    if current != expected_current {
        return Err(GitError::new(
            "file changed on disk after it was loaded; reload before saving",
        ));
    }
    save_file_content(worktree_path, file_path, content)
}

fn validate_relative_path(worktree_path: &Path, file_path: &str) -> Result<PathBuf, GitError> {
    let file_path = Path::new(file_path);
    if file_path.is_absolute() {
        return Err(GitError::new("absolute file paths are not allowed"));
    }

    let worktree_path = fs::canonicalize(worktree_path)
        .map_err(|error| GitError::new(format!("invalid worktree path: {error}")))?;
    let resolved_path = worktree_path.join(file_path);
    let parent = resolved_path
        .parent()
        .ok_or_else(|| GitError::new("file path has no parent"))?;
    let canonical_parent = fs::canonicalize(parent)
        .map_err(|error| GitError::new(format!("invalid file parent: {error}")))?;

    if !canonical_parent.starts_with(&worktree_path) {
        return Err(GitError::new("file path escapes worktree"));
    }
    if resolved_path.exists() {
        let canonical_file = fs::canonicalize(&resolved_path)
            .map_err(|error| GitError::new(format!("invalid file path: {error}")))?;
        if !canonical_file.starts_with(&worktree_path) {
            return Err(GitError::new(
                "file path escapes worktree through a symbolic link",
            ));
        }
    }

    Ok(resolved_path)
}

fn parse_worktree_porcelain(output: &str) -> Vec<RegisteredWorktree> {
    output
        .split("\n\n")
        .filter_map(|block| {
            let mut path = None;
            let mut branch = None;
            let mut is_bare = false;
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(value));
                } else if let Some(value) = line.strip_prefix("branch refs/heads/") {
                    branch = Some(value.to_owned());
                } else if line == "bare" {
                    is_bare = true;
                }
            }
            path.map(|path| RegisteredWorktree {
                path,
                branch,
                is_bare,
            })
        })
        .collect()
}

fn paths_refer_to_same_location(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn parse_changed_files(name_status: &str, numstat: &str, status: &str) -> Vec<DiffFile> {
    let mut files = Vec::<DiffFile>::new();
    let stats = parse_numstat(numstat);

    for line in name_status
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let parts = line.split('\t').collect::<Vec<_>>();
        if parts.len() < 2 {
            continue;
        }
        let file_path = if parts.len() >= 3 { parts[2] } else { parts[1] };
        let (additions, deletions, is_binary) = stats
            .iter()
            .find(|(path, _, _, _)| path == file_path)
            .map(|(_, additions, deletions, is_binary)| (*additions, *deletions, *is_binary))
            .unwrap_or((0, 0, false));

        files.push(DiffFile {
            path: file_path.to_owned(),
            status: parse_git_status(parts[0]),
            additions,
            deletions,
            is_binary,
        });
    }

    for line in status.lines() {
        if line.len() < 4 {
            continue;
        }
        let file_path = line[3..].to_owned();
        if files.iter().any(|file| file.path == file_path) {
            continue;
        }

        if let Some(status) = parse_working_tree_status(&line[0..2]) {
            files.push(DiffFile {
                path: file_path,
                status,
                additions: 0,
                deletions: 0,
                is_binary: false,
            });
        }
    }

    files
}

fn parse_numstat(output: &str) -> Vec<(String, u32, u32, bool)> {
    output
        .lines()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() < 3 {
                return None;
            }

            Some((
                parts[2..].join("\t"),
                parts[0].parse::<u32>().unwrap_or(0),
                parts[1].parse::<u32>().unwrap_or(0),
                parts[0] == "-" || parts[1] == "-",
            ))
        })
        .collect()
}

fn parse_git_status(status: &str) -> DiffFileStatus {
    if status.starts_with('R') {
        return DiffFileStatus::Renamed;
    }

    match status.chars().next() {
        Some('A') => DiffFileStatus::Added,
        Some('D') => DiffFileStatus::Deleted,
        Some('M') => DiffFileStatus::Modified,
        _ => DiffFileStatus::Modified,
    }
}

fn parse_working_tree_status(xy: &str) -> Option<DiffFileStatus> {
    let mut chars = xy.chars();
    let index = chars.next()?;
    let worktree = chars.next()?;

    if xy == "??" {
        return Some(DiffFileStatus::Added);
    }
    if index == 'A' || worktree == 'A' {
        return Some(DiffFileStatus::Added);
    }
    if index == 'D' || worktree == 'D' {
        return Some(DiffFileStatus::Deleted);
    }
    if index == 'M' || worktree == 'M' {
        return Some(DiffFileStatus::Modified);
    }
    if index == 'R' || worktree == 'R' {
        return Some(DiffFileStatus::Renamed);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn branch_ref_preserves_branch_name() {
        let branch = BranchRef::new("feature/native-board");

        assert_eq!(branch.as_str(), "feature/native-board");
    }

    #[test]
    fn pull_request_parser_selects_latest_updated_entry() {
        let parsed = parse_pull_request_list(
            r#"[
                {"url":"https://github.com/acme/repo/pull/1","state":"OPEN","mergedAt":null,"updatedAt":"2026-07-20T00:00:00Z"},
                {"url":"https://github.com/acme/repo/pull/2","state":"MERGED","mergedAt":"2026-07-28T12:00:00Z","updatedAt":"2026-07-28T12:00:00Z"}
            ]"#,
        )
        .expect("valid gh payload")
        .expect("latest pull request");

        assert_eq!(parsed.url, "https://github.com/acme/repo/pull/2");
        assert_eq!(parsed.state, "MERGED");
        assert_eq!(parsed.merged_at.as_deref(), Some("2026-07-28T12:00:00Z"));
    }

    #[cfg(unix)]
    #[test]
    fn local_pull_request_lookup_executes_bounded_gh_contract() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = create_remote_ssh_fixture("local-gh", "exit 0");
        let binary = fixture.join("gh");
        let arguments = fixture.join("arguments.txt");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' '[{{\"url\":\"https://github.com/acme/repo/pull/310\",\"state\":\"OPEN\",\"mergedAt\":null,\"updatedAt\":\"2026-07-29T00:00:00Z\"}}]'\n",
                arguments.display()
            ),
        )
        .expect("write fake gh");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).expect("chmod fake gh");

        let pull_request =
            pull_request_for_branch_with_binary(&fixture, "perf/rust-backend", &binary)
                .expect("query pull request")
                .expect("pull request");

        assert_eq!(pull_request.url, "https://github.com/acme/repo/pull/310");
        assert_eq!(
            fs::read_to_string(arguments).expect("captured arguments"),
            "pr\nlist\n--head\nperf/rust-backend\n--state\nall\n--json\nurl,state,mergedAt,updatedAt\n"
        );
        fs::remove_dir_all(fixture).expect("remove fixture");
    }

    #[test]
    fn active_branch_pull_checks_origin_and_fast_forwards_only() {
        let repo = create_temp_git_repo("pull-sync");
        write_file(repo.join("README.md"), "first\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        let fixture_root = repo.parent().expect("fixture root");
        let bare = fixture_root.join("origin.git");
        fs::create_dir_all(&bare).expect("bare remote directory");
        git_raw(&bare, &["init", "--bare"]);
        git(
            &repo,
            &["remote", "add", "origin", bare.to_string_lossy().as_ref()],
        );
        git(&repo, &["push", "-u", "origin", "main"]);
        let upstream = fixture_root.join("upstream");
        let clone = Command::new("git")
            .args([
                "clone",
                "-b",
                "main",
                bare.to_string_lossy().as_ref(),
                upstream.to_string_lossy().as_ref(),
            ])
            .output()
            .expect("clone upstream");
        assert!(clone.status.success());
        git(&upstream, &["config", "user.email", "qa@kanvibe.test"]);
        git(&upstream, &["config", "user.name", "KanVibe QA"]);
        write_file(upstream.join("README.md"), "second\n");
        git(&upstream, &["add", "."]);
        git(&upstream, &["commit", "-m", "remote update"]);
        git(&upstream, &["push", "origin", "main"]);

        assert!(remote_branch_exists(&repo, "main").expect("origin main exists"));
        assert!(!remote_branch_exists(&repo, "missing").expect("missing branch"));
        let updated = pull_current_branch(&repo).expect("fast-forward pull");
        assert!(!updated.to_ascii_lowercase().contains("already up to date"));
        assert_eq!(
            fs::read_to_string(repo.join("README.md")).expect("pulled file"),
            "second\n"
        );
        let noop = pull_current_branch(&repo).expect("no-op pull");
        assert!(
            noop.to_ascii_lowercase().contains("already up to date")
                || noop.to_ascii_lowercase().contains("already up-to-date")
        );
        fs::remove_dir_all(fixture_root).expect("remove pull fixture");
    }

    #[test]
    fn github_cli_install_policy_is_non_interactive_and_covers_supported_managers() {
        assert!(GITHUB_CLI_INSTALL_SCRIPT.contains("sudo -n"));
        for manager in ["brew", "apt-get", "dnf", "yum", "zypper", "pacman", "apk"] {
            assert!(
                GITHUB_CLI_INSTALL_SCRIPT.contains(&format!("command -v {manager}")),
                "missing {manager} install policy"
            );
        }
        assert!(GITHUB_CLI_INSTALL_SCRIPT.contains("githubcli-archive-keyring.gpg"));
    }

    #[test]
    fn worktree_path_and_session_name_match_electron_contract() {
        let project_path = Path::new("/tmp/kanvibe");

        assert_eq!(
            build_managed_worktree_path(project_path, "qa/branch-from-task"),
            PathBuf::from("/tmp/kanvibe__worktrees/qa-branch-from-task")
        );
        assert_eq!(
            format_session_name("kanvibe", "qa/branch-from-task"),
            "kanvibe-qa-branch-from-task"
        );
    }

    #[test]
    fn repository_scan_finds_nested_git_roots_without_following_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "kanvibe-git-scan-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let first = root.join("team/api");
        let second = root.join("tools/cli");
        let too_deep = root.join("one/two/three/four");
        fs::create_dir_all(&first).expect("first repo");
        fs::create_dir_all(&second).expect("second repo");
        fs::create_dir_all(&too_deep).expect("deep repo");
        git_raw(&first, &["init", "-b", "main"]);
        git_raw(&second, &["init", "-b", "main"]);
        git_raw(&too_deep, &["init", "-b", "main"]);
        fs::create_dir_all(root.join("plain/directory")).expect("plain directory");

        let repositories = scan_git_repositories(&root).expect("scan repositories");

        assert_eq!(repositories, vec![first, second]);
        fs::remove_dir_all(root).expect("remove scan fixture");
    }

    #[test]
    fn changed_files_include_branch_diff_and_working_tree_entries() {
        let repo = create_temp_git_repo("diff-files");
        write_file(repo.join("src/app.rs"), "fn main() {}\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        git(&repo, &["checkout", "-b", "feat/native-diff"]);
        write_file(
            repo.join("src/app.rs"),
            "fn main() {\n    println!(\"native\");\n}\n",
        );
        git(&repo, &["add", "src/app.rs"]);
        git(&repo, &["commit", "-m", "modify app"]);
        write_file(repo.join("src/new.rs"), "pub fn added() {}\n");
        fs::write(repo.join("src/image.bin"), [0, 159, 146, 150])
            .expect("write binary diff fixture");
        git(&repo, &["add", "src/image.bin"]);
        git(&repo, &["commit", "-m", "add binary"]);

        let files = changed_files(&repo, "main", "feat/native-diff").expect("changed files");
        assert!(
            files
                .iter()
                .any(|file| file.path == "src/app.rs" && file.status == DiffFileStatus::Modified)
        );
        assert!(
            files
                .iter()
                .any(|file| file.path == "src/new.rs" && file.status == DiffFileStatus::Added)
        );
        assert!(
            files
                .iter()
                .any(|file| file.path == "src/image.bin" && file.is_binary)
        );

        assert_eq!(
            original_file_content(&repo, "main", "src/app.rs").expect("original content"),
            "fn main() {}\n"
        );
        assert!(
            file_content(&repo, "src/app.rs")
                .expect("current content")
                .contains("native")
        );
        save_file_content(&repo, "src/app.rs", "fn main() { println!(\"saved\"); }\n")
            .expect("save content");
        assert!(
            file_content(&repo, "src/app.rs")
                .expect("saved content")
                .contains("saved")
        );
    }

    #[test]
    fn worktree_creation_matches_branch_from_task_flow() {
        let repo = create_temp_git_repo("worktree-flow");
        write_file(repo.join("README.md"), "# KanVibe\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);

        let session =
            create_worktree_with_session(&repo, "qa/branch-from-task", "main", SessionType::Tmux)
                .expect("worktree should be created");

        assert!(session.worktree_path.exists());
        assert_eq!(session.session_name, "worktree-flow-qa-branch-from-task");
        assert_eq!(
            current_branch(&session.worktree_path).expect("worktree branch"),
            "qa/branch-from-task"
        );
    }

    #[test]
    fn worktree_cleanup_removes_only_registered_linked_checkout_and_branch() {
        let repo = create_temp_git_repo("worktree-cleanup");
        write_file(repo.join("README.md"), "# KanVibe\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        let session = create_worktree_with_session(&repo, "qa/cleanup", "main", SessionType::Tmux)
            .expect("worktree should be created");

        remove_worktree_and_branch(&repo, "qa/cleanup", Some(&session.worktree_path))
            .expect("managed worktree cleanup");

        assert!(!session.worktree_path.exists());
        assert!(
            !list_branches(&repo)
                .expect("branches")
                .contains(&"qa/cleanup".to_owned())
        );
        assert!(repo.exists());
    }

    #[test]
    fn worktree_cleanup_rejects_unregistered_path_and_preserves_project_root_branch() {
        let repo = create_temp_git_repo("worktree-cleanup-guards");
        write_file(repo.join("README.md"), "# KanVibe\n");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        git(&repo, &["branch", "qa/unregistered"]);
        let unrelated = repo
            .parent()
            .expect("repo parent")
            .join("unregistered-worktree");

        let error = remove_worktree_and_branch(&repo, "qa/unregistered", Some(unrelated.as_path()))
            .expect_err("unregistered path must be rejected");
        assert!(error.to_string().contains("not registered"));
        assert!(
            list_branches(&repo)
                .expect("branches")
                .contains(&"qa/unregistered".to_owned())
        );

        remove_worktree_and_branch(&repo, "main", Some(repo.as_path()))
            .expect("project root checkout must be preserved");
        assert!(repo.exists());
        assert_eq!(current_branch(&repo).expect("current branch"), "main");
    }

    #[test]
    fn guarded_file_save_rejects_external_changes() {
        let repo = create_temp_git_repo("guarded-save");
        write_file(repo.join("src/app.rs"), "first\n");

        save_file_content_if_unchanged(&repo, "src/app.rs", "first\n", "second\n")
            .expect("unchanged file saves");
        let error = save_file_content_if_unchanged(&repo, "src/app.rs", "first\n", "third\n")
            .expect_err("stale editor content must be rejected");

        assert!(error.to_string().contains("changed on disk"));
        assert_eq!(
            file_content(&repo, "src/app.rs").expect("saved file"),
            "second\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_reuses_control_path_and_quotes_untrusted_arguments() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = create_remote_ssh_fixture(
            "remote-worktrees",
            "printf 'worktree /remote/repo\\nHEAD abc\\nbranch refs/heads/main\\n\\nworktree /remote/repo__worktrees/feature-login\\nHEAD def\\nbranch refs/heads/feature/login\\n\\n'",
        );
        let capture_path = fixture.join("ssh-args.txt");
        let ssh_binary = fixture.join("ssh-capture");
        fs::write(
            &ssh_binary,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n{}",
                capture_path.display(),
                fixture.join("response.sh").display()
            ),
        )
        .expect("write fake ssh");
        fs::set_permissions(&ssh_binary, fs::Permissions::from_mode(0o700))
            .expect("make fake ssh executable");

        let injected_path = fixture.join("injected");
        let hostile_repo_path = format!(
            "/remote/repo'; touch '{}'; '",
            injected_path.to_string_lossy()
        );
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid remote host")
            .with_test_process_options(ssh_binary, Duration::from_secs(1), 1024 * 1024);
        let worktrees = client
            .list_worktrees(&hostile_repo_path)
            .expect("remote worktree list");

        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[1].branch.as_deref(), Some("feature/login"));
        let args = fs::read_to_string(capture_path).expect("captured ssh args");
        assert!(args.contains("ControlMaster=auto"));
        assert!(args.contains("ControlPersist=10m"));
        assert!(args.contains("ControlPath="));
        assert!(args.contains("remote-host"));
        assert!(
            args.lines()
                .last()
                .is_some_and(|line| line.starts_with("sh -lc "))
        );

        let command =
            build_remote_git_command(&hostile_repo_path, &["worktree", "list", "--porcelain"]);
        let _ = Command::new("sh")
            .args(["-lc", &command])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        assert!(!injected_path.exists());
        fs::remove_dir_all(fixture).expect("remove remote worktree fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_cleanup_removes_only_registered_linked_checkout_then_branch() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = create_remote_ssh_fixture("remote-cleanup", "exit 1");
        let capture_path = fixture.join("commands.txt");
        let ssh_path = fixture.join("ssh-cleanup");
        fs::write(
            &ssh_path,
            format!(
                "#!/bin/sh\n\
                 for argument do command=$argument; done\n\
                 printf '%s\\n' \"$command\" >> '{}'\n\
                 case \"$command\" in\n\
                   *worktree*list*porcelain*)\n\
                     printf 'worktree /remote/repo\\nHEAD abc\\nbranch refs/heads/main\\n\\nworktree /remote/repo__worktrees/feature-cleanup\\nHEAD def\\nbranch refs/heads/feature/cleanup\\n\\n';;\n\
                   *worktree*remove*/remote/repo__worktrees/feature-cleanup*force*) exit 0;;\n\
                   *show-ref*verify*quiet*refs/heads/feature/cleanup*) exit 0;;\n\
                   *branch*D*feature/cleanup*) exit 0;;\n\
                   *) exit 9;;\n\
                 esac\n",
                capture_path.display(),
            ),
        )
        .expect("write cleanup fake ssh");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make cleanup fake ssh executable");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid cleanup client")
            .with_test_process_options(ssh_path, Duration::from_secs(1), 1024 * 1024);

        client
            .remove_worktree_and_branch(
                "/remote/repo",
                "feature/cleanup",
                Some(Path::new("/remote/repo__worktrees/feature-cleanup")),
            )
            .expect("registered remote worktree cleanup");

        let commands = fs::read_to_string(capture_path).expect("read captured cleanup commands");
        let list_index = commands.find("worktree").expect("list command");
        let remove_index = commands
            .find("/remote/repo__worktrees/feature-cleanup")
            .expect("remove command");
        let branch_index = commands.rfind("feature/cleanup").expect("branch command");
        assert!(list_index < remove_index);
        assert!(remove_index < branch_index);
        assert_eq!(
            commands
                .lines()
                .filter(|command| command.contains("worktree") && command.contains("remove"))
                .count(),
            1
        );
        fs::remove_dir_all(fixture).expect("remove cleanup fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_cleanup_rejects_unregistered_expected_path_without_mutation() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = create_remote_ssh_fixture("remote-cleanup-guard", "exit 1");
        let capture_path = fixture.join("commands.txt");
        let ssh_path = fixture.join("ssh-cleanup-guard");
        fs::write(
            &ssh_path,
            format!(
                "#!/bin/sh\n\
                 for argument do command=$argument; done\n\
                 printf '%s\\n' \"$command\" >> '{}'\n\
                 case \"$command\" in\n\
                   *worktree*list*porcelain*)\n\
                     printf 'worktree /remote/repo\\nHEAD abc\\nbranch refs/heads/main\\n\\nworktree /remote/repo__worktrees/feature-cleanup\\nHEAD def\\nbranch refs/heads/feature/cleanup\\n\\n';;\n\
                   *) exit 9;;\n\
                 esac\n",
                capture_path.display(),
            ),
        )
        .expect("write guarded cleanup fake ssh");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make guarded cleanup fake ssh executable");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid guarded cleanup client")
            .with_test_process_options(ssh_path, Duration::from_secs(1), 1024 * 1024);

        let error = client
            .remove_worktree_and_branch(
                "/remote/repo",
                "feature/cleanup",
                Some(Path::new("/remote/unrelated")),
            )
            .expect_err("unregistered expected path must be rejected");

        assert_eq!(error.kind(), GitErrorKind::InvalidInput);
        let commands = fs::read_to_string(&capture_path).expect("read guarded commands");
        assert_eq!(commands.lines().count(), 1);
        assert!(commands.contains("worktree"));
        assert!(commands.contains("porcelain"));
        assert!(!commands.contains("remove"));
        assert!(!commands.contains("feature/cleanup"));

        let root_error = client
            .remove_worktree_and_branch(
                "/remote/repo",
                "main",
                Some(Path::new("/remote/unrelated")),
            )
            .expect_err("a root branch cannot authorize an unrelated expected path");
        assert_eq!(root_error.kind(), GitErrorKind::InvalidInput);

        client
            .remove_worktree_and_branch("/remote/repo", "main", Some(Path::new("/remote/repo")))
            .expect("the exact project root must be preserved without mutation");
        let commands = fs::read_to_string(capture_path).expect("read root guard commands");
        assert_eq!(commands.lines().count(), 3);
        assert!(!commands.contains("remove"));
        fs::remove_dir_all(fixture).expect("remove guarded cleanup fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_diff_reads_exact_content_and_guards_save_paths() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = create_remote_ssh_fixture("remote-diff", "exit 1");
        let capture_path = fixture.join("commands.txt");
        let ssh_path = fixture.join("ssh-diff");
        fs::write(
            &ssh_path,
            format!(
                "#!/bin/sh\n\
                 for argument do command=$argument; done\n\
                 printf '%s\\n' \"$command\" >> '{}'\n\
                 case \"$command\" in\n\
                   *diff*main...feature/diff*name-status*) printf 'M\\tsrc/app.rs\\nA\\tsrc/new.rs\\n';;\n\
                   *diff*main...feature/diff*numstat*) printf '2\\t1\\tsrc/app.rs\\n1\\t0\\tsrc/new.rs\\n';;\n\
                   *status*porcelain*) printf '?? notes.txt\\n';;\n\
                   *show*main:src/app.rs*) printf 'base content\\n\\n';;\n\
                   *src*app.rs*KANVIBE_FILE_PRESENT*) printf '__KANVIBE_FILE_PRESENT__current content\\n\\n';;\n\
                   *KANVIBE_EXPECTED_CONTENT*) exit 0;;\n\
                   *) exit 9;;\n\
                 esac\n",
                capture_path.display(),
            ),
        )
        .expect("write remote diff fake ssh");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make remote diff fake ssh executable");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid diff client")
            .with_test_process_options(ssh_path, Duration::from_secs(1), 1024 * 1024);

        let files = client
            .changed_files("/remote/repo", "main", "feature/diff")
            .expect("remote changed files");
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src/app.rs");
        assert_eq!((files[0].additions, files[0].deletions), (2, 1));
        assert_eq!(files[2].path, "notes.txt");
        assert_eq!(
            client
                .original_file_content("/remote/repo", "main", "src/app.rs")
                .expect("remote original"),
            "base content\n\n"
        );
        assert_eq!(
            client
                .file_content("/remote/repo", "src/app.rs")
                .expect("remote current"),
            "current content\n\n"
        );
        client
            .save_file_content_if_unchanged(
                "/remote/repo",
                "src/app.rs",
                "current content\n\n",
                "saved\n",
            )
            .expect("guarded remote save");

        let commands_before_rejection = fs::read_to_string(&capture_path)
            .expect("read diff commands before rejection")
            .lines()
            .count();
        let error = client
            .file_content("/remote/repo", "../outside")
            .expect_err("remote traversal must fail before SSH");
        assert_eq!(error.kind(), GitErrorKind::InvalidInput);
        assert_eq!(
            fs::read_to_string(&capture_path)
                .expect("read diff commands after rejection")
                .lines()
                .count(),
            commands_before_rejection
        );
        fs::remove_dir_all(fixture).expect("remove remote diff fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_save_surfaces_conflict_and_rejects_oversized_payload_before_ssh() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = create_remote_ssh_fixture("remote-save-conflict", "exit 1");
        let capture_path = fixture.join("commands.txt");
        let ssh_path = fixture.join("ssh-save-conflict");
        fs::write(
            &ssh_path,
            format!(
                "#!/bin/sh\n\
                 for argument do command=$argument; done\n\
                 printf '%s\\n' \"$command\" >> '{}'\n\
                 printf '%s' 'KANVIBE_CONFLICT' >&2\n\
                 exit 75\n",
                capture_path.display(),
            ),
        )
        .expect("write conflict fake ssh");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make conflict fake ssh executable");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid conflict client")
            .with_test_process_options(ssh_path, Duration::from_secs(1), 1024 * 1024);

        let conflict = client
            .save_file_content_if_unchanged("/remote/repo", "src/app.rs", "old", "new")
            .expect_err("remote conflict must surface");
        assert_eq!(conflict.kind(), GitErrorKind::Command);
        assert!(conflict.to_string().contains("changed on remote host"));
        let command_count = fs::read_to_string(&capture_path)
            .expect("read conflict command")
            .lines()
            .count();
        let oversized = "x".repeat(REMOTE_EDIT_MAX_BYTES + 1);
        let oversized_error = client
            .save_file_content_if_unchanged("/remote/repo", "src/app.rs", "old", &oversized)
            .expect_err("oversized editor payload must fail locally");
        assert_eq!(oversized_error.kind(), GitErrorKind::OutputLimit);
        assert_eq!(
            fs::read_to_string(&capture_path)
                .expect("read commands after oversized rejection")
                .lines()
                .count(),
            command_count
        );
        fs::remove_dir_all(fixture).expect("remove conflict fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_save_shell_preserves_quoted_paths_and_rejects_symlink_targets() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let fixture = create_remote_ssh_fixture("remote-save-shell", "exit 1");
        let ssh_path = fixture.join("ssh-save-shell");
        fs::write(
            &ssh_path,
            "#!/bin/sh\nfor argument do command=$argument; done\neval \"$command\"\n",
        )
        .expect("write executing fake ssh");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make executing fake ssh executable");
        let worktree = fixture.join("repo's quoted path");
        fs::create_dir_all(worktree.join("src")).expect("create quoted worktree");
        let target = worktree.join("src/app.rs");
        fs::write(&target, "old\n").expect("write remote target");
        let outside = fixture.join("outside.txt");
        fs::write(&outside, "outside\n").expect("write outside target");
        symlink(&outside, worktree.join("src/link.rs")).expect("create remote symlink");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid shell client")
            .with_test_process_options(ssh_path, Duration::from_secs(2), 1024 * 1024);
        let worktree_path = worktree.to_string_lossy();

        client
            .save_file_content_if_unchanged(&worktree_path, "src/app.rs", "old\n", "saved\n")
            .expect("quoted remote path save");
        assert_eq!(
            fs::read_to_string(&target).expect("read saved target"),
            "saved\n"
        );

        let conflict = client
            .save_file_content_if_unchanged(&worktree_path, "src/app.rs", "old\n", "stale\n")
            .expect_err("stale remote content must conflict");
        assert!(conflict.to_string().contains("changed on remote host"));
        let symlink_error = client
            .save_file_content_if_unchanged(&worktree_path, "src/link.rs", "outside\n", "escaped\n")
            .expect_err("symlink target must be rejected");
        assert!(symlink_error.to_string().contains("symbolic-link"));
        assert_eq!(
            fs::read_to_string(outside).expect("read preserved outside file"),
            "outside\n"
        );
        fs::remove_dir_all(fixture).expect("remove save shell fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_provider_file_transport_creates_files_and_discovers_callback_host() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let fixture = create_remote_ssh_fixture("remote-provider-files", "exit 1");
        let ssh_path = fixture.join("ssh-provider-files");
        fs::write(
            &ssh_path,
            "#!/bin/sh\nfor argument do command=$argument; done\ncase \"$command\" in *SSH_CONNECTION*) printf '192.0.2.10 54321 198.51.100.20 22';; *) eval \"$command\";; esac\n",
        )
        .expect("write provider transport fake ssh");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make provider transport fake ssh executable");
        let worktree = fixture.join("remote repo");
        fs::create_dir_all(&worktree).expect("create provider worktree");
        let git = Command::new("git")
            .arg("-C")
            .arg(&worktree)
            .args(["init", "-q"])
            .output()
            .expect("initialize provider worktree");
        assert!(git.status.success());
        let outside = fixture.join("outside");
        fs::create_dir_all(&outside).expect("create provider outside directory");
        symlink(&outside, worktree.join(".escaped")).expect("create provider parent symlink");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid provider client")
            .with_test_process_options(ssh_path, Duration::from_secs(2), 1024 * 1024);
        let worktree_path = worktree.to_string_lossy();

        assert_eq!(
            client.ssh_client_address().expect("SSH callback address"),
            "192.0.2.10"
        );
        assert_eq!(
            client
                .read_optional_file(&worktree_path, ".claude/settings.json")
                .expect("missing provider file"),
            None
        );
        client
            .write_file(
                &worktree_path,
                ".claude/hooks/kanvibe-stop-hook.sh",
                "#!/bin/bash\nexit 0\n",
                true,
            )
            .expect("create remote provider script");
        let script_path = worktree.join(".claude/hooks/kanvibe-stop-hook.sh");
        assert_eq!(
            fs::read_to_string(&script_path).expect("read provider script"),
            "#!/bin/bash\nexit 0\n"
        );
        assert_eq!(
            fs::metadata(&script_path)
                .expect("provider script metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(
            client
                .read_optional_file(&worktree_path, ".claude/hooks/kanvibe-stop-hook.sh",)
                .expect("read provider script"),
            Some("#!/bin/bash\nexit 0\n".to_owned())
        );
        client
            .ensure_git_exclude_lines(
                &worktree_path,
                &["# KanVibe hooks", ".claude/hooks/", ".kanvibe/"],
            )
            .expect("install remote Git exclude lines");
        client
            .ensure_git_exclude_lines(
                &worktree_path,
                &["# KanVibe hooks", ".claude/hooks/", ".kanvibe/"],
            )
            .expect("repeat remote Git exclude lines");
        let exclude =
            fs::read_to_string(worktree.join(".git/info/exclude")).expect("read remote exclude");
        assert_eq!(exclude.matches("# KanVibe hooks").count(), 1);
        assert_eq!(exclude.matches(".claude/hooks/").count(), 1);
        let escape_error = client
            .write_file(&worktree_path, ".escaped/hook.sh", "escaped\n", true)
            .expect_err("symlink parent must be rejected");
        assert!(escape_error.to_string().contains("symbolic-link"));
        assert!(!outside.join("hook.sh").exists());
        fs::remove_dir_all(fixture).expect("remove provider transport fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_home_session_transport_is_read_only_and_rejects_symlink_escape() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let fixture = create_remote_ssh_fixture("remote-session-files", "exit 1");
        let home = fixture.join("home");
        let sessions = home.join(".codex/sessions/2026");
        fs::create_dir_all(&sessions).expect("session directory");
        fs::write(
            sessions.join("rollout.jsonl"),
            "{\"type\":\"session_meta\"}\n",
        )
        .expect("session file");
        let outside = fixture.join("outside.jsonl");
        fs::write(&outside, "outside\n").expect("outside file");
        symlink(&outside, sessions.join("escaped.jsonl")).expect("session symlink");
        let ssh_path = fixture.join("ssh-session-files");
        fs::write(
            &ssh_path,
            format!(
                "#!/bin/sh\nexport HOME={}\nfor argument do command=$argument; done\neval \"$command\"\n",
                quote_for_posix_shell(home.to_string_lossy().as_ref())
            ),
        )
        .expect("write session transport");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make session transport executable");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("remote session client")
            .with_test_process_options(ssh_path, Duration::from_secs(2), 1024 * 1024);
        let home_string = home.to_string_lossy().into_owned();

        assert_eq!(client.home_directory().expect("remote HOME"), home_string);
        let files = client
            .list_home_files(&format!("{home_string}/.codex/sessions"), ".jsonl", true)
            .expect("list session files");
        assert_eq!(
            files,
            vec![sessions.join("rollout.jsonl").to_string_lossy()]
        );
        assert_eq!(
            client.read_home_text(&files[0]).expect("read session file"),
            "{\"type\":\"session_meta\"}\n"
        );
        let escaped = client
            .read_home_text(&sessions.join("escaped.jsonl").to_string_lossy())
            .expect_err("symlink session file must be rejected");
        assert!(escaped.to_string().contains("symlink"));
        assert_eq!(
            client
                .query_open_code_history(
                    &format!("{home_string}/.local/share/opencode/opencode.db"),
                    None,
                )
                .expect("missing OpenCode database"),
            "__KANVIBE_DB_MISSING__"
        );
        fs::remove_dir_all(fixture).expect("remove remote session fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_classifies_invalid_host_timeout_transport_and_output_limit() {
        assert_eq!(
            RemoteGitClient::new("-oProxyCommand=bad", "/tmp/control")
                .expect_err("option-shaped host must be rejected")
                .kind(),
            GitErrorKind::InvalidInput
        );
        assert_eq!(
            RemoteGitClient::new("bad\nhost", "/tmp/control")
                .expect_err("control character must be rejected")
                .kind(),
            GitErrorKind::InvalidInput
        );

        let timeout_fixture = create_remote_ssh_fixture("remote-timeout", "while :; do :; done");
        let timeout_client = RemoteGitClient::new("remote-host", timeout_fixture.join("control"))
            .expect("valid timeout client")
            .with_test_process_options(timeout_fixture.join("ssh"), Duration::ZERO, 1024);
        assert_eq!(
            timeout_client
                .list_worktrees("/remote/repo")
                .expect_err("zero deadline must time out")
                .kind(),
            GitErrorKind::Timeout
        );

        let transport_fixture = create_remote_ssh_fixture(
            "remote-transport",
            "printf 'connection refused\\n' >&2\nexit 255",
        );
        let transport_client =
            RemoteGitClient::new("remote-host", transport_fixture.join("control"))
                .expect("valid transport client")
                .with_test_process_options(
                    transport_fixture.join("ssh"),
                    Duration::from_secs(1),
                    1024,
                );
        assert_eq!(
            transport_client
                .list_worktrees("/remote/repo")
                .expect_err("ssh 255 must be transport failure")
                .kind(),
            GitErrorKind::Transport
        );

        let output_fixture =
            create_remote_ssh_fixture("remote-output", "printf '0123456789abcdef'");
        let output_client = RemoteGitClient::new("remote-host", output_fixture.join("control"))
            .expect("valid output client")
            .with_test_process_options(output_fixture.join("ssh"), Duration::from_secs(1), 8);
        assert_eq!(
            output_client
                .list_worktrees("/remote/repo")
                .expect_err("bounded output must reject oversized response")
                .kind(),
            GitErrorKind::OutputLimit
        );
        fs::remove_dir_all(timeout_fixture).expect("remove timeout fixture");
        fs::remove_dir_all(transport_fixture).expect("remove transport fixture");
        fs::remove_dir_all(output_fixture).expect("remove output fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_session_dependency_uses_the_bounded_typed_ssh_boundary() {
        let fixture = create_remote_ssh_fixture(
            "remote-session-dependency",
            "case \"$*\" in *\"command -v tmux\"*) exit 0;; *) exit 9;; esac",
        );
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid remote host")
            .with_test_process_options(fixture.join("ssh"), Duration::from_secs(1), 1024 * 1024);

        assert!(
            client
                .session_dependency_available(SessionType::Tmux)
                .expect("dependency check")
        );
        fs::remove_dir_all(fixture).expect("remove dependency fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_hook_health_uses_validated_ip_and_bounded_transport() {
        let fixture = create_remote_ssh_fixture(
            "remote-hook-health",
            "case \"$*\" in *curl*api/hooks/health*) exit 0;; *) exit 9;; esac",
        );
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid remote host")
            .with_test_process_options(fixture.join("ssh"), Duration::from_secs(1), 1024);

        assert!(
            client
                .native_hook_server_reachable("127.0.0.1".parse().expect("callback ip"), 9736,)
                .expect("hook health check")
        );
        fs::remove_dir_all(fixture).expect("remove hook health fixture");
    }

    #[cfg(unix)]
    #[test]
    fn remote_git_retries_once_after_transport_failure() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = create_remote_ssh_fixture("remote-retry", "exit 1");
        let count_path = fixture.join("attempt-count");
        let ssh_path = fixture.join("ssh-retry");
        fs::write(
            &ssh_path,
            format!(
                "#!/bin/sh\n\
                 case \" $* \" in *\" -O exit \"*) exit 0;; esac\n\
                 count=$(cat '{}' 2>/dev/null || printf 0)\n\
                 count=$((count + 1))\n\
                 printf '%s' \"$count\" > '{}'\n\
                 if [ \"$count\" -eq 1 ]; then printf 'connection reset\\n' >&2; exit 255; fi\n\
                 printf 'worktree /remote/repo\\nHEAD abc\\nbranch refs/heads/main\\n\\n'\n",
                count_path.display(),
                count_path.display(),
            ),
        )
        .expect("write retrying fake ssh");
        fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
            .expect("make retrying fake ssh executable");
        let client = RemoteGitClient::new("remote-host", fixture.join("control"))
            .expect("valid retry client")
            .with_test_process_options(ssh_path, Duration::from_secs(1), 1024);

        let worktrees = client
            .list_worktrees("/remote/repo")
            .expect("second SSH attempt succeeds");

        assert_eq!(worktrees.len(), 1);
        assert_eq!(
            fs::read_to_string(count_path).expect("read attempt count"),
            "2"
        );
        fs::remove_dir_all(fixture).expect("remove retry fixture");
    }

    #[test]
    fn remote_git_child_environment_excludes_runtime_state() {
        let environment = filter_ssh_child_environment([
            ("HOME".into(), "/home/qa".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("SSH_AUTH_SOCK".into(), "/tmp/agent.sock".into()),
            ("GH_TOKEN".into(), "test-token".into()),
            ("LC_CTYPE".into(), "en_US.UTF-8".into()),
            ("PORT".into(), "3000".into()),
            ("NODE_ENV".into(), "development".into()),
            ("KANVIBE_DB_PATH".into(), "/secret/database".into()),
        ]);

        assert_eq!(
            environment,
            vec![
                ("GH_TOKEN".into(), "test-token".into()),
                ("HOME".into(), "/home/qa".into()),
                ("LC_CTYPE".into(), "en_US.UTF-8".into()),
                ("PATH".into(), "/usr/bin:/bin".into()),
                ("SSH_AUTH_SOCK".into(), "/tmp/agent.sock".into()),
            ]
        );
    }

    #[cfg(unix)]
    fn create_remote_ssh_fixture(name: &str, response: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let fixture = std::env::temp_dir().join(format!(
            "kanvibe-git-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&fixture).expect("create remote fixture");
        let response_path = fixture.join("response.sh");
        fs::write(&response_path, format!("#!/bin/sh\n{response}\n"))
            .expect("write response script");
        fs::set_permissions(&response_path, fs::Permissions::from_mode(0o700))
            .expect("make response executable");
        let ssh_path = fixture.join("ssh");
        if !ssh_path.exists() {
            fs::copy(&response_path, &ssh_path).expect("create fake ssh");
            fs::set_permissions(&ssh_path, fs::Permissions::from_mode(0o700))
                .expect("make fake ssh executable");
        }
        fixture
    }

    fn create_temp_git_repo(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kanvibe-git-{name}-{}-{unique}",
            std::process::id()
        ));
        let path = root.join(name);
        fs::create_dir_all(&path).expect("temp repo dir");
        git_raw(&path, &["init", "-b", "main"]);
        git(&path, &["config", "user.email", "qa@kanvibe.test"]);
        git(&path, &["config", "user.name", "KanVibe QA"]);
        fs::create_dir_all(path.join("src")).expect("src dir");
        path
    }

    fn write_file(path: PathBuf, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("file parent");
        }
        fs::write(path, content).expect("write test file");
    }

    fn git(repo: &Path, args: &[&str]) {
        exec_git(repo, args).unwrap_or_else(|error| panic!("git {args:?} failed: {error}"));
    }

    fn git_raw(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
