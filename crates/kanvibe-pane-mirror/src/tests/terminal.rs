//! terminal 세션 미러. 멀티플렉서 없이 KanVibe가 가진 PTY에 직접 붙는다.
//! 지금 화면은 레지스트리의 VT 화면 직렬화(`kanvibe_terminal::screen`)에서 오며 xterm 직렬화와 바이트 단위로 같지 않다

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use kanvibe_domain::task::SessionType;
use kanvibe_terminal::terminal_bridge::CallbackTerminalClient;
use kanvibe_terminal::terminal_registry::TerminalClient;
use serde_json::json;

use super::fakes::*;
use super::*;
use crate::error::{PaneMirrorError, SurfaceUnavailableReason};
use crate::surfaces::SurfaceQuery;

const FIRST_TAB_ID: &str = "term-task-1";

fn terminal_harness() -> Harness {
    let harness = Harness::new();
    harness.set_task(task(
        "term-task",
        Some(SessionType::Terminal),
        Some("kanvibe-term"),
    ));
    harness
}

// parity: TS-main-mobileBridgeService-4c2066
#[test]
fn each_tab_becomes_one_pane_whose_id_is_the_tab_id() {
    let harness = terminal_harness();
    let second_tab = harness.registry.create_local_terminal_tab("term-task");

    let surfaces = harness
        .mirror
        .get_task_surfaces("term-task", &SurfaceQuery::default())
        .unwrap();

    assert_eq!(surfaces.session_type, SessionType::Terminal);
    let pane_ids: Vec<Vec<String>> = surfaces
        .tabs
        .iter()
        .map(|tab| tab.panes.iter().map(|pane| pane.id.clone()).collect())
        .collect();
    assert_eq!(
        pane_ids,
        vec![vec![FIRST_TAB_ID.to_string()], vec![second_tab.id]]
    );
    assert_eq!(
        (
            surfaces.tabs[0].panes[0].width,
            surfaces.tabs[0].panes[0].height
        ),
        (120, 30)
    );
}

// parity: TS-main-mobileBridgeService-55ca46
#[test]
fn missing_pty_is_created_and_an_existing_one_sends_its_screen_before_following_output() {
    let harness = terminal_harness();
    let first = harness.subscribe("term-task", FIRST_TAB_ID);
    assert_eq!(harness.ptys.count(), 1);

    harness.ptys.pty(0).emit_output("hello");
    let later = harness.subscribe("term-task", FIRST_TAB_ID);
    harness.ptys.pty(0).emit_output(" world");

    assert_eq!(harness.ptys.count(), 1);
    assert_eq!(first.chunks(), vec!["hello", " world"]);
    let later_chunks = later.chunks();
    assert!(later_chunks[0].contains("hello"));
    assert_eq!(later_chunks[1..].to_vec(), vec![" world"]);
}

// parity: TS-main-mobileBridgeService-9e6ecd
#[test]
fn output_while_the_screen_is_prepared_follows_the_screen_exactly_once() {
    let harness = terminal_harness();
    let _subscriber = harness.subscribe("term-task", FIRST_TAB_ID);
    harness.ptys.pty(0).emit_output("before");
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&chunks);
    let client = Arc::new(CallbackTerminalClient::new(move |chunk| {
        received.lock().unwrap().push(chunk.to_string())
    }));
    client.hold_output();

    let (attachment, screen_until_now) = harness
        .registry
        .attach_client_to_terminal_tab("term-task", FIRST_TAB_ID, client.clone())
        .unwrap();
    harness.ptys.pty(0).emit_output("during");
    client.release_held_output(&screen_until_now);
    harness.ptys.pty(0).emit_output("after");

    let chunks = chunks.lock().unwrap().clone();
    assert!(chunks[0].contains("before"));
    assert!(!chunks[0].contains("during"));
    assert_eq!(chunks[1..].to_vec(), vec!["during", "after"]);
    harness.registry.client_closed(&attachment);
}

// parity: TS-main-mobileBridgeService-a92b21
#[test]
fn pty_creation_failure_rejects_with_the_reason_without_announcing_an_end() {
    let harness = terminal_harness();
    harness
        .attacher
        .set_override(|_, _, client: Arc<dyn TerminalClient>| {
            client.close(None);
            Err("터미널 프로세스 생성 실패".to_string())
        });

    let end_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let ended = Arc::clone(&end_count);

    let error = harness
        .mirror
        .subscribe_to_pane(
            "term-task",
            FIRST_TAB_ID,
            Arc::new(|_| {}),
            Arc::new(move || {
                ended.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .err()
        .unwrap();

    assert_eq!(error.to_string(), "터미널 프로세스 생성 실패");
    assert_eq!(end_count.load(Ordering::SeqCst), 0);
}

// parity: TS-main-mobileBridgeService-10314d
#[test]
fn input_goes_to_the_pty_as_is_and_resize_shaped_input_does_not_resize() {
    let harness = terminal_harness();
    let _subscriber = harness.subscribe("term-task", FIRST_TAB_ID);
    let resize_like_input = format!("\x01{}", json!({ "type": "resize", "cols": 10, "rows": 5 }));

    harness
        .mirror
        .write_to_pane("term-task", FIRST_TAB_ID, "ls\r")
        .unwrap();
    harness
        .mirror
        .write_to_pane("term-task", FIRST_TAB_ID, &resize_like_input)
        .unwrap();

    assert_eq!(
        harness.ptys.pty(0).writes(),
        vec!["ls\r".to_string(), resize_like_input]
    );
    assert!(harness.ptys.pty(0).resizes().is_empty());
}

// parity: TS-main-mobileBridgeService-6d8976
#[test]
fn tabs_of_another_task_cannot_be_subscribed_or_written() {
    let harness = terminal_harness();
    let session_not_running =
        PaneMirrorError::SurfaceUnavailable(SurfaceUnavailableReason::SessionNotRunning);

    assert_eq!(
        harness
            .try_subscribe("term-task", "other-task-1")
            .err()
            .unwrap(),
        session_not_running
    );
    assert_eq!(
        harness
            .mirror
            .write_to_pane("term-task", "other-task-1", "ls")
            .unwrap_err(),
        session_not_running
    );
    assert_eq!(harness.ptys.count(), 0);
}

// parity: TS-main-mobileBridgeService-4d7386
#[test]
fn end_is_announced_only_when_the_shell_exits_and_the_pty_outlives_subscribers() {
    let harness = terminal_harness();
    let subscriber = harness.subscribe("term-task", FIRST_TAB_ID);

    subscriber.handle.stop();
    assert_eq!(subscriber.ended(), 0);
    assert_eq!(harness.ptys.pty(0).kill_count.load(Ordering::SeqCst), 0);

    let resubscribed = harness.subscribe("term-task", FIRST_TAB_ID);
    harness.ptys.pty(0).emit_exit();

    wait_until("셸이 끝나면 알린다", || resubscribed.ended() == 1);
    assert_eq!(subscriber.ended(), 0);
}
