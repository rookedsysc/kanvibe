//! 로컬 tmux·zellij 세션 확인과 부트스트랩(TS `execSync`로 돌던 명령). 모든 명령은 `exec_git`을 거친다.

use std::path::Path;
use std::time::Duration;

use kanvibe_ai::node_path;
use kanvibe_domain::task::SessionType;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::worktree::{
    PaneLayoutSelection, TmuxSessionBootstrapOptions, ZELLIJ_LAYOUT_FILENAME,
    build_tmux_hide_status_bar_command, build_tmux_session_bootstrap_command,
    parse_alive_zellij_session_names, quote_for_posix_shell,
};

use crate::environment::TerminalHostEnvironment;
use crate::herdr::build_herdr_command;
use crate::tmux_clipboard::should_enable_local_tmux_clipboard;

/// tmux 서버 기동은 사용자 설정에 달려 있어 오래 걸릴 수 있다. 플러그인 관리자를 run-shell로 부르는 설정은 기동에만 수 초가 걸린다
pub(super) const TMUX_BOOTSTRAP_TIMEOUT: Duration = Duration::from_millis(30_000);
/// 상태바 정리는 설정 확인처럼 짧게 잡는다
const TMUX_STATUS_BAR_TIMEOUT: Duration = Duration::from_millis(5_000);
const SESSION_ALIVE_CHECK_TIMEOUT: Duration = Duration::from_millis(3_000);

fn with_timeout(timeout: Duration) -> ExecGitOptions {
    ExecGitOptions {
        timeout: Some(timeout),
        ..ExecGitOptions::default()
    }
}

pub(super) fn is_tmux_session_alive(runner: &dyn ShellCommandRunner, session_name: &str) -> bool {
    let command = format!(
        "tmux has-session -t {} 2>/dev/null",
        quote_for_posix_shell(session_name)
    );
    runner
        .exec_git(&command, None, &with_timeout(SESSION_ALIVE_CHECK_TIMEOUT))
        .is_ok()
}

pub(super) fn is_zellij_session_alive(runner: &dyn ShellCommandRunner, session_name: &str) -> bool {
    match runner.exec_git(
        "zellij list-sessions",
        None,
        &with_timeout(SESSION_ALIVE_CHECK_TIMEOUT),
    ) {
        Ok(output) => parse_alive_zellij_session_names(&output)
            .iter()
            .any(|alive_session_name| alive_session_name == session_name),
        Err(_) => false,
    }
}

/// 이미 살아 있는 세션에 붙기 전에 tmux 상태바를 끈다. 표시가 한 줄 겹칠 뿐이라 실패해도 attach를 막지 않는다
pub(super) fn hide_existing_local_tmux_status_bar(
    runner: &dyn ShellCommandRunner,
    session_name: &str,
) {
    let command = build_tmux_hide_status_bar_command(session_name);
    if let Err(error) = runner.exec_git(&command, None, &with_timeout(TMUX_STATUS_BAR_TIMEOUT)) {
        eprintln!("[터미널] 기존 tmux 세션 상태바 정리 실패: {error}");
    }
}

/// 로컬 tmux 세션을 만든다. 사용자 설정이 세션 기동을 막으면 설정 파일 없이 한 번 더 시도해 소켓을 바꾸지 않고 세션을 살린다.
/// OSC 52 전달은 사용자 tmux 설정이 `set-clipboard`를 정해 두지 않았을 때만 켠다(CLAUDE.md 승인 예외)
pub(super) fn bootstrap_local_tmux_session(
    runner: &dyn ShellCommandRunner,
    session_name: &str,
    working_directory: &str,
    pane_layout: Option<&PaneLayoutSelection>,
) -> Result<(), ExecGitError> {
    let should_enable_clipboard = should_enable_local_tmux_clipboard(runner);
    let run_bootstrap = |should_skip_user_config_file: bool| {
        let command = build_tmux_session_bootstrap_command(
            session_name,
            working_directory,
            pane_layout,
            TmuxSessionBootstrapOptions {
                should_attach_after_bootstrap: false,
                should_skip_user_config_file,
                should_enable_clipboard,
            },
        );
        runner.exec_git(&command, None, &with_timeout(TMUX_BOOTSTRAP_TIMEOUT))
    };
    let Err(error) = run_bootstrap(false) else {
        return Ok(());
    };
    eprintln!("[터미널] 사용자 tmux 설정으로 세션 생성에 실패해 설정 없이 재시도합니다: {error}");
    let kill_command = format!(
        "tmux kill-session -t {} 2>/dev/null || true",
        quote_for_posix_shell(session_name)
    );
    runner.exec_git(&kill_command, None, &with_timeout(TMUX_BOOTSTRAP_TIMEOUT))?;
    run_bootstrap(true).map(|_| ())
}

/// 화면 attach 없이 로컬 zellij 세션을 background로 만든다. worktree에 레이아웃 파일이 있으면 적용한다.
/// TS는 `execSync`의 cwd로 작업 디렉터리를 정했다. `exec_git`에는 cwd가 없어 같은 디렉터리로 먼저 이동한다
pub(super) fn create_local_zellij_session_in_background(
    runner: &dyn ShellCommandRunner,
    session_name: &str,
    worktree_path: Option<&str>,
    working_directory: &str,
) -> Result<(), ExecGitError> {
    let layout_argument = worktree_path
        .map(|worktree_path| node_path::join(&[worktree_path, ZELLIJ_LAYOUT_FILENAME]))
        .filter(|layout_file| Path::new(layout_file).exists())
        .map(|layout_file| format!("--layout {} ", quote_for_posix_shell(&layout_file)))
        .unwrap_or_default();
    let command = format!(
        "cd {} && zellij {layout_argument}attach --create-background {}",
        quote_for_posix_shell(working_directory),
        quote_for_posix_shell(session_name)
    );
    runner
        .exec_git(&command, None, &with_timeout(TMUX_BOOTSTRAP_TIMEOUT))
        .map(|_| ())
}

/// 로컬 PTY로 띄울 프로세스
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LocalPtySpawnPlan {
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: String,
}

/// 세션 타입별로 띄울 프로세스. tmux는 만들어 둔 세션에 attach하고, zellij는 세션이 없으면 생성까지 겸하며,
/// terminal은 멀티플렉서 없이 로그인 셸을 바로 띄운다
pub(super) fn build_local_pty_spawn_plan(
    environment: &TerminalHostEnvironment,
    session_type: SessionType,
    session_name: &str,
    working_directory: Option<&str>,
    should_create_zellij_session: bool,
) -> Result<LocalPtySpawnPlan, String> {
    let home_directory = environment.home_directory.clone();
    let directory_or_home = working_directory
        .map(str::to_string)
        .unwrap_or(home_directory.clone());
    let plan = |program: &str, args: Vec<String>, working_directory: String| LocalPtySpawnPlan {
        program: program.to_string(),
        args,
        working_directory,
    };
    Ok(match session_type {
        SessionType::Terminal => plan(
            &environment.resolve_login_shell(),
            vec!["-l".into()],
            directory_or_home,
        ),
        SessionType::Herdr => {
            let herdr_command = format!("exec {}", build_herdr_command(session_name, &[])?);
            plan(
                "/bin/sh",
                vec!["-c".into(), herdr_command],
                directory_or_home,
            )
        }
        SessionType::Tmux => plan(
            "tmux",
            vec!["attach-session".into(), "-t".into(), session_name.into()],
            home_directory,
        ),
        SessionType::Zellij if !should_create_zellij_session => plan(
            "zellij",
            vec!["attach".into(), session_name.into()],
            home_directory,
        ),
        SessionType::Zellij => plan(
            "zellij",
            build_zellij_creation_args(session_name, working_directory),
            directory_or_home,
        ),
    })
}

/// 세션이 없으면 `--session`으로 생성과 attach를 함께 한다. worktree에 KDL 레이아웃 파일이 있으면 새 세션에 적용한다
fn build_zellij_creation_args(session_name: &str, working_directory: Option<&str>) -> Vec<String> {
    let mut args = vec!["--session".to_string(), session_name.to_string()];
    if let Some(working_directory) = working_directory {
        let layout_file = node_path::join(&[working_directory, ZELLIJ_LAYOUT_FILENAME]);
        if Path::new(&layout_file).exists() {
            args.push("--new-session-with-layout".to_string());
            args.push(layout_file);
        }
    }
    args
}
