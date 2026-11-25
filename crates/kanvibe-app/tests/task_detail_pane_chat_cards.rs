//! pane 채팅의 선택 카드(TS `PaneChatPromptCard.test.tsx`)와 모델 팝오버(TS `PaneChatModelPicker.test.tsx`).
//! 카드와 팝오버는 채팅 화면 안에서 열리므로, 가짜 서비스가 돌려준 선택 화면·모델 설정으로 채팅 화면을 열고 확인한다.

mod task_detail_pane_chat_support;

use std::sync::{Arc, Mutex};

use gpui_kit::{
    Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, gpui, point, px,
};
use kanvibe_ai::pane_chat::model_settings::PaneChatModelSettingChange;
use kanvibe_app::views::task_detail::pane_chat::{
    PaneChatView, PaneChatViewEvent, PromptCardModel, PromptOptionModel,
};
use kanvibe_domain::pane_chat::{PaneChatPromptAnswer, PaneChatPromptStep};
use serde_json::{Value, json};
use task_detail_pane_chat_support::*;

const SNAPSHOT: &str = "paneChat.getPaneChatSnapshot";
const ANSWER: &str = "paneChat.answerPaneChatPrompt";
const SETTINGS: &str = "paneChat.getPaneChatModelSettings";
const CHANGE: &str = "paneChat.changePaneChatModelSetting";

fn option(number: u64, label: &str, extra: Value) -> Value {
    let mut value = json!({ "number": number, "label": label, "description": "", "isChecked": false, "isTextEntry": false });
    if let (Some(target), Some(source)) = (value.as_object_mut(), extra.as_object()) {
        target.extend(source.clone());
    }
    value
}

/// TS `question`
fn question() -> Value {
    json!({ "id": "prompt-question", "kind": "question", "title": "Which color do you prefer?", "detail": "", "stepsLine": null,
        "highlightedNumber": 1, "isMultiSelect": false, "isTextEntryActive": false, "hasHiddenOptions": false,
        "options": [option(1, "Red", json!({ "description": "Warm" })), option(2, "Green", json!({ "description": "Calm" })),
            option(3, "Type something.", json!({ "isTextEntry": true }))] })
}

fn change(prompt: Value, key: &str, value: Value) -> Value {
    with(prompt, key, value)
}

fn open_prompt(
    cx: &mut TestAppContext,
    prompt: Value,
) -> (FakeBackend, Entity<PaneChatView>, &mut VisualTestContext) {
    init(cx);
    open_prompt_in(cx, prompt)
}

fn open_prompt_in(
    cx: &mut TestAppContext,
    prompt: Value,
) -> (FakeBackend, Entity<PaneChatView>, &mut VisualTestContext) {
    let mut value = snapshot("process-a");
    value["provider"] = json!("claude");
    value["inputState"] = json!("occupied");
    value["prompt"] = prompt;
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, value);
    backend.respond(ANSWER, success());
    let (view, cx) = open_view(cx, &backend, None, None);
    (backend, view, cx)
}

fn card(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> PromptCardModel {
    view.read_with(cx, |view, cx| view.prompt_card_model(cx))
        .expect("prompt card")
}

fn answer(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, answer: PaneChatPromptAnswer) {
    view.update_in(cx, |view, window, cx| {
        view.answer_shown_prompt(answer, window, cx)
    });
    settle(cx);
}

fn answers(backend: &FakeBackend) -> Vec<Value> {
    backend
        .calls(ANSWER)
        .into_iter()
        .map(|call| call[3].clone())
        .collect()
}

// parity: TS-renderer-PaneChatPromptCard-a1cd37
#[gpui::test]
fn question_shows_options_with_descriptions_and_answers_the_chosen_number(cx: &mut TestAppContext) {
    let (backend, view, cx) = open_prompt(cx, question());
    let model = card(&view, cx);
    assert_eq!(model.kind_label, t("promptQuestion"));
    assert_eq!(model.title, "Which color do you prefer?");
    assert!(
        matches!(&model.options[1], PromptOptionModel::Choice { description, .. } if description == "Calm")
    );
    answer(&view, cx, PaneChatPromptAnswer::Option { number: 2 });
    assert_eq!(
        answers(&backend),
        vec![json!({ "type": "option", "number": 2 })]
    );
}

// parity: TS-renderer-PaneChatPromptCard-7f0586
#[gpui::test]
fn blank_text_entry_is_an_input_whose_text_becomes_the_answer(cx: &mut TestAppContext) {
    let (backend, view, cx) = open_prompt(cx, question());
    assert_eq!(
        card(&view, cx).options[2],
        PromptOptionModel::TextEntry {
            number: 3,
            can_submit: false
        }
    );
    let input = view
        .read_with(cx, |view, _| view.prompt_text_input(3))
        .expect("input");
    input.update_in(cx, |state, window, cx| {
        state.set_value("Purple", window, cx)
    });
    assert_eq!(
        card(&view, cx).options[2],
        PromptOptionModel::TextEntry {
            number: 3,
            can_submit: true
        }
    );
    view.update_in(cx, |view, window, cx| {
        view.submit_prompt_text(3, window, cx)
    });
    settle(cx);
    assert_eq!(
        answers(&backend),
        vec![json!({ "type": "text", "number": 3, "text": "Purple" })]
    );
}

// parity: TS-renderer-PaneChatPromptCard-355ad1
#[gpui::test]
fn entered_single_select_text_is_shown_and_answers_with_that_text(cx: &mut TestAppContext) {
    let prompt = change(
        question(),
        "options",
        json!([
            option(1, "Red", json!({})),
            option(2, "Green", json!({})),
            option(
                3,
                "Purple",
                json!({ "isTextEntry": true, "isChecked": true })
            )
        ]),
    );
    let (backend, view, cx) = open_prompt(cx, change(prompt, "isTextEntryActive", json!(true)));
    assert_eq!(
        card(&view, cx).options[2],
        PromptOptionModel::EnteredText {
            number: 3,
            label: "Purple".into(),
            can_submit: Some(true)
        }
    );
    assert!(
        view.read_with(cx, |view, _| view.prompt_text_input(3))
            .is_none()
    );
    answer(
        &view,
        cx,
        PaneChatPromptAnswer::Text {
            number: 3,
            text: "Purple".into(),
        },
    );
    assert_eq!(
        answers(&backend),
        vec![json!({ "type": "text", "number": 3, "text": "Purple" })]
    );
}

// parity: TS-renderer-PaneChatPromptCard-1f6b7b
#[gpui::test]
fn entered_multi_select_text_has_no_answer_button(cx: &mut TestAppContext) {
    let prompt = change(
        question(),
        "options",
        json!([
            option(1, "Cheese", json!({})),
            option(2, "Olives", json!({})),
            option(
                3,
                "Garlic",
                json!({ "isTextEntry": true, "isChecked": true })
            )
        ]),
    );
    let (_backend, view, cx) = open_prompt(cx, change(prompt, "isMultiSelect", json!(true)));
    assert_eq!(
        card(&view, cx).options[2],
        PromptOptionModel::EnteredText {
            number: 3,
            label: "Garlic".into(),
            can_submit: None
        }
    );
}

// parity: TS-renderer-PaneChatPromptCard-180586
#[gpui::test]
fn step_buttons_work_while_text_entry_is_active(cx: &mut TestAppContext) {
    let prompt = change(
        change(question(), "stepsLine", json!("←  ☐ Size  ✔ Submit  →")),
        "isTextEntryActive",
        json!(true),
    );
    let (_backend, view, cx) = open_prompt(cx, prompt);
    assert_eq!(card(&view, cx).are_steps_enabled, Some(true));
}

// parity: TS-renderer-PaneChatPromptCard-ea48c6
#[gpui::test]
fn hidden_options_notice_appears_only_when_options_are_hidden(cx: &mut TestAppContext) {
    init(cx);
    for (has_hidden_options, expected) in [(true, Some(t("promptHiddenOptions"))), (false, None)] {
        let (_backend, view, window_cx) = open_prompt_in(
            cx,
            change(question(), "hasHiddenOptions", json!(has_hidden_options)),
        );
        assert_eq!(card(&view, window_cx).hidden_options_notice, expected);
    }
}

// parity: TS-renderer-PaneChatPromptCard-a58901
#[gpui::test]
fn multi_select_reports_pressed_state_and_steps_move_between_questions(cx: &mut TestAppContext) {
    let prompt = json!({ "id": "prompt-question", "kind": "question", "title": "Which toppings?", "detail": "",
        "stepsLine": "←  ☒ Size  ☐ Toppings  ✔ Submit  →", "highlightedNumber": 1, "isMultiSelect": true,
        "isTextEntryActive": false, "hasHiddenOptions": false,
        "options": [option(1, "Cheese", json!({ "isChecked": true })), option(2, "Olives", json!({}))] });
    let (backend, view, cx) = open_prompt(cx, prompt);
    let model = card(&view, cx);
    assert_eq!(
        model.steps_line.as_deref(),
        Some("← ☒ Size ☐ Toppings ✔ Submit →")
    );
    assert!(matches!(
        model.options[0],
        PromptOptionModel::Choice {
            is_pressed: Some(true),
            ..
        }
    ));
    assert!(matches!(
        model.options[1],
        PromptOptionModel::Choice {
            is_pressed: Some(false),
            ..
        }
    ));
    answer(
        &view,
        cx,
        PaneChatPromptAnswer::Step {
            direction: PaneChatPromptStep::Next,
        },
    );
    answer(
        &view,
        cx,
        PaneChatPromptAnswer::Step {
            direction: PaneChatPromptStep::Previous,
        },
    );
    assert_eq!(
        answers(&backend),
        vec![
            json!({ "type": "step", "direction": "next" }),
            json!({ "type": "step", "direction": "previous" }),
        ]
    );
}

// parity: TS-renderer-PaneChatPromptCard-067ee9
#[gpui::test]
fn prompt_without_steps_has_no_step_buttons(cx: &mut TestAppContext) {
    let (_backend, view, cx) = open_prompt(cx, question());
    assert_eq!(card(&view, cx).are_steps_enabled, None);
}

// parity: TS-renderer-PaneChatPromptCard-8afb75
#[gpui::test]
fn permission_shows_the_tool_command_and_offers_cancel_and_terminal(cx: &mut TestAppContext) {
    let prompt = change(
        change(
            change(question(), "kind", json!("permission")),
            "title",
            json!("Do you want to proceed?"),
        ),
        "detail",
        json!("Bash command\n\ntouch probe.txt"),
    );
    let prompt = change(
        prompt,
        "options",
        json!([option(1, "Yes", json!({})), option(2, "No", json!({}))]),
    );
    let (backend, view, cx) = open_prompt(cx, prompt);
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &PaneChatViewEvent, _| {
            sink.lock().unwrap().push(*event)
        })
        .detach()
    });
    let model = card(&view, cx);
    assert_eq!(model.kind_label, t("promptPermission"));
    assert_eq!(model.detail, "Bash command\n\ntouch probe.txt");
    answer(&view, cx, PaneChatPromptAnswer::Cancel);
    view.update(cx, |view, cx| view.open_terminal(cx));
    assert_eq!(answers(&backend), vec![json!({ "type": "cancel" })]);
    assert_eq!(
        *events.lock().unwrap(),
        vec![PaneChatViewEvent::OpenTerminal]
    );
}

// parity: TS-renderer-PaneChatPromptCard-c51268
#[gpui::test]
fn cli_confirmation_is_labelled_as_such(cx: &mut TestAppContext) {
    let prompt = change(
        change(question(), "kind", json!("confirmation")),
        "title",
        json!("Switch model?"),
    );
    let (_backend, view, cx) = open_prompt(cx, prompt);
    assert_eq!(card(&view, cx).kind_label, t("promptConfirmation"));
}

// parity: TS-renderer-PaneChatPromptCard-65b6b0
#[gpui::test]
fn options_lock_while_answering_and_the_failure_reason_is_shown(cx: &mut TestAppContext) {
    let (backend, view, cx) = open_prompt(cx, question());
    let finish = backend.hold(ANSWER);
    answer(&view, cx, PaneChatPromptAnswer::Option { number: 1 });
    assert!(matches!(
        card(&view, cx).options[0],
        PromptOptionModel::Choice {
            is_enabled: false,
            ..
        }
    ));
    finish
        .send(Err(error(
            "CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.",
        )))
        .unwrap();
    settle(cx);
    assert_eq!(
        card(&view, cx).error.as_deref(),
        Some("CLI의 선택 화면이 바뀌어 답을 보내지 않았습니다.")
    );
}

// parity: TS-renderer-PaneChatPromptCard-6e8414
#[gpui::test]
fn two_digit_options_cannot_be_chosen(cx: &mut TestAppContext) {
    let mut options: Vec<Value> = (1..=9)
        .map(|number| option(number, &format!("Choice {number}"), json!({})))
        .collect();
    options.push(option(10, "Tenth", json!({})));
    let (_backend, view, cx) = open_prompt(cx, change(question(), "options", json!(options)));
    let model = card(&view, cx);
    assert!(matches!(
        model.options[9],
        PromptOptionModel::Choice {
            is_enabled: false,
            ..
        }
    ));
    assert!(matches!(
        model.options[8],
        PromptOptionModel::Choice {
            is_enabled: true,
            ..
        }
    ));
}

// PaneChatModelPicker.test.tsx

fn model_settings(model: &str) -> Value {
    json!({ "model": model, "effort": "high", "isFast": false, "models": [
        { "id": "a", "label": "Alpha", "supportsFast": true, "effortLevels": [{ "id": "low", "label": "Light" }, { "id": "high", "label": "High" }] },
        { "id": "b", "label": "Beta", "supportsFast": false, "effortLevels": [] }] })
}

/// API로 모델을 고르는 CLI의 채팅. `with_snapshot_settings`면 스냅샷이 모델 설정을 함께 준다
fn picker_backend(with_snapshot_settings: bool) -> FakeBackend {
    let mut value = snapshot("pane-a");
    value["modelPicker"] = json!({ "supported": true, "mode": "native-cli", "supportsApi": true });
    if with_snapshot_settings {
        value["modelSettings"] = model_settings("a");
    }
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, value);
    backend.respond(SETTINGS, model_settings("a"));
    backend.respond(CHANGE, success());
    backend
}

fn open_picker(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.open_model_picker(window, cx));
    settle(cx);
}

fn checked_model(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        view.model_picker()
            .and_then(|picker| picker.settings.as_ref())
            .and_then(|settings| settings.model.clone())
    })
}

fn is_model_focused(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, index: usize) -> bool {
    let handle = view.read_with(cx, |view, _| {
        view.model_picker()
            .and_then(|picker| picker.option_focus.get(index).cloned())
    });
    handle.is_some_and(|handle| cx.update(|window, _| handle.is_focused(window)))
}

fn change_setting(
    view: &Entity<PaneChatView>,
    cx: &mut VisualTestContext,
    change: PaneChatModelSettingChange,
) {
    view.update_in(cx, |view, window, cx| {
        view.change_model_setting(change, window, cx)
    });
    settle(cx);
}

fn picker_flag(
    view: &Entity<PaneChatView>,
    cx: &mut VisualTestContext,
    read: fn(&kanvibe_app::views::task_detail::pane_chat::ModelPickerState) -> bool,
) -> bool {
    view.read_with(cx, |view, _| view.model_picker().is_some_and(read))
}

// parity: TS-renderer-PaneChatModelPicker-e067a3
#[gpui::test]
fn snapshot_options_show_at_once_and_refresh_through_the_api(cx: &mut TestAppContext) {
    init(cx);
    let backend = picker_backend(true);
    let complete = backend.hold(SETTINGS);
    let (view, cx) = open_view(cx, &backend, None, None);
    open_picker(&view, cx);
    assert_eq!(checked_model(&view, cx).as_deref(), Some("a"));
    assert!(is_model_focused(&view, cx, 0));
    complete.send(Ok(model_settings("b"))).unwrap();
    settle(cx);
    assert_eq!(checked_model(&view, cx).as_deref(), Some("b"));
}

// parity: TS-renderer-PaneChatModelPicker-3aebd3
#[gpui::test]
fn arrows_move_between_models_without_applying_and_escape_or_outside_click_closes(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = picker_backend(false);
    let (view, cx) = open_view(cx, &backend, None, None);
    open_picker(&view, cx);
    assert!(is_model_focused(&view, cx, 0));
    cx.simulate_keystrokes("down");
    settle(cx);
    assert!(is_model_focused(&view, cx, 1));
    assert_eq!(backend.call_count(CHANGE), 0);
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(view.read_with(cx, |view, _| view.model_picker().is_none()));
    open_picker(&view, cx);
    cx.simulate_mouse_down(
        point(px(1_200.0), px(300.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    settle(cx);
    assert!(view.read_with(cx, |view, _| view.model_picker().is_none()));
}

// parity: TS-renderer-PaneChatModelPicker-d0ee12
#[gpui::test]
fn effort_reset_and_fast_apply_and_wait_for_the_confirmed_state(cx: &mut TestAppContext) {
    init(cx);
    let backend = picker_backend(false);
    let (view, cx) = open_view(cx, &backend, None, None);
    open_picker(&view, cx);
    change_setting(
        &view,
        cx,
        PaneChatModelSettingChange::Effort {
            effort: "low".into(),
        },
    );
    assert_eq!(
        backend.last_call(CHANGE).unwrap()[2],
        json!({ "kind": "effort", "effort": "low" })
    );
    assert!(!picker_flag(&view, cx, |picker| picker.is_busy));
    change_setting(&view, cx, PaneChatModelSettingChange::Reset);
    assert_eq!(
        backend.last_call(CHANGE).unwrap()[2],
        json!({ "kind": "reset" })
    );
    change_setting(
        &view,
        cx,
        PaneChatModelSettingChange::Fast { is_fast: true },
    );
    assert_eq!(
        backend.last_call(CHANGE).unwrap()[2],
        json!({ "kind": "fast", "isFast": true })
    );
    let is_fast = view.read_with(cx, |view, _| {
        view.model_picker()
            .and_then(|picker| picker.settings.as_ref())
            .and_then(|settings| settings.is_fast)
    });
    assert_eq!(is_fast, Some(false));
}

// parity: TS-renderer-PaneChatModelPicker-fa7034
#[gpui::test]
fn failed_updates_keep_the_confirmed_choice_and_catalog_can_be_retried(cx: &mut TestAppContext) {
    init(cx);
    let backend = picker_backend(true);
    backend.once(SETTINGS, Err(error("offline")));
    let (view, cx) = open_view(cx, &backend, None, None);
    open_picker(&view, cx);
    assert!(picker_flag(&view, cx, |picker| picker.has_error));
    view.update_in(cx, |view, window, cx| view.load_model_settings(window, cx));
    settle(cx);
    assert!(!picker_flag(&view, cx, |picker| picker.has_error));
    backend.once(CHANGE, Err(error("rejected")));
    change_setting(
        &view,
        cx,
        PaneChatModelSettingChange::Model { model: "b".into() },
    );
    assert!(picker_flag(&view, cx, |picker| picker.has_error));
    assert_eq!(checked_model(&view, cx).as_deref(), Some("a"));
}

// parity: TS-renderer-PaneChatModelPicker-4045cc
#[gpui::test]
fn closed_picker_is_not_refreshed_or_notified_after_an_update(cx: &mut TestAppContext) {
    init(cx);
    let backend = picker_backend(false);
    let complete = backend.hold(CHANGE);
    let (view, cx) = open_view(cx, &backend, None, None);
    open_picker(&view, cx);
    change_setting(
        &view,
        cx,
        PaneChatModelSettingChange::Model { model: "b".into() },
    );
    view.update_in(cx, |view, window, cx| {
        view.close_model_picker(false, window, cx)
    });
    let reads = backend.call_count(SNAPSHOT);
    complete.send(Ok(success())).unwrap();
    settle(cx);
    assert_eq!(backend.call_count(SETTINGS), 1);
    assert_eq!(backend.call_count(SNAPSHOT), reads);
}

// parity: TS-renderer-PaneChatModelPicker-a5a817
#[gpui::test]
fn older_catalog_reply_is_ignored_after_a_confirmed_update(cx: &mut TestAppContext) {
    init(cx);
    let backend = picker_backend(true);
    let finish_initial = backend.hold(SETTINGS);
    backend.respond(SETTINGS, model_settings("b"));
    let (view, cx) = open_view(cx, &backend, None, None);
    open_picker(&view, cx);
    let reads = backend.call_count(SNAPSHOT);
    change_setting(
        &view,
        cx,
        PaneChatModelSettingChange::Model { model: "b".into() },
    );
    assert_eq!(backend.call_count(SNAPSHOT), reads + 1);
    finish_initial.send(Ok(model_settings("a"))).unwrap();
    settle(cx);
    assert_eq!(checked_model(&view, cx).as_deref(), Some("b"));
}

// parity: TS-renderer-PaneChatModelPicker-ea00fa
#[gpui::test]
fn focus_stays_in_the_popup_while_a_change_disables_its_controls(cx: &mut TestAppContext) {
    init(cx);
    let backend = picker_backend(false);
    let finish = backend.hold(CHANGE);
    let (view, cx) = open_view(cx, &backend, None, None);
    open_picker(&view, cx);
    change_setting(
        &view,
        cx,
        PaneChatModelSettingChange::Model { model: "b".into() },
    );
    assert!(picker_flag(&view, cx, |picker| picker.is_busy));
    let dialog = view
        .read_with(cx, |view, _| {
            view.model_picker()
                .map(|picker| picker.focus_handle.clone())
        })
        .unwrap();
    assert!(cx.update(|window, _| dialog.is_focused(window)));
    finish.send(Ok(success())).unwrap();
    settle(cx);
    assert!(is_model_focused(&view, cx, 0));
    cx.simulate_keystrokes("escape");
    settle(cx);
    assert!(view.read_with(cx, |view, _| view.model_picker().is_none()));
}
