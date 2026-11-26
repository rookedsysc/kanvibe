//! 한 기록 파일에서 한 대화가 남긴 메시지. 같은 이벤트 목록을 같은 조건으로 다시 받으면 지난번에 읽은 곳 뒤로 붙은
//! 이벤트만 읽어, 긴 대화를 보는 동안 매번 전체를 다시 만들지 않는다.
//!
//! TS는 이벤트 배열을 키로 하는 `WeakMap`에 기억한다. 여기서는 이어 읽기 목록은 `log_id`로, 통째로 읽은 목록은
//! `Weak` 포인터로 같은 목록인지 가린다. 파일을 처음부터 다시 읽으면 목록이 바뀌어 함께 버려진다.

use serde_json::{Value, json};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use super::events::{
    extract_claude_event_messages, is_claude_api_request_log, is_claude_image_companion,
    string_field,
};
use crate::javascript::stringify;
use crate::jsonl::AppendedJsonLines;
use crate::message_files::with_tool_call;
use crate::message_text::{make_preview_message, truncate_text_to};
use crate::session_listing::determine_match_scope;
use crate::types::{AggregatedAiMessage, AiMessageRole, AiSessionReaderContext};

const SESSION_TITLE_LENGTH: usize = 80;
/// 검색어처럼 조건이 계속 바뀌는 조회가 한 목록에 결과를 끝없이 쌓지 않도록 조건 수를 제한한다
const MAX_TRANSCRIPTS_PER_EVENT_LIST: usize = 8;
/// `WeakMap`과 달리 이어 읽기 목록은 사라졌는지 알 수 없어, 기억하는 목록 수에 상한을 둔다
const MAX_EVENT_LISTS: usize = 64;

/// 상세 조회가 읽은 이벤트 목록
#[derive(Clone)]
pub(super) enum ClaudeEventLog {
    /// 뒤에 붙은 줄만 이어 읽은 목록
    Appended(AppendedJsonLines),
    /// 파일을 통째로 읽은 목록
    Whole(Arc<Vec<Value>>),
}

impl ClaudeEventLog {
    fn len(&self) -> usize {
        match self {
            Self::Appended(appended) => appended.lines.len(),
            Self::Whole(events) => events.len(),
        }
    }

    fn event(&self, index: usize) -> &Value {
        match self {
            Self::Appended(appended) => &appended.lines[index],
            Self::Whole(events) => &events[index],
        }
    }

    fn identity(&self) -> EventLogIdentity {
        match self {
            Self::Appended(appended) => EventLogIdentity::Appended(appended.log_id),
            Self::Whole(events) => EventLogIdentity::Whole(Arc::downgrade(events)),
        }
    }
}

enum EventLogIdentity {
    Appended(u64),
    Whole(Weak<Vec<Value>>),
}

impl EventLogIdentity {
    fn is_same_log(&self, log: &ClaudeEventLog) -> bool {
        match (self, log) {
            (Self::Appended(log_id), ClaudeEventLog::Appended(appended)) => {
                *log_id == appended.log_id
            }
            (Self::Whole(weak), ClaudeEventLog::Whole(events)) => {
                std::ptr::eq(weak.as_ptr(), Arc::as_ptr(events))
            }
            _ => false,
        }
    }

    fn is_alive(&self) -> bool {
        match self {
            Self::Appended(_) => true,
            Self::Whole(weak) => weak.strong_count() > 0,
        }
    }

    /// 파일을 다시 처음부터 이어 읽기 시작해 새 `log_id`를 받으면, 같은 파일의 옛 이어 읽기 목록은 다시 오지 않는다
    fn is_replaced_by(&self, log: &ClaudeEventLog) -> bool {
        matches!(
            (self, log),
            (Self::Appended(log_id), ClaudeEventLog::Appended(appended)) if *log_id != appended.log_id
        )
    }
}

/// 한 대화의 메시지와, 이벤트를 어디까지 읽어 만들었는지
#[derive(Default)]
pub(super) struct ClaudeSessionTranscript {
    read_event_count: usize,
    is_session_in_scope: bool,
    pub title: Option<String>,
    pub matched_path: Option<String>,
    /// 파일에 적힌 순서
    pub messages: Vec<AggregatedAiMessage>,
    /// messages의 최신순 위치. 메시지가 늘면 버리고 다음에 다시 만든다
    pub newest_first_order: Option<Vec<usize>>,
}

/// 한 대화의 기억. 전역 목록 잠금은 이것을 찾는 동안만 쥐고, 파싱과 조회는 대화별 잠금으로 한다
type SharedTranscript = Arc<Mutex<ClaudeSessionTranscript>>;

struct TranscriptSet {
    identity: EventLogIdentity,
    file_path: String,
    /// 넣은 순서대로. 가득 차면 맨 앞을 버린다
    transcripts: Vec<(String, SharedTranscript)>,
}

static TRANSCRIPT_SETS: LazyLock<Mutex<Vec<TranscriptSet>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// TS `JSON.stringify([filePath, sessionId, context])`에 해당하는 조건 키
fn transcript_conditions(
    file_path: &str,
    session_id: &str,
    context: &AiSessionReaderContext,
) -> String {
    stringify(&json!([file_path, session_id, {
        "worktreePath": context.worktree_path,
        "repoPath": context.repo_path,
        "query": context.query,
        "roles": context.roles,
        "includeInstructions": context.include_instructions,
        "sshHost": context.ssh_host(),
    }]))
}

/// 이벤트 목록에 맞는 대화를 최신 상태로 만든 뒤 `inspect`에 넘긴다. 기억해 둔 대화는 붙은 이벤트만 더 읽는다
pub(super) fn with_session_transcript<R>(
    log: &ClaudeEventLog,
    file_path: &str,
    session_id: &str,
    context: &AiSessionReaderContext,
    inspect: impl FnOnce(&mut ClaudeSessionTranscript) -> R,
) -> R {
    let conditions = transcript_conditions(file_path, session_id, context);
    let shared = shared_transcript(log, file_path, conditions);
    let mut transcript = shared.lock().expect("Claude 대화");
    consume_new_events(&mut transcript, log, file_path, session_id, context);
    inspect(&mut transcript)
}

/// 조건에 맞는 대화 기억을 찾거나 새로 넣는다. 전역 잠금은 이 안에서만 쥔다
fn shared_transcript(
    log: &ClaudeEventLog,
    file_path: &str,
    conditions: String,
) -> SharedTranscript {
    let mut sets = TRANSCRIPT_SETS.lock().expect("Claude 대화 기억");
    let set_index = find_or_insert_set(&mut sets, log, file_path);
    let transcripts = &mut sets[set_index].transcripts;
    if let Some((_, transcript)) = transcripts.iter().find(|(key, _)| *key == conditions) {
        return transcript.clone();
    }
    if transcripts.len() >= MAX_TRANSCRIPTS_PER_EVENT_LIST {
        transcripts.remove(0);
    }
    let transcript = SharedTranscript::default();
    transcripts.push((conditions, transcript.clone()));
    transcript
}

fn find_or_insert_set(
    sets: &mut Vec<TranscriptSet>,
    log: &ClaudeEventLog,
    file_path: &str,
) -> usize {
    sets.retain(|set| set.identity.is_alive());
    if let Some(index) = sets.iter().position(|set| set.identity.is_same_log(log)) {
        let set = sets.remove(index);
        sets.push(set);
    } else {
        sets.retain(|set| !(set.file_path == file_path && set.identity.is_replaced_by(log)));
        if sets.len() >= MAX_EVENT_LISTS {
            sets.remove(0);
        }
        sets.push(TranscriptSet {
            identity: log.identity(),
            file_path: file_path.to_string(),
            transcripts: Vec::new(),
        });
    }
    sets.len() - 1
}

fn consume_new_events(
    transcript: &mut ClaudeSessionTranscript,
    log: &ClaudeEventLog,
    file_path: &str,
    session_id: &str,
    context: &AiSessionReaderContext,
) {
    while transcript.read_event_count < log.len() {
        let event = log.event(transcript.read_event_count);
        transcript.read_event_count += 1;
        if string_field(event, "sessionId") != Some(session_id) {
            continue;
        }
        let cwd = string_field(event, "cwd");
        if cwd.is_some() {
            transcript.is_session_in_scope = determine_match_scope(cwd, context).is_some();
        }
        if !transcript.is_session_in_scope
            || is_claude_image_companion(event)
            || is_claude_api_request_log(event)
        {
            continue;
        }
        if transcript.matched_path.is_none() {
            transcript.matched_path = cwd.map(str::to_string);
        }
        consume_event_messages(transcript, event, file_path, context);
    }
}

fn consume_event_messages(
    transcript: &mut ClaudeSessionTranscript,
    event: &Value,
    file_path: &str,
    context: &AiSessionReaderContext,
) {
    for message in extract_claude_event_messages(event, file_path) {
        if !is_role_selected(context, message.role) || !matches_detail_query(context, &message) {
            continue;
        }
        if message.role == AiMessageRole::User
            && !message.text.is_empty()
            && transcript.title.is_none()
        {
            transcript.title = Some(truncate_text_to(&message.text, SESSION_TITLE_LENGTH));
        }
        let timestamp = event.get("timestamp").unwrap_or(&Value::Null);
        if let Some(preview) =
            make_preview_message(message.role, timestamp, &message.text, message.attachments)
        {
            let tool_name = message.tool_name.as_deref();
            transcript
                .messages
                .push(with_tool_call(preview, tool_name, &message.files));
            transcript.newest_first_order = None;
        }
    }
}

fn is_role_selected(context: &AiSessionReaderContext, role: AiMessageRole) -> bool {
    match context.roles.as_deref() {
        Some(roles) if !roles.is_empty() => roles.contains(&role),
        _ => true,
    }
}

fn matches_detail_query(
    context: &AiSessionReaderContext,
    message: &super::events::ClaudeEventMessage,
) -> bool {
    let Some(query) = context.query.as_deref().filter(|query| !query.is_empty()) else {
        return true;
    };
    let searchable = std::iter::once(message.text.as_str())
        .chain(
            message
                .attachments
                .iter()
                .map(|attachment| attachment.name.as_str()),
        )
        .collect::<Vec<_>>()
        .join("\n");
    searchable.to_lowercase().contains(&query.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appended(log_id: u64) -> ClaudeEventLog {
        ClaudeEventLog::Appended(AppendedJsonLines {
            log_id,
            lines: Default::default(),
        })
    }

    #[test]
    fn a_restarted_appended_log_drops_the_previous_transcripts_of_the_same_file() {
        let mut sets = Vec::new();
        find_or_insert_set(&mut sets, &appended(1), "/logs/a.jsonl");
        find_or_insert_set(&mut sets, &appended(2), "/logs/b.jsonl");

        find_or_insert_set(&mut sets, &appended(3), "/logs/a.jsonl");

        let remaining: Vec<&str> = sets.iter().map(|set| set.file_path.as_str()).collect();
        assert_eq!(remaining, ["/logs/b.jsonl", "/logs/a.jsonl"]);
        assert!(sets[1].identity.is_same_log(&appended(3)));
    }

    #[test]
    fn reading_one_conversation_does_not_block_reading_another_file() {
        let context =
            AiSessionReaderContext::new(crate::test_support::local_host("/home/user"), None);
        let first_log = ClaudeEventLog::Whole(Arc::new(Vec::new()));
        let second_log = ClaudeEventLog::Whole(Arc::new(Vec::new()));
        let (finished_sender, finished_receiver) = std::sync::mpsc::channel();

        let was_other_read_finished = with_session_transcript(
            &first_log,
            "/logs/first.jsonl",
            "session-1",
            &context,
            |_| {
                let other_context = context.clone();
                std::thread::spawn(move || {
                    with_session_transcript(
                        &second_log,
                        "/logs/second.jsonl",
                        "session-2",
                        &other_context,
                        |_| (),
                    );
                    let _ = finished_sender.send(());
                });
                finished_receiver
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .is_ok()
            },
        );

        assert!(was_other_read_finished);
    }
}
