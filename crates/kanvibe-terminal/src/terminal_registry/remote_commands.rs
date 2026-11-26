//! 원격 세션에 붙거나 세션을 미리 만드는 셸 명령 문자열(TS `buildRemote*Command`). 바이트 단위로 TS와 같아야 한다.

use kanvibe_ai::node_path;
use kanvibe_domain::task::SessionType;
use kanvibe_process::worktree::{
    PaneLayoutSelection, TmuxSessionBootstrapOptions, ZELLIJ_LAYOUT_FILENAME,
    build_tmux_hide_status_bar_command, build_tmux_session_bootstrap_command,
    build_zellij_alive_session_check_command, quote_for_posix_shell,
};

use crate::herdr::build_herdr_command;
use crate::tmux_clipboard::build_tmux_user_clipboard_directive_test_command;

const LAUNCH_LOGIN_SHELL: &str = r#"exec "${SHELL:-/bin/sh}" -l"#;

pub(super) fn build_remote_shell_command(command: &str) -> String {
    format!("sh -lc {}", quote_for_posix_shell(command))
}

/// 세션 타입별로 원격에서 실행할 attach 명령. Herdr 세션 이름이 잘못됐으면 오류다
pub(super) fn build_remote_attach_command(
    session_type: SessionType,
    session_name: &str,
    worktree_path: Option<&str>,
    pane_layout: Option<&PaneLayoutSelection>,
) -> Result<String, String> {
    let worktree_path = worktree_path.filter(|path| !path.is_empty());
    Ok(match session_type {
        SessionType::Herdr => format!("exec {}", build_herdr_command(session_name, &[])?),
        SessionType::Tmux => {
            build_remote_tmux_attach_command(session_name, worktree_path, pane_layout)
        }
        SessionType::Zellij => build_remote_zellij_attach_command(session_name, worktree_path),
        SessionType::Terminal => build_remote_plain_shell_command(worktree_path),
    })
}

/// 멀티플렉서 없이 원격 로그인 셸만 띄운다. host transport에서는 daemon이 이 셸의 PTY를 소유하고 재attach할 수 있다
fn build_remote_plain_shell_command(worktree_path: Option<&str>) -> String {
    let Some(worktree_path) = worktree_path else {
        return LAUNCH_LOGIN_SHELL.to_string();
    };
    // 작업 디렉터리로 못 들어가도 셸은 열어 주되, 왜 다른 위치인지 보이도록 남긴다
    [
        format!("cd {} ||", quote_for_posix_shell(worktree_path)),
        format!(
            "printf '%s\\n' {} >&2;",
            quote_for_posix_shell(&format!(
                "KanVibe: cannot enter {worktree_path}; starting the shell in the default directory."
            ))
        ),
        LAUNCH_LOGIN_SHELL.to_string(),
    ]
    .join(" ")
}

/// 원격 tmux 세션에 붙는다. 세션이 있으면 상태바를 끄고 attach하고, 없으면 생성부터 attach까지를 tmux 호출 한 번으로 처리한다.
/// OSC 52 판정은 세션 생성 분기 안에서 원격 셸이 직접 한다. 미리 SSH로 물으면 재attach에도 왕복 지연이 붙기 때문이다
fn build_remote_tmux_attach_command(
    session_name: &str,
    worktree_path: Option<&str>,
    pane_layout: Option<&PaneLayoutSelection>,
) -> String {
    let quoted_session_name = quote_for_posix_shell(session_name);
    let bootstrap_then_attach = build_remote_tmux_bootstrap_command(
        session_name,
        worktree_path,
        pane_layout,
        true,
        &build_remote_interactive_shell_fallback_command(),
    );
    [
        format!("if tmux has-session -t {quoted_session_name} 2>/dev/null; then"),
        format!(
            "{} 2>/dev/null;",
            build_tmux_hide_status_bar_command(session_name)
        ),
        format!("exec tmux attach-session -t {quoted_session_name};"),
        format!("else {bootstrap_then_attach}"),
        "fi".to_string(),
    ]
    .join(" ")
}

/// 원격 tmux 세션을 만드는 셸 조각. 사용자가 tmux 설정에서 set-clipboard를 정해 뒀으면 그 값을 건드리지 않는다.
/// 사용자 설정 때문에 기동이 실패하면 설정 없이 한 번 더 만들고, 그래도 실패하면 `failure_command`를 실행한다
pub(super) fn build_remote_tmux_bootstrap_command(
    session_name: &str,
    worktree_path: Option<&str>,
    pane_layout: Option<&PaneLayoutSelection>,
    should_attach_after_bootstrap: bool,
    failure_command: &str,
) -> String {
    let build_bootstrap_with_fallback = |should_enable_clipboard: bool| {
        let build_bootstrap_command = |should_skip_user_config_file: bool| {
            build_tmux_session_bootstrap_command(
                session_name,
                worktree_path.unwrap_or_default(),
                pane_layout,
                TmuxSessionBootstrapOptions {
                    should_attach_after_bootstrap,
                    should_skip_user_config_file,
                    should_enable_clipboard,
                },
            )
        };
        let bootstrap_with_retry = [
            build_bootstrap_command(false),
            format!(
                "{{ tmux kill-session -t {} 2>/dev/null; {}; }}",
                quote_for_posix_shell(session_name),
                build_bootstrap_command(true)
            ),
        ]
        .join(" || ");
        format!("{bootstrap_with_retry} || {{ {failure_command}; }};")
    };
    [
        format!(
            "__kanvibe_clipboard_probe=0; {} || __kanvibe_clipboard_probe=$?;",
            build_tmux_user_clipboard_directive_test_command()
        ),
        format!(
            "if [ \"$__kanvibe_clipboard_probe\" -eq 1 ]; then {}",
            build_bootstrap_with_fallback(true)
        ),
        format!("else {} fi;", build_bootstrap_with_fallback(false)),
    ]
    .join(" ")
}

/// 원격 zellij 세션에 붙는다. 세션이 없으면 SSH가 그대로 끊기므로 생성과 레이아웃 적용까지 처리한다.
/// 존재 확인과 attach를 나눠, 대화형 세션의 stderr를 숨기지 않고 비정상 종료 코드를 "세션 없음"으로 오해하지 않는다
fn build_remote_zellij_attach_command(session_name: &str, worktree_path: Option<&str>) -> String {
    let quoted_session_name = quote_for_posix_shell(session_name);
    let create_arguments = format!("--session {quoted_session_name}");
    let attach_when_alive = [
        format!(
            "if {}; then",
            build_zellij_alive_session_check_command(session_name)
        ),
        format!("exec zellij attach {quoted_session_name};"),
        "fi;".to_string(),
    ]
    .join(" ");
    let Some(worktree_path) = worktree_path else {
        return format!("{attach_when_alive} exec zellij {create_arguments}");
    };
    let layout_file =
        quote_for_posix_shell(&node_path::join(&[worktree_path, ZELLIJ_LAYOUT_FILENAME]));
    // 레이아웃 파일 경로가 절대경로라 cd 실패는 조용히 통과한다. 작업 디렉터리가 어긋난 이유가 보이도록 남긴다
    let change_directory = [
        format!("cd {} ||", quote_for_posix_shell(worktree_path)),
        format!(
            "printf '%s\\n' {} >&2;",
            quote_for_posix_shell(&format!(
                "KanVibe: cannot enter {worktree_path}; starting the zellij session in the default directory."
            ))
        ),
    ]
    .join(" ");
    [
        attach_when_alive,
        change_directory,
        format!("if [ -f {layout_file} ]; then"),
        format!("exec zellij {create_arguments} --new-session-with-layout {layout_file};"),
        "else".to_string(),
        format!("exec zellij {create_arguments};"),
        "fi".to_string(),
    ]
    .join(" ")
}

/// 화면 attach 없이 원격 zellij 세션을 background로 만든다. cd 실패는 기본 디렉터리에서 계속하되, 생성 실패는 명령 실패로 남긴다
fn build_remote_zellij_prepare_command(session_name: &str, worktree_path: Option<&str>) -> String {
    let quoted_session_name = quote_for_posix_shell(session_name);
    let create_in_background = format!("zellij attach --create-background {quoted_session_name}");
    let create_steps = match worktree_path {
        Some(worktree_path) => {
            let layout_file =
                quote_for_posix_shell(&node_path::join(&[worktree_path, ZELLIJ_LAYOUT_FILENAME]));
            [
                format!("cd {} 2>/dev/null || true;", quote_for_posix_shell(worktree_path)),
                format!("if [ -f {layout_file} ]; then"),
                format!("zellij --layout {layout_file} attach --create-background {quoted_session_name};"),
                format!("else {create_in_background}; fi"),
            ]
            .join(" ")
        }
        None => create_in_background,
    };
    format!(
        "if {}; then :; else {create_steps}; fi",
        build_zellij_alive_session_check_command(session_name)
    )
}

/// 화면 attach 없이 원격 멀티플렉서 세션만 만드는 명령. 이미 있으면 아무것도 하지 않는다
pub(super) fn build_remote_session_prepare_command(
    session_type: SessionType,
    session_name: &str,
    worktree_path: Option<&str>,
    pane_layout: Option<&PaneLayoutSelection>,
) -> String {
    let worktree_path = worktree_path.filter(|path| !path.is_empty());
    if session_type != SessionType::Tmux {
        return build_remote_zellij_prepare_command(session_name, worktree_path);
    }
    format!(
        "if tmux has-session -t {} 2>/dev/null; then :; else {} fi",
        quote_for_posix_shell(session_name),
        build_remote_tmux_bootstrap_command(
            session_name,
            worktree_path,
            pane_layout,
            false,
            "exit 1"
        )
    )
}

fn build_remote_interactive_shell_fallback_command() -> String {
    [
        format!(
            "printf '%s\\n' {} >&2",
            quote_for_posix_shell(
                "KanVibe: tmux session setup failed; leaving the SSH shell open."
            )
        ),
        LAUNCH_LOGIN_SHELL.to_string(),
    ]
    .join("; ")
}
