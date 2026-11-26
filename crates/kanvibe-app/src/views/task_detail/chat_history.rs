//! AI 대화 기록 화면의 상태(TS `InlineAiChatView`의 상태와 요청 순서 규칙).
//!
//! 화면은 요청을 만들어 보내고, 끝난 응답을 요청 번호와 함께 돌려준다. 그 사이에 검색·역할 필터·세션 선택이 바뀌었으면
//! 늦게 도착한 응답은 버린다. 그래서 이 모듈은 GPUI 없이 요청과 응답의 순서만으로 확인할 수 있다.

use std::collections::HashSet;

use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AggregatedAiMessage, AggregatedAiSession, AggregatedAiSessionDetail,
    AggregatedAiSessionsResult, AiMessageRole,
};
use kanvibe_services::project::ports::AiSessionListRequest;

use super::session_queries::SessionDetailQuery;

/// TS `INLINE_CHAT_SESSION_LIMIT`
pub const SESSION_PAGE_LIMIT: usize = 20;
/// TS `INLINE_CHAT_DETAIL_LIMIT`
pub const DETAIL_PAGE_LIMIT: usize = 40;
/// 역할 필터 버튼 순서(TS `INLINE_CHAT_ROLE_FILTERS`)
pub const ROLE_FILTERS: [AiMessageRole; 7] = [
    AiMessageRole::User,
    AiMessageRole::Assistant,
    AiMessageRole::System,
    AiMessageRole::Developer,
    AiMessageRole::Reasoning,
    AiMessageRole::Tool,
    AiMessageRole::Unknown,
];
/// 기록이 없어도 늘 보이는 provider 필터
const ALWAYS_SHOWN_PROVIDERS: [AiSessionProvider; 4] = [
    AiSessionProvider::Claude,
    AiSessionProvider::Codex,
    AiSessionProvider::Opencode,
    AiSessionProvider::Gemini,
];

/// 세션 목록 한 쪽 요청
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRequest {
    pub id: u64,
    pub is_append: bool,
    pub list: AiSessionListRequest,
}

/// 대화 상세 한 쪽 요청. `is_older`면 이전 메시지를 뒤에 붙인다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailRequest {
    pub id: u64,
    pub is_older: bool,
    pub query: SessionDetailQuery,
}

/// 메시지 한 줄과 펼침 상태를 이어 줄 열쇠
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptRow<'a> {
    pub message: &'a AggregatedAiMessage,
    pub key: String,
}

#[derive(Debug, Default)]
pub struct ChatHistoryState {
    pub history: Option<AggregatedAiSessionsResult>,
    pub history_error: bool,
    pub is_history_loading: bool,
    pub selected_key: Option<String>,
    pub detail: Option<AggregatedAiSessionDetail>,
    /// 상세 조회가 실패한 세션 id
    pub detail_error_session: Option<String>,
    pub is_older_loading: bool,
    pub active_roles: Vec<AiMessageRole>,
    pub active_providers: Vec<AiSessionProvider>,
    pub applied_query: Option<String>,
    pub preview_file: Option<String>,
    history_request_id: u64,
    detail_request_id: u64,
}

pub fn session_key(session: &AggregatedAiSession) -> String {
    format!(
        "{}:{}:{}",
        session.provider.id(),
        session.id,
        session.source_ref.as_deref().unwrap_or_default()
    )
}

/// 목록 제목(TS `title ?? firstUserPrompt ?? id`)
pub fn session_title(session: &AggregatedAiSession) -> String {
    session
        .title
        .clone()
        .or_else(|| session.first_user_prompt.clone())
        .unwrap_or_else(|| session.id.clone())
}

/// 제목과 다를 때만 첫 입력을 미리보기로 보인다
pub fn session_preview(session: &AggregatedAiSession) -> Option<String> {
    let preview = session.first_user_prompt.as_deref()?.trim();
    let title = session_title(session);
    (!preview.is_empty() && preview != title.trim())
        .then(|| session.first_user_prompt.clone().unwrap_or_default())
}

fn normalize_query(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// FNV-1a 32비트(TS `Math.imul` 해시와 같은 값)
fn message_hash(text: &str) -> u32 {
    text.encode_utf16().fold(2_166_136_261_u32, |hash, unit| {
        (hash ^ u32::from(unit)).wrapping_mul(16_777_619)
    })
}

/// 최신순 기록에서 열쇠를 계산해 시간순으로 뒤집는다. 이전 페이지가 붙어도 같은 메시지는 같은 열쇠를 지킨다
pub fn transcript_rows<'a>(
    messages: &'a [AggregatedAiMessage],
    session_key: Option<&str>,
) -> Vec<TranscriptRow<'a>> {
    let mut occurrences: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut rows: Vec<TranscriptRow<'a>> = messages
        .iter()
        .map(|message| {
            let body = if message.full_text.is_empty() {
                &message.text
            } else {
                &message.full_text
            };
            let attachment_ids: Vec<&str> = message
                .attachment_list()
                .iter()
                .map(|attachment| attachment.id.as_str())
                .collect();
            let hashed = format!("{body}\u{0}{}", attachment_ids.join(","));
            let role = serde_json::to_value(message.role)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default();
            let identity = format!(
                "{role}:{}:{}",
                message.timestamp.as_deref().unwrap_or_default(),
                message_hash(&hashed)
            );
            let occurrence = occurrences.entry(identity.clone()).or_default();
            let key = format!("{}:{identity}:{occurrence}", session_key.unwrap_or("null"));
            *occurrence += 1;
            TranscriptRow { message, key }
        })
        .collect();
    rows.reverse();
    rows
}

impl ChatHistoryState {
    pub fn selected_session(&self) -> Option<&AggregatedAiSession> {
        let selected_key = self.selected_key.as_deref()?;
        self.history
            .as_ref()?
            .sessions
            .iter()
            .find(|session| session_key(session) == selected_key)
    }

    pub fn filtered_sessions(&self) -> Vec<&AggregatedAiSession> {
        let Some(history) = self.history.as_ref() else {
            return Vec::new();
        };
        history
            .sessions
            .iter()
            .filter(|session| {
                self.active_providers.is_empty()
                    || self.active_providers.contains(&session.provider)
            })
            .collect()
    }

    /// 필터 버튼을 둘 provider(목록 순서). 네 provider는 늘, 나머지는 기록이 있을 때만
    pub fn provider_filters(&self) -> Vec<(AiSessionProvider, usize)> {
        AiSessionProvider::ALL
            .into_iter()
            .filter(|provider| {
                ALWAYS_SHOWN_PROVIDERS.contains(provider)
                    || self.history.as_ref().is_some_and(|history| {
                        history
                            .sources
                            .iter()
                            .any(|source| source.provider == *provider && source.available)
                            || history
                                .sessions
                                .iter()
                                .any(|session| session.provider == *provider)
                    })
            })
            .map(|provider| {
                let count = self.history.as_ref().map_or(0, |history| {
                    history
                        .sessions
                        .iter()
                        .filter(|session| session.provider == provider)
                        .count()
                });
                (provider, count)
            })
            .collect()
    }

    /// 목록 머리말 개수. 필터 중이면 `보이는 수/전체`
    pub fn history_count_label(&self) -> Option<String> {
        let history = self.history.as_ref()?;
        Some(match self.active_providers.is_empty() {
            true => history.sessions.len().to_string(),
            false => format!(
                "{}/{}",
                self.filtered_sessions().len(),
                history.sessions.len()
            ),
        })
    }

    /// 목록을 다시 읽는다(검색·새로고침). 늦게 오는 이전 응답은 이 요청 뒤로 모두 버린다
    pub fn begin_history_load(&mut self, query: &str) -> HistoryRequest {
        self.next_history_request(normalize_query(query), None, false)
    }

    /// 다음 세션 쪽. 읽는 중이거나 다음 쪽이 없으면 `None`
    pub fn begin_more_sessions(&mut self) -> Option<HistoryRequest> {
        if self.is_history_loading {
            return None;
        }
        let cursor = self.history.as_ref()?.next_cursor.clone()?;
        let query = self.applied_query.clone();
        Some(self.next_history_request(query, Some(cursor), true))
    }

    fn next_history_request(
        &mut self,
        query: Option<String>,
        cursor: Option<String>,
        is_append: bool,
    ) -> HistoryRequest {
        self.history_request_id += 1;
        self.is_history_loading = true;
        self.history_error = false;
        HistoryRequest {
            id: self.history_request_id,
            is_append,
            list: AiSessionListRequest {
                query,
                cursor,
                limit: Some(SESSION_PAGE_LIMIT),
            },
        }
    }

    pub fn finish_history_load(
        &mut self,
        request: &HistoryRequest,
        result: Result<AggregatedAiSessionsResult, ()>,
    ) {
        if request.id != self.history_request_id {
            return;
        }
        self.is_history_loading = false;
        let Ok(result) = result else {
            self.history_error = true;
            return;
        };
        self.applied_query = request.list.query.clone();
        match (request.is_append, self.history.take()) {
            (true, Some(current)) => self.history = Some(merge_session_pages(current, result)),
            _ => {
                self.history = Some(result);
                self.detail_request_id += 1;
                self.selected_key = None;
                self.detail = None;
                self.detail_error_session = None;
                self.active_roles.clear();
            }
        }
    }

    pub fn toggle_provider(&mut self, provider: AiSessionProvider) {
        toggle(&mut self.active_providers, provider);
        let is_selection_hidden = self.selected_key.as_deref().is_some_and(|selected_key| {
            !self
                .filtered_sessions()
                .iter()
                .any(|session| session_key(session) == selected_key)
        });
        if is_selection_hidden {
            self.clear_selection();
        }
    }

    /// 세션을 고르고 첫 상세 쪽을 요청한다
    pub fn select_session(&mut self, key: String) -> Option<DetailRequest> {
        self.selected_key = Some(key);
        self.active_roles.clear();
        self.preview_file = None;
        self.begin_detail_load()
    }

    /// 대화에서 기록으로 돌아간다(목록은 그대로)
    pub fn clear_selection(&mut self) {
        self.detail_request_id += 1;
        self.selected_key = None;
        self.detail = None;
        self.detail_error_session = None;
        self.active_roles.clear();
        self.preview_file = None;
    }

    pub fn toggle_role(&mut self, role: AiMessageRole) -> Option<DetailRequest> {
        toggle(&mut self.active_roles, role);
        self.begin_detail_load()
    }

    /// 고른 세션의 최신 쪽을 처음부터 다시 읽는다(선택·역할·검색이 바뀌었거나 다시 시도)
    pub fn begin_detail_load(&mut self) -> Option<DetailRequest> {
        self.detail_request_id += 1;
        self.detail = None;
        self.detail_error_session = None;
        self.is_older_loading = false;
        let query = self.detail_query(None)?;
        Some(DetailRequest {
            id: self.detail_request_id,
            is_older: false,
            query,
        })
    }

    /// 이전 메시지 쪽. 읽는 중이거나 더 없으면 `None`
    pub fn begin_older_messages(&mut self) -> Option<DetailRequest> {
        if self.is_older_loading {
            return None;
        }
        let cursor = self.detail.as_ref()?.next_cursor.clone()?;
        let query = self.detail_query(Some(cursor))?;
        self.detail_request_id += 1;
        self.is_older_loading = true;
        self.detail_error_session = None;
        Some(DetailRequest {
            id: self.detail_request_id,
            is_older: true,
            query,
        })
    }

    fn detail_query(&self, cursor: Option<String>) -> Option<SessionDetailQuery> {
        let session = self.selected_session()?;
        Some(SessionDetailQuery {
            provider: session.provider,
            session_id: session.id.clone(),
            source_ref: session.source_ref.clone(),
            cursor,
            limit: DETAIL_PAGE_LIMIT,
            query: self.applied_query.clone(),
            roles: (!self.active_roles.is_empty()).then(|| self.active_roles.clone()),
        })
    }

    pub fn finish_detail_load(
        &mut self,
        request: &DetailRequest,
        result: Result<Option<AggregatedAiSessionDetail>, ()>,
    ) {
        if request.id != self.detail_request_id {
            return;
        }
        self.is_older_loading = false;
        let Ok(Some(page)) = result else {
            self.detail_error_session = Some(request.query.session_id.clone());
            return;
        };
        self.detail = match (request.is_older, self.detail.take()) {
            (true, Some(mut current)) if current.session_id == page.session_id => {
                current.messages.extend(page.messages);
                current.next_cursor = page.next_cursor;
                Some(current)
            }
            _ => Some(page),
        };
    }

    /// 고른 세션의 메시지(최신순). 상세가 다른 세션 것이면 비어 있다
    pub fn messages(&self) -> &[AggregatedAiMessage] {
        match (self.selected_session(), self.detail.as_ref()) {
            (Some(session), Some(detail)) if detail.session_id == session.id => &detail.messages,
            _ => &[],
        }
    }

    pub fn has_detail_error(&self) -> bool {
        self.selected_session().is_some_and(|session| {
            self.detail_error_session.as_deref() == Some(session.id.as_str())
        })
    }

    pub fn is_detail_loading(&self) -> bool {
        self.selected_session().is_some_and(|session| {
            self.detail
                .as_ref()
                .is_none_or(|detail| detail.session_id != session.id)
                && !self.has_detail_error()
        })
    }

    pub fn has_older_messages(&self) -> bool {
        !self.is_detail_loading()
            && self
                .detail
                .as_ref()
                .is_some_and(|detail| detail.next_cursor.is_some())
    }
}

fn toggle<T: PartialEq>(values: &mut Vec<T>, value: T) {
    match values.iter().position(|candidate| *candidate == value) {
        Some(index) => {
            values.remove(index);
        }
        None => values.push(value),
    }
}

/// 다음 쪽을 이어 붙인다. 이미 있는 세션은 다시 넣지 않고, 새 쪽에 출처가 없으면 앞 쪽 출처를 쓴다
fn merge_session_pages(
    mut current: AggregatedAiSessionsResult,
    next: AggregatedAiSessionsResult,
) -> AggregatedAiSessionsResult {
    let mut seen: HashSet<String> = current.sessions.iter().map(session_key).collect();
    for session in next.sessions {
        if seen.insert(session_key(&session)) {
            current.sessions.push(session);
        }
    }
    AggregatedAiSessionsResult {
        sources: if next.sources.is_empty() {
            current.sources
        } else {
            next.sources
        },
        sessions: current.sessions,
        ..next
    }
}

#[cfg(test)]
mod tests;
