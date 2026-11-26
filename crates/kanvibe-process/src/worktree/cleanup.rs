//! worktree와 브랜치 삭제. 등록된 worktree를 찾아 지우고, git이 잊은 관리 폴더를 안전 검사 뒤 지운 다음 브랜치를 지운다.

use super::WorktreeCleanupError;
use super::naming::{
    build_managed_worktree_base_path, build_managed_worktree_path, posix_normalize,
    quote_for_posix_shell,
};
use crate::exec::{ExecGitOptions, ShellCommandRunner};
use crate::git_operations::{WorktreeEntry, list_worktrees, resolve_path_for_shell};
use crate::host_file_access::HostFileAccess;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceCleanupOptions {
    /// 정리 실패를 오류로 알린다. 끄면 이미 지워진 자원으로 보고 넘어간다
    pub should_throw_on_error: bool,
    /// task에 저장된 worktree 경로. git 목록에서 브랜치를 찾지 못할 때의 대체 후보다
    pub worktree_path: Option<String>,
}

struct ManagedWorktreePaths {
    project_path: String,
    base_path: String,
    managed_path: String,
}

struct ResolvedBranchWorktree {
    path: Option<String>,
    is_project_root_checkout: bool,
    registered_worktrees: Vec<WorktreeEntry>,
}

fn normalize_git_worktree_path(worktree_path: &str) -> String {
    let normalized = posix_normalize(worktree_path);
    let trimmed = normalized.trim_end_matches('/');
    if trimmed.is_empty() {
        normalized
    } else {
        trimmed.to_string()
    }
}

fn managed_worktree_paths(project_path: &str, branch_name: &str) -> ManagedWorktreePaths {
    ManagedWorktreePaths {
        project_path: normalize_git_worktree_path(project_path),
        base_path: normalize_git_worktree_path(&build_managed_worktree_base_path(project_path)),
        managed_path: normalize_git_worktree_path(&build_managed_worktree_path(
            project_path,
            branch_name,
        )),
    }
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
}

/// 지워도 되는 후보 폴더. 관리 폴더 아래이면서 이번 task의 관리 경로이거나 넘겨받은 후보여야 한다
fn managed_cleanup_candidate_paths(
    project_path: &str,
    branch_name: &str,
    candidates: &[Option<&str>],
) -> Vec<String> {
    let paths = managed_worktree_paths(project_path, branch_name);
    let normalized_candidates: Vec<String> = candidates
        .iter()
        .filter_map(|candidate| non_blank(*candidate))
        .map(normalize_git_worktree_path)
        .collect();
    let mut allowed_paths: Vec<String> = vec![paths.managed_path.clone()];
    for candidate in &normalized_candidates {
        if !allowed_paths.contains(candidate) {
            allowed_paths.push(candidate.clone());
        }
    }
    let base_prefix = format!("{}/", paths.base_path);
    allowed_paths
        .into_iter()
        .filter(|candidate| {
            *candidate != paths.project_path
                && *candidate != paths.base_path
                && candidate.starts_with(&base_prefix)
                && (*candidate == paths.managed_path || normalized_candidates.contains(candidate))
        })
        .collect()
}

/// 셸에서 경로를 다시 확인한 뒤에만 지우는 명령. 프로젝트 루트, 관리 폴더 밖, 심볼릭 링크는 건드리지 않는다
fn build_managed_worktree_directory_cleanup_command(
    project_path: &str,
    branch_name: &str,
    candidate_path: &str,
    fallback_worktree_path: Option<&str>,
) -> String {
    let paths = managed_worktree_paths(project_path, branch_name);
    let normalized_fallback_path =
        non_blank(fallback_worktree_path).map(normalize_git_worktree_path);
    let target = quote_for_posix_shell(candidate_path);
    let project_root = quote_for_posix_shell(&paths.project_path);
    let base_prefix_pattern = format!(
        "{}*",
        quote_for_posix_shell(&format!("{}/", paths.base_path))
    );
    let mut allowed_values: Vec<String> = vec![paths.managed_path.clone()];
    if let Some(fallback) =
        normalized_fallback_path.filter(|fallback| !allowed_values.contains(fallback))
    {
        allowed_values.push(fallback);
    }
    let allowed_path_checks = allowed_values
        .iter()
        .map(|value| format!("[ {target} = {} ]", quote_for_posix_shell(value)))
        .collect::<Vec<_>>()
        .join(" || ");
    [
        format!("test -n {target}"),
        format!("if [ {target} = {project_root} ]; then exit 0; fi"),
        format!("case {target} in {base_prefix_pattern}) ;; *) exit 0;; esac"),
        format!("if ! {{ {allowed_path_checks}; }}; then exit 0; fi"),
        format!("test -d {target} || exit 0"),
        format!("test ! -L {target}"),
        format!("rm -rf -- {target}"),
        format!("test ! -e {target}"),
    ]
    .join("; ")
}

fn remove_stale_managed_worktree_directories(
    runner: &dyn ShellCommandRunner,
    project_path: &str,
    branch_name: &str,
    candidates: &[Option<&str>],
    registered_worktrees: &[WorktreeEntry],
    ssh_host: Option<&str>,
    options: &ResourceCleanupOptions,
) {
    let registered_paths: Vec<String> = registered_worktrees
        .iter()
        .map(|worktree| normalize_git_worktree_path(&worktree.path))
        .collect();
    let cleanup_paths = managed_cleanup_candidate_paths(project_path, branch_name, candidates)
        .into_iter()
        .filter(|cleanup_path| !registered_paths.contains(cleanup_path));
    for cleanup_path in cleanup_paths {
        let command = build_managed_worktree_directory_cleanup_command(
            project_path,
            branch_name,
            &cleanup_path,
            options.worktree_path.as_deref(),
        );
        // 폴더를 지우지 못해도 남은 후보 폴더와 브랜치 정리는 이어간다
        if let Err(error) = runner.exec_git(&command, ssh_host, &ExecGitOptions::default()) {
            eprintln!(
                "managed worktree 디렉토리 정리 실패, 다음 정리 단계로 넘어갑니다: {cleanup_path} {error}"
            );
        }
    }
}

fn resolve_branch_worktree_path(
    files: &HostFileAccess,
    project_path: &str,
    branch_name: &str,
    fallback_worktree_path: Option<&str>,
    ssh_host: Option<&str>,
) -> ResolvedBranchWorktree {
    let worktrees = list_worktrees(files, project_path, ssh_host);
    let paths = managed_worktree_paths(project_path, branch_name);
    let normalized_fallback = fallback_worktree_path
        .filter(|path| !path.is_empty())
        .map(normalize_git_worktree_path);
    let is_branch_checkout =
        |worktree: &&WorktreeEntry| !worktree.is_bare && worktree.branch == branch_name;
    let linked_worktree = worktrees
        .iter()
        .filter(is_branch_checkout)
        .find(|worktree| normalize_git_worktree_path(&worktree.path) != paths.project_path);
    let fallback_worktree = normalized_fallback.as_ref().and_then(|fallback| {
        worktrees.iter().find(|worktree| {
            !worktree.is_bare
                && normalize_git_worktree_path(&worktree.path) == *fallback
                && *fallback == paths.managed_path
                && *fallback != paths.project_path
        })
    });
    let is_project_root_checkout = worktrees
        .iter()
        .filter(is_branch_checkout)
        .any(|worktree| normalize_git_worktree_path(&worktree.path) == paths.project_path);
    ResolvedBranchWorktree {
        path: linked_worktree
            .or(fallback_worktree)
            .map(|worktree| worktree.path.clone()),
        is_project_root_checkout,
        registered_worktrees: worktrees.clone(),
    }
}

/// 등록된 worktree를 지운다. 폴더의 .git 파일이 사라진 worktree는 remove가 거부되므로 prune으로 깨진 등록을 정리하고,
/// prune마저 실패해도 폴더·브랜치 정리를 이어가도록 오류를 넘기지 않는다.
fn remove_registered_worktree(
    files: &HostFileAccess,
    project_path: &str,
    worktree_path: &str,
    ssh_host: Option<&str>,
    options: &ResourceCleanupOptions,
) {
    let home = files.local_home_directory();
    let project_path_for_shell = resolve_path_for_shell(project_path, ssh_host, home);
    let remove_command = format!(
        "git -C {project_path_for_shell} worktree remove {} --force",
        resolve_path_for_shell(worktree_path, ssh_host, home)
    );
    let worktree_command = match options.should_throw_on_error {
        true => format!(
            "if git -C {project_path_for_shell} worktree list --porcelain | grep -Fxq {}; then {remove_command}; fi",
            quote_for_posix_shell(&format!("worktree {worktree_path}"))
        ),
        false => remove_command,
    };
    let runner = files.runner();
    let Err(error) = runner.exec_git(&worktree_command, ssh_host, &ExecGitOptions::default())
    else {
        return;
    };
    eprintln!("worktree 제거 실패, prune으로 등록 정리를 시도합니다: {worktree_path} {error}");
    let prune_command = format!("git -C {project_path_for_shell} worktree prune");
    if let Err(error) = runner.exec_git(&prune_command, ssh_host, &ExecGitOptions::default()) {
        eprintln!("worktree prune 실패, 다음 정리 단계로 넘어갑니다: {worktree_path} {error}");
    }
}

fn build_branch_delete_command(
    project_path_for_shell: &str,
    branch_name: &str,
    should_verify_branch: bool,
) -> String {
    let delete_command = format!(
        "git -C {project_path_for_shell} branch -D {}",
        quote_for_posix_shell(branch_name)
    );
    if !should_verify_branch {
        return delete_command;
    }
    format!(
        "if git -C {project_path_for_shell} show-ref --verify --quiet {}; then {delete_command}; fi",
        quote_for_posix_shell(&format!("refs/heads/{branch_name}"))
    )
}

/// worktree와 브랜치를 지운다. 세션은 건드리지 않는다
pub fn remove_worktree_and_branch(
    files: &HostFileAccess,
    project_path: &str,
    branch_name: &str,
    ssh_host: Option<&str>,
    options: &ResourceCleanupOptions,
) -> Result<(), WorktreeCleanupError> {
    let fallback_path = options.worktree_path.as_deref();
    let resolved =
        resolve_branch_worktree_path(files, project_path, branch_name, fallback_path, ssh_host);
    if let Some(worktree_path) = &resolved.path {
        remove_registered_worktree(files, project_path, worktree_path, ssh_host, options);
    }
    let managed_path = build_managed_worktree_path(project_path, branch_name);
    let candidates = [
        resolved.path.as_deref(),
        fallback_path,
        Some(managed_path.as_str()),
    ];
    remove_stale_managed_worktree_directories(
        files.runner(),
        project_path,
        branch_name,
        &candidates,
        &resolved.registered_worktrees,
        ssh_host,
        options,
    );
    if resolved.is_project_root_checkout {
        return Ok(());
    }
    let project_path_for_shell =
        resolve_path_for_shell(project_path, ssh_host, files.local_home_directory());
    let branch_command = build_branch_delete_command(
        &project_path_for_shell,
        branch_name,
        options.should_throw_on_error,
    );
    // 브랜치가 이미 지워졌으면 무시하고, 정리 실패를 알려야 할 때만 오류로 돌려준다
    match files
        .runner()
        .exec_git(&branch_command, ssh_host, &ExecGitOptions::default())
    {
        Err(_) if options.should_throw_on_error => {
            Err(WorktreeCleanupError::Branch(branch_name.to_string()))
        }
        _ => Ok(()),
    }
}
