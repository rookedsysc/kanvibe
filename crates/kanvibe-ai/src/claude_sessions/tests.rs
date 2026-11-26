use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{Value, json};

use super::*;
use crate::host::{CommandOptions, CommandRunner, SessionHost};
use crate::javascript::stringify;
use crate::test_support::{
    json_lines, local_host, remote_file_system, remote_host, temp_home, write_file,
};
use crate::types::{AiAttachmentKind, AiFileChange, AiMessageFile, LiveAiSessionWindows};

const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aRZkAAAAASUVORK5CYII=";

fn context_for(host: SessionHost, worktree_path: &str) -> AiSessionReaderContext {
    let mut context = AiSessionReaderContext::new(host, Some(worktree_path));
    context.repo_path = Some(worktree_path.to_string());
    context
}

/// TS 테스트의 `claudeProjectDirectoryName`
fn claude_project_directory_name(target_path: &str) -> String {
    to_claude_project_dir_name(target_path)
}

fn write_json_lines(file_path: &str, values: &[Value]) {
    write_file(file_path, &json_lines(values));
}

/// 줄바꿈으로 잇되 끝에는 붙이지 않는다(TS `events.map(JSON.stringify).join("\n")`)
fn write_joined_events(file_path: &str, values: &[Value]) {
    let lines: Vec<String> = values.iter().map(stringify).collect();
    write_file(file_path, &lines.join("\n"));
}

fn detail_pairs(detail: &Option<AggregatedAiSessionDetail>) -> Vec<(AiMessageRole, String)> {
    detail
        .iter()
        .flat_map(|detail| detail.messages.iter())
        .map(|message| (message.role, message.full_text.clone()))
        .collect()
}

fn full_texts(detail: &Option<AggregatedAiSessionDetail>) -> Vec<String> {
    detail_pairs(detail)
        .into_iter()
        .map(|(_, text)| text)
        .collect()
}

fn read_source_detail(
    context: &AiSessionReaderContext,
    session_id: &str,
    source: &str,
) -> Option<AggregatedAiSessionDetail> {
    read_claude_session_detail(context, session_id, Some(source), None, None).unwrap()
}

mod recorded_user_text {
    use super::*;

    const WORKTREE: &str = "/qa/recorded-text-worktree";

    /// Claude Code 2.1.326이 실제로 남긴 모양의 사용자 줄들을 대화 기록으로 읽는다. 최신 메시지가 먼저 온다
    fn read_recorded_user_texts(contents: &[&str]) -> Vec<String> {
        let (_directory, root) = temp_home();
        let source = format!("{root}/session.jsonl");
        let events: Vec<Value> = contents
            .iter()
            .enumerate()
            .map(|(index, content)| json!({"type": "user", "sessionId": "recorded-text", "cwd": WORKTREE, "timestamp": format!("2026-11-17T01:00:0{index}Z"), "message": {"role": "user", "content": content}}))
            .collect();
        write_joined_events(&source, &events);
        let context = context_for(local_host(&root), WORKTREE);
        full_texts(&read_source_detail(&context, "recorded-text", &source))
    }

    #[test]
    fn unwraps_folded_pastes_to_the_sent_text() {
        let texts = read_recorded_user_texts(&[
            "\n\n<pasted_content id=\"71e5\">\nline 1\nline 2\nline 3\nline 4\n</pasted_content id=\"71e5\">\n",
            "Compare these:\n\n<pasted_content id=\"0a1b\">\nfirst\nsecond\n</pasted_content id=\"0a1b\">\n",
        ]);
        assert_eq!(
            texts,
            [
                "Compare these:\n\nfirst\nsecond",
                "line 1\nline 2\nline 3\nline 4"
            ]
        );
    }

    #[test]
    fn recorded_shell_command_reads_as_bang_command() {
        assert_eq!(
            read_recorded_user_texts(&["<bash-input>echo hi-from-chat</bash-input>"]),
            ["!echo hi-from-chat"]
        );
    }

    #[test]
    fn mismatched_paste_ids_stay_literal() {
        let literal = "<pasted_content id=\"1\">\ntext\n</pasted_content id=\"2\">";
        assert_eq!(read_recorded_user_texts(&[literal]), [literal]);
    }
}

mod detail_reuse {
    use super::*;

    const SESSION_ID: &str = "claude-reuse";

    struct ReuseFixture {
        _directory: tempfile::TempDir,
        context: AiSessionReaderContext,
        worktree_path: String,
        session_file: String,
        next_modified_at: u64,
    }

    impl ReuseFixture {
        fn new() -> Self {
            let (directory, root) = temp_home();
            let worktree_path = format!("{root}/repo");
            Self {
                context: context_for(local_host(&root), &worktree_path),
                session_file: format!("{root}/{SESSION_ID}.jsonl"),
                worktree_path,
                _directory: directory,
                next_modified_at: 1_800_000_000,
            }
        }

        fn entry_for(&self, minute: u32, role: &str, text: &str, session_id: &str) -> String {
            let event = json!({"type": role, "sessionId": session_id, "cwd": self.worktree_path, "timestamp": format!("2026-01-01T00:{minute:02}:00.000Z"), "message": {"role": role, "content": [{"type": "text", "text": text}]}});
            format!("{}\n", stringify(&event))
        }

        fn entry(&self, minute: u32, role: &str, text: &str) -> String {
            self.entry_for(minute, role, text, SESSION_ID)
        }

        fn record(&self, fields: Value) -> String {
            let mut event = json!({"sessionId": SESSION_ID});
            event
                .as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            format!("{}\n", stringify(&event))
        }

        fn write(&mut self, content: &str) {
            std::fs::write(&self.session_file, content).unwrap();
            self.mark_modified();
        }

        fn append(&mut self, content: &str) {
            let mut file = File::options()
                .append(true)
                .open(&self.session_file)
                .unwrap();
            file.write_all(content.as_bytes()).unwrap();
            self.mark_modified();
        }

        /// 같은 순간의 수정도 파일 캐시가 알아채도록 수정 시각을 매번 1초씩 밀어 준다
        fn mark_modified(&mut self) {
            self.next_modified_at += 1;
            let file = File::options()
                .write(true)
                .open(&self.session_file)
                .unwrap();
            file.set_modified(UNIX_EPOCH + Duration::from_secs(self.next_modified_at))
                .unwrap();
        }

        fn read(&self, cursor: Option<&str>, limit: usize) -> Option<AggregatedAiSessionDetail> {
            read_claude_session_detail(
                &self.context,
                SESSION_ID,
                Some(&self.session_file),
                cursor,
                Some(limit),
            )
            .unwrap()
        }
    }

    #[test]
    fn unchanged_history_returns_the_same_detail() {
        let mut fixture = ReuseFixture::new();
        let content = fixture.entry(1, "user", "첫 질문") + &fixture.entry(2, "assistant", "첫 답");
        fixture.write(&content);
        let first = fixture.read(None, 20);
        let second = fixture.read(None, 20);
        assert_eq!(full_texts(&second), ["첫 답", "첫 질문"]);
        assert_eq!(second, first);
    }

    #[test]
    fn appended_lines_lead_the_newest_first_page() {
        let mut fixture = ReuseFixture::new();
        let content = fixture.entry(1, "user", "첫 질문") + &fixture.entry(2, "assistant", "첫 답");
        fixture.write(&content);
        let first = fixture.read(None, 20).unwrap();
        let appended =
            fixture.entry(3, "user", "다음 질문") + &fixture.entry(4, "assistant", "다음 답");
        fixture.append(&appended);
        let second = fixture.read(None, 20);
        assert_eq!(
            full_texts(&second),
            ["다음 답", "다음 질문", "첫 답", "첫 질문"]
        );
        let second = second.unwrap();
        assert_eq!(second.messages[2], first.messages[0]);
        assert_eq!(second.title.as_deref(), Some("첫 질문"));
    }

    #[test]
    fn half_written_line_waits_until_complete() {
        let mut fixture = ReuseFixture::new();
        let unfinished = fixture.entry(2, "assistant", "쓰는 중인 답");
        let content = fixture.entry(1, "user", "첫 질문") + &unfinished[..40];
        fixture.write(&content);
        assert_eq!(full_texts(&fixture.read(None, 20)), ["첫 질문"]);
        fixture.append(&unfinished[40..]);
        assert_eq!(
            full_texts(&fixture.read(None, 20)),
            ["쓰는 중인 답", "첫 질문"]
        );
    }

    #[test]
    fn shorter_rewrite_drops_previous_messages() {
        let mut fixture = ReuseFixture::new();
        let content =
            fixture.entry(1, "user", "지워질 질문") + &fixture.entry(2, "assistant", "지워질 답");
        fixture.write(&content);
        fixture.read(None, 20);
        let rewritten = fixture.entry(5, "user", "새 질문");
        fixture.write(&rewritten);
        let detail = fixture.read(None, 20);
        assert_eq!(full_texts(&detail), ["새 질문"]);
        assert_eq!(detail.unwrap().title.as_deref(), Some("새 질문"));
    }

    #[test]
    fn page_cursor_keeps_order_after_append() {
        let mut fixture = ReuseFixture::new();
        let content = fixture.entry(1, "user", "하나")
            + &fixture.entry(2, "assistant", "둘")
            + &fixture.entry(3, "user", "셋");
        fixture.write(&content);
        let latest = fixture.read(None, 2);
        assert_eq!(full_texts(&latest), ["셋", "둘"]);
        let cursor = latest.unwrap().next_cursor;
        assert_eq!(full_texts(&fixture.read(cursor.as_deref(), 2)), ["하나"]);
        let appended = fixture.entry(4, "assistant", "넷");
        fixture.append(&appended);
        let after_append = fixture.read(None, 2);
        assert_eq!(full_texts(&after_append), ["넷", "셋"]);
        let older = fixture.read(after_append.unwrap().next_cursor.as_deref(), 2);
        assert_eq!(full_texts(&older), ["둘", "하나"]);
        assert_eq!(older.unwrap().next_cursor, None);
    }

    #[test]
    fn other_conversation_lines_stay_out_after_append() {
        let mut fixture = ReuseFixture::new();
        let content = fixture.entry(1, "user", "내 질문")
            + &fixture.entry_for(2, "user", "남의 질문", "other-session");
        fixture.write(&content);
        fixture.read(None, 20);
        let appended = fixture.entry_for(3, "assistant", "남의 답", "other-session")
            + &fixture.entry(4, "assistant", "내 답");
        fixture.append(&appended);
        assert_eq!(full_texts(&fixture.read(None, 20)), ["내 답", "내 질문"]);
    }

    #[test]
    fn api_request_logs_are_not_messages() {
        let mut fixture = ReuseFixture::new();
        let request_copy = json!([{"type": "text", "text": "<system-reminder>저장소 지침</system-reminder>"}, {"type": "text", "text": "질문"}]);
        let content = fixture.entry(1, "user", "질문")
            + &fixture.record(json!({"type": "api-request-blob", "hash": "request-user", "message": {"role": "user", "content": request_copy}}))
            + &fixture.record(json!({"type": "api-request-blob", "hash": "request-system", "message": {"role": "system", "content": [{"type": "text", "text": "hook 출력"}]}}))
            + &fixture.entry(2, "assistant", "답");
        fixture.write(&content);
        assert_eq!(full_texts(&fixture.read(None, 20)), ["답", "질문"]);
    }

    #[test]
    fn meta_user_lines_become_system_role() {
        let mut fixture = ReuseFixture::new();
        let skill_body = "Base directory for this skill: /skills/example";
        let content = fixture.entry(1, "user", "질문")
            + &fixture.record(json!({"type": "user", "isMeta": true, "cwd": fixture.worktree_path, "timestamp": "2026-01-01T00:02:00.000Z", "message": {"role": "user", "content": skill_body}}));
        fixture.write(&content);
        assert_eq!(
            detail_pairs(&fixture.read(None, 20)),
            [
                (AiMessageRole::System, skill_body.to_string()),
                (AiMessageRole::User, "질문".to_string())
            ]
        );
    }
}

mod history_readers {
    use super::*;

    fn session_file_for(home: &str, worktree_path: &str, file_name: &str) -> String {
        format!(
            "{home}/.claude/projects/{}/{file_name}",
            claude_project_directory_name(worktree_path)
        )
    }

    #[test]
    fn classifies_user_system_assistant_and_tool_results() {
        let (_directory, home) = temp_home();
        let worktree_path = format!("{home}/repo__worktrees/task");
        let session_file = session_file_for(&home, &worktree_path, "claude-session.jsonl");
        let event = |kind: &str, minute: u32, message: Value| json!({"type": kind, "sessionId": "claude-session", "cwd": worktree_path, "timestamp": format!("2026-01-01T00:0{minute}:00.000Z"), "message": message});
        write_json_lines(
            &session_file,
            &[
                event(
                    "system",
                    0,
                    json!({"role": "system", "content": "You are Claude Code."}),
                ),
                event(
                    "user",
                    1,
                    json!({"role": "user", "content": [{"type": "text", "text": "fix the bug"}]}),
                ),
                event(
                    "assistant",
                    2,
                    json!({"role": "assistant", "content": [{"type": "text", "text": "I fixed it."}]}),
                ),
                event(
                    "user",
                    3,
                    json!({"role": "user", "content": [{"type": "tool_result", "content": "file contents"}]}),
                ),
            ],
        );
        let context = context_for(local_host(&home), &worktree_path);

        let sessions = read_claude_sessions(&context).unwrap();
        assert_eq!(
            sessions.sessions[0].updated_at.as_deref(),
            Some("2026-01-01T00:02:00.000Z")
        );

        let detail = read_claude_session_detail(
            &context,
            "claude-session",
            Some(&session_file),
            None,
            Some(20),
        )
        .unwrap();
        assert_eq!(
            detail_pairs(&detail),
            [
                (AiMessageRole::Tool, "file contents".to_string()),
                (AiMessageRole::Assistant, "I fixed it.".to_string()),
                (AiMessageRole::User, "fix the bug".to_string()),
                (AiMessageRole::System, "You are Claude Code.".to_string()),
            ]
        );
    }

    #[test]
    fn tool_calls_and_thinking_become_ordered_messages_with_files() {
        let (_directory, home) = temp_home();
        let worktree_path = format!("{home}/repo__worktrees/task");
        let session_file = session_file_for(&home, &worktree_path, "claude-tools.jsonl");
        let created_path = format!("{worktree_path}/docs/plan.md");
        let modified_path = format!("{worktree_path}/src/app.ts");
        let assistant_entry = |timestamp: &str, content: Value| json!({"type": "assistant", "sessionId": "claude-tools", "cwd": worktree_path, "timestamp": timestamp, "message": {"role": "assistant", "content": content}});
        write_json_lines(
            &session_file,
            &[
                assistant_entry(
                    "2026-01-01T00:01:00.000Z",
                    json!([{"type": "tool_use", "id": "toolu_1", "name": "Write", "input": {"file_path": created_path, "content": "# plan"}}]),
                ),
                assistant_entry(
                    "2026-01-01T00:02:00.000Z",
                    json!([{"type": "tool_use", "id": "toolu_2", "name": "Edit", "input": {"file_path": modified_path, "old_string": "a", "new_string": "b"}}]),
                ),
                assistant_entry(
                    "2026-01-01T00:03:00.000Z",
                    json!([
                        {"type": "thinking", "thinking": "어디를 고칠지 본다", "signature": "sig"},
                        {"type": "text", "text": "테스트를 돌립니다."},
                        {"type": "tool_use", "id": "toolu_3", "name": "Bash", "input": {"command": "pnpm test", "description": "테스트 실행"}}
                    ]),
                ),
                assistant_entry(
                    "2026-01-01T00:04:00.000Z",
                    json!([{"type": "thinking", "thinking": "", "signature": "sig"}]),
                ),
            ],
        );
        let context = context_for(local_host(&home), &worktree_path);
        let detail = read_claude_session_detail(
            &context,
            "claude-tools",
            Some(&session_file),
            None,
            Some(20),
        )
        .unwrap()
        .unwrap();
        let messages: Vec<_> = detail.messages.iter().rev().collect();
        let summary: Vec<_> = messages
            .iter()
            .map(|message| {
                (
                    message.role,
                    message.full_text.as_str(),
                    message.tool_name.as_deref(),
                    message.files.clone(),
                )
            })
            .collect();
        let file = |path: &str, change| {
            Some(vec![AiMessageFile {
                path: path.to_string(),
                change,
            }])
        };
        assert_eq!(
            summary,
            [
                (
                    AiMessageRole::Tool,
                    created_path.as_str(),
                    Some("Write"),
                    file(&created_path, AiFileChange::Created)
                ),
                (
                    AiMessageRole::Tool,
                    modified_path.as_str(),
                    Some("Edit"),
                    file(&modified_path, AiFileChange::Modified)
                ),
                (AiMessageRole::Reasoning, "어디를 고칠지 본다", None, None),
                (AiMessageRole::Assistant, "테스트를 돌립니다.", None, None),
                (AiMessageRole::Tool, "pnpm test", Some("Bash"), None),
            ]
        );
        assert!(
            !serde_json::to_value(messages[4])
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("files")
        );
    }

    #[test]
    fn discovers_sessions_by_cwd_when_encoded_directory_misses() {
        let (_directory, home) = temp_home();
        let worktree_path = format!("{home}/repo__worktrees/task");
        let session_file =
            format!("{home}/.claude/projects/legacy-or-aliased-project-dir/claude-session.jsonl");
        let event = |kind: &str, minute: u32, text: &str| json!({"type": kind, "sessionId": "claude-aliased-session", "cwd": worktree_path, "timestamp": format!("2026-01-01T00:0{minute}:00.000Z"), "message": {"role": kind, "content": [{"type": "text", "text": text}]}});
        write_json_lines(
            &session_file,
            &[
                event("user", 1, "load aliased claude history"),
                event("assistant", 2, "aliased history loaded"),
            ],
        );

        let sessions = read_claude_sessions(&context_for(local_host(&home), &worktree_path))
            .unwrap()
            .sessions;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "claude-aliased-session");
        assert_eq!(sessions[0].provider, AiSessionProvider::Claude);
        assert_eq!(
            sessions[0].matched_path.as_deref(),
            Some(worktree_path.as_str())
        );
        assert_eq!(
            sessions[0].first_user_prompt.as_deref(),
            Some("load aliased claude history")
        );
        assert_eq!(
            sessions[0].source_ref.as_deref(),
            Some(session_file.as_str())
        );
    }

    #[test]
    fn reads_latest_conversation_time_beyond_summary_head() {
        let (_directory, home) = temp_home();
        let worktree_path = format!("{home}/repo__worktrees/task");
        let session_file = session_file_for(&home, &worktree_path, "long-claude-session.jsonl");
        let mut events = vec![
            json!({"type": "user", "sessionId": "long-claude-session", "cwd": worktree_path, "timestamp": "2026-01-01T00:00:00.000Z", "message": {"role": "user", "content": [{"type": "text", "text": "Start the long task."}]}}),
            json!({"type": "assistant", "sessionId": "long-claude-session", "cwd": worktree_path, "timestamp": "2026-01-01T00:10:00.000Z", "message": {"role": "assistant", "content": [{"type": "text", "text": "The long task is complete."}]}}),
        ];
        events.extend((0..65).map(|index| json!({"type": "progress", "sessionId": "long-claude-session", "cwd": worktree_path, "timestamp": format!("2026-01-01T00:00:{:02}.000Z", index + 2), "message": {"role": "user", "content": [{"type": "tool_result", "content": format!("tool {index} {}", "x".repeat(4096))}]}})));
        write_json_lines(&session_file, &events);

        let sessions =
            read_claude_sessions(&context_for(local_host(&home), &worktree_path)).unwrap();
        assert_eq!(
            sessions.sessions[0].updated_at.as_deref(),
            Some("2026-01-01T00:10:00.000Z")
        );
    }

    #[test]
    fn reads_remote_sessions_and_detail_over_ssh() {
        let worktree_path = "/remote/repo__worktrees/task";
        let session_file = format!(
            "/remote/home/.claude/projects/{}/remote-claude.jsonl",
            claude_project_directory_name(worktree_path)
        );
        let event = |kind: &str, minute: u32, text: &str| json!({"type": kind, "sessionId": "remote-claude", "cwd": worktree_path, "timestamp": format!("2026-01-01T00:0{minute}:00.000Z"), "message": {"role": kind, "content": [{"type": "text", "text": text}]}});
        let content = json_lines(&[
            event("user", 1, "load repo claude history"),
            event("assistant", 2, "Claude history loaded remotely."),
        ]);
        let runner = remote_file_system(&[(session_file.as_str(), content.as_str())]);
        let mut context = context_for(remote_host(runner.clone(), "remote-host"), worktree_path);
        context.repo_path = Some("/remote/repo".to_string());

        let sessions = read_claude_sessions(&context).unwrap();
        assert!(sessions.available);
        assert_eq!(sessions.session_count, 1);
        let session = &sessions.sessions[0];
        assert_eq!(session.id, "remote-claude");
        assert_eq!(session.matched_path.as_deref(), Some(worktree_path));
        assert_eq!(session.match_scope, AiSessionMatchScope::Worktree);
        assert_eq!(
            session.first_user_prompt.as_deref(),
            Some("load repo claude history")
        );
        assert_eq!(session.source_ref.as_deref(), Some(session_file.as_str()));

        let detail = read_claude_session_detail(
            &context,
            "remote-claude",
            Some(&session_file),
            None,
            Some(20),
        )
        .unwrap();
        assert_eq!(
            detail_pairs(&detail),
            [
                (
                    AiMessageRole::Assistant,
                    "Claude history loaded remotely.".to_string()
                ),
                (AiMessageRole::User, "load repo claude history".to_string()),
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
}

mod image_history {
    use super::*;

    const WORKTREE: &str = "/qa/image-worktree";

    fn rollout(root: &str, events: &[Value]) -> String {
        let file = format!("{root}/claude/images.jsonl");
        write_joined_events(&file, events);
        file
    }

    fn user_event(timestamp: &str, content: Value) -> Value {
        json!({"type": "user", "sessionId": "image-session", "cwd": WORKTREE, "timestamp": timestamp, "message": {"role": "user", "content": content}})
    }

    fn image_part() -> Value {
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": PNG}})
    }

    #[test]
    fn omits_native_image_companion_and_keeps_literal_text() {
        let (_directory, root) = temp_home();
        let mut companion = user_event(
            "2026-11-13T01:00:01Z",
            json!([{"type": "text", "text": "[Image: source: /qa/private-staging/image.png]"}]),
        );
        companion["isMeta"] = json!(true);
        companion["turnCompanion"] = json!(true);
        let source = rollout(
            &root,
            &[
                user_event(
                    "2026-11-13T01:00:00Z",
                    json!([{"type": "text", "text": "Caption"}, image_part()]),
                ),
                companion,
                user_event(
                    "2026-11-13T01:00:02Z",
                    json!("[Image: source: literal text]"),
                ),
            ],
        );
        let detail = read_source_detail(
            &context_for(local_host(&root), WORKTREE),
            "image-session",
            &source,
        );
        assert_eq!(
            full_texts(&detail),
            ["[Image: source: literal text]", "Caption"]
        );
        assert_eq!(
            detail.as_ref().unwrap().messages[1].attachment_list().len(),
            1
        );
        assert!(
            !serde_json::to_string(&detail)
                .unwrap()
                .contains("private-staging")
        );
    }

    #[test]
    fn companions_stay_out_of_list_counts_search_and_current_activity() {
        let (_directory, home) = temp_home();
        let source = format!("{home}/.claude/projects/-qa-image-worktree/image-session.jsonl");
        let mut companion = user_event(
            "2026-11-13T01:00:01Z",
            json!("[Image: source: /qa/private-staging/image.png]"),
        );
        companion["isMeta"] = json!(true);
        companion["turnCompanion"] = json!(true);
        write_joined_events(
            &source,
            &[
                user_event("2026-11-13T01:00:00Z", json!("Caption")),
                companion,
            ],
        );
        let context = context_for(local_host(&home), WORKTREE);

        let sessions = read_claude_sessions(&context).unwrap().sessions;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].message_count, 1);
        assert_eq!(sessions[0].title.as_deref(), Some("Caption"));
        assert_eq!(
            sessions[0].updated_at.as_deref(),
            Some("2026-11-13T01:00:00.000Z")
        );

        let mut search_context = context.clone();
        search_context.query = Some("private-staging".to_string());
        assert!(
            read_claude_sessions(&search_context)
                .unwrap()
                .sessions
                .is_empty()
        );

        let windows = LiveAiSessionWindows {
            running_window_ms: 60_000,
            recent_window_ms: 60_000,
        };
        let live =
            read_claude_live_sessions(&context, windows, crate::javascript::now_ms()).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].current_task.as_deref(), Some("Caption"));
    }

    #[test]
    fn keeps_image_only_messages_with_lazy_metadata() {
        let (_directory, root) = temp_home();
        let source = rollout(
            &root,
            &[
                user_event("2026-11-13T01:00:00Z", json!([image_part()])),
                user_event(
                    "2026-11-13T01:00:01Z",
                    json!([{"type": "text", "text": "Inspect this"}, image_part()]),
                ),
            ],
        );
        let detail = read_source_detail(
            &context_for(local_host(&root), WORKTREE),
            "image-session",
            &source,
        );
        assert_eq!(full_texts(&detail), ["Inspect this", ""]);
        let attachments = detail.as_ref().unwrap().messages[1]
            .attachment_list()
            .to_vec();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].kind, AiAttachmentKind::Image);
        assert_eq!(attachments[0].mime_type.as_deref(), Some("image/png"));
        assert!(!attachments[0].id.is_empty());
        assert!(!serde_json::to_string(&detail).unwrap().contains(PNG));
    }

    #[test]
    fn foreign_cwd_is_not_inherited_by_rows_without_cwd() {
        let (_directory, root) = temp_home();
        let secret = format!("{root}/owned-secret.txt");
        std::fs::write(&secret, "Owned foreign task bytes").unwrap();
        let source = rollout(
            &root,
            &[
                json!({"type": "user", "sessionId": "foreign-session", "cwd": "/qa/foreign-worktree", "message": {"role": "user", "content": "Foreign task"}}),
                json!({"type": "user", "sessionId": "foreign-session", "message": {"role": "user", "content": [{"type": "file", "path": secret}]}}),
            ],
        );
        assert_eq!(
            read_source_detail(
                &context_for(local_host(&root), WORKTREE),
                "foreign-session",
                &source
            ),
            None
        );
    }

    #[test]
    fn cleans_image_paste_markers_but_keeps_literal_text() {
        let (_directory, root) = temp_home();
        let mut pasted = user_event(
            "2026-11-13T01:00:00Z",
            json!([{"type": "text", "text": "[Image #1]Caption"}, image_part()]),
        );
        pasted["imagePasteIds"] = json!([1]);
        let source = rollout(
            &root,
            &[
                pasted,
                user_event("2026-11-13T01:00:01Z", json!("[Image #1] literal text")),
            ],
        );
        let detail = read_source_detail(
            &context_for(local_host(&root), WORKTREE),
            "image-session",
            &source,
        );
        assert_eq!(full_texts(&detail), ["[Image #1] literal text", "Caption"]);
    }
}

mod live_and_graph {
    use super::*;

    const WINDOWS: LiveAiSessionWindows = LiveAiSessionWindows {
        running_window_ms: 90_000,
        recent_window_ms: 6 * 60 * 60 * 1000,
    };

    fn set_mtime(file_path: &str, seconds: u64) {
        let file = File::options().write(true).open(file_path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn live_snapshot_reads_last_answer_and_running_subagents() {
        let (_directory, home) = temp_home();
        let worktree_path = format!("{home}/repo");
        let project = format!(
            "{home}/.claude/projects/{}",
            claude_project_directory_name(&worktree_path)
        );
        let session_file = format!("{project}/session-a.jsonl");
        write_json_lines(
            &session_file,
            &[
                json!({"message": {"role": "user", "content": "do it"}}),
                json!({"message": {"role": "assistant", "content": [{"type": "text", "text": "working on it"}]}}),
            ],
        );
        let subagent = format!("{project}/session-a/subagents/agent-abc.jsonl");
        write_json_lines(
            &subagent,
            &[json!({"message": {"role": "user", "content": "Explore the repo"}})],
        );
        set_mtime(&session_file, 1_800_000_000);
        set_mtime(&subagent, 1_800_000_000);

        let context = context_for(local_host(&home), &worktree_path);
        let live = read_claude_live_sessions(&context, WINDOWS, 1_800_000_010_000.0).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].session_id.as_deref(), Some("session-a"));
        assert_eq!(live[0].current_task.as_deref(), Some("working on it"));
        assert_eq!(
            live[0].last_active_at.as_deref(),
            Some("2027-01-15T08:00:00.000Z")
        );
        assert_eq!(live[0].running_subtasks[0].id, "abc");
        assert_eq!(
            live[0].running_subtasks[0].name.as_deref(),
            Some("Explore the repo")
        );
    }

    #[test]
    fn call_graph_links_finished_and_running_subagents() {
        let (_directory, home) = temp_home();
        let worktree_path = format!("{home}/repo");
        let project = format!(
            "{home}/.claude/projects/{}",
            claude_project_directory_name(&worktree_path)
        );
        write_json_lines(
            &format!("{project}/s1.jsonl"),
            &[
                json!({"timestamp": "2026-01-01T00:00:00.000Z", "message": {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "t1", "name": "Agent", "input": {"subagent_type": "Explore", "description": "find files", "prompt": "find"}},
                    {"type": "tool_use", "id": "t2", "name": "Task", "input": {"prompt": "still running"}}
                ]}}),
                json!({"timestamp": "2026-01-01T00:05:00.000Z", "toolUseResult": {"agentId": "a1"}, "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1"}]}}),
            ],
        );
        let first = format!("{project}/s1/subagents/agent-a1.jsonl");
        let second = format!("{project}/s1/subagents/agent-a2.jsonl");
        write_json_lines(
            &first,
            &[
                json!({"agentId": "a1", "attributionSkill": "search", "message": {"role": "user", "content": "find"}}),
            ],
        );
        write_json_lines(
            &second,
            &[json!({"agentId": "a2", "message": {"role": "user", "content": "still running"}})],
        );
        set_mtime(&first, 1_800_000_000);
        set_mtime(&second, 1_800_000_000);

        let context = context_for(local_host(&home), &worktree_path);
        let roots =
            read_claude_agent_call_graph(&context, "s1", WINDOWS, 1_800_000_200_000.0).unwrap();
        assert_eq!(roots.len(), 2);
        assert_eq!(
            (
                roots[0].id.as_str(),
                roots[0].agent_type.as_deref(),
                roots[0].skill.as_deref()
            ),
            ("a1", Some("Explore"), Some("search"))
        );
        assert_eq!(roots[0].task.as_deref(), Some("find files"));
        assert_eq!(
            roots[0].ended_at.as_deref(),
            Some("2026-01-01T00:05:00.000Z")
        );
        assert_eq!(roots[1].id, "a2");
        assert_eq!(
            roots[1].ended_at.as_deref(),
            Some("2027-01-15T08:00:00.000Z")
        );
        assert!(
            read_claude_agent_call_graph(&context, "missing", WINDOWS, 0.0)
                .unwrap()
                .is_empty()
        );
    }

    /// 원격 파일 시스템 응답마다 지연을 넣고, 한꺼번에 돌던 명령 수의 최댓값을 잰다
    fn slow_remote_file_system(
        files: &[(String, String)],
    ) -> (Arc<crate::test_support::ScriptedRunner>, Arc<AtomicUsize>) {
        let file_refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(file_path, content)| (file_path.as_str(), content.as_str()))
            .collect();
        let file_system = remote_file_system(&file_refs);
        let file_paths: Vec<String> = files
            .iter()
            .map(|(file_path, _)| file_path.clone())
            .collect();
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak_in_flight = Arc::new(AtomicUsize::new(0));
        let peak = peak_in_flight.clone();
        let runner = crate::test_support::ScriptedRunner::new(move |command, ssh_host| {
            let running = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(running, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(30));
            let answer = match recently_modified_root(command) {
                Some(root) => Ok(list_with_fixed_mtime(&file_paths, &root)),
                None => file_system.run(command, ssh_host, CommandOptions::default()),
            };
            in_flight.fetch_sub(1, Ordering::SeqCst);
            answer
        });
        (runner, peak_in_flight)
    }

    /// 수정 시각을 함께 내는 원격 `find ... -mmin` 명령이면 찾는 디렉터리
    fn recently_modified_root(command: &str) -> Option<String> {
        if !command.contains(" -mmin ") {
            return None;
        }
        let quoted_root = command.strip_prefix("test -d '")?;
        Some(quoted_root[..quoted_root.find('\'')?].to_string())
    }

    /// 원격 파일 시스템의 수정 시각(1700000000초)을 붙인 `find` 출력
    fn list_with_fixed_mtime(file_paths: &[String], root: &str) -> String {
        file_paths
            .iter()
            .filter(|file_path| file_path.starts_with(&format!("{root}/")))
            .map(|file_path| format!("1700000000\t{file_path}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn live_reads_of_several_remote_sessions_run_together_within_the_limit() {
        let worktree_path = "/remote/home/repo";
        let project = format!(
            "/remote/home/.claude/projects/{}",
            claude_project_directory_name(worktree_path)
        );
        let files: Vec<(String, String)> = (0..4)
            .map(|index| {
                let answer =
                    json!({"message": {"role": "assistant", "content": format!("answer {index}")}});
                (
                    format!("{project}/session-{index}.jsonl"),
                    json_lines(&[answer]),
                )
            })
            .collect();
        let (runner, peak_in_flight) = slow_remote_file_system(&files);
        let context = context_for(remote_host(runner, "remote-host"), worktree_path);

        let live = read_claude_live_sessions(&context, WINDOWS, 1_700_000_010_000.0).unwrap();

        let session_ids: Vec<_> = live
            .iter()
            .map(|snapshot| snapshot.session_id.clone().unwrap())
            .collect();
        let current_tasks: Vec<_> = live
            .iter()
            .map(|snapshot| snapshot.current_task.clone().unwrap())
            .collect();
        let expected_tasks: Vec<_> = session_ids
            .iter()
            .map(|session_id| session_id.replace("session-", "answer "))
            .collect();
        assert_eq!(session_ids.len(), 4);
        assert_eq!(current_tasks, expected_tasks);
        let peak = peak_in_flight.load(Ordering::SeqCst);
        assert!(peak > 1, "peak {peak}");
        assert!(
            peak <= crate::session_listing::REMOTE_SESSION_FILE_PARSE_CONCURRENCY,
            "peak {peak}"
        );
    }

    #[test]
    fn call_graph_reads_remote_subagent_transcripts_together() {
        let worktree_path = "/remote/home/repo";
        let project = format!(
            "/remote/home/.claude/projects/{}",
            claude_project_directory_name(worktree_path)
        );
        let session = json_lines(&[json!({"message": {"role": "assistant", "content": [
            {"type": "tool_use", "id": "t1", "name": "Agent", "input": {"prompt": "first"}},
            {"type": "tool_use", "id": "t2", "name": "Agent", "input": {"prompt": "second"}}
        ]}})]);
        let files = vec![
            (format!("{project}/s1.jsonl"), session),
            (
                format!("{project}/s1/subagents/agent-a1.jsonl"),
                json_lines(&[
                    json!({"agentId": "a1", "message": {"role": "user", "content": "first"}}),
                ]),
            ),
            (
                format!("{project}/s1/subagents/agent-a2.jsonl"),
                json_lines(&[
                    json!({"agentId": "a2", "message": {"role": "user", "content": "second"}}),
                ]),
            ),
        ];
        let (runner, peak_in_flight) = slow_remote_file_system(&files);
        let context = context_for(remote_host(runner, "remote-host"), worktree_path);

        let roots =
            read_claude_agent_call_graph(&context, "s1", WINDOWS, 1_700_000_010_000.0).unwrap();

        let root_ids: Vec<_> = roots.iter().map(|root| root.id.as_str()).collect();
        assert_eq!(root_ids, ["a1", "a2"]);
        assert!(peak_in_flight.load(Ordering::SeqCst) > 1);
    }
}
