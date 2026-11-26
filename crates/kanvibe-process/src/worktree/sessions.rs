//! worktree·세션 생성과 세션 정리·생존 확인.
//! tmux와 terminal 세션은 터미널 연결 시점에 만들어지고, zellij는 TTY 없이 실행할 수 없어 연결 시점에 쓸 레이아웃 파일만 미리 남긴다.

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use kanvibe_domain::task::SessionType;

use super::WorktreeCleanupError;
use super::cleanup::ResourceCleanupOptions;
use super::naming::{
    build_managed_worktree_path, format_project_branch_session_name, posix_join,
    quote_for_posix_shell, sanitize_herdr_session_name, sanitize_zellij_session_name,
};
use super::tmux::{
    PaneLayoutSelection, build_tmux_alive_check_command, build_tmux_session_cleanup_command,
};
use super::zellij::{
    ZELLIJ_LAYOUT_FILENAME, build_zellij_session_cleanup_command, generate_zellij_layout_kdl,
    parse_alive_zellij_session_names,
};
use crate::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use crate::git_operations::resolve_path_for_shell;
use crate::host_file_access::{HostFileAccess, HostFileError};

/// 프로젝트(없으면 전역)에 적용할 pane 레이아웃을 찾는다. TS `getEffectivePaneLayout` 자리이며 실패는 오류 문자열로 돌려준다
pub type PaneLayoutLookup<'a> = dyn Fn() -> Result<Option<PaneLayoutSelection>, String> + 'a;

/// Herdr 공유 서버의 task workspace 조작(TS `lib/herdr.ts`). 구현은 터미널 계층이 맡는다
pub trait HerdrTaskWorkspaces {
    fn close_task_workspace(
        &self,
        session_name: &str,
        ssh_host: Option<&str>,
    ) -> Result<(), String>;
    fn is_task_alive(&self, session_name: &str, ssh_host: Option<&str>) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSession {
    pub worktree_path: String,
    pub session_name: String,
}

fn build_session_name(project_path: &str, branch_name: &str, session_type: SessionType) -> String {
    let raw_session_name = format_project_branch_session_name(project_path, branch_name);
    match session_type {
        SessionType::Herdr => format!("kanvibe:{}", sanitize_herdr_session_name(&raw_session_name)),
        _ => raw_session_name,
    }
}

/// KanVibe가 만드는 zellij 세션이 쓸 레이아웃 파일을 준비한다.
/// pane 분할을 쓰지 않아도 파일을 반드시 남긴다. 레이아웃을 주지 않으면 zellij가 기본 레이아웃의
/// tab-bar·status-bar 플러그인을 붙여 KanVibe 탭 바와 같은 정보가 두 줄로 나온다.
fn prepare_zellij_layout_file(
    files: &HostFileAccess,
    worktree_path: &str,
    pane_layout: &PaneLayoutLookup<'_>,
    ssh_host: Option<&str>,
) -> Result<(), HostFileError> {
    let (layout_type, panes): (PaneLayoutType, Vec<PaneCommand>) = match pane_layout() {
        Ok(Some(selection)) => (selection.layout_type, selection.panes),
        Ok(None) => (PaneLayoutType::Single, Vec::new()),
        Err(error) => {
            eprintln!("Zellij pane 레이아웃 조회 실패 (단일 pane으로 생성): {error}");
            (PaneLayoutType::Single, Vec::new())
        }
    };
    let layout_path = posix_join(worktree_path, ZELLIJ_LAYOUT_FILENAME);
    let kdl = generate_zellij_layout_kdl(layout_type, &panes, worktree_path);
    files.write_text_file(&layout_path, &kdl, ssh_host, &ExecGitOptions::default())
}

fn prepare_zellij_layout_file_or_log(
    files: &HostFileAccess,
    worktree_path: &str,
    pane_layout: &PaneLayoutLookup<'_>,
    ssh_host: Option<&str>,
) {
    if let Err(error) = prepare_zellij_layout_file(files, worktree_path, pane_layout, ssh_host) {
        eprintln!("Zellij 레이아웃 파일 생성 실패 (레이아웃 없이 세션 생성 예정): {error}");
    }
}

/// git worktree를 만들고 브랜치별 독립 세션 이름을 정한다. 세션 자체는 터미널 연결 시점에 만든다
pub fn create_worktree_with_session(
    files: &HostFileAccess,
    project_path: &str,
    branch_name: &str,
    base_branch: &str,
    session_type: SessionType,
    ssh_host: Option<&str>,
    pane_layout: &PaneLayoutLookup<'_>,
) -> Result<WorktreeSession, ExecGitError> {
    let worktree_path = build_managed_worktree_path(project_path, branch_name);
    let session_name = build_session_name(project_path, branch_name, session_type);
    let home = files.local_home_directory();
    let command = format!(
        "git -C {} worktree add {} -b {} {}",
        resolve_path_for_shell(project_path, ssh_host, home),
        resolve_path_for_shell(&worktree_path, ssh_host, home),
        quote_for_posix_shell(branch_name),
        quote_for_posix_shell(base_branch)
    );
    files
        .runner()
        .exec_git(&command, ssh_host, &ExecGitOptions::default())?;

    if session_type != SessionType::Zellij {
        return Ok(WorktreeSession {
            worktree_path,
            session_name,
        });
    }
    // 원격도 로컬과 같은 레이아웃으로 열리도록 KDL 레이아웃 파일을 worktree 디렉터리에 저장한다
    prepare_zellij_layout_file_or_log(files, &worktree_path, pane_layout, ssh_host);
    Ok(WorktreeSession {
        worktree_path,
        session_name: sanitize_zellij_session_name(&session_name),
    })
}

/// worktree 없이 기존 디렉터리에서 쓸 세션 이름을 정한다. zellij는 연결 시점에 쓸 레이아웃 파일만 미리 남긴다
pub fn create_session_without_worktree(
    files: &HostFileAccess,
    project_path: &str,
    branch_name: &str,
    session_type: SessionType,
    ssh_host: Option<&str>,
    working_directory: Option<&str>,
    pane_layout: &PaneLayoutLookup<'_>,
) -> String {
    let session_name = build_session_name(project_path, branch_name, session_type);
    if session_type != SessionType::Zellij {
        return session_name;
    }
    if let Some(directory) = working_directory.filter(|directory| !directory.is_empty()) {
        prepare_zellij_layout_file_or_log(files, directory, pane_layout, ssh_host);
    }
    sanitize_zellij_session_name(&session_name)
}

fn run_session_cleanup(
    runner: &dyn ShellCommandRunner,
    herdr: &dyn HerdrTaskWorkspaces,
    session_type: SessionType,
    session_name: &str,
    ssh_host: Option<&str>,
    should_verify_cleanup: bool,
) -> Result<(), String> {
    let command = match session_type {
        SessionType::Tmux => {
            build_tmux_session_cleanup_command(session_name, should_verify_cleanup)
        }
        SessionType::Zellij => {
            build_zellij_session_cleanup_command(session_name, should_verify_cleanup)
        }
        SessionType::Herdr => return herdr.close_task_workspace(session_name, ssh_host),
        // terminal 세션은 KanVibe 밖에 없으므로 정리할 외부 세션이 없다. PTY는 터미널 계층이 이미 끊는다
        SessionType::Terminal => return Ok(()),
    };
    runner
        .exec_git(&command, ssh_host, &ExecGitOptions::default())
        .map(|_| ())
        .map_err(|error| error.message)
}

/// 브랜치별 독립 세션을 종료한다. worktree와 브랜치는 지우지 않는다.
/// 이미 끝난 세션은 무시하고, `should_throw_on_error`일 때만 정리 실패를 알린다.
pub fn remove_session_only(
    runner: &dyn ShellCommandRunner,
    herdr: &dyn HerdrTaskWorkspaces,
    session_type: SessionType,
    session_name: &str,
    ssh_host: Option<&str>,
    options: &ResourceCleanupOptions,
) -> Result<(), WorktreeCleanupError> {
    let result = run_session_cleanup(
        runner,
        herdr,
        session_type,
        session_name,
        ssh_host,
        options.should_throw_on_error,
    );
    match result {
        Err(_) if options.should_throw_on_error => {
            Err(WorktreeCleanupError::Session(session_name.to_string()))
        }
        _ => Ok(()),
    }
}

/// 세션이 살아 있는지 확인한다. terminal 세션은 KanVibe 프로세스 안에만 있어 외부 명령으로 확인할 수 없다
pub fn is_session_alive(
    runner: &dyn ShellCommandRunner,
    herdr: &dyn HerdrTaskWorkspaces,
    session_type: SessionType,
    session_name: &str,
    ssh_host: Option<&str>,
) -> bool {
    let options = ExecGitOptions::default();
    match session_type {
        SessionType::Tmux => runner
            .exec_git(
                &build_tmux_alive_check_command(session_name),
                ssh_host,
                &options,
            )
            .is_ok(),
        SessionType::Herdr => herdr.is_task_alive(session_name, ssh_host),
        SessionType::Zellij => runner
            .exec_git("zellij list-sessions", ssh_host, &options)
            .map(|output| {
                parse_alive_zellij_session_names(&output)
                    .iter()
                    .any(|name| name == session_name)
            })
            .unwrap_or(false),
        SessionType::Terminal => false,
    }
}
