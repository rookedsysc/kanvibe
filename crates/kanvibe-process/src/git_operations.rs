//! `exec_git` 위에 얹은 git 헬퍼(TS `gitOperations.ts`의 git 함수들).
//! 로컬과 원격이 같은 셸 문자열을 쓰고, `~` 경로만 로컬은 사용자 홈으로, 원격은 원격 `$HOME`으로 바꾼다.

use crate::exec::{ExecGitError, ExecGitOptions};
use crate::host_file_access::HostFileAccess;

pub use crate::exec::{
    is_ssh_connection_error, is_ssh_transport_error, run_with_background_ssh_commands,
};

use crate::worktree::quote_for_posix_shell;

/// 셸 명령에 넣을 경로. `~`로 시작하면 로컬은 홈 디렉터리로 펼치고, 원격은 원격 `$HOME` 기준으로 둔다
pub fn resolve_path_for_shell(
    target_path: &str,
    ssh_host: Option<&str>,
    local_home_directory: &str,
) -> String {
    let Some(suffix) = target_path.strip_prefix('~') else {
        return quote_for_posix_shell(target_path);
    };
    if ssh_host.is_some_and(|host| !host.is_empty()) {
        return format!("\"$HOME\"{}", quote_for_posix_shell(suffix));
    }
    quote_for_posix_shell(&format!("{local_home_directory}{suffix}"))
}

fn repository_path(access: &HostFileAccess, repo_path: &str, ssh_host: Option<&str>) -> String {
    resolve_path_for_shell(repo_path, ssh_host, access.local_home_directory())
}

/// remote origin의 최신 브랜치 정보를 가져온다. 네트워크 실패에도 로컬 브랜치 목록은 정상 제공해야 하므로 실패를 무시한다
pub fn fetch_origin(access: &HostFileAccess, repo_path: &str, ssh_host: Option<&str>) {
    let command = format!(
        "git -C {} fetch --prune",
        repository_path(access, repo_path, ssh_host)
    );
    let _ = access
        .runner()
        .exec_git(&command, ssh_host, &ExecGitOptions::default());
}

/// 현재 checkout된 브랜치를 fast-forward 가능한 경우에만 pull한다
pub fn pull_current_branch(
    access: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
    options: &ExecGitOptions,
) -> Result<String, ExecGitError> {
    let command = format!(
        "git -C {} pull --ff-only",
        repository_path(access, repo_path, ssh_host)
    );
    access.runner().exec_git(&command, ssh_host, options)
}

/// origin에 브랜치가 있는지. `ls-remote --exit-code`가 2면 없음, 그 밖의 실패는 오류다
pub fn remote_branch_exists(
    access: &HostFileAccess,
    repo_path: &str,
    branch_name: &str,
    ssh_host: Option<&str>,
    options: &ExecGitOptions,
) -> Result<bool, ExecGitError> {
    let remote_ref = quote_for_posix_shell(&format!("refs/heads/{branch_name}"));
    let command = [
        format!(
            "git -C {} ls-remote --exit-code --heads origin {remote_ref} >/dev/null",
            repository_path(access, repo_path, ssh_host)
        ),
        "status=$?".to_string(),
        "if [ \"$status\" -eq 0 ]; then printf exists; elif [ \"$status\" -eq 2 ]; then printf missing; else exit \"$status\"; fi".to_string(),
    ]
    .join("; ");
    let output = access.runner().exec_git(&command, ssh_host, options)?;
    Ok(output.trim() == "exists")
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

/// 브랜치 목록. 기본으로 origin을 먼저 fetch하고, 로컬에 없는 remote-only 브랜치는 git ref 해석이 정확하도록 `origin/`을 유지한다
pub fn list_branches(
    access: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
    should_refresh: bool,
) -> Result<Vec<String>, ExecGitError> {
    if should_refresh {
        fetch_origin(access, repo_path, ssh_host);
    }
    let command = format!(
        "git -C {} branch -a --format='%(refname:short)'",
        repository_path(access, repo_path, ssh_host)
    );
    let output = access
        .runner()
        .exec_git(&command, ssh_host, &ExecGitOptions::default())?;
    let all_branches: Vec<&str> = output
        .split('\n')
        .filter(|branch| !branch.is_empty())
        .collect();
    let mut local_branches: Vec<String> = Vec::new();
    for branch in all_branches
        .iter()
        .filter(|branch| !branch.starts_with("origin/"))
    {
        push_unique(&mut local_branches, branch.to_string());
    }
    let remote_only_branches = all_branches
        .iter()
        .filter_map(|branch| branch.strip_prefix("origin/"))
        .filter(|branch| *branch != "HEAD" && !local_branches.iter().any(|local| local == branch))
        .map(|branch| format!("origin/{branch}"))
        .collect::<Vec<_>>();
    let mut branches = local_branches;
    for branch in remote_only_branches {
        push_unique(&mut branches, branch);
    }
    Ok(branches)
}

/// 경로가 git 작업 트리인지
pub fn validate_git_repo(access: &HostFileAccess, repo_path: &str, ssh_host: Option<&str>) -> bool {
    let command = format!(
        "git -C {} rev-parse --is-inside-work-tree",
        repository_path(access, repo_path, ssh_host)
    );
    access
        .runner()
        .exec_git(&command, ssh_host, &ExecGitOptions::default())
        .is_ok()
}

/// HEAD가 가리키는 브랜치. 알 수 없으면 `main`이다
pub fn get_default_branch(
    access: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
) -> String {
    let command = format!(
        "git -C {} symbolic-ref --short HEAD",
        repository_path(access, repo_path, ssh_host)
    );
    match access
        .runner()
        .exec_git(&command, ssh_host, &ExecGitOptions::default())
    {
        Ok(output) if !output.is_empty() => output,
        _ => "main".to_string(),
    }
}

/// 하위 git 저장소 경로 목록. 일반 저장소의 `.git` 디렉터리와 worktree의 `.git` 파일을 모두 찾는다
pub fn scan_git_repos(
    access: &HostFileAccess,
    root_path: &str,
    ssh_host: Option<&str>,
) -> Vec<String> {
    let resolved_path = resolve_path_for_shell(root_path, ssh_host, access.local_home_directory());
    let command = format!(
        "find {resolved_path} -maxdepth 4 -name \".git\" \\( -type d -o -type f \\) 2>/dev/null"
    );
    let output = match access
        .runner()
        .exec_git(&command, ssh_host, &ExecGitOptions::default())
    {
        Ok(output) => output,
        Err(error) => {
            eprintln!(
                "[remote-scan] git repository scan failed sshHost={ssh_host:?} rootPath={root_path} resolvedPath={resolved_path} command={command} error={}",
                error.message
            );
            return Vec::new();
        }
    };
    let mut repositories = Vec::new();
    for git_directory in output.split('\n').filter(|line| !line.is_empty()) {
        let repository = git_directory.strip_suffix("/.git").unwrap_or(git_directory);
        push_unique(&mut repositories, repository.to_string());
    }
    repositories
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub branch: String,
    pub is_bare: bool,
}

fn parse_worktree_block(block: &str) -> Option<WorktreeEntry> {
    let mut entry = WorktreeEntry {
        path: String::new(),
        branch: String::new(),
        is_bare: false,
    };
    for line in block.split('\n') {
        if line.starts_with("worktree ") {
            entry.path = line.replacen("worktree ", "", 1);
        } else if line.starts_with("branch ") {
            entry.branch = line.replacen("branch refs/heads/", "", 1);
        } else if line == "bare" {
            entry.is_bare = true;
        }
    }
    (!entry.path.is_empty()).then_some(entry)
}

/// 프로젝트의 git worktree 목록(bare 포함). 실패하면 빈 목록이다
pub fn list_worktrees(
    access: &HostFileAccess,
    repo_path: &str,
    ssh_host: Option<&str>,
) -> Vec<WorktreeEntry> {
    let command = format!(
        "git -C {} worktree list --porcelain",
        repository_path(access, repo_path, ssh_host)
    );
    let Ok(output) = access
        .runner()
        .exec_git(&command, ssh_host, &ExecGitOptions::default())
    else {
        return Vec::new();
    };
    output
        .split("\n\n")
        .filter(|block| !block.is_empty())
        .filter_map(parse_worktree_block)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::exec::testing::ScriptedCommandRunner;

    fn access_with(runner: ScriptedCommandRunner) -> (HostFileAccess, Arc<ScriptedCommandRunner>) {
        let runner = Arc::new(runner);
        (
            HostFileAccess::new(runner.clone(), "/home/local-user"),
            runner,
        )
    }

    // parity: TS-lib-gitOperations-131d5b
    #[test]
    fn local_tilde_paths_expand_to_the_local_home_directory() {
        assert_eq!(
            resolve_path_for_shell("~/work", None, "/home/local-user"),
            "'/home/local-user/work'"
        );
    }

    // parity: TS-lib-gitOperations-0354bd
    #[test]
    fn remote_tilde_paths_resolve_against_the_remote_home() {
        assert_eq!(
            resolve_path_for_shell("~/work", Some("remote-host"), "/home/local-user"),
            "\"$HOME\"'/work'"
        );
    }

    // parity: TS-lib-gitOperations-570fed
    #[test]
    fn remote_branch_exists_returns_true_when_origin_has_the_branch() {
        let (access, runner) = access_with(ScriptedCommandRunner::returning("exists"));

        let exists = remote_branch_exists(
            &access,
            "/workspace/repo",
            "feature/exists",
            None,
            &ExecGitOptions::default(),
        );

        assert!(exists.unwrap());
        assert!(runner.commands()[0].contains("refs/heads/feature/exists"));
    }

    // parity: TS-lib-gitOperations-73d032
    #[test]
    fn remote_branch_exists_returns_false_when_origin_lacks_the_branch() {
        let (access, runner) = access_with(ScriptedCommandRunner::returning("missing"));

        let exists = remote_branch_exists(
            &access,
            "/workspace/repo",
            "feature/missing",
            None,
            &ExecGitOptions::default(),
        );

        assert!(!exists.unwrap());
        assert!(runner.commands()[0].contains("refs/heads/feature/missing"));
    }

    // parity: TS-lib-gitOperations-ddd236
    #[test]
    fn scan_git_repos_finds_regular_and_worktree_repositories() {
        let (access, runner) = access_with(ScriptedCommandRunner::returning(
            "/workspace/api/.git\n/workspace/feature-worktree/.git",
        ));

        let repositories = scan_git_repos(&access, "/workspace", None);

        assert_eq!(
            runner.commands(),
            ["find '/workspace' -maxdepth 4 -name \".git\" \\( -type d -o -type f \\) 2>/dev/null"]
        );
        assert_eq!(
            repositories,
            ["/workspace/api", "/workspace/feature-worktree"]
        );
    }

    #[test]
    fn list_branches_keeps_origin_prefix_only_for_remote_only_branches() {
        let (access, runner) = access_with(ScriptedCommandRunner::returning(
            "main\nfeature\norigin/HEAD\norigin/main\norigin/remote-only",
        ));

        let branches = list_branches(&access, "/repo", None, true).unwrap();

        assert_eq!(branches, ["main", "feature", "origin/remote-only"]);
        assert_eq!(runner.commands()[0], "git -C '/repo' fetch --prune");
    }

    #[test]
    fn list_worktrees_parses_porcelain_blocks() {
        let output =
            "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\nworktree /repo/bare\nbare";
        let (access, _runner) = access_with(ScriptedCommandRunner::returning(output));

        let worktrees = list_worktrees(&access, "/repo", None);

        assert_eq!(
            worktrees,
            [
                WorktreeEntry {
                    path: "/repo".into(),
                    branch: "main".into(),
                    is_bare: false
                },
                WorktreeEntry {
                    path: "/repo/bare".into(),
                    branch: String::new(),
                    is_bare: true
                },
            ]
        );
    }

    #[test]
    fn default_branch_falls_back_to_main() {
        let (access, _runner) = access_with(ScriptedCommandRunner::new(|_| {
            Err(ExecGitError::new("detached"))
        }));

        assert_eq!(get_default_branch(&access, "/repo", None), "main");
        assert!(!validate_git_repo(&access, "/repo", None));
    }
}
