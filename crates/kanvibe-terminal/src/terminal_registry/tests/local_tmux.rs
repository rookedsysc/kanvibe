//! TS `terminal.test.ts` "attachLocalSession — tmux 세션 자동 생성", focusSession, plain terminal 화면

use std::process::Command;

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use kanvibe_domain::task::SessionType;
use kanvibe_process::worktree::PaneLayoutSelection;

use super::fakes::*;

fn tmux_request(task_id: &str) -> crate::terminal_registry::LocalAttachRequest {
    local_request(
        task_id,
        None,
        SessionType::Tmux,
        "feat-login",
        Some("/workspace"),
    )
}

fn harness_without_tmux_session() -> Harness {
    let harness = Harness::new();
    harness
        .runner
        .fail_when_contains("has-session", "session not found");
    harness
}

fn pane(position: i64, command: &str) -> PaneCommand {
    PaneCommand {
        position,
        command: command.to_string(),
    }
}

fn vertical_layout(panes: Vec<PaneCommand>) -> Option<PaneLayoutSelection> {
    Some(PaneLayoutSelection {
        layout_type: PaneLayoutType::Vertical2,
        panes,
    })
}

#[test]
fn local_tmux_session_name_with_shell_metacharacters_is_passed_literally() {
    // Given
    let harness = Harness::new();
    let session_name = "repo-$(printf SUBSTITUTED)-`printf BACKTICK`-'quoted'";
    let request = local_request(
        "task-literal-name",
        None,
        SessionType::Tmux,
        session_name,
        Some("/workspace"),
    );

    // When
    harness.attach_local(request, &FakeClient::new()).unwrap();

    // Then: tmux를 함수로 바꾼 셸에서 세 번째 인자가 이름 그대로 나온다
    let command = harness.command_containing("has-session").unwrap();
    let script = format!("tmux() {{ printf '%s\\n' \"$3\"; }}; {command}");
    let output = Command::new("/bin/sh")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), session_name);
}

#[test]
fn missing_local_tmux_session_is_created_in_the_working_directory() {
    // Given
    let harness = harness_without_tmux_session();

    // When
    harness
        .attach_local(tmux_request("task-1"), &FakeClient::new())
        .unwrap();

    // Then
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(bootstrap_command.contains("-s 'feat-login'"));
    assert!(bootstrap_command.contains("-c '/workspace'"));
    let call = harness
        .runner
        .calls()
        .into_iter()
        .find(|call| call.command.contains("new-session"))
        .unwrap();
    assert_eq!(call.ssh_host, None);
}

#[test]
fn alive_local_tmux_session_gets_its_status_bar_hidden_before_attach() {
    // Given: 부트스트랩은 세션 생성 때만 돌아서, 탭 바 이전에 만든 세션은 상태바가 탭 바와 겹친 채 남는다
    let harness = Harness::new();

    // When
    harness
        .attach_local(tmux_request("task-existing"), &FakeClient::new())
        .unwrap();

    // Then
    assert_eq!(harness.command_containing("new-session"), None);
    assert_eq!(
        harness.command_containing("status off").as_deref(),
        Some("tmux set-option -t 'feat-login' status off")
    );
}

#[test]
fn hiding_the_status_bar_failure_does_not_block_attaching() {
    // Given
    let harness = Harness::new();
    harness
        .runner
        .fail_when_contains("status off", "set-option failed");

    // When
    let attachment =
        harness.attach_local(tmux_request("task-existing-failure"), &FakeClient::new());

    // Then
    assert!(attachment.unwrap().is_some());
    assert_eq!(harness.ptys.count(), 1);
}

#[test]
fn local_tmux_bootstrap_applies_pane_layout_in_one_tmux_call() {
    // Given
    let harness = harness_without_tmux_session();
    let mut request = tmux_request("task-layout");
    request.columns = Some(120);
    request.rows = Some(30);
    request.pane_layout = vertical_layout(vec![pane(0, "pnpm dev"), pane(1, "pnpm test")]);

    // When
    harness.attach_local(request, &FakeClient::new()).unwrap();

    // Then
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(bootstrap_command.contains("tmux new-session -d -s 'feat-login' -c '/workspace'"));
    assert!(bootstrap_command.contains("split-window -h -t 'feat-login:0' -c '/workspace'"));
    assert!(bootstrap_command.contains("send-keys -t 'feat-login:0.0' -- 'pnpm dev' Enter"));
    assert!(bootstrap_command.contains("send-keys -t 'feat-login:0.1' -- 'pnpm test' Enter"));
    // 사용자 설정이 세션을 지우기 전에 옵션이 적용되도록 한 번의 tmux 호출로 묶여야 한다
    assert_eq!(bootstrap_command.matches("tmux ").count(), 1);
}

#[test]
fn local_tmux_bootstrap_keeps_unattached_session_alive() {
    // Given
    let harness = harness_without_tmux_session();

    // When
    harness
        .attach_local(tmux_request("task-hardening"), &FakeClient::new())
        .unwrap();

    // Then
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(bootstrap_command.contains("set-option -t 'feat-login' destroy-unattached off"));
}

#[test]
fn local_tmux_status_bar_is_hidden_only_for_the_kanvibe_session() {
    // Given
    let harness = harness_without_tmux_session();

    // When
    harness
        .attach_local(tmux_request("task-1"), &FakeClient::new())
        .unwrap();

    // Then
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(bootstrap_command.contains("set-option -t 'feat-login' status off"));
    assert!(!bootstrap_command.contains("set-option -g status"));
    assert!(!bootstrap_command.contains("set-option -s status"));
}

#[test]
fn local_tmux_window_size_option_is_scoped_to_the_session() {
    // Given
    let harness = harness_without_tmux_session();

    // When
    harness
        .attach_local(tmux_request("task-window-size"), &FakeClient::new())
        .unwrap();

    // Then
    assert!(
        !harness
            .commands()
            .iter()
            .any(|command| command.contains("set-option -g window-size"))
    );
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(bootstrap_command.contains("set-option -t 'feat-login' window-size latest"));
}

#[test]
fn local_tmux_bootstrap_enables_osc52_when_user_config_is_silent() {
    // Given
    let harness = harness_without_tmux_session();

    // When
    harness
        .attach_local(tmux_request("task-clipboard-default"), &FakeClient::new())
        .unwrap();

    // Then
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(bootstrap_command.contains("set-option -s set-clipboard on"));
}

#[test]
fn local_tmux_bootstrap_leaves_set_clipboard_to_the_user_choice() {
    // Given
    let harness = harness_without_tmux_session();
    harness
        .runner
        .respond_when_contains("set-clipboard", "kanvibe-user-set-clipboard");

    // When
    harness
        .attach_local(
            tmux_request("task-clipboard-user-choice"),
            &FakeClient::new(),
        )
        .unwrap();

    // Then
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(!bootstrap_command.contains("set-clipboard"));
    assert!(bootstrap_command.contains("set-option -t 'feat-login' destroy-unattached off"));
}

#[test]
fn local_tmux_bootstrap_leaves_set_clipboard_alone_when_the_check_fails() {
    // Given
    let harness = harness_without_tmux_session();
    harness
        .runner
        .fail_when_contains("set-clipboard", "grep unavailable");

    // When
    harness
        .attach_local(
            tmux_request("task-clipboard-check-failure"),
            &FakeClient::new(),
        )
        .unwrap();

    // Then
    assert!(
        !harness
            .command_containing("new-session")
            .unwrap()
            .contains("set-clipboard")
    );
}

#[test]
fn local_tmux_bootstrap_retries_without_the_user_config_after_failure() {
    // Given
    let harness = harness_without_tmux_session();
    let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted_attempts = attempts.clone();
    harness.runner.respond(move |command, _| {
        if !command.contains("new-session") {
            return None;
        }
        let attempt = counted_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        (attempt == 0).then(|| Err(kanvibe_process::exec::ExecGitError::new("no sessions")))
    });

    // When
    harness
        .attach_local(tmux_request("task-retry"), &FakeClient::new())
        .unwrap();

    // Then
    let bootstrap_commands: Vec<String> = harness
        .commands()
        .into_iter()
        .filter(|command| command.contains("new-session"))
        .collect();
    assert_eq!(bootstrap_commands.len(), 2);
    assert!(!bootstrap_commands[0].contains("-f /dev/null"));
    assert!(bootstrap_commands[1].contains("tmux -f /dev/null new-session"));
}

#[test]
fn local_tmux_bootstrap_with_multiline_quoted_pane_command_is_posix_valid() {
    // Given
    let harness = harness_without_tmux_session();
    let mut request = tmux_request("task-layout-quoted");
    request.pane_layout = vertical_layout(vec![pane(
        0,
        "nvm use 24 && \nnode -e \"console.log('ok')\"",
    )]);

    // When
    harness.attach_local(request, &FakeClient::new()).unwrap();

    // Then
    assert_valid_posix_syntax(&harness.command_containing("new-session").unwrap());
}

#[test]
fn alive_local_tmux_session_is_not_created_again() {
    // Given
    let harness = Harness::new();

    // When
    harness
        .attach_local(tmux_request("task-2"), &FakeClient::new())
        .unwrap();

    // Then
    assert_eq!(harness.command_containing("new-session"), None);
}

#[test]
fn local_tmux_pty_attaches_the_session_without_window_targeting() {
    // Given
    let harness = Harness::new();

    // When
    harness
        .attach_local(tmux_request("task-3"), &FakeClient::new())
        .unwrap();

    // Then
    let request = harness.ptys.request(0);
    assert_eq!(request.program, "tmux");
    assert_eq!(request.args, vec!["attach-session", "-t", "feat-login"]);
    assert_eq!(request.environment, harness.environment.shell_environment);
    assert_eq!(
        request.environment.get("PATH").map(String::as_str),
        Some("/usr/bin:/bin")
    );
}

#[test]
fn local_tmux_creation_failure_closes_the_client_with_1008() {
    // Given
    let harness = harness_without_tmux_session();
    harness
        .runner
        .fail_when_contains("new-session", "tmux creation failed");
    let client = FakeClient::new();

    // When
    let result = harness.attach_local(tmux_request("task-4"), &client);

    // Then
    assert_eq!(result, Err("tmux 세션 생성에 실패했습니다.".to_string()));
    assert_eq!(
        client.closures(),
        vec![closure(1008, "tmux 세션 생성에 실패했습니다.")]
    );
    assert_eq!(harness.ptys.count(), 0);
}

#[test]
fn focusing_a_task_does_not_switch_the_host_tmux_client() {
    // Given
    let harness = Harness::new();
    harness
        .attach_local(tmux_request("task-focus"), &FakeClient::new())
        .unwrap();
    harness.runner.clear_calls();

    // When
    harness.registry.focus_session("task-focus");

    // Then
    assert_eq!(harness.command_containing("switch-client"), None);
}

#[test]
fn plain_terminal_snapshot_reads_rendered_text_instead_of_control_bytes() {
    // Given
    let harness = Harness::new();
    let request = local_request(
        "plain-chat",
        Some("plain-tab"),
        SessionType::Terminal,
        "plain-chat",
        Some("/workspace"),
    );
    harness.attach_local(request, &FakeClient::new()).unwrap();

    // When
    harness
        .ptys
        .pty(0)
        .emit_output("\x1b[31mOLD\x1b[0m\r\x1b[2K\x1b[32mSelected model\x1b[0m\r\n› ");
    let snapshot = harness
        .registry
        .read_local_terminal_tab_snapshot("plain-chat", "plain-tab")
        .unwrap();

    // Then
    assert_eq!(snapshot.pid, 1000);
    assert_eq!(snapshot.reading.input_line, "› ");
    assert_eq!(snapshot.reading.screen, "Selected model\n›");
    assert!(!snapshot.reading.screen.contains('\x1b'));
    assert!(!snapshot.reading.screen.contains("OLD"));
    harness.registry.kill_all_terminal_sessions();
}
