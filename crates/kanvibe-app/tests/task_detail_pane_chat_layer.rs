//! 터미널 위 pane 채팅 층(TS `TerminalPaneChatLayer.test.tsx`). pane 배치·CLI 확인·저장된 모드를 가짜 서비스로 정하고,
//! 어느 pane이 채팅으로 보이는지, 모드 저장 호출, 터미널로 돌아가기를 확인한다.

mod task_detail_pane_chat_support;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext, gpui, px};
use kanvibe_app::views::task_detail::pane_chat::{PaneArea, PaneChatLayer, PaneChatLayerEvent};
use serde_json::{Value, json};
use task_detail_pane_chat_support::*;

const LAYOUT: &str = "paneChat.getPaneChatLayout";
const PROVIDER: &str = "paneChat.getPaneChatAgentProvider";
const SNAPSHOT: &str = "paneChat.getPaneChatSnapshot";
const FOCUS: &str = "paneChat.focusPaneChatTerminal";
const MODES: &str = "paneChat.getPaneChatModes";
const SET_MODE: &str = "paneChat.setPaneChatMode";

fn split(active: &str) -> Value {
    json!({ "activePaneId": active, "panes": [
        { "id": "%1", "tabId": "@1", "bounds": { "left": 0, "top": 0, "width": 39, "height": 24 } },
        { "id": "%2", "tabId": "@1", "bounds": { "left": 40, "top": 0, "width": 40, "height": 24 } }] })
}

/// 작업별로 저장된 pane 모드(TS `savedModes`). 층을 다시 열어도 같은 저장소를 본다
type SavedModes = Arc<Mutex<HashMap<String, serde_json::Map<String, Value>>>>;

fn chat_state(can_send: bool, reason: Option<&str>) -> Value {
    let mut value = snapshot("process-a");
    value["canSend"] = json!(can_send);
    value["transcript"]["reason"] = json!(reason);
    value
}

fn backend(saved: &SavedModes, is_default_chat: bool) -> FakeBackend {
    let backend = FakeBackend::new();
    backend.respond(LAYOUT, split("%1"));
    backend.respond(PROVIDER, json!("claude"));
    backend.respond(SNAPSHOT, chat_state(true, None));
    backend.respond(FOCUS, success());
    let read = saved.clone();
    backend.respond_with(MODES, move |arguments| {
        let task = arguments[0].as_str().unwrap_or_default();
        Ok(Value::Object(
            read.lock().unwrap().get(task).cloned().unwrap_or_default(),
        ))
    });
    let write = saved.clone();
    backend.respond_with(SET_MODE, move |arguments| {
        let task = arguments[0].as_str().unwrap_or_default().to_string();
        let pane = arguments[1].as_str().unwrap_or_default().to_string();
        write
            .lock()
            .unwrap()
            .entry(task)
            .or_default()
            .insert(pane, arguments[2].clone());
        Ok(Value::Null)
    });
    backend.respond_with("appSettings.getAppSetting", move |arguments| {
        let is_default_key = arguments[0] == json!("pane_chat_default_enabled");
        Ok(if is_default_key && is_default_chat {
            json!("true")
        } else {
            Value::Null
        })
    });
    backend
}

fn saved_modes() -> SavedModes {
    Arc::new(Mutex::new(HashMap::new()))
}

fn is_visible(layer: &Entity<PaneChatLayer>, cx: &mut VisualTestContext, pane: &str) -> bool {
    layer.read_with(cx, |layer, _| layer.visible_chat(pane).is_some())
}

fn is_opened(layer: &Entity<PaneChatLayer>, cx: &mut VisualTestContext, pane: &str) -> bool {
    layer.read_with(cx, |layer, _| layer.chat_view(pane).is_some())
}

fn is_loading(layer: &Entity<PaneChatLayer>, cx: &mut VisualTestContext) -> bool {
    layer.read_with(cx, |layer, _| layer.is_loading_shown())
}

fn toggle(layer: &Entity<PaneChatLayer>, cx: &mut VisualTestContext, pane: Option<&str>) {
    let pane = pane.map(str::to_string);
    layer.update_in(cx, |layer, window, cx| layer.toggle(pane, window, cx));
    settle(cx);
}

fn close(layer: &Entity<PaneChatLayer>, cx: &mut VisualTestContext, pane: &str) {
    let pane = pane.to_string();
    layer.update_in(cx, |layer, window, cx| layer.close_pane(pane, window, cx));
    settle(cx);
}

fn press_terminal(layer: &Entity<PaneChatLayer>, cx: &mut VisualTestContext) {
    layer.update_in(cx, |layer, window, cx| {
        layer.handle_terminal_pointer_down(window, cx)
    });
    settle(cx);
}

fn mode_calls(backend: &FakeBackend) -> Vec<Vec<Value>> {
    backend.calls(SET_MODE)
}

fn advance_poll(cx: &mut VisualTestContext) {
    advance(cx, 1_100);
}

/// 층이 다시 그려 달라고 알린(notify) 횟수. 돌려받은 구독을 쥐고 있는 동안 센다
fn count_notifications(
    layer: &Entity<PaneChatLayer>,
    cx: &mut VisualTestContext,
) -> (Rc<Cell<usize>>, gpui_kit::Subscription) {
    let notifications = Rc::new(Cell::new(0));
    let counter = notifications.clone();
    let subscription =
        cx.update(|_, cx| cx.observe(layer, move |_, _| counter.set(counter.get() + 1)));
    (notifications, subscription)
}

#[gpui::test]
fn default_chat_waits_for_layout_and_cli_without_a_connecting_screen(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    let layout = backend.hold(LAYOUT);
    let provider = backend.hold(PROVIDER);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(!is_loading(&layer, cx));
    layout.send(Ok(split("%1"))).unwrap();
    settle(cx);
    assert!(!is_loading(&layer, cx) && !is_opened(&layer, cx, "%1"));
    assert_eq!(
        backend.calls(PROVIDER)[0],
        vec![json!("task-1"), json!("%1")]
    );
    provider.send(Ok(json!("claude"))).unwrap();
    settle(cx);
    assert!(is_visible(&layer, cx, "%1"));
    assert!(mode_calls(&backend).contains(&vec![json!("task-1"), json!("%1"), json!(true)]));
}

#[gpui::test]
fn default_chat_skips_panes_without_a_cli_and_saves_nothing(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    backend.respond(PROVIDER, Value::Null);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert_eq!(
        backend.calls(PROVIDER)[0],
        vec![json!("task-1"), json!("%1")]
    );
    assert!(!is_opened(&layer, cx, "%1") && !is_loading(&layer, cx));
    assert!(mode_calls(&backend).is_empty());
}

#[gpui::test]
fn default_chat_opens_once_a_cli_starts_later(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    backend.respond(PROVIDER, Value::Null);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(!is_opened(&layer, cx, "%1"));
    backend.respond(PROVIDER, json!("claude"));
    advance_poll(cx);
    assert!(is_visible(&layer, cx, "%1"));
    assert!(mode_calls(&backend).contains(&vec![json!("task-1"), json!("%1"), json!(true)]));
}

#[gpui::test]
fn saved_chat_pane_waits_as_terminal_until_its_cli_runs(cx: &mut TestAppContext) {
    init(cx);
    let saved = saved_modes();
    saved.lock().unwrap().insert(
        "task-1".into(),
        json!({ "%1": true }).as_object().unwrap().clone(),
    );
    let backend = backend(&saved, false);
    backend.respond(PROVIDER, Value::Null);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(!is_opened(&layer, cx, "%1") && !is_loading(&layer, cx));
    backend.respond(PROVIDER, json!("codex"));
    advance_poll(cx);
    assert!(is_visible(&layer, cx, "%1"));
    assert!(mode_calls(&backend).is_empty());
}

#[gpui::test]
fn cli_first_screen_without_a_conversation_is_not_covered_until_it_can_connect(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    backend.respond(
        SNAPSHOT,
        chat_state(false, Some("exact-session-unavailable")),
    );
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    advance_poll(cx);
    assert_eq!(
        backend.calls(SNAPSHOT)[0],
        vec![json!("task-1"), json!("%1")]
    );
    assert!(!is_opened(&layer, cx, "%1") && !is_loading(&layer, cx));
    assert!(mode_calls(&backend).is_empty());
    backend.respond(SNAPSHOT, chat_state(true, None));
    advance_poll(cx);
    assert!(is_visible(&layer, cx, "%1"));
    assert!(mode_calls(&backend).contains(&vec![json!("task-1"), json!("%1"), json!(true)]));
}

#[gpui::test]
fn codex_waiting_for_reveal_is_opened_in_chat(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    backend.respond(PROVIDER, json!("codex"));
    backend.respond(
        SNAPSHOT,
        chat_state(false, Some("codex-thread-unconfirmed")),
    );
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(is_visible(&layer, cx, "%1"));
}

#[gpui::test]
fn pane_without_a_cli_is_not_read_further(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    backend.respond(PROVIDER, Value::Null);
    let (_layer, _cx) = open_layer(cx, &backend, "task-1", true);
    assert_eq!(
        backend.calls(PROVIDER)[0],
        vec![json!("task-1"), json!("%1")]
    );
    assert_eq!(backend.call_count(SNAPSHOT), 0);
}

#[gpui::test]
fn explicit_toggle_opens_chat_even_without_a_cli(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    backend.respond(PROVIDER, Value::Null);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    assert!(is_visible(&layer, cx, "%1"));
}

#[gpui::test]
fn reshown_tab_shows_the_connecting_screen_only_while_rereading_the_layout(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    set_layer_visible(&layer, cx, false);
    let layout = backend.hold(LAYOUT);
    set_layer_visible(&layer, cx, true);
    assert!(is_loading(&layer, cx));
    layout.send(Ok(split("%1"))).unwrap();
    settle(cx);
    assert!(!is_loading(&layer, cx));
    assert!(is_visible(&layer, cx, "%1"));
}

#[gpui::test]
fn pressing_the_terminal_keeps_an_open_chat_without_a_connecting_screen(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    backend.hang(LAYOUT);
    press_terminal(&layer, cx);
    assert!(!is_loading(&layer, cx));
    assert!(is_visible(&layer, cx, "%1"));
}

#[gpui::test]
fn default_chat_applies_to_a_newly_added_pane(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(is_visible(&layer, cx, "%1"));
    backend.respond(LAYOUT, split("%2"));
    advance_poll(cx);
    assert!(is_visible(&layer, cx, "%2"));
}

#[gpui::test]
fn closing_a_default_chat_pane_wins_over_the_default_setting(cx: &mut TestAppContext) {
    init(cx);
    let saved = saved_modes();
    let backend = backend(&saved, true);
    {
        let (layer, window_cx) = open_layer(cx, &backend, "task-1", true);
        assert!(is_visible(&layer, window_cx, "%1"));
        close(&layer, window_cx, "%1");
    }
    let (layer, window_cx) = open_layer(cx, &backend, "task-1", true);
    assert!(!is_opened(&layer, window_cx, "%1"));
}

#[gpui::test]
fn reopened_screen_restores_open_chat_but_not_closed_chat(cx: &mut TestAppContext) {
    init(cx);
    let saved = saved_modes();
    let backend = backend(&saved, false);
    {
        let (layer, window_cx) = open_layer(cx, &backend, "task-1", true);
        toggle(&layer, window_cx, None);
    }
    {
        let (layer, window_cx) = open_layer(cx, &backend, "task-1", true);
        assert!(is_visible(&layer, window_cx, "%1"));
        close(&layer, window_cx, "%1");
    }
    let (layer, window_cx) = open_layer(cx, &backend, "task-1", true);
    assert!(!is_opened(&layer, window_cx, "%1"));
}

#[gpui::test]
fn app_saved_chat_mode_opens_chat_without_window_storage(cx: &mut TestAppContext) {
    init(cx);
    let saved = saved_modes();
    saved.lock().unwrap().insert(
        "task-1".into(),
        json!({ "%1": true }).as_object().unwrap().clone(),
    );
    let backend = backend(&saved, false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(is_visible(&layer, cx, "%1"));
}

#[gpui::test]
fn chat_and_terminal_modes_are_saved_per_pane(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    assert_eq!(
        mode_calls(&backend),
        vec![vec![json!("task-1"), json!("%1"), json!(true)]]
    );
    close(&layer, cx, "%1");
    assert_eq!(
        mode_calls(&backend).last().unwrap(),
        &vec![json!("task-1"), json!("%1"), json!(false)]
    );
    assert_eq!(mode_calls(&backend).len(), 2);
}

#[gpui::test]
fn pane_left_as_terminal_stays_terminal_with_default_chat_on(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), true);
    let modes = backend.hold(MODES);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert!(!is_opened(&layer, cx, "%1"));
    modes.send(Ok(json!({ "%1": false }))).unwrap();
    settle(cx);
    assert!(!is_opened(&layer, cx, "%1"));
    assert!(mode_calls(&backend).is_empty());
}

#[gpui::test]
fn unreadable_saved_modes_open_as_terminal_and_toggle_still_works(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    backend.fail(MODES, "settings unavailable");
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert_eq!(
        layer.read_with(cx, |layer, _| layer.alert_message_key()),
        None
    );
    assert!(!is_opened(&layer, cx, "%1"));
    toggle(&layer, cx, None);
    assert!(is_visible(&layer, cx, "%1"));
    assert!(mode_calls(&backend).contains(&vec![json!("task-1"), json!("%1"), json!(true)]));
}

#[gpui::test]
fn restored_active_pane_takes_focus_whichever_read_finishes_first(cx: &mut TestAppContext) {
    init(cx);
    for is_layout_first in [true, false] {
        let saved = saved_modes();
        let backend = backend(&saved, false);
        let modes = backend.hold(MODES);
        let layout = backend.hold(LAYOUT);
        let (layer, window_cx) = open_layer(cx, &backend, "task-1", true);
        let finish_modes = || modes.send(Ok(json!({ "%1": true }))).unwrap();
        let finish_layout = || layout.send(Ok(split("%1"))).unwrap();
        if is_layout_first {
            finish_layout();
            settle(window_cx);
            finish_modes();
        } else {
            finish_modes();
            settle(window_cx);
            finish_layout();
        }
        settle(window_cx);
        assert!(is_visible(&layer, window_cx, "%1"));
        assert!(layer.read_with(window_cx, |layer, _| layer.is_chat_focused()));
        let view = layer
            .read_with(window_cx, |layer, _| layer.chat_view("%1"))
            .unwrap();
        assert!(window_cx.update(
            |window, cx| view.focus_handle(cx).contains_focused(window, cx)
                || view.read(cx).is_input_focused(window, cx)
        ));
    }
}

#[gpui::test]
fn another_task_with_the_same_pane_id_does_not_restore_chat(cx: &mut TestAppContext) {
    init(cx);
    let saved = saved_modes();
    let backend = backend(&saved, false);
    {
        let (layer, window_cx) = open_layer(cx, &backend, "task-1", true);
        toggle(&layer, window_cx, None);
    }
    let (layer, window_cx) = open_layer(cx, &backend, "task-2", true);
    assert!(!is_opened(&layer, window_cx, "%1"));
}

fn width_of(
    layer: &Entity<PaneChatLayer>,
    cx: &mut VisualTestContext,
    pane: &str,
) -> Option<(f32, f32)> {
    match layer.read_with(cx, |layer, _| layer.area_of(pane)) {
        Some(PaneArea::Cells { left, width, .. }) => Some((f32::from(left), f32::from(width))),
        _ => None,
    }
}

#[gpui::test]
fn chat_covers_only_the_focused_split(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    assert_eq!(width_of(&layer, cx, "%1"), Some((0.0, 390.0)));
    assert!(!is_opened(&layer, cx, "%2"));
    assert_eq!(backend.calls(LAYOUT)[0], vec![json!("task-1"), json!("@1")]);
}

#[gpui::test]
fn each_pane_toggles_independently_and_keeps_its_draft(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    let first = layer
        .read_with(cx, |layer, _| layer.chat_view("%1"))
        .unwrap();
    first.update_in(cx, |view, window, cx| {
        view.set_composer_text("Draft for Codex", 15, window, cx)
    });
    close(&layer, cx, "%1");
    assert!(is_opened(&layer, cx, "%1") && !is_visible(&layer, cx, "%1"));
    backend.respond(LAYOUT, split("%2"));
    press_terminal(&layer, cx);
    toggle(&layer, cx, None);
    assert!(is_visible(&layer, cx, "%2"));
    assert_eq!(
        first.read_with(cx, |view, _| view.draft_text()),
        "Draft for Codex"
    );
    backend.respond(LAYOUT, split("%1"));
    press_terminal(&layer, cx);
    toggle(&layer, cx, None);
    assert!(is_visible(&layer, cx, "%1") && is_visible(&layer, cx, "%2"));
}

#[gpui::test]
fn hidden_terminal_tab_does_not_act(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", false);
    toggle(&layer, cx, None);
    assert_eq!(backend.call_count(LAYOUT), 0);
    assert!(!is_opened(&layer, cx, "%1"));
}

#[gpui::test]
fn cover_hides_when_its_pane_is_no_longer_shown(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    backend.respond(
        LAYOUT,
        json!({ "activePaneId": "%3", "panes": [{ "id": "%3", "tabId": "@2", "bounds": null }] }),
    );
    advance(cx, 1_600);
    assert!(is_opened(&layer, cx, "%1") && !is_visible(&layer, cx, "%1"));
}

#[gpui::test]
fn toggle_result_after_the_terminal_is_hidden_is_ignored(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    let layout = backend.hold(LAYOUT);
    toggle(&layer, cx, None);
    set_layer_visible(&layer, cx, false);
    let _ = layout.send(Ok(split("%1")));
    settle(cx);
    assert!(!is_opened(&layer, cx, "%1"));
}

#[gpui::test]
fn runtime_focus_is_restored_before_releasing_the_pane(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    let focused = backend.hold(FOCUS);
    close(&layer, cx, "%1");
    assert_eq!(
        backend.last_call(FOCUS).unwrap(),
        vec![json!("task-1"), json!("%1")]
    );
    assert!(is_visible(&layer, cx, "%1"));
    assert!(layer.read_with(cx, |layer, _| layer.is_chat_focused()));
    focused.send(Ok(success())).unwrap();
    settle(cx);
    assert!(!is_visible(&layer, cx, "%1"));
    assert!(!layer.read_with(cx, |layer, _| layer.is_chat_focused()));
}

#[gpui::test]
fn chat_stays_when_restoring_the_runtime_pane_fails(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    backend.fail(FOCUS, "pane disappeared");
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    close(&layer, cx, "%1");
    assert!(is_visible(&layer, cx, "%1"));
    assert_eq!(
        layer.read_with(cx, |layer, _| layer.alert_message_key()),
        Some("returnError")
    );
    assert!(layer.read_with(cx, |layer, _| layer.is_chat_focused()));
    assert_eq!(
        t("returnError"),
        translator().translate("paneChat.returnError")
    );
}

#[gpui::test]
fn toggle_sits_on_its_pane_and_opens_chat_over_that_pane_only(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    assert_eq!(
        layer
            .read_with(cx, |layer, _| layer.toggle_pane().map(str::to_string))
            .as_deref(),
        Some("%1")
    );
    assert_eq!(width_of(&layer, cx, "%1"), Some((0.0, 390.0)));
    toggle(&layer, cx, Some("%1"));
    assert!(is_visible(&layer, cx, "%1"));
    assert!(!is_opened(&layer, cx, "%2"));
    assert_eq!(px(390.0), px(width_of(&layer, cx, "%1").unwrap().1));
}

#[gpui::test]
fn clicked_toggle_stays_with_its_pane_when_focus_moves_before_the_fresh_read(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    backend.respond(LAYOUT, split("%2"));
    toggle(&layer, cx, Some("%1"));
    assert!(is_visible(&layer, cx, "%1"));
    assert!(!is_opened(&layer, cx, "%2"));
}

/// 채팅 화면이 입력 포커스를 쥐고 있는지(입력창 또는 화면 자체)
fn is_chat_input_focused(
    layer: &Entity<PaneChatLayer>,
    cx: &mut VisualTestContext,
    pane: &str,
) -> bool {
    let Some(view) = layer.read_with(cx, |layer, _| layer.chat_view(pane)) else {
        return false;
    };
    cx.update(|window, cx| {
        view.focus_handle(cx).contains_focused(window, cx)
            || view.read(cx).is_input_focused(window, cx)
    })
}

#[gpui::test]
fn reshown_chat_pane_keeps_the_keyboard_until_returning_to_the_terminal(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    let returned = Rc::new(Cell::new(0));
    let counter = returned.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&layer, move |_, event: &PaneChatLayerEvent, _| {
            if *event == PaneChatLayerEvent::ReturnToTerminal {
                counter.set(counter.get() + 1);
            }
        })
    });
    toggle(&layer, cx, None);

    // 탭을 가렸다가 다시 보이면 채팅 pane이 돌아오고 키보드 입력은 터미널이 아니라 채팅으로 간다.
    // 터미널 질의 응답(DSR 등)은 레지스트리가 PTY 출력에서 바로 답하므로 채팅 포커스와 상관없이 유지된다
    set_layer_visible(&layer, cx, false);
    set_layer_visible(&layer, cx, true);
    assert!(is_visible(&layer, cx, "%1"));
    assert!(layer.read_with(cx, |layer, _| layer.is_chat_focused()));
    assert!(is_chat_input_focused(&layer, cx, "%1"));
    assert_eq!(returned.get(), 0);

    // 채팅의 터미널 버튼을 누르면 채팅을 걷고 작업 상세에 터미널로 입력을 돌리라고 알린다
    let chat = layer
        .read_with(cx, |layer, _| layer.chat_view("%1"))
        .unwrap();
    chat.update(cx, |view, cx| view.open_terminal(cx));
    settle(cx);
    assert!(!is_visible(&layer, cx, "%1"));
    assert!(!layer.read_with(cx, |layer, _| layer.is_chat_focused()));
    assert_eq!(returned.get(), 1);
}

#[gpui::test]
fn pane_shortcut_opens_the_active_pane_only_while_the_terminal_is_shown(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let layout = backend.hold(LAYOUT);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    // 배치를 읽기 전에는 pane 전환 버튼을 그리지 않는다
    assert_eq!(
        layer.read_with(cx, |layer, _| layer.toggle_pane().map(str::to_string)),
        None
    );
    layout.send(Ok(split("%1"))).unwrap();
    settle(cx);

    // 전환 단축키는 배치를 새로 읽어 활성 pane을 채팅으로 연다
    toggle(&layer, cx, None);
    assert!(is_visible(&layer, cx, "%1"));
    close(&layer, cx, "%1");

    // dock이 기록을 열면 터미널이 가려지고, 그동안의 단축키는 배치를 읽지도 pane을 열지도 않는다
    set_layer_visible(&layer, cx, false);
    backend.clear_calls();
    toggle(&layer, cx, None);
    assert_eq!(backend.call_count(LAYOUT), 0);
    assert!(!is_visible(&layer, cx, "%1"));
}

/// 배치가 그대로인 1초 주기 조회는 층을 다시 그리지 않는다(창 전체 다시 그리기 약 31ms를 아낀다)
#[gpui::test]
fn unchanged_layout_polls_do_not_redraw_the_layer(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    let reads_before = backend.call_count(LAYOUT);
    let (notifications, _subscription) = count_notifications(&layer, cx);

    for _ in 0..3 {
        advance_poll(cx);
    }

    assert_eq!(backend.call_count(LAYOUT), reads_before + 3);
    assert_eq!(notifications.get(), 0);
}

/// 활성 pane·좌표가 바뀌거나 조회가 실패했다가 복구되면 알리고, 덮개 위치와 안내가 새 상태를 따른다
#[gpui::test]
fn layout_changes_and_read_failures_still_redraw_the_layer(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    let (notifications, _subscription) = count_notifications(&layer, cx);

    backend.respond(
        LAYOUT,
        json!({ "activePaneId": "%1", "panes": [
            { "id": "%1", "tabId": "@1", "bounds": { "left": 0, "top": 0, "width": 29, "height": 24 } },
            { "id": "%2", "tabId": "@1", "bounds": { "left": 30, "top": 0, "width": 50, "height": 24 } }] }),
    );
    advance_poll(cx);
    assert!(notifications.get() >= 1);
    assert_eq!(width_of(&layer, cx, "%1"), Some((0.0, 290.0)));

    notifications.set(0);
    backend.respond(LAYOUT, split("%2"));
    advance_poll(cx);
    assert!(notifications.get() >= 1);
    assert_eq!(
        layer
            .read_with(cx, |layer, _| layer.toggle_pane().map(str::to_string))
            .as_deref(),
        Some("%2")
    );

    notifications.set(0);
    backend.fail(LAYOUT, "tmux unavailable");
    advance_poll(cx);
    assert!(notifications.get() >= 1);
    assert_eq!(
        layer.read_with(cx, |layer, _| layer.alert_message_key()),
        Some("readError")
    );
    assert!(!is_visible(&layer, cx, "%1"));
    assert!(!layer.read_with(cx, |layer, _| layer.is_chat_focused()));

    notifications.set(0);
    backend.respond(LAYOUT, split("%1"));
    advance_poll(cx);
    assert!(notifications.get() >= 1);
    assert_eq!(
        layer.read_with(cx, |layer, _| layer.alert_message_key()),
        None
    );
    assert!(is_visible(&layer, cx, "%1"));
}

/// 입력을 쥔 채팅 pane이 배치에서 사라지면 알리고 포커스를 놓는다
#[gpui::test]
fn focused_chat_pane_disappearing_redraws_and_releases_focus(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(&saved_modes(), false);
    let (layer, cx) = open_layer(cx, &backend, "task-1", true);
    toggle(&layer, cx, None);
    assert!(layer.read_with(cx, |layer, _| layer.is_chat_focused()));
    let (notifications, _subscription) = count_notifications(&layer, cx);

    backend.respond(
        LAYOUT,
        json!({ "activePaneId": "%2", "panes": [
            { "id": "%2", "tabId": "@1", "bounds": { "left": 0, "top": 0, "width": 80, "height": 24 } }] }),
    );
    advance_poll(cx);

    assert!(notifications.get() >= 1);
    assert!(!layer.read_with(cx, |layer, _| layer.is_chat_focused()));
    assert!(!is_visible(&layer, cx, "%1"));
}
