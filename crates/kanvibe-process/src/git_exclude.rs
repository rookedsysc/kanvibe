//! `.git/info/exclude`에 AI 도구 경로와 `.kanvibe/`를 넣는다(TS `gitExclude.ts`).
//! main worktree와 linked worktree가 함께 쓰는 git common dir의 exclude 한 곳에 마커 블록으로 기록해 멱등성을 지킨다.

use crate::exec::ExecGitOptions;
use crate::host_file_access::{HostFileAccess, HostFileError, quote_shell_argument};
use crate::kanvibe_project_state::join_posix_path;

pub const KANVIBE_GIT_EXCLUDE_MARKER: &str = "# KanVibe AI hooks (auto-generated)";
pub const KANVIBE_STATE_DIR_EXCLUDE_PATTERN: &str = ".kanvibe/";

const EXCLUDE_PATTERNS: [&str; 12] = [
    ".claude/hooks/",
    ".claude/settings.json",
    ".gemini/hooks/",
    ".gemini/settings.json",
    ".codex/hooks/",
    ".codex/hooks.json",
    ".codex/config.toml",
    ".opencode/plugins/",
    ".opencode/kanvibe-tui.mjs",
    ".opencode/tui.json",
    ".zellij-layout.kdl",
    KANVIBE_STATE_DIR_EXCLUDE_PATTERN,
];

const LEGACY_EXCLUDE_PATTERNS: [&str; 4] = [
    ".kanvibe/hooks-targets.json",
    ".kanvibe/task-state.json",
    ".kanvibe/status.md",
    ".kanvibe/status.json",
];

fn is_generated_block_line(line: &str) -> bool {
    EXCLUDE_PATTERNS.contains(&line) || LEGACY_EXCLUDE_PATTERNS.contains(&line) || line.is_empty()
}

/// 기존 마커 블록(마커와 그 뒤의 자동 생성 줄)을 걷어 내고 최신 블록을 끝에 붙인다
fn build_exclude_content(current_content: &str) -> String {
    let normalized_content = current_content.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized_content.split('\n').collect();
    let mut preserved_lines: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index] != KANVIBE_GIT_EXCLUDE_MARKER {
            preserved_lines.push(lines[index]);
            index += 1;
            continue;
        }
        index += 1;
        while index < lines.len() && is_generated_block_line(lines[index]) {
            index += 1;
        }
    }

    let preserved_content = preserved_lines.join("\n");
    let preserved_content = preserved_content.trim_end();
    let marker_block = std::iter::once(KANVIBE_GIT_EXCLUDE_MARKER)
        .chain(EXCLUDE_PATTERNS)
        .collect::<Vec<_>>()
        .join("\n");
    if preserved_content.is_empty() {
        return format!("{marker_block}\n");
    }
    format!("{preserved_content}\n\n{marker_block}\n")
}

fn strict_options() -> ExecGitOptions {
    ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    }
}

/// `git rev-parse`가 돌려준 경로를 저장소 기준 절대 경로로 바꾼다
fn resolve_absolute_git_path(repo_path: &str, git_path: &str) -> String {
    let trimmed = git_path.trim();
    if trimmed.is_empty() {
        return repo_path.to_string();
    }
    if trimmed.starts_with('/') {
        return trimmed.to_string();
    }
    join_posix_path(&[repo_path, trimmed])
}

fn get_common_exclude_path(
    files: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
) -> Result<String, HostFileError> {
    let command = format!(
        "git -C {} rev-parse --path-format=absolute --git-common-dir",
        quote_shell_argument(repo_path)
    );
    let git_common_dir_output = files
        .runner()
        .exec_git(&command, ssh_host, &strict_options())?;
    let git_common_dir = resolve_absolute_git_path(repo_path, &git_common_dir_output);
    Ok(join_posix_path(&[&git_common_dir, "info", "exclude"]))
}

/// AI 코딩 도구의 hook 설정 파일을 git common dir의 info/exclude에 넣어 git 추적에서 뺀다.
/// 마커 블록으로 멱등성을 지키고, 원격 저장소도 같은 방식으로 다룬다.
pub fn add_ai_tool_patterns_to_git_exclude(
    files: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let exclude_path = get_common_exclude_path(files, repo_path, ssh_host)?;
    let content = files.read_text_file(&exclude_path, ssh_host, &strict_options())?;
    let next_content = build_exclude_content(&content);
    if content == next_content {
        return Ok(());
    }
    files.write_text_file(&exclude_path, &next_content, ssh_host, &strict_options())
}

/// hook 설치 과정에서 git exclude를 갱신한다.
/// exclude 갱신 실패는 hook 동작을 막지 않으므로 경고만 남기고 설치를 계속한다.
pub fn ensure_ai_tool_patterns_excluded(
    files: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
) {
    if let Err(error) = add_ai_tool_patterns_to_git_exclude(files, repo_path, ssh_host) {
        eprintln!(
            "[hooks] git exclude 패턴 추가 실패 {{ repoPath: {repo_path:?}, sshHost: {:?}, error: {:?} }}",
            ssh_host,
            error.to_string()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::Arc;

    use super::*;
    use crate::exec::local::exec_local;
    use crate::exec::testing::ScriptedCommandRunner;
    use crate::exec::{ExecGitError, ShellCommandRunner};
    use crate::shell_environment::ShellEnvironment;

    /// 실제 로컬 git을 실행하는 실행기. 사용자 git 설정이 끼어들지 않도록 HOME을 임시 디렉터리로 둔다
    struct LocalGitRunner {
        environment: ShellEnvironment,
    }

    impl ShellCommandRunner for LocalGitRunner {
        fn exec_git(
            &self,
            command: &str,
            _ssh_host: Option<&str>,
            options: &ExecGitOptions,
        ) -> Result<String, ExecGitError> {
            exec_local(command, &self.environment, options)
        }
    }

    struct GitFixture {
        directory: tempfile::TempDir,
        home: tempfile::TempDir,
    }

    impl GitFixture {
        fn new() -> Self {
            let fixture = Self {
                directory: tempfile::tempdir().unwrap(),
                home: tempfile::tempdir().unwrap(),
            };
            fixture.git(&fixture.repo_path(), &["init", "-q"]);
            fixture
        }

        fn repo_path(&self) -> String {
            self.directory.path().to_string_lossy().into_owned()
        }

        fn environment(&self) -> ShellEnvironment {
            let mut environment = ShellEnvironment::new();
            environment.insert("PATH".into(), std::env::var("PATH").unwrap_or_default());
            environment.insert(
                "HOME".into(),
                self.home.path().to_string_lossy().into_owned(),
            );
            environment.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
            environment
        }

        fn git(&self, cwd: &str, arguments: &[&str]) -> String {
            let output = Command::new("git")
                .args(arguments)
                .current_dir(cwd)
                .env_clear()
                .envs(self.environment())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        fn files(&self) -> HostFileAccess {
            let runner = LocalGitRunner {
                environment: self.environment(),
            };
            HostFileAccess::new(Arc::new(runner), self.home.path().to_string_lossy())
        }

        fn exclude_path(&self) -> std::path::PathBuf {
            self.directory.path().join(".git/info/exclude")
        }

        fn commit_readme(&self) {
            let repo_path = self.repo_path();
            self.git(&repo_path, &["config", "user.name", "KanVibe Test"]);
            self.git(&repo_path, &["config", "user.email", "kanvibe@example.com"]);
            std::fs::write(self.directory.path().join("README.md"), "# test\n").unwrap();
            self.git(&repo_path, &["add", "README.md"]);
            self.git(&repo_path, &["commit", "-q", "-m", "init"]);
        }
    }

    fn read_exclude(fixture: &GitFixture) -> String {
        std::fs::read_to_string(fixture.exclude_path()).unwrap()
    }

    #[test]
    fn creates_exclude_file_with_marker_block_when_it_does_not_exist() {
        let fixture = GitFixture::new();
        let _ = std::fs::remove_file(fixture.exclude_path());

        add_ai_tool_patterns_to_git_exclude(&fixture.files(), &fixture.repo_path(), None).unwrap();

        let content = read_exclude(&fixture);
        assert!(content.contains(KANVIBE_GIT_EXCLUDE_MARKER));
        assert!(!content.contains(".kanvibe/task-id"));
        for pattern in [
            ".claude/hooks/",
            ".claude/settings.json",
            ".gemini/hooks/",
            ".gemini/settings.json",
            ".codex/hooks/",
            ".codex/hooks.json",
            ".codex/config.toml",
            ".opencode/plugins/",
            ".kanvibe/",
        ] {
            assert!(content.contains(pattern), "{pattern}");
        }
    }

    #[test]
    fn does_not_duplicate_patterns_when_called_multiple_times() {
        let fixture = GitFixture::new();
        let files = fixture.files();

        add_ai_tool_patterns_to_git_exclude(&files, &fixture.repo_path(), None).unwrap();
        add_ai_tool_patterns_to_git_exclude(&files, &fixture.repo_path(), None).unwrap();

        assert_eq!(
            read_exclude(&fixture)
                .matches(KANVIBE_GIT_EXCLUDE_MARKER)
                .count(),
            1
        );
    }

    #[test]
    fn preserves_existing_content_in_exclude_file() {
        let fixture = GitFixture::new();
        std::fs::write(fixture.exclude_path(), "# existing patterns\n*.log\n.env\n").unwrap();

        add_ai_tool_patterns_to_git_exclude(&fixture.files(), &fixture.repo_path(), None).unwrap();

        let content = read_exclude(&fixture);
        assert!(content.contains("*.log"));
        assert!(content.contains(".env"));
        assert!(content.contains(KANVIBE_GIT_EXCLUDE_MARKER));
        assert!(content.contains(".claude/hooks/"));
    }

    #[test]
    fn restores_missing_patterns_when_only_the_marker_block_remains() {
        let fixture = GitFixture::new();
        std::fs::write(
            fixture.exclude_path(),
            format!("{KANVIBE_GIT_EXCLUDE_MARKER}\n"),
        )
        .unwrap();

        add_ai_tool_patterns_to_git_exclude(&fixture.files(), &fixture.repo_path(), None).unwrap();

        let content = read_exclude(&fixture);
        for pattern in [
            KANVIBE_GIT_EXCLUDE_MARKER,
            ".claude/hooks/",
            ".codex/config.toml",
            ".opencode/plugins/",
            ".kanvibe/",
        ] {
            assert!(content.contains(pattern), "{pattern}");
        }
    }

    #[test]
    fn prunes_legacy_kanvibe_state_files_from_the_generated_block() {
        let fixture = GitFixture::new();
        let existing = [
            "# user pattern",
            "*.log",
            KANVIBE_GIT_EXCLUDE_MARKER,
            ".claude/hooks/",
            ".kanvibe/hooks-targets.json",
            ".kanvibe/task-state.json",
            ".kanvibe/status.md",
            ".kanvibe/status.json",
            "",
            "# keep me",
            "dist/",
            "",
        ]
        .join("\n");
        std::fs::write(fixture.exclude_path(), existing).unwrap();

        add_ai_tool_patterns_to_git_exclude(&fixture.files(), &fixture.repo_path(), None).unwrap();

        let content = read_exclude(&fixture);
        for kept in ["*.log", "# keep me", "dist/", ".kanvibe/"] {
            assert!(content.contains(kept), "{kept}");
        }
        for legacy in LEGACY_EXCLUDE_PATTERNS {
            assert!(!content.contains(legacy), "{legacy}");
        }
        assert_eq!(content.matches(KANVIBE_GIT_EXCLUDE_MARKER).count(), 1);
    }

    #[test]
    fn updates_the_shared_common_dir_exclude_from_a_linked_worktree() {
        let fixture = GitFixture::new();
        fixture.commit_readme();
        let worktree_parent = tempfile::tempdir().unwrap();
        let worktree_path = worktree_parent
            .path()
            .join("feature")
            .to_string_lossy()
            .into_owned();
        fixture.git(
            &fixture.repo_path(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "feature/test",
                &worktree_path,
                "HEAD",
            ],
        );
        let common_dir = fixture.git(
            &worktree_path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        let common_exclude_path = std::path::Path::new(&common_dir).join("info/exclude");
        std::fs::write(
            &common_exclude_path,
            format!("{KANVIBE_GIT_EXCLUDE_MARKER}\n"),
        )
        .unwrap();

        add_ai_tool_patterns_to_git_exclude(&fixture.files(), &worktree_path, None).unwrap();

        let content = std::fs::read_to_string(common_exclude_path).unwrap();
        for pattern in [
            ".claude/hooks/",
            ".gemini/settings.json",
            ".codex/hooks.json",
        ] {
            assert!(content.contains(pattern), "{pattern}");
        }
    }

    #[test]
    fn fails_when_path_is_not_a_git_repository() {
        let fixture = GitFixture::new();
        let non_git_directory = tempfile::tempdir().unwrap();

        let result = add_ai_tool_patterns_to_git_exclude(
            &fixture.files(),
            &non_git_directory.path().to_string_lossy(),
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn creates_info_directory_when_it_does_not_exist() {
        let fixture = GitFixture::new();
        let info_directory = fixture.directory.path().join(".git/info");
        let _ = std::fs::remove_dir_all(&info_directory);

        add_ai_tool_patterns_to_git_exclude(&fixture.files(), &fixture.repo_path(), None).unwrap();

        let content = std::fs::read_to_string(info_directory.join("exclude")).unwrap();
        assert!(content.contains(KANVIBE_GIT_EXCLUDE_MARKER));
    }

    #[test]
    fn updates_the_common_git_exclude_when_called_from_a_worktree() {
        let fixture = GitFixture::new();
        fixture.commit_readme();
        let worktree_parent = tempfile::tempdir().unwrap();
        let worktree_path = worktree_parent
            .path()
            .join("worktree")
            .to_string_lossy()
            .into_owned();
        fixture.git(
            &fixture.repo_path(),
            &[
                "worktree",
                "add",
                "-q",
                &worktree_path,
                "-b",
                "worktree-test",
            ],
        );

        add_ai_tool_patterns_to_git_exclude(&fixture.files(), &worktree_path, None).unwrap();

        let content = read_exclude(&fixture);
        assert!(content.contains(KANVIBE_GIT_EXCLUDE_MARKER));
        assert!(content.contains(".claude/hooks/"));
    }

    #[test]
    fn writes_ai_hook_patterns_to_the_remote_common_git_exclude() {
        let runner = Arc::new(ScriptedCommandRunner::new(|call| {
            if call.command.starts_with("git -C") {
                return Ok("/remote/main/.git".into());
            }
            if call.command.starts_with("test -f") {
                return Ok("# existing\n".into());
            }
            Ok(String::new())
        }));
        let files = HostFileAccess::new(runner.clone(), "/unused-home");

        add_ai_tool_patterns_to_git_exclude(&files, "/remote/worktree/task-1", Some("remote-host"))
            .unwrap();

        let calls = runner.calls();
        assert_eq!(
            calls[0].command,
            "git -C '/remote/worktree/task-1' rev-parse --path-format=absolute --git-common-dir"
        );
        assert!(!calls.iter().any(|call| call.command.contains("--git-dir")));
        assert_eq!(
            calls[1].command,
            "test -f '/remote/main/.git/info/exclude' && cat '/remote/main/.git/info/exclude' || true"
        );
        let write_call = &calls[2];
        assert!(
            write_call
                .command
                .contains("'/remote/main/.git/info/exclude'")
        );
        let encoded = write_call.command.split('\'').nth(5).unwrap();
        let written = String::from_utf8(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).unwrap(),
        )
        .unwrap();
        assert!(written.contains(".codex/config.toml"));
        assert!(written.contains(".kanvibe/"));
        assert!(!written.contains(".kanvibe/hooks-targets.json"));
        for call in &calls {
            assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
            assert!(call.options.should_check_host_key_strictly);
        }
    }
}
