//! pane CLI의 연결 신호(hook·세션 기록 파일)를 받은 채팅 화면과 층. 신호마다 그 pane을 바로 한 번 다시 확인하고,
//! 자동으로 여는 채팅은 여전히 CLI와 대화가 확인된 뒤에만 연결 화면 없이 연다.

mod task_detail_pane_chat_support;

use gpui_kit::{Entity, TestAppContext, VisualTestContext, gpui};
use kanvibe_app::views::task_detail::pane_chat::{PaneChatLayer, PaneChatView};
use serde_json::{Value, json};
use task_detail_pane_chat_support::*;

const LAYOUT: &str = "paneChat.getPaneChatLayout";
const PROVIDER: &str = "paneChat.getPaneChatAgentProvider";
const SNAPSHOT: &str = "paneChat.getPaneChatSnapshot";
const MODES: &str = "paneChat.getPaneChatModes";

fn single_pane() -> Value {
    json!({ "activePaneId": "%1", "panes": [
        { "id": "%1", "tabId": "@1", "bounds": { "left": 0, "top": 0, "width": 80, "height": 24 } }] })
}

/// 아직 대화를 잇지 못한 Claude pane(첫 화면 전 등)
fn unconnected() -> Value {
    let mut value = snapshot("process-a");
    value["provider"] = json!("claude");
    value["canSend"] = json!(false);
    value["targetToken"] = Value::Null;
    value["reason"] = json!("exact-session-unavailable");
    value["transcript"] = json!({ "status": "unavailable", "reason": "exact-session-unavailable",
        "sessionId": null, "nextCursor": null, "messages": [] });
    value
}

/// 기록 파일이 아직 없는 새 대화. 메시지 없이 보낼 수 있다
fn new_conversation(token: &str, session_id: &str) -> Value {
    let mut value = snapshot(token);
    value["provider"] = json!("claude");
    value["transcript"]["sessionId"] = json!(session_id);
    value["transcript"]["messages"] = json!([]);
    value
}

/// 층의 pane(`%1`)에서 읽은 상태
fn on_layer_pane(mut value: Value) -> Value {
    value["paneId"] = json!("%1");
    value
}

fn layer_backend(is_default_chat: bool) -> FakeBackend {
    let backend = FakeBackend::new();
    backend.respond(LAYOUT, single_pane());
    backend.respond(PROVIDER, Value::Null);
    backend.respond(SNAPSHOT, unconnected());
    backend.respond(MODES, json!({}));
    backend.respond("paneChat.setPaneChatMode", Value::Null);
    backend.respond_with("appSettings.getAppSetting", move |arguments| {
        let is_default_key = arguments[0] == json!("pane_chat_default_enabled");
        Ok(match is_default_key && is_default_chat {
            true => json!("true"),
            false => Value::Null,
        })
    });
    backend
}

fn opened_view(
    layer: &Entity<PaneChatLayer>,
    cx: &mut VisualTestContext,
) -> Option<Entity<PaneChatView>> {
    layer.read_with(cx, |layer, _| layer.visible_chat("%1"))
}

fn is_input_editable(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.is_input_editable())
}

#[gpui::test]
fn cli_signal_opens_default_chat_at_once_with_the_confirmed_conversation(cx: &mut TestAppContext) {
    init(cx);
    let backend = layer_backend(true);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(opened_view(&layer, cx).is_none());
    let probes = backend.call_count(PROVIDER);

    // CLI가 시작해 새 대화 ID를 남겼다. 다음 배치 조회(1초)를 기다리지 않고 확인한다
    backend.respond(PROVIDER, json!("claude"));
    backend.once(
        SNAPSHOT,
        Ok(on_layer_pane(new_conversation("token-new", "session-new"))),
    );
    let view_read = backend.hold(SNAPSHOT);
    backend.signal_connection("task-1");
    settle(cx);
    assert_eq!(backend.call_count(PROVIDER), probes + 1);
    let view = opened_view(&layer, cx).expect("확인된 pane은 바로 열린다");
    assert!(!layer.read_with(cx, |layer, _| layer.is_loading_shown()));

    // 화면의 첫 조회가 끝나기 전에도, 층이 방금 확인한 그 대화로 보낼 수 있다
    let shown = view
        .read_with(cx, |view, _| view.snapshot().cloned())
        .unwrap();
    assert_eq!(shown.target_token.as_deref(), Some("token-new"));
    assert_eq!(shown.transcript.session_id.as_deref(), Some("session-new"));
    assert!(is_input_editable(&view, cx));
    view_read
        .send(Ok(on_layer_pane(new_conversation(
            "token-new",
            "session-new",
        ))))
        .unwrap();
    settle(cx);
    assert!(is_input_editable(&view, cx));
}

#[gpui::test]
fn signal_without_a_connectable_conversation_keeps_the_terminal_and_no_connecting_screen(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = layer_backend(true);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    backend.respond(PROVIDER, json!("claude"));
    backend.signal_connection("task-1");
    settle(cx);
    assert!(opened_view(&layer, cx).is_none());
    assert!(!layer.read_with(cx, |layer, _| layer.is_loading_shown()));
}

#[gpui::test]
fn signal_during_a_probe_rechecks_once_after_it_and_other_tasks_are_ignored(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = layer_backend(true);
    let probe = backend.hold(PROVIDER);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    let probes = backend.call_count(PROVIDER);
    backend.signal_connection("task-2");
    backend.signal_connection("task-1");
    backend.signal_connection("task-1");
    settle(cx);
    assert_eq!(
        backend.call_count(PROVIDER),
        probes,
        "확인 중에는 겹쳐 확인하지 않는다"
    );

    // 신호 전에 시작한 확인은 CLI를 못 봤다. 그 신호 몫으로 한 번만 더 확인해 연다
    backend.respond(PROVIDER, json!("claude"));
    backend.respond(
        SNAPSHOT,
        on_layer_pane(new_conversation("token-new", "session-new")),
    );
    probe.send(Ok(Value::Null)).unwrap();
    settle(cx);
    assert_eq!(backend.call_count(PROVIDER), probes + 1);
    assert!(opened_view(&layer, cx).is_some());
}

#[gpui::test]
fn open_chat_rereads_on_each_signal_without_waiting_for_the_poll(cx: &mut TestAppContext) {
    init(cx);
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, unconnected());
    let (view, cx) = open_view(cx, &backend, Some("%3"), None);
    let reads = backend.call_count(SNAPSHOT);
    assert!(!is_input_editable(&view, cx));

    backend.respond(SNAPSHOT, new_conversation("token-a", "session-a"));
    backend.signal_connection("task-1");
    settle(cx);
    assert_eq!(backend.call_count(SNAPSHOT), reads + 1);
    assert!(is_input_editable(&view, cx));

    // `/clear`로 대화가 바뀌면 이전 대화의 전송 대상을 쓰지 않고 새 대상으로 바꾼다
    backend.respond(SNAPSHOT, new_conversation("token-b", "session-b"));
    backend.signal_connection("task-1");
    settle(cx);
    let shown = view
        .read_with(cx, |view, _| view.snapshot().cloned())
        .unwrap();
    assert_eq!(shown.target_token.as_deref(), Some("token-b"));
    assert_eq!(shown.transcript.session_id.as_deref(), Some("session-b"));
    assert!(shown.transcript.messages.is_empty());
}

#[gpui::test]
fn signals_during_a_read_add_exactly_one_follow_up_read(cx: &mut TestAppContext) {
    init(cx);
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, unconnected());
    let (view, cx) = open_view(cx, &backend, Some("%3"), None);
    let in_flight = backend.hold(SNAPSHOT);
    backend.signal_connection("task-1");
    settle(cx);
    let reads = backend.call_count(SNAPSHOT);
    for _ in 0..3 {
        backend.signal_connection("task-1");
    }
    settle(cx);
    assert_eq!(backend.call_count(SNAPSHOT), reads);

    backend.respond(SNAPSHOT, new_conversation("token-a", "session-a"));
    in_flight.send(Ok(unconnected())).unwrap();
    settle(cx);
    assert_eq!(backend.call_count(SNAPSHOT), reads + 1);
    assert!(is_input_editable(&view, cx));
    settle(cx);
    assert_eq!(
        backend.call_count(SNAPSHOT),
        reads + 1,
        "신호가 없으면 주기까지 다시 읽지 않는다"
    );
}

#[gpui::test]
fn hidden_chat_ignores_signals(cx: &mut TestAppContext) {
    init(cx);
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, unconnected());
    let (view, cx) = open_view(cx, &backend, Some("%3"), None);
    view.update_in(cx, |view, window, cx| view.set_visible(false, window, cx));
    settle(cx);
    let reads = backend.call_count(SNAPSHOT);
    backend.signal_connection("task-1");
    settle(cx);
    assert_eq!(backend.call_count(SNAPSHOT), reads);
    assert_eq!(backend.connection_watch_count("task-1"), 1);
}
