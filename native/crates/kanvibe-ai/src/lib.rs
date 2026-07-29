use std::{
    error::Error,
    fmt::{Display, Formatter},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const CRATE_PURPOSE: &str =
    "Claude, Codex, Gemini, and OpenCode session discovery and detail contracts";
pub const DEFAULT_SESSION_LIMIT: usize = 20;
pub const DEFAULT_DETAIL_LIMIT: usize = 20;
const MAX_PAGE_LIMIT: usize = 200;
const MAX_PROVIDER_FILES: usize = 5_000;
const MAX_SESSION_FILE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiSessionProvider {
    Claude,
    Codex,
    Gemini,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl AiSessionProvider {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::OpenCode, Self::Gemini];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMessageRole {
    User,
    Assistant,
    Tool,
    System,
    Developer,
    Reasoning,
    Unknown,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: AiMessageRole,
    pub timestamp: Option<String>,
    pub text: String,
    pub full_text: String,
    pub is_truncated: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AiSession {
    pub id: String,
    pub provider: AiSessionProvider,
    pub started_at: Option<String>,
    pub updated_at: Option<String>,
    pub matched_path: String,
    pub title: Option<String>,
    pub first_user_prompt: Option<String>,
    pub message_count: usize,
    pub source_ref: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AiSourceStatus {
    pub provider: AiSessionProvider,
    pub available: bool,
    pub session_count: usize,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AiSessionAggregation {
    pub sessions: Vec<AiSession>,
    pub sources: Vec<AiSourceStatus>,
}

pub fn aggregate_ai_sessions(mut sessions: Vec<AiSession>) -> AiSessionAggregation {
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    let sources = AiSessionProvider::ALL
        .into_iter()
        .map(|provider| AiSourceStatus {
            provider,
            available: true,
            session_count: sessions
                .iter()
                .filter(|session| session.provider == provider)
                .count(),
            reason: None,
        })
        .collect();
    AiSessionAggregation { sessions, sources }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AiSessionsPage {
    pub is_remote: bool,
    pub target_path: PathBuf,
    pub repo_path: PathBuf,
    pub sessions: Vec<AiSession>,
    pub sources: Vec<AiSourceStatus>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AiSessionDetail {
    pub session_id: String,
    pub provider: AiSessionProvider,
    pub title: Option<String>,
    pub matched_path: String,
    pub source_ref: String,
    pub messages: Vec<AiMessage>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AiSessionQuery {
    pub target_path: PathBuf,
    pub repo_path: PathBuf,
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub limit: usize,
}

impl AiSessionQuery {
    pub fn new(target_path: impl Into<PathBuf>, repo_path: impl Into<PathBuf>) -> Self {
        Self {
            target_path: target_path.into(),
            repo_path: repo_path.into(),
            query: None,
            cursor: None,
            limit: DEFAULT_SESSION_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AiDetailQuery {
    pub sessions: AiSessionQuery,
    pub provider: AiSessionProvider,
    pub session_id: String,
    pub source_ref: Option<String>,
    pub roles: Vec<AiMessageRole>,
    pub cursor: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AiSessionError {
    message: String,
}

impl AiSessionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for AiSessionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AiSessionError {}

pub trait AiSessionDataSource {
    fn is_remote(&self) -> bool;
    fn home_directory(&self) -> Result<PathBuf, AiSessionError>;
    fn path_exists(&self, path: &Path) -> Result<bool, AiSessionError>;
    fn list_files(
        &self,
        root: &Path,
        suffix: &str,
        recursive: bool,
    ) -> Result<Vec<PathBuf>, AiSessionError>;
    fn read_text(&self, path: &Path) -> Result<String, AiSessionError>;
    fn open_code_sessions(
        &self,
        database_path: &Path,
    ) -> Result<Option<Vec<OpenCodeSessionRow>>, AiSessionError>;
    fn open_code_messages(
        &self,
        database_path: &Path,
        session_id: &str,
    ) -> Result<Option<Vec<OpenCodeMessageRow>>, AiSessionError>;
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenCodeSessionRow {
    pub id: String,
    pub directory: String,
    pub title: Option<String>,
    pub time_created: Option<i64>,
    pub time_updated: Option<i64>,
    pub part_count: usize,
    pub first_user_part: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenCodeMessageRow {
    pub session_id: String,
    pub directory: String,
    pub title: Option<String>,
    pub part_data: String,
    pub time_created: Option<i64>,
    pub message_data: String,
}

#[derive(Debug, Clone, Default)]
pub struct LocalAiSessionDataSource;

impl AiSessionDataSource for LocalAiSessionDataSource {
    fn is_remote(&self) -> bool {
        false
    }

    fn home_directory(&self) -> Result<PathBuf, AiSessionError> {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| AiSessionError::new("local HOME directory is unavailable"))
    }

    fn path_exists(&self, path: &Path) -> Result<bool, AiSessionError> {
        Ok(path.exists())
    }

    fn list_files(
        &self,
        root: &Path,
        suffix: &str,
        recursive: bool,
    ) -> Result<Vec<PathBuf>, AiSessionError> {
        list_local_files(root, suffix, recursive)
    }

    fn read_text(&self, path: &Path) -> Result<String, AiSessionError> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| AiSessionError::new(format!("{}: {error}", path.display())))?;
        if metadata.len() > MAX_SESSION_FILE_BYTES {
            return Err(AiSessionError::new(format!(
                "{} exceeds the {} byte AI session limit",
                path.display(),
                MAX_SESSION_FILE_BYTES
            )));
        }
        std::fs::read_to_string(path)
            .map_err(|error| AiSessionError::new(format!("{}: {error}", path.display())))
    }

    fn open_code_sessions(
        &self,
        database_path: &Path,
    ) -> Result<Option<Vec<OpenCodeSessionRow>>, AiSessionError> {
        if !database_path.exists() {
            return Ok(None);
        }
        let connection = rusqlite::Connection::open_with_flags(
            database_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|error| AiSessionError::new(error.to_string()))?;
        let mut statement = connection
            .prepare(
                "SELECT s.id, s.directory, s.title, s.time_created, s.time_updated,
                        (SELECT COUNT(*) FROM part p WHERE p.session_id = s.id),
                        (SELECT p.data FROM part p JOIN message m ON m.id = p.message_id
                         WHERE p.session_id = s.id
                           AND json_extract(m.data, '$.role') = 'user'
                           AND json_extract(p.data, '$.type') = 'text'
                         ORDER BY p.time_created ASC LIMIT 1)
                 FROM session s ORDER BY s.time_updated DESC LIMIT 120",
            )
            .map_err(|error| AiSessionError::new(error.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                Ok(OpenCodeSessionRow {
                    id: row.get(0)?,
                    directory: row.get(1)?,
                    title: row.get(2)?,
                    time_created: row.get(3)?,
                    time_updated: row.get(4)?,
                    part_count: row.get::<_, i64>(5)?.max(0) as usize,
                    first_user_part: row.get(6)?,
                })
            })
            .map_err(|error| AiSessionError::new(error.to_string()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AiSessionError::new(error.to_string()))?;
        Ok(Some(rows))
    }

    fn open_code_messages(
        &self,
        database_path: &Path,
        session_id: &str,
    ) -> Result<Option<Vec<OpenCodeMessageRow>>, AiSessionError> {
        if !database_path.exists() {
            return Ok(None);
        }
        let connection = rusqlite::Connection::open_with_flags(
            database_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|error| AiSessionError::new(error.to_string()))?;
        let mut statement = connection
            .prepare(
                "SELECT s.id, s.directory, s.title, p.data, p.time_created, m.data
                 FROM session s JOIN part p ON p.session_id = s.id
                 JOIN message m ON m.id = p.message_id
                 WHERE s.id = ?1 ORDER BY p.time_created ASC",
            )
            .map_err(|error| AiSessionError::new(error.to_string()))?;
        let rows = statement
            .query_map([session_id], |row| {
                Ok(OpenCodeMessageRow {
                    session_id: row.get(0)?,
                    directory: row.get(1)?,
                    title: row.get(2)?,
                    part_data: row.get(3)?,
                    time_created: row.get(4)?,
                    message_data: row.get(5)?,
                })
            })
            .map_err(|error| AiSessionError::new(error.to_string()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| AiSessionError::new(error.to_string()))?;
        Ok(Some(rows))
    }
}

fn list_local_files(
    root: &Path,
    suffix: &str,
    recursive: bool,
) -> Result<Vec<PathBuf>, AiSessionError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| AiSessionError::new(format!("{}: {error}", directory.display())))?;
        for entry in entries {
            let entry = entry.map_err(|error| AiSessionError::new(error.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|error| AiSessionError::new(error.to_string()))?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() && recursive {
                pending.push(path);
            } else if file_type.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(suffix))
            {
                files.push(path);
                if files.len() > MAX_PROVIDER_FILES {
                    return Err(AiSessionError::new(format!(
                        "{} contains more than {MAX_PROVIDER_FILES} AI session files",
                        root.display()
                    )));
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

pub fn read_ai_sessions(
    source: &dyn AiSessionDataSource,
    query: &AiSessionQuery,
) -> Result<AiSessionsPage, AiSessionError> {
    let home = source.home_directory()?;
    let mut sessions = Vec::new();
    let mut sources = Vec::new();
    for provider in AiSessionProvider::ALL {
        match read_provider_sessions(source, query, &home, provider) {
            Ok(provider_sessions) => {
                sources.push(AiSourceStatus {
                    provider,
                    available: true,
                    session_count: provider_sessions.len(),
                    reason: provider_sessions
                        .is_empty()
                        .then(|| format!("No {} sessions matched this task", provider.as_str())),
                });
                sessions.extend(provider_sessions);
            }
            Err(error) => sources.push(AiSourceStatus {
                provider,
                available: false,
                session_count: 0,
                reason: Some(error.to_string()),
            }),
        }
    }
    sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    let (sessions, next_cursor) = paginate(sessions, query.cursor.as_deref(), query.limit);
    Ok(AiSessionsPage {
        is_remote: source.is_remote(),
        target_path: query.target_path.clone(),
        repo_path: query.repo_path.clone(),
        sessions,
        sources,
        next_cursor,
    })
}

pub fn read_ai_session_detail(
    source: &dyn AiSessionDataSource,
    query: &AiDetailQuery,
) -> Result<Option<AiSessionDetail>, AiSessionError> {
    let home = source.home_directory()?;
    let mut summary_query = query.sessions.clone();
    summary_query.query = None;
    summary_query.cursor = None;
    summary_query.limit = usize::MAX;
    let summary = read_provider_sessions(source, &summary_query, &home, query.provider)?
        .into_iter()
        .find(|session| {
            session.id == query.session_id
                && query
                    .source_ref
                    .as_deref()
                    .is_none_or(|source_ref| source_ref == session.source_ref)
        });
    let Some(summary) = summary else {
        return Ok(None);
    };
    let mut messages = match query.provider {
        AiSessionProvider::Claude => {
            let content = source.read_text(Path::new(&summary.source_ref))?;
            json_lines(&content)
                .into_iter()
                .filter(|value| {
                    value.get("sessionId").and_then(serde_json::Value::as_str)
                        == Some(query.session_id.as_str())
                })
                .filter_map(|value| {
                    make_message(
                        claude_role(&value),
                        timestamp_value(value.get("timestamp")),
                        extract_plain_text(value.pointer("/message/content")),
                    )
                })
                .collect()
        }
        AiSessionProvider::Codex => {
            let content = source.read_text(Path::new(&summary.source_ref))?;
            json_lines(&content)
                .into_iter()
                .flat_map(|value| {
                    let timestamp = timestamp_value(value.get("timestamp"));
                    codex_messages(&value)
                        .into_iter()
                        .filter(|(role, text)| !should_skip_codex_message(*role, text))
                        .filter_map(move |(role, text)| make_message(role, timestamp.clone(), text))
                })
                .collect()
        }
        AiSessionProvider::Gemini => {
            let content = source.read_text(Path::new(&summary.source_ref))?;
            serde_json::from_str::<serde_json::Value>(&content)
                .map(|value| gemini_messages(&value))
                .unwrap_or_default()
        }
        AiSessionProvider::OpenCode => {
            let database_path = home.join(".local/share/opencode/opencode.db");
            source
                .open_code_messages(&database_path, &query.session_id)?
                .unwrap_or_default()
                .into_iter()
                .filter_map(|row| {
                    let role = open_code_role(&row.message_data, &row.part_data);
                    make_message(
                        role,
                        row.time_created.map(epoch_millis_to_iso),
                        open_code_part_text(&row.part_data),
                    )
                })
                .collect()
        }
    };
    messages.retain(|message| {
        (query.roles.is_empty() || query.roles.contains(&message.role))
            && matches_query(
                query.sessions.query.as_deref(),
                [message.full_text.as_str()],
            )
    });
    messages.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    let (messages, next_cursor) = paginate(messages, query.cursor.as_deref(), query.limit.max(1));
    Ok(Some(AiSessionDetail {
        session_id: summary.id,
        provider: summary.provider,
        title: summary.title,
        matched_path: summary.matched_path,
        source_ref: summary.source_ref,
        messages,
        next_cursor,
    }))
}

fn read_provider_sessions(
    source: &dyn AiSessionDataSource,
    query: &AiSessionQuery,
    home: &Path,
    provider: AiSessionProvider,
) -> Result<Vec<AiSession>, AiSessionError> {
    match provider {
        AiSessionProvider::Claude => read_claude_sessions(source, query, home),
        AiSessionProvider::Codex => read_codex_sessions(source, query, home),
        AiSessionProvider::Gemini => read_gemini_sessions(source, query, home),
        AiSessionProvider::OpenCode => read_open_code_sessions(source, query, home),
    }
}

fn read_claude_sessions(
    source: &dyn AiSessionDataSource,
    query: &AiSessionQuery,
    home: &Path,
) -> Result<Vec<AiSession>, AiSessionError> {
    let root = home.join(".claude/projects");
    if !source.path_exists(&home.join(".claude"))? {
        return Err(AiSessionError::new("Claude Code directory not found"));
    }
    let files = source.list_files(&root, ".jsonl", true)?;
    let mut sessions = Vec::new();
    for file in files {
        let content = source.read_text(&file)?;
        let mut session: Option<AiSession> = None;
        let mut query_match = query.query.is_none();
        for value in json_lines(&content) {
            let Some(session_id) = value.get("sessionId").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            let Some(cwd) = value.get("cwd").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if !path_matches(cwd, &query.target_path) {
                continue;
            }
            let role = claude_role(&value);
            let text = extract_plain_text(value.pointer("/message/content"));
            let timestamp = timestamp_value(value.get("timestamp"));
            let current = session.get_or_insert_with(|| AiSession {
                id: session_id.to_owned(),
                provider: AiSessionProvider::Claude,
                started_at: timestamp.clone(),
                updated_at: timestamp.clone(),
                matched_path: cwd.to_owned(),
                title: None,
                first_user_prompt: None,
                message_count: 0,
                source_ref: file.to_string_lossy().into_owned(),
            });
            if current.id != session_id {
                continue;
            }
            if let Some(timestamp) = timestamp {
                current.updated_at = Some(timestamp);
            }
            if !text.is_empty() {
                current.message_count = current.message_count.saturating_add(1);
                if role == AiMessageRole::User && current.first_user_prompt.is_none() {
                    current.first_user_prompt = Some(truncate(&text, 500));
                    current.title = Some(truncate(&text, 80));
                }
                query_match |= matches_query(query.query.as_deref(), [&text, cwd, session_id]);
            }
        }
        if query_match && let Some(session) = session {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

fn read_codex_sessions(
    source: &dyn AiSessionDataSource,
    query: &AiSessionQuery,
    home: &Path,
) -> Result<Vec<AiSession>, AiSessionError> {
    let root = home.join(".codex/sessions");
    if !source.path_exists(&root)? {
        return Err(AiSessionError::new("Codex sessions directory not found"));
    }
    let files = source.list_files(&root, ".jsonl", true)?;
    let mut sessions = Vec::new();
    for file in files {
        let values = json_lines(&source.read_text(&file)?);
        let Some(meta) = values.iter().find(|value| {
            value.get("type").and_then(serde_json::Value::as_str) == Some("session_meta")
        }) else {
            continue;
        };
        let payload = meta.get("payload").unwrap_or(&serde_json::Value::Null);
        let Some(cwd) = payload.get("cwd").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !path_matches(cwd, &query.target_path) {
            continue;
        }
        let session_id = payload
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                file.file_stem()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default();
        let started_at =
            timestamp_value(payload.get("timestamp").or_else(|| meta.get("timestamp")));
        let mut updated_at = started_at.clone();
        let mut first_user_prompt = None;
        let mut message_count = 0usize;
        let mut query_match = matches_query(query.query.as_deref(), [&session_id, cwd]);
        let mut seen = std::collections::BTreeSet::new();
        for value in &values {
            for (role, text) in codex_messages(value) {
                if should_skip_codex_message(role, &text) {
                    continue;
                }
                let timestamp = timestamp_value(value.get("timestamp"));
                let key = format!("{role:?}\0{}\0{text}", timestamp.as_deref().unwrap_or(""));
                if !seen.insert(key) {
                    continue;
                }
                if role == AiMessageRole::User && first_user_prompt.is_none() {
                    first_user_prompt = Some(text.clone());
                }
                query_match |= matches_query(query.query.as_deref(), [text.as_str()]);
                message_count = message_count.saturating_add(1);
                updated_at = timestamp.or(updated_at);
            }
        }
        if query_match {
            sessions.push(AiSession {
                id: session_id,
                provider: AiSessionProvider::Codex,
                started_at,
                updated_at,
                matched_path: cwd.to_owned(),
                title: first_user_prompt.as_deref().map(|text| truncate(text, 80)),
                first_user_prompt: first_user_prompt.as_deref().map(|text| truncate(text, 500)),
                message_count,
                source_ref: file.to_string_lossy().into_owned(),
            });
        }
    }
    Ok(sessions)
}

fn read_gemini_sessions(
    source: &dyn AiSessionDataSource,
    query: &AiSessionQuery,
    home: &Path,
) -> Result<Vec<AiSession>, AiSessionError> {
    let root = home.join(".gemini");
    if !source.path_exists(&root)? {
        return Err(AiSessionError::new("Gemini CLI directory not found"));
    }
    let project_paths = gemini_project_paths(source, &root)?;
    let files = source
        .list_files(&root.join("tmp"), ".json", true)?
        .into_iter()
        .filter(|file| {
            file.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                == Some("chats")
        });
    let mut sessions = Vec::new();
    for file in files {
        let value = match serde_json::from_str::<serde_json::Value>(&source.read_text(&file)?) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let matched_path = gemini_matched_path(source, &file, &value, &project_paths)?;
        let Some(matched_path) = matched_path else {
            continue;
        };
        if !path_matches(&matched_path, &query.target_path) {
            continue;
        }
        let messages = gemini_messages(&value);
        let first_user_prompt = messages
            .iter()
            .find(|message| message.role == AiMessageRole::User)
            .map(|message| message.full_text.clone());
        let id = string_field(&value, &["sessionId", "id"])
            .or_else(|| {
                file.file_stem()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_default();
        let title = string_field(&value, &["title"])
            .or_else(|| first_user_prompt.as_deref().map(|text| truncate(text, 80)));
        if !matches_query(
            query.query.as_deref(),
            std::iter::once(id.as_str())
                .chain(std::iter::once(matched_path.as_str()))
                .chain(title.as_deref())
                .chain(first_user_prompt.as_deref())
                .chain(messages.iter().map(|message| message.full_text.as_str())),
        ) {
            continue;
        }
        sessions.push(AiSession {
            id,
            provider: AiSessionProvider::Gemini,
            started_at: timestamp_fields(&value, &["startTime", "createdAt", "timestamp"]),
            updated_at: timestamp_fields(&value, &["lastUpdated", "updatedAt", "timestamp"]),
            matched_path,
            title,
            first_user_prompt: first_user_prompt.as_deref().map(|text| truncate(text, 500)),
            message_count: messages.len(),
            source_ref: file.to_string_lossy().into_owned(),
        });
    }
    Ok(sessions)
}

fn read_open_code_sessions(
    source: &dyn AiSessionDataSource,
    query: &AiSessionQuery,
    home: &Path,
) -> Result<Vec<AiSession>, AiSessionError> {
    let database_path = home.join(".local/share/opencode/opencode.db");
    let Some(rows) = source.open_code_sessions(&database_path)? else {
        return Err(AiSessionError::new("OpenCode database not found"));
    };
    Ok(rows
        .into_iter()
        .filter(|row| path_matches(&row.directory, &query.target_path))
        .filter_map(|row| {
            let first_user_prompt = row
                .first_user_part
                .as_deref()
                .map(open_code_part_text)
                .filter(|text| !text.is_empty());
            let title = row
                .title
                .clone()
                .or_else(|| first_user_prompt.as_deref().map(|text| truncate(text, 80)));
            matches_query(
                query.query.as_deref(),
                [
                    row.id.as_str(),
                    row.directory.as_str(),
                    title.as_deref().unwrap_or(""),
                    first_user_prompt.as_deref().unwrap_or(""),
                ],
            )
            .then(|| AiSession {
                id: row.id.clone(),
                provider: AiSessionProvider::OpenCode,
                started_at: row.time_created.map(epoch_millis_to_iso),
                updated_at: row.time_updated.map(epoch_millis_to_iso),
                matched_path: row.directory,
                title,
                first_user_prompt: first_user_prompt.as_deref().map(|text| truncate(text, 500)),
                message_count: row.part_count,
                source_ref: row.id,
            })
        })
        .collect())
}

fn json_lines(content: &str) -> Vec<serde_json::Value> {
    content
        .lines()
        .filter_map(|line| serde_json::from_str(line.trim()).ok())
        .collect()
}

fn path_matches(candidate: &str, target: &Path) -> bool {
    let candidate = normalize_path(Path::new(candidate));
    let target = normalize_path(target);
    candidate == target || candidate.starts_with(&target)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn matches_query<'a>(query: Option<&str>, values: impl IntoIterator<Item = &'a str>) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    let query = query.to_lowercase();
    values
        .into_iter()
        .any(|value| value.to_lowercase().contains(&query))
}

fn truncate(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }
    let prefix_length = max_chars.saturating_sub(3);
    format!(
        "{}...",
        normalized
            .chars()
            .take(prefix_length)
            .collect::<String>()
            .trim_end()
    )
}

fn extract_plain_text(value: Option<&serde_json::Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Array(values) => values
            .iter()
            .map(|value| {
                value.get("text").map_or_else(
                    || extract_plain_text(Some(value)),
                    |text| extract_plain_text(Some(text)),
                )
            })
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Object(object) => [
            "text",
            "input_text",
            "output_text",
            "display",
            "description",
            "subject",
            "output",
            "result",
            "content",
            "message",
            "title",
        ]
        .iter()
        .find_map(|key| object.get(*key))
        .map(|value| extract_plain_text(Some(value)))
        .unwrap_or_default(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Null => String::new(),
    }
}

fn claude_role(value: &serde_json::Value) -> AiMessageRole {
    let content = value.pointer("/message/content");
    if content.is_some_and(|content| {
        content.as_array().is_some_and(|parts| {
            parts.iter().any(|part| {
                part.get("type").and_then(serde_json::Value::as_str) == Some("tool_result")
            })
        })
    }) {
        return AiMessageRole::Tool;
    }
    match value
        .pointer("/message/role")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("type").and_then(serde_json::Value::as_str))
    {
        Some("user") => AiMessageRole::User,
        Some("assistant") => AiMessageRole::Assistant,
        Some("system") => AiMessageRole::System,
        Some("developer") => AiMessageRole::Developer,
        Some("tool") | Some("tool_result") | Some("progress") => AiMessageRole::Tool,
        _ => AiMessageRole::Unknown,
    }
}

fn codex_messages(value: &serde_json::Value) -> Vec<(AiMessageRole, String)> {
    let event_type = value.get("type").and_then(serde_json::Value::as_str);
    let payload = value.get("payload").unwrap_or(&serde_json::Value::Null);
    if event_type == Some("event_msg")
        && payload.get("type").and_then(serde_json::Value::as_str) == Some("agent_message")
    {
        let text = extract_plain_text(payload.get("message"));
        return (!text.is_empty())
            .then_some((AiMessageRole::Assistant, text))
            .into_iter()
            .collect();
    }
    if event_type != Some("response_item") {
        return Vec::new();
    }
    let payload_type = payload.get("type").and_then(serde_json::Value::as_str);
    let (role, text) = match payload_type {
        Some("message") => {
            let role = match payload.get("role").and_then(serde_json::Value::as_str) {
                Some("user") => AiMessageRole::User,
                Some("assistant") => AiMessageRole::Assistant,
                Some("system") => AiMessageRole::System,
                Some("developer") => AiMessageRole::Developer,
                Some("tool") => AiMessageRole::Tool,
                _ => AiMessageRole::Unknown,
            };
            (role, extract_plain_text(payload.get("content")))
        }
        Some("reasoning") => (
            AiMessageRole::Reasoning,
            extract_plain_text(
                payload
                    .get("summary")
                    .or_else(|| payload.get("content"))
                    .or_else(|| payload.get("text"))
                    .or(Some(payload)),
            ),
        ),
        Some("function_call" | "tool_call") => {
            let name = payload
                .get("name")
                .or_else(|| payload.get("call_id"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool");
            let arguments = extract_plain_text(
                payload
                    .get("arguments")
                    .or_else(|| payload.get("args"))
                    .or_else(|| payload.get("input")),
            );
            (
                AiMessageRole::Tool,
                if arguments.is_empty() {
                    format!("{name} called")
                } else {
                    format!("{name}: {arguments}")
                },
            )
        }
        Some("function_call_output" | "tool_result") => (
            AiMessageRole::Tool,
            extract_plain_text(
                payload
                    .get("output")
                    .or_else(|| payload.get("result"))
                    .or_else(|| payload.get("content"))
                    .or(Some(payload)),
            ),
        ),
        _ => (AiMessageRole::Unknown, extract_plain_text(Some(payload))),
    };
    (!text.is_empty())
        .then_some((role, text))
        .into_iter()
        .collect()
}

fn should_skip_codex_message(role: AiMessageRole, text: &str) -> bool {
    let text = text.trim();
    (role == AiMessageRole::User
        && ["<environment_context>", "<skill>", "<system-reminder>"]
            .iter()
            .any(|prefix| text.starts_with(prefix)))
        || (role == AiMessageRole::Developer
            && [
                "<permissions instructions>",
                "<apps_instructions>",
                "<skills_instructions>",
                "<collaboration_mode>",
            ]
            .iter()
            .any(|marker| text.contains(marker)))
}

fn gemini_project_paths(
    source: &dyn AiSessionDataSource,
    root: &Path,
) -> Result<std::collections::BTreeMap<String, String>, AiSessionError> {
    let projects_path = root.join("projects.json");
    if !source.path_exists(&projects_path)? {
        return Ok(std::collections::BTreeMap::new());
    }
    let value = match serde_json::from_str::<serde_json::Value>(&source.read_text(&projects_path)?)
    {
        Ok(value) => value,
        Err(_) => return Ok(std::collections::BTreeMap::new()),
    };
    let object = value
        .get("projects")
        .and_then(serde_json::Value::as_object)
        .or_else(|| value.as_object());
    let mut projects = std::collections::BTreeMap::new();
    for (path, value) in object.into_iter().flatten() {
        let id = value.as_str().or_else(|| {
            ["id", "hash", "projectId", "projectHash"]
                .iter()
                .find_map(|key| value.get(*key).and_then(serde_json::Value::as_str))
        });
        if let Some(id) = id {
            projects.insert(id.to_owned(), path.to_owned());
        }
    }
    Ok(projects)
}

fn gemini_matched_path(
    source: &dyn AiSessionDataSource,
    file: &Path,
    value: &serde_json::Value,
    projects: &std::collections::BTreeMap<String, String>,
) -> Result<Option<String>, AiSessionError> {
    if let Some(path) = string_field(value, &["cwd", "projectPath", "directory"]) {
        return Ok(Some(path));
    }
    let project_directory = file.parent().and_then(Path::parent);
    let project_id = project_directory
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    if let Some(mapped) = project_id.and_then(|id| projects.get(id)) {
        return Ok(Some(mapped.clone()));
    }
    let Some(project_directory) = project_directory else {
        return Ok(None);
    };
    let project_root = project_directory.join(".project_root");
    if !source.path_exists(&project_root)? {
        return Ok(None);
    }
    Ok(source.read_text(&project_root)?.trim().to_owned().into())
}

fn gemini_messages(value: &serde_json::Value) -> Vec<AiMessage> {
    value
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|message| {
            let timestamp = timestamp_fields(message, &["timestamp", "createdAt", "time"]);
            let mut messages = Vec::new();
            if let Some(thoughts) = ["thoughts", "thought", "reasoning"]
                .iter()
                .find_map(|key| message.get(*key))
            {
                for thought in thoughts
                    .as_array()
                    .map_or_else(|| vec![thoughts], |thoughts| thoughts.iter().collect())
                {
                    if let Some(message) = make_message(
                        AiMessageRole::Reasoning,
                        timestamp.clone(),
                        extract_plain_text(Some(thought)),
                    ) {
                        messages.push(message);
                    }
                }
            }
            if let Some(tool_calls) = [
                "toolCalls",
                "tool_calls",
                "functionCalls",
                "function_calls",
                "tools",
            ]
            .iter()
            .find_map(|key| message.get(*key))
            {
                for tool_call in tool_calls.as_array().map_or_else(
                    || vec![tool_calls],
                    |tool_calls| tool_calls.iter().collect(),
                ) {
                    if let Some(message) = make_message(
                        AiMessageRole::Tool,
                        timestamp.clone(),
                        format_gemini_tool_call(tool_call),
                    ) {
                        messages.push(message);
                    }
                }
            }
            let role = match string_field(message, &["type", "role"]).as_deref() {
                Some("user") => AiMessageRole::User,
                Some("gemini" | "assistant" | "model") => AiMessageRole::Assistant,
                Some("system" | "info") => AiMessageRole::System,
                Some("tool") => AiMessageRole::Tool,
                Some("reasoning" | "thought") => AiMessageRole::Reasoning,
                Some("developer") => AiMessageRole::Developer,
                _ => AiMessageRole::Unknown,
            };
            let text = extract_plain_text(
                ["content", "parts", "text", "message"]
                    .iter()
                    .find_map(|key| message.get(*key)),
            );
            if let Some(message) = make_message(role, timestamp, text) {
                messages.push(message);
            }
            messages
        })
        .collect()
}

fn format_gemini_tool_call(value: &serde_json::Value) -> String {
    let Some(object) = value.as_object() else {
        return extract_plain_text(Some(value));
    };
    let name = ["name", "tool", "functionName"]
        .iter()
        .find_map(|key| object.get(*key).and_then(serde_json::Value::as_str))
        .unwrap_or("tool");
    let arguments = ["args", "arguments", "input"]
        .iter()
        .find_map(|key| object.get(*key))
        .map(stringify_json)
        .unwrap_or_default();
    let result = ["result", "output", "response", "content"]
        .iter()
        .find_map(|key| object.get(*key))
        .map(|value| extract_plain_text(Some(value)))
        .unwrap_or_default();
    [
        Some(name.to_owned()),
        (!arguments.is_empty()).then(|| format!("args: {arguments}")),
        (!result.is_empty()).then(|| format!("result: {result}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" | ")
}

fn stringify_json(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn open_code_part_text(raw: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return String::new();
    };
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("text" | "reasoning") => extract_plain_text(value.get("text")),
        Some("tool") => {
            let tool = value
                .get("tool")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("tool");
            let output = extract_plain_text(value.get("state"));
            if output.is_empty() {
                format!("{tool} executed")
            } else {
                format!("{tool}: {output}")
            }
        }
        _ => String::new(),
    }
}

fn open_code_role(message_data: &str, part_data: &str) -> AiMessageRole {
    let part_type = serde_json::from_str::<serde_json::Value>(part_data)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        });
    match part_type.as_deref() {
        Some("tool") => return AiMessageRole::Tool,
        Some("reasoning") => return AiMessageRole::Reasoning,
        _ => {}
    }
    match serde_json::from_str::<serde_json::Value>(message_data)
        .ok()
        .and_then(|value| {
            value
                .get("role")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .as_deref()
    {
        Some("user") => AiMessageRole::User,
        Some("assistant") => AiMessageRole::Assistant,
        Some("system") => AiMessageRole::System,
        Some("developer") => AiMessageRole::Developer,
        Some("tool") => AiMessageRole::Tool,
        _ => AiMessageRole::Unknown,
    }
}

fn string_field(value: &serde_json::Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| value.get(*field).and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)
}

fn timestamp_fields(value: &serde_json::Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| timestamp_value(value.get(*field)))
}

fn timestamp_value(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(value) if !value.trim().is_empty() => Some(value.clone()),
        serde_json::Value::Number(value) => value.as_i64().map(|value| {
            let millis = if value > 1_000_000_000_000 {
                value
            } else {
                value.saturating_mul(1000)
            };
            epoch_millis_to_iso(millis)
        }),
        _ => None,
    }
}

fn epoch_millis_to_iso(epoch_millis: i64) -> String {
    let seconds = epoch_millis.div_euclid(1000);
    let millis = epoch_millis.rem_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3600;
    let minute = (day_seconds % 3600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn make_message(role: AiMessageRole, timestamp: Option<String>, text: String) -> Option<AiMessage> {
    let full_text = text.trim().to_owned();
    if full_text.is_empty() {
        return None;
    }
    let preview = truncate(&full_text, 500);
    Some(AiMessage {
        role,
        timestamp,
        is_truncated: preview != full_text,
        text: preview,
        full_text,
    })
}

fn paginate<T>(items: Vec<T>, cursor: Option<&str>, limit: usize) -> (Vec<T>, Option<String>) {
    let offset = cursor
        .and_then(|cursor| cursor.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = limit.clamp(1, MAX_PAGE_LIMIT);
    let total = items.len();
    let page = items
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let next = offset
        .saturating_add(page.len())
        .lt(&total)
        .then(|| offset.saturating_add(page.len()).to_string());
    (page, next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FixtureSource {
        home: PathBuf,
    }

    impl AiSessionDataSource for FixtureSource {
        fn is_remote(&self) -> bool {
            false
        }

        fn home_directory(&self) -> Result<PathBuf, AiSessionError> {
            Ok(self.home.clone())
        }

        fn path_exists(&self, path: &Path) -> Result<bool, AiSessionError> {
            Ok(path.exists())
        }

        fn list_files(
            &self,
            root: &Path,
            suffix: &str,
            recursive: bool,
        ) -> Result<Vec<PathBuf>, AiSessionError> {
            list_local_files(root, suffix, recursive)
        }

        fn read_text(&self, path: &Path) -> Result<String, AiSessionError> {
            std::fs::read_to_string(path).map_err(|error| AiSessionError::new(error.to_string()))
        }

        fn open_code_sessions(
            &self,
            database_path: &Path,
        ) -> Result<Option<Vec<OpenCodeSessionRow>>, AiSessionError> {
            LocalAiSessionDataSource.open_code_sessions(database_path)
        }

        fn open_code_messages(
            &self,
            database_path: &Path,
            session_id: &str,
        ) -> Result<Option<Vec<OpenCodeMessageRow>>, AiSessionError> {
            LocalAiSessionDataSource.open_code_messages(database_path, session_id)
        }
    }

    #[test]
    fn local_reader_discovers_all_provider_formats_and_filters_by_exact_worktree() {
        let fixture = fixture_directory("all-providers");
        let worktree = fixture.join("workspace/repo");
        std::fs::create_dir_all(&worktree).expect("worktree");
        let source = FixtureSource {
            home: fixture.join("home"),
        };
        let home = source.home_directory().expect("fixture home");

        write(
            home.join(".claude/projects/project/claude.jsonl"),
            &format!(
                "{{\"sessionId\":\"claude-1\",\"cwd\":{},\"timestamp\":\"2026-07-29T01:00:00Z\",\"message\":{{\"role\":\"user\",\"content\":\"Claude request\"}}}}\n\
                 {{\"sessionId\":\"claude-1\",\"cwd\":{},\"timestamp\":\"2026-07-29T01:01:00Z\",\"message\":{{\"role\":\"assistant\",\"content\":\"Claude response\"}}}}\n",
                serde_json::to_string(&worktree).expect("worktree JSON"),
                serde_json::to_string(&worktree).expect("worktree JSON")
            ),
        );
        write(
            home.join(".codex/sessions/2026/codex.jsonl"),
            &format!(
                "{{\"type\":\"session_meta\",\"timestamp\":\"2026-07-29T02:00:00Z\",\"payload\":{{\"id\":\"codex-1\",\"cwd\":{},\"timestamp\":\"2026-07-29T02:00:00Z\"}}}}\n\
                 {{\"type\":\"response_item\",\"timestamp\":\"2026-07-29T02:01:00Z\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"Codex request\"}}]}}}}\n\
                 {{\"type\":\"response_item\",\"timestamp\":\"2026-07-29T02:02:00Z\",\"payload\":{{\"type\":\"reasoning\",\"summary\":[{{\"text\":\"Codex reasoning\"}}]}}}}\n\
                 {{\"type\":\"response_item\",\"timestamp\":\"2026-07-29T02:03:00Z\",\"payload\":{{\"type\":\"function_call\",\"name\":\"cargo_test\",\"arguments\":\"--workspace\"}}}}\n",
                serde_json::to_string(&worktree).expect("worktree JSON")
            ),
        );
        write(
            home.join(".gemini/tmp/gemini-project/chats/gemini.json"),
            &serde_json::json!({
                "sessionId": "gemini-1",
                "cwd": worktree,
                "timestamp": "2026-07-29T03:00:00Z",
                "messages": [
                    {
                        "type": "user",
                        "timestamp": "2026-07-29T03:00:00Z",
                        "content": "Gemini request",
                        "thoughts": [{"text": "Gemini reasoning"}],
                        "toolCalls": [{"name": "read_file", "args": {"path": "README.md"}}]
                    }
                ]
            })
            .to_string(),
        );
        create_open_code_fixture(&home.join(".local/share/opencode/opencode.db"), &worktree);

        let page = read_ai_sessions(&source, &AiSessionQuery::new(&worktree, &worktree))
            .expect("read provider sessions");

        assert_eq!(page.sessions.len(), 4);
        assert_eq!(
            page.sessions
                .iter()
                .map(|session| session.provider)
                .collect::<std::collections::BTreeSet<_>>(),
            AiSessionProvider::ALL.into_iter().collect()
        );
        assert!(
            page.sources
                .iter()
                .all(|status| status.available && status.session_count == 1)
        );
        assert_eq!(page.sessions[0].provider, AiSessionProvider::OpenCode);
        for session in &page.sessions {
            let detail = read_ai_session_detail(
                &source,
                &AiDetailQuery {
                    sessions: AiSessionQuery::new(&worktree, &worktree),
                    provider: session.provider,
                    session_id: session.id.clone(),
                    source_ref: Some(session.source_ref.clone()),
                    roles: Vec::new(),
                    cursor: None,
                    limit: DEFAULT_DETAIL_LIMIT,
                },
            )
            .expect("read session detail")
            .expect("session detail");
            assert_eq!(detail.provider, session.provider);
            assert!(
                !detail.messages.is_empty(),
                "{} detail should contain messages",
                session.provider.as_str()
            );
            let roles = detail
                .messages
                .iter()
                .map(|message| message.role)
                .collect::<std::collections::BTreeSet<_>>();
            if matches!(
                session.provider,
                AiSessionProvider::Codex | AiSessionProvider::Gemini
            ) {
                assert!(roles.contains(&AiMessageRole::Reasoning));
                assert!(roles.contains(&AiMessageRole::Tool));
            }
        }
        std::fs::remove_dir_all(fixture).expect("remove fixture");
    }

    #[test]
    fn pagination_and_query_match_message_bodies_without_path_prefix_false_positives() {
        assert!(!path_matches(
            "/workspace/repository",
            Path::new("/workspace/repo")
        ));
        assert!(path_matches(
            "/workspace/repo/subdirectory",
            Path::new("/workspace/repo")
        ));
        let (page, next) = paginate(vec![1, 2, 3], Some("1"), 1);
        assert_eq!(page, vec![2]);
        assert_eq!(next.as_deref(), Some("2"));
        assert!(matches_query(Some("MIGRATION"), ["database migration"]));
    }

    fn create_open_code_fixture(database_path: &Path, worktree: &Path) {
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).expect("OpenCode parent");
        }
        let connection = rusqlite::Connection::open(database_path).expect("OpenCode database");
        connection
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY, directory TEXT NOT NULL, title TEXT,
                    time_created INTEGER, time_updated INTEGER
                 );
                 CREATE TABLE message (
                    id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL
                 );
                 CREATE TABLE part (
                    id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL,
                    time_created INTEGER, data TEXT NOT NULL
                 );",
            )
            .expect("OpenCode schema");
        connection
            .execute(
                "INSERT INTO session VALUES ('opencode-1', ?1, 'OpenCode request', 1785301200000, 1785301260000)",
                [worktree.to_string_lossy().as_ref()],
            )
            .expect("OpenCode session");
        connection
            .execute(
                "INSERT INTO message VALUES ('message-1', 'opencode-1', '{\"role\":\"user\"}')",
                [],
            )
            .expect("OpenCode message");
        connection
            .execute(
                "INSERT INTO part VALUES (
                    'part-1', 'message-1', 'opencode-1', 1785301200000,
                    '{\"type\":\"text\",\"text\":\"OpenCode request\"}'
                 )",
                [],
            )
            .expect("OpenCode part");
    }

    fn fixture_directory(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let fixture =
            std::env::temp_dir().join(format!("kanvibe-ai-{name}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&fixture).expect("fixture directory");
        fixture
    }

    fn write(path: PathBuf, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("file parent");
        }
        std::fs::write(path, content).expect("write fixture");
    }
}
