//! 한 rollout 파일에서 대화 하나의 메시지를 만들고, 같은 이벤트 목록을 다시 받으면 뒤에 붙은 이벤트만 이어 읽는다.
//!
//! TS는 이벤트 배열을 키로 둔 WeakMap에 결과를 걸어 두었다. Rust에서는 이어 읽는 기록은 로그 번호로,
//! 통째로 읽은 기록은 그 목록을 가리키는 약한 참조로 같은 목록인지 가린다.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use serde_json::Value;

use super::messages::{
    create_codex_message_key, event_timestamp, extract_codex_event_messages,
    should_skip_codex_message,
};
use crate::jsonl::AppendedJsonLines;
use crate::message_files::with_tool_call;
use crate::message_text::{make_preview_message, truncate_text_to};
use crate::node_path;
use crate::session_listing::determine_match_scope;
use crate::types::{AggregatedAiMessage, AiSessionReaderContext};

const SESSION_TITLE_LENGTH: usize = 80;

/// 파싱 캐시에 올리는 rollout 이벤트 목록
pub(crate) enum RolloutEvents {
    /// 파일을 처음부터 끝까지 읽은 목록
    Parsed(Vec<Value>),
    /// 뒤에 붙은 줄만 이어 읽는 목록
    Appended(AppendedJsonLines),
}

impl RolloutEvents {
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Parsed(events) => events.len(),
            Self::Appended(lines) => lines.lines.len(),
        }
    }

    pub(crate) fn get(&self, index: usize) -> &Value {
        match self {
            Self::Parsed(events) => &events[index],
            Self::Appended(lines) => &lines.lines[index],
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Value> {
        (0..self.len()).map(|index| self.get(index))
    }
}

/// 한 rollout 파일의 대화가 남긴 메시지와, 그 파일의 이벤트를 어디까지 읽어 만들었는지
#[derive(Default)]
pub(crate) struct CodexSessionTranscript {
    read_event_count: usize,
    /// 파일의 session_meta가 다른 대화나 다른 작업 경로를 가리킨다. 한 번 그렇게 판정되면 뒤에 붙는 줄도 읽지 않는다
    pub is_other_session: bool,
    pub matched_path: Option<String>,
    pub title: Option<String>,
    /// 파일에 적힌 순서
    pub messages: Vec<AggregatedAiMessage>,
    /// messages를 최신순으로 정렬해 둔 것. 메시지가 늘면 버리고 다음에 다시 만든다
    pub newest_first_messages: Option<Vec<AggregatedAiMessage>>,
    seen_messages: HashSet<String>,
    tool_names: HashMap<String, String>,
}

/// 같은 이벤트 목록인지 가리는 열쇠
enum EventListKey {
    Parsed(Weak<RolloutEvents>),
    Appended(u64),
}

impl EventListKey {
    fn of(events: &Arc<RolloutEvents>) -> Self {
        match events.as_ref() {
            RolloutEvents::Parsed(_) => Self::Parsed(Arc::downgrade(events)),
            RolloutEvents::Appended(lines) => Self::Appended(lines.log_id),
        }
    }

    fn is_same(&self, events: &Arc<RolloutEvents>) -> bool {
        match (self, events.as_ref()) {
            (Self::Parsed(known), RolloutEvents::Parsed(_)) => {
                std::ptr::eq(known.as_ptr(), Arc::as_ptr(events))
            }
            (Self::Appended(log_id), RolloutEvents::Appended(lines)) => *log_id == lines.log_id,
            _ => false,
        }
    }

    fn is_alive(&self) -> bool {
        match self {
            Self::Parsed(known) => known.strong_count() > 0,
            Self::Appended(_) => true,
        }
    }
}

type SharedTranscript = Arc<Mutex<CodexSessionTranscript>>;

struct EventListTranscripts {
    key: EventListKey,
    /// 조건 문자열과 그 조건으로 만든 메시지. 앞이 가장 오래됐다
    transcripts: Vec<(String, SharedTranscript)>,
}

/// 검색어처럼 조건이 계속 바뀌는 조회가 한 파일에 결과를 끝없이 쌓지 않도록 조건 수를 제한한다
const MAX_TRANSCRIPTS_PER_EVENT_LIST: usize = 8;
/// 이어 읽는 로그는 약한 참조로 수명을 알 수 없어 기억해 둘 목록 수를 따로 묶는다
const MAX_EVENT_LISTS: usize = 64;

static TRANSCRIPTS: LazyLock<Mutex<Vec<EventListTranscripts>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// TS `JSON.stringify([filePath, sessionId, context])`에 해당하는 조건 문자열
fn transcript_conditions(
    file_path: &str,
    session_id: &str,
    context: &AiSessionReaderContext,
) -> String {
    format!(
        "{file_path}\u{0}{session_id}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}\u{0}{:?}",
        context.worktree_path,
        context.repo_path,
        context.query,
        context.roles,
        context.include_instructions,
        context.cursor,
        context.limit,
        context.ssh_host(),
    )
}

fn shared_transcript(events: &Arc<RolloutEvents>, conditions: String) -> SharedTranscript {
    let mut lists = TRANSCRIPTS.lock().expect("Codex 대화 캐시");
    lists.retain(|list| list.key.is_alive());
    let index = match lists.iter().position(|list| list.key.is_same(events)) {
        Some(index) => index,
        None => {
            if lists.len() >= MAX_EVENT_LISTS {
                lists.remove(0);
            }
            lists.push(EventListTranscripts {
                key: EventListKey::of(events),
                transcripts: Vec::new(),
            });
            lists.len() - 1
        }
    };
    let transcripts = &mut lists[index].transcripts;
    if let Some((_, transcript)) = transcripts.iter().find(|(known, _)| *known == conditions) {
        return transcript.clone();
    }
    if transcripts.len() >= MAX_TRANSCRIPTS_PER_EVENT_LIST {
        transcripts.remove(0);
    }
    let transcript = SharedTranscript::default();
    transcripts.push((conditions, transcript.clone()));
    transcript
}

/// 이벤트 목록에서 한 대화의 메시지를 만든다. 같은 목록을 같은 조건으로 다시 받으면 지난번에 읽은 곳 뒤로 붙은 이벤트만 읽는다
pub(crate) fn read_codex_session_transcript(
    events: &Arc<RolloutEvents>,
    file_path: &str,
    session_id: &str,
    context: &AiSessionReaderContext,
) -> SharedTranscript {
    let shared = shared_transcript(
        events,
        transcript_conditions(file_path, session_id, context),
    );
    {
        let mut transcript = shared.lock().expect("Codex 대화");
        while !transcript.is_other_session && transcript.read_event_count < events.len() {
            let event = events.get(transcript.read_event_count);
            read_transcript_event(&mut transcript, event, file_path, session_id, context);
            transcript.read_event_count += 1;
        }
    }
    shared
}

fn read_transcript_event(
    transcript: &mut CodexSessionTranscript,
    event: &Value,
    file_path: &str,
    session_id: &str,
    context: &AiSessionReaderContext,
) {
    if event.get("type").and_then(Value::as_str) == Some("session_meta") {
        read_transcript_meta(transcript, event, file_path, session_id, context);
        return;
    }
    let messages = extract_codex_event_messages(event, file_path, &mut transcript.tool_names);
    for message in messages {
        if !context.include_instructions && should_skip_codex_message(message.role, &message.text) {
            continue;
        }
        let timestamp = event_timestamp(event);
        let message_key =
            create_codex_message_key(message.role, timestamp, &message.text, &message.attachments);
        if !transcript.seen_messages.insert(message_key) {
            continue;
        }
        if !is_selected_message(context, &message.role, &message.text, &message.attachments) {
            continue;
        }
        if message.role == crate::types::AiMessageRole::User && transcript.title.is_none() {
            transcript.title = Some(truncate_text_to(&message.text, SESSION_TITLE_LENGTH));
        }
        let preview = make_preview_message(
            message.role,
            timestamp,
            &message.text,
            message.attachments.clone(),
        );
        if let Some(preview) = preview {
            transcript.messages.push(with_tool_call(
                preview,
                message.tool_name.as_deref(),
                &message.files,
            ));
            transcript.newest_first_messages = None;
            #[cfg(test)]
            tests::record_built_message();
        }
    }
}

fn read_transcript_meta(
    transcript: &mut CodexSessionTranscript,
    event: &Value,
    file_path: &str,
    session_id: &str,
    context: &AiSessionReaderContext,
) {
    let payload = event.get("payload").filter(|payload| !payload.is_null());
    let candidate_session_id = payload
        .and_then(|payload| payload.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| node_path::basename_without(file_path, ".jsonl"));
    let cwd = payload
        .and_then(|payload| payload.get("cwd"))
        .and_then(Value::as_str);
    if candidate_session_id != session_id || determine_match_scope(cwd, context).is_none() {
        transcript.is_other_session = true;
        return;
    }
    transcript.matched_path = cwd.map(str::to_string);
}

/// 역할 거르기와 본문·첨부 이름 검색
fn is_selected_message(
    context: &AiSessionReaderContext,
    role: &crate::types::AiMessageRole,
    text: &str,
    attachments: &[crate::types::AiMessageAttachment],
) -> bool {
    if let Some(roles) = context.roles.as_ref().filter(|roles| !roles.is_empty())
        && !roles.contains(role)
    {
        return false;
    }
    let Some(query) = context.query.as_deref().filter(|query| !query.is_empty()) else {
        return true;
    };
    let searchable = std::iter::once(text)
        .chain(
            attachments
                .iter()
                .map(|attachment| attachment.name.as_str()),
        )
        .collect::<Vec<_>>()
        .join("\n");
    searchable.to_lowercase().contains(&query.to_lowercase())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;

    thread_local! {
        static BUILT_MESSAGES: Cell<usize> = const { Cell::new(0) };
    }

    pub(crate) fn record_built_message() {
        BUILT_MESSAGES.with(|count| count.set(count.get() + 1));
    }

    /// 이 스레드에서 지금까지 새로 만든 대화 메시지 수
    pub(crate) fn built_message_count() -> usize {
        BUILT_MESSAGES.with(Cell::get)
    }
}
