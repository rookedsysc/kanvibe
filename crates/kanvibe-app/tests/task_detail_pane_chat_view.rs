//! pane 채팅 화면(TS `PaneChatView.test.tsx`, `usePaneChat.test.tsx`). 가짜 pane chat 서비스로 스냅샷과 동작 결과를 정하고,
//! 화면이 보이는 글·버튼 상태와 서비스에 보낸 호출을 확인한다.

mod task_detail_pane_chat_support;

use std::sync::{Arc, Mutex};

use gpui_kit::{Entity, TestAppContext, VisualTestContext, gpui};
use kanvibe_app::views::task_detail::pane_chat::{BodyBlock, PaneChatView, PaneChatViewEvent};
use kanvibe_domain::pane_chat::PaneChatPromptAnswer;
use serde_json::{Value, json};
use task_detail_pane_chat_support::*;

const SNAPSHOT: &str = "paneChat.getPaneChatSnapshot";
const SEND: &str = "paneChat.sendPaneChatPrompt";

fn backend(snapshot_value: Value) -> FakeBackend {
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, snapshot_value);
    backend.respond(SEND, success());
    backend.respond("paneChat.interruptPaneChat", success());
    backend.respond("paneChat.openPaneChatModelPicker", success());
    backend.respond("paneChat.sendPaneChatKey", success());
    backend
}

fn type_text(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, text: &str) {
    view.update_in(cx, |view, window, cx| {
        view.set_composer_text(text, text.len(), window, cx)
    });
    settle(cx);
}

fn send(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.submit(window, cx));
    settle(cx);
}

fn texts(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.body_texts(cx))
}

fn blocks(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Vec<BodyBlock> {
    view.read_with(cx, |view, cx| view.body_blocks(cx))
}

fn pending_count(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> usize {
    blocks(view, cx)
        .iter()
        .filter(|block| matches!(block, BodyBlock::Pending { .. }))
        .count()
}

fn record_events(
    view: &Entity<PaneChatView>,
    cx: &mut VisualTestContext,
) -> Arc<Mutex<Vec<PaneChatViewEvent>>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(view, move |_, event: &PaneChatViewEvent, _| {
            sink.lock().unwrap().push(*event)
        })
        .detach()
    });
    events
}

fn refresh(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.refresh(window, cx));
    settle(cx);
}

fn with_transcript(base: Value, key: &str, value: Value) -> Value {
    let mut snapshot = base;
    snapshot["transcript"][key] = value;
    snapshot
}

fn unavailable(reason: &str, provider: &str, input_state: &str) -> Value {
    let mut value = snapshot("process-a");
    value["provider"] = json!(provider);
    value["canSend"] = json!(false);
    value["targetToken"] = Value::Null;
    value["inputState"] = json!(input_state);
    value["reason"] = json!(reason);
    value["transcript"] = json!({ "status": "unavailable", "reason": reason, "sessionId": null, "messages": [], "nextCursor": null });
    value
}

#[gpui::test]
fn pending_bubble_shows_before_the_reply_and_disappears_when_the_record_arrives(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let reply = backend.hold(SEND);
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "Immediate request");
    send(&view, cx);
    assert!(texts(&view, cx).contains(&"Immediate request".to_string()));
    assert_eq!(pending_count(&view, cx), 1);
    let mut recorded = snapshot("process-a");
    let user = json!({ "role": "user", "timestamp": "2026-11-15T12:00:00Z", "text": "Immediate request", "fullText": "Immediate request", "isTruncated": false });
    recorded["transcript"]["messages"] = json!([user, message("assistant", "Exact pane response")]);
    backend.respond(SNAPSHOT, recorded);
    reply.send(Ok(success())).unwrap();
    settle(cx);
    assert_eq!(pending_count(&view, cx), 0);
    let shown = texts(&view, cx);
    assert_eq!(
        shown
            .iter()
            .filter(|text| *text == "Immediate request")
            .count(),
        1
    );
}

#[gpui::test]
fn older_pages_stay_expanded_after_the_latest_poll(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(with_transcript(
        snapshot("process-a"),
        "nextCursor",
        json!("100"),
    ));
    backend.respond(
        "paneChat.getPaneChatTranscriptPage",
        json!({ "status": "available", "reason": null, "sessionId": "session-a", "nextCursor": null, "messages": [message("user", "Older request")] }),
    );
    let (view, cx) = open_view(cx, &backend, None, None);
    view.update_in(cx, |view, window, cx| view.load_older_messages(window, cx));
    settle(cx);
    assert!(texts(&view, cx).contains(&"Older request".to_string()));
    assert_eq!(
        backend
            .last_call("paneChat.getPaneChatTranscriptPage")
            .unwrap(),
        vec![json!("task-1"), json!("process-a"), json!("100")]
    );
    advance(cx, 1_500);
    refresh(&view, cx);
    assert!(texts(&view, cx).contains(&"Older request".to_string()));
    assert!(
        !blocks(&view, cx)
            .iter()
            .any(|block| matches!(block, BodyBlock::LoadOlder { .. }))
    );
}

#[gpui::test]
fn show_instructions_reveals_system_and_agents_context_and_is_saved(cx: &mut TestAppContext) {
    init(cx);
    let mut value = snapshot("process-a");
    value["transcript"]["messages"] = json!([
        message("developer", "Project instructions"),
        message(
            "user",
            "# AGENTS.md instructions for /repo\n<INSTRUCTIONS>rules</INSTRUCTIONS>\n<environment_context>cwd</environment_context>"
        ),
        message("assistant", "Exact pane response"),
    ]);
    let backend = backend(value);
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(!texts(&view, cx).contains(&"Project instructions".to_string()));
    let (is_enabled, is_checked) = view.read_with(cx, |view, cx| view.show_instructions_state(cx));
    assert!(is_enabled && !is_checked);
    view.update(cx, |view, cx| view.toggle_show_instructions(cx));
    settle(cx);
    assert!(texts(&view, cx).contains(&"Project instructions".to_string()));
    assert_eq!(
        backend.last_call("appSettings.setAppSetting").unwrap(),
        vec![json!("pane_chat_show_instructions"), json!("true")]
    );
}

#[gpui::test]
fn codex_api_model_picker_opens_in_chat_without_terminal_switch(cx: &mut TestAppContext) {
    init(cx);
    let value = with(
        snapshot("process-a"),
        "modelPicker",
        json!({ "supported": true, "mode": "native-cli", "supportsApi": true }),
    );
    let backend = backend(value);
    backend.respond(
        "paneChat.getPaneChatModelSettings",
        json!({ "model": "a", "effort": "high", "isFast": null, "models": [
            { "id": "a", "label": "A", "effortLevels": [{ "id": "high", "label": "High" }], "supportsFast": false },
            { "id": "b", "label": "B", "effortLevels": [], "supportsFast": false }] }),
    );
    backend.respond("paneChat.changePaneChatModelSetting", success());
    let (view, cx) = open_view(cx, &backend, None, None);
    let events = record_events(&view, cx);
    view.update_in(cx, |view, window, cx| view.open_model_picker(window, cx));
    settle(cx);
    assert!(view.read_with(cx, |view, _| {
        view.model_picker()
            .is_some_and(|picker| picker.settings.is_some())
    }));
    view.update_in(cx, |view, window, cx| {
        let change = kanvibe_ai::pane_chat::model_settings::PaneChatModelSettingChange::Model {
            model: "b".into(),
        };
        view.change_model_setting(change, window, cx)
    });
    settle(cx);
    assert_eq!(
        backend
            .last_call("paneChat.changePaneChatModelSetting")
            .unwrap(),
        vec![
            json!("task-1"),
            json!("process-a"),
            json!({ "kind": "model", "model": "b" })
        ]
    );
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(backend.call_count("paneChat.openPaneChatModelPicker"), 0);
}

#[gpui::test]
fn claude_metadata_opens_the_model_popover_without_api_or_terminal(cx: &mut TestAppContext) {
    init(cx);
    let settings = json!({ "model": "sonnet", "effort": "high", "isFast": false, "models": [{ "id": "sonnet", "label": "Sonnet", "supportsFast": false, "effortLevels": [] }] });
    let mut value = snapshot("process-a");
    value["provider"] = json!("claude");
    value["modelSettings"] = settings.clone();
    value["modelPicker"] = json!({ "supported": true, "mode": "native-cli", "supportsApi": false });
    let backend = backend(value);
    backend.respond("paneChat.getPaneChatModelSettings", settings);
    let (view, cx) = open_view(cx, &backend, None, None);
    let events = record_events(&view, cx);
    assert!(
        view.read_with(cx, |view, _| view.model_trigger_label())
            .contains("Sonnet")
    );
    view.update_in(cx, |view, window, cx| view.open_model_picker(window, cx));
    settle(cx);
    let labels = view.read_with(cx, |view, _| {
        view.model_picker()
            .and_then(|picker| picker.settings.clone())
            .map(|settings| settings.models[0].label.clone())
    });
    assert_eq!(labels.as_deref(), Some("Sonnet"));
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(backend.call_count("paneChat.openPaneChatModelPicker"), 0);
}

#[gpui::test]
fn cli_capture_is_hidden_once_the_transcript_renders(cx: &mut TestAppContext) {
    init(cx);
    for input_state in ["ready", "occupied"] {
        let backend = backend(with(
            snapshot("process-a"),
            "inputState",
            json!(input_state),
        ));
        let (view, window_cx) = open_view(cx, &backend, None, None);
        let shown = texts(&view, window_cx);
        assert!(shown.contains(&"Exact pane response".to_string()));
        assert!(!shown.contains(&"Codex ready".to_string()));
        assert!(
            !blocks(&view, window_cx)
                .iter()
                .any(|block| matches!(block, BodyBlock::Screen { .. }))
        );
    }
}

#[gpui::test]
fn undelivered_send_drops_the_bubble_and_keeps_text_and_reason(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    backend.respond_with(SEND, |_| Err(error("CLI 입력란이 비어 있지 않습니다.")));
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "Unsent request");
    send(&view, cx);
    assert_eq!(pending_count(&view, cx), 0);
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Unsent request"
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.composer_notes())[0],
        "CLI 입력란이 비어 있지 않습니다."
    );
}

#[gpui::test]
fn possibly_delivered_send_keeps_the_bubble_with_the_send_error(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    backend.respond_with(SEND, |_| {
        Err(kanvibe_app::views::task_detail::pane_chat::backend::PaneChatCallError::from_ipc_message(
            "Error invoking remote method 'kanvibe:invoke': PaneChatDeliveryUncertainError: CLI 전송 결과를 확인할 수 없습니다.",
        ))
    });
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "Unsent request");
    send(&view, cx);
    let pending = blocks(&view, cx)
        .into_iter()
        .find(|block| matches!(block, BodyBlock::Pending { .. }))
        .expect("bubble");
    let shown = pending.texts();
    assert!(shown.contains(&"Unsent request".to_string()));
    assert!(shown.contains(&t("sendError")));
}

fn prompt_snapshot(prompt: Value) -> Value {
    let mut value = snapshot("process-a");
    value["provider"] = json!("claude");
    value["inputState"] = json!("occupied");
    value["prompt"] = prompt;
    value
}

fn option(number: u64, label: &str, is_checked: bool, is_text_entry: bool) -> Value {
    json!({ "number": number, "label": label, "description": "", "isChecked": is_checked, "isTextEntry": is_text_entry })
}

fn confirmation_prompt() -> Value {
    json!({ "id": "prompt-a", "kind": "confirmation", "title": "Switch model?", "detail": "", "stepsLine": null,
        "highlightedNumber": 1, "isMultiSelect": false, "isTextEntryActive": false, "hasHiddenOptions": false,
        "options": [option(1, "Yes, switch", false, false), option(2, "No, go back", false, false)] })
}

fn press_option(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, number: u8) {
    view.update_in(cx, |view, window, cx| {
        view.answer_shown_prompt(PaneChatPromptAnswer::Option { number }, window, cx)
    });
    settle(cx);
}

fn prompt_error(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, cx| {
        view.prompt_card_model(cx).and_then(|card| card.error)
    })
}

#[gpui::test]
fn prompt_card_replaces_the_composer_and_answers_with_target_and_prompt_id(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend(prompt_snapshot(confirmation_prompt()));
    backend.respond("paneChat.answerPaneChatPrompt", success());
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(view.read_with(cx, |view, _| view.is_composer_hidden()));
    press_option(&view, cx, 2);
    assert_eq!(
        backend.last_call("paneChat.answerPaneChatPrompt").unwrap(),
        vec![
            json!("task-1"),
            json!("process-a"),
            json!("prompt-a"),
            json!({ "type": "option", "number": 2 })
        ]
    );
}

#[gpui::test]
fn prompt_failure_shows_the_service_reason_only_on_that_card(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(prompt_snapshot(confirmation_prompt()));
    backend.respond_with("paneChat.answerPaneChatPrompt", |_| {
        Err(kanvibe_app::views::task_detail::pane_chat::backend::PaneChatCallError::from_ipc_message(
            "Error invoking remote method 'kanvibe:invoke': PaneChatInputError: CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.",
        ))
    });
    let (view, cx) = open_view(cx, &backend, None, None);
    press_option(&view, cx, 1);
    assert_eq!(
        prompt_error(&view, cx).as_deref(),
        Some("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.")
    );
    let mut next = confirmation_prompt();
    next["id"] = json!("prompt-b");
    next["title"] = json!("Do you want to proceed?");
    backend.respond(SNAPSHOT, prompt_snapshot(next));
    refresh(&view, cx);
    let card = view
        .read_with(cx, |view, cx| view.prompt_card_model(cx))
        .unwrap();
    assert_eq!(card.title, "Do you want to proceed?");
    assert_eq!(card.error, None);
}

fn toppings(id: &str, cheese_checked: bool, second: Value) -> Value {
    json!({ "id": id, "kind": "question", "title": "Which toppings?", "detail": "", "stepsLine": null,
        "highlightedNumber": 1, "isMultiSelect": true, "isTextEntryActive": false, "hasHiddenOptions": false,
        "options": [option(1, "Cheese", cheese_checked, false), second] })
}

#[gpui::test]
fn same_dialog_check_change_keeps_the_reason_and_typed_text(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(prompt_snapshot(toppings(
        "prompt-a",
        false,
        option(2, "Type something", false, true),
    )));
    backend.respond_with("paneChat.answerPaneChatPrompt", |_| {
        Err(error("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다."))
    });
    let (view, cx) = open_view(cx, &backend, None, None);
    let input = view
        .read_with(cx, |view, _| view.prompt_text_input(2))
        .expect("text entry");
    input.update_in(cx, |state, window, cx| {
        state.set_value("Garlic", window, cx)
    });
    backend.respond(
        SNAPSHOT,
        prompt_snapshot(toppings(
            "prompt-a-checked",
            true,
            option(2, "Type something", false, true),
        )),
    );
    press_option(&view, cx, 1);
    let card = view
        .read_with(cx, |view, cx| view.prompt_card_model(cx))
        .unwrap();
    assert!(matches!(
        card.options[0],
        kanvibe_app::views::task_detail::pane_chat::PromptOptionModel::Choice {
            is_pressed: Some(true),
            ..
        }
    ));
    assert_eq!(
        card.error.as_deref(),
        Some("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.")
    );
    let same_input = view
        .read_with(cx, |view, _| view.prompt_text_input(2))
        .unwrap();
    assert_eq!(
        same_input.read_with(cx, |state, _| state.value().to_string()),
        "Garlic"
    );
}

#[gpui::test]
fn reopened_prompt_does_not_show_the_previous_rejection(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(prompt_snapshot(confirmation_prompt()));
    backend.respond_with("paneChat.answerPaneChatPrompt", |_| {
        Err(error("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다."))
    });
    let (view, cx) = open_view(cx, &backend, None, None);
    backend.respond(SNAPSHOT, snapshot("process-a"));
    press_option(&view, cx, 1);
    assert!(
        view.read_with(cx, |view, cx| view.prompt_card_model(cx))
            .is_none()
    );
    backend.respond(SNAPSHOT, prompt_snapshot(confirmation_prompt()));
    refresh(&view, cx);
    assert!(
        view.read_with(cx, |view, cx| view.prompt_card_model(cx))
            .is_some()
    );
    assert_eq!(prompt_error(&view, cx), None);
}

fn option_enabled(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, index: usize) -> bool {
    let card = view
        .read_with(cx, |view, cx| view.prompt_card_model(cx))
        .unwrap();
    matches!(
        card.options[index],
        kanvibe_app::views::task_detail::pane_chat::PromptOptionModel::Choice {
            is_enabled: true,
            ..
        }
    )
}

#[gpui::test]
fn prompt_stays_locked_until_the_changed_screen_is_read(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(prompt_snapshot(confirmation_prompt()));
    backend.respond("paneChat.answerPaneChatPrompt", success());
    let (view, cx) = open_view(cx, &backend, None, None);
    let deliver = backend.hold(SNAPSHOT);
    press_option(&view, cx, 1);
    assert_eq!(backend.call_count("paneChat.answerPaneChatPrompt"), 1);
    assert!(!option_enabled(&view, cx, 0));
    deliver
        .send(Ok(prompt_snapshot(confirmation_prompt())))
        .unwrap();
    settle(cx);
    assert!(option_enabled(&view, cx, 0));
}

fn hold_cheese(
    cx: &mut TestAppContext,
) -> (
    FakeBackend,
    Entity<PaneChatView>,
    &mut VisualTestContext,
    futures::channel::oneshot::Sender<task_detail_pane_chat_support::Reply>,
) {
    init(cx);
    let backend = backend(prompt_snapshot(toppings(
        "prompt-a",
        false,
        option(2, "Olives", false, false),
    )));
    backend.respond("paneChat.answerPaneChatPrompt", success());
    let finish = backend.hold("paneChat.answerPaneChatPrompt");
    let (view, cx) = open_view(cx, &backend, None, None);
    press_option(&view, cx, 1);
    assert_eq!(backend.call_count("paneChat.answerPaneChatPrompt"), 1);
    (backend, view, cx, finish)
}

#[gpui::test]
fn multi_select_clicks_during_a_send_are_queued_once_and_sent_with_the_new_prompt_id(
    cx: &mut TestAppContext,
) {
    let (backend, view, cx, finish) = hold_cheese(cx);
    press_option(&view, cx, 2);
    press_option(&view, cx, 1);
    backend.respond(
        SNAPSHOT,
        prompt_snapshot(toppings(
            "prompt-cheese-checked",
            true,
            option(2, "Olives", false, false),
        )),
    );
    finish.send(Ok(success())).unwrap();
    settle(cx);
    let answers: Vec<(Value, Value)> = backend
        .calls("paneChat.answerPaneChatPrompt")
        .into_iter()
        .map(|call| (call[2].clone(), call[3].clone()))
        .collect();
    assert_eq!(
        answers,
        vec![
            (json!("prompt-a"), json!({ "type": "option", "number": 1 })),
            (
                json!("prompt-cheese-checked"),
                json!({ "type": "option", "number": 2 })
            ),
        ]
    );
}

#[gpui::test]
fn queued_answers_are_dropped_when_the_question_moved_on(cx: &mut TestAppContext) {
    let (backend, view, cx, finish) = hold_cheese(cx);
    press_option(&view, cx, 2);
    let mut next = toppings("prompt-next", false, option(2, "Olives", false, false));
    next["title"] = json!("Which size?");
    backend.respond(SNAPSHOT, prompt_snapshot(next));
    finish.send(Ok(success())).unwrap();
    settle(cx);
    assert_eq!(
        view.read_with(cx, |view, cx| view.prompt_card_model(cx))
            .unwrap()
            .title,
        "Which size?"
    );
    assert_eq!(backend.call_count("paneChat.answerPaneChatPrompt"), 1);
}

#[gpui::test]
fn without_a_prompt_the_composer_shows_and_no_card(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(
        view.read_with(cx, |view, cx| view.prompt_card_model(cx))
            .is_none()
    );
    assert!(!view.read_with(cx, |view, _| view.is_composer_hidden()));
}

#[gpui::test]
fn queries_the_explicit_pane(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let (_view, _cx) = open_view(cx, &backend, Some("%3"), None);
    assert_eq!(
        backend.calls(SNAPSHOT)[0],
        vec![json!("task-1"), json!("%3")]
    );
}

#[gpui::test]
fn another_pane_does_not_send_while_its_first_snapshot_loads(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    backend.hang(SNAPSHOT);
    let (view, cx) = open_view(cx, &backend, Some("%4"), None);
    assert!(!view.read_with(cx, |view, _| view.is_input_editable()));
    type_text(&view, cx, "Draft");
    send(&view, cx);
    assert_eq!(backend.call_count(SEND), 0);
}

#[gpui::test]
fn header_leads_with_the_task_title_and_names_only_the_snapshot_pane(cx: &mut TestAppContext) {
    init(cx);
    let pane_id = "67adc931-cb8d-46da-af83-651880ce17e2";
    let backend = backend(with(snapshot("process-a"), "paneId", json!(pane_id)));
    let (view, cx) = open_view(cx, &backend, None, Some("Fix checkout validation"));
    assert_eq!(
        view.read_with(cx, |view, _| view.header_title()),
        "Fix checkout validation"
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.header_subtitle()),
        "Codex · 80ce17e2"
    );
}

#[gpui::test]
fn draft_is_accepted_only_after_the_target_is_confirmed(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let deliver = backend.hold(SNAPSHOT);
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(!view.read_with(cx, |view, _| view.is_input_editable()));
    deliver.send(Ok(snapshot("process-a"))).unwrap();
    settle(cx);
    assert!(view.read_with(cx, |view, _| view.is_input_editable()));
}

#[gpui::test]
fn multiline_draft_goes_to_the_exact_process_and_clears_only_after_acceptance(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let accept = backend.hold(SEND);
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "First line\nSecond line");
    send(&view, cx);
    let call = backend.last_call(SEND).unwrap();
    assert_eq!(
        &call[..4],
        &[
            json!("task-1"),
            json!("process-a"),
            json!("First line\nSecond line"),
            json!([])
        ]
    );
    assert!(call[4].is_string());
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "First line\nSecond line"
    );
    accept.send(Ok(success())).unwrap();
    settle(cx);
    assert_eq!(view.read_with(cx, |view, _| view.draft_text()), "");
}

#[gpui::test]
fn rejected_write_keeps_the_draft_and_is_never_resent(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    backend.fail(SEND, "CLI target changed");
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "Keep this draft");
    send(&view, cx);
    advance(cx, 3_000);
    assert!(
        view.read_with(cx, |view, _| view.composer_notes())
            .contains(&"CLI target changed".to_string())
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Keep this draft"
    );
    assert_eq!(backend.call_count(SEND), 1);
}

#[gpui::test]
fn unsupported_native_model_dialog_opens_the_same_terminal(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let (view, cx) = open_view(cx, &backend, None, None);
    let events = record_events(&view, cx);
    view.update_in(cx, |view, window, cx| view.open_model_picker(window, cx));
    settle(cx);
    assert_eq!(
        *events.lock().unwrap(),
        vec![PaneChatViewEvent::OpenTerminal]
    );
    assert_eq!(
        backend
            .last_call("paneChat.openPaneChatModelPicker")
            .unwrap(),
        vec![json!("task-1"), json!("process-a")]
    );
    assert_eq!(backend.call_count("paneChat.sendPaneChatKey"), 0);
}

#[gpui::test]
fn drafts_are_isolated_per_process_and_restored_on_return(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "For process A only");
    backend.respond(SNAPSHOT, with(snapshot("process-b"), "paneId", json!("%4")));
    refresh(&view, cx);
    assert_eq!(view.read_with(cx, |view, _| view.draft_text()), "");
    backend.respond(SNAPSHOT, snapshot("process-a"));
    refresh(&view, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "For process A only"
    );
    assert_eq!(backend.call_count(SEND), 0);
}

#[gpui::test]
fn occupied_cli_keeps_the_draft_and_blocks_send_and_model(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(with(snapshot("process-a"), "inputState", json!("occupied")));
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(
        view.read_with(cx, |view, _| view.composer_notes())
            .contains(&t("inputOccupied"))
    );
    type_text(&view, cx, "Do not append this");
    assert!(view.read_with(cx, |view, _| view.is_send_disabled()));
    assert!(view.read_with(cx, |view, _| view.is_model_trigger_disabled()));
    send(&view, cx);
    assert_eq!(backend.call_count(SEND), 0);
}

fn set_visible(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, is_visible: bool) {
    view.update_in(cx, |view, window, cx| {
        view.set_visible(is_visible, window, cx)
    });
    settle(cx);
}

#[gpui::test]
fn hidden_pane_keeps_its_draft_and_pauses_polling(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "Return to this draft");
    set_visible(&view, cx, false);
    let reads = backend.call_count(SNAPSHOT);
    advance(cx, 6_000);
    assert_eq!(backend.call_count(SNAPSHOT), reads);
    set_visible(&view, cx, true);
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Return to this draft"
    );
}

#[gpui::test]
fn returning_from_the_terminal_rechecks_the_pane_before_accepting_input(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let (view, cx) = open_view(cx, &backend, None, None);
    type_text(&view, cx, "Original pane draft");
    set_visible(&view, cx, false);
    let deliver = backend.hold(SNAPSHOT);
    set_visible(&view, cx, true);
    assert!(!view.read_with(cx, |view, _| view.is_input_editable()));
    deliver
        .send(Ok(with(snapshot("process-b"), "paneId", json!("%4"))))
        .unwrap();
    settle(cx);
    assert_eq!(view.read_with(cx, |view, _| view.draft_text()), "");
    assert_eq!(backend.call_count(SEND), 0);
}

#[gpui::test]
fn unavailable_transcript_shows_the_cli_screen_and_keeps_history_separate(cx: &mut TestAppContext) {
    init(cx);
    let value = with(
        snapshot("process-a"),
        "transcript",
        json!({ "status": "unavailable", "reason": "exact-session-unavailable", "sessionId": null, "messages": [], "nextCursor": null }),
    );
    let backend = backend(value);
    let (view, cx) = open_view(cx, &backend, None, None);
    let events = record_events(&view, cx);
    assert!(texts(&view, cx).contains(&t("transcriptUnavailable")));
    view.update(cx, |view, cx| view.toggle_screen(cx));
    settle(cx);
    assert!(texts(&view, cx).contains(&"Codex ready".to_string()));
    view.update(cx, |view, cx| view.open_history(cx));
    assert_eq!(
        *events.lock().unwrap(),
        vec![PaneChatViewEvent::OpenHistory]
    );
    assert_eq!(backend.call_count(SEND), 0);
}

fn has_welcome(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> bool {
    blocks(view, cx)
        .iter()
        .any(|block| matches!(block, BodyBlock::Welcome { .. }))
}

#[gpui::test]
fn empty_sendable_conversation_shows_the_welcome_with_the_cli_name(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(with_transcript(
        snapshot("process-a"),
        "messages",
        json!([]),
    ));
    let (view, cx) = open_view(cx, &backend, None, None);
    let welcome = blocks(&view, cx)
        .into_iter()
        .find(|block| matches!(block, BodyBlock::Welcome { .. }))
        .expect("welcome");
    assert_eq!(
        welcome.texts()[0],
        translator().translate_with("paneChat.welcomeTitle", &[("provider", "Codex")])
    );
    assert!(view.read_with(cx, |view, _| view.is_input_editable()));
}

#[gpui::test]
fn conversation_with_messages_has_no_welcome(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("process-a"));
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(!has_welcome(&view, cx));
}

#[gpui::test]
fn first_message_removes_the_welcome(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(with_transcript(
        snapshot("process-a"),
        "messages",
        json!([]),
    ));
    backend.hang(SEND);
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(has_welcome(&view, cx));
    type_text(&view, cx, "First request");
    send(&view, cx);
    assert!(texts(&view, cx).contains(&"First request".to_string()));
    assert!(!has_welcome(&view, cx));
}

#[gpui::test]
fn recoverable_unavailable_offers_retry_and_terminal(cx: &mut TestAppContext) {
    init(cx);
    let mut value = unavailable("exact-session-unavailable", "codex", "ready");
    value["canSend"] = json!(false);
    let backend = backend(value);
    let (view, cx) = open_view(cx, &backend, None, None);
    let shown = texts(&view, cx);
    assert!(shown.contains(&t("transcriptUnavailable")));
    assert!(!shown.contains(&t("transcriptPending")));
    let events = record_events(&view, cx);
    refresh(&view, cx);
    assert_eq!(backend.call_count(SNAPSHOT), 2);
    view.update(cx, |view, cx| view.open_terminal(cx));
    assert_eq!(
        *events.lock().unwrap(),
        vec![PaneChatViewEvent::OpenTerminal]
    );
}

#[gpui::test]
fn unavailable_polling_backs_off_and_retry_reads_at_once(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(unavailable("exact-session-unavailable", "codex", "ready"));
    let (view, cx) = open_view(cx, &backend, None, None);
    for _ in 0..7 {
        advance(cx, 1_000);
    }
    assert_eq!(backend.call_count(SNAPSHOT), 3);
    for _ in 0..3 {
        advance(cx, 1_000);
    }
    assert_eq!(backend.call_count(SNAPSHOT), 4);
    refresh(&view, cx);
    assert_eq!(backend.call_count(SNAPSHOT), 5);
}

#[gpui::test]
fn terminal_input_wait_keeps_checking_and_connects_when_input_ends(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(unavailable("codex-thread-input-busy", "codex", "occupied"));
    let (view, cx) = open_view(cx, &backend, None, None);
    let shown = texts(&view, cx);
    assert!(shown.contains(&t("transcriptInputBusy")));
    assert!(!shown.contains(&t("transcriptIdentityMissing")));
    for _ in 0..6 {
        advance(cx, 1_000);
    }
    assert!(backend.call_count(SNAPSHOT) > 3);
    backend.respond(SNAPSHOT, snapshot("process-a"));
    advance(cx, 1_500);
    let shown = texts(&view, cx);
    assert!(shown.contains(&"Exact pane response".to_string()));
    assert!(!shown.contains(&t("transcriptInputBusy")));
    assert_eq!(backend.call_count(SEND), 0);
}

fn reveal_control(
    view: &Entity<PaneChatView>,
    cx: &mut VisualTestContext,
) -> Option<kanvibe_app::views::task_detail::pane_chat::RevealControl> {
    blocks(view, cx).into_iter().find_map(|block| match block {
        BodyBlock::Unavailable(card) => card.reveal,
        _ => None,
    })
}

fn reveal(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.reveal_codex_thread(window, cx));
    settle(cx);
}

#[gpui::test]
fn codex_daemon_card_explains_both_fixes_and_reveal_reports_its_result(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(unavailable("codex-thread-unconfirmed", "codex", "ready"));
    backend.respond(
        "paneChat.revealPaneChatCodexThread",
        json!({ "outcome": "thread-not-found" }),
    );
    let (view, cx) = open_view(cx, &backend, None, None);
    let shown = texts(&view, cx);
    for key in ["codexDaemonLimit", "codexRevealHint", "codexNoDaemonHint"] {
        assert!(shown.contains(&t(key)), "{key}");
    }
    assert!(!shown.contains(&t("transcriptIdentityMissing")));
    reveal(&view, cx);
    assert_eq!(
        backend
            .last_call("paneChat.revealPaneChatCodexThread")
            .unwrap(),
        vec![json!("task-1"), Value::Null]
    );
    let control = reveal_control(&view, cx).unwrap();
    assert_eq!(control.outcome, Some((t("revealThreadNotFound"), false)));
}

#[gpui::test]
fn each_reveal_outcome_has_its_own_message(cx: &mut TestAppContext) {
    init(cx);
    for (outcome, key) in [
        ("revealed", "revealRevealed"),
        ("input-not-empty", "revealInputNotEmpty"),
        ("unsupported-version", "revealUnsupportedVersion"),
    ] {
        let backend = backend(unavailable("codex-thread-unconfirmed", "codex", "ready"));
        backend.respond(
            "paneChat.revealPaneChatCodexThread",
            json!({ "outcome": outcome }),
        );
        let (view, window_cx) = open_view(cx, &backend, None, None);
        reveal(&view, window_cx);
        assert!(texts(&view, window_cx).contains(&t(key)), "{outcome}");
    }
}

#[gpui::test]
fn busy_codex_blocks_reveal_and_says_why(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(unavailable("codex-thread-input-busy", "codex", "occupied"));
    let (view, cx) = open_view(cx, &backend, None, None);
    let control = reveal_control(&view, cx).unwrap();
    assert!(control.is_disabled);
    assert_eq!(control.busy_note, Some(t("codexRevealBusy")));
    assert_eq!(backend.call_count("paneChat.revealPaneChatCodexThread"), 0);
}

#[gpui::test]
fn each_unavailable_reason_has_its_own_message_without_reveal(cx: &mut TestAppContext) {
    init(cx);
    let cases = [
        (
            "claude",
            "exact-session-unavailable",
            "claudeIdentityMissing",
        ),
        ("codex", "exact-session-unavailable", "codexIdentityMissing"),
        ("codex", "codex-thread-record-missing", "codexRecordMissing"),
        (
            "gemini",
            "exact-session-unavailable",
            "transcriptIdentityMissing",
        ),
        ("claude", "transcript-read-failed", "transcriptReadFailed"),
    ];
    for (provider, reason, key) in cases {
        let backend = backend(unavailable(reason, provider, "ready"));
        let (view, window_cx) = open_view(cx, &backend, None, None);
        assert!(
            texts(&view, window_cx).contains(&t(key)),
            "{provider} {reason}"
        );
        assert!(reveal_control(&view, window_cx).is_none());
    }
}

fn working(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    blocks(view, cx)
        .into_iter()
        .find(|block| matches!(block, BodyBlock::Working { .. }))
        .map(|block| block.texts())
}

#[gpui::test]
fn working_cli_shows_elapsed_seconds_and_clears_when_done(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(with(
        snapshot("process-a"),
        "agentActivity",
        json!("working"),
    ));
    let (view, cx) = open_view(cx, &backend, None, None);
    assert_eq!(working(&view, cx).unwrap(), vec![t("agentWorking")]);
    advance(cx, 1_000);
    let elapsed = translator().translate_with("paneChat.agentWorkingElapsed", &[("seconds", "1")]);
    assert!(working(&view, cx).unwrap().contains(&elapsed));
    backend.respond(
        SNAPSHOT,
        with(snapshot("process-a"), "agentActivity", json!("idle")),
    );
    refresh(&view, cx);
    assert!(working(&view, cx).is_none());
}

#[gpui::test]
fn unknown_activity_counts_as_working_until_the_send_is_recorded(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(with(
        snapshot("process-a"),
        "agentActivity",
        json!("unknown"),
    ));
    backend.hang(SEND);
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(working(&view, cx).is_none());
    type_text(&view, cx, "Start working");
    send(&view, cx);
    assert!(working(&view, cx).is_some());
}

#[gpui::test]
fn empty_composer_while_working_turns_send_into_stop(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(with(
        snapshot("process-a"),
        "agentActivity",
        json!("working"),
    ));
    let (view, cx) = open_view(cx, &backend, None, None);
    assert!(view.read_with(cx, |view, _| view.is_stop_in_place_of_send()));
    view.update_in(cx, |view, window, cx| view.interrupt(window, cx));
    settle(cx);
    assert_eq!(
        backend.last_call("paneChat.interruptPaneChat").unwrap(),
        vec![json!("task-1"), json!("process-a")]
    );
    type_text(&view, cx, "Follow-up");
    assert!(!view.read_with(cx, |view, _| view.is_stop_in_place_of_send()));
}

// usePaneChat.test.tsx

#[gpui::test]
fn accepted_send_returns_to_its_own_draft_after_the_conversation_changes(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(snapshot("X"));
    let accept = backend.hold(SEND);
    let (view, cx) = open_view(cx, &backend, Some("%3"), None);
    type_text(&view, cx, "For X");
    send(&view, cx);
    backend.respond(SNAPSHOT, snapshot("Y"));
    refresh(&view, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view
            .snapshot()
            .and_then(|s| s.target_token.clone()))
            .as_deref(),
        Some("Y")
    );
    accept.send(Ok(success())).unwrap();
    settle(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view
            .snapshot()
            .and_then(|s| s.target_token.clone()))
            .as_deref(),
        Some("Y")
    );
    assert!(
        view.read_with(cx, |view, _| view.composer_notes())
            .is_empty()
    );
    assert_eq!(backend.call_count(SEND), 1);
    backend.respond(SNAPSHOT, snapshot("X"));
    refresh(&view, cx);
    assert_eq!(view.read_with(cx, |view, _| view.draft_text()), "");
}

#[gpui::test]
fn keeps_discovering_a_cli_that_starts_after_unavailable_reads(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend(unavailable("exact-session-unavailable", "codex", "ready"));
    let (view, cx) = open_view(cx, &backend, Some("%3"), None);
    for _ in 0..3 {
        advance(cx, 1_000);
    }
    assert_eq!(backend.call_count(SNAPSHOT), 3);
    backend.respond(SNAPSHOT, snapshot("new-cli"));
    for _ in 0..5 {
        advance(cx, 1_000);
    }
    assert_eq!(
        view.read_with(cx, |view, _| view
            .snapshot()
            .and_then(|s| s.target_token.clone()))
            .as_deref(),
        Some("new-cli")
    );
}

/// 메시지를 보내 "전송됨" 말풍선이 남은 상태에서, CLI가 그 메시지를 기록한 대화를 다시 읽는다
fn send_then_record<'a>(
    cx: &'a mut TestAppContext,
    sent: &str,
    recorded: &str,
) -> (Entity<PaneChatView>, &'a mut VisualTestContext) {
    let mut value = with_transcript(snapshot("X"), "messages", json!([]));
    value["composer"] = composer_capabilities();
    let backend = backend(value.clone());
    backend.respond("paneChat.stagePaneChatAttachment", json!({ "id": "attached-file", "name": "note.txt", "mimeType": "text/plain", "kind": "file", "sizeBytes": 4 }));
    let (view, cx) = open_view(cx, &backend, Some("%3"), None);
    if sent.is_empty() {
        let file = kanvibe_app::views::task_detail::pane_chat::PickedFile {
            name: "note.txt".into(),
            mime_type: "text/plain".into(),
            bytes: Arc::new(b"note".to_vec()),
        };
        view.update_in(cx, |view, window, cx| {
            view.attach_files(vec![file], window, cx)
        });
        settle(cx);
    } else {
        type_text(&view, cx, sent);
    }
    send(&view, cx);
    assert_eq!(pending_count(&view, cx), 1);
    assert!(view.read_with(cx, |view, _| view.is_agent_working()));
    let user = json!({ "role": "user", "timestamp": "2026-11-17T12:00:00Z", "text": recorded, "fullText": recorded, "isTruncated": false });
    backend.respond(SNAPSHOT, with_transcript(value, "messages", json!([user])));
    refresh(&view, cx);
    (view, cx)
}

#[gpui::test]
fn recorded_shapes_of_the_sent_text_clear_the_bubble_and_working(cx: &mut TestAppContext) {
    let cases = [
        ("Ends with a newline  \n", "Ends with a newline"),
        ("tab\there\n\tindented", "tab    here\n    indented"),
        (
            "See the\tattached file",
            "See the    attached file\n\n@/attachments/note.txt",
        ),
        ("", "@/attachments/note.txt"),
    ];
    init(cx);
    for (sent, recorded) in cases {
        let (view, cx) = send_then_record(cx, sent, recorded);
        assert_eq!(pending_count(&view, cx), 0, "{sent:?}");
        assert!(!view.read_with(cx, |view, _| view.is_agent_working()));
    }
}

#[gpui::test]
fn longer_record_starting_with_the_sent_text_is_not_its_record(cx: &mut TestAppContext) {
    init(cx);
    let (view, cx) = send_then_record(cx, "Deploy now", "Deploy now please");
    assert_eq!(pending_count(&view, cx), 1);
    assert!(view.read_with(cx, |view, _| view.is_agent_working()));
}
