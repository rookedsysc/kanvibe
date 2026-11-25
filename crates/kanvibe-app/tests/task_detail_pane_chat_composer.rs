//! pane 채팅 입력창(TS `PaneChatComposer.test.tsx`). 첨부·자동 완성·초안·전송 요청 식별값을 가짜 pane chat 서비스로 확인한다.

mod task_detail_pane_chat_support;

use std::sync::Arc;

use gpui_kit::{
    ClipboardItem, Entity, ExternalPaths, Image, ImageFormat, TestAppContext, VisualTestContext,
    gpui,
};
use kanvibe_app::views::task_detail::pane_chat::drafts::AttachmentStatus;
use kanvibe_app::views::task_detail::pane_chat::{CompletionKey, PaneChatView, PickedFile};
use serde_json::{Value, json};
use task_detail_pane_chat_support::*;

const SNAPSHOT: &str = "paneChat.getPaneChatSnapshot";
const SEND: &str = "paneChat.sendPaneChatPrompt";
const STAGE: &str = "paneChat.stagePaneChatAttachment";
const REMOVE: &str = "paneChat.removePaneChatAttachment";
const SUGGEST: &str = "paneChat.getPaneChatSuggestions";
const PNG: [u8; 4] = [137, 80, 78, 71];

fn composer_snapshot(token: &str) -> Value {
    with(snapshot(token), "composer", composer_capabilities())
}

fn staged(id: &str, name: &str) -> Value {
    json!({ "id": id, "name": name, "mimeType": "image/png", "kind": "image", "sizeBytes": 4 })
}

fn suggestions(token: &str) -> Value {
    json!({ "targetToken": token, "truncated": false, "items": [
        { "id": "skill-1", "label": "Review", "description": "Review code", "insertText": "$review" },
        { "id": "skill-2", "label": "Test", "description": "Run tests", "insertText": "$test" }] })
}

fn backend(token: &str) -> FakeBackend {
    let backend = FakeBackend::new();
    backend.respond(SNAPSHOT, composer_snapshot(token));
    backend.respond(SEND, success());
    backend.respond(STAGE, staged("image-1", "photo.png"));
    backend.respond(REMOVE, success());
    backend.respond_with(SUGGEST, |arguments| {
        Ok(suggestions(arguments[1].as_str().unwrap_or_default()))
    });
    backend
}

fn image() -> PickedFile {
    PickedFile {
        name: "photo.png".into(),
        mime_type: "image/png".into(),
        bytes: Arc::new(PNG.to_vec()),
    }
}

fn text_file() -> PickedFile {
    PickedFile {
        name: "notes.txt".into(),
        mime_type: "text/plain".into(),
        bytes: Arc::new(b"hello".to_vec()),
    }
}

fn attach(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, files: Vec<PickedFile>) {
    view.update_in(cx, |view, window, cx| view.attach_files(files, window, cx));
    settle(cx);
}

fn edit(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, text: &str) {
    view.update_in(cx, |view, window, cx| {
        view.set_composer_text(text, text.len(), window, cx)
    });
    settle(cx);
}

fn send(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.submit(window, cx));
    settle(cx);
}

fn key(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, key: CompletionKey) {
    view.update_in(cx, |view, window, cx| {
        view.handle_completion_key(key, window, cx);
    });
    settle(cx);
}

fn enter(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.handle_enter(window, cx));
    settle(cx);
}

fn is_send_disabled(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.is_send_disabled())
}

fn names(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| {
        view.attachments()
            .into_iter()
            .map(|item| item.name)
            .collect()
    })
}

fn attachment_errors(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| {
        view.attachments()
            .iter()
            .filter_map(|item| view.attachment_error_text(item))
            .collect()
    })
}

fn first_local_id(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> u64 {
    view.read_with(cx, |view, _| view.attachments()[0].local_id)
}

fn options(view: &Entity<PaneChatView>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    view.read_with(cx, |view, _| view.suggestion_results())
        .map(|results| results.items.into_iter().map(|item| item.label).collect())
}

fn switch_to(
    backend: &FakeBackend,
    view: &Entity<PaneChatView>,
    cx: &mut VisualTestContext,
    snapshot_value: Value,
) {
    backend.respond(SNAPSHOT, snapshot_value);
    view.update_in(cx, |view, window, cx| view.refresh(window, cx));
    settle(cx);
}

fn close(view: Entity<PaneChatView>, cx: &mut VisualTestContext) {
    drop(view);
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
}

// parity: TS-renderer-PaneChatComposer-67b2ec
#[gpui::test]
fn composer_edge_resizes_and_busy_api_conversations_still_accept_messages(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let mut value = composer_snapshot("pane-a");
    value["inputState"] = json!("occupied");
    value["composer"]["canSendWhileBusy"] = json!(true);
    backend.respond(SNAPSHOT, value);
    let (view, cx) = open_view(cx, &backend, None, None);
    view.update_in(cx, |view, window, cx| {
        view.handle_resize_key("up", window, cx)
    });
    assert_eq!(view.read_with(cx, |view, _| view.composer_height()), 88.0);
    view.update_in(cx, |view, window, cx| {
        view.handle_resize_key("home", window, cx)
    });
    assert_eq!(view.read_with(cx, |view, _| view.composer_height()), 64.0);
    edit(&view, cx, "Continue while working");
    assert!(!is_send_disabled(&view, cx));
    send(&view, cx);
    let call = backend.last_call(SEND).unwrap();
    assert_eq!(&call[2..4], &[json!("Continue while working"), json!([])]);
    assert!(call[4].is_string());
}

// parity: TS-renderer-PaneChatComposer-fbfa5d
#[gpui::test]
fn resizing_clamps_between_the_minimum_and_sixty_percent_of_the_chat(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    view.update_in(cx, |view, window, cx| {
        view.resize_composer(64.0 + 150.0, window, cx)
    });
    assert_eq!(view.read_with(cx, |view, _| view.composer_height()), 214.0);
    view.update_in(cx, |view, window, cx| {
        view.resize_composer(214.0 - 350.0, window, cx)
    });
    assert_eq!(view.read_with(cx, |view, _| view.composer_height()), 64.0);
    view.update_in(cx, |view, window, cx| {
        view.resize_composer(10_000.0, window, cx)
    });
    let maximum = view.update_in(cx, |view, window, _| view.composer_max_height(window));
    assert_eq!(
        view.read_with(cx, |view, _| view.composer_height()),
        maximum
    );
}

// parity: TS-renderer-PaneChatComposer-d2aaff
#[gpui::test]
fn image_bytes_are_staged_and_sent_without_text_by_opaque_id(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    assert!(view.read_with(cx, |view, _| view.attachments()[0].preview.is_some()));
    assert!(!is_send_disabled(&view, cx));
    assert_eq!(
        backend.last_call(STAGE).unwrap()[2],
        json!({ "name": "photo.png", "mimeType": "image/png", "dataUrl": "data:image/png;base64,iVBORw==" })
    );
    send(&view, cx);
    let call = backend.last_call(SEND).unwrap();
    assert_eq!(&call[2..4], &[json!(""), json!(["image-1"])]);
    assert!(names(&view, cx).is_empty());
}

// parity: TS-renderer-PaneChatComposer-6fa72d
#[gpui::test]
fn photo_selection_clipboard_images_and_dropped_files_are_all_staged(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    backend.respond_with(STAGE, |arguments| {
        Ok(staged(
            arguments[2]["name"].as_str().unwrap_or_default(),
            "file",
        ))
    });
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    assert_eq!(backend.call_count(STAGE), 1);
    cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
        ImageFormat::Png,
        PNG.to_vec(),
    )));
    let is_pasted = view.update_in(cx, |view, window, cx| {
        view.paste_clipboard_image(window, cx)
    });
    assert!(is_pasted);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, "hello").unwrap();
    view.update_in(cx, |view, window, cx| {
        view.drop_paths(&ExternalPaths(vec![path].into()), window, cx)
    });
    cx.executor().allow_parking();
    settle(cx);
    assert_eq!(backend.call_count(STAGE), 3);
    assert!(names(&view, cx).contains(&"notes.txt".to_string()));
}

// parity: TS-renderer-PaneChatComposer-b82cb8
#[gpui::test]
fn failed_upload_blocks_sending_and_retries_the_same_file(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    backend.once(STAGE, Err(error("Upload unavailable")));
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Keep this caption");
    attach(&view, cx, vec![image()]);
    assert_eq!(
        attachment_errors(&view, cx),
        vec!["Upload unavailable".to_string()]
    );
    assert!(is_send_disabled(&view, cx));
    let local_id = first_local_id(&view, cx);
    view.update_in(cx, |view, window, cx| {
        view.retry_attachment(local_id, window, cx)
    });
    settle(cx);
    assert!(attachment_errors(&view, cx).is_empty());
    assert!(!is_send_disabled(&view, cx));
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Keep this caption"
    );
    assert_eq!(backend.call_count(STAGE), 2);
}

fn remove(view: &Entity<PaneChatView>, cx: &mut VisualTestContext, local_id: u64) {
    view.update_in(cx, |view, window, cx| {
        view.remove_attachment(local_id, window, cx)
    });
    settle(cx);
}

// parity: TS-renderer-PaneChatComposer-0232b8
#[gpui::test]
fn pending_upload_removed_before_its_acknowledgement_is_cleaned_up(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let acknowledge = backend.hold(STAGE);
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    let local_id = first_local_id(&view, cx);
    remove(&view, cx, local_id);
    acknowledge
        .send(Ok(staged("image-1", "photo.png")))
        .unwrap();
    settle(cx);
    assert!(names(&view, cx).is_empty());
    assert_eq!(
        backend.last_call(REMOVE).unwrap(),
        vec![json!("task-1"), json!("pane-a"), json!("image-1")]
    );
}

// parity: TS-renderer-PaneChatComposer-3d7875
#[gpui::test]
fn failed_removal_keeps_the_attachment_and_can_be_retried(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    backend.once(REMOVE, Err(error("Remove unavailable")));
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    let local_id = first_local_id(&view, cx);
    remove(&view, cx, local_id);
    assert_eq!(
        attachment_errors(&view, cx),
        vec!["Remove unavailable".to_string()]
    );
    remove(&view, cx, local_id);
    assert!(names(&view, cx).is_empty());
}

// parity: TS-renderer-PaneChatComposer-4d507a
#[gpui::test]
fn uploads_and_drafts_stay_with_their_pane(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let acknowledge = backend.hold(STAGE);
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Pane A caption");
    attach(&view, cx, vec![image()]);
    switch_to(&backend, &view, cx, composer_snapshot("pane-b"));
    edit(&view, cx, "Pane B caption");
    acknowledge
        .send(Ok(staged("image-1", "photo.png")))
        .unwrap();
    settle(cx);
    assert!(names(&view, cx).is_empty());
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Pane B caption"
    );
    switch_to(&backend, &view, cx, composer_snapshot("pane-a"));
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Pane A caption"
    );
    assert_eq!(names(&view, cx), vec!["photo.png".to_string()]);
}

// parity: TS-renderer-PaneChatComposer-8f9137
#[gpui::test]
fn earlier_send_completion_keeps_newer_text_and_attachments(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let accept = backend.hold(SEND);
    backend.once(STAGE, Ok(staged("image-1", "photo.png")));
    backend.once(STAGE, Ok(json!({ "id": "file-2", "name": "notes.txt", "mimeType": "text/plain", "kind": "file", "sizeBytes": 5 })));
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Original");
    attach(&view, cx, vec![image()]);
    send(&view, cx);
    edit(&view, cx, "New text");
    attach(&view, cx, vec![text_file()]);
    assert_eq!(backend.call_count(STAGE), 2);
    accept.send(Ok(success())).unwrap();
    settle(cx);
    assert_eq!(view.read_with(cx, |view, _| view.draft_text()), "New text");
    assert_eq!(names(&view, cx), vec!["notes.txt".to_string()]);
}

// parity: TS-renderer-PaneChatComposer-8c1bf0
#[gpui::test]
fn failed_send_reuses_the_request_id_only_for_an_unchanged_retry(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    backend.respond(SEND, json!({ "success": false }));
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Retry safely");
    send(&view, cx);
    assert!(
        view.read_with(cx, |view, _| view.composer_notes())
            .contains(&t("sendError"))
    );
    send(&view, cx);
    let calls = backend.calls(SEND);
    assert_eq!(calls[0][4], calls[1][4]);
    edit(&view, cx, "New request");
    send(&view, cx);
    assert_ne!(backend.calls(SEND)[2][4], calls[0][4]);
}

// parity: TS-renderer-PaneChatComposer-8676e0
#[gpui::test]
fn caret_token_suggestions_move_with_arrows_and_tab_inserts_without_sending(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Please $e");
    assert_eq!(
        options(&view, cx),
        Some(vec!["Review".to_string(), "Test".to_string()])
    );
    assert_eq!(
        backend.last_call(SUGGEST).unwrap(),
        vec![
            json!("task-1"),
            json!("pane-a"),
            json!("skill"),
            json!(""),
            json!(512)
        ]
    );
    key(&view, cx, CompletionKey::Down);
    key(&view, cx, CompletionKey::Tab);
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Please $test "
    );
    assert_eq!(backend.call_count(SEND), 0);
}

// parity: TS-renderer-PaneChatComposer-16d106
#[gpui::test]
fn skill_button_command_and_file_lookups_and_escape(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    view.update_in(cx, |view, window, cx| view.toggle_skill_picker(window, cx));
    view.update_in(cx, |view, window, cx| {
        view.set_skill_query("review", window, cx)
    });
    settle(cx);
    let review = view
        .read_with(cx, |view, _| view.suggestion_results())
        .unwrap()
        .items[0]
        .clone();
    view.update_in(cx, |view, window, cx| {
        view.choose_suggestion(&review, window, cx)
    });
    settle(cx);
    assert_eq!(
        backend.last_call(SUGGEST).unwrap(),
        vec![
            json!("task-1"),
            json!("pane-a"),
            json!("skill"),
            json!(""),
            json!(512)
        ]
    );
    assert_eq!(view.read_with(cx, |view, _| view.draft_text()), "$review ");
    edit(&view, cx, "/te");
    assert!(options(&view, cx).unwrap().contains(&"Test".to_string()));
    assert_eq!(backend.last_call(SUGGEST).unwrap()[2], json!("command"));
    edit(&view, cx, "@src");
    advance(cx, 200);
    assert!(options(&view, cx).unwrap().contains(&"Review".to_string()));
    assert_eq!(
        backend.last_call(SUGGEST).unwrap(),
        vec![
            json!("task-1"),
            json!("pane-a"),
            json!("file"),
            json!("src")
        ]
    );
    key(&view, cx, CompletionKey::Escape);
    assert_eq!(options(&view, cx), None);
    assert_eq!(view.read_with(cx, |view, _| view.draft_text()), "@src");
    assert_eq!(backend.call_count(SEND), 0);
}

// parity: TS-renderer-PaneChatComposer-27b8a9
#[gpui::test]
fn stale_file_replies_are_ignored_and_lookup_errors_can_be_retried(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let first = backend.hold(SUGGEST);
    backend.once(SUGGEST, Err(error("offline")));
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "@r");
    advance(cx, 200);
    assert_eq!(backend.call_count(SUGGEST), 1);
    edit(&view, cx, "@te");
    advance(cx, 200);
    assert!(
        view.read_with(cx, |view, _| view.suggestion_results())
            .unwrap()
            .has_error
    );
    // 앞 질의는 새 질의에 밀려 이미 버려졌을 수 있다. 늦게 온 답이 메뉴를 바꾸지 않으면 된다
    let _ = first.send(Ok(suggestions("pane-a")));
    settle(cx);
    assert_eq!(options(&view, cx), Some(Vec::new()));
    view.update_in(cx, |view, window, cx| view.retry_suggestions(window, cx));
    advance(cx, 200);
    assert!(options(&view, cx).unwrap().contains(&"Test".to_string()));
}

// parity: TS-renderer-PaneChatComposer-11454a
#[gpui::test]
fn skill_list_is_requested_once_and_enter_is_blocked_while_loading_or_empty(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend("pane-a");
    backend.respond(
        SUGGEST,
        json!({ "targetToken": "pane-a", "items": [], "truncated": false }),
    );
    let (view, cx) = open_view(cx, &backend, None, None);
    for text in ["$r", "$re", "$review"] {
        view.update_in(cx, |view, window, cx| {
            view.set_composer_text(text, text.len(), window, cx)
        });
    }
    enter(&view, cx);
    assert_eq!(backend.call_count(SEND), 0);
    let results = view
        .read_with(cx, |view, _| view.suggestion_results())
        .unwrap();
    assert!(results.items.is_empty() && !results.is_loading && !results.has_error);
    assert_eq!(
        backend.calls(SUGGEST),
        vec![vec![
            json!("task-1"),
            json!("pane-a"),
            json!("skill"),
            json!(""),
            json!(512)
        ]]
    );
    enter(&view, cx);
    assert_eq!(backend.call_count(SEND), 0);
    key(&view, cx, CompletionKey::Escape);
    enter(&view, cx);
    assert_eq!(backend.call_count(SEND), 1);
}

// parity: TS-renderer-PaneChatComposer-8a83bf
#[gpui::test]
fn loaded_command_list_is_filtered_on_screen_per_keystroke(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "/");
    assert_eq!(
        options(&view, cx),
        Some(vec!["Review".to_string(), "Test".to_string()])
    );
    edit(&view, cx, "/te");
    assert_eq!(options(&view, cx), Some(vec!["Test".to_string()]));
    edit(&view, cx, "/tes");
    assert_eq!(options(&view, cx), Some(vec!["Test".to_string()]));
    edit(&view, cx, "/zzz");
    let results = view
        .read_with(cx, |view, _| view.suggestion_results())
        .unwrap();
    assert!(results.items.is_empty() && !results.is_loading);
    assert_eq!(backend.call_count(SUGGEST), 1);
}

// parity: TS-renderer-PaneChatComposer-3905dc
#[gpui::test]
fn incomplete_skill_list_asks_the_desktop_per_query(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    backend.respond_with(SUGGEST, |arguments| {
        let token = arguments[1].clone();
        Ok(match arguments.get(4) {
            Some(_) => json!({ "targetToken": token, "truncated": true, "items": [{ "id": "skill-0", "label": "Alpha", "description": "First of many", "insertText": "$alpha" }] }),
            None => json!({ "targetToken": token, "truncated": false, "items": [{ "id": "skill-found", "label": format!("Found {}", arguments[3].as_str().unwrap_or_default()), "description": "From desktop", "insertText": "$found" }] }),
        })
    });
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "$");
    advance(cx, 200);
    assert_eq!(options(&view, cx), Some(vec!["Found ".to_string()]));
    edit(&view, cx, "$rev");
    advance(cx, 200);
    assert_eq!(options(&view, cx), Some(vec!["Found rev".to_string()]));
    assert_eq!(
        backend.last_call(SUGGEST).unwrap(),
        vec![
            json!("task-1"),
            json!("pane-a"),
            json!("skill"),
            json!("rev")
        ]
    );
}

// parity: TS-renderer-PaneChatComposer-38ba20
#[gpui::test]
fn reopened_menu_shows_the_remembered_list_and_refreshes_in_the_background(
    cx: &mut TestAppContext,
) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "$");
    assert!(options(&view, cx).is_some());
    edit(&view, cx, "");
    assert_eq!(options(&view, cx), None);
    let refresh = backend.hold(SUGGEST);
    edit(&view, cx, "$");
    assert_eq!(
        options(&view, cx),
        Some(vec!["Review".to_string(), "Test".to_string()])
    );
    assert_eq!(backend.call_count(SUGGEST), 2);
    let mut refreshed = suggestions("pane-a");
    refreshed["items"].as_array_mut().unwrap().push(json!({ "id": "skill-3", "label": "Plan", "description": "Outline work", "insertText": "$plan" }));
    refresh.send(Ok(refreshed)).unwrap();
    settle(cx);
    assert_eq!(
        options(&view, cx),
        Some(vec![
            "Plan".to_string(),
            "Review".to_string(),
            "Test".to_string()
        ])
    );
}

// parity: TS-renderer-PaneChatComposer-70698b
#[gpui::test]
fn failed_background_refresh_keeps_the_remembered_list(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "$");
    edit(&view, cx, "");
    backend.once(SUGGEST, Err(error("offline")));
    edit(&view, cx, "$");
    assert_eq!(backend.call_count(SUGGEST), 2);
    assert_eq!(
        options(&view, cx),
        Some(vec!["Review".to_string(), "Test".to_string()])
    );
    assert!(
        !view
            .read_with(cx, |view, _| view.suggestion_results())
            .unwrap()
            .has_error
    );
}

// parity: TS-renderer-PaneChatComposer-3f3a04
#[gpui::test]
fn skill_list_errors_retry_and_lists_for_another_pane_are_rejected(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    backend.once(SUGGEST, Err(error("offline")));
    backend.once(SUGGEST, Ok(suggestions("pane-b")));
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "$");
    assert!(
        view.read_with(cx, |view, _| view.suggestion_results())
            .unwrap()
            .has_error
    );
    view.update_in(cx, |view, window, cx| view.retry_suggestions(window, cx));
    settle(cx);
    assert_eq!(backend.call_count(SUGGEST), 2);
    let results = view
        .read_with(cx, |view, _| view.suggestion_results())
        .unwrap();
    assert!(results.has_error && results.items.is_empty());
    view.update_in(cx, |view, window, cx| view.retry_suggestions(window, cx));
    settle(cx);
    assert!(options(&view, cx).unwrap().contains(&"Review".to_string()));
}

// parity: TS-renderer-PaneChatComposer-0d677c
#[gpui::test]
fn file_lookup_waits_for_typing_to_pause(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    for text in ["@s", "@sr", "@src"] {
        edit(&view, cx, text);
    }
    advance(cx, 200);
    assert!(options(&view, cx).unwrap().contains(&"Review".to_string()));
    assert_eq!(
        backend.calls(SUGGEST),
        vec![vec![
            json!("task-1"),
            json!("pane-a"),
            json!("file"),
            json!("src")
        ]]
    );
}

// parity: TS-renderer-PaneChatComposer-01397a
#[gpui::test]
fn upload_acknowledged_after_close_is_removed(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let acknowledge = backend.hold(STAGE);
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    close(view, cx);
    acknowledge
        .send(Ok(staged("image-1", "photo.png")))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(
        backend.last_call(REMOVE).unwrap(),
        vec![json!("task-1"), json!("pane-a"), json!("image-1")]
    );
}

// parity: TS-renderer-PaneChatComposer-f15301
#[gpui::test]
fn closing_discards_ready_attachments_of_every_pane(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    switch_to(&backend, &view, cx, composer_snapshot("pane-b"));
    backend.respond(STAGE, staged("image-b", "photo.png"));
    attach(&view, cx, vec![image()]);
    close(view, cx);
    let mut removed = backend.calls(REMOVE);
    removed.sort_by_key(|call| call[1].to_string());
    assert_eq!(
        removed,
        vec![
            vec![json!("task-1"), json!("pane-a"), json!("image-1")],
            vec![json!("task-1"), json!("pane-b"), json!("image-b")],
        ]
    );
}

// parity: TS-renderer-PaneChatComposer-ae254d
#[gpui::test]
fn captured_send_is_disposed_only_after_its_outcome(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let accept = backend.hold(SEND);
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    send(&view, cx);
    close(view, cx);
    assert_eq!(backend.call_count(REMOVE), 0);
    accept.send(Ok(success())).unwrap();
    cx.run_until_parked();
    assert_eq!(
        backend.last_call(REMOVE).unwrap(),
        vec![json!("task-1"), json!("pane-a"), json!("image-1")]
    );
}

// parity: TS-renderer-PaneChatComposer-24bcb9
#[gpui::test]
fn oversized_files_and_too_many_files_are_rejected_without_staging(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("pane-a");
    let mut value = composer_snapshot("pane-a");
    value["composer"]["maxAttachmentBytes"] = json!(3);
    backend.respond(SNAPSHOT, value);
    let (view, cx) = open_view(cx, &backend, None, None);
    attach(&view, cx, vec![image()]);
    assert_eq!(attachment_errors(&view, cx), vec![t("attachmentTooLarge")]);
    attach(&view, cx, (0..10).map(|_| text_file()).collect());
    assert!(
        view.read_with(cx, |view, _| view.composer_notes())
            .contains(&t("attachmentLimit"))
    );
    assert_eq!(backend.call_count(STAGE), 0);
}

fn conversation(token: &str, key: &str) -> Value {
    with(composer_snapshot(token), "conversationKey", json!(key))
}

// parity: TS-renderer-PaneChatComposer-948dde
#[gpui::test]
fn renewed_lease_keeps_the_draft_and_requires_restaging(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("execution-a");
    backend.respond(
        SNAPSHOT,
        conversation("execution-a", "scoped-conversation-a"),
    );
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Keep my draft after reconnect");
    attach(&view, cx, vec![text_file()]);
    switch_to(
        &backend,
        &view,
        cx,
        conversation("execution-b", "scoped-conversation-a"),
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.draft_text()),
        "Keep my draft after reconnect"
    );
    let item = view.read_with(cx, |view, _| view.attachments()[0].clone());
    assert_eq!(item.status, AttachmentStatus::Failed);
    assert!(is_send_disabled(&view, cx));
    view.update_in(cx, |view, window, cx| {
        view.retry_attachment(item.local_id, window, cx)
    });
    settle(cx);
    let call = backend.last_call(STAGE).unwrap();
    assert_eq!(call[1], json!("execution-b"));
    assert_eq!(call[2]["name"], json!("notes.txt"));
}

// parity: TS-renderer-PaneChatComposer-d01506
#[gpui::test]
fn unresolved_operation_id_survives_token_renewal_and_restaging(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("old-token");
    backend.respond(
        SNAPSHOT,
        conversation("old-token", "uncertain-conversation"),
    );
    backend.respond(SEND, json!({ "success": false }));
    backend.once(STAGE, Ok(json!({ "id": "original-file", "name": "notes.txt", "mimeType": "text/plain", "kind": "file", "sizeBytes": 5 })));
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Unknown result");
    attach(&view, cx, vec![text_file()]);
    send(&view, cx);
    let request_id = backend.last_call(SEND).unwrap()[4].clone();
    backend.once(STAGE, Ok(json!({ "id": "restaged-file", "name": "notes.txt", "mimeType": "text/plain", "kind": "file", "sizeBytes": 5 })));
    switch_to(
        &backend,
        &view,
        cx,
        conversation("new-token", "uncertain-conversation"),
    );
    let local_id = first_local_id(&view, cx);
    view.update_in(cx, |view, window, cx| {
        view.retry_attachment(local_id, window, cx)
    });
    settle(cx);
    assert!(!is_send_disabled(&view, cx));
    send(&view, cx);
    let call = backend.last_call(SEND).unwrap();
    assert_eq!(
        call[1..],
        [
            json!("new-token"),
            json!("Unknown result"),
            json!(["restaged-file"]),
            request_id
        ]
    );
}

// parity: TS-renderer-PaneChatComposer-386e77
#[gpui::test]
fn old_upload_failure_after_restaging_is_ignored(cx: &mut TestAppContext) {
    init(cx);
    let backend = backend("old-execution");
    backend.respond(
        SNAPSHOT,
        conversation("old-execution", "same-scoped-conversation"),
    );
    let reject_old = backend.hold(STAGE);
    let (view, cx) = open_view(cx, &backend, None, None);
    edit(&view, cx, "Retain this draft");
    attach(&view, cx, vec![text_file()]);
    switch_to(
        &backend,
        &view,
        cx,
        conversation("renewed-execution", "same-scoped-conversation"),
    );
    let local_id = first_local_id(&view, cx);
    view.update_in(cx, |view, window, cx| {
        view.retry_attachment(local_id, window, cx)
    });
    settle(cx);
    assert!(!is_send_disabled(&view, cx));
    reject_old
        .send(Err(error("Old execution upload failed")))
        .unwrap();
    settle(cx);
    assert!(attachment_errors(&view, cx).is_empty());
    assert!(!is_send_disabled(&view, cx));
}
