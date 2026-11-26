//! pane 접근 범위, 미러 자식 프로세스의 수명, pane 구독 정리, 첫 화면 스냅샷

use kanvibe_domain::task::SessionType;
use serde_json::json;

use super::fakes::*;
use super::*;
use crate::error::{PaneMirrorError, SurfaceUnavailableReason};

const CANCEL_PIPE: &str = "tmux pipe-pane -t '%19'";

fn session_not_running() -> PaneMirrorError {
    PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::SessionNotRunning)
}

#[test]
fn pane_outside_the_session_cannot_be_subscribed() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let error = harness
        .try_subscribe("task-scope-read", "%3")
        .err()
        .unwrap();

    assert_eq!(error, session_not_running());
    assert!(
        harness
            .runner
            .commands_containing("capture-pane")
            .is_empty()
    );
}

#[test]
fn keys_cannot_reach_a_pane_outside_the_session() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    let error = harness
        .mirror
        .write_to_pane("task-scope-write", "%3", "ls")
        .unwrap_err();

    assert_eq!(error, session_not_running());
    assert!(harness.runner.commands_containing("send-keys").is_empty());
}

#[test]
fn keys_reach_a_pane_inside_the_session_as_is() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();

    harness
        .mirror
        .write_to_pane("task-scope-write", "%19", "ls")
        .unwrap();

    assert!(
        !harness
            .runner
            .commands_containing("send-keys -t '%19'")
            .is_empty()
    );
}

#[test]
fn subscriber_is_told_when_the_child_dies_by_error_or_exit() {
    for is_read_failure in [true, false] {
        let harness = Harness::new();
        harness.stub_tmux_mirror();
        let subscriber = harness.subscribe("task-death", "%19");

        match is_read_failure {
            true => harness.processes.child(0).fail(),
            false => harness.processes.child(0).exit(),
        }

        wait_until("끝을 알린다", || subscriber.ended() == 1);
    }
}

#[test]
fn dead_subscription_frees_its_slot_so_the_next_subscriber_gets_a_new_stream() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();
    let first = harness.subscribe("task-revive", "%19");
    harness.processes.child(0).exit();
    wait_until("첫 스트림이 끝난다", || first.ended() == 1);

    let second = harness.subscribe("task-revive", "%19");
    harness.processes.child(1).emit("살아 있다");

    wait_until("새 스트림 출력", || second.chunks().len() == 2);
    assert_eq!(second.chunks(), vec![SNAPSHOT_REDRAW, "살아 있다"]);
}

#[test]
fn quitting_the_app_removes_the_pipes_of_open_mirrors() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();
    let _subscriber = harness.subscribe("task-quit", "%19");
    assert!(!harness.commands().contains(&CANCEL_PIPE.to_string()));

    harness.mirror.stop_all_pane_mirrors(false);

    wait_until("파이프를 끊는다", || {
        harness.commands().contains(&CANCEL_PIPE.to_string())
    });
    assert_eq!(harness.processes.child(0).terminate_count(), 1);
}

#[test]
fn stream_stays_while_a_claimed_subscriber_remains_after_the_earlier_one_leaves() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();
    let first = harness.subscribe("task-race", "%19");
    let second = harness.subscribe("task-race", "%19");

    first.handle.stop();
    harness.processes.child(0).emit("stream");

    wait_until("스트림 출력", || second.chunks().len() == 2);
    assert_eq!(second.chunks(), vec![SNAPSHOT_REDRAW, "stream"]);
    assert!(!harness.commands().contains(&CANCEL_PIPE.to_string()));
    assert_eq!(harness.processes.count(), 1);
    second.handle.stop();
    wait_until("마지막 구독자가 파이프를 끊는다", || {
        harness.commands().contains(&CANCEL_PIPE.to_string())
    });
}

#[test]
fn releasing_the_same_subscription_twice_does_not_take_the_remaining_share() {
    let harness = Harness::new();
    harness.stub_tmux_mirror();
    let first = harness.subscribe("task-idempotent", "%19");
    let second = harness.subscribe("task-idempotent", "%19");

    first.handle.stop();
    first.handle.stop();
    harness.processes.child(0).emit("stream");

    wait_until("스트림 출력", || second.chunks().len() == 2);
    assert_eq!(second.chunks(), vec![SNAPSHOT_REDRAW, "stream"]);
    assert!(!harness.commands().contains(&CANCEL_PIPE.to_string()));
    assert_eq!(harness.processes.child(0).terminate_count(), 0);
}

fn stub_capture(harness: &Harness, capture: &'static str) {
    harness.set_task(tmux_task());
    harness.runner.replace(move |command, _| {
        Some(Ok(if command.contains("list-panes") {
            MIRROR_PANE_LINE.to_string()
        } else if command.contains("capture-pane") {
            capture.to_string()
        } else {
            String::new()
        }))
    });
}

#[test]
fn tmux_snapshot_returns_to_the_line_start_on_each_line_and_ends_at_the_desktop_cursor() {
    let harness = Harness::new();
    // capture-pane은 pane 높이만큼 줄을 내보내고, 이어서 커서 위치 줄이 온다. 실행기는 끝 공백을 다듬는다
    stub_capture(&harness, "$ ls\nREADME.md\n\n2 2 3 0 0 0 0 0 0");

    let subscriber = harness.subscribe("task-screen", "%19");

    assert_eq!(
        subscriber.chunks()[0],
        "\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[H\x1b[2J\x1b[3J$ ls\r\nREADME.md\r\n\x1b[3G"
    );
}

#[test]
fn tmux_alternate_screen_and_sgr_mouse_modes_set_before_connecting_are_restored_in_the_first_frame()
{
    let harness = Harness::new();
    stub_capture(&harness, "ready\n2 0 1 1 1 0 0 1 0");

    let subscriber = harness.subscribe("task-screen-mouse", "%19");

    let first = &subscriber.chunks()[0];
    for expected in ["\x1b[?1049h", "\x1b[?1000h", "\x1b[?1006h", "ready\x1b[3G"] {
        assert!(first.contains(expected), "{expected:?}");
    }
    assert!(!first.contains("2 0 1 1 1 0 0 1 0"));
}

#[test]
fn zellij_screen_dump_is_cleared_and_redrawn_so_dumps_do_not_pile_up() {
    let harness = Harness::new();
    harness.set_task(task(
        "task-zj-screen",
        Some(SessionType::Zellij),
        Some("kanvibe-zj"),
    ));
    harness.runner.replace(|command, _| {
        Some(Ok(if command.contains("--version") {
            "zellij 0.44.3".to_string()
        } else if command.contains("list-sessions") {
            "kanvibe-zj [Created 1m ago]".to_string()
        } else if command.contains("dump-screen") {
            "$ ls\nREADME.md".to_string()
        } else {
            json!([{ "id": 1, "is_plugin": false, "tab_id": 0, "tab_name": "Tab #1", "title": "Pane #1",
                "pane_x": 0, "pane_y": 0, "pane_rows": 24, "pane_columns": 80 }])
            .to_string()
        }))
    });

    let subscriber = harness.subscribe("task-zj-screen", "terminal_1");

    // 휠을 Zellij에 넘기도록 대체 화면에 두고, Zellij가 휠로 올려 둔 위치가 보이게 스크롤백 없이 보이는 화면만 뜬다
    assert_eq!(
        subscriber.chunks()[0],
        "\x1b[H\x1b[2J\x1b[3J\x1b[?1049h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l\x1b[H\x1b[2J\x1b[3J$ ls\r\nREADME.md"
    );
    assert!(harness.commands().contains(
        &"zellij --session 'kanvibe-zj' action dump-screen -p 'terminal_1' -a".to_string()
    ));
}
