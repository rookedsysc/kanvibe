//! 모바일 History 화면의 AI 기록 조회. 목록에서 발견한 원본 경로(`sourceRef`)를 서버에만 기억하고
//! 네트워크 응답에서는 지운다. 경로는 탐색 힌트일 뿐이라 상세를 읽을 때마다 리더가 현재 태스크 경로와 세션을 다시 검증한다.

use std::collections::{BTreeMap, HashMap};
use std::sync::MutexGuard;

use kanvibe_ai::providers::AiSessionProvider;
use kanvibe_ai::types::{
    AggregatedAiSession, AggregatedAiSessionsResult, AiMessageRole, AiSessionAttachmentContent,
};
use kanvibe_services::diff::TaskWorkspaceFile;
use kanvibe_services::project::ports::{
    AiSessionAttachmentRequest, AiSessionDetailRequest, AiSessionListRequest,
};
use serde_json::Value;

use super::MobileBridge;
use crate::error::{MobileError, MobileResult};
use crate::ports::AiHistoryReadError;

const MOBILE_AI_SOURCE_CACHE_LIMIT: usize = 500;
const ATTACHMENT_FAILURE_MESSAGE: &str =
    "첨부 파일을 불러올 수 없습니다. 파일 형식·크기와 원본 파일을 확인해 주세요.";

/// 목록·상세 조회 조건. 라우트가 개수 범위와 역할 이름을 먼저 검증한다
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MobileAiHistoryOptions {
    pub query: Option<String>,
    pub cursor: Option<String>,
    pub limit: usize,
    pub roles: Option<Vec<AiMessageRole>>,
}

/// 최근에 기억한 순서를 지키는 경로 캐시(TS `Map`의 delete 후 set). 상한을 넘으면 가장 오래된 것부터 버린다
#[derive(Default)]
pub(super) struct AiSourceCache {
    next_order: u64,
    entries: HashMap<String, (u64, Option<String>)>,
    order: BTreeMap<u64, String>,
}

impl AiSourceCache {
    fn remember(&mut self, key: String, source_ref: Option<String>) {
        if let Some((previous_order, _)) = self.entries.remove(&key) {
            self.order.remove(&previous_order);
        }
        let order = self.next_order;
        self.next_order += 1;
        self.order.insert(order, key.clone());
        self.entries.insert(key, (order, source_ref));
        if self.entries.len() > MOBILE_AI_SOURCE_CACHE_LIMIT
            && let Some((_, oldest_key)) = self.order.pop_first()
        {
            self.entries.remove(&oldest_key);
        }
    }

    fn get(&self, key: &str) -> Option<Option<String>> {
        self.entries.get(key).map(|(_, source)| source.clone())
    }

    fn forget(&mut self, key: &str) {
        if let Some((order, _)) = self.entries.remove(key) {
            self.order.remove(&order);
        }
    }
}

/// TS `JSON.stringify([taskId, provider, sessionId])`. 다른 태스크나 Agent의 같은 세션 ID는 캐시를 공유하지 않는다
fn mobile_ai_session_key(task_id: &str, provider: AiSessionProvider, session_id: &str) -> String {
    serde_json::to_string(&[task_id, provider.id(), session_id]).unwrap_or_default()
}

impl MobileBridge {
    fn ai_sources(&self) -> MutexGuard<'_, AiSourceCache> {
        self.ai_sources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn remember_mobile_ai_sources(&self, task_id: &str, sessions: &[AggregatedAiSession]) {
        let mut cache = self.ai_sources();
        for session in sessions {
            let key = mobile_ai_session_key(task_id, session.provider, &session.id);
            cache.remember(key, session.source_ref.clone());
        }
    }

    /// 상한보다 많은 세션이 있어도 고른 세션 하나는 찾도록 전체 목록에서 그 세션만 다시 기억한다
    fn refresh_mobile_ai_source(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
    ) -> MobileResult<()> {
        let history = self
            .projects
            .get_task_ai_sessions(task_id, &AiSessionListRequest::default())?;
        let session = history
            .sessions
            .into_iter()
            .find(|entry| entry.provider == provider && entry.id == session_id);
        if let Some(session) = session {
            self.remember_mobile_ai_sources(task_id, &[session]);
        }
        Ok(())
    }

    pub fn is_mobile_ai_session_provider(provider: &str) -> bool {
        AiSessionProvider::parse(provider).is_some()
    }

    pub fn get_mobile_ai_sessions(
        &self,
        task_id: &str,
        options: &MobileAiHistoryOptions,
    ) -> MobileResult<AggregatedAiSessionsResult> {
        let request = AiSessionListRequest {
            query: options.query.clone(),
            cursor: options.cursor.clone(),
            limit: Some(options.limit),
        };
        let mut history = self.projects.get_task_ai_sessions(task_id, &request)?;
        self.remember_mobile_ai_sources(task_id, &history.sessions);
        for session in &mut history.sessions {
            session.source_ref = None;
        }
        Ok(history)
    }

    fn read_ai_session_detail(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
        source_ref: Option<String>,
        options: &MobileAiHistoryOptions,
    ) -> Result<Option<Value>, AiHistoryReadError> {
        let request = AiSessionDetailRequest {
            provider,
            session_id: session_id.to_string(),
            source_ref,
            cursor: options.cursor.clone(),
            limit: options.limit,
            query: options.query.clone(),
            roles: options.roles.clone(),
        };
        let detail = self
            .projects
            .get_task_ai_session_detail(task_id, &request)?;
        Ok(detail.map(|detail| {
            let mut value = serde_json::to_value(detail).unwrap_or(Value::Null);
            if let Value::Object(fields) = &mut value {
                fields.remove("sourceRef");
            }
            value
        }))
    }

    /// 캐시한 경로로 읽지 못하면(세션과 어긋나거나 파일이 지워짐) 새 경로를 한 번만 다시 찾는다.
    /// 지워진 파일이 아닌 읽기 실패(권한 등)는 그대로 실패로 올린다
    pub fn get_mobile_ai_session_detail(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
        options: &MobileAiHistoryOptions,
    ) -> MobileResult<Option<Value>> {
        let key = mobile_ai_session_key(task_id, provider, session_id);
        let had_cached_source = self.ai_sources().get(&key).is_some();
        if !had_cached_source {
            self.refresh_mobile_ai_source(task_id, provider, session_id)?;
        }
        let Some(source_ref) = self.ai_sources().get(&key) else {
            return Ok(None);
        };
        let first = self.read_ai_session_detail(task_id, provider, session_id, source_ref, options);
        let detail = match first {
            Ok(detail) => detail,
            Err(AiHistoryReadError::SourceMissing(_)) => None,
            Err(AiHistoryReadError::Failed(message)) => return Err(MobileError::Internal(message)),
        };
        if detail.is_some() {
            return Ok(detail);
        }
        self.ai_sources().forget(&key);
        if !had_cached_source {
            return Ok(None);
        }
        self.retry_ai_session_detail(task_id, provider, session_id, options)
    }

    fn retry_ai_session_detail(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
        options: &MobileAiHistoryOptions,
    ) -> MobileResult<Option<Value>> {
        let key = mobile_ai_session_key(task_id, provider, session_id);
        self.refresh_mobile_ai_source(task_id, provider, session_id)?;
        let Some(source_ref) = self.ai_sources().get(&key) else {
            return Ok(None);
        };
        let detail = self
            .read_ai_session_detail(task_id, provider, session_id, source_ref, options)
            .map_err(|error| match error {
                AiHistoryReadError::SourceMissing(message)
                | AiHistoryReadError::Failed(message) => MobileError::Internal(message),
            })?;
        if detail.is_none() {
            self.ai_sources().forget(&key);
        }
        Ok(detail)
    }

    /// 첨부는 현재 태스크에서 발견한 기록에서만 읽는다. 내부 파일 오류는 경로가 섞여 있어 고정 문구로 바꾼다
    pub fn get_mobile_ai_session_attachment(
        &self,
        task_id: &str,
        provider: AiSessionProvider,
        session_id: &str,
        attachment_id: &str,
        source_ref: Option<&str>,
    ) -> MobileResult<Option<AiSessionAttachmentContent>> {
        let key = mobile_ai_session_key(task_id, provider, session_id);
        let resolved_source = match source_ref {
            Some(source_ref) => Some(source_ref.to_string()),
            None => {
                if self.ai_sources().get(&key).is_none() {
                    self.refresh_mobile_ai_source(task_id, provider, session_id)?;
                }
                let Some(cached) = self.ai_sources().get(&key) else {
                    return Ok(None);
                };
                cached
            }
        };
        let request = AiSessionAttachmentRequest {
            provider,
            session_id: session_id.to_string(),
            attachment_id: attachment_id.to_string(),
            source_ref: resolved_source,
        };
        self.projects
            .get_task_ai_session_attachment(task_id, &request)
            .map_err(|_| MobileError::task_input(ATTACHMENT_FAILURE_MESSAGE))
    }

    /// 태스크 확인과 경로 검증은 diff 서비스가 맡고, 실패 사유는 그대로 라우트까지 올린다
    pub fn get_mobile_task_file(
        &self,
        task_id: &str,
        file_path: &str,
    ) -> MobileResult<TaskWorkspaceFile> {
        self.diff
            .read_task_workspace_file(task_id, file_path)
            .map_err(MobileError::TaskWorkspaceFile)
    }
}
