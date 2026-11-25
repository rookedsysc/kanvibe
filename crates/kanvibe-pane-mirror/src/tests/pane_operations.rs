//! 모바일 터미널 탭 생성, 모바일 pane 종료, 모바일 pane 종료 정보, 모바일 pane 단축키

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kanvibe_domain::task::SessionType;
use kanvibe_terminal::pane_mirror::build_tmux_capture_pane_command;
use serde_json::json;

use super::fakes::*;
use super::*;
use crate::error::{PaneMirrorError, SurfaceUnavailableReason};

fn tab_ids(surfaces: &crate::surfaces::TaskSurfaces) -> Vec<String> {
    surfaces.tabs.iter().map(|tab| tab.id.clone()).collect()
}

fn zellij_pane(id: i64, tab_id: i64, tab_name: &str) -> serde_json::Value {
    json!({ "id": id, "tab_id": tab_id, "tab_name": tab_name, "is_plugin": false, "terminal_command": "zsh",
        "pane_x": 0, "pane_y": 0, "pane_rows": 24, "pane_columns": 80 })
}

// parity: TS-main-mobileBridgeService-b16e85
#[test]
fn terminal_session_keeps_existing_tabs_and_input_reaches_the_new_tab_pane() {
    let harness = Harness::new();
    harness.set_task(task(
        "new-tab-task",
        Some(SessionType::Terminal),
        Some("new-tab-session"),
    ));

    let created = harness
        .mirror
        .create_mobile_terminal_tab("new-tab-task")
        .unwrap();

    assert_eq!(
        tab_ids(&created.surfaces),
        vec!["new-tab-task-1", "new-tab-task-2"]
    );
    assert_eq!(created.pane_id, "new-tab-task-2");
    let _subscriber = harness.subscribe("new-tab-task", &created.pane_id);
    harness
        .mirror
        .write_to_pane("new-tab-task", &created.pane_id, "pwd\r")
        .unwrap();
    assert_eq!(harness.ptys.count(), 1);
    assert!(harness.ptys.pty(0).writes().contains(&"pwd\r".to_string()));
    assert!(harness.commands().is_empty());
}

// parity: TS-main-mobileBridgeService-85f619
#[test]
fn remote_tmux_creation_uses_the_stored_session_and_worktree_and_returns_the_new_window_pane() {
    let harness = Harness::new();
    let mut task = tmux_task();
    task.ssh_host = Some("remote-tab".to_string());
    task.worktree_path = Some("/work/my project".to_string());
    harness.set_task(task);
    let is_created = AtomicBool::new(false);
    harness.runner.replace(move |command, _| {
        if command.contains("new-window") {
            is_created.store(true, Ordering::SeqCst);
            return Some(Ok("@11".to_string()));
        }
        Some(Ok(match (command.contains("list-panes"), is_created.load(Ordering::SeqCst)) {
            (true, true) => format!("{MIRROR_PANE_LINE}\n%20\t@10\tbash\t0\t0\t80\t24\tfirst\n%21\t@11\tzsh\t0\t0\t80\t24\tsecond"),
            (true, false) => MIRROR_PANE_LINE.to_string(),
            _ => "snapshot".to_string(),
        }))
    });

    let result = harness.mirror.create_mobile_terminal_tab("task-1").unwrap();

    assert_eq!(result.pane_id, "%21");
    assert_eq!(tab_ids(&result.surfaces), vec!["@10", "@11"]);
    let create_calls: Vec<_> = harness
        .runner
        .calls()
        .into_iter()
        .filter(|(command, _)| command.contains("new-window"))
        .collect();
    assert_eq!(
        create_calls,
        vec![(
            "tmux new-window -t 'kanvibe-task': -P -F '#{window_id}' -c '/work/my project'"
                .to_string(),
            Some("remote-tab".to_string()),
        )]
    );
}

// parity: TS-main-mobileBridgeService-6ab29d
#[test]
fn zellij_waits_for_a_late_new_tab_pane_without_repeating_the_create_command() {
    let harness = Harness::new();
    let mut task = task(
        "task-zellij",
        Some(SessionType::Zellij),
        Some("new-tab-zellij"),
    );
    task.worktree_path = Some("/work/tree".to_string());
    harness.set_task(task);
    let is_created = Arc::new(AtomicBool::new(false));
    let reads_after_create = Arc::new(AtomicUsize::new(0));
    let created = Arc::clone(&is_created);
    let reads = Arc::clone(&reads_after_create);
    harness.runner.replace(move |command, _| {
        Some(Ok(if command.contains("--version") {
            "zellij 0.44.3".to_string()
        } else if command.contains("list-sessions") {
            "new-tab-zellij [Created 1m ago]".to_string()
        } else if command.contains("action new-tab") {
            created.store(true, Ordering::SeqCst);
            String::new()
        } else if command.contains("list-panes") {
            let is_ready =
                created.load(Ordering::SeqCst) && reads.fetch_add(1, Ordering::SeqCst) + 1 > 1;
            let panes = match is_ready {
                true => json!([zellij_pane(1, 0, "first"), zellij_pane(2, 1, "second")]),
                false => json!([zellij_pane(1, 0, "first")]),
            };
            panes.to_string()
        } else {
            String::new()
        }))
    });

    let result = harness
        .mirror
        .create_mobile_terminal_tab("task-zellij")
        .unwrap();

    assert_eq!(result.pane_id, "terminal_2");
    assert_eq!(result.surfaces.tabs.len(), 2);
    let create_calls: Vec<_> = harness
        .runner
        .calls()
        .into_iter()
        .filter(|(command, _)| command.contains("action new-tab"))
        .collect();
    assert_eq!(
        create_calls,
        vec![(
            "zellij --session 'new-tab-zellij' action new-tab --cwd '/work/tree'".to_string(),
            None
        )]
    );
    assert_eq!(reads_after_create.load(Ordering::SeqCst), 2);
}

// parity: TS-main-mobileBridgeService-0f83e8
#[test]
fn task_without_a_session_runs_no_create_command() {
    let harness = Harness::new();
    harness.set_task(task("task-missing", None, None));

    let error = harness
        .mirror
        .create_mobile_terminal_tab("task-missing")
        .unwrap_err();

    assert_eq!(
        error,
        PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::NoSession)
    );
    assert!(harness.commands().is_empty());
}

// parity: TS-main-mobileBridgeService-fc1ed0
#[test]
fn failed_create_command_is_not_repeated_and_the_listing_is_not_polled() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();
    harness.runner.respond(|command, _| {
        command
            .contains("new-window")
            .then(|| Err("tmux create failed".to_string()))
    });

    let error = harness
        .mirror
        .create_mobile_terminal_tab("task-1")
        .unwrap_err();

    assert!(error.to_string().contains("tmux create failed"));
    assert_eq!(harness.runner.commands_containing("new-window").len(), 1);
    assert_eq!(harness.runner.commands_containing("list-panes").len(), 1);
}

// parity: TS-main-mobileBridgeService-b7a495
#[test]
fn new_pane_that_never_appears_fails_at_the_deadline_after_one_create() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let error = harness
        .mirror
        .create_mobile_terminal_tab("task-1")
        .unwrap_err();

    assert_eq!(error.to_string(), "추가한 터미널 탭을 찾을 수 없습니다.");
    assert_eq!(harness.runner.commands_containing("new-window").len(), 1);
}

const FIRST_PANE_LINE: &str = "%19\t@10\tbash\t0\t0\t50\t30\tfirst";
const SECOND_PANE_LINE: &str = "%20\t@10\tclaude\t51\t0\t49\t30\tfirst";

/// 종료 명령을 받은 pane은 이후 목록에서 빠지는 tmux 세션
fn stub_tmux_panes(harness: &Harness, initial_lines: &[&str]) {
    let remaining = Mutex::new(
        initial_lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>(),
    );
    harness.set_task(tmux_task());
    harness.runner.replace(move |command, _| {
        if let Some(killed) = command
            .strip_prefix("tmux kill-pane -t '")
            .and_then(|rest| rest.strip_suffix('\''))
        {
            remaining
                .lock()
                .unwrap()
                .retain(|line| !line.starts_with(&format!("{killed}\t")));
            return Some(Ok(String::new()));
        }
        Some(Ok(match command.contains("list-panes") {
            true => remaining.lock().unwrap().join("\n"),
            false => "snapshot".to_string(),
        }))
    });
}

fn kill_commands(harness: &Harness) -> Vec<String> {
    harness.runner.commands_containing("kill-pane")
}

// parity: TS-main-mobileBridgeService-1a72c3
#[test]
fn closing_one_of_two_panes_in_a_window_leaves_the_other_and_returns_the_remaining_panes() {
    let harness = Harness::new();
    stub_tmux_panes(&harness, &[FIRST_PANE_LINE, SECOND_PANE_LINE]);

    let result = harness.mirror.close_mobile_pane("task-1", "%20").unwrap();

    assert_eq!(kill_commands(&harness), vec!["tmux kill-pane -t '%20'"]);
    let pane_ids: Vec<Vec<String>> = result
        .surfaces
        .tabs
        .iter()
        .map(|tab| tab.panes.iter().map(|pane| pane.id.clone()).collect())
        .collect();
    assert_eq!(pane_ids, vec![vec!["%19".to_string()]]);
}

// parity: TS-main-mobileBridgeService-53a747
#[test]
fn the_only_pane_of_a_session_is_refused_without_a_close_command() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    assert_eq!(
        harness
            .mirror
            .close_mobile_pane("task-1", "%19")
            .unwrap_err(),
        PaneMirrorError::LastPane
    );
    assert!(kill_commands(&harness).is_empty());
}

// parity: TS-main-mobileBridgeService-bb4a6b
#[test]
fn pane_outside_the_session_is_refused_without_a_close_command() {
    let harness = Harness::new();
    stub_tmux_panes(&harness, &[FIRST_PANE_LINE, SECOND_PANE_LINE]);

    assert_eq!(
        harness
            .mirror
            .close_mobile_pane("task-1", "%3")
            .unwrap_err(),
        PaneMirrorError::PaneNotFound
    );
    assert!(kill_commands(&harness).is_empty());
}

// parity: TS-main-mobileBridgeService-d94f65
#[test]
fn two_devices_closing_the_last_two_panes_together_close_only_one_and_keep_the_session() {
    let harness = Arc::new(Harness::new());
    stub_tmux_panes(&harness, &[FIRST_PANE_LINE, SECOND_PANE_LINE]);

    let closes: Vec<_> = ["%19", "%20"]
        .into_iter()
        .map(|pane_id| {
            let harness = Arc::clone(&harness);
            std::thread::spawn(move || harness.mirror.close_mobile_pane("task-1", pane_id))
        })
        .collect();
    let results: Vec<_> = closes
        .into_iter()
        .map(|close| close.join().unwrap())
        .collect();

    assert_eq!(kill_commands(&harness).len(), 1);
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
}

// parity: TS-main-mobileBridgeService-881980
#[test]
fn zellij_closes_only_the_pane_named_by_id() {
    let harness = Harness::new();
    harness.set_task(task(
        "task-zellij",
        Some(SessionType::Zellij),
        Some("close-zellij"),
    ));
    let is_second_closed = AtomicBool::new(false);
    harness.runner.replace(move |command, _| {
        Some(Ok(if command.contains("--version") {
            "zellij 0.44.3".to_string()
        } else if command.contains("list-sessions") {
            "close-zellij [Created 1m ago]".to_string()
        } else if command.contains("action close-pane") {
            is_second_closed.store(true, Ordering::SeqCst);
            String::new()
        } else if command.contains("list-panes") {
            match is_second_closed.load(Ordering::SeqCst) {
                true => json!([zellij_pane(1, 0, "first")]).to_string(),
                false => {
                    json!([zellij_pane(1, 0, "first"), zellij_pane(2, 1, "second")]).to_string()
                }
            }
        } else {
            String::new()
        }))
    });

    let result = harness
        .mirror
        .close_mobile_pane("task-zellij", "terminal_2")
        .unwrap();

    assert_eq!(
        harness.runner.commands_containing("close-pane"),
        vec!["zellij --session 'close-zellij' action close-pane --pane-id 'terminal_2'"]
    );
    assert_eq!(result.surfaces.tabs.len(), 1);
}

// parity: TS-main-mobileBridgeService-466f2a
#[test]
fn terminal_session_pane_is_its_tab_so_only_that_tab_closes() {
    let harness = Harness::new();
    harness.set_task(task(
        "close-tab-task",
        Some(SessionType::Terminal),
        Some("close-tab-session"),
    ));
    harness
        .mirror
        .create_mobile_terminal_tab("close-tab-task")
        .unwrap();

    let result = harness
        .mirror
        .close_mobile_pane("close-tab-task", "close-tab-task-2")
        .unwrap();

    assert_eq!(tab_ids(&result.surfaces), vec!["close-tab-task-1"]);
    assert!(harness.commands().is_empty());
}

// parity: TS-main-mobileBridgeService-8f42c5
#[test]
fn the_only_terminal_session_tab_is_refused() {
    let harness = Harness::new();
    harness.set_task(task(
        "single-tab-task",
        Some(SessionType::Terminal),
        Some("single-tab-session"),
    ));

    let error = harness
        .mirror
        .close_mobile_pane("single-tab-task", "single-tab-task-1")
        .unwrap_err();

    assert_eq!(error, PaneMirrorError::LastPane);
}

fn stub_pane_command(harness: &Harness, command_name: &str) {
    harness.set_task(tmux_task());
    let line = format!("%19\t@10\t{command_name}\t0\t0\t80\t24\tfirst");
    harness.runner.replace(move |command, _| {
        Some(Ok(match command.contains("list-panes") {
            true => line.clone(),
            false => "snapshot".to_string(),
        }))
    });
}

// parity: TS-main-mobileBridgeService-31c9b7
#[test]
fn idle_shell_reports_no_running_process() {
    for shell in ["sh", "bash", "zsh", "fish", "dash"] {
        let harness = Harness::new();
        stub_pane_command(&harness, shell);

        let info = harness
            .mirror
            .get_mobile_pane_close_info("task-1", "%19")
            .unwrap();

        assert!(info.process_names.is_empty(), "{shell}");
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            json!({ "processNames": [] })
        );
    }
}

// parity: TS-main-mobileBridgeService-50a87f
#[test]
fn running_non_shell_command_is_reported_by_name() {
    let harness = Harness::new();
    stub_pane_command(&harness, "vim");

    let info = harness
        .mirror
        .get_mobile_pane_close_info("task-1", "%19")
        .unwrap();

    assert_eq!(info.process_names, vec!["vim"]);
}

// parity: TS-main-mobileBridgeService-12ac46
#[test]
fn failed_lookup_raises_an_error_instead_of_assuming_nothing_runs() {
    let harness = Harness::new();
    harness.set_task(tmux_task());
    harness
        .runner
        .replace(|_, _| Some(Err("tmux unreachable".to_string())));

    assert!(
        harness
            .mirror
            .get_mobile_pane_close_info("task-1", "%19")
            .is_err()
    );
}

// parity: TS-main-mobileBridgeService-cc1e09
#[test]
fn pane_outside_the_session_is_refused_without_lookup() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let error = harness
        .mirror
        .get_mobile_pane_close_info("task-1", "%3")
        .unwrap_err();

    assert_eq!(error, PaneMirrorError::PaneNotFound);
}

const SPLIT_PANE_LINES: &str =
    "%19\t@10\tbash\t0\t0\t80\t24\tfirst\n%20\t@10\tbash\t40\t0\t40\t24\tfirst";

fn shortcut(
    harness: &Harness,
    task_id: &str,
    pane_id: &str,
    action: &str,
) -> crate::error::MirrorResult<crate::pane_operations::ShortcutOutcome> {
    harness
        .mirror
        .execute_mobile_terminal_shortcut(task_id, pane_id, action)
}

// parity: TS-main-mobileBridgeService-f56038
#[test]
fn split_affects_only_the_selected_pane_and_returns_the_updated_listing() {
    let harness = Harness::new();
    harness.set_task(tmux_task());
    let did_split = AtomicBool::new(false);
    harness.runner.replace(move |command, _| {
        if command.contains("split-window") {
            did_split.store(true, Ordering::SeqCst);
        }
        Some(Ok(
            match (
                command.contains("list-panes"),
                did_split.load(Ordering::SeqCst),
            ) {
                (true, true) => SPLIT_PANE_LINES.to_string(),
                (true, false) => MIRROR_PANE_LINE.to_string(),
                _ => "snapshot".to_string(),
            },
        ))
    });

    let result = shortcut(&harness, "shortcut-task", "%19", "split-right").unwrap();

    assert!(
        harness
            .commands()
            .contains(&"tmux split-window -h -d -t '%19' -c '#{pane_current_path}'".to_string())
    );
    assert_eq!(result.surfaces.session_type, SessionType::Tmux);
    assert_eq!(result.surfaces.tabs[0].panes[0].id, "%19");
    assert_eq!(result.screen, None);
    assert!(
        serde_json::to_value(&result)
            .unwrap()
            .get("screen")
            .is_none()
    );
}

// parity: TS-main-mobileBridgeService-433a31
#[test]
fn late_split_is_awaited_without_repeating_the_command() {
    let harness = Harness::new();
    harness.set_task(tmux_task());
    let list_calls = AtomicUsize::new(0);
    harness.runner.replace(move |command, _| {
        if command.contains("list-panes") {
            let count = list_calls.fetch_add(1, Ordering::SeqCst) + 1;
            return Some(Ok(match count > 2 {
                true => SPLIT_PANE_LINES.to_string(),
                false => MIRROR_PANE_LINE.to_string(),
            }));
        }
        Some(Ok("snapshot".to_string()))
    });

    let result = shortcut(&harness, "shortcut-task", "%19", "split-down").unwrap();

    assert_eq!(result.surfaces.tabs[0].panes.len(), 2);
    assert_eq!(harness.runner.commands_containing("split-window").len(), 1);
}

// parity: TS-main-mobileBridgeService-77d33a
#[test]
fn shortcut_is_not_run_on_another_task_pane() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let error = shortcut(&harness, "shortcut-task", "%3", "toggle-fullscreen").unwrap_err();

    assert_eq!(
        error,
        PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::SessionNotRunning)
    );
    assert!(harness.runner.commands_containing("resize-pane").is_empty());
}

// parity: TS-main-mobileBridgeService-81e200
#[test]
fn unknown_action_runs_no_shell_command() {
    for action in ["unknown", "toString", "; rm -rf /"] {
        let harness = Harness::new();
        harness.stub_tmux_mirror();

        let error = shortcut(&harness, "shortcut-task", "%19", action).unwrap_err();

        assert!(
            matches!(error, PaneMirrorError::MobileTaskInput(_)),
            "{action}"
        );
        assert!(harness.commands().is_empty(), "{action}");
    }
}

// parity: TS-main-mobileBridgeService-36d31c
#[test]
fn terminal_session_refuses_multiplexer_commands() {
    let harness = Harness::new();
    harness.set_task(task(
        "shortcut-task",
        Some(SessionType::Terminal),
        Some("term"),
    ));

    let error = shortcut(&harness, "shortcut-task", "term-pane", "split-down").unwrap_err();

    assert!(matches!(error, PaneMirrorError::MobileTaskInput(_)));
    assert!(harness.commands().is_empty());
}

// parity: TS-main-mobileBridgeService-96e926
#[test]
fn tmux_copy_mode_screen_is_returned_because_pipe_pane_never_shows_it() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let result = shortcut(&harness, "shortcut-task", "%19", "scroll-up").unwrap();

    assert!(
        harness
            .commands()
            .contains(&"tmux capture-pane -peM -t '%19'".to_string())
    );
    assert_eq!(result.screen.as_deref(), Some(SNAPSHOT_REDRAW));
}

// parity: TS-main-mobileBridgeService-5632b4
#[test]
fn returning_from_tmux_scroll_leaves_copy_mode_and_returns_the_original_screen() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let result = shortcut(&harness, "shortcut-task", "%19", "scroll-bottom").unwrap();

    let commands = harness.commands();
    assert!(
        commands
            .contains(&"tmux copy-mode -t '%19' && tmux send-keys -X -t '%19' cancel".to_string())
    );
    assert!(commands.contains(&build_tmux_capture_pane_command("%19")));
    assert_eq!(result.screen.as_deref(), Some(SNAPSHOT_REDRAW));
}

// parity: TS-main-mobileBridgeService-225699
#[test]
fn remote_zellij_runs_the_pane_id_command_on_the_same_host() {
    let harness = Harness::new();
    let ssh_host = "shortcut-zellij-host";
    let mut task = task(
        "shortcut-zellij",
        Some(SessionType::Zellij),
        Some("remote-session"),
    );
    task.ssh_host = Some(ssh_host.to_string());
    harness.set_task(task);
    harness.runner.replace(|command, _| {
        Some(Ok(if command.contains("--version") {
            "zellij 0.44.0".to_string()
        } else if command.contains("list-sessions") {
            "remote-session [Created 1m ago]".to_string()
        } else if command.contains("list-panes") {
            json!([{ "id": 3, "is_plugin": false, "tab_id": 4, "pane_x": 0, "pane_y": 0, "pane_columns": 80, "pane_rows": 24 }]).to_string()
        } else {
            String::new()
        }))
    });

    shortcut(&harness, "shortcut-zellij", "terminal_3", "toggle-floating").unwrap();

    assert!(harness.runner.calls().contains(&(
        "zellij --session 'remote-session' action toggle-pane-embed-or-floating --pane-id 'terminal_3'".to_string(),
        Some(ssh_host.to_string()),
    )));
}
