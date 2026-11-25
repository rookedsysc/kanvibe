//! TS `terminalSessionPreparation.test.ts`

use std::sync::Arc;

use kanvibe_domain::task::SessionType;

use super::fakes::*;
use crate::terminal_registry::{
    HerdrPaneTab, PreparedTerminalSession, TerminalSessionPreparationTarget,
};

fn base_target() -> TerminalSessionPreparationTarget {
    TerminalSessionPreparationTarget {
        task_id: "task-1".to_string(),
        session_type: SessionType::Tmux,
        session_name: "repo-feat-new".to_string(),
        worktree_path: Some("/repo__worktrees/feat-new".to_string()),
        ssh_host: None,
        pane_layout: None,
    }
}

fn prepared() -> PreparedTerminalSession {
    PreparedTerminalSession {
        is_prepared: true,
        skipped_reason: None,
        herdr_workspace_id: None,
        herdr_panes: None,
    }
}

/// tmux 세션 생성 명령(new-session)이 실행된 횟수. 존재 확인·설정 확인 명령은 세지 않는다
fn count_tmux_bootstraps(harness: &Harness) -> usize {
    harness
        .commands()
        .iter()
        .filter(|command| command.contains("new-session"))
        .count()
}

fn harness_without_tmux_session() -> Harness {
    let harness = Harness::new();
    harness.runner.respond(|command, _| {
        command
            .starts_with("tmux has-session")
            .then(|| Err(kanvibe_process::exec::ExecGitError::new("no session")))
    });
    harness
}

// parity: TS-lib-terminalSessionPreparation-31dc9d
#[test]
fn missing_local_tmux_session_is_bootstrapped_once_without_attach() {
    // Given
    let harness = harness_without_tmux_session();

    // When
    let result = harness.registry.prepare_terminal_session(base_target());

    // Then
    assert_eq!(result, Ok(prepared()));
    assert_eq!(count_tmux_bootstraps(&harness), 1);
    let bootstrap_command = harness.command_containing("new-session").unwrap();
    assert!(bootstrap_command.contains("/repo__worktrees/feat-new"));
    assert!(!bootstrap_command.contains("attach-session"));
}

// parity: TS-lib-terminalSessionPreparation-674584
#[test]
fn concurrent_preparations_of_one_session_bootstrap_once() {
    // Given: 첫 부트스트랩이 끝나기 전에 같은 세션의 준비가 한 번 더 들어온다
    let harness = harness_without_tmux_session();
    let bootstrap_gate = Gate::new();
    let gate_for_runner = Arc::clone(&bootstrap_gate);
    harness.runner.respond(move |command, _| {
        if command.contains("new-session") {
            gate_for_runner.pass();
        }
        None
    });
    let first_registry = Arc::clone(&harness.registry);
    let first = std::thread::spawn(move || first_registry.prepare_terminal_session(base_target()));
    wait_until("첫 준비가 부트스트랩 중이다", || {
        bootstrap_gate.entered_count() == 1
    });
    let second_registry = Arc::clone(&harness.registry);
    let second =
        std::thread::spawn(move || second_registry.prepare_terminal_session(base_target()));
    wait_until("두 번째 준비가 기다린다", || {
        harness
            .registry
            .lock_state()
            .pending_preparations
            .values()
            .any(|preparation| preparation.waiting_count() > 0)
    });

    // When
    bootstrap_gate.open();
    let results = [first.join().unwrap(), second.join().unwrap()];

    // Then
    assert_eq!(results, [Ok(prepared()), Ok(prepared())]);
    assert_eq!(count_tmux_bootstraps(&harness), 1);
}

// parity: TS-lib-terminalSessionPreparation-968478
#[test]
fn alive_tmux_session_is_not_bootstrapped_again() {
    // Given
    let harness = Harness::new();

    // When
    let result = harness.registry.prepare_terminal_session(base_target());

    // Then
    assert_eq!(result, Ok(prepared()));
    assert_eq!(count_tmux_bootstraps(&harness), 0);
}

// parity: TS-lib-terminalSessionPreparation-f87371
#[test]
fn terminal_type_has_nothing_to_prepare_and_runs_no_command() {
    // Given
    let harness = Harness::new();
    let target = TerminalSessionPreparationTarget {
        session_type: SessionType::Terminal,
        ..base_target()
    };

    // When
    let result = harness.registry.prepare_terminal_session(target).unwrap();

    // Then
    assert!(!result.is_prepared);
    assert!(
        result
            .skipped_reason
            .unwrap()
            .contains("미리 만들 세션이 없습니다")
    );
    assert!(harness.commands().is_empty());
}

// parity: TS-lib-terminalSessionPreparation-985b34
#[test]
fn missing_local_zellij_session_is_created_in_background_with_the_layout() {
    // Given: worktree에 레이아웃 파일이 있다
    let harness = Harness::new();
    let worktree = harness.temp_directory.path().join("feat-new");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join(".zellij-layout.kdl"), "layout {}\n").unwrap();
    let worktree = worktree.to_string_lossy().into_owned();
    let target = TerminalSessionPreparationTarget {
        session_type: SessionType::Zellij,
        worktree_path: Some(worktree.clone()),
        ..base_target()
    };

    // When
    harness.registry.prepare_terminal_session(target).unwrap();

    // Then: TS는 execSync의 cwd로 worktree를 정했다. exec_git에는 cwd가 없어 같은 디렉터리로 먼저 이동한다
    assert_eq!(
        harness.command_containing("--create-background").unwrap(),
        format!(
            "cd '{worktree}' && zellij --layout '{worktree}/.zellij-layout.kdl' attach --create-background 'repo-feat-new'"
        )
    );
}

// parity: TS-lib-terminalSessionPreparation-a76e4d
#[test]
fn remote_tmux_preparation_runs_one_login_shell_command_without_attach() {
    // Given
    let harness = Harness::new();
    let target = TerminalSessionPreparationTarget {
        ssh_host: Some("devbox".to_string()),
        ..base_target()
    };

    // When
    harness.registry.prepare_terminal_session(target).unwrap();

    // Then
    let calls = harness.runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].ssh_host.as_deref(), Some("devbox"));
    let remote_command = &calls[0].command;
    assert!(remote_command.starts_with("sh -lc "));
    assert!(remote_command.contains("has-session"));
    assert!(remote_command.contains("new-session"));
    assert!(!remote_command.contains("attach-session"));
    assert_valid_posix_syntax(remote_command);
}

// parity: TS-lib-terminalSessionPreparation-e225f5
#[test]
fn remote_zellij_preparation_creates_in_background_only_when_missing() {
    // Given
    let harness = Harness::new();
    let target = TerminalSessionPreparationTarget {
        session_type: SessionType::Zellij,
        ssh_host: Some("devbox".to_string()),
        ..base_target()
    };

    // When
    harness.registry.prepare_terminal_session(target).unwrap();

    // Then
    let remote_command = harness.commands()[0].clone();
    assert!(remote_command.contains("attach --create-background"));
    assert!(remote_command.contains(".zellij-layout.kdl"));
    assert_valid_posix_syntax(&remote_command);
}

// parity: TS-lib-terminalSessionPreparation-d25563
#[test]
fn herdr_preparation_uses_the_space_bootstrap_and_returns_its_panes() {
    // Given
    let harness = Harness::new();
    harness
        .herdr
        .workspace_ids
        .lock()
        .unwrap()
        .insert("repo-feat-new".to_string(), "w7".to_string());
    *harness.herdr.pane_tabs.lock().unwrap() = vec![HerdrPaneTab {
        pane_id: "w7:p1".to_string(),
        tab_id: "w7:t1".to_string(),
    }];
    let target = TerminalSessionPreparationTarget {
        session_type: SessionType::Herdr,
        session_name: "kanvibe:repo-feat-new".to_string(),
        ssh_host: Some("devbox".to_string()),
        ..base_target()
    };

    // When
    let result = harness.registry.prepare_terminal_session(target).unwrap();

    // Then
    let ensure_call = harness.herdr.ensure_session_calls.lock().unwrap()[0].clone();
    assert_eq!(
        ensure_call,
        (
            "kanvibe:repo-feat-new".to_string(),
            "/repo__worktrees/feat-new".to_string(),
            None,
            Some("devbox".to_string())
        )
    );
    assert_eq!(
        result,
        PreparedTerminalSession {
            is_prepared: true,
            skipped_reason: None,
            herdr_workspace_id: Some("w7".to_string()),
            herdr_panes: Some(vec![HerdrPaneTab {
                pane_id: "w7:p1".to_string(),
                tab_id: "w7:t1".to_string(),
            }]),
        }
    );
}
