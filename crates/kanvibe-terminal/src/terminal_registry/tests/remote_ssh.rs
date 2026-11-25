//! TS `terminal.test.ts` "attachRemoteSession — zellij 원격 세션", "attachRemoteSession — ssh 바이너리 기반 연결"

use std::process::Command;

use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutType};
use kanvibe_domain::task::SessionType;
use kanvibe_process::worktree::PaneLayoutSelection;

use super::fakes::*;
use crate::terminal_registry::RemoteAttachRequest;

fn tmux_request(task_id: &str, worktree_path: Option<&str>, panes: &[&str]) -> RemoteAttachRequest {
    let mut request = remote_request(
        task_id,
        None,
        "remote-host",
        SessionType::Tmux,
        "remote-session",
        worktree_path,
    );
    if !panes.is_empty() {
        request.pane_layout = Some(PaneLayoutSelection {
            layout_type: PaneLayoutType::Vertical2,
            panes: panes
                .iter()
                .enumerate()
                .map(|(position, command)| PaneCommand {
                    position: position as i64,
                    command: command.to_string(),
                })
                .collect(),
        });
    }
    request
}

/// 원격 attach를 띄우고 ssh 인자와 풀어 낸 원격 명령을 돌려준다
fn spawn_remote(harness: &Harness, request: RemoteAttachRequest) -> (Vec<String>, String) {
    harness.attach_remote(request, &FakeClient::new()).unwrap();
    let spawn_request = harness.ptys.request(0);
    assert_eq!(spawn_request.program, "ssh");
    let attach_command = extract_remote_shell_payload(spawn_request.args.last().unwrap());
    (spawn_request.args, attach_command)
}

// parity: TS-lib-terminal-c8b438
#[test]
fn remote_zellij_attach_creates_the_session_with_its_layout_when_missing() {
    // Given
    let harness = Harness::new();
    let request = remote_request(
        "task-remote-zellij",
        None,
        "remote-host",
        SessionType::Zellij,
        "remote-session",
        Some("/remote/worktree"),
    );

    // When
    let (ssh_args, attach_command) = spawn_remote(&harness, request);

    // Then: 존재 확인에만 stderr를 감춰야 대화형 세션이 내는 zellij 오류가 사용자에게 보인다
    assert!(attach_command.contains("zellij list-sessions 2>/dev/null"));
    assert!(attach_command.contains("exec zellij attach 'remote-session';"));
    assert!(!attach_command.contains("zellij attach 'remote-session' 2>/dev/null"));
    // cd 실패를 조용히 넘기면 레이아웃은 맞는데 작업 디렉터리만 엉뚱한 세션이 생긴다
    assert!(attach_command.contains("cd '/remote/worktree' || printf"));
    assert!(attach_command.contains("cannot enter /remote/worktree"));
    assert!(attach_command.contains(
        "exec zellij --session 'remote-session' --new-session-with-layout '/remote/worktree/.zellij-layout.kdl'"
    ));
    assert!(attach_command.contains("exec zellij --session 'remote-session'"));
    assert_valid_posix_syntax(ssh_args.last().unwrap());
    assert_valid_posix_syntax(&attach_command);
}

// parity: TS-lib-terminal-95197d
#[test]
fn remote_zellij_attach_still_creates_the_session_without_a_worktree() {
    // Given
    let harness = Harness::new();
    let request = remote_request(
        "task-remote-zellij-no-worktree",
        None,
        "remote-host",
        SessionType::Zellij,
        "remote-session",
        None,
    );

    // When
    let (_, attach_command) = spawn_remote(&harness, request);

    // Then
    assert!(attach_command.contains(
        "| grep -qFx -- 'remote-session'; then exec zellij attach 'remote-session'; fi;"
    ));
    assert!(attach_command.contains("exec zellij --session 'remote-session'"));
    assert_valid_posix_syntax(&attach_command);
}

// parity: TS-lib-terminal-ee04cc
#[test]
fn remote_tmux_spawns_ssh_with_forced_tty_and_health_options() {
    // Given
    let harness = Harness::new();

    // When
    let (ssh_args, attach_command) = spawn_remote(
        &harness,
        tmux_request("task-r1", Some("/remote/worktree"), &[]),
    );

    // Then
    let expected: Vec<&str> = vec![
        "-i",
        "/tmp/test-key",
        "-p",
        "2202",
        "-o",
        "BatchMode=yes",
        "-o",
        "IdentitiesOnly=yes",
        "-tt",
        "-o",
        "ConnectTimeout=8",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "remote-host",
    ];
    assert_eq!(ssh_args[..ssh_args.len() - 1], expected);
    assert!(
        !ssh_args
            .iter()
            .any(|arg| arg == "-Y" || arg == "ControlMaster=auto")
    );
    assert!(attach_command.contains("tmux has-session -t 'remote-session'"));
    assert!(
        attach_command.contains("tmux new-session -d -s 'remote-session' -c '/remote/worktree'")
    );
    assert!(harness.ptys.pty(0).writes().is_empty());
    assert_eq!(
        harness.ptys.request(0).working_directory,
        "/home/local-user"
    );
}

// parity: TS-lib-terminal-9390a4
#[test]
fn remote_tmux_attach_command_is_posix_valid() {
    // Given
    let harness = Harness::new();

    // When
    let (ssh_args, attach_command) = spawn_remote(
        &harness,
        tmux_request("task-r-shell", Some("/remote/worktree"), &[]),
    );

    // Then
    assert_valid_posix_syntax(ssh_args.last().unwrap());
    assert_valid_posix_syntax(&attach_command);
    assert!(harness.ptys.pty(0).writes().is_empty());
}

// parity: TS-lib-terminal-00373f
#[test]
fn remote_tmux_bootstrap_is_passed_as_the_ssh_command() {
    // Given
    let harness = Harness::new();

    // When
    let (ssh_args, attach_command) = spawn_remote(
        &harness,
        tmux_request("task-r-interactive", Some("/remote/worktree"), &[]),
    );

    // Then
    assert_eq!(ssh_args[ssh_args.len() - 2], "remote-host");
    assert!(ssh_args.last().unwrap().starts_with("sh -lc "));
    assert!(attach_command.contains("tmux has-session -t 'remote-session'"));
    assert!(harness.ptys.pty(0).writes().is_empty());
}

// parity: TS-lib-terminal-f6c37c
#[test]
fn trusted_x11_forwarding_is_requested_only_with_a_local_display() {
    // Given
    let harness = Harness::with_parent_variables(&[("DISPLAY", ":0")]);
    let mut request = tmux_request("task-r-x11", None, &[]);
    request.columns = None;
    request.rows = None;

    // When
    let (ssh_args, _) = spawn_remote(&harness, request);

    // Then
    let x11_index = ssh_args.iter().position(|arg| arg == "-Y").unwrap();
    let destination_index = ssh_args
        .iter()
        .position(|arg| arg == "remote-host")
        .unwrap();
    assert!(x11_index < destination_index);
}

// parity: TS-lib-terminal-d2cc93
#[test]
fn remote_tmux_bootstrap_applies_pane_layout() {
    // Given
    let harness = Harness::new();

    // When
    let request = tmux_request(
        "task-r2",
        Some("/remote/worktree"),
        &["pnpm dev", "pnpm test"],
    );
    let (_, attach_command) = spawn_remote(&harness, request);

    // Then
    assert!(attach_command.contains("split-window -h -t 'remote-session:0' -c '/remote/worktree'"));
    assert!(attach_command.contains("send-keys -t 'remote-session:0.0' -- 'pnpm dev' Enter"));
    assert!(attach_command.contains("send-keys -t 'remote-session:0.1' -- 'pnpm test' Enter"));
    assert!(harness.ptys.pty(0).writes().is_empty());
}

// parity: TS-lib-terminal-171b32
#[test]
fn remote_tmux_bootstraps_and_attaches_the_default_socket_in_one_hardened_sequence() {
    // Given
    let harness = Harness::new();

    // When
    let request = tmux_request(
        "task-r-tmux-default-socket",
        Some("/remote/worktree"),
        &["pnpm dev"],
    );
    let (ssh_args, attach_command) = spawn_remote(&harness, request);

    // Then: 사용자가 원격에서 직접 tmux attach를 쳐도 같은 세션에 붙도록 기본 소켓만 쓴다
    assert!(!attach_command.contains("-L 'kanvibe'"));
    assert!(attach_command.contains("if tmux has-session -t 'remote-session' 2>/dev/null;"));
    assert!(attach_command.contains("exec tmux attach-session -t 'remote-session';"));
    assert!(attach_command.contains(
        "tmux new-session -d -s 'remote-session' -c '/remote/worktree' \\; set-option -t 'remote-session' destroy-unattached off"
    ));
    assert!(attach_command.contains("\\; attach-session -t 'remote-session'"));
    assert!(attach_command.contains("tmux -f /dev/null new-session -d -s 'remote-session'"));
    assert!(!attach_command.contains("tmux default server failed"));
    assert!(!attach_command.contains("kanvibe-fallback"));
    assert_eq!(attach_command.matches("has-session").count(), 1);
    assert!(attach_command.contains("tmux session setup failed; leaving the SSH shell open"));
    assert!(attach_command.contains(r#"exec "${SHELL:-/bin/sh}" -l"#));
    assert!(attach_command.len() < 3_000);
    assert_valid_posix_syntax(ssh_args.last().unwrap());
    assert_valid_posix_syntax(&attach_command);
    assert!(harness.ptys.pty(0).writes().is_empty());
}

// parity: TS-lib-terminal-0894ac
#[test]
fn remote_osc52_decision_runs_inside_the_remote_shell() {
    // Given
    let harness = Harness::new();

    // When
    let (_, attach_command) = spawn_remote(
        &harness,
        tmux_request("task-r-clipboard", Some("/remote/worktree"), &[]),
    );

    // Then: 재attach가 대부분인데 판정 왕복을 매번 붙이면 터미널이 열리기 전에 순수 지연만 늘어난다
    assert!(harness.commands().is_empty());
    assert!(attach_command.contains(
        "if tmux has-session -t 'remote-session' 2>/dev/null; then tmux set-option -t 'remote-session' status off 2>/dev/null; exec tmux attach-session -t 'remote-session'; else "
    ));
    assert!(attach_command.contains("set-option -s set-clipboard on"));
    assert_valid_posix_syntax(&attach_command);
}

// parity: TS-lib-terminal-a01913
#[test]
fn remote_set_clipboard_branch_runs_only_when_the_remote_config_is_silent() {
    // Given
    let harness = Harness::new();
    let (_, attach_command) = spawn_remote(
        &harness,
        tmux_request("task-r-clipboard-user", Some("/remote/worktree"), &[]),
    );
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".tmux.conf"), "# synthetic config\n").unwrap();

    for probe_status in [0, 1, 2] {
        // When: grep과 tmux를 셸 함수로 바꿔 실제 tmux 없이 분기만 실행한다
        let stub = format!(
            "grep() {{ return {probe_status}; }}; tmux() {{ case \"$1\" in has-session) return 1 ;; *) printf '%s\\n' \"$*\" ;; esac; }}; "
        );
        let output = Command::new("/bin/sh")
            .env_clear()
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join("config"))
            .args(["-c", &format!("{stub}{attach_command}")])
            .output()
            .unwrap();

        // Then
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            stdout.contains("set-option -s set-clipboard on"),
            probe_status == 1
        );
        assert!(stdout.contains("new-session"));
    }
}

// parity: TS-lib-terminal-aa5b9d
#[test]
fn remote_tmux_attach_with_multiline_quoted_pane_command_is_posix_valid() {
    // Given
    let harness = Harness::new();
    let request = tmux_request(
        "task-r-quoted",
        Some("/remote/worktree"),
        &["nvm use 24 && \nnode -e \"console.log('ok')\""],
    );

    // When
    let (ssh_args, attach_command) = spawn_remote(&harness, request);

    // Then
    assert_valid_posix_syntax(ssh_args.last().unwrap());
    assert_valid_posix_syntax(&attach_command);
    assert!(harness.ptys.pty(0).writes().is_empty());
}

// parity: TS-lib-terminal-6a6db4
#[test]
fn long_remote_tmux_bootstrap_stays_out_of_the_login_prompt() {
    // Given
    let harness = Harness::new();
    let session_name = "2026-06-toss-node-developer-agent-2026-06-toss-node-developer-export-hermes-session-history-20260605-205232";
    let mut request = tmux_request(
        "task-r-long-bootstrap",
        Some(
            "/home/rookedsysc/Documents/toss/2026-06-toss-node-developer__worktrees/agent-2026-06-toss-node-developer-export-hermes-session-history-20260605-205232",
        ),
        &["nvm use 24"],
    );
    request.session_name = session_name.to_string();

    // When
    let (ssh_args, attach_command) = spawn_remote(&harness, request);

    // Then
    assert_eq!(ssh_args[ssh_args.len() - 2], "remote-host");
    assert!(ssh_args.last().unwrap().starts_with("sh -lc "));
    assert!(attach_command.contains("nvm use 24"));
    assert!(!attach_command.contains("-L 'kanvibe'"));
    assert!(!attach_command.contains("kanvibe-fallback"));
    assert!(!attach_command.contains("tmux default server failed"));
    assert!(attach_command.contains(session_name));
    assert_valid_posix_syntax(ssh_args.last().unwrap());
    assert_valid_posix_syntax(&attach_command);
    assert!(harness.ptys.pty(0).writes().is_empty());
}
