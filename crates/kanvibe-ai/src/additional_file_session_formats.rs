//! 파일 기반 provider 15종의 기록 형식을 한 모양(세션 ID, 작업 경로, 제목, 시각, 메시지)으로 푼다.

use std::cmp::Ordering;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::javascript::{js_string, js_trim, parse_date_ms, parse_json, stringify};
use crate::message_text::{extract_plain_text, make_preview_message, to_iso_string};
use crate::node_path;
use crate::providers::AiSessionProvider;
use crate::types::{AggregatedAiMessage, AiMessageRole};

pub type RecordValue = Map<String, Value>;

const MUSE_REPEATED_USER_WINDOW_MS: f64 = 60_000.0;

static EMPTY_RECORD: LazyLock<RecordValue> = LazyLock::new(RecordValue::new);

#[derive(Debug, Clone, PartialEq)]
pub struct AdditionalFileSession {
    pub id: String,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub messages: Vec<AggregatedAiMessage>,
}

/// 배열이 아닌 객체면 그대로, 아니면 빈 객체
pub fn session_record(value: &Value) -> &RecordValue {
    match value {
        Value::Object(record) => record,
        _ => &EMPTY_RECORD,
    }
}

/// 공백이 아닌 문자열이면 다듬은 값
pub fn session_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(js_trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// 객체 필드. 없으면 `null`이다
pub fn field<'a>(record: &'a RecordValue, key: &str) -> &'a Value {
    record.get(key).unwrap_or(&Value::Null)
}

/// TS `a ?? b`
fn coalesce<'a>(first: &'a Value, second: &'a Value) -> &'a Value {
    if first.is_null() { second } else { first }
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

fn first_string(record: &RecordValue, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| session_string(field(record, key)))
}

fn is_str(value: &Value, expected: &str) -> bool {
    value.as_str() == Some(expected)
}

fn decode_kiro_blocks(value: &Value) -> Vec<Value> {
    array(value)
        .iter()
        .flat_map(|raw| {
            let block = session_record(raw);
            let data = field(block, "data");
            match block.get("kind").and_then(Value::as_str) {
                Some("text") => vec![text_block(data.clone())],
                Some("json") => vec![text_block(json_text(block.get("data")))],
                Some("toolResult") => decode_kiro_blocks(field(session_record(data), "content")),
                Some("toolUse") => vec![text_block(Value::String(kiro_tool_use_text(
                    session_record(data),
                )))],
                _ => Vec::new(),
            }
        })
        .collect()
}

fn text_block(text: Value) -> Value {
    if text.is_null() {
        return json!({});
    }
    json!({ "text": text })
}

/// TS `JSON.stringify(value)`. 없는 값은 `undefined`라 글이 되지 않는다
fn json_text(value: Option<&Value>) -> Value {
    value.map_or(Value::Null, |value| Value::String(stringify(value)))
}

fn kiro_tool_use_text(data: &RecordValue) -> String {
    let name = coalesce(field(data, "name"), &Value::Null);
    let name = if name.is_null() {
        "tool".to_string()
    } else {
        js_string(name)
    };
    let empty = Value::Object(Map::new());
    format!(
        "{name}: {}",
        stringify(coalesce(field(data, "input"), &empty))
    )
}

static USER_REQUEST_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)<USER_REQUEST>(.*?)(?:</USER_REQUEST>|$)").expect("사용자 요청 패턴")
});

fn strip_user_envelope(value: &Value) -> Value {
    let Some(text) = value.as_str() else {
        return value.clone();
    };
    match USER_REQUEST_PATTERN.captures(text) {
        Some(found) => Value::String(found[1].to_string()),
        None => value.clone(),
    }
}

/// JS 문자열 비교(UTF-16 코드 단위)
fn is_less_like_js(left: &str, right: &str) -> bool {
    left.encode_utf16().cmp(right.encode_utf16()) == Ordering::Less
}

/// 기록을 읽어 가며 세션을 채운다
struct SessionBuilder {
    session: AdditionalFileSession,
}

impl SessionBuilder {
    fn touch(&mut self, value: &Value) {
        let Some(time) = to_iso_string(value).filter(|time| !time.is_empty()) else {
            return;
        };
        let session = &mut self.session;
        if session
            .started_at
            .as_deref()
            .is_none_or(|started| is_less_like_js(&time, started))
        {
            session.started_at = Some(time.clone());
        }
        if session
            .updated_at
            .as_deref()
            .is_none_or(|updated| is_less_like_js(updated, &time))
        {
            session.updated_at = Some(time);
        }
    }

    fn add(&mut self, role: AiMessageRole, content: &Value, time: &Value) {
        let text = extract_plain_text(content);
        let message = make_preview_message(role, time, &text, Vec::new());
        self.touch(time);
        if let Some(message) = message {
            self.session.messages.push(message);
        }
    }

    fn add_message(&mut self, raw_message: &Value) {
        let message = session_record(raw_message);
        if let Some(role) = basic_role(field(message, "role")) {
            let time = coalesce(field(message, "timestamp"), field(message, "ts"));
            self.add(role, field(message, "content"), time);
        }
    }

    fn adopt_id(&mut self, value: Option<String>) {
        if let Some(id) = value {
            self.session.id = id;
        }
    }

    fn adopt_cwd_if_missing(&mut self, value: Option<String>) {
        if self.session.cwd.is_none() {
            self.session.cwd = value;
        }
    }
}

fn basic_role(value: &Value) -> Option<AiMessageRole> {
    match value.as_str()? {
        "user" => Some(AiMessageRole::User),
        "assistant" => Some(AiMessageRole::Assistant),
        "tool" => Some(AiMessageRole::Tool),
        "system" => Some(AiMessageRole::System),
        _ => None,
    }
}

fn user_or_assistant(value: &Value) -> Option<AiMessageRole> {
    match value.as_str()? {
        "user" => Some(AiMessageRole::User),
        "assistant" => Some(AiMessageRole::Assistant),
        _ => None,
    }
}

/// 기록 하나(JSON 문서 또는 JSONL 줄 배열)와 provider별 곁 파일을 세션으로 푼다
pub fn decode_additional_file_session(
    provider: AiSessionProvider,
    file_path: &str,
    raw: &Value,
    sidecar: &RecordValue,
) -> AdditionalFileSession {
    let records: Vec<&RecordValue> = match raw {
        Value::Array(items) => items.iter().map(session_record).collect(),
        other => vec![session_record(other)],
    };
    let mut builder = SessionBuilder {
        session: AdditionalFileSession {
            id: node_path::basename_without(file_path, &node_path::extname(file_path)),
            cwd: None,
            title: None,
            started_at: None,
            updated_at: None,
            messages: Vec::new(),
        },
    };
    let document = session_record(raw);
    decode_provider(
        provider,
        &mut builder,
        file_path,
        &records,
        document,
        sidecar,
    );
    builder.session
}

fn decode_provider(
    provider: AiSessionProvider,
    builder: &mut SessionBuilder,
    file_path: &str,
    records: &[&RecordValue],
    document: &RecordValue,
    sidecar: &RecordValue,
) {
    use AiSessionProvider as Provider;
    match provider {
        Provider::Qoder => decode_qoder(builder, records),
        Provider::Codebuddy => decode_codebuddy(builder, records),
        Provider::Pi | Provider::Omp | Provider::PrimeAgent | Provider::Openclaw => {
            decode_pi(builder, records)
        }
        Provider::Copilot => decode_copilot(builder, records),
        Provider::Cursor => decode_cursor(builder, file_path, records, sidecar),
        Provider::Droid => decode_droid(builder, records),
        Provider::Hermes | Provider::Jcode => decode_hermes(builder, provider, document),
        Provider::Rovo => decode_rovo(builder, file_path, document, sidecar),
        Provider::Grok => decode_grok(builder, file_path, document, sidecar),
        Provider::Devin => decode_devin(builder, document, sidecar),
        Provider::Cline => decode_cline(builder, file_path, document, sidecar),
        Provider::Kiro => decode_kiro(builder, document, sidecar),
        Provider::Kimi => decode_kimi(builder, file_path, document, sidecar),
        Provider::Antigravity => decode_antigravity(builder, file_path, records, sidecar),
        Provider::Muse => decode_muse(builder, file_path, records),
        _ => {}
    }
}

fn decode_qoder(builder: &mut SessionBuilder, records: &[&RecordValue]) {
    for record in records {
        builder.adopt_id(session_string(field(record, "sessionId")));
        builder.adopt_cwd_if_missing(session_string(field(record, "cwd")));
        let record_type = field(record, "type");
        if is_str(record_type, "workspace-directories") {
            let first = array(field(record, "directories"))
                .first()
                .unwrap_or(&Value::Null);
            builder.adopt_cwd_if_missing(session_string(first));
        }
        if is_str(record_type, "summary") {
            builder.session.title =
                session_string(field(record, "summary")).or(builder.session.title.take());
        }
        if let Some(role) = user_or_assistant(record_type) {
            let content = field(session_record(field(record, "message")), "content");
            builder.add(role, content, field(record, "timestamp"));
        }
    }
}

const CODEBUDDY_TEXT_TYPES: [&str; 3] = ["text", "input_text", "output_text"];

fn decode_codebuddy(builder: &mut SessionBuilder, records: &[&RecordValue]) {
    for record in records {
        builder.adopt_id(session_string(field(record, "sessionId")));
        builder.adopt_cwd_if_missing(session_string(field(record, "cwd")));
        let record_type = field(record, "type");
        if is_str(record_type, "summary") {
            builder.session.title =
                session_string(field(record, "summary")).or(builder.session.title.take());
        }
        if is_str(record_type, "ai-title") {
            builder.session.title =
                session_string(field(record, "aiTitle")).or(builder.session.title.take());
        }
        let Some(role) =
            user_or_assistant(field(record, "role")).filter(|_| is_str(record_type, "message"))
        else {
            continue;
        };
        // skipRun은 실제로 보내지 않은 주입 문맥이므로 대화에서 뺀다
        let is_skipped =
            field(session_record(field(record, "providerData")), "skipRun") == &Value::Bool(true);
        if role == AiMessageRole::User && is_skipped {
            continue;
        }
        let content = match field(record, "content") {
            Value::Array(parts) => Value::Array(
                parts
                    .iter()
                    .filter(|part| {
                        let part_type = session_record(part)
                            .get("type")
                            .map_or_else(|| "undefined".to_string(), js_string);
                        CODEBUDDY_TEXT_TYPES.contains(&part_type.as_str())
                    })
                    .cloned()
                    .collect(),
            ),
            other => other.clone(),
        };
        builder.add(role, &content, field(record, "timestamp"));
    }
}

fn decode_pi(builder: &mut SessionBuilder, records: &[&RecordValue]) {
    for record in records {
        let record_type = field(record, "type");
        if is_str(record_type, "session") {
            builder.adopt_id(session_string(field(record, "id")));
            builder.adopt_cwd_if_missing(session_string(field(record, "cwd")));
            builder.touch(field(record, "timestamp"));
        }
        if is_str(record_type, "title") {
            builder.session.title =
                session_string(field(record, "title")).or(builder.session.title.take());
        }
        if !is_str(record_type, "message") {
            continue;
        }
        let message = session_record(field(record, "message"));
        let role = match field(message, "role").as_str() {
            Some("toolResult") => Some(AiMessageRole::Tool),
            _ => user_or_assistant(field(message, "role")),
        };
        if let Some(role) = role {
            builder.add(role, field(message, "content"), field(record, "timestamp"));
        }
    }
}

static TRUSTED_FOLDER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Folder ([^\n\r\x{2028}\x{2029}]+) has been added to trusted folders\.$")
        .expect("신뢰 폴더 패턴")
});

fn decode_copilot(builder: &mut SessionBuilder, records: &[&RecordValue]) {
    for record in records {
        let data = session_record(field(record, "data"));
        let record_type = field(record, "type");
        if is_str(record_type, "session.start") {
            builder.adopt_id(session_string(field(data, "sessionId")));
            let context = session_record(field(data, "context"));
            let cwd = session_string(field(context, "cwd"))
                .or_else(|| session_string(field(context, "gitRoot")));
            builder.adopt_cwd_if_missing(cwd);
            builder.touch(field(data, "startTime"));
        }
        if is_str(record_type, "session.info") && builder.session.cwd.is_none() {
            let message = session_string(field(data, "message"));
            builder.session.cwd = message
                .as_deref()
                .and_then(|text| TRUSTED_FOLDER_PATTERN.captures(text))
                .map(|found| js_trim(&found[1]).to_string());
        }
        if is_str(record_type, "user.message") {
            let content = coalesce(field(data, "content"), field(data, "transformedContent"));
            builder.add(AiMessageRole::User, content, field(record, "timestamp"));
        }
        if is_str(record_type, "assistant.message") {
            builder.add(
                AiMessageRole::Assistant,
                field(data, "content"),
                field(record, "timestamp"),
            );
        }
    }
}

fn decode_cursor(
    builder: &mut SessionBuilder,
    file_path: &str,
    records: &[&RecordValue],
    sidecar: &RecordValue,
) {
    let parent_name = node_path::basename(&node_path::dirname(file_path));
    if parent_name != "agent-transcripts" {
        builder.session.id = parent_name;
    }
    builder.session.cwd = session_string(field(sidecar, "cwd"));
    builder.session.title = session_string(field(sidecar, "title"));
    builder.touch(field(sidecar, "createdAtMs"));
    builder.touch(field(sidecar, "updatedAtMs"));
    for record in records {
        if let Some(role) = user_or_assistant(field(record, "role")) {
            let content = coalesce(
                field(session_record(field(record, "message")), "content"),
                field(record, "content"),
            );
            builder.add(role, content, field(record, "timestamp"));
        }
    }
}

fn decode_droid(builder: &mut SessionBuilder, records: &[&RecordValue]) {
    for record in records {
        let record_type = field(record, "type");
        if ["session_start", "session", "session_meta"]
            .iter()
            .any(|kind| is_str(record_type, kind))
        {
            builder.adopt_id(session_string(field(record, "id")));
            builder.adopt_cwd_if_missing(session_string(field(record, "cwd")));
        }
        let session_id = session_string(field(record, "session_id"))
            .or_else(|| session_string(field(record, "sessionId")));
        builder.adopt_id(session_id);
        builder.adopt_cwd_if_missing(session_string(field(record, "cwd")));
        let message = session_record(field(record, "message"));
        let role = coalesce(field(record, "role"), field(message, "role"));
        if let Some(role) = user_or_assistant(role) {
            let content = coalesce(field(message, "content"), field(record, "content"));
            builder.add(role, content, field(record, "timestamp"));
        }
        if is_str(record_type, "completion") {
            builder.add(
                AiMessageRole::Assistant,
                field(record, "finalText"),
                field(record, "timestamp"),
            );
        }
    }
}

static JCODE_HIDDEN_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\[Scheduled task\]|<system-reminder>)").expect("jcode 숨김 패턴")
});

fn is_hidden_jcode_message(value: &RecordValue) -> bool {
    let display_role = value
        .get("display_role")
        .map_or_else(|| "undefined".to_string(), js_string);
    ["system", "background_task"].contains(&display_role.as_str())
        || is_str(field(value, "role"), "system")
        || JCODE_HIDDEN_TEXT.is_match(&extract_plain_text(field(value, "content")))
}

fn decode_hermes(
    builder: &mut SessionBuilder,
    provider: AiSessionProvider,
    document: &RecordValue,
) {
    let is_hermes = provider == AiSessionProvider::Hermes;
    let id_keys: &[&str] = if is_hermes {
        &["session_id"]
    } else {
        &["id", "session_id"]
    };
    let cwd_keys: &[&str] = if is_hermes {
        &["cwd"]
    } else {
        &["working_dir", "working_directory"]
    };
    builder.adopt_id(first_string(document, id_keys));
    builder.session.cwd = first_string(document, cwd_keys);
    builder.session.title = first_string(document, &["custom_title", "title"]);
    builder.touch(coalesce(
        field(document, "session_start"),
        field(document, "created_at"),
    ));
    builder.touch(coalesce(
        field(document, "last_updated"),
        field(document, "updated_at"),
    ));
    for message in array(field(document, "messages")) {
        let value = session_record(message);
        if !is_hermes && is_hidden_jcode_message(value) {
            continue;
        }
        builder.add_message(message);
    }
}

const ROVO_CWD_KEYS: [&str; 8] = [
    "workspace_path",
    "workspacePath",
    "workspace",
    "cwd",
    "working_directory",
    "workingDirectory",
    "project_path",
    "projectPath",
];

fn decode_rovo(
    builder: &mut SessionBuilder,
    file_path: &str,
    document: &RecordValue,
    sidecar: &RecordValue,
) {
    builder.session.id = node_path::basename(&node_path::dirname(file_path));
    builder.session.cwd = first_string(document, &ROVO_CWD_KEYS);
    builder.session.title = first_string(document, &["title", "name", "summary"]);
    builder.touch(coalesce(
        field(document, "created_at"),
        field(document, "createdAt"),
    ));
    builder.touch(coalesce(
        field(document, "updated_at"),
        field(document, "updatedAt"),
    ));
    for message in array(field(sidecar, "messages")) {
        builder.add_message(message);
    }
    for raw_entry in array(field(sidecar, "message_history")) {
        let entry = session_record(raw_entry);
        let role = match field(entry, "role") {
            Value::Null => match field(entry, "kind").as_str() {
                Some("request") => Some(AiMessageRole::User),
                Some("response") => Some(AiMessageRole::Assistant),
                _ => None,
            },
            other => user_or_assistant(other),
        };
        let Some(role) = role else {
            continue;
        };
        let parts: Vec<Value> = array(field(entry, "parts"))
            .iter()
            .filter(|part| {
                let kind = field(session_record(part), "part_kind");
                is_str(kind, "text") || (role == AiMessageRole::User && is_str(kind, "user-prompt"))
            })
            .map(|part| {
                coalesce(
                    field(session_record(part), "content"),
                    field(session_record(part), "text"),
                )
                .clone()
            })
            .collect();
        builder.add(role, &Value::Array(parts), field(entry, "timestamp"));
    }
}

fn decode_grok(
    builder: &mut SessionBuilder,
    file_path: &str,
    document: &RecordValue,
    sidecar: &RecordValue,
) {
    let info = session_record(field(document, "info"));
    builder.session.id = session_string(field(info, "id"))
        .unwrap_or_else(|| node_path::basename(&node_path::dirname(file_path)));
    builder.session.cwd = session_string(field(info, "cwd"));
    builder.session.title = first_string(document, &["generated_title", "session_summary"]);
    builder.touch(field(document, "created_at"));
    builder.touch(coalesce(
        field(document, "updated_at"),
        field(document, "last_active_at"),
    ));
    for value in array(field(sidecar, "lines")) {
        let record = session_record(value);
        if let Some(role) = user_or_assistant(field(record, "type")) {
            builder.add(role, field(record, "content"), field(record, "timestamp"));
        }
    }
}

fn decode_devin(builder: &mut SessionBuilder, document: &RecordValue, sidecar: &RecordValue) {
    builder.adopt_id(first_string(document, &["session_id", "sessionId"]));
    builder.session.cwd = session_string(field(document, "working_directory"))
        .or_else(|| session_string(field(sidecar, "working_directory")));
    builder.session.title = session_string(field(sidecar, "title"));
    builder.touch(field(sidecar, "created_at"));
    builder.touch(field(sidecar, "last_activity_at"));
    for raw_step in array(field(document, "steps")) {
        let step = session_record(raw_step);
        let metadata = session_record(field(step, "metadata"));
        let source = field(step, "source");
        if is_str(source, "system") {
            continue;
        }
        let is_user =
            is_str(source, "user") || field(metadata, "is_user_input") == &Value::Bool(true);
        let is_agent = is_str(source, "agent")
            || is_str(field(step, "role"), "assistant")
            || crate::javascript::is_truthy(field(step, "tool_calls"));
        if !is_user && !is_agent {
            continue;
        }
        let message_content = field(session_record(field(step, "message")), "content");
        let content = [
            message_content,
            field(step, "message"),
            field(step, "text"),
            field(step, "content"),
        ]
        .into_iter()
        .find(|value| !value.is_null())
        .unwrap_or(&Value::Null);
        let role = if is_user {
            AiMessageRole::User
        } else {
            AiMessageRole::Assistant
        };
        builder.add(
            role,
            content,
            coalesce(field(step, "timestamp"), field(metadata, "created_at")),
        );
    }
}

fn decode_cline(
    builder: &mut SessionBuilder,
    file_path: &str,
    document: &RecordValue,
    sidecar: &RecordValue,
) {
    builder.session.id = session_string(field(document, "session_id"))
        .unwrap_or_else(|| node_path::basename(&node_path::dirname(file_path)));
    builder.session.cwd = first_string(document, &["cwd", "workspace_root"]);
    builder.session.title = session_string(field(document, "prompt"));
    builder.touch(field(document, "started_at"));
    builder.touch(field(sidecar, "updated_at"));
    for message in array(field(sidecar, "messages")) {
        builder.add_message(message);
    }
}

fn decode_kiro(builder: &mut SessionBuilder, document: &RecordValue, sidecar: &RecordValue) {
    builder.session.cwd = session_string(field(document, "cwd"));
    builder.session.title = session_string(field(document, "title"));
    builder.touch(field(document, "created_at"));
    builder.touch(field(document, "updated_at"));
    for value in array(field(sidecar, "lines")) {
        let record = session_record(value);
        let data = session_record(field(record, "data"));
        let role = match field(record, "kind").as_str() {
            Some("Prompt") => AiMessageRole::User,
            Some("AssistantMessage") => AiMessageRole::Assistant,
            Some("ToolResults") => AiMessageRole::Tool,
            _ => continue,
        };
        let blocks = Value::Array(decode_kiro_blocks(field(data, "content")));
        builder.add(
            role,
            &blocks,
            field(session_record(field(data, "meta")), "timestamp"),
        );
    }
}

fn decode_kimi(
    builder: &mut SessionBuilder,
    file_path: &str,
    document: &RecordValue,
    sidecar: &RecordValue,
) {
    builder.session.id = node_path::basename(&node_path::dirname(file_path));
    builder.session.cwd = session_string(field(sidecar, "workDir"));
    builder.session.title = first_string(document, &["title", "lastPrompt"]);
    builder.touch(field(document, "createdAt"));
    builder.touch(field(document, "updatedAt"));
    let mut pending = String::new();
    for value in array(field(sidecar, "lines")) {
        let record = session_record(value);
        let record_type = field(record, "type");
        if is_str(record_type, "context.append_message") {
            let message = session_record(field(record, "message"));
            let is_user_origin = is_str(
                field(session_record(field(message, "origin")), "kind"),
                "user",
            );
            if is_str(field(message, "role"), "user") && is_user_origin {
                flush_kimi(builder, &mut pending);
                builder.add(AiMessageRole::User, field(message, "content"), &Value::Null);
            }
        }
        if is_str(record_type, "context.append_loop_event") {
            let event = session_record(field(record, "event"));
            let part = session_record(field(event, "part"));
            let event_type = field(event, "type");
            if let (true, true, Some(text)) = (
                is_str(event_type, "content.part"),
                is_str(field(part, "type"), "text"),
                field(part, "text").as_str(),
            ) {
                pending.push_str(text);
            }
            if is_str(event_type, "step.end") {
                flush_kimi(builder, &mut pending);
            }
        }
    }
    flush_kimi(builder, &mut pending);
}

fn flush_kimi(builder: &mut SessionBuilder, pending: &mut String) {
    if !pending.is_empty() {
        builder.add(
            AiMessageRole::Assistant,
            &Value::String(std::mem::take(pending)),
            &Value::Null,
        );
    }
}

fn decode_antigravity(
    builder: &mut SessionBuilder,
    file_path: &str,
    records: &[&RecordValue],
    sidecar: &RecordValue,
) {
    let conversation_directory =
        node_path::dirname(&node_path::dirname(&node_path::dirname(file_path)));
    builder.session.id = node_path::basename(&conversation_directory);
    builder.session.cwd = session_string(field(sidecar, "workspace"));
    for record in records {
        let source = field(record, "source");
        let record_type = field(record, "type");
        let is_user_source = is_str(source, "USER_EXPLICIT") || is_str(source, "USER");
        if is_user_source && (is_str(record_type, "USER_INPUT") || is_str(record_type, "REQUEST")) {
            let content = strip_user_envelope(field(record, "content"));
            builder.add(AiMessageRole::User, &content, field(record, "created_at"));
        }
        if is_str(source, "MODEL") && is_str(record_type, "PLANNER_RESPONSE") {
            builder.add(
                AiMessageRole::Assistant,
                field(record, "content"),
                field(record, "created_at"),
            );
        }
    }
}

/// muse 봉투 하나를 기록 목록으로 편다. `children`의 `record_json`은 문자열 JSON일 수도 있다
fn muse_entries(envelope: &RecordValue) -> Vec<RecordValue> {
    let Some(children) = envelope.get("children").and_then(Value::as_array) else {
        return vec![envelope.clone()];
    };
    children
        .iter()
        .filter_map(|child| {
            let value = field(session_record(child), "record_json");
            let parsed = match value {
                Value::String(text) => parse_json(text)?,
                other => other.clone(),
            };
            Some(session_record(&parsed).clone())
        })
        .collect()
}

/// 마이크로초로 기록된 시각은 밀리초로 낮춘다
fn muse_recorded_at(record: &RecordValue) -> Value {
    let recorded_at = field(record, "recorded_at");
    match recorded_at.as_f64() {
        Some(number) if number >= 1e14 => json!((number / 1000.0).floor()),
        _ => recorded_at.clone(),
    }
}

/// muse 사용자 요청과 그 시각. runtime.started가 같은 요청을 반복하면 거른다
#[derive(Default)]
struct MuseLastUser {
    text: Option<String>,
    time: Option<String>,
}

fn decode_muse(builder: &mut SessionBuilder, file_path: &str, records: &[&RecordValue]) {
    builder.session.id = node_path::basename(&node_path::dirname(file_path));
    let mut last_user = MuseLastUser::default();
    for envelope in records {
        for record in muse_entries(envelope) {
            decode_muse_record(builder, &record, &mut last_user);
        }
    }
}

fn decode_muse_record(
    builder: &mut SessionBuilder,
    record: &RecordValue,
    last_user: &mut MuseLastUser,
) {
    let payload = session_record(field(record, "payload"));
    let value = session_record(field(payload, "record"));
    let recorded_at = muse_recorded_at(record);
    match field(record, "payload_type").as_str() {
        Some("runtime.session.metadata") => {
            builder.adopt_cwd_if_missing(session_string(field(value, "workspace_root")))
        }
        Some("runtime.session.route_facts") => {
            builder.adopt_cwd_if_missing(session_string(field(value, "cwd")))
        }
        Some("runtime.user_intent.accepted") => {
            let text = muse_intent_text(payload);
            if !text.is_empty() {
                builder.add(
                    AiMessageRole::User,
                    &Value::String(text.clone()),
                    &recorded_at,
                );
                last_user.text = Some(text);
                last_user.time = to_iso_string(&recorded_at);
            }
        }
        Some("runtime.session") => decode_muse_event(
            builder,
            session_record(field(payload, "event")),
            &recorded_at,
            last_user,
        ),
        _ => {}
    }
}

fn muse_intent_text(payload: &RecordValue) -> String {
    let refill = array(field(payload, "refill_blocks"));
    let model_content: Vec<Value> = array(field(payload, "model_messages"))
        .iter()
        .flat_map(|message| array(field(session_record(message), "content")).to_vec())
        .collect();
    let blocks = if refill.is_empty() {
        model_content.as_slice()
    } else {
        refill
    };
    let found = blocks
        .iter()
        .find(|block| session_string(field(session_record(block), "text")).is_some());
    extract_plain_text(found.unwrap_or(&Value::Null))
}

fn decode_muse_event(
    builder: &mut SessionBuilder,
    event: &RecordValue,
    recorded_at: &Value,
    last_user: &MuseLastUser,
) {
    let kind = field(event, "kind");
    if let (true, Some(prompt)) = (is_str(kind, "started"), field(event, "prompt").as_str()) {
        let time = to_iso_string(recorded_at);
        // runtime.started는 받아들인 요청을 반복하므로 같은 분 안의 반복만 뺀다
        let is_repeated = last_user.text.as_deref() == Some(prompt)
            && match (time.as_deref(), last_user.time.as_deref()) {
                (Some(time), Some(last_time)) if !time.is_empty() && !last_time.is_empty() => {
                    let distance = (parse_date_ms(time).unwrap_or(f64::NAN)
                        - parse_date_ms(last_time).unwrap_or(f64::NAN))
                    .abs();
                    distance < MUSE_REPEATED_USER_WINDOW_MS
                }
                _ => false,
            };
        if !is_repeated {
            builder.add(
                AiMessageRole::User,
                &Value::String(prompt.to_string()),
                recorded_at,
            );
        }
    }
    if is_str(kind, "assistant_message_committed") {
        builder.add(AiMessageRole::Assistant, field(event, "text"), recorded_at);
    }
}
