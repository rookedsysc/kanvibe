//! Herdr 세션 미러

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kanvibe_domain::task::SessionType;
use kanvibe_terminal::herdr::build_herdr_command;
use kanvibe_terminal::pane_mirror::{
    build_herdr_observe_pane_command, build_herdr_remote_observe_pane_command,
};
use kanvibe_terminal::remote_terminal_protocol::build_wheel_delegated_screen_redraw;
use serde_json::{Value, json};

use super::fakes::*;
use super::*;
use crate::error::{PaneMirrorError, SurfaceUnavailableReason};
use crate::surfaces::SurfaceQuery;

const OBSERVE: &str = "'session' 'observe'";
const CONTROL: &str = "'session' 'control'";

fn herdr_command(arguments: &[&str]) -> String {
    build_herdr_command("herdr-session", arguments).unwrap()
}

fn observe_command(columns: i64, rows: i64) -> String {
    build_herdr_observe_pane_command("herdr-session", "w1:p1", columns, rows).unwrap()
}

/// 이력 다시 그리기. 모바일을 대체 화면에 두고 마우스 보고를 꺼 손가락 스크롤이 Herdr 제어 연결로 가게 한다
fn history_redraw(history_rows: i64) -> String {
    build_wheel_delegated_screen_redraw(&format!("history-{history_rows}"))
}

fn herdr_harness_with(timings: crate::mirror::MirrorTimings) -> Harness {
    let harness = Harness::with_timings(timings);
    harness.set_task(task(
        "herdr-task",
        Some(SessionType::Herdr),
        Some("herdr-session"),
    ));
    let herdr = Arc::clone(&harness.herdr);
    harness.runner.replace(move |command, _| {
        command
            .contains("'pane' 'read'")
            .then(|| Ok(format!("history-{}", *herdr.history_rows.lock().unwrap())))
    });
    harness
}

fn herdr_harness() -> Harness {
    herdr_harness_with(test_timings())
}

fn set_history_rows(harness: &Harness, rows: i64) {
    *harness.herdr.history_rows.lock().unwrap() = rows;
}

fn observers(harness: &Harness) -> Vec<Arc<FakeChild>> {
    harness.processes.matching(OBSERVE)
}

fn controllers(harness: &Harness) -> Vec<Arc<FakeChild>> {
    harness.processes.matching(CONTROL)
}

fn json_lines(lines: &[String]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn scroll(direction: &str, column: i64, row: i64) -> Value {
    json!({ "type": "scroll", "direction": direction, "column": column, "row": row })
}

fn last_chunk(subscriber: &Subscriber) -> Option<String> {
    subscriber.chunks().last().cloned()
}

#[test]
fn minimal_surfaces_return_the_first_pane_coordinates_without_reading_commands() {
    let harness = herdr_harness();
    let query = SurfaceQuery {
        is_minimal: true,
        tab_id: None,
    };

    let surfaces = harness
        .mirror
        .get_task_surfaces("herdr-task", &query)
        .unwrap();

    let pane = &surfaces.tabs[0].panes[0];
    assert_eq!(
        (
            pane.id.as_str(),
            pane.command.as_str(),
            pane.width,
            pane.height
        ),
        ("w1:p1", "", 120, 38)
    );
    assert_eq!(surfaces.geometry_tab_id.as_deref(), Some("w1:t1"));
    assert!(harness.herdr.list_calls()[0].is_minimal);
    let full = harness
        .mirror
        .get_task_surfaces("herdr-task", &SurfaceQuery::default())
        .unwrap();
    assert_eq!(full.tabs[0].panes[0].command, "claude");
    assert!(
        serde_json::to_value(&full)
            .unwrap()
            .get("geometryTabId")
            .is_none()
    );
}

#[test]
fn minimal_surfaces_mark_the_requested_non_first_tab_as_displayable() {
    let harness = herdr_harness();
    *harness.herdr.panes.lock().unwrap() = vec![
        herdr_pane("w1:p1", "w1:t1", "", 0, 0),
        herdr_pane("w1:p2", "w1:t2", "", 0, 60),
        herdr_pane("w1:p3", "w1:t2", "", 60, 60),
    ];
    let query = SurfaceQuery {
        is_minimal: true,
        tab_id: Some("w1:t2".to_string()),
    };

    let surfaces = harness
        .mirror
        .get_task_surfaces("herdr-task", &query)
        .unwrap();

    assert_eq!(surfaces.geometry_tab_id.as_deref(), Some("w1:t2"));
    let layout: Vec<(String, i64, i64)> = surfaces.tabs[1]
        .panes
        .iter()
        .map(|pane| (pane.id.clone(), pane.left, pane.width))
        .collect();
    assert_eq!(
        layout,
        vec![("w1:p2".to_string(), 0, 60), ("w1:p3".to_string(), 60, 60)]
    );
    assert_eq!(
        harness.herdr.list_calls()[0].tab_id.as_deref(),
        Some("w1:t2")
    );
    assert_eq!(
        serde_json::to_value(&surfaces).unwrap()["geometryTabId"],
        "w1:t2"
    );
}

#[test]
fn herdr_surfaces_come_from_herdr_queries_without_tmux_commands() {
    let harness = herdr_harness();

    let surfaces = harness
        .mirror
        .get_task_surfaces("herdr-task", &SurfaceQuery::default())
        .unwrap();

    assert_eq!(surfaces.session_type, SessionType::Herdr);
    assert!(
        surfaces.can_resize
            && surfaces.can_refresh_history
            && !surfaces.can_prepare_direct_connection
    );
    assert_eq!(surfaces.tabs.len(), 1);
    assert_eq!(
        (surfaces.tabs[0].id.as_str(), surfaces.tabs[0].name.as_str()),
        ("w1:t1", "work")
    );
    let pane = &surfaces.tabs[0].panes[0];
    assert_eq!(
        (
            pane.id.as_str(),
            pane.command.as_str(),
            pane.width,
            pane.height
        ),
        ("w1:p1", "claude", 120, 38)
    );
    assert!(harness.runner.commands_containing("tmux").is_empty());
}

#[test]
fn missing_session_and_missing_herdr_report_the_install_reason_without_starting() {
    let harness = herdr_harness();
    harness.herdr.is_alive.store(false, Ordering::SeqCst);
    harness
        .runner
        .replace(|_, _| Some(Err("not found".to_string())));

    let error = harness
        .mirror
        .get_task_surfaces("herdr-task", &SurfaceQuery::default())
        .unwrap_err();

    assert_eq!(
        error,
        PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::HerdrMissing)
    );
    assert_eq!(harness.attacher.call_count(), 0);
}

#[test]
fn keys_go_through_send_text_and_nul_through_ctrl_space() {
    let harness = herdr_harness();

    harness
        .mirror
        .write_to_pane("herdr-task", "w1:p1", "a\0")
        .unwrap();

    let expected = format!(
        "{} && {}",
        herdr_command(&["pane", "send-text", "w1:p1", "a"]),
        herdr_command(&["pane", "send-keys", "w1:p1", "ctrl+space"])
    );
    assert!(harness.commands().contains(&expected));
    assert!(harness.runner.commands_containing("tmux").is_empty());
}

#[test]
fn input_is_not_sent_to_a_pane_outside_the_session() {
    let harness = herdr_harness();

    let error = harness
        .mirror
        .write_to_pane("herdr-task", "w9:p9", "x")
        .unwrap_err();

    assert_eq!(
        error,
        PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::SessionNotRunning)
    );
    assert!(harness.runner.commands_containing("send-text").is_empty());
}

#[test]
fn split_shortcut_waits_for_the_new_pane_and_scroll_shortcut_is_refused() {
    let harness = herdr_harness();
    let herdr = Arc::clone(&harness.herdr);
    harness.runner.respond(move |command, _| {
        command.contains("'pane' 'split'").then(|| {
            herdr
                .panes
                .lock()
                .unwrap()
                .push(herdr_pane("w1:p2", "w1:t1", "", 60, 60));
            Ok(String::new())
        })
    });

    let outcome = harness
        .mirror
        .execute_mobile_terminal_shortcut("herdr-task", "w1:p1", "split-right")
        .unwrap();

    let pane_ids: Vec<String> = outcome.surfaces.tabs[0]
        .panes
        .iter()
        .map(|pane| pane.id.clone())
        .collect();
    assert_eq!(pane_ids, vec!["w1:p1", "w1:p2"]);
    let error = harness
        .mirror
        .execute_mobile_terminal_shortcut("herdr-task", "w1:p1", "scroll-up")
        .unwrap_err();
    assert!(error.to_string().contains("지원하지 않는"));
}

#[test]
fn mobile_wheel_goes_to_the_size_control_connection_as_one_terminal_scroll_line() {
    let harness = herdr_harness();
    let _subscriber = harness.subscribe("herdr-task", "w1:p1");
    let viewport = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();
    viewport.resize(44, 70).unwrap();

    viewport.scroll(&scroll("up", 4, 2)).unwrap();
    viewport.scroll(&scroll("down", 0, 69)).unwrap();

    // 첫 크기는 제어 연결을 여는 명령 인자로 갔으므로 stdin에는 휠만 남는다
    assert_eq!(
        json_lines(&controllers(&harness)[0].stdin_lines()),
        vec![
            json!({ "type": "terminal.scroll", "direction": "up", "lines": 1, "column": 4, "row": 2 }),
            json!({ "type": "terminal.scroll", "direction": "down", "lines": 1, "column": 0, "row": 69 }),
        ]
    );
    assert!(harness.runner.commands_containing("'send-text'").is_empty());
    viewport.release().unwrap();
}

#[test]
fn wheel_before_the_control_connection_and_malformed_wheels_are_dropped() {
    let harness = herdr_harness();
    let _subscriber = harness.subscribe("herdr-task", "w1:p1");
    let viewport = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();

    viewport.scroll(&scroll("up", 0, 0)).unwrap();
    assert!(controllers(&harness).is_empty());
    viewport.resize(44, 70).unwrap();
    viewport.scroll(&scroll("sideways", 0, 0)).unwrap();
    viewport.scroll(&scroll("up", -1, 0)).unwrap();

    assert!(controllers(&harness)[0].stdin_lines().is_empty());
    viewport.release().unwrap();
}

#[test]
fn pane_listing_is_read_once_by_the_subscription_and_once_by_the_size_claim() {
    let harness = herdr_harness();
    let _subscriber = harness.subscribe("herdr-task", "w1:p1");
    let viewport = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();

    viewport.resize(44, 70).unwrap();

    assert_eq!(harness.herdr.list_calls().len(), 2);
    viewport.release().unwrap();
}

#[test]
fn latest_mobile_request_drives_one_controller_and_the_last_release_hands_control_back() {
    let harness = herdr_harness();
    let _subscriber = harness.subscribe("herdr-task", "w1:p1");
    let first = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();
    let second = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();

    first.resize(44, 70).unwrap();
    second.resize(80, 24).unwrap();
    second.release().unwrap();

    let controller_children = controllers(&harness);
    assert_eq!(controller_children.len(), 1);
    let controller = &controller_children[0];
    assert_eq!(
        json_lines(&controller.stdin_lines()),
        vec![
            json!({ "type": "terminal.resize", "cols": 80, "rows": 24 }),
            json!({ "type": "terminal.resize", "cols": 44, "rows": 70 }),
        ]
    );
    assert!(!controller.is_stdin_ended.load(Ordering::SeqCst));
    assert_eq!(
        harness.processes.commands_containing(OBSERVE),
        vec![
            observe_command(120, 38),
            observe_command(44, 70),
            observe_command(80, 24),
            observe_command(44, 70),
        ]
    );
    assert!(harness.processes.commands_containing("takeover").is_empty());
    first.release().unwrap();
    assert_eq!(
        controller.stdin_lines().last().map(String::as_str),
        Some(r#"{"type":"terminal.release"}"#)
    );
    assert!(controller.is_stdin_ended.load(Ordering::SeqCst));
}

#[test]
fn different_panes_of_the_same_tab_do_not_share_size_control_or_release() {
    let harness = herdr_harness();
    harness
        .herdr
        .panes
        .lock()
        .unwrap()
        .push(herdr_pane("w1:p2", "w1:t1", "", 60, 60));
    let first = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();
    let second = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p2")
        .unwrap();

    first.resize(44, 70).unwrap();
    second.resize(80, 24).unwrap();
    assert_eq!(controllers(&harness).len(), 2);
    first.release().unwrap();

    let commands = harness.processes.commands_containing(CONTROL);
    assert_eq!(commands.len(), 2);
    assert!(commands[0].contains("'w1:p1'"));
    assert!(commands[1].contains("'w1:p2'"));
    second.release().unwrap();
}

fn withheld_ack_harness(command_timeout: Duration) -> Harness {
    let harness = herdr_harness_with(crate::mirror::MirrorTimings {
        command_timeout,
        ..test_timings()
    });
    harness
        .processes
        .should_withhold_controller_ack
        .store(true, Ordering::SeqCst);
    harness
}

#[test]
fn missing_size_ack_cleans_up_the_controller_and_ends_the_wait() {
    let harness = withheld_ack_harness(Duration::from_millis(200));
    let viewport = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();

    let error = viewport.resize(44, 70).unwrap_err();

    assert_eq!(error.to_string(), "Herdr 터미널 크기 응답이 없습니다.");
    assert_eq!(controllers(&harness)[0].terminate_count(), 1);
    viewport.release().unwrap();
}

#[test]
fn resize_succeeds_only_after_a_full_frame_of_the_requested_grid() {
    let harness = withheld_ack_harness(Duration::from_secs(3));
    let viewport = Arc::new(
        harness
            .mirror
            .claim_pane_viewport("herdr-task", "w1:p1")
            .unwrap(),
    );
    let has_finished = Arc::new(AtomicBool::new(false));
    let resizing_viewport = Arc::clone(&viewport);
    let finished = Arc::clone(&has_finished);
    let resizing = std::thread::spawn(move || {
        let result = resizing_viewport.resize(44, 70);
        finished.store(true, Ordering::SeqCst);
        result
    });
    wait_until("제어 연결이 열린다", || {
        controllers(&harness).len() == 1
    });
    let controller = controllers(&harness)[0].clone();

    controller.emit(&sized_frame_record(120, 38));
    std::thread::sleep(Duration::from_millis(50));
    assert!(!has_finished.load(Ordering::SeqCst));
    controller.emit(&sized_frame_record(44, 70));

    resizing.join().unwrap().unwrap();
    assert!(has_finished.load(Ordering::SeqCst));
    viewport.release().unwrap();
}

#[test]
fn closed_record_instead_of_the_first_frame_rejects_resize_even_with_exit_zero() {
    let harness = withheld_ack_harness(Duration::from_secs(3));
    let viewport = Arc::new(
        harness
            .mirror
            .claim_pane_viewport("herdr-task", "w1:p1")
            .unwrap(),
    );
    let resizing_viewport = Arc::clone(&viewport);
    let resizing = std::thread::spawn(move || resizing_viewport.resize(44, 70));
    wait_until("제어 연결이 열린다", || {
        controllers(&harness).len() == 1
    });
    let controller = controllers(&harness)[0].clone();

    controller.emit(&format!(
        "{}\n",
        json!({ "type": "terminal.closed", "reason": "already attached" })
    ));
    controller.exit();

    let error = resizing.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("Herdr"));
    viewport.release().unwrap();
}

#[test]
fn lost_controller_also_ends_the_observation_stream_so_a_wrong_grid_is_not_shown() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");
    let viewport = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();
    viewport.resize(44, 70).unwrap();

    controllers(&harness)[0].exit();

    wait_until("관찰 스트림도 끝난다", || subscriber.ended() == 1);
    subscriber.handle.stop();
    viewport.release().unwrap();
}

#[test]
fn history_and_late_subscriber_resends_use_the_mobile_grid_and_drop_stale_frames() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");
    let viewport = harness
        .mirror
        .claim_pane_viewport("herdr-task", "w1:p1")
        .unwrap();
    viewport.resize(44, 70).unwrap();

    observers(&harness)[0].emit(&frame_record("stale desktop grid", false));
    std::thread::sleep(Duration::from_millis(20));
    assert!(
        !subscriber
            .chunks()
            .contains(&"stale desktop grid".to_string())
    );
    set_history_rows(&harness, 5);
    observers(&harness)
        .last()
        .unwrap()
        .emit(&frame_record("new output", false));
    wait_until(
        "이력을 다시 보내고 observe를 새로 띄운다",
        || observers(&harness).len() == 3,
    );
    let _late = harness.subscribe("herdr-task", "w1:p1");
    wait_until("늦은 구독자에게 다시 보낸다", || {
        observers(&harness).len() == 4
    });

    let commands = harness.processes.commands_containing(OBSERVE);
    assert_eq!(commands[1..].to_vec(), vec![observe_command(44, 70); 3]);
    viewport.release().unwrap();
}

#[test]
fn chat_read_returns_only_the_first_full_frame_and_joins_partial_records() {
    let harness = Arc::new(herdr_harness());
    let reading_harness = Arc::clone(&harness);
    let read = std::thread::spawn(move || {
        reading_harness
            .mirror
            .read_herdr_pane_frame_for_chat("herdr-task", "w1:p1")
    });
    wait_until("observe를 띄운다", || harness.processes.count() == 1);
    let observer = harness.processes.child(0);

    observer.emit(&frame_record("delta", false));
    let full = frame_record("\x1b[3;3H› ", true);
    observer.emit(&full[..12]);
    observer.emit(&full[12..]);

    let frame = read.join().unwrap().unwrap();
    assert_eq!(
        (frame.output.as_str(), frame.width, frame.height),
        ("\x1b[3;3H› ", 120, 38)
    );
    assert_eq!(observer.terminate_count(), 1);
}

#[test]
fn a_remote_herdr_observe_is_spawned_to_end_with_its_ssh_channel() {
    let harness = herdr_harness();
    let mut remote_task = task(
        "herdr-task",
        Some(SessionType::Herdr),
        Some("herdr-session"),
    );
    remote_task.ssh_host = Some("remote-b".to_string());
    harness.set_task(remote_task);

    let _subscriber = harness.subscribe("herdr-task", "w1:p1");

    assert_eq!(
        harness.processes.child(0).command,
        build_herdr_remote_observe_pane_command("herdr-session", "w1:p1", 120, 38).unwrap()
    );
}

#[test]
fn history_goes_first_and_split_ndjson_lines_are_joined_into_frames() {
    let harness = herdr_harness();

    let subscriber = harness.subscribe("herdr-task", "w1:p1");

    assert_eq!(subscriber.chunks(), vec![history_redraw(0)]);
    assert_eq!(harness.processes.child(0).command, observe_command(120, 38));
    let line = frame_record("screen", true);
    harness.processes.child(0).emit(&line[..10]);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(subscriber.chunks().len(), 1);
    harness.processes.child(0).emit(&line[10..]);
    wait_until("잘린 줄을 이어 보낸다", || {
        last_chunk(&subscriber).as_deref() == Some("screen")
    });
}

#[test]
fn quiet_output_with_more_history_resends_history_and_restarts_observe() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");
    let first_observer = harness.processes.child(0);

    set_history_rows(&harness, 5);
    first_observer.emit(&frame_record("first", false));
    std::thread::sleep(Duration::from_millis(60));
    first_observer.emit(&frame_record("second", false));
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(harness.processes.count(), 1);
    wait_until("잠잠해지면 observe를 새로 띄운다", || {
        harness.processes.count() == 2
    });
    wait_until("이력을 다시 보낸다", || {
        subscriber.chunks().contains(&history_redraw(5))
    });

    // 멈춘 observe의 뒤늦은 프레임과 종료는 새 스트림을 흔들지 않는다
    first_observer.emit(&frame_record("stale", false));
    first_observer.exit();
    std::thread::sleep(Duration::from_millis(30));
    assert!(!subscriber.chunks().contains(&"stale".to_string()));
    assert_eq!(subscriber.ended(), 0);

    // 새 observe의 첫 전체 프레임이 다시 재시작을 부르면 끝없이 반복된다
    harness
        .processes
        .child(1)
        .emit(&frame_record("repaint", true));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(last_chunk(&subscriber).as_deref(), Some("repaint"));
    assert_eq!(harness.processes.count(), 2);
}

#[test]
fn requested_history_refresh_resends_even_without_new_rows_and_repeated_requests_run_once() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");
    subscriber.chunks.lock().unwrap().clear();

    harness.mirror.refresh_pane_history("herdr-task", "w1:p1");
    harness.mirror.refresh_pane_history("herdr-task", "w1:p1");

    assert_eq!(subscriber.chunks(), vec![history_redraw(0)]);
    assert_eq!(harness.processes.count(), 2);
}

#[test]
fn history_refresh_for_an_unsubscribed_pane_runs_nothing() {
    let harness = herdr_harness();

    harness.mirror.refresh_pane_history("herdr-task", "w1:p1");

    assert!(harness.commands().is_empty());
    assert_eq!(harness.processes.count(), 0);
}

#[test]
fn unchanged_history_does_not_restart_observe() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");

    harness
        .processes
        .child(0)
        .emit(&frame_record("typing", false));
    std::thread::sleep(Duration::from_millis(400));

    assert_eq!(harness.processes.count(), 1);
    assert_eq!(last_chunk(&subscriber).as_deref(), Some("typing"));
}

#[test]
fn late_subscriber_gets_the_screen_through_a_new_observe() {
    let harness = herdr_harness();
    let first = harness.subscribe("herdr-task", "w1:p1");
    assert_eq!(harness.processes.count(), 1);

    let second = harness.subscribe("herdr-task", "w1:p1");
    assert_eq!(harness.processes.count(), 2);
    harness
        .processes
        .child(1)
        .emit(&frame_record("screen", true));

    wait_until("두 구독자가 화면을 받는다", || {
        last_chunk(&first).as_deref() == Some("screen")
            && last_chunk(&second).as_deref() == Some("screen")
    });
}

#[test]
fn observe_ending_by_itself_tells_the_subscriber() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");

    harness.processes.child(0).exit();

    wait_until("끝을 알린다", || subscriber.ended() == 1);
}

#[test]
fn failed_history_check_ends_the_stream_and_stops_observe_and_later_checks() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");
    let observer = harness.processes.child(0);
    let extent_reads_before_failure = harness.herdr.extent_calls.load(Ordering::SeqCst);
    *harness.herdr.extent_failure.lock().unwrap() = Some("ssh disconnected".to_string());

    observer.emit(&frame_record("output", false));
    wait_until("끝을 알린다", || subscriber.ended() == 1);
    assert_eq!(observer.terminate_count(), 1);

    subscriber.handle.stop();
    observer.emit(&frame_record("late output", false));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        harness.herdr.extent_calls.load(Ordering::SeqCst),
        extent_reads_before_failure + 1
    );
}

#[test]
fn history_check_scheduled_before_observe_ended_does_not_restart_it() {
    let harness = herdr_harness();
    let subscriber = harness.subscribe("herdr-task", "w1:p1");

    set_history_rows(&harness, 5);
    harness
        .processes
        .child(0)
        .emit(&frame_record("output", false));
    harness.processes.child(0).exit();
    wait_until("끝을 알린다", || subscriber.ended() == 1);
    subscriber.handle.stop();
    std::thread::sleep(Duration::from_millis(400));

    assert_eq!(harness.processes.count(), 1);
}
