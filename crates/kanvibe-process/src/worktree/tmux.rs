//! tmux 세션 부트스트랩·pane 분할·조회·정리 명령 문자열.

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};

use super::naming::quote_for_posix_shell;

/// KanVibe 1.0.6까지 원격 세션이 쓰던 격리 소켓.
/// 새 세션은 기본 소켓에만 만들고, 조회와 정리에서만 이 소켓에 남은 세션을 흡수한다.
const LEGACY_TMUX_SOCKET_NAME: &str = "kanvibe";

/// 세션을 만들 때 적용할 pane 레이아웃(TS `TmuxPaneLayoutConfig`). zellij 레이아웃 파일에도 같은 값을 쓴다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLayoutSelection {
    pub layout_type: PaneLayoutType,
    pub panes: Vec<PaneCommand>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TmuxSessionBootstrapOptions {
    /// 같은 tmux 호출에서 attach까지 이어간다. 원격 SSH가 단일 명령으로 세션을 열 때 쓴다
    pub should_attach_after_bootstrap: bool,
    /// 사용자 tmux 설정이 세션 기동을 막을 때 `-f /dev/null`로 다시 시도한다.
    /// tmux는 설정 파일을 서버 기동 시 한 번만 읽으므로 서버가 아직 없을 때만 실제로 설정을 건너뛴다.
    pub should_skip_user_config_file: bool,
    /// OSC 52 전달을 켠다. 서버 스코프 옵션이라 사용자의 다른 세션까지 바뀌므로,
    /// 사용자가 자기 tmux 설정에서 `set-clipboard`를 정해 두지 않았을 때만 켜야 한다(판정은 터미널 계층이 맡는다).
    pub should_enable_clipboard: bool,
}

/// 세션을 찾아야 하는 tmux 소켓 목록. 기본 소켓이 먼저다
pub(crate) fn build_tmux_lookup_commands() -> [String; 2] {
    [
        "tmux".to_string(),
        format!("tmux -L {}", quote_for_posix_shell(LEGACY_TMUX_SOCKET_NAME)),
    ]
}

/// 작업 디렉터리를 모르는 원격 세션은 -c 없이 만들어 tmux 기본 디렉터리를 쓰게 한다
fn build_tmux_create_session_argument(session_name: &str, working_directory: &str) -> String {
    let create_argument = format!("new-session -d -s {}", quote_for_posix_shell(session_name));
    if working_directory.is_empty() {
        return create_argument;
    }
    format!(
        "{create_argument} -c {}",
        quote_for_posix_shell(working_directory)
    )
}

fn build_tmux_pane_target(session_name: &str, position: i64) -> String {
    quote_for_posix_shell(&format!("{session_name}:0.{position}"))
}

fn build_tmux_split_arguments(
    session_name: &str,
    layout_type: PaneLayoutType,
    worktree_path: &str,
) -> Vec<String> {
    let window_target = quote_for_posix_shell(&format!("{session_name}:0"));
    let directory = quote_for_posix_shell(worktree_path);
    let split = |direction: &str, target: &str| {
        format!("split-window {direction} -t {target} -c {directory}")
    };
    let pane = |position: i64| build_tmux_pane_target(session_name, position);
    match layout_type {
        PaneLayoutType::Single => vec![],
        PaneLayoutType::Horizontal2 => vec![split("-v", &window_target)],
        PaneLayoutType::Vertical2 => vec![split("-h", &window_target)],
        PaneLayoutType::LeftRightTb => vec![split("-h", &window_target), split("-v", &pane(1))],
        PaneLayoutType::LeftTbRight => vec![split("-h", &window_target), split("-v", &pane(0))],
        PaneLayoutType::Quad => vec![
            split("-h", &window_target),
            split("-v", &pane(0)),
            split("-v", &pane(2)),
        ],
    }
}

pub fn build_tmux_pane_layout_arguments(
    session_name: &str,
    layout_type: PaneLayoutType,
    panes: &[PaneCommand],
    worktree_path: &str,
) -> Vec<String> {
    let send_keys_arguments = panes
        .iter()
        .filter(|pane| !pane.command.trim().is_empty())
        .map(|pane| {
            format!(
                "send-keys -t {} -- {} Enter",
                build_tmux_pane_target(session_name, pane.position),
                quote_for_posix_shell(&pane.command)
            )
        });
    build_tmux_split_arguments(session_name, layout_type, worktree_path)
        .into_iter()
        .chain(send_keys_arguments)
        .collect()
}

/// KanVibe가 만드는 tmux 세션의 안정화·클립보드 옵션.
/// destroy-unattached를 끄지 않으면 사용자 설정에 따라 detached 부트스트랩 세션이 생성 직후 사라지고,
/// window-size latest는 웹 터미널 크기가 다른 클라이언트에 묶이지 않게 한다. 상태바는 KanVibe 탭 바와 겹치므로 세션 스코프로 끈다.
/// set-clipboard는 세션 스코프로 좁힐 수 없는 서버 옵션이라 CLAUDE.md 예외 조건을 만족할 때만 켠다.
fn build_tmux_session_hardening_arguments(
    session_name: &str,
    should_enable_clipboard: bool,
) -> Vec<String> {
    let target = quote_for_posix_shell(session_name);
    let mut arguments = vec![
        format!("set-option -t {target} destroy-unattached off"),
        format!("set-option -t {target} window-size latest"),
        format!("set-option -t {target} status off"),
    ];
    if should_enable_clipboard {
        arguments.push("set-option -s set-clipboard on".into());
    }
    arguments
}

/// 세션 생성과 안정화, 레이아웃, 선택적 attach를 tmux 호출 한 번으로 실행하는 명령.
/// 한 명령 시퀀스로 묶어야 사용자 설정이 세션을 없애기 전에 안정화 옵션이 적용된다.
pub fn build_tmux_session_bootstrap_command(
    session_name: &str,
    working_directory: &str,
    pane_layout: Option<&PaneLayoutSelection>,
    options: TmuxSessionBootstrapOptions,
) -> String {
    let mut tmux_arguments = vec![build_tmux_create_session_argument(
        session_name,
        working_directory,
    )];
    tmux_arguments.extend(build_tmux_session_hardening_arguments(
        session_name,
        options.should_enable_clipboard,
    ));
    // pane 분할은 각 pane의 작업 디렉터리를 지정해야 하므로 작업 디렉터리를 아는 경우에만 적용한다
    if let Some(layout) = pane_layout
        && !working_directory.is_empty()
        && layout.layout_type != PaneLayoutType::Single
    {
        tmux_arguments.extend(build_tmux_pane_layout_arguments(
            session_name,
            layout.layout_type,
            &layout.panes,
            working_directory,
        ));
    }
    if options.should_attach_after_bootstrap {
        tmux_arguments.push(format!(
            "attach-session -t {}",
            quote_for_posix_shell(session_name)
        ));
    }
    let tmux_command = match options.should_skip_user_config_file {
        true => "tmux -f /dev/null",
        false => "tmux",
    };
    format!("{tmux_command} {}", tmux_arguments.join(" \\; "))
}

/// 이미 살아 있는 tmux 세션의 상태바를 끈다. `status`는 세션 스코프이고 멱등이라 다시 붙을 때마다 걸어도 된다
pub fn build_tmux_hide_status_bar_command(session_name: &str) -> String {
    format!(
        "tmux set-option -t {} status off",
        quote_for_posix_shell(session_name)
    )
}

pub(crate) fn build_tmux_session_cleanup_command(
    session_name: &str,
    should_verify_cleanup: bool,
) -> String {
    let target = quote_for_posix_shell(session_name);
    let tmux_commands = build_tmux_lookup_commands();
    let kill_commands = tmux_commands
        .iter()
        .map(|tmux_command| format!("{tmux_command} kill-session -t {target} 2>/dev/null || true"));
    if !should_verify_cleanup {
        return kill_commands.collect::<Vec<_>>().join("; ");
    }
    let verify_commands = tmux_commands.iter().map(|tmux_command| {
        format!("if {tmux_command} has-session -t {target} 2>/dev/null; then exit 1; fi")
    });
    std::iter::once("command -v tmux >/dev/null 2>&1 || exit 1".to_string())
        .chain(kill_commands)
        .chain(verify_commands)
        .collect::<Vec<_>>()
        .join("; ")
}

pub(crate) fn build_tmux_alive_check_command(session_name: &str) -> String {
    let target = quote_for_posix_shell(session_name);
    build_tmux_lookup_commands()
        .iter()
        .map(|tmux_command| format!("{tmux_command} has-session -t {target} 2>/dev/null && exit 0"))
        .chain(std::iter::once("exit 1".to_string()))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_runs_creation_hardening_layout_and_attach_in_one_tmux_call() {
        let layout = PaneLayoutSelection {
            layout_type: PaneLayoutType::Vertical2,
            panes: vec![PaneCommand {
                position: 1,
                command: "pnpm dev".into(),
            }],
        };
        let options = TmuxSessionBootstrapOptions {
            should_attach_after_bootstrap: true,
            ..TmuxSessionBootstrapOptions::default()
        };

        let command = build_tmux_session_bootstrap_command("s", "/w", Some(&layout), options);

        assert_eq!(
            command,
            "tmux new-session -d -s 's' -c '/w' \\; set-option -t 's' destroy-unattached off \\; set-option -t 's' window-size latest \\; set-option -t 's' status off \\; split-window -h -t 's:0' -c '/w' \\; send-keys -t 's:0.1' -- 'pnpm dev' Enter \\; attach-session -t 's'"
        );
    }

    #[test]
    fn bootstrap_without_working_directory_skips_layout_and_can_bypass_user_config() {
        let options = TmuxSessionBootstrapOptions {
            should_skip_user_config_file: true,
            should_enable_clipboard: true,
            ..TmuxSessionBootstrapOptions::default()
        };

        let command = build_tmux_session_bootstrap_command("s", "", None, options);

        assert_eq!(
            command,
            "tmux -f /dev/null new-session -d -s 's' \\; set-option -t 's' destroy-unattached off \\; set-option -t 's' window-size latest \\; set-option -t 's' status off \\; set-option -s set-clipboard on"
        );
    }
}
