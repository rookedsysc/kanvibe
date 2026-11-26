//! TS `worktree.test.ts`의 세션·정리 시나리오. `execGit`과 `listWorktrees` 모의 대신 명령을 기록하는 실행기를 쓴다.

use std::process::Command;
use std::sync::Arc;

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use kanvibe_domain::task::SessionType;

use super::*;
use crate::exec::testing::{RecordedCommand, ScriptedCommandRunner};
use crate::exec::{ExecGitError, ShellCommandRunner};
use crate::git_operations::{list_worktrees, resolve_path_for_shell};
use crate::host_file_access::HostFileAccess;

const LOCAL_HOME: &str = "/home/local-user";

struct NoHerdr;

impl HerdrTaskWorkspaces for NoHerdr {
    fn close_task_workspace(
        &self,
        _session_name: &str,
        _ssh_host: Option<&str>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn is_task_alive(&self, _session_name: &str, _ssh_host: Option<&str>) -> bool {
        false
    }
}

fn file_access(runner: &Arc<ScriptedCommandRunner>) -> HostFileAccess {
    let shared_runner: Arc<dyn ShellCommandRunner> = runner.clone();
    HostFileAccess::new(shared_runner, LOCAL_HOME)
}

fn returning(output: &str) -> Arc<ScriptedCommandRunner> {
    Arc::new(ScriptedCommandRunner::returning(output))
}

fn failing(message: &str) -> Arc<ScriptedCommandRunner> {
    let message = message.to_string();
    Arc::new(ScriptedCommandRunner::new(move |_| {
        Err(ExecGitError::new(message.clone()))
    }))
}

fn porcelain(worktrees: &[(&str, &str)]) -> String {
    worktrees
        .iter()
        .map(|(path, branch)| format!("worktree {path}\nHEAD 0123abc\nbranch refs/heads/{branch}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `git worktree list`에는 주어진 목록을, `fails_on`을 포함한 명령에는 실패를, 나머지에는 빈 출력을 돌려준다
fn repository_runner(
    worktrees: &[(&str, &str)],
    fails_on: &'static [&'static str],
) -> Arc<ScriptedCommandRunner> {
    let listing = porcelain(worktrees);
    Arc::new(ScriptedCommandRunner::new(move |call: &RecordedCommand| {
        if call.command.ends_with("worktree list --porcelain") {
            return Ok(listing.clone());
        }
        if fails_on
            .iter()
            .any(|pattern| call.command.contains(pattern))
        {
            return Err(ExecGitError::new("git failed"));
        }
        Ok(String::new())
    }))
}

fn filter_calls(runner: &ScriptedCommandRunner, pattern: &str) -> Vec<String> {
    runner
        .commands()
        .into_iter()
        .filter(|command| command.contains(pattern))
        .collect()
}

fn has_call(runner: &ScriptedCommandRunner, command: &str, ssh_host: Option<&str>) -> bool {
    runner
        .calls()
        .iter()
        .any(|call| call.command == command && call.ssh_host.as_deref() == ssh_host)
}

fn no_layout() -> Result<Option<PaneLayoutSelection>, String> {
    Ok(None)
}

fn cleanup_options(
    should_throw_on_error: bool,
    worktree_path: Option<&str>,
) -> ResourceCleanupOptions {
    ResourceCleanupOptions {
        should_throw_on_error,
        worktree_path: worktree_path.map(str::to_string),
    }
}

#[test]
fn tmux_session_alive_checks_both_sockets() {
    let runner = returning("");

    let is_alive = is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Tmux,
        "feat-branch",
        None,
    );

    assert!(is_alive);
    assert!(has_call(
        &runner,
        "tmux has-session -t 'feat-branch' 2>/dev/null && exit 0; tmux -L 'kanvibe' has-session -t 'feat-branch' 2>/dev/null && exit 0; exit 1",
        None
    ));
}

#[test]
fn tmux_session_lookup_includes_the_pre_1_0_6_socket() {
    let runner = returning("");

    is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Tmux,
        "feat-branch",
        None,
    );

    assert!(runner.commands()[0].contains("tmux -L 'kanvibe' has-session -t 'feat-branch'"));
}

#[test]
fn missing_tmux_session_is_not_alive() {
    let runner = failing("no session");

    assert!(!is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Tmux,
        "nonexistent",
        None
    ));
}

#[test]
fn listed_zellij_session_is_alive() {
    let runner = returning("feat-branch\nother-session\n");

    assert!(is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Zellij,
        "feat-branch",
        None
    ));
}

#[test]
fn unlisted_zellij_session_is_not_alive() {
    let runner = returning("other-session\n");

    assert!(!is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Zellij,
        "nonexistent",
        None
    ));
}

#[test]
fn remote_session_check_passes_the_ssh_host() {
    let runner = returning("");

    is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Tmux,
        "feat-branch",
        Some("remote-host"),
    );

    let call = &runner.calls()[0];
    assert!(call.command.contains("tmux has-session -t 'feat-branch'"));
    assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
}

#[test]
fn exited_zellij_session_is_not_alive() {
    let runner = returning("EXITED: feat-branch\nother-session\n");

    assert!(!is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Zellij,
        "feat-branch",
        None
    ));
}

#[test]
fn colored_zellij_session_name_is_alive() {
    let runner = returning("\x1b[32;1mfeat-branch\x1b[m [Created \x1b[35;1m2s\x1b[m ago] \n");

    assert!(is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Zellij,
        "feat-branch",
        None
    ));
}

#[test]
fn zellij_session_name_prefix_is_not_a_match() {
    let runner = returning("feat-branch-extra [Created 1h ago]\n");

    assert!(!is_session_alive(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Zellij,
        "feat-branch",
        None
    ));
}

#[test]
fn removes_the_worktree_path_resolved_from_git() {
    let runner = repository_runner(
        &[(
            "/workspace/custom-worktrees/feature-real-path",
            "feature/real-path",
        )],
        &[],
    );
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/real-path",
        Some("remote-host"),
        &ResourceCleanupOptions::default(),
    )
    .unwrap();

    let remote = Some("remote-host");
    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' worktree list --porcelain",
        remote
    ));
    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' worktree remove '/workspace/custom-worktrees/feature-real-path' --force",
        remote
    ));
    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' branch -D 'feature/real-path'",
        remote
    ));
}

#[test]
fn skips_worktree_removal_when_the_branch_has_no_worktree() {
    let runner = repository_runner(&[("/workspace/repo", "main")], &[]);
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/missing",
        None,
        &ResourceCleanupOptions::default(),
    )
    .unwrap();

    assert!(filter_calls(&runner, "worktree remove").is_empty());
    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' branch -D 'feature/missing'",
        None
    ));
}

#[test]
fn deletes_a_stale_managed_directory_git_no_longer_lists() {
    let runner = repository_runner(&[("/workspace/repo", "main")], &[]);
    let files = file_access(&runner);
    let stale_path = "/workspace/repo__worktrees/feature-stale-dir";

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/stale-dir",
        Some("remote-host"),
        &cleanup_options(true, Some(stale_path)),
    )
    .unwrap();

    let stale_cleanup_command =
        filter_calls(&runner, &format!("rm -rf -- '{stale_path}'")).remove(0);
    assert!(stale_cleanup_command.contains(&format!("test -d '{stale_path}'")));
    assert!(stale_cleanup_command.contains(&format!("test ! -L '{stale_path}'")));
    assert!(stale_cleanup_command.contains(&format!("'{stale_path}' = '{stale_path}'")));
    assert!(has_call(
        &runner,
        &stale_cleanup_command,
        Some("remote-host")
    ));
}

#[test]
fn keeps_a_sanitized_managed_path_registered_to_another_worktree() {
    let runner = repository_runner(
        &[
            ("/workspace/repo", "main"),
            ("/workspace/repo__worktrees/feature-a-b", "feature-a/b"),
        ],
        &[],
    );
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/a-b",
        Some("remote-host"),
        &cleanup_options(true, None),
    )
    .unwrap();

    assert!(filter_calls(&runner, "worktree remove").is_empty());
    assert!(
        filter_calls(
            &runner,
            "rm -rf -- '/workspace/repo__worktrees/feature-a-b'"
        )
        .is_empty()
    );
}

#[test]
fn falls_back_to_the_managed_task_path_when_the_branch_changed() {
    let runner = repository_runner(
        &[
            ("/workspace/repo", "main"),
            (
                "/workspace/repo__worktrees/feature-task",
                "debug/other-branch",
            ),
        ],
        &[],
    );
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/task",
        Some("remote-host"),
        &cleanup_options(false, Some("/workspace/repo__worktrees/feature-task/")),
    )
    .unwrap();

    let remote = Some("remote-host");
    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' worktree remove '/workspace/repo__worktrees/feature-task' --force",
        remote
    ));
    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' branch -D 'feature/task'",
        remote
    ));
}

#[test]
fn does_not_fall_back_to_a_path_owned_by_another_worktree() {
    let runner = repository_runner(
        &[
            ("/workspace/repo", "main"),
            (
                "/workspace/custom-worktrees/other-task",
                "feature/other-task",
            ),
        ],
        &[],
    );
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/task",
        Some("remote-host"),
        &cleanup_options(false, Some("/workspace/custom-worktrees/other-task")),
    )
    .unwrap();

    assert!(filter_calls(&runner, "worktree remove").is_empty());
    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' branch -D 'feature/task'",
        Some("remote-host")
    ));
}

#[test]
fn prunes_a_broken_registration_and_still_deletes_the_branch() {
    let runner = repository_runner(
        &[
            ("/workspace/repo", "main"),
            ("/tmp/broken-worktree", "feature/broken"),
        ],
        &["worktree remove"],
    );
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/broken",
        None,
        &cleanup_options(true, None),
    )
    .unwrap();

    assert!(has_call(
        &runner,
        "git -C '/workspace/repo' worktree prune",
        None
    ));
    assert_eq!(filter_calls(&runner, "branch -D 'feature/broken'").len(), 1);
}

#[test]
fn still_deletes_the_branch_when_remove_and_prune_fail() {
    let runner = repository_runner(
        &[
            ("/workspace/repo", "main"),
            ("/tmp/broken-worktree", "feature/broken"),
        ],
        &["worktree remove", "worktree prune"],
    );
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/broken",
        None,
        &cleanup_options(true, None),
    )
    .unwrap();

    assert_eq!(filter_calls(&runner, "branch -D 'feature/broken'").len(), 1);
}

#[test]
fn still_deletes_the_branch_when_stale_directory_cleanup_fails() {
    let runner = repository_runner(&[("/workspace/repo", "main")], &["rm -rf --"]);
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/stale-dir",
        None,
        &cleanup_options(true, Some("/workspace/repo__worktrees/feature-stale-dir")),
    )
    .unwrap();

    assert!(!filter_calls(&runner, "rm -rf --").is_empty());
    assert_eq!(
        filter_calls(&runner, "branch -D 'feature/stale-dir'").len(),
        1
    );
}

#[test]
fn keeps_a_branch_checked_out_in_the_project_root() {
    let runner = repository_runner(&[("/workspace/repo", "feature/root-checkout")], &[]);
    let files = file_access(&runner);

    remove_worktree_and_branch(
        &files,
        "/workspace/repo/",
        "feature/root-checkout",
        None,
        &cleanup_options(true, None),
    )
    .unwrap();

    assert!(filter_calls(&runner, "worktree remove").is_empty());
    assert!(filter_calls(&runner, "branch -D").is_empty());
}

#[test]
fn branch_cleanup_failure_is_reported_only_when_requested() {
    let runner = repository_runner(&[("/workspace/repo", "main")], &["branch -D"]);
    let files = file_access(&runner);

    let quiet = remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/x",
        None,
        &cleanup_options(false, None),
    );
    let strict = remove_worktree_and_branch(
        &files,
        "/workspace/repo",
        "feature/x",
        None,
        &cleanup_options(true, None),
    );

    assert_eq!(quiet, Ok(()));
    assert_eq!(
        strict.unwrap_err().to_string(),
        "브랜치 정리 실패: feature/x"
    );
}

#[test]
fn kills_a_tmux_session_on_both_sockets() {
    let runner = returning("");

    remove_session_only(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Tmux,
        "feat-branch",
        None,
        &ResourceCleanupOptions::default(),
    )
    .unwrap();

    assert!(has_call(
        &runner,
        "tmux kill-session -t 'feat-branch' 2>/dev/null || true; tmux -L 'kanvibe' kill-session -t 'feat-branch' 2>/dev/null || true",
        None
    ));
}

const ZELLIJ_CLEANUP: &str = "zellij kill-session 'feat-branch' 2>/dev/null || true; zellij delete-session --force 'feat-branch' 2>/dev/null || true";

#[test]
fn kills_a_zellij_session_and_deletes_its_resurrect_data() {
    let runner = returning("");

    remove_session_only(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Zellij,
        "feat-branch",
        None,
        &ResourceCleanupOptions::default(),
    )
    .unwrap();

    assert!(has_call(&runner, ZELLIJ_CLEANUP, None));
}

#[test]
fn remote_zellij_cleanup_passes_the_ssh_host() {
    let runner = returning("");

    remove_session_only(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Zellij,
        "feat-branch",
        Some("remote-host"),
        &ResourceCleanupOptions::default(),
    )
    .unwrap();

    assert!(has_call(&runner, ZELLIJ_CLEANUP, Some("remote-host")));
}

fn strict_session_cleanup_command(session_type: SessionType) -> RecordedCommand {
    let runner = returning("");
    remove_session_only(
        runner.as_ref(),
        &NoHerdr,
        session_type,
        "feat-branch",
        Some("remote-host"),
        &cleanup_options(true, None),
    )
    .unwrap();
    runner.calls().remove(0)
}

#[test]
fn strict_zellij_cleanup_verifies_the_session_is_gone() {
    let call = strict_session_cleanup_command(SessionType::Zellij);

    for expected in [
        "command -v zellij >/dev/null 2>&1",
        "zellij kill-session 'feat-branch'",
        "zellij delete-session --force 'feat-branch'",
        "zellij list-sessions",
        "grep -Fx -- 'feat-branch'",
    ] {
        assert!(call.command.contains(expected), "{expected}");
    }
    assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
}

/// zellij 0.46의 실측 동작을 흉내 낸다. 모르는 하위 명령은 rc 2로 거절하고, 실행 중인 세션은 --force 없이는 지우지 않으며,
/// 파이프로 받아도 세션 이름에 색을 입혀 목록을 내보낸다. 남은 세션 여부는 상태 파일로 본다.
fn run_zellij_cleanup(can_remove_session: bool) -> (Option<i32>, bool) {
    let cleanup_command = strict_session_cleanup_command(SessionType::Zellij).command;
    let state_directory = tempfile::tempdir().unwrap();
    let session_state_file = state_directory.path().join("alive");
    std::fs::write(&session_state_file, "").unwrap();
    let remove_session = if can_remove_session {
        "rm -f \"$state\""
    } else {
        ":"
    };
    let fake_zellij = [
        format!("state={}", serde_json::to_string(&session_state_file.to_string_lossy()).unwrap()),
        "zellij() { case \"$1\" in".to_string(),
        format!("  kill-session) {remove_session} ;;"),
        format!("  delete-session) if [ \"$2\" = \"--force\" ]; then {remove_session}; elif [ -e \"$state\" ]; then return 1; fi ;;"),
        "  list-sessions) if [ -e \"$state\" ]; then printf '\\033[32;1mfeat-branch\\033[m [Created \\033[35;1m0s\\033[m ago] \\n'; fi ;;".to_string(),
        "  *) return 2 ;;".to_string(),
        "esac; }".to_string(),
        "command() { return 0; }".to_string(),
    ]
    .join("\n");
    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("{fake_zellij}\n{cleanup_command}"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .status()
        .unwrap();
    (status.code(), session_state_file.exists())
}

#[test]
fn zellij_cleanup_in_shell_removes_a_running_session() {
    assert_eq!(run_zellij_cleanup(true), (Some(0), false));
}

#[test]
fn zellij_cleanup_in_shell_fails_when_a_colored_session_remains() {
    assert_eq!(run_zellij_cleanup(false), (Some(1), true));
}

#[test]
fn strict_tmux_cleanup_verifies_by_exact_session_name() {
    let call = strict_session_cleanup_command(SessionType::Tmux);

    for expected in [
        "command -v tmux >/dev/null 2>&1",
        "tmux -L 'kanvibe' kill-session -t 'feat-branch'",
        "tmux kill-session -t 'feat-branch'",
        "if tmux -L 'kanvibe' has-session -t 'feat-branch' 2>/dev/null; then exit 1; fi",
        "if tmux has-session -t 'feat-branch' 2>/dev/null; then exit 1; fi",
    ] {
        assert!(call.command.contains(expected), "{expected}");
    }
    assert!(!call.command.contains("tmux list-sessions"));
    assert!(!call.command.contains("grep -Fx"));
    assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
}

#[test]
fn already_terminated_session_is_not_an_error() {
    let runner = failing("session not found");

    let result = remove_session_only(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Tmux,
        "dead-session",
        None,
        &ResourceCleanupOptions::default(),
    );

    assert_eq!(result, Ok(()));
}

#[test]
fn strict_session_cleanup_failure_names_the_session() {
    let runner = failing("tmux failed");

    let result = remove_session_only(
        runner.as_ref(),
        &NoHerdr,
        SessionType::Tmux,
        "feat",
        None,
        &cleanup_options(true, None),
    );

    assert_eq!(result.unwrap_err().to_string(), "세션 정리 실패: feat");
}

fn create_without_worktree(
    session_type: SessionType,
    ssh_host: Option<&str>,
    working_directory: Option<&str>,
    branch_name: &str,
) -> (String, Arc<ScriptedCommandRunner>) {
    let runner = returning("");
    let files = file_access(&runner);
    let session_name = create_session_without_worktree(
        &files,
        "/repo/path",
        branch_name,
        session_type,
        ssh_host,
        working_directory,
        &no_layout,
    );
    (session_name, runner)
}

#[test]
fn session_name_comes_from_project_and_branch() {
    let (session_name, _) =
        create_without_worktree(SessionType::Tmux, None, None, "feat/my-feature");

    assert_eq!(session_name, "path-feat-my-feature");
}

#[test]
fn local_tmux_session_is_created_at_terminal_attach() {
    let (session_name, runner) =
        create_without_worktree(SessionType::Tmux, None, Some("/custom/dir"), "feat/test");

    assert_eq!(session_name, "path-feat-test");
    assert!(filter_calls(&runner, "tmux has-session").is_empty());
    assert!(filter_calls(&runner, "tmux new-session").is_empty());
}

#[test]
fn local_tmux_session_is_not_probed_before_attach() {
    let (_, runner) = create_without_worktree(SessionType::Tmux, None, None, "main");

    assert!(filter_calls(&runner, "tmux has-session").is_empty());
    assert!(filter_calls(&runner, "tmux new-session").is_empty());
}

#[test]
fn remote_tmux_dependency_checks_wait_for_attach() {
    let (_, runner) = create_without_worktree(
        SessionType::Tmux,
        Some("remote-host"),
        Some("/custom/dir"),
        "main",
    );

    assert!(filter_calls(&runner, "command -v tmux").is_empty());
    assert!(filter_calls(&runner, "tmux has-session").is_empty());
    assert!(filter_calls(&runner, "tmux new-session").is_empty());
}

fn layout(layout_type: PaneLayoutType, commands: &[(i64, &str)]) -> PaneLayoutSelection {
    PaneLayoutSelection {
        layout_type,
        panes: commands
            .iter()
            .map(|(position, command)| PaneCommand {
                position: *position,
                command: command.to_string(),
            })
            .collect(),
    }
}

/// 임시 디렉터리 아래 프로젝트에 zellij worktree를 만들고 기록된 레이아웃 파일 내용을 돌려준다
fn create_local_zellij_worktree(
    pane_layout: &PaneLayoutLookup<'_>,
) -> (String, String, Arc<ScriptedCommandRunner>) {
    let root = tempfile::tempdir().unwrap();
    let project_path = root.path().join("kanvibe").to_string_lossy().into_owned();
    let runner = returning("");
    let files = file_access(&runner);

    let session = create_worktree_with_session(
        &files,
        &project_path,
        "feat-new",
        "main",
        SessionType::Zellij,
        None,
        pane_layout,
    )
    .unwrap();

    let layout_path = format!("{}/{ZELLIJ_LAYOUT_FILENAME}", session.worktree_path);
    let written_kdl = std::fs::read_to_string(&layout_path).unwrap();
    (layout_path, written_kdl, runner)
}

#[test]
fn remote_worktree_creation_runs_only_git_worktree_add() {
    let runner = returning("");
    let files = file_access(&runner);

    create_worktree_with_session(
        &files,
        "/home/user/kanvibe",
        "feat-new",
        "main",
        SessionType::Tmux,
        Some("remote-create-host"),
        &no_layout,
    )
    .unwrap();

    assert!(filter_calls(&runner, "command -v tmux").is_empty());
    assert!(filter_calls(&runner, "tmux has-session").is_empty());
    assert!(filter_calls(&runner, "tmux new-session").is_empty());
    assert_eq!(filter_calls(&runner, "git -C").len(), 1);
}

#[test]
fn local_worktree_creation_runs_only_git_worktree_add() {
    let runner = returning("");
    let files = file_access(&runner);

    let session = create_worktree_with_session(
        &files,
        "/home/user/kanvibe",
        "feat-local",
        "main",
        SessionType::Tmux,
        None,
        &no_layout,
    )
    .unwrap();

    assert!(filter_calls(&runner, "tmux has-session").is_empty());
    assert!(filter_calls(&runner, "tmux new-session").is_empty());
    assert_eq!(
        runner.commands(),
        [
            "git -C '/home/user/kanvibe' worktree add '/home/user/kanvibe__worktrees/feat-local' -b 'feat-local' 'main'"
        ]
    );
    assert_eq!(session.session_name, "kanvibe-feat-local");
}

#[test]
fn zellij_layout_file_is_written_without_starting_zellij() {
    let lookup = || {
        Ok(Some(layout(
            PaneLayoutType::Vertical2,
            &[(0, "pnpm dev"), (1, "pnpm test")],
        )))
    };

    let (layout_path, written_kdl, runner) = create_local_zellij_worktree(&lookup);

    assert!(layout_path.contains("kanvibe__worktrees/feat-new"));
    assert!(layout_path.contains(".zellij-layout.kdl"));
    assert!(written_kdl.contains(r#"split_direction="vertical""#));
    assert!(written_kdl.contains(r#""pnpm dev""#));
    assert!(filter_calls(&runner, "zellij").is_empty());
}

#[test]
fn single_layout_still_writes_a_bar_free_layout_file() {
    let lookup = || Ok(Some(layout(PaneLayoutType::Single, &[(0, "echo hello")])));

    let (_, written_kdl, _) = create_local_zellij_worktree(&lookup);

    assert!(written_kdl.contains("pane"));
    assert!(!written_kdl.contains("zellij:tab-bar"));
    assert!(!written_kdl.contains("zellij:status-bar"));
}

#[test]
fn remote_layout_file_is_written_through_the_remote_runner() {
    let runner = returning("");
    let files = file_access(&runner);
    let lookup = || {
        Ok(Some(layout(
            PaneLayoutType::Vertical2,
            &[(0, "pnpm dev"), (1, "pnpm test")],
        )))
    };

    create_worktree_with_session(
        &files,
        "/home/user/kanvibe",
        "feat-new",
        "main",
        SessionType::Zellij,
        Some("remote-host"),
        &lookup,
    )
    .unwrap();

    let remote_layout_writes: Vec<RecordedCommand> = runner
        .calls()
        .into_iter()
        .filter(|call| call.command.contains(".zellij-layout.kdl"))
        .collect();
    assert_eq!(remote_layout_writes.len(), 1);
    assert_eq!(
        remote_layout_writes[0].ssh_host.as_deref(),
        Some("remote-host")
    );
}

#[test]
fn layout_lookup_failure_falls_back_to_a_single_pane_file() {
    let lookup = || Err("DB error".to_string());

    let (_, written_kdl, _) = create_local_zellij_worktree(&lookup);

    assert!(!written_kdl.contains("zellij:tab-bar"));
    assert!(written_kdl.contains("pane cwd="));
}

#[test]
fn three_pane_layout_is_written_to_the_worktree() {
    let commands = [
        (0, "claude --dangerously-skip-permissions"),
        (1, "pnpm dev"),
        (2, "pnpm test:watch"),
    ];
    let lookup = || Ok(Some(layout(PaneLayoutType::LeftRightTb, &commands)));

    let (_, written_kdl, _) = create_local_zellij_worktree(&lookup);

    for (_, command) in commands {
        assert!(written_kdl.contains(&format!("\"{command}\"")));
    }
}

#[test]
fn quad_layout_is_written_to_the_worktree() {
    let commands = [(0, "cmd0"), (1, "cmd1"), (2, "cmd2"), (3, "cmd3")];
    let lookup = || Ok(Some(layout(PaneLayoutType::Quad, &commands)));

    let (_, written_kdl, _) = create_local_zellij_worktree(&lookup);

    for (_, command) in commands {
        assert!(written_kdl.contains(&format!("\"{command}\"")));
    }
    assert!(written_kdl.contains(r#"split_direction="vertical""#));
}

#[test]
fn zellij_session_without_worktree_does_not_start_zellij() {
    let (session_name, runner) =
        create_without_worktree(SessionType::Zellij, None, None, "feat/test");

    assert_eq!(session_name, "path-feat-test");
    assert!(filter_calls(&runner, "zellij").is_empty());
}

#[test]
fn herdr_session_for_an_existing_directory_uses_the_shared_spaces_reference() {
    let runner = returning("");
    let files = file_access(&runner);

    let session_name = create_session_without_worktree(
        &files,
        "/work/project",
        "feature/login",
        SessionType::Herdr,
        None,
        None,
        &no_layout,
    );

    assert_eq!(session_name, "kanvibe:project-feature-login");
    assert!(runner.commands().is_empty());
}

#[test]
fn herdr_session_for_a_new_worktree_uses_the_shared_spaces_reference() {
    let runner = returning("");
    let files = file_access(&runner);

    let session = create_worktree_with_session(
        &files,
        "/work/project",
        "feature/login",
        "main",
        SessionType::Herdr,
        None,
        &no_layout,
    )
    .unwrap();

    assert_eq!(session.session_name, "kanvibe:project-feature-login");
    assert_eq!(
        session.worktree_path,
        "/work/project__worktrees/feature-login"
    );
}

#[test]
fn remote_home_expands_while_trailing_shell_characters_stay_literal() {
    let suffix = "/$ABSENT$(printf nope)`printf nope`\"'";
    let quoted = resolve_path_for_shell(&format!("~{suffix}"), Some("fixture"), LOCAL_HOME);

    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("printf '%s' {quoted}"))
        .env_clear()
        .env("HOME", "/fixture/home")
        .output()
        .unwrap();

    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("/fixture/home{suffix}")
    );
}

fn run_git(arguments: &[&str], home: &std::path::Path) {
    let status = Command::new("git")
        .args(arguments)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success());
}

/// 셸 특수문자를 모두 담은 이름의 저장소를 만들고 경로를 돌려준다
fn create_literal_repository(root_path: &std::path::Path) -> String {
    let repo = root_path.join("repo$ABSENT$(printf substituted)`printf tick`\"quote'");
    let repo_text = repo.to_string_lossy().into_owned();
    std::fs::create_dir(&repo).unwrap();
    run_git(&["init", "-q", "-b", "main", &repo_text], root_path);
    let commit_arguments = [
        "-C",
        &repo_text,
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "fixture",
    ];
    run_git(&commit_arguments, root_path);
    repo_text
}

/// 테스트 HOME만 담은 좁은 셸 환경으로 실제 프로세스를 띄우는 실행기
fn system_runner(home: &str) -> Arc<dyn ShellCommandRunner> {
    let parent_environment = [
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        ),
        ("HOME".to_string(), home.to_string()),
        ("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string()),
    ]
    .into();
    let environment = crate::shell_environment::create_local_shell_environment(
        &parent_environment,
        crate::shell_environment::ShellPlatform::current(),
        home,
    );
    Arc::new(crate::exec::system::SystemShellCommandRunner::new(
        environment,
        home,
        crate::exec::remote::RemoteSshSettings::default(),
    ))
}

#[test]
fn worktree_lifecycle_keeps_special_characters_in_paths_literal() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let repo_text = create_literal_repository(&root_path);
    let home = root_path.to_string_lossy().into_owned();
    let files = HostFileAccess::new(system_runner(&home), home.clone());

    assert!(crate::git_operations::validate_git_repo(
        &files, &repo_text, None
    ));
    assert_eq!(
        crate::git_operations::get_default_branch(&files, &repo_text, None),
        "main"
    );
    let branches = crate::git_operations::list_branches(&files, &repo_text, None, false).unwrap();
    assert!(branches.contains(&"main".to_string()));
    let initial_worktrees = list_worktrees(&files, &repo_text, None);
    assert_eq!(initial_worktrees.len(), 1);
    assert_eq!(initial_worktrees[0].path, repo_text);
    assert_eq!(initial_worktrees[0].branch, "main");
    let root_text = root_path.to_string_lossy().into_owned();
    assert!(crate::git_operations::scan_git_repos(&files, &root_text, None).contains(&repo_text));
    let created = create_worktree_with_session(
        &files,
        &repo_text,
        "feature/literal",
        "main",
        SessionType::Terminal,
        None,
        &no_layout,
    )
    .unwrap();
    let listed = list_worktrees(&files, &repo_text, None);
    assert!(
        listed
            .iter()
            .any(|worktree| worktree.path == created.worktree_path)
    );
    let options = cleanup_options(true, Some(&created.worktree_path));
    remove_worktree_and_branch(&files, &repo_text, "feature/literal", None, &options).unwrap();
    assert_eq!(list_worktrees(&files, &repo_text, None).len(), 1);
}
