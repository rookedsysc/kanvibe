use serde_json::{Value, json};

use super::*;
use crate::aggregate::get_ai_session_detail;
use crate::message_attachments::extract_message_attachments;
use crate::test_support::{local_host, temp_home, write_file};

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aRZkAAAAASUVORK5CYII=";
const WORKTREE: &str = "/qa/image-worktree";

fn context_for(worktree: &str) -> AiSessionReaderContext {
    let mut context = AiSessionReaderContext::new(local_host("/unused-home"), Some(worktree));
    context.repo_path = Some(worktree.to_string());
    context
}

fn context() -> AiSessionReaderContext {
    context_for(WORKTREE)
}

/// TS 테스트의 `rollout(provider, events)`. 마지막 줄바꿈 없이 줄로 잇는다
fn rollout(fixture: &str, provider: &str, events: &[Value]) -> String {
    let file = format!("{fixture}/{provider}/images.jsonl");
    let lines: Vec<String> = events.iter().map(crate::javascript::stringify).collect();
    write_file(&file, &lines.join("\n"));
    file
}

fn codex_meta(cwd: &str) -> Value {
    json!({"type": "session_meta", "payload": {"id": "image-session", "cwd": cwd}})
}

fn codex_user(content: Value) -> Value {
    json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": content}})
}

fn first_attachment_id(provider: AiSessionProvider, source: &str) -> String {
    let detail = get_ai_session_detail(
        &context(),
        provider,
        "image-session",
        Some(source),
        None,
        None,
    )
    .unwrap()
    .unwrap();
    detail.messages[0].attachment_list()[0].id.clone()
}

#[test]
fn foreign_worktree_or_session_images_are_hidden() {
    let (_directory, fixture) = temp_home();
    let source = rollout(
        &fixture,
        "codex",
        &[
            codex_meta("/qa/foreign-worktree"),
            codex_user(
                json!([{"type": "input_image", "image_url": format!("data:image/png;base64,{PNG}")}]),
            ),
        ],
    );
    for session_id in ["image-session", "other-session"] {
        let detail = get_ai_session_detail(
            &context(),
            AiSessionProvider::Codex,
            session_id,
            Some(&source),
            None,
            None,
        )
        .unwrap();
        assert!(detail.is_none());
    }
}

#[test]
fn loads_authorized_inline_bytes_and_refuses_foreign_ids() {
    let (_directory, fixture) = temp_home();
    let source = rollout(
        &fixture,
        "claude",
        &[json!({
            "type": "user", "sessionId": "image-session", "cwd": WORKTREE,
            "message": {"role": "user", "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": PNG}}]}
        })],
    );
    let id = first_attachment_id(AiSessionProvider::Claude, &source);
    let read = |context: &AiSessionReaderContext, session_id: &str, attachment_id: &str| {
        read_ai_session_attachment(
            context,
            AiSessionProvider::Claude,
            session_id,
            attachment_id,
            Some(&source),
        )
        .unwrap()
    };
    assert_eq!(
        read(&context(), "image-session", &id),
        Some(AiSessionAttachmentContent {
            id: id.clone(),
            name: "image.png".to_string(),
            mime_type: "image/png".to_string(),
            data_url: format!("data:image/png;base64,{PNG}"),
        })
    );
    assert_eq!(read(&context(), "other-session", &id), None);
    assert_eq!(read(&context_for("/other"), "image-session", &id), None);
    assert_eq!(read(&context(), "image-session", "/etc/passwd"), None);
}

#[test]
fn loads_recorded_local_image_until_it_is_removed() {
    let (_directory, fixture) = temp_home();
    let image_path = format!("{fixture}/owned image.png");
    std::fs::write(&image_path, STANDARD.decode(PNG).unwrap()).unwrap();
    let source = rollout(
        &fixture,
        "codex",
        &[
            codex_meta(WORKTREE),
            codex_user(json!([{"type": "local_image", "path": image_path}])),
        ],
    );
    let id = first_attachment_id(AiSessionProvider::Codex, &source);
    let read = || {
        read_ai_session_attachment(
            &context(),
            AiSessionProvider::Codex,
            "image-session",
            &id,
            Some(&source),
        )
        .unwrap()
    };
    assert_eq!(
        read().map(|content| content.data_url),
        Some(format!("data:image/png;base64,{PNG}"))
    );
    std::fs::remove_file(&image_path).unwrap();
    assert_eq!(read(), None);
}

#[test]
fn refuses_remote_urls_and_unsafe_image_markup() {
    let (_directory, fixture) = temp_home();
    let mut remote = codex_user(
        json!([{"type": "input_image", "image_url": "https://example.invalid/private.png"}]),
    );
    remote["timestamp"] = json!("2026-11-13T01:00:00Z");
    let mut svg = codex_user(
        json!([{"type": "input_image", "image_url": "data:image/svg+xml;base64,PHN2Zy8+"}]),
    );
    svg["timestamp"] = json!("2026-11-13T01:00:01Z");
    let source = rollout(&fixture, "codex", &[codex_meta(WORKTREE), remote, svg]);
    let detail = get_ai_session_detail(
        &context(),
        AiSessionProvider::Codex,
        "image-session",
        Some(&source),
        None,
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(detail.messages.len(), 2);
    for message in &detail.messages {
        let id = &message.attachment_list()[0].id;
        assert!(
            read_ai_session_attachment(
                &context(),
                AiSessionProvider::Codex,
                "image-session",
                id,
                Some(&source)
            )
            .is_err()
        );
    }
}

#[test]
fn foreign_claude_cwd_is_not_inherited_by_later_rows() {
    let (_directory, fixture) = temp_home();
    let secret = format!("{fixture}/owned-secret.txt");
    std::fs::write(&secret, "Owned foreign task bytes").unwrap();
    let content = json!([{"type": "file", "path": secret}]);
    let source = rollout(
        &fixture,
        "claude",
        &[
            json!({"type": "user", "sessionId": "foreign-session", "cwd": "/qa/foreign-worktree", "message": {"role": "user", "content": "Foreign task"}}),
            json!({"type": "user", "sessionId": "foreign-session", "message": {"role": "user", "content": content}}),
        ],
    );
    let id = extract_message_attachments(&content, &source)[0].id.clone();
    let attachment = read_ai_session_attachment(
        &context(),
        AiSessionProvider::Claude,
        "foreign-session",
        &id,
        Some(&source),
    )
    .unwrap();
    assert_eq!(attachment, None);
    let detail = get_ai_session_detail(
        &context(),
        AiSessionProvider::Claude,
        "foreign-session",
        Some(&source),
        None,
        None,
    )
    .unwrap();
    assert!(detail.is_none());
}

#[test]
fn rejects_recorded_fifo_without_waiting_for_a_writer() {
    let (_directory, fixture) = temp_home();
    let fifo = format!("{fixture}/owned-fifo.png");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let source = rollout(
        &fixture,
        "codex",
        &[
            codex_meta(WORKTREE),
            codex_user(json!([{"type": "local_image", "path": fifo}])),
        ],
    );
    let id = first_attachment_id(AiSessionProvider::Codex, &source);
    let (sender, receiver) = std::sync::mpsc::channel();
    let read_context = context();
    std::thread::spawn(move || {
        let outcome = read_ai_session_attachment(
            &read_context,
            AiSessionProvider::Codex,
            "image-session",
            &id,
            Some(&source),
        );
        let _ = sender.send(outcome.is_err());
    });
    let is_rejected = receiver.recv_timeout(std::time::Duration::from_secs(1));
    if is_rejected.is_err() {
        // 막혔다면 쓰는 쪽을 열어 읽기 스레드를 풀어 준다
        let _ = std::fs::OpenOptions::new().write(true).open(&fifo);
    }
    assert_eq!(is_rejected, Ok(true));
}

#[test]
fn decode_rejects_malformed_payloads_and_mismatched_images() {
    assert!(decode_attachment_data_url("data:image/png;base64,").is_err());
    assert!(decode_attachment_data_url("data:image/png,abcd").is_err());
    assert!(decode_attachment_data_url("data:image/png;base64,aGVsbG8=").is_err());
    let (bytes, mime_type) =
        decode_attachment_data_url("data:application/octet-stream;base64,aGVsbG8=").unwrap();
    assert_eq!(
        (bytes.as_slice(), mime_type.as_str()),
        (b"hello".as_slice(), "application/octet-stream")
    );
}
