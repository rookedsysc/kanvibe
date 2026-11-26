use std::process::Command;

use kanvibe_process::exec::ExecGitError;

use super::*;
use crate::test_support::ScriptedShellRunner;

#[test]
fn directive_test_searches_every_tmux_config_path_in_tmux_lookup_order() {
    let command = build_tmux_user_clipboard_directive_test_command();

    // XDG_CONFIG_HOME이 기본값이 아니면 하드코딩된 ~/.config는 tmux가 읽지도 않는 파일이다
    assert!(command.contains(r#""${XDG_CONFIG_HOME:-$HOME/.config}/tmux/tmux.conf""#));
    assert!(!command.contains(r#""$HOME/.config/tmux/tmux.conf""#));
    assert!(command.contains(r#""$HOME/.tmux.conf""#));
    // tmux는 사용자 설정보다 먼저 $prefix/etc/tmux.conf를 읽는다
    let system_config = command.find(r#""/etc/tmux.conf""#).expect("시스템 설정");
    let user_config = command.find(r#""$HOME/.tmux.conf""#).expect("사용자 설정");
    assert!(system_config < user_config);
    assert!(command.contains(r#""/opt/homebrew/etc/tmux.conf""#));
    assert!(command.contains("grep -sE"));
}

#[test]
fn preference_command_reuses_the_same_path_list_and_prints_the_marker() {
    let preference_command = build_tmux_user_clipboard_preference_command();

    assert!(preference_command.contains(&build_tmux_user_clipboard_directive_test_command()));
    assert!(preference_command.contains(TMUX_USER_CLIPBOARD_PREFERENCE_MARKER));
}

/// grep을 셸 함수로 바꿔 종료 코드를 고정하고 실제 sh로 실행한다. 사용자 홈 대신 임시 디렉터리를 쓴다
fn run_with_stubbed_grep(status: i32, command: &str, home: &std::path::Path) -> (i32, String) {
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("grep() {{ return {status}; }}; {command}"))
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("sh 실행");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn grep_errors_stay_unknown_and_only_missing_settings_count_as_absent() {
    let directory = tempfile::tempdir().expect("임시 디렉터리");
    std::fs::write(directory.path().join(".tmux.conf"), "# fixture\n").expect("설정 파일");

    for status in [0, 1, 2] {
        let (directive_status, _) = run_with_stubbed_grep(
            status,
            &build_tmux_user_clipboard_directive_test_command(),
            directory.path(),
        );
        let (preference_status, stdout) = run_with_stubbed_grep(
            status,
            &build_tmux_user_clipboard_preference_command(),
            directory.path(),
        );

        assert_eq!(directive_status, status);
        assert_eq!(preference_status, if status == 2 { 2 } else { 0 });
        let expected_stdout = if status == 0 {
            TMUX_USER_CLIPBOARD_PREFERENCE_MARKER
        } else {
            ""
        };
        assert_eq!(stdout, expected_stdout);
    }
}

#[test]
fn local_clipboard_is_enabled_only_when_the_user_config_is_silent() {
    let silent = ScriptedShellRunner::new();
    let chosen = ScriptedShellRunner::new();
    chosen.respond_when_contains("set-clipboard", TMUX_USER_CLIPBOARD_PREFERENCE_MARKER);
    let unknown = ScriptedShellRunner::new();
    unknown.respond(|_, _| Some(Err(ExecGitError::new("grep failed"))));

    assert!(should_enable_local_tmux_clipboard(silent.as_ref()));
    assert!(!should_enable_local_tmux_clipboard(chosen.as_ref()));
    assert!(!should_enable_local_tmux_clipboard(unknown.as_ref()));
    assert_eq!(
        silent.calls()[0].options.timeout,
        Some(std::time::Duration::from_millis(5_000))
    );
}
