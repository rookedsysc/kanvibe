//! tmux `set-clipboard` 사용자 설정 감지. CLAUDE.md의 승인된 예외 항목이 이 판정에 기댄다.
//!
//! `set-clipboard`는 tmux 서버 옵션이라 KanVibe 세션으로 좁힐 수 없다. 그래서 KanVibe가 자기 세션을 부트스트랩할 때만,
//! 사용자 tmux 설정이 값을 정해 두지 않았을 때만 켠다. 감지는 tmux 자신의 설정 검색 순서를 따르고,
//! 확인하지 못하면(모름) 켜지 않는다. `source-file`로 끌어온 파일은 따라가지 않는다.

use std::time::Duration;

use kanvibe_process::exec::{ExecGitOptions, ShellCommandRunner};
use kanvibe_process::worktree::quote_for_posix_shell;

pub const TMUX_USER_CLIPBOARD_PREFERENCE_MARKER: &str = "kanvibe-user-set-clipboard";

/// 설정 파일 grep 한 번이라 부트스트랩보다 훨씬 짧게 잡는다
const TMUX_CLIPBOARD_PREFERENCE_TIMEOUT: Duration = Duration::from_millis(5_000);

/// tmux가 실제로 읽는 설정 파일을 tmux의 검색 순서대로 둔다. `$prefix/etc/tmux.conf`가 먼저이고
/// 컴파일 시점 prefix는 알 수 없어 실제로 쓰이는 설치 경로를 열거한다. `XDG_CONFIG_HOME`은 tmux와 같은 기본값 규칙으로 전개한다
const TMUX_CONFIG_FILE_CANDIDATES: [&str; 5] = [
    r#""/etc/tmux.conf""#,
    r#""/usr/local/etc/tmux.conf""#,
    r#""/opt/homebrew/etc/tmux.conf""#,
    r#""$HOME/.tmux.conf""#,
    r#""${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf""#,
];

/// 사용자 tmux 설정이 `set-clipboard`를 다루면 0, 다루지 않으면 1, 확인하지 못하면 2로 끝나는 조건 명령.
/// 주석 처리된 줄은 세지 않는다
pub fn build_tmux_user_clipboard_directive_test_command() -> String {
    format!(
        "(for __kanvibe_tmux_config in {}; do if [ -e \"$__kanvibe_tmux_config\" ]; then grep -sE '^[[:space:]]*set(-option)?[[:space:]].*set-clipboard' \"$__kanvibe_tmux_config\" >/dev/null; __kanvibe_probe_status=$?; case \"$__kanvibe_probe_status\" in 0) exit 0 ;; 1) ;; *) exit 2 ;; esac; fi; done; exit 1)",
        TMUX_CONFIG_FILE_CANDIDATES.join(" ")
    )
}

/// 위 조건을 별도 프로세스에서 실행할 때 쓰는 명령. 사용자 값이 있으면 표시 문자열을 찍고, 모르면 그 종료 코드로 실패한다
pub fn build_tmux_user_clipboard_preference_command() -> String {
    format!(
        "if {}; then printf '%s' {}; else __kanvibe_probe_status=$?; if [ \"$__kanvibe_probe_status\" -ne 1 ]; then exit \"$__kanvibe_probe_status\"; fi; fi",
        build_tmux_user_clipboard_directive_test_command(),
        quote_for_posix_shell(TMUX_USER_CLIPBOARD_PREFERENCE_MARKER)
    )
}

/// 위 명령의 출력으로 사용자가 `set-clipboard`를 직접 정해 뒀는지 판정한다
pub fn has_user_tmux_clipboard_preference(preference_command_output: &str) -> bool {
    preference_command_output.contains(TMUX_USER_CLIPBOARD_PREFERENCE_MARKER)
}

/// 로컬 tmux 세션을 부트스트랩할 때 OSC 52 전달을 켜도 되는지 판정한다. 확인에 실패하면 사용자 설정을 덮지 않는 쪽으로 기운다
pub fn should_enable_local_tmux_clipboard(runner: &dyn ShellCommandRunner) -> bool {
    let options = ExecGitOptions {
        timeout: Some(TMUX_CLIPBOARD_PREFERENCE_TIMEOUT),
        ..ExecGitOptions::default()
    };
    match runner.exec_git(
        &build_tmux_user_clipboard_preference_command(),
        None,
        &options,
    ) {
        Ok(output) => !has_user_tmux_clipboard_preference(&output),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests;
