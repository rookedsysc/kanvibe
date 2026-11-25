//! TS `paneChatService.test.ts`의 "pane chat exact CLI control"과 "conversation lifetime fences"(69~750행).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use kanvibe_ai::AiResult;
use kanvibe_ai::host::SessionHost;
use kanvibe_ai::pane_chat::model_settings::{PaneChatModelSettingChange, PaneChatModelSettings};
use kanvibe_ai::pane_chat::process::{PaneChatProcess, PaneChatProvider};
use kanvibe_ai::pane_chat::session_bindings::{
    PaneChatBindingQuery, PaneChatSessionBinding, PaneChatSessionBindings, PaneChatSessionCatalog,
};
use kanvibe_ai::pane_chat::snapshot::{
    PaneChatBounds, PaneChatInputState, PaneChatLayout, PaneChatLayoutPane, PaneChatSnapshot,
    PaneChatTranscriptStatus,
};
use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AggregatedAiSession, AggregatedAiSessionDetail, AiMessageRole, AiSessionReaderContext,
};
use kanvibe_domain::pane_chat::{FilteredPaneChatSuggestions, PaneChatSuggestion};
use kanvibe_domain::task::SessionType;
use serde_json::{Value, json};

use super::super::dependencies::{
    AiPaneChatSessionRecords, BeforeSend, CodexThreadAddress, CodexThreadDelivery,
    PaneChatReaderScope, PaneChatSessionRecords,
};
use super::super::{PaneChatError, PaneChatReadOptions};
use super::*;
use crate::herdr::HerdrAgentSession;
use crate::pane_chat::attachments::{PaneChatAttachmentInput, PaneChatAttachmentKind};
use crate::pane_chat::composer_catalog::PaneChatSuggestionKind;
use crate::pane_chat::surface::{PaneChatSurface, PaneChatSurfaceError};
use crate::session_command_runner::ShellSessionCommandRunner;
use crate::test_support::ScriptedShellRunner;

const TASK: &str = "task-chat";

fn claude_table() -> String {
    process_table(DEFAULT_PROCESS_START).replace(
        "/usr/bin/python3 python3 /tmp/codex",
        "/tmp/claude /tmp/claude",
    )
}

fn snapshot(harness: &Harness, pane_id: Option<&str>) -> PaneChatSnapshot {
    harness
        .service
        .get_pane_chat_snapshot(TASK, pane_id)
        .unwrap()
}

fn token_of(snapshot: &PaneChatSnapshot) -> String {
    snapshot.target_token.clone().expect("보낼 수 있는 대상")
}

fn send(harness: &Harness, token: &str, text: &str) -> Result<(), PaneChatError> {
    harness
        .service
        .send_pane_chat_prompt(TASK, token, text, &[], None)
}

fn name_of<T: std::fmt::Debug>(result: Result<T, PaneChatError>) -> &'static str {
    error_name(&result.expect_err("오류여야 한다"))
}

fn utf16_length(text: &str) -> usize {
    text.encode_utf16().count()
}

/// TS `showClaudeEchoingPastes`. 붙여넣은 글을 경계선 아래 입력란에 그대로 그리는 Claude를 흉내 낸다
fn show_claude_echoing_pastes(harness: &Harness, empty_input: PaneChatSurface) {
    harness.surfaces.set(Some(empty_input.clone()));
    let surfaces = harness.surfaces.clone();
    harness.bridge.respond(move |_, _, bytes| {
        let Some(pasted) = bytes
            .strip_prefix("\x1b[200~")
            .and_then(|rest| rest.strip_suffix("\x1b[201~"))
        else {
            return Ok(());
        };
        let rows: Vec<String> = pasted
            .trim_end()
            .split('\n')
            .enumerate()
            .map(|(index, line)| match (index, line.is_empty()) {
                (0, _) => format!("❯\u{a0}{line}"),
                (_, true) => String::new(),
                (_, false) => format!("  {line}"),
            })
            .collect();
        let last = rows.last().cloned().unwrap_or_default();
        let mut viewport = vec!["─".repeat(20)];
        viewport.extend(rows.iter().cloned());
        surfaces.set(Some(PaneChatSurface {
            viewport_lines: Some(viewport),
            cursor_row: Some(rows.len()),
            cursor_column: Some(utf16_length(&last)),
            input_line: Some(last),
            is_input_placeholder: false,
            ..empty_input.clone()
        }));
        Ok(())
    });
}

/// TS `bindCodexSession(model)`. hook이 묶어 준 Codex 기록에서 현재 모델 설정을 읽을 수 있게 한다
fn bind_codex_session(harness: &Harness, model: &str) {
    harness
        .sessions
        .set_binding(Some(binding("exact-session", "/allowed/exact.jsonl")));
    harness.sessions.respond_detail(|call| {
        Ok(Some(detail(
            "exact-session",
            provider_of(call.provider),
            Vec::new(),
            None,
        )))
    });
    harness.sessions.set_tail(Ok(vec![json!({
        "type": "event_msg",
        "payload": { "type": "thread_settings_applied", "thread_settings": { "model": model, "reasoning_effort": "low", "service_tier": null } }
    })]));
}

fn provider_of(provider: PaneChatProvider) -> AiSessionProvider {
    match provider {
        PaneChatProvider::Claude => AiSessionProvider::Claude,
        PaneChatProvider::Codex => AiSessionProvider::Codex,
        PaneChatProvider::Gemini => AiSessionProvider::Gemini,
        PaneChatProvider::Opencode => AiSessionProvider::Opencode,
    }
}

/// TS `mocks.write.mockImplementationOnce(action)`. 첫 쓰기 뒤에 한 번만 `action`을 실행한다
fn after_first_write(harness: &Harness, action: impl Fn() + Send + Sync + 'static) {
    let has_run = AtomicBool::new(false);
    harness.bridge.respond(move |_, _, _| {
        if !has_run.swap(true, Ordering::SeqCst) {
            action();
        }
        Ok(())
    });
}

fn replace_surface_after_first_write(harness: &Harness, next: PaneChatSurface) {
    let surfaces = harness.surfaces.clone();
    after_first_write(harness, move || surfaces.set(Some(next.clone())));
}

fn replace_process_after_first_write(harness: &Harness, start: &str) {
    let runner = harness.runner.clone();
    let table = process_table(start);
    after_first_write(harness, move || {
        let table = table.clone();
        runner.respond(move |_, _| Some(Ok(table.clone())));
    });
}

fn attachment_input(name: &str, mime_type: &str, data_url: &str) -> PaneChatAttachmentInput {
    PaneChatAttachmentInput {
        name: name.to_string(),
        mime_type: mime_type.to_string(),
        data_url: data_url.to_string(),
    }
}

// parity: TS-main-paneChatService-13c8fe
#[test]
fn gemini_and_opencode_support_exact_reading_paging_sending_and_conversation_fencing() {
    let cases = [
        (
            PaneChatProvider::Gemini,
            "node node --no-warnings /opt/node_modules/@google/gemini-cli/bundle/gemini.js",
            " >   Type your message or @path/to/file",
            3,
        ),
        (
            PaneChatProvider::Opencode,
            "opencode.exe opencode",
            " ┃  Ask anything… \"Fix a TODO\"",
            4,
        ),
    ];
    for (provider, command, input_line, cursor_column) in cases {
        let harness = Harness::new();
        harness.respond_all_commands(&format!(
            "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 {command}"
        ));
        let is_opencode = provider == PaneChatProvider::Opencode;
        let (viewport, cursor_row) = match is_opencode {
            true => (lines(&[input_line, " ┃  Build · QA model", " ╹▀▀▀"]), 0),
            false => (lines(&["──────", input_line, "──────"]), 1),
        };
        harness.surfaces.set(Some(PaneChatSurface {
            viewport_lines: viewport,
            cursor_row: Some(cursor_row),
            input_line: Some(input_line.to_string()),
            cursor_column: Some(cursor_column),
            ..surface()
        }));
        let set_binding = |harness: &Harness, session_id: &str, source_ref: &str, revision: u64| {
            let value = Some(PaneChatSessionBinding {
                revision: Some(revision),
                ..binding(session_id, source_ref)
            });
            match is_opencode {
                true => harness.sessions.set_process_binding(value),
                false => harness.sessions.set_binding(value),
            }
        };
        set_binding(&harness, "additional-exact", "/allowed/additional", 1);
        harness.sessions.respond_detail(move |call| {
            let reply = vec![message(AiMessageRole::Assistant, "exact reply")];
            Ok((call.provider == provider)
                .then(|| detail("additional-exact", provider_of(provider), reply, Some("1"))))
        });
        let shown = snapshot(&harness, Some("%3"));
        assert_eq!(shown.provider, Some(provider_of(provider)));
        assert!(shown.can_send);
        assert_eq!(shown.input_state, PaneChatInputState::Ready);
        assert_eq!(
            shown.transcript.session_id.as_deref(),
            Some("additional-exact")
        );
        assert_eq!(shown.transcript.messages[0].text, "exact reply");
        assert!(!shown.composer.as_ref().unwrap().can_attach);
        assert!(shown.conversation_key.is_some());
        let token = token_of(&shown);
        harness
            .service
            .get_pane_chat_transcript_page(TASK, &token, "1")
            .unwrap();
        let last_read = harness.sessions.detail_calls().pop().unwrap();
        assert_eq!(last_read.scope.worktree_path.as_deref(), Some("/repo"));
        assert_eq!(
            (
                last_read.session_id.as_str(),
                last_read.source_ref.as_str(),
                last_read.cursor.as_deref(),
                last_read.limit
            ),
            ("additional-exact", "/allowed/additional", Some("1"), 100)
        );
        let request_id = format!("send-{provider:?}");
        harness
            .service
            .send_pane_chat_prompt(TASK, &token, "hello", &[], Some(&request_id))
            .unwrap();
        assert_eq!(
            harness.writes(),
            Harness::expected_writes(TASK, "%3", &[&bracketed("hello"), "\r"])
        );
        harness.bridge.clear();
        let draft_line = match is_opencode {
            true => " ┃  native draft".to_string(),
            false => " >   native draft".to_string(),
        };
        harness.surfaces.set(Some(PaneChatSurface {
            input_line: Some(draft_line),
            cursor_column: Some(cursor_column + 6),
            ..surface()
        }));
        assert_eq!(
            name_of(send(&harness, &token, "blocked")),
            "PaneChatInputError"
        );
        assert!(harness.writes().is_empty());
        set_binding(&harness, "other", "/allowed/other", 2);
        let page = harness
            .service
            .get_pane_chat_transcript_page(TASK, &token, "1");
        assert_eq!(name_of(page), "PaneChatTargetChangedError");
        assert_eq!(
            name_of(send(&harness, &token, "stale")),
            "PaneChatTargetChangedError"
        );
        assert!(harness.writes().is_empty());
    }
}

// parity: TS-main-paneChatService-ecf8af
#[test]
fn gemini_does_not_overwrite_a_native_draft_below_an_empty_first_row() {
    let harness = Harness::new();
    harness.respond_all_commands("100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 gemini gemini");
    let gemini_surface = |rows: &[&str]| PaneChatSurface {
        input_line: Some(rows[1].to_string()),
        cursor_column: Some(3),
        cursor_row: Some(1),
        viewport_lines: lines(rows),
        ..surface()
    };
    harness.surfaces.set(Some(gemini_surface(&[
        "──────",
        " >  ",
        "",
        "   ───",
        "   native draft",
        "──────",
    ])));
    harness
        .sessions
        .set_binding(Some(binding("gemini-exact", "/allowed/gemini.jsonl")));
    harness.sessions.respond_detail(|_| {
        Ok(Some(detail(
            "gemini-exact",
            AiSessionProvider::Gemini,
            Vec::new(),
            None,
        )))
    });
    let shown = snapshot(&harness, Some("%3"));
    assert_eq!(shown.input_state, PaneChatInputState::Occupied);
    assert_eq!(
        name_of(send(&harness, &token_of(&shown), "new prompt")),
        "PaneChatInputError"
    );
    assert!(harness.writes().is_empty());
    harness.surfaces.set(Some(gemini_surface(&[
        "──────",
        " >  ",
        "",
        "",
        "",
        "──────",
    ])));
    assert_eq!(
        snapshot(&harness, Some("%3")).input_state,
        PaneChatInputState::Ready
    );
}

fn opencode_table() -> &'static str {
    "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 opencode.exe opencode"
}

// parity: TS-main-paneChatService-c5b15b
#[test]
fn opencode_does_not_overwrite_a_multiline_draft_above_an_empty_cursor_row() {
    let harness = Harness::new();
    harness.respond_all_commands(opencode_table());
    let opencode_surface = |draft_row: &str| {
        let rows = [
            "  ┃  previous transcript",
            "",
            "  ┃",
            draft_row,
            "  ┃   ",
            "  ┃  Build · QA model",
            "  ╹▀▀▀",
        ];
        PaneChatSurface {
            input_line: Some(rows[4].to_string()),
            cursor_column: Some(5),
            cursor_row: Some(4),
            viewport_lines: lines(&rows),
            ..surface()
        }
    };
    harness
        .surfaces
        .set(Some(opencode_surface("  ┃  native draft")));
    harness
        .sessions
        .set_process_binding(Some(PaneChatSessionBinding {
            revision: Some(1),
            is_empty: true,
            ..binding("opencode-home:200", "/repo/state.json")
        }));
    let shown = snapshot(&harness, Some("%3"));
    assert_eq!(shown.input_state, PaneChatInputState::Occupied);
    assert_eq!(
        name_of(send(&harness, &token_of(&shown), "new prompt")),
        "PaneChatInputError"
    );
    assert!(harness.writes().is_empty());
    harness.surfaces.set(Some(opencode_surface("  ┃")));
    assert_eq!(
        snapshot(&harness, Some("%3")).input_state,
        PaneChatInputState::Ready
    );
}

// parity: TS-main-paneChatService-91c557
#[test]
fn opencode_home_sends_the_first_prompt_into_an_empty_conversation() {
    let harness = Harness::new();
    harness.respond_all_commands(opencode_table());
    let rows = [
        "                  ┃  Ask anything…",
        "                  ┃  Build · QA model",
        "                  ╹▀▀▀",
    ];
    harness.surfaces.set(Some(PaneChatSurface {
        input_line: Some(rows[0].to_string()),
        cursor_column: Some(21),
        cursor_row: Some(0),
        viewport_lines: lines(&rows),
        ..surface()
    }));
    harness
        .sessions
        .set_process_binding(Some(PaneChatSessionBinding {
            revision: Some(1),
            is_empty: true,
            ..binding(
                "opencode-home:200",
                "/repo/.kanvibe/pane-chat/opencode-200.json",
            )
        }));
    let shown = snapshot(&harness, Some("%3"));
    assert_eq!(shown.provider, Some(AiSessionProvider::Opencode));
    assert!(shown.can_send);
    assert_eq!(shown.input_state, PaneChatInputState::Ready);
    assert!(shown.transcript.messages.is_empty());
    assert_eq!(shown.transcript.next_cursor, None);
    send(&harness, &token_of(&shown), "first prompt").unwrap();
    assert_eq!(
        harness.bridge.written_bytes().last().map(String::as_str),
        Some("\r")
    );
}

// parity: TS-main-paneChatService-61fa2b
#[test]
fn gemini_and_opencode_without_an_exact_session_never_guess_history_or_allow_input() {
    for (provider, name) in [
        (AiSessionProvider::Gemini, "gemini"),
        (AiSessionProvider::Opencode, "opencode"),
    ] {
        let harness = Harness::new();
        harness.respond_all_commands(&format!(
            "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh\n200 100 200 200 ttys010 Thu Nov 12 19:00:00 2026 {name} {name}"
        ));
        harness.sessions.set_binding(None);
        let shown = snapshot(&harness, Some("%3"));
        assert_eq!(shown.provider, Some(provider));
        assert_eq!(shown.target_token, None);
        assert!(!shown.can_send);
        assert_eq!(shown.reason.as_deref(), Some("exact-session-unavailable"));
        assert!(harness.writes().is_empty());
    }
}

// parity: TS-main-paneChatService-345f3d
#[test]
fn suggestions_are_pinned_to_the_target_and_rejected_when_the_cli_changes_while_loading() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, Some("%3")));
    harness.catalog.respond(|_, _, _, _| {
        let item = PaneChatSuggestion {
            id: "skill:review".to_string(),
            label: "review".to_string(),
            description: "Review".to_string(),
            insert_text: "$review".to_string(),
        };
        Ok(FilteredPaneChatSuggestions {
            items: vec![item],
            is_truncated: false,
        })
    });
    let found = harness
        .service
        .get_pane_chat_suggestions(TASK, &token, PaneChatSuggestionKind::Skill, "review", None)
        .unwrap();
    assert_eq!(found.target_token, token);
    assert_eq!(found.items[0].insert_text, "$review");
    let runner = harness.runner.clone();
    harness.catalog.respond(move |_, _, _, _| {
        let table = process_table("Thu Nov 12 20:00:00 2026");
        runner.respond(move |_, _| Some(Ok(table.clone())));
        Ok(FilteredPaneChatSuggestions {
            items: Vec::new(),
            is_truncated: false,
        })
    });
    let changed = harness.service.get_pane_chat_suggestions(
        TASK,
        &token,
        PaneChatSuggestionKind::File,
        "readme",
        None,
    );
    assert_eq!(name_of(changed), "PaneChatTargetChangedError");
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-befe75
#[test]
fn a_raised_suggestion_limit_reaches_the_catalog_and_limits_outside_1_to_512_are_rejected() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, Some("%3")));
    harness
        .service
        .get_pane_chat_suggestions(TASK, &token, PaneChatSuggestionKind::Skill, "", Some(512))
        .unwrap();
    let (_, kind, query, limit) = harness.catalog.calls().pop().unwrap();
    assert_eq!(
        (kind, query.as_str(), limit),
        (PaneChatSuggestionKind::Skill, "", 512)
    );
    harness.catalog.clear_calls();
    // 1.5와 NaN은 `Option<usize>` 인자로 표현할 수 없어 정수 경계만 본다
    for limit in [0, 513] {
        let rejected = harness.service.get_pane_chat_suggestions(
            TASK,
            &token,
            PaneChatSuggestionKind::Skill,
            "",
            Some(limit),
        );
        assert_eq!(name_of(rejected), "PaneChatInputError");
    }
    assert!(harness.catalog.calls().is_empty());
}

// parity: TS-main-paneChatService-23562f
#[test]
fn the_same_request_can_retry_after_a_definite_refusal_before_any_write() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    harness.surfaces.set(Some(PaneChatSurface {
        input_line: Some("› native text".to_string()),
        cursor_column: Some(13),
        ..surface()
    }));
    let refused =
        harness
            .service
            .send_pane_chat_prompt(TASK, &token, "retry", &[], Some("pre-write-retry"));
    assert_eq!(name_of(refused), "PaneChatInputError");
    assert!(harness.writes().is_empty());
    harness.surfaces.set(Some(surface()));
    harness
        .service
        .send_pane_chat_prompt(TASK, &token, "retry", &[], Some("pre-write-retry"))
        .unwrap();
    assert_eq!(harness.writes().len(), 2);
}

// parity: TS-main-paneChatService-08c36c
#[test]
fn a_staged_image_is_pasted_as_a_native_attachment_for_image_only_input() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, Some("%3")));
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aOK0AAAAASUVORK5CYII=";
    let input = attachment_input(
        "image.png",
        "image/png",
        &format!("data:image/png;base64,{png}"),
    );
    let attachment = harness
        .service
        .stage_pane_chat_attachment(TASK, &token, &input)
        .unwrap();
    assert_eq!(attachment.kind, PaneChatAttachmentKind::Image);
    assert_eq!(
        (attachment.mime_type.as_str(), attachment.size_bytes),
        ("image/png", 68)
    );
    harness
        .service
        .send_pane_chat_prompt(
            TASK,
            &token,
            "",
            std::slice::from_ref(&attachment.id),
            Some("image-only"),
        )
        .unwrap();
    let writes = harness.writes();
    assert_eq!(writes.len(), 2);
    let pasted = &writes[0].2;
    assert_eq!((writes[0].0.as_str(), writes[0].1.as_str()), (TASK, "%3"));
    assert!(
        pasted.starts_with("\x1b[200~/") && pasted.ends_with(".png\x1b[201~"),
        "{pasted:?}"
    );
    assert_eq!(
        writes[1],
        (TASK.to_string(), "%3".to_string(), "\r".to_string())
    );
    harness
        .service
        .remove_pane_chat_attachment(TASK, &token, &attachment.id)
        .unwrap();
}

// parity: TS-main-paneChatService-ddc755
#[test]
fn an_attachment_from_another_live_cli_target_is_rejected_before_any_write() {
    let harness = Harness::new();
    let first = token_of(&snapshot(&harness, Some("%3")));
    let input = attachment_input("notes.txt", "text/plain", "data:text/plain;base64,aGVsbG8=");
    let attachment = harness
        .service
        .stage_pane_chat_attachment(TASK, &first, &input)
        .unwrap();
    let second = token_of(&snapshot(&harness, None));
    let rejected = harness.service.send_pane_chat_prompt(
        TASK,
        &second,
        "read",
        std::slice::from_ref(&attachment.id),
        None,
    );
    assert_eq!(name_of(rejected), "PaneChatInputError");
    assert!(harness.writes().is_empty());
    harness
        .service
        .remove_pane_chat_attachment(TASK, &first, &attachment.id)
        .unwrap();
}

// parity: TS-main-paneChatService-03e389
#[test]
fn one_request_receipt_is_replayed_and_the_same_id_with_another_payload_is_rejected() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    harness
        .service
        .send_pane_chat_prompt(TASK, &token, "once", &[], Some("retry-one"))
        .unwrap();
    harness
        .service
        .send_pane_chat_prompt(TASK, &token, "once", &[], Some("retry-one"))
        .unwrap();
    assert_eq!(harness.writes().len(), 2);
    let different =
        harness
            .service
            .send_pane_chat_prompt(TASK, &token, "different", &[], Some("retry-one"));
    assert_eq!(name_of(different), "PaneChatInputError");
    assert_eq!(harness.writes().len(), 2);
}

// parity: TS-main-paneChatService-cd4358
#[test]
fn a_possibly_accepted_paste_is_not_repeated_after_an_interrupted_delivery() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    harness.bridge.fail_once("socket closed after write");
    let first = harness.service.send_pane_chat_prompt(
        TASK,
        &token,
        "uncertain",
        &[],
        Some("uncertain-one"),
    );
    assert_eq!(name_of(first), "PaneChatDeliveryUncertainError");
    let repeated = harness.service.send_pane_chat_prompt(
        TASK,
        &token,
        "uncertain",
        &[],
        Some("uncertain-one"),
    );
    assert!(repeated.is_err());
    assert_eq!(harness.writes().len(), 1);
}

// parity: TS-main-paneChatService-d64c7b
#[test]
fn a_native_codex_skill_completion_token_is_closed_before_the_single_submit_enter() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    send(&harness, &token, "Use $qa-skill").unwrap();
    assert_eq!(
        harness.writes(),
        Harness::expected_writes(TASK, "%3", &[&bracketed("Use $qa-skill "), "\r"])
    );
}

// parity: TS-main-paneChatService-612ed8
#[test]
fn a_staged_claude_file_reference_is_closed_before_the_single_submit_enter() {
    for text in ["", "Read this file"] {
        let harness = Harness::new();
        harness.respond_all_commands(&claude_table());
        show_claude_echoing_pastes(
            &harness,
            PaneChatSurface {
                input_line: Some("❯ ".to_string()),
                ..surface()
            },
        );
        let token = token_of(&snapshot(&harness, Some("%3")));
        let input = attachment_input(
            "file with spaces.txt",
            "text/plain",
            "data:text/plain;base64,aGVsbG8=",
        );
        let attachment = harness
            .service
            .stage_pane_chat_attachment(TASK, &token, &input)
            .unwrap();
        harness
            .service
            .send_pane_chat_prompt(
                TASK,
                &token,
                text,
                std::slice::from_ref(&attachment.id),
                None,
            )
            .unwrap();
        let writes = harness.writes();
        assert_eq!(writes.len(), 2);
        let caption = if text.is_empty() {
            String::new()
        } else {
            format!("{text}\n\n")
        };
        let paste = &writes[0].2;
        let mention = paste
            .strip_prefix(&format!("\x1b[200~{caption}"))
            .and_then(|rest| rest.strip_suffix(" \x1b[201~"))
            .expect("파일 참조를 공백으로 닫은 붙여넣기");
        let is_mention = (mention.starts_with("@\"") && mention.ends_with(".txt\""))
            || (mention.starts_with("@/") && mention.ends_with(".txt"));
        assert!(is_mention && !mention.contains('\n'), "{mention:?}");
        assert_eq!(writes[1].2, "\r");
        harness
            .service
            .remove_pane_chat_attachment(TASK, &token, &attachment.id)
            .unwrap();
    }
}

// parity: TS-main-paneChatService-4b02a7
#[test]
fn claude_text_is_preserved_and_only_a_final_file_mention_is_terminated() {
    let cases = [
        ("Read @/repo/file.txt", "Read @/repo/file.txt "),
        (
            "Read @\"/repo/file with spaces.txt\"",
            "Read @\"/repo/file with spaces.txt\" ",
        ),
        (
            "Read @\"/repo/file \\\"quoted\\\".txt\"",
            "Read @\"/repo/file \\\"quoted\\\".txt\" ",
        ),
        ("Read @/repo/file.txt ", "Read @/repo/file.txt "),
        ("Read @/repo/file.txt please", "Read @/repo/file.txt please"),
        ("Contact person@example.com", "Contact person@example.com"),
    ];
    for (text, expected) in cases {
        let harness = Harness::new();
        harness.respond_all_commands(&claude_table());
        show_claude_echoing_pastes(
            &harness,
            PaneChatSurface {
                input_line: Some("❯ ".to_string()),
                ..surface()
            },
        );
        let token = token_of(&snapshot(&harness, Some("%3")));
        send(&harness, &token, text).unwrap();
        assert_eq!(
            harness.writes(),
            Harness::expected_writes(TASK, "%3", &[&bracketed(expected), "\r"]),
            "{text}"
        );
    }
}

// parity: TS-main-paneChatService-d869b1
#[test]
fn returning_to_the_terminal_restores_the_requested_tmux_pane() {
    let harness = Harness::new();
    harness.respond_all_commands("");
    harness
        .service
        .focus_pane_chat_terminal(TASK, "%3")
        .unwrap();
    assert!(
        harness
            .surfaces
            .calls()
            .contains(&(TASK.to_string(), Some("%3".to_string()), false))
    );
    let calls = harness.runner.calls();
    assert!(
        calls
            .iter()
            .any(|call| call.command == "tmux select-pane -t '%3'" && call.ssh_host.is_none())
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-e3d6e1
#[test]
fn an_invalid_or_missing_return_target_never_focuses_another_pane() {
    let harness = Harness::new();
    assert_eq!(
        name_of(harness.service.focus_pane_chat_terminal(TASK, "")),
        "PaneChatInputError"
    );
    harness.surfaces.set(None);
    assert_eq!(
        name_of(harness.service.focus_pane_chat_terminal(TASK, "%foreign")),
        "PaneChatTargetChangedError"
    );
    assert!(harness.runner.commands().is_empty());
    assert!(
        harness
            .terminals
            .focused_herdr_panes
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(harness.writes().is_empty());
}

fn surface_on(session_type: SessionType, pane_id: &str) -> PaneChatSurface {
    let mut target = surface();
    target.task.session_type = Some(session_type);
    target.pane_id = pane_id.to_string();
    target
}

// parity: TS-main-paneChatService-36d4f7
#[test]
fn returning_targets_the_selected_zellij_session_and_pane() {
    let harness = Harness::new();
    let mut zellij = surface_on(SessionType::Zellij, "terminal_7");
    zellij.task.session_name = Some("task's zellij".to_string());
    harness.surfaces.set(Some(zellij));
    harness.respond_all_commands("");
    harness
        .service
        .focus_pane_chat_terminal(TASK, "terminal_7")
        .unwrap();
    let expected = "zellij --session 'task'\"'\"'s zellij' action focus-pane-id 'terminal_7'";
    assert!(
        harness
            .runner
            .calls()
            .iter()
            .any(|call| call.command == expected && call.ssh_host.is_none())
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-c41f7a
#[test]
fn herdr_returns_through_the_task_client_and_a_stale_plain_tab_is_rejected() {
    let harness = Harness::new();
    harness
        .surfaces
        .set(Some(surface_on(SessionType::Herdr, "w1:p2")));
    harness
        .service
        .focus_pane_chat_terminal(TASK, "w1:p2")
        .unwrap();
    assert_eq!(
        *harness.terminals.focused_herdr_panes.lock().unwrap(),
        vec![(TASK.to_string(), "w1:p2".to_string())]
    );
    harness
        .surfaces
        .set(Some(surface_on(SessionType::Terminal, "term-a")));
    *harness.terminals.tabs.lock().unwrap() = vec![terminal_tab("term-a", true)];
    harness
        .service
        .focus_pane_chat_terminal(TASK, "term-a")
        .unwrap();
    *harness.terminals.tabs.lock().unwrap() =
        vec![terminal_tab("term-a", false), terminal_tab("term-b", true)];
    assert_eq!(
        name_of(harness.service.focus_pane_chat_terminal(TASK, "term-a")),
        "PaneChatTargetChangedError"
    );
    assert!(harness.runner.commands().is_empty());
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-026cf0
#[test]
fn invalid_layout_requests_are_rejected_and_unavailable_geometry_is_hidden() {
    let harness = Harness::new();
    assert_eq!(
        name_of(harness.service.get_pane_chat_layout("", None)),
        "PaneChatInputError"
    );
    assert_eq!(
        name_of(harness.service.get_pane_chat_layout(TASK, Some(""))),
        "PaneChatInputError"
    );
    assert!(harness.surfaces.layout_calls().is_empty());
    harness
        .surfaces
        .set_layout(Err(PaneChatSurfaceError("closed session".to_string())));
    let layout = harness.service.get_pane_chat_layout(TASK, None).unwrap();
    assert_eq!(
        layout,
        PaneChatLayout {
            active_pane_id: None,
            panes: Vec::new()
        }
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-a49f85
#[test]
fn the_service_boundary_preserves_the_selected_terminal_identity_and_cell_geometry() {
    let harness = Harness::new();
    let layout = PaneChatLayout {
        active_pane_id: Some("%3".to_string()),
        panes: vec![PaneChatLayoutPane {
            id: "%3".to_string(),
            tab_id: "@1".to_string(),
            bounds: Some(PaneChatBounds {
                left: 41,
                top: 1,
                width: 59,
                height: 24,
            }),
        }],
    };
    harness.surfaces.set_layout(Ok(layout.clone()));
    assert_eq!(
        harness
            .service
            .get_pane_chat_layout(TASK, Some("term-1"))
            .unwrap(),
        layout
    );
    assert_eq!(
        harness.surfaces.layout_calls(),
        vec![(TASK.to_string(), Some("term-1".to_string()))]
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-f08ccb
#[test]
fn a_pinned_chat_stays_on_its_pane_when_runtime_focus_moves_to_a_sibling() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, Some("%3")));
    harness.surfaces.respond(|_, requested, _| {
        let target = match requested {
            Some("%3") => surface(),
            _ => PaneChatSurface {
                pane_id: "%9".to_string(),
                ..surface()
            },
        };
        Ok(Some(target))
    });
    harness.service.interrupt_pane_chat(TASK, &token).unwrap();
    assert_eq!(
        harness.surfaces.calls().pop(),
        Some((TASK.to_string(), Some("%3".to_string()), false))
    );
    assert_eq!(
        harness.writes(),
        Harness::expected_writes(TASK, "%3", &["\x03"])
    );
}

// parity: TS-main-paneChatService-56c77b
#[test]
fn scrollback_is_read_only_when_the_screen_is_requested_or_the_transcript_is_unavailable() {
    let harness = Harness::new();
    let with_transcript = snapshot(&harness, Some("%3"));
    assert!(with_transcript.can_send);
    assert_eq!(with_transcript.screen, "");
    assert!(harness.surfaces.screen_calls().is_empty());
    let options = PaneChatReadOptions {
        should_include_screen: true,
        is_shared_with_other_viewers: false,
    };
    let requested = harness
        .service
        .read_pane_chat(TASK, Some("%3"), options)
        .unwrap();
    assert!(requested.snapshot.can_send);
    assert_eq!(requested.snapshot.screen, "CLI screen");
    assert_eq!(harness.surfaces.screen_calls().len(), 1);
    harness.sessions.set_binding(None);
    let unbound = snapshot(&harness, Some("%3"));
    assert!(!unbound.can_send);
    assert_eq!(unbound.screen, "CLI screen");
    assert_eq!(harness.surfaces.screen_calls().len(), 2);
}

// parity: TS-main-paneChatService-f54971
#[test]
fn a_pane_whose_screen_cannot_be_read_is_hidden_even_with_a_bound_conversation() {
    let harness = Harness::new();
    harness
        .surfaces
        .respond_screen(|_| Err(PaneChatSurfaceError("capture failed".to_string())));
    let options = PaneChatReadOptions {
        should_include_screen: true,
        is_shared_with_other_viewers: false,
    };
    let shown = harness
        .service
        .read_pane_chat(TASK, Some("%3"), options)
        .unwrap()
        .snapshot;
    assert_eq!(shown.pane_id, None);
    assert!(!shown.can_send);
    assert_eq!(shown.reason.as_deref(), Some("pane-unavailable"));
}

// parity: TS-main-paneChatService-ebbbf9
#[test]
fn a_watcher_reading_is_reused_within_one_second_and_refreshed_after_time_or_input() {
    let harness = Harness::new();
    let shared = PaneChatReadOptions {
        should_include_screen: false,
        is_shared_with_other_viewers: true,
    };
    let watched = harness
        .service
        .read_pane_chat(TASK, Some("%7"), shared)
        .unwrap();
    let reads_by_watcher = harness.surfaces.calls().len();
    assert_eq!(snapshot(&harness, Some("%7")), watched.snapshot);
    assert_eq!(harness.surfaces.calls().len(), reads_by_watcher);
    // 화면이 필요한 조회는 화면 없이 읽은 결과를 쓰지 않고, 다른 pane 식별자로 요청한 조회와는 lease가 달라 나누지 않는다
    let with_screen = harness
        .service
        .read_recent_pane_chat(TASK, Some("%7"), true)
        .unwrap();
    assert_eq!(with_screen.snapshot.screen, "CLI screen");
    snapshot(&harness, None);
    let reads_after_other_requests = harness.surfaces.calls().len();
    assert!(reads_after_other_requests > reads_by_watcher);
    harness.clock.advance(1_001);
    snapshot(&harness, Some("%7"));
    assert!(harness.surfaces.calls().len() > reads_after_other_requests);
    let rewatched = harness
        .service
        .read_pane_chat(TASK, Some("%7"), shared)
        .unwrap();
    harness
        .service
        .interrupt_pane_chat(TASK, &token_of(&rewatched.snapshot))
        .unwrap();
    let reads_after_input = harness.surfaces.calls().len();
    snapshot(&harness, Some("%7"));
    assert!(harness.surfaces.calls().len() > reads_after_input);
}

// parity: TS-main-paneChatService-bac213
#[test]
fn the_exact_cli_stays_visible_but_input_is_refused_while_the_conversation_is_unmapped() {
    let harness = Harness::new();
    harness.sessions.set_binding(None);
    let first = snapshot(&harness, None);
    let next = snapshot(&harness, None);
    assert!(!first.can_send);
    assert_eq!(first.pane_id.as_deref(), Some("%3"));
    assert_eq!(first.provider, Some(AiSessionProvider::Codex));
    assert_eq!(first.screen, "CLI screen");
    assert_eq!(
        first.transcript.status,
        PaneChatTranscriptStatus::Unavailable
    );
    assert!(first.transcript.messages.is_empty());
    assert_eq!(first.target_token, next.target_token);
}

// parity: TS-main-paneChatService-e3a16f
#[test]
fn recent_history_is_never_substituted_for_a_shell_or_background_provider() {
    let harness = Harness::new();
    harness.respond_all_commands("100 1 100 100 ttys010 Thu Nov 12 18:00:00 2026 /bin/zsh /bin/zsh\n200 100 200 100 ttys010 Thu Nov 12 19:00:00 2026 /tmp/codex /tmp/codex");
    let shown = snapshot(&harness, None);
    assert!(!shown.can_send);
    assert_eq!(shown.target_token, None);
    assert_eq!(
        shown.transcript.status,
        PaneChatTranscriptStatus::Unavailable
    );
    assert!(shown.transcript.messages.is_empty());
    assert!(harness.sessions.detail_calls().is_empty());
}

// parity: TS-main-paneChatService-7cf139
#[test]
fn multiline_text_is_pasted_and_a_separate_cr_goes_to_the_same_fenced_process() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    send(&harness, &token, "First line\nSecond line").unwrap();
    assert_eq!(
        harness.writes(),
        Harness::expected_writes(TASK, "%3", &[&bracketed("First line\nSecond line"), "\r"])
    );
}

// parity: TS-main-paneChatService-b1c242
#[test]
fn a_reused_pid_is_rejected_before_any_write() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    harness.respond_all_commands(&process_table("Thu Nov 12 19:01:00 2026"));
    assert_eq!(
        name_of(send(&harness, &token, "Hello")),
        "PaneChatTargetChangedError"
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-66d990
#[test]
fn no_cr_is_submitted_if_the_cli_exits_after_the_paste() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    replace_process_after_first_write(&harness, "Thu Nov 12 19:02:00 2026");
    assert_eq!(
        name_of(send(&harness, &token, "Hello")),
        "PaneChatTargetChangedError"
    );
    assert_eq!(harness.writes().len(), 1);
}

// parity: TS-main-paneChatService-4401fa
#[test]
fn control_sequence_injection_and_cross_task_token_reuse_are_rejected() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    assert_eq!(
        name_of(send(&harness, &token, "Hi\x1b[201~\rcommand")),
        "PaneChatInputError"
    );
    assert_eq!(
        name_of(
            harness
                .service
                .interrupt_pane_chat("different-task", &token)
        ),
        "PaneChatTargetChangedError"
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-605be8
#[test]
fn the_codex_picker_is_opened_by_pasting_its_command_once_and_only_known_keys_are_sent() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    harness
        .service
        .open_pane_chat_model_picker(TASK, &token)
        .unwrap();
    assert_eq!(
        harness.bridge.written_bytes(),
        vec![bracketed("/model"), "\r".to_string()]
    );
    harness
        .service
        .send_pane_chat_key(TASK, &token, "down")
        .unwrap();
    assert_eq!(
        harness.writes().pop(),
        Some((TASK.to_string(), "%3".to_string(), "\x1b[B".to_string()))
    );
    assert_eq!(
        name_of(harness.service.send_pane_chat_key(TASK, &token, "command")),
        "PaneChatInputError"
    );
}

// parity: TS-main-paneChatService-d983cb
#[test]
fn enter_is_not_pressed_if_the_cli_is_replaced_after_the_command_paste() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    replace_process_after_first_write(&harness, "Thu Nov 12 19:02:00 2026");
    let opened = harness.service.open_pane_chat_model_picker(TASK, &token);
    assert_eq!(name_of(opened), "PaneChatTargetChangedError");
    assert_eq!(harness.bridge.written_bytes(), vec![bracketed("/model")]);
}

/// TS `openPickerWithInputLineAfterPaste`. 명령을 붙여넣은 뒤 Claude 커서 줄이 `input_line`으로 바뀐 상황을 만든다
fn open_claude_picker_with_input_line_after_paste(
    harness: &Harness,
    input_line: &str,
) -> Result<(), PaneChatError> {
    let empty_input = PaneChatSurface {
        input_line: Some("❯\u{a0}".to_string()),
        cursor_column: Some(2),
        ..surface()
    };
    harness.respond_all_commands(&claude_table());
    harness.surfaces.set(Some(empty_input.clone()));
    let token = token_of(&snapshot(harness, Some("%3")));
    replace_surface_after_first_write(
        harness,
        PaneChatSurface {
            input_line: Some(input_line.to_string()),
            cursor_column: Some(utf16_length(input_line)),
            ..empty_input
        },
    );
    harness.service.open_pane_chat_model_picker(TASK, &token)
}

// parity: TS-main-paneChatService-447ecf
#[test]
fn claude_command_enter_is_pressed_when_the_composer_still_shows_the_command() {
    let harness = Harness::new();
    open_claude_picker_with_input_line_after_paste(&harness, "❯\u{a0}/model").unwrap();
    assert_eq!(
        harness.bridge.written_bytes(),
        vec![bracketed("/model"), "\r".to_string()]
    );
}

// parity: TS-main-paneChatService-657500
#[test]
fn claude_command_enter_is_withheld_when_the_composer_changed_or_disappeared() {
    let changed_lines = [
        "❯\u{a0}Reply with exactly: RACE",
        "   ❯ 1. Yes, switch to Opus 5.5",
        "  Enter to confirm · Esc to cancel",
    ];
    for input_line in changed_lines {
        let harness = Harness::new();
        let opened = open_claude_picker_with_input_line_after_paste(&harness, input_line);
        assert_eq!(name_of(opened), "PaneChatInputError", "{input_line}");
        assert_eq!(harness.bridge.written_bytes(), vec![bracketed("/model")]);
    }
}

const BORDER_WIDTH: usize = 40;

/// TS `claudeComposer(...rows)`. 입력란에 `rows`가 그려진 Claude 화면이고 커서는 마지막 줄 끝에 있다
fn claude_composer(rows: &[&str]) -> PaneChatSurface {
    let border = "─".repeat(BORDER_WIDTH);
    let mut viewport = vec!["● earlier reply".to_string(), border.clone()];
    viewport.extend(rows.iter().map(|row| row.to_string()));
    viewport.extend([border, "  ? for shortcuts".to_string()]);
    let last = rows.last().copied().unwrap_or_default();
    PaneChatSurface {
        viewport_lines: Some(viewport),
        cursor_row: Some(rows.len() + 1),
        cursor_column: Some(utf16_length(last)),
        input_line: Some(last.to_string()),
        ..surface()
    }
}

/// TS `sendWithScreenAfterPaste`. 보낸 뒤 Enter를 기다리는 사이 화면이 `after_paste`로 바뀐 상황을 만든다
fn send_with_claude_screen_after_paste(
    text: &str,
    after_paste: PaneChatSurface,
) -> (Harness, Result<(), PaneChatError>) {
    let harness = Harness::new();
    harness.respond_all_commands(&claude_table());
    harness.surfaces.set(Some(PaneChatSurface {
        cursor_column: Some(2),
        ..claude_composer(&["❯\u{a0}"])
    }));
    let token = token_of(&snapshot(&harness, Some("%3")));
    replace_surface_after_first_write(&harness, after_paste);
    let result =
        harness
            .service
            .send_pane_chat_prompt(TASK, &token, text, &[], Some("claude-message"));
    (harness, result)
}

// parity: TS-main-paneChatService-074064
#[test]
fn claude_message_enter_is_pressed_when_the_composer_shows_the_paste() {
    let cases: [(&str, &[&str]); 3] = [
        ("Reply with exactly: OK", &["❯\u{a0}Reply with exactly: OK"]),
        (
            "word000 word001 word002 word003",
            &["❯\u{a0}word000 word001", "  word002 word003"],
        ),
        ("a\nb\nc\nd\ne", &["❯\u{a0}[Pasted text #1 +4 lines]"]),
    ];
    for (text, composer_rows) in cases {
        let (harness, result) =
            send_with_claude_screen_after_paste(text, claude_composer(composer_rows));
        result.unwrap();
        assert_eq!(
            harness.bridge.written_bytes(),
            vec![bracketed(text), "\r".to_string()],
            "{text}"
        );
    }
}

// parity: TS-main-paneChatService-39d0e0
#[test]
fn claude_message_enter_is_withheld_when_the_composer_changed_cleared_or_a_prompt_appeared() {
    let border = "─".repeat(BORDER_WIDTH);
    let prompt_screen = PaneChatSurface {
        viewport_lines: lines(&[&border, " Do you want to proceed?", " ❯ 1. Yes", "   2. No"]),
        cursor_row: Some(2),
        cursor_column: Some(1),
        input_line: Some(" ❯ 1. Yes".to_string()),
        ..surface()
    };
    let after_pastes = [
        claude_composer(&["❯\u{a0}Reply with exactly: RACE"]),
        PaneChatSurface {
            cursor_column: Some(2),
            ..claude_composer(&["❯\u{a0}"])
        },
        prompt_screen,
    ];
    for after_paste in after_pastes {
        let (harness, result) =
            send_with_claude_screen_after_paste("Reply with exactly: OK", after_paste);
        assert_eq!(name_of(result), "PaneChatDeliveryUncertainError");
        assert_eq!(
            harness.bridge.written_bytes(),
            vec![bracketed("Reply with exactly: OK")]
        );
    }
}

// parity: TS-main-paneChatService-88a821
#[test]
fn a_navigation_key_and_an_interrupt_each_verify_the_target_once() {
    let harness = Harness::new();
    let token = token_of(&snapshot(&harness, None));
    harness.surfaces.clear_calls();
    harness
        .service
        .send_pane_chat_key(TASK, &token, "down")
        .unwrap();
    assert_eq!(harness.surfaces.calls().len(), 1);
    harness.surfaces.clear_calls();
    harness.service.interrupt_pane_chat(TASK, &token).unwrap();
    assert_eq!(harness.surfaces.calls().len(), 1);
}

// parity: TS-main-paneChatService-16f160
#[test]
fn current_model_settings_and_selectable_options_come_from_the_exact_session() {
    let harness = Harness::new();
    bind_codex_session(&harness, "gpt-sol");
    let settings = snapshot(&harness, None).model_settings.expect("모델 설정");
    assert_eq!(
        serde_json::to_value(settings).unwrap(),
        json!({
            "model": "gpt-sol", "effort": "low", "isFast": false,
            "models": [
                { "id": "gpt-sol", "label": "GPT Sol", "effortLevels": [{ "id": "low", "label": "Light" }, { "id": "high", "label": "High" }], "supportsFast": true },
                { "id": "gpt-luna", "label": "GPT Luna", "effortLevels": [], "supportsFast": false },
            ],
        })
    );
    assert!(
        harness
            .sessions
            .tail_calls()
            .contains(&"/allowed/exact.jsonl".to_string())
    );
}

struct NoSessionCatalog;

impl PaneChatSessionCatalog for NoSessionCatalog {
    fn read_sessions(
        &self,
        _provider: PaneChatProvider,
        _context: &AiSessionReaderContext,
    ) -> AiResult<Vec<AggregatedAiSession>> {
        Ok(Vec::new())
    }

    fn find_codex_main_thread_rollout(
        &self,
        _context: &AiSessionReaderContext,
        _thread_id: &str,
    ) -> AiResult<Option<String>> {
        Ok(None)
    }
}

/// 기록 끝부분만 실제 구현(파일 수정 시각 캐시)으로 읽고 나머지는 가짜에 맡긴다
struct RealTailSessions {
    fake: Arc<FakeSessions>,
    real: AiPaneChatSessionRecords,
}

impl PaneChatSessionRecords for RealTailSessions {
    fn read_session_detail(
        &self,
        provider: PaneChatProvider,
        scope: &PaneChatReaderScope,
        session_id: &str,
        source_ref: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Option<AggregatedAiSessionDetail>, String> {
        self.fake
            .read_session_detail(provider, scope, session_id, source_ref, cursor, limit)
    }

    fn find_codex_main_thread_rollout(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
    ) -> Result<Option<String>, String> {
        self.fake.find_codex_main_thread_rollout(scope, thread_id)
    }

    fn read_codex_empty_thread(
        &self,
        scope: &PaneChatReaderScope,
        thread_id: &str,
        socket: &Option<Option<String>>,
    ) -> Option<String> {
        self.fake.read_codex_empty_thread(scope, thread_id, socket)
    }

    fn send_codex_thread_input(
        &self,
        delivery: &CodexThreadDelivery,
        before_send: BeforeSend<'_>,
        on_write: &mut dyn FnMut(),
    ) -> Result<(), PaneChatError> {
        self.fake
            .send_codex_thread_input(delivery, before_send, on_write)
    }

    fn read_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
    ) -> Result<PaneChatModelSettings, String> {
        self.fake.read_codex_thread_settings(address)
    }

    fn update_codex_thread_settings(
        &self,
        address: &CodexThreadAddress,
        change: &PaneChatModelSettingChange,
        before_write: BeforeSend<'_>,
    ) -> Result<(), PaneChatError> {
        self.fake
            .update_codex_thread_settings(address, change, before_write)
    }

    fn read_recent_records(
        &self,
        source_ref: &str,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        self.real.read_recent_records(source_ref, ssh_host)
    }

    fn read_json_lines_head(
        &self,
        file_path: &str,
        max_lines: usize,
        ssh_host: Option<&str>,
    ) -> Result<Vec<Value>, String> {
        self.fake
            .read_json_lines_head(file_path, max_lines, ssh_host)
    }

    fn home_directory(&self, ssh_host: Option<&str>) -> Result<String, String> {
        self.fake.home_directory(ssh_host)
    }

    fn path_exists(&self, target_path: &str, ssh_host: Option<&str>) -> Result<bool, String> {
        self.fake.path_exists(target_path, ssh_host)
    }

    fn read_text_file(&self, target_path: &str, ssh_host: Option<&str>) -> Result<String, String> {
        self.fake.read_text_file(target_path, ssh_host)
    }

    fn find_session_binding(
        &self,
        query: &PaneChatBindingQuery,
    ) -> Result<Option<PaneChatSessionBinding>, String> {
        self.fake.find_session_binding(query)
    }

    fn find_process_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        process: &PaneChatProcess,
    ) -> Option<PaneChatSessionBinding> {
        self.fake.find_process_session_binding(scope, process)
    }

    fn find_reported_session_binding(
        &self,
        scope: &PaneChatReaderScope,
        provider: PaneChatProvider,
        session_id: &str,
    ) -> Result<Option<PaneChatSessionBinding>, String> {
        self.fake
            .find_reported_session_binding(scope, provider, session_id)
    }
}

// parity: TS-main-paneChatService-8b9401
#[test]
fn an_unchanged_remote_transcript_is_not_fetched_again_for_model_settings() {
    let remote_file = "/remote/exact-control-model-cache.jsonl";
    let remote = ScriptedShellRunner::new();
    let mtime_seconds = Arc::new(Mutex::new("1700000000".to_string()));
    let tail_lines = Arc::new(Mutex::new(
        r#"{"type":"assistant","message":{"model":"claude-sonnet-4-6"}}"#.to_string(),
    ));
    let (mtime, tail) = (mtime_seconds.clone(), tail_lines.clone());
    remote.respond(move |command, _| {
        if command.contains("stat -c %Y") {
            return Some(Ok(mtime.lock().unwrap().clone()));
        }
        command
            .starts_with("tail -n 200")
            .then(|| Ok(tail.lock().unwrap().clone()))
    });
    let host = SessionHost::local(
        "/home",
        BTreeMap::new(),
        Arc::new(ShellSessionCommandRunner::new(remote.clone())),
    );
    let harness = Harness::with_session_records(move |fake| {
        let bindings = Arc::new(PaneChatSessionBindings::new(Arc::new(NoSessionCatalog)));
        Arc::new(RealTailSessions {
            fake,
            real: AiPaneChatSessionRecords::new(host, None, bindings),
        })
    });
    let mut remote_surface = surface();
    remote_surface.task.ssh_host = Some("remote-host".to_string());
    harness.surfaces.set(Some(remote_surface));
    harness
        .sessions
        .set_binding(Some(binding("exact-session", remote_file)));
    harness.respond_process_table(&claude_table());
    let model = |harness: &Harness| {
        let reading = harness
            .service
            .read_pane_chat(TASK, Some("%3"), PaneChatReadOptions::default())
            .unwrap();
        reading
            .snapshot
            .model_settings
            .and_then(|settings| settings.model)
    };
    let tail_reads = || {
        remote
            .calls()
            .iter()
            .filter(|call| {
                call.command.starts_with("tail -n 200")
                    && call.ssh_host.as_deref() == Some("remote-host")
            })
            .count()
    };
    assert_eq!(model(&harness).as_deref(), Some("sonnet"));
    assert_eq!(model(&harness).as_deref(), Some("sonnet"));
    assert_eq!(tail_reads(), 1);
    *mtime_seconds.lock().unwrap() = "1700000005".to_string();
    *tail_lines.lock().unwrap() =
        r#"{"type":"assistant","message":{"model":"claude-opus-4-6"}}"#.to_string();
    assert_eq!(model(&harness).as_deref(), Some("opus"));
    assert_eq!(tail_reads(), 2);
    let last_tail = remote
        .calls()
        .into_iter()
        .rfind(|call| call.command.starts_with("tail -n 200"))
        .unwrap();
    assert!(last_tail.command.contains(remote_file));
}

// parity: TS-main-paneChatService-63eaad
#[test]
fn claude_picker_metadata_is_read_without_opening_a_cli_picker() {
    let harness = Harness::new();
    harness.respond_all_commands(&claude_table());
    harness.sessions.set_tail(Ok(vec![
        json!({ "type": "assistant", "message": { "model": "claude-sonnet-4-6" } }),
    ]));
    let shown = snapshot(&harness, Some("%3"));
    assert_eq!(shown.model_picker.is_api_supported, Some(false));
    assert_eq!(
        shown.composer.as_ref().unwrap().can_send_while_busy,
        Some(false)
    );
    let settings = harness
        .service
        .get_pane_chat_model_settings(TASK, &token_of(&shown))
        .unwrap();
    assert_eq!(settings.model.as_deref(), Some("sonnet"));
    assert!(settings.models.iter().any(|model| model.id == "sonnet"));
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-f44375
#[test]
fn transcript_and_model_metadata_are_read_concurrently() {
    let harness = Harness::new();
    let tail_started = Arc::new((Mutex::new(false), Condvar::new()));
    let saw_tail_while_reading = Arc::new(AtomicBool::new(false));
    let started = tail_started.clone();
    harness.sessions.respond_tail(move |_, _| {
        *started.0.lock().unwrap() = true;
        started.1.notify_all();
        Err("No transcript tail".to_string())
    });
    let (waiting, saw) = (tail_started.clone(), saw_tail_while_reading.clone());
    harness.sessions.respond_detail(move |_| {
        // 대화 기록 읽기가 막혀 있는 동안 모델 설정 읽기가 시작되는지를 기다려 본다
        let flag = waiting.0.lock().unwrap();
        let (flag, _) = waiting
            .1
            .wait_timeout_while(flag, Duration::from_secs(5), |is_started| !*is_started)
            .unwrap();
        saw.store(*flag, Ordering::SeqCst);
        Ok(Some(detail(
            "exact-session",
            AiSessionProvider::Codex,
            Vec::new(),
            None,
        )))
    });
    let shown = snapshot(&harness, Some("%3"));
    assert!(saw_tail_while_reading.load(Ordering::SeqCst));
    assert_eq!(shown.transcript.status, PaneChatTranscriptStatus::Available);
}

// parity: TS-main-paneChatService-5f038b
#[test]
fn an_empty_claude_descriptor_session_loads_before_its_first_transcript_exists() {
    let harness = Harness::new();
    harness.respond_all_commands(&claude_table());
    harness
        .sessions
        .set_process_binding(Some(PaneChatSessionBinding {
            is_pending: true,
            ..binding("empty-claude", "/allowed/not-created-yet.jsonl")
        }));
    let shown = snapshot(&harness, Some("%3"));
    assert!(shown.can_send);
    assert_eq!(shown.transcript.status, PaneChatTranscriptStatus::Available);
    assert_eq!(shown.transcript.session_id.as_deref(), Some("empty-claude"));
    assert!(shown.transcript.messages.is_empty());
    let settings = shown.model_settings.clone().expect("모델 설정");
    assert_eq!(settings.model, None);
    assert!(settings.models.iter().any(|model| model.id == "sonnet"));
    let read = harness
        .service
        .get_pane_chat_model_settings(TASK, &token_of(&shown))
        .unwrap();
    assert_eq!(read.model, None);
    assert!(
        harness
            .sessions
            .detail_calls()
            .iter()
            .all(|call| call.provider != PaneChatProvider::Claude)
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-07d75d
#[test]
fn exact_herdr_session_reports_are_used_without_a_footer_or_cli_commands() {
    let harness = Harness::new();
    harness.surfaces.set(Some(PaneChatSurface {
        agent_session: Some(HerdrAgentSession {
            provider: AiSessionProvider::Codex,
            session_id: "reported".to_string(),
        }),
        ..surface()
    }));
    harness.sessions.set_binding(None);
    let reported_calls = Arc::new(Mutex::new(Vec::new()));
    let calls = reported_calls.clone();
    *harness.sessions.reported_binding.lock().unwrap() = Arc::new(
        move |scope: &PaneChatReaderScope, provider: PaneChatProvider, session_id: &str| {
            calls.lock().unwrap().push((
                scope.worktree_path.clone(),
                provider,
                session_id.to_string(),
            ));
            Ok(Some(binding("reported", "/allowed/reported.jsonl")))
        },
    );
    harness.sessions.respond_detail(|_| {
        Ok(Some(detail(
            "reported",
            AiSessionProvider::Codex,
            Vec::new(),
            None,
        )))
    });
    assert_eq!(
        snapshot(&harness, Some("%3"))
            .transcript
            .session_id
            .as_deref(),
        Some("reported")
    );
    assert!(reported_calls.lock().unwrap().contains(&(
        Some("/repo".to_string()),
        PaneChatProvider::Codex,
        "reported".to_string()
    )));
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-7af379
#[test]
fn the_current_process_descriptor_wins_over_an_older_hook_session() {
    let harness = Harness::new();
    harness
        .sessions
        .set_process_binding(Some(binding("process-session", "/allowed/process.jsonl")));
    let hook_lookups = Arc::new(Mutex::new(0));
    let lookups = hook_lookups.clone();
    *harness.sessions.binding.lock().unwrap() = Arc::new(move |_: &PaneChatBindingQuery| {
        *lookups.lock().unwrap() += 1;
        Ok(Some(binding("exact-session", "/allowed/exact.jsonl")))
    });
    harness.sessions.respond_detail(|_| {
        Ok(Some(detail(
            "process-session",
            AiSessionProvider::Codex,
            Vec::new(),
            None,
        )))
    });
    assert_eq!(
        snapshot(&harness, Some("%3"))
            .transcript
            .session_id
            .as_deref(),
        Some("process-session")
    );
    assert_eq!(*hook_lookups.lock().unwrap(), 0);
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-e6d053
#[test]
fn model_settings_are_omitted_when_the_transcript_tail_cannot_be_read() {
    let harness = Harness::new();
    let shown = snapshot(&harness, None);
    assert_eq!(shown.transcript.status, PaneChatTranscriptStatus::Available);
    assert_eq!(shown.model_settings, None);
    assert!(
        serde_json::to_value(&shown)
            .unwrap()
            .get("modelSettings")
            .is_none()
    );
}

fn change(harness: &Harness, token: &str, change: Value) -> Result<(), PaneChatError> {
    harness
        .service
        .change_pane_chat_model_setting(TASK, token, &change)
}

// parity: TS-main-paneChatService-55c4d5
#[test]
fn a_model_setting_changes_by_pasting_the_command_built_from_selectable_options() {
    let harness = Harness::new();
    bind_codex_session(&harness, "gpt-sol");
    let token = token_of(&snapshot(&harness, None));
    change(
        &harness,
        &token,
        json!({ "kind": "effort", "effort": "high" }),
    )
    .unwrap();
    assert_eq!(
        harness.bridge.written_bytes(),
        vec![bracketed("/model --reasoning=high"), "\r".to_string()]
    );
    harness.bridge.clear();
    change(&harness, &token, json!({ "kind": "fast", "isFast": true })).unwrap();
    assert_eq!(
        harness.bridge.written_bytes(),
        vec![bracketed("/model --speed=priority"), "\r".to_string()]
    );
}

// parity: TS-main-paneChatService-343322
#[test]
fn the_target_is_verified_the_same_number_of_times_however_long_the_model_command_is() {
    let harness = Harness::new();
    bind_codex_session(&harness, "gpt-sol");
    let token = token_of(&snapshot(&harness, None));
    harness.surfaces.clear_calls();
    change(
        &harness,
        &token,
        json!({ "kind": "model", "model": "gpt-luna" }),
    )
    .unwrap();
    let reads_for_short_command = harness.surfaces.calls().len();
    harness.surfaces.clear_calls();
    change(
        &harness,
        &token,
        json!({ "kind": "effort", "effort": "high" }),
    )
    .unwrap();
    assert_eq!(
        harness.bridge.written_bytes(),
        vec![
            bracketed("/model gpt-luna"),
            "\r".to_string(),
            bracketed("/model --reasoning=high"),
            "\r".to_string()
        ]
    );
    assert_eq!(harness.surfaces.calls().len(), reads_for_short_command);
}

// parity: TS-main-paneChatService-6f884b
#[test]
fn unselectable_or_malformed_model_changes_are_refused_without_writing() {
    let changes = [
        json!({ "kind": "model", "model": "gpt-sol\rrm -rf /" }),
        json!({ "kind": "effort", "effort": "ultra" }),
        json!({ "kind": "effort" }),
        json!({ "kind": "theme", "value": "dark" }),
    ];
    for requested in changes {
        let harness = Harness::new();
        bind_codex_session(&harness, "gpt-sol");
        let token = token_of(&snapshot(&harness, None));
        assert_eq!(
            name_of(change(&harness, &token, requested.clone())),
            "PaneChatInputError",
            "{requested}"
        );
        assert!(harness.writes().is_empty());
    }
}

// parity: TS-main-paneChatService-554894
#[test]
fn fast_mode_is_refused_for_a_model_without_it_and_while_the_cli_input_is_occupied() {
    let harness = Harness::new();
    bind_codex_session(&harness, "gpt-luna");
    let token = token_of(&snapshot(&harness, None));
    assert_eq!(
        name_of(change(
            &harness,
            &token,
            json!({ "kind": "fast", "isFast": true })
        )),
        "PaneChatInputError"
    );
    bind_codex_session(&harness, "gpt-sol");
    harness.surfaces.set(Some(PaneChatSurface {
        input_line: Some("› unfinished draft".to_string()),
        cursor_column: Some(18),
        ..surface()
    }));
    assert_eq!(
        name_of(change(
            &harness,
            &token,
            json!({ "kind": "effort", "effort": "high" })
        )),
        "PaneChatInputError"
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-09f37a
#[test]
fn only_the_hook_bound_exact_session_is_read_and_newest_first_order_is_kept() {
    let harness = Harness::new();
    let messages = vec![
        message(AiMessageRole::Assistant, "New"),
        message(AiMessageRole::User, "Old"),
    ];
    let replies = messages.clone();
    harness.sessions.respond_detail(move |_| {
        Ok(Some(detail(
            "exact-session",
            AiSessionProvider::Codex,
            replies.clone(),
            None,
        )))
    });
    let shown = snapshot(&harness, None);
    assert_eq!(shown.transcript.status, PaneChatTranscriptStatus::Available);
    assert_eq!(
        shown.transcript.session_id.as_deref(),
        Some("exact-session")
    );
    assert_eq!(shown.transcript.messages, messages);
    let read = harness.sessions.detail_calls().pop().unwrap();
    assert_eq!(read.scope.worktree_path.as_deref(), Some("/repo"));
    assert_eq!(
        (
            read.session_id.as_str(),
            read.source_ref.as_str(),
            read.cursor.as_deref(),
            read.limit
        ),
        ("exact-session", "/allowed/exact.jsonl", None, 100)
    );
}

// parity: TS-main-paneChatService-682b92
#[test]
fn a_dim_claude_suggestion_after_a_reply_counts_as_an_empty_composer() {
    let harness = Harness::new();
    harness.respond_all_commands(&claude_table());
    let suggestion = PaneChatSurface {
        input_line: Some("❯\u{a0}숨김 폴더 포함하면 몇 개야?".to_string()),
        cursor_column: Some(2),
        is_input_placeholder: true,
        ..surface()
    };
    show_claude_echoing_pastes(&harness, suggestion.clone());
    let shown = snapshot(&harness, Some("%3"));
    assert_eq!(shown.input_state, PaneChatInputState::Ready);
    send(&harness, &token_of(&shown), "hello").unwrap();
    assert_eq!(
        harness.writes(),
        Harness::expected_writes(TASK, "%3", &[&bracketed("hello"), "\r"])
    );
    harness.surfaces.set(Some(PaneChatSurface {
        is_input_placeholder: false,
        ..suggestion
    }));
    assert_eq!(
        snapshot(&harness, Some("%3")).input_state,
        PaneChatInputState::Occupied
    );
}

// parity: TS-main-paneChatService-fb70b1
#[test]
fn text_typed_directly_into_the_claude_composer_stays_blocked() {
    let harness = Harness::new();
    harness.respond_all_commands(&claude_table());
    harness.surfaces.set(Some(PaneChatSurface {
        input_line: Some("❯\u{a0}abc".to_string()),
        cursor_column: Some(5),
        is_input_placeholder: false,
        ..surface()
    }));
    let shown = snapshot(&harness, Some("%3"));
    assert_eq!(shown.input_state, PaneChatInputState::Occupied);
    assert_eq!(
        name_of(send(&harness, &token_of(&shown), "hello")),
        "PaneChatInputError"
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-56568c
#[test]
fn a_dim_dynamic_suggestion_counts_as_empty_on_a_codex_version_without_display_mode() {
    let harness = Harness::new();
    let rows = ["› Summarize recent commits", "", "  gpt-sol low · /repo"];
    harness.surfaces.set(Some(PaneChatSurface {
        input_line: Some(rows[0].to_string()),
        cursor_column: Some(2),
        cursor_row: Some(0),
        viewport_lines: lines(&rows),
        is_input_placeholder: true,
        ..surface()
    }));
    assert_eq!(
        snapshot(&harness, Some("%3")).input_state,
        PaneChatInputState::Ready
    );
}

// parity: TS-main-paneChatService-fe5b17
#[test]
fn a_terminal_draft_is_kept_and_prompt_or_model_commands_are_refused_while_occupied() {
    let harness = Harness::new();
    harness.surfaces.set(Some(PaneChatSurface {
        input_line: Some("› unfinished draft".to_string()),
        cursor_column: Some(18),
        ..surface()
    }));
    let shown = snapshot(&harness, None);
    assert!(shown.can_send);
    assert_eq!(shown.input_state, PaneChatInputState::Occupied);
    let token = token_of(&shown);
    assert_eq!(
        name_of(send(&harness, &token, "Next prompt")),
        "PaneChatInputError"
    );
    assert_eq!(
        name_of(harness.service.open_pane_chat_model_picker(TASK, &token)),
        "PaneChatInputError"
    );
    assert!(harness.writes().is_empty());
    harness.service.interrupt_pane_chat(TASK, &token).unwrap();
    assert_eq!(
        harness.writes().pop(),
        Some((TASK.to_string(), "%3".to_string(), "\x03".to_string()))
    );
}

// parity: TS-main-paneChatService-b06a47
#[test]
fn ambiguous_foreground_provider_families_are_disabled_even_when_one_is_a_child() {
    let harness = Harness::new();
    harness.respond_all_commands(&format!(
        "{}\n201 200 200 200 ttys010 Thu Nov 12 19:00:00 2026 claude claude",
        process_table(DEFAULT_PROCESS_START)
    ));
    let shown = snapshot(&harness, None);
    assert!(!shown.can_send);
    assert_eq!(shown.target_token, None);
}

// parity: TS-main-paneChatService-9b0f89
#[test]
fn a_token_is_retired_when_the_same_cli_changes_conversation() {
    let harness = Harness::new();
    bind_codex_session(&harness, "gpt-sol");
    let first = snapshot(&harness, Some("%3"));
    harness
        .sessions
        .set_binding(Some(binding("session-b", "/allowed/b.jsonl")));
    harness.sessions.respond_detail(|_| {
        Ok(Some(detail(
            "session-b",
            AiSessionProvider::Codex,
            Vec::new(),
            None,
        )))
    });
    let crossed = send(&harness, &token_of(&first), "Must not cross sessions");
    assert_eq!(name_of(crossed), "PaneChatTargetChangedError");
    assert!(harness.writes().is_empty());
    let next = snapshot(&harness, Some("%3"));
    assert_ne!(next.target_token, first.target_token);
    assert_eq!(next.transcript.session_id.as_deref(), Some("session-b"));
}

// parity: TS-main-paneChatService-ae48cf
#[test]
fn sending_is_refused_when_the_exact_conversation_identity_is_unavailable() {
    let harness = Harness::new();
    harness.sessions.set_binding(None);
    let shown = snapshot(&harness, Some("%3"));
    assert!(!shown.can_send);
    assert_eq!(shown.target_token, None);
    assert_eq!(
        shown.transcript.status,
        PaneChatTranscriptStatus::Unavailable
    );
}

// parity: TS-main-paneChatService-eaf416
#[test]
fn a_transcript_finished_after_a_same_process_session_change_is_discarded() {
    let harness = Harness::new();
    bind_codex_session(&harness, "gpt-sol");
    let sessions = harness.sessions.clone();
    let has_switched = AtomicBool::new(false);
    harness.sessions.respond_detail(move |_| {
        if !has_switched.swap(true, Ordering::SeqCst) {
            sessions.set_binding(Some(binding("session-b", "/allowed/b.jsonl")));
            let leaked = vec![message(AiMessageRole::Assistant, "Must not leak A")];
            return Ok(Some(detail(
                "exact-session",
                AiSessionProvider::Codex,
                leaked,
                None,
            )));
        }
        Ok(Some(detail(
            "session-b",
            AiSessionProvider::Codex,
            Vec::new(),
            None,
        )))
    });
    let shown = snapshot(&harness, Some("%3"));
    assert!(shown.transcript.messages.is_empty());
    assert!(!shown.can_send);
}

// parity: TS-main-paneChatService-37b95c
#[test]
fn a_retired_token_is_not_resurrected_after_a_to_b_to_a_in_the_same_process() {
    let harness = Harness::new();
    bind_codex_session(&harness, "gpt-sol");
    let a = snapshot(&harness, Some("%3"));
    harness
        .sessions
        .set_binding(Some(binding("session-b", "/allowed/b.jsonl")));
    harness.sessions.respond_detail(|_| {
        Ok(Some(detail(
            "session-b",
            AiSessionProvider::Codex,
            Vec::new(),
            None,
        )))
    });
    let b = snapshot(&harness, Some("%3"));
    bind_codex_session(&harness, "gpt-sol");
    let returned = snapshot(&harness, Some("%3"));
    assert_ne!(returned.target_token, a.target_token);
    assert_ne!(returned.target_token, b.target_token);
    assert_eq!(returned.conversation_key, a.conversation_key);
    assert_ne!(b.conversation_key, a.conversation_key);
    assert_eq!(
        name_of(send(&harness, &token_of(&a), "Stale A")),
        "PaneChatTargetChangedError"
    );
    assert!(harness.writes().is_empty());
}

// parity: TS-main-paneChatService-d3fccd
#[test]
fn old_input_is_rejected_after_the_host_changes_or_the_pane_closes() {
    let mut remote = surface();
    remote.task.ssh_host = Some("another-host".to_string());
    for changed in [Some(remote), None] {
        let harness = Harness::new();
        let a = snapshot(&harness, Some("%3"));
        harness.surfaces.set(changed);
        assert_eq!(
            name_of(send(&harness, &token_of(&a), "Do not deliver")),
            "PaneChatTargetChangedError"
        );
        assert!(harness.writes().is_empty());
    }
}

// parity: TS-main-paneChatService-78f7fd
#[test]
fn a_conversation_draft_key_is_not_shared_across_hosts_with_the_same_session_id() {
    let harness = Harness::new();
    let local = snapshot(&harness, Some("%3"));
    let mut remote_surface = surface();
    remote_surface.task.ssh_host = Some("another-host".to_string());
    harness.surfaces.set(Some(remote_surface));
    let remote = snapshot(&harness, Some("%3"));
    assert!(local.conversation_key.is_some());
    assert!(remote.conversation_key.is_some());
    assert_ne!(remote.conversation_key, local.conversation_key);
}
