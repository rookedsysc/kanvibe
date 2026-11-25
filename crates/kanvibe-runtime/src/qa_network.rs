//! QA 모드의 바깥 HTTP 응답 표(`parity/fixtures/network/*.json`). 표가 주어지면 AI 사용량 조회, 릴리스 확인,
//! 프로젝트 아이콘 내려받기가 실제 네트워크 대신 이 표로 답하고, 표에 없는 주소는 즉시 실패한다.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kanvibe_ai::usage::context::{HttpRequest, HttpResponse, HttpTransport};
use kanvibe_process::github_project_icon::{IconHttpClient, IconHttpResponse};
use kanvibe_services::release_update::{KANVIBE_RELEASES_API_URL, ReleaseFeedClient};
use serde::Deserialize;
use serde_json::Value;

/// 고정 응답 한 줄. `urlPrefix`로 요청 주소 앞부분을 맞춘다
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NetworkResponse {
    #[serde(rename = "urlPrefix")]
    pub url_prefix: String,
    pub status: u16,
    #[serde(default)]
    pub body: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct NetworkResponseTable {
    pub responses: Vec<NetworkResponse>,
}

#[derive(Debug, thiserror::Error)]
pub enum NetworkTableLoadError {
    #[error("네트워크 응답 표를 읽지 못했습니다: {0}")]
    Read(#[from] std::io::Error),
    #[error("네트워크 응답 표 형식이 잘못됐습니다: {0}")]
    Parse(#[from] serde_json::Error),
}

impl NetworkResponseTable {
    pub fn load_file(path: &Path) -> Result<Self, NetworkTableLoadError> {
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn find(&self, url: &str) -> Option<&NetworkResponse> {
        self.responses
            .iter()
            .find(|response| url.starts_with(&response.url_prefix))
    }

    fn find_or_block(&self, url: &str) -> Result<&NetworkResponse, String> {
        self.find(url)
            .ok_or_else(|| format!("QA 모드에서 응답 표에 없는 주소입니다: {url}"))
    }
}

/// 응답 본문 글자. 문자열 값은 그대로, 나머지는 JSON으로 쓴다
fn body_text(body: &Value) -> String {
    match body {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// AI 사용량·계정 조회의 HTTP
pub struct TableHttpTransport(pub Arc<NetworkResponseTable>);

impl HttpTransport for TableHttpTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let response = self.0.find_or_block(&request.url)?;
        Ok(HttpResponse {
            status: response.status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body_text(&response.body),
        })
    }
}

/// 릴리스 확인. 실패 문구는 실제 GitHub 클라이언트와 같다
pub struct TableReleaseFeedClient(pub Arc<NetworkResponseTable>);

impl ReleaseFeedClient for TableReleaseFeedClient {
    fn fetch_releases(&self) -> Result<Value, String> {
        let response = self.0.find_or_block(KANVIBE_RELEASES_API_URL)?;
        if !(200..=299).contains(&response.status) {
            return Err(format!(
                "GitHub releases request failed with status {}",
                response.status
            ));
        }
        Ok(response.body.clone())
    }
}

/// 프로젝트 아이콘(GitHub 아바타) 내려받기
pub struct TableIconHttpClient(pub Arc<NetworkResponseTable>);

impl IconHttpClient for TableIconHttpClient {
    fn get(&self, url: &str, _timeout: Duration) -> Result<IconHttpResponse, String> {
        let response = self.0.find_or_block(url)?;
        Ok(IconHttpResponse {
            is_success: (200..=299).contains(&response.status),
            content_type: "application/json".into(),
            body: body_text(&response.body).into_bytes(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Arc<NetworkResponseTable> {
        Arc::new(
            serde_json::from_str(
                r#"{"responses":[{"urlPrefix":"https://api.github.com/repos/rookedsysc/kanvibe/releases","status":200,"body":[]}]}"#,
            )
            .unwrap(),
        )
    }

    #[test]
    fn table_matches_by_url_prefix_and_rejects_unknown_urls() {
        let table = table();
        assert_eq!(
            table
                .find("https://api.github.com/repos/rookedsysc/kanvibe/releases?per_page=1")
                .map(|response| response.status),
            Some(200)
        );
        assert!(table.find("https://example.com").is_none());
    }

    #[test]
    fn release_feed_and_http_transport_answer_from_the_table_only() {
        assert_eq!(
            TableReleaseFeedClient(table()).fetch_releases(),
            Ok(serde_json::json!([]))
        );
        let blocked = TableHttpTransport(table()).send(&HttpRequest {
            method: "GET".into(),
            url: "https://api.anthropic.com/api/oauth/usage".into(),
            headers: Vec::new(),
            body: None,
            timeout_ms: 1000,
        });
        assert!(blocked.unwrap_err().contains("api.anthropic.com"));
    }
}
