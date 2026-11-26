use std::fs::File;
use std::io::Write;
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use super::transcript::tests::built_message_count;
use super::*;
use crate::error::AiError;
use crate::host::SessionHost;
use crate::jsonl::read_json_lines_head;
use crate::test_support::{
    ScriptedRunner, json_lines, local_host, remote_file_system, remote_host, temp_home, write_file,
};
use crate::types::{AiFileChange, AiMessageFile, LiveAiSessionWindows};

const THREAD_ID: &str = "0190aaaa-0000-7000-8000-00000000000a";
const OTHER_THREAD_ID: &str = "0190bbbb-0000-7000-8000-00000000000b";

fn context(host: SessionHost, worktree_path: &str) -> AiSessionReaderContext {
    let mut context = AiSessionReaderContext::new(host, Some(worktree_path));
    context.repo_path = Some(worktree_path.to_string());
    context
}

fn detail(
    context: &AiSessionReaderContext,
    session_id: &str,
    source: &str,
) -> AggregatedAiSessionDetail {
    read_codex_session_detail(context, session_id, Some(source), None, Some(20))
        .unwrap()
        .expect("상세가 있다")
}

fn roles(detail: &AggregatedAiSessionDetail) -> Vec<AiMessageRole> {
    detail.messages.iter().map(|message| message.role).collect()
}

fn role_texts(detail: &AggregatedAiSessionDetail) -> Vec<(AiMessageRole, String)> {
    detail
        .messages
        .iter()
        .map(|message| (message.role, message.full_text.clone()))
        .collect()
}

fn response_item(minute: u32, payload: Value) -> Value {
    json!({ "type": "response_item", "timestamp": format!("2026-01-01T00:{minute:02}:00.000Z"), "payload": payload })
}

fn session_meta(id: &str, cwd: &str) -> Value {
    json!({ "type": "session_meta", "timestamp": "2026-01-01T00:00:00.000Z", "payload": { "id": id, "cwd": cwd, "timestamp": "2026-01-01T00:00:00.000Z" } })
}

fn user_text(text: &str) -> Value {
    json!({ "type": "message", "role": "user", "content": [{ "type": "input_text", "text": text }] })
}

fn assistant_text(text: &str) -> Value {
    json!({ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": text }] })
}

#[test]
fn resolves_exact_main_thread_and_refuses_duplicate_identities() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/task");
    let day_directory = format!("{home}/.codex/sessions/2026/11/14");
    let file = format!("{day_directory}/rollout-first-{THREAD_ID}.jsonl");
    let context = context(local_host(&home), &worktree_path);
    write_file(
        &file,
        &json_lines(&[
            json!({ "type": "session_meta", "payload": { "id": THREAD_ID, "cwd": worktree_path, "source": "cli" } }),
        ]),
    );
    write_file(
        &format!("{day_directory}/rollout-other-{OTHER_THREAD_ID}.jsonl"),
        &json_lines(&[
            json!({ "type": "session_meta", "payload": { "id": OTHER_THREAD_ID, "cwd": worktree_path } }),
        ]),
    );
    assert_eq!(
        find_codex_main_thread_rollout(&context, THREAD_ID).unwrap(),
        Some(file)
    );
    write_file(
        &format!("{day_directory}/rollout-copy-{THREAD_ID}.jsonl"),
        &json_lines(&[
            json!({ "type": "session_meta", "payload": { "id": THREAD_ID, "cwd": worktree_path } }),
        ]),
    );
    assert_eq!(
        find_codex_main_thread_rollout(&context, THREAD_ID).unwrap(),
        None
    );
}

#[test]
fn refuses_child_mismatched_and_out_of_scope_records() {
    let extras = [
        json!({ "source": { "subagent": { "spawn": { "parent_thread_id": "parent" } } } }),
        json!({ "parent_thread_id": "parent" }),
        json!({ "id": OTHER_THREAD_ID }),
        json!({ "cwd": "/different-task" }),
    ];
    for extra in extras {
        let (_directory, home) = temp_home();
        let worktree_path = format!("{home}/task");
        let mut payload = json!({ "id": THREAD_ID, "cwd": worktree_path });
        for (key, value) in extra.as_object().unwrap() {
            payload[key] = value.clone();
        }
        let file = format!("{home}/.codex/sessions/rollout-test-{THREAD_ID}.jsonl");
        write_file(
            &file,
            &json_lines(&[json!({ "type": "session_meta", "payload": payload })]),
        );
        let context = context(local_host(&home), &worktree_path);
        assert_eq!(
            find_codex_main_thread_rollout(&context, THREAD_ID).unwrap(),
            None,
            "{extra}"
        );
    }
}

#[test]
fn classifies_developer_user_reasoning_tool_and_assistant_events() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/repo__worktrees/task");
    let session_file = format!("{home}/.codex/sessions/2026/codex-session.jsonl");
    write_file(
        &session_file,
        &json_lines(&[
            session_meta("codex-session", &worktree_path),
            response_item(
                1,
                json!({ "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": "Follow project rules." }] }),
            ),
            response_item(2, user_text("implement history view")),
            response_item(
                3,
                json!({ "type": "reasoning", "text": "Need to inspect schemas." }),
            ),
            response_item(
                4,
                json!({ "type": "function_call", "name": "read_file", "arguments": "{\"path\":\"x\"}" }),
            ),
            response_item(
                5,
                json!({ "type": "function_call_output", "output": "file text" }),
            ),
            response_item(6, assistant_text("Done.")),
        ]),
    );
    let detail = detail(
        &context(local_host(&home), &worktree_path),
        "codex-session",
        &session_file,
    );
    assert_eq!(
        roles(&detail),
        [
            AiMessageRole::Assistant,
            AiMessageRole::Tool,
            AiMessageRole::Tool,
            AiMessageRole::Reasoning,
            AiMessageRole::User,
            AiMessageRole::Developer,
        ]
    );
    let developer = detail
        .messages
        .iter()
        .find(|message| message.role == AiMessageRole::Developer)
        .unwrap();
    assert_eq!(developer.full_text, "Follow project rules.");
}

#[test]
fn tool_calls_carry_names_and_apply_patch_files() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/repo__worktrees/task");
    let session_file = format!("{home}/.codex/sessions/2026/codex-tools.jsonl");
    let patch = "*** Begin Patch\n*** Add File: docs/plan.md\n+# plan\n*** Update File: src/app.ts\n@@\n-a\n+b\n*** Delete File: src/old.ts\n*** End Patch";
    write_file(
        &session_file,
        &json_lines(&[
            session_meta("codex-tools", &worktree_path),
            response_item(
                1,
                json!({ "type": "function_call", "name": "read_file", "arguments": "{\"path\":\"x\"}", "call_id": "call_1" }),
            ),
            response_item(
                2,
                json!({ "type": "custom_tool_call", "status": "completed", "call_id": "call_2", "name": "apply_patch", "input": patch }),
            ),
            response_item(
                3,
                json!({ "type": "custom_tool_call_output", "call_id": "call_2", "output": [{ "type": "input_text", "text": "Success. Updated the following files" }] }),
            ),
        ]),
    );
    let detail = detail(
        &context(local_host(&home), &worktree_path),
        "codex-tools",
        &session_file,
    );
    let messages: Vec<_> = detail
        .messages
        .iter()
        .rev()
        .map(|message| {
            (
                message.role,
                message.full_text.as_str(),
                message.tool_name.as_deref(),
                message.files.clone(),
            )
        })
        .collect();
    let patch_files = vec![
        AiMessageFile {
            path: "docs/plan.md".to_string(),
            change: AiFileChange::Created,
        },
        AiMessageFile {
            path: "src/app.ts".to_string(),
            change: AiFileChange::Modified,
        },
    ];
    assert_eq!(
        messages,
        [
            (
                AiMessageRole::Tool,
                "read_file: {\"path\":\"x\"}",
                Some("read_file"),
                None
            ),
            (
                AiMessageRole::Tool,
                "docs/plan.md\nsrc/app.ts",
                Some("apply_patch"),
                Some(patch_files)
            ),
            (
                AiMessageRole::Tool,
                "Success. Updated the following files",
                Some("apply_patch"),
                None
            ),
        ]
    );
}

#[test]
fn uses_exec_prompts_and_final_answers_instead_of_environment_messages() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/repo__worktrees/task");
    let session_file = format!("{home}/.codex/sessions/2026/codex-exec-session.jsonl");
    write_file(
        &session_file,
        &json_lines(&[
            json!({ "type": "session_meta", "timestamp": "2026-01-01T00:00:00.000Z", "payload": { "id": "codex-exec-session", "cwd": worktree_path, "timestamp": "2026-01-01T00:00:00.000Z", "source": "exec" } }),
            response_item(
                1,
                json!({ "type": "message", "role": "developer", "content": [{ "type": "input_text", "text": "<permissions instructions>\nSandbox metadata\n</permissions instructions>" }] }),
            ),
            response_item(
                2,
                user_text(&format!(
                    "<environment_context>\n  <cwd>{worktree_path}</cwd>\n</environment_context>"
                )),
            ),
            response_item(3, user_text("Run the headless smoke check.")),
            json!({ "type": "event_msg", "timestamp": "2026-01-01T00:04:00.000Z", "payload": { "type": "agent_message", "message": "KANVIBE_CODEX_HEADLESS_OK", "phase": "final_answer" } }),
        ]),
    );
    let mut query_context = context(local_host(&home), &worktree_path);
    query_context.query = Some("KANVIBE_CODEX_HEADLESS_OK".to_string());
    let sessions = read_codex_sessions(&query_context).unwrap();
    assert_eq!(
        (
            sessions.provider,
            sessions.available,
            sessions.session_count
        ),
        (AiSessionProvider::Codex, true, 1)
    );
    let session = &sessions.sessions[0];
    assert_eq!(session.id, "codex-exec-session");
    assert_eq!(
        session.title.as_deref(),
        Some("Run the headless smoke check.")
    );
    assert_eq!(
        session.first_user_prompt.as_deref(),
        Some("Run the headless smoke check.")
    );
    assert_eq!(session.message_count, 2);
    assert_eq!(session.source_ref.as_deref(), Some(session_file.as_str()));

    let plain_context = context(local_host(&home), &worktree_path);
    assert_eq!(
        role_texts(&detail(&plain_context, "codex-exec-session", &session_file)),
        [
            (
                AiMessageRole::Assistant,
                "KANVIBE_CODEX_HEADLESS_OK".to_string()
            ),
            (
                AiMessageRole::User,
                "Run the headless smoke check.".to_string()
            ),
        ]
    );
    let mut instruction_context = context(local_host(&home), &worktree_path);
    instruction_context.include_instructions = true;
    let with_instructions = detail(&instruction_context, "codex-exec-session", &session_file);
    assert_eq!(
        roles(&with_instructions),
        [
            AiMessageRole::Assistant,
            AiMessageRole::User,
            AiMessageRole::User,
            AiMessageRole::Developer,
        ]
    );
    assert!(
        with_instructions
            .messages
            .last()
            .unwrap()
            .full_text
            .contains("<permissions instructions>")
    );
}

#[test]
fn latest_user_or_assistant_message_sets_activity_time() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/repo__worktrees/task");
    write_file(
        &format!("{home}/.codex/sessions/2026/codex-activity.jsonl"),
        &json_lines(&[
            session_meta("codex-activity", &worktree_path),
            response_item(1, user_text("Update the chat.")),
            response_item(2, assistant_text("Chat updated.")),
            response_item(
                3,
                json!({ "type": "reasoning", "text": "Internal follow-up bookkeeping." }),
            ),
        ]),
    );
    let sessions = read_codex_sessions(&context(local_host(&home), &worktree_path)).unwrap();
    assert_eq!(
        sessions.sessions[0].updated_at.as_deref(),
        Some("2026-01-01T00:02:00.000Z")
    );
}

#[test]
fn reads_remote_worktree_sessions_and_detail_over_ssh() {
    let worktree_path = "/remote/repo__worktrees/task";
    let session_file = "/remote/home/.codex/sessions/2026/remote-codex.jsonl";
    let content = json_lines(&[
        session_meta("remote-codex", worktree_path),
        response_item(1, user_text("load repo codex history")),
        response_item(2, assistant_text("Codex history loaded remotely.")),
    ]);
    let runner = remote_file_system(&[(session_file, &content)]);
    let mut remote_context = context(remote_host(runner.clone(), "remote-host"), worktree_path);
    remote_context.repo_path = Some("/remote/repo".to_string());

    let sessions = read_codex_sessions(&remote_context).unwrap();
    assert_eq!((sessions.available, sessions.session_count), (true, 1));
    let session = &sessions.sessions[0];
    assert_eq!(session.id, "remote-codex");
    assert_eq!(session.matched_path.as_deref(), Some(worktree_path));
    assert_eq!(session.match_scope, AiSessionMatchScope::Worktree);
    assert_eq!(
        session.first_user_prompt.as_deref(),
        Some("load repo codex history")
    );
    assert_eq!(session.source_ref.as_deref(), Some(session_file));

    assert_eq!(
        role_texts(&detail(&remote_context, "remote-codex", session_file)),
        [
            (
                AiMessageRole::Assistant,
                "Codex history loaded remotely.".to_string()
            ),
            (AiMessageRole::User, "load repo codex history".to_string()),
        ]
    );
    let calls = runner.calls.lock().unwrap();
    assert!(!calls.is_empty());
    assert!(
        calls
            .iter()
            .all(|call| call.ssh_host.as_deref() == Some("remote-host"))
    );
}

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aRZkAAAAASUVORK5CYII=";
const IMAGE_WORKTREE: &str = "/qa/image-worktree";

fn image_rollout(events: &[Value]) -> (tempfile::TempDir, String) {
    let (directory, root) = temp_home();
    let file = format!("{root}/codex/images.jsonl");
    let lines: Vec<String> = events.iter().map(crate::javascript::stringify).collect();
    write_file(&file, &lines.join("\n"));
    (directory, file)
}

#[test]
fn image_only_rows_keep_distinct_media_identities_across_pages() {
    let (_directory, source) = image_rollout(&[
        json!({ "type": "session_meta", "payload": { "id": "image-session", "cwd": IMAGE_WORKTREE } }),
        json!({ "type": "response_item", "timestamp": "2026-11-13T01:00:00Z", "payload": { "type": "message", "role": "user", "content": [{ "type": "input_image", "image_url": format!("data:image/png;base64,{PNG}") }] } }),
        json!({ "type": "response_item", "timestamp": "2026-11-13T01:00:00Z", "payload": { "type": "message", "role": "user", "content": [{ "type": "local_image", "path": "/qa/second.png" }] } }),
    ]);
    let context = context(local_host("/unused-home"), IMAGE_WORKTREE);
    let first = read_codex_session_detail(&context, "image-session", Some(&source), None, Some(1))
        .unwrap()
        .unwrap();
    let second = read_codex_session_detail(
        &context,
        "image-session",
        Some(&source),
        first.next_cursor.as_deref(),
        Some(1),
    )
    .unwrap()
    .unwrap();
    assert_eq!(first.messages.len(), 1);
    assert_eq!(second.messages.len(), 1);
    assert_ne!(
        first.messages[0].attachment_list()[0].id,
        second.messages[0].attachment_list()[0].id
    );
    assert_eq!(
        second.messages[0].attachment_list()[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert!(!serde_json::to_string(&second).unwrap().contains(PNG));
}

#[test]
fn native_image_framing_becomes_attachment_and_clean_caption() {
    let (_directory, source) = image_rollout(&[
        json!({ "type": "session_meta", "payload": { "id": "image-session", "cwd": IMAGE_WORKTREE } }),
        json!({ "type": "response_item", "payload": { "type": "message", "role": "user", "content": [
            { "type": "input_text", "text": "<image name=[Image #1] path=\"/qa/owned image.png\">" },
            { "type": "input_image", "image_url": format!("data:image/png;base64,{PNG}"), "detail": "high" },
            { "type": "input_text", "text": "</image>" },
            { "type": "input_text", "text": "[Image #1] Inspect this" },
        ] } }),
    ]);
    let context = context(local_host("/unused-home"), IMAGE_WORKTREE);
    let detail = read_codex_session_detail(&context, "image-session", Some(&source), None, None)
        .unwrap()
        .unwrap();
    assert_eq!(detail.messages[0].full_text, "Inspect this");
    assert_eq!(detail.messages[0].attachment_list().len(), 1);
}

#[test]
fn unrelated_rollouts_are_excluded_before_loading_bodies() {
    let meta = crate::javascript::stringify(
        &json!({ "type": "session_meta", "payload": { "id": "foreign", "cwd": "/another/worktree" } }),
    );
    let runner = ScriptedRunner::new(move |command, _| {
        if command == "printf '%s' \"$HOME\"" {
            return Ok("/remote/home".to_string());
        }
        if command.contains(" && find ") {
            return Ok("/remote/foreign.jsonl".to_string());
        }
        if command.starts_with("test -e ") {
            return Ok("1".to_string());
        }
        if command.starts_with("head -n ") {
            return Ok(meta.clone());
        }
        Err(AiError::message(format!(
            "unexpected remote command: {command}"
        )))
    });
    let context = context(
        remote_host(runner.clone(), "foreign-host"),
        "/selected/worktree",
    );
    assert!(read_codex_sessions(&context).unwrap().sessions.is_empty());
    assert!(
        runner
            .commands()
            .iter()
            .all(|command| !command.contains(" cat "))
    );
}

#[test]
fn remote_header_read_transfers_only_requested_lines() {
    let runner = ScriptedRunner::new(|_, _| Ok("{\"type\":\"session_meta\"}\n".to_string()));
    let host = remote_host(runner.clone(), "remote");
    assert_eq!(
        read_json_lines_head(&host, "/remote/history.jsonl", 1).unwrap(),
        [json!({ "type": "session_meta" })]
    );
    assert_eq!(runner.commands(), ["head -n 1 '/remote/history.jsonl'"]);
}

/// TS `codexSessionDetailReuse.test.ts`의 고정 파일. 같은 틱 안의 수정도 파일 캐시가 알아채도록 수정 시각을 매번 1초씩 민다
struct ReuseFixture {
    _directory: tempfile::TempDir,
    worktree_path: String,
    rollout_file: String,
    next_modified_at: u64,
}

const REUSE_SESSION_ID: &str = "codex-reuse";

impl ReuseFixture {
    fn new() -> Self {
        let (directory, root) = temp_home();
        Self {
            _directory: directory,
            worktree_path: format!("{root}/repo"),
            rollout_file: format!("{root}/{REUSE_SESSION_ID}.jsonl"),
            next_modified_at: 1_800_000_000,
        }
    }

    fn meta(&self, session_id: &str) -> String {
        format!(
            "{}\n",
            crate::javascript::stringify(
                &json!({ "type": "session_meta", "timestamp": timestamp(0), "payload": { "id": session_id, "cwd": self.worktree_path } })
            )
        )
    }

    fn write(&mut self, content: &str) {
        std::fs::write(&self.rollout_file, content).unwrap();
        self.mark_modified();
    }

    fn append(&mut self, content: &str) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.rollout_file)
            .unwrap();
        file.write_all(content.as_bytes()).unwrap();
        self.mark_modified();
    }

    fn mark_modified(&mut self) {
        self.next_modified_at += 1;
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(self.next_modified_at);
        File::options()
            .write(true)
            .open(&self.rollout_file)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    fn read(&self, cursor: Option<&str>, limit: usize) -> Option<AggregatedAiSessionDetail> {
        let context = context(local_host("/unused-home"), &self.worktree_path);
        read_codex_session_detail(
            &context,
            REUSE_SESSION_ID,
            Some(&self.rollout_file),
            cursor,
            Some(limit),
        )
        .unwrap()
    }
}

fn timestamp(minute: u32) -> String {
    format!("2026-01-01T00:{minute:02}:00.000Z")
}

fn message(minute: u32, role: &str, text: &str) -> String {
    let content_type = if role == "user" {
        "input_text"
    } else {
        "output_text"
    };
    let event = json!({ "type": "response_item", "timestamp": timestamp(minute), "payload": { "type": "message", "role": role, "content": [{ "type": content_type, "text": text }] } });
    format!("{}\n", crate::javascript::stringify(&event))
}

fn texts(detail: &Option<AggregatedAiSessionDetail>) -> Vec<String> {
    detail
        .as_ref()
        .unwrap()
        .messages
        .iter()
        .map(|message| message.full_text.clone())
        .collect()
}

#[test]
fn unchanged_rollout_returns_same_detail_without_rebuilding() {
    let mut fixture = ReuseFixture::new();
    let content = fixture.meta(REUSE_SESSION_ID)
        + &message(1, "user", "첫 질문")
        + &message(2, "assistant", "첫 답");
    fixture.write(&content);
    let first = fixture.read(None, 20);
    let built_after_first = built_message_count();
    let second = fixture.read(None, 20);
    assert_eq!(texts(&second), ["첫 답", "첫 질문"]);
    assert_eq!(second, first);
    assert_eq!(built_message_count(), built_after_first);
}

#[test]
fn appended_lines_are_added_newest_first_without_rebuilding_older_messages() {
    let mut fixture = ReuseFixture::new();
    let content = fixture.meta(REUSE_SESSION_ID)
        + &message(1, "user", "첫 질문")
        + &message(2, "assistant", "첫 답");
    fixture.write(&content);
    let first = fixture.read(None, 20);
    let built_after_first = built_message_count();
    fixture.append(&(message(3, "user", "다음 질문") + &message(4, "assistant", "다음 답")));
    let second = fixture.read(None, 20);
    assert_eq!(texts(&second), ["다음 답", "다음 질문", "첫 답", "첫 질문"]);
    assert_eq!(
        second.as_ref().unwrap().messages[2],
        first.as_ref().unwrap().messages[0]
    );
    assert_eq!(built_message_count(), built_after_first + 2);
    assert_eq!(second.unwrap().title.as_deref(), Some("첫 질문"));
}

#[test]
fn repeated_message_after_append_is_kept_once() {
    let mut fixture = ReuseFixture::new();
    let content = fixture.meta(REUSE_SESSION_ID) + &message(1, "user", "같은 질문");
    fixture.write(&content);
    fixture.read(None, 20);
    fixture.append(&(message(1, "user", "같은 질문") + &message(2, "assistant", "답")));
    assert_eq!(texts(&fixture.read(None, 20)), ["답", "같은 질문"]);
}

#[test]
fn other_conversation_stays_empty_after_append() {
    let mut fixture = ReuseFixture::new();
    let content = fixture.meta("another-session") + &message(1, "user", "남의 질문");
    fixture.write(&content);
    assert_eq!(fixture.read(None, 20), None);
    fixture.append(&message(2, "assistant", "남의 답"));
    assert_eq!(fixture.read(None, 20), None);
}

#[test]
fn shorter_rewrite_drops_previous_messages() {
    let mut fixture = ReuseFixture::new();
    let content = fixture.meta(REUSE_SESSION_ID)
        + &message(1, "user", "지워질 질문")
        + &message(2, "assistant", "지워질 답");
    fixture.write(&content);
    fixture.read(None, 20);
    let rewritten = fixture.meta(REUSE_SESSION_ID) + &message(5, "user", "새 질문");
    fixture.write(&rewritten);
    assert_eq!(texts(&fixture.read(None, 20)), ["새 질문"]);
}

#[test]
fn previous_page_cursor_points_to_same_order_after_append() {
    let mut fixture = ReuseFixture::new();
    let content = fixture.meta(REUSE_SESSION_ID)
        + &message(1, "user", "하나")
        + &message(2, "assistant", "둘")
        + &message(3, "user", "셋");
    fixture.write(&content);
    fixture.append(&message(4, "assistant", "넷"));
    let latest = fixture.read(None, 2);
    assert_eq!(texts(&latest), ["넷", "셋"]);
    let older = fixture.read(latest.as_ref().unwrap().next_cursor.as_deref(), 2);
    assert_eq!(texts(&older), ["둘", "하나"]);
    assert_eq!(older.unwrap().next_cursor, None);
}

fn live_windows() -> LiveAiSessionWindows {
    LiveAiSessionWindows {
        running_window_ms: 90_000,
        recent_window_ms: 6 * 60 * 60 * 1000,
    }
}

fn file_mtime(file_path: &str) -> f64 {
    crate::host::local_mtime_ms(file_path).unwrap()
}

#[test]
fn live_sessions_pair_main_threads_with_recent_subagents() {
    let (_directory, home) = temp_home();
    let worktree_path = format!("{home}/task");
    let main_file = format!("{home}/.codex/sessions/rollout-main-{THREAD_ID}.jsonl");
    let child_file = format!("{home}/.codex/sessions/rollout-child-{OTHER_THREAD_ID}.jsonl");
    write_file(
        &main_file,
        &json_lines(&[
            json!({ "type": "session_meta", "payload": { "id": THREAD_ID, "cwd": worktree_path, "timestamp": "2026-01-01T00:00:00.000Z" } }),
            response_item(1, user_text("요청")),
            response_item(2, assistant_text("작업 중")),
        ]),
    );
    write_file(
        &child_file,
        &json_lines(&[
            json!({ "type": "session_meta", "payload": { "id": OTHER_THREAD_ID, "parent_thread_id": THREAD_ID, "agent_nickname": "helper", "cwd": worktree_path, "timestamp": "2026-01-01T00:01:00.000Z" } }),
        ]),
    );
    let context = context(local_host(&home), &worktree_path);
    let now_ms = file_mtime(&main_file).max(file_mtime(&child_file)) + 1_000.0;

    let snapshots = read_codex_live_sessions(&context, live_windows(), now_ms).unwrap();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].session_id.as_deref(), Some(THREAD_ID));
    assert_eq!(snapshots[0].current_task.as_deref(), Some("작업 중"));
    assert_eq!(snapshots[0].running_subtasks.len(), 1);
    assert_eq!(
        snapshots[0].running_subtasks[0].name.as_deref(),
        Some("helper")
    );

    let graph = read_codex_agent_call_graph(&context, THREAD_ID, live_windows(), now_ms).unwrap();
    assert_eq!(graph.len(), 1);
    assert_eq!(graph[0].task.as_deref(), Some("helper"));
    assert_eq!(graph[0].ended_at, None);
    assert_eq!(
        graph[0].started_at.as_deref(),
        Some("2026-01-01T00:01:00.000Z")
    );
    let later =
        read_codex_agent_call_graph(&context, THREAD_ID, live_windows(), now_ms + 200_000.0)
            .unwrap();
    assert!(later[0].ended_at.is_some());
}
