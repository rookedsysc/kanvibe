//! QA 모드(`kanvibe --qa`). 스크린샷 재생 드라이버가 TS 드라이버와 같은 결정성을 얻도록 고정 시각, 고정 네트워크 응답,
//! 서비스 고장 주입, 제어 소켓을 제공한다. 모든 입력은 실행 인자로 받고 프로세스 환경은 바꾸지 않는다.

pub mod control;
pub mod faults;
pub mod options;
pub mod targets;

use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::Global;
use serde::Deserialize;
use serde_json::Value;

use crate::qa::options::QaOptions;

/// QA 모드로 실행 중일 때만 있는 전역 상태
pub struct QaRuntime {
    pub options: QaOptions,
    pub network_responses: NetworkResponseTable,
}

impl Global for QaRuntime {}

/// 앱이 보는 현재 시각. QA 모드에서는 고정 시각에서 출발해 실제 경과 시간만큼만 흐른다.
#[derive(Debug, Clone, Copy)]
pub struct AppClock {
    fixed_origin_ms: Option<i64>,
    started_at: Instant,
}

impl AppClock {
    pub fn system() -> Self {
        Self {
            fixed_origin_ms: None,
            started_at: Instant::now(),
        }
    }

    pub fn fixed(origin_ms: i64) -> Self {
        Self {
            fixed_origin_ms: Some(origin_ms),
            started_at: Instant::now(),
        }
    }

    pub fn now_ms(&self) -> i64 {
        match self.fixed_origin_ms {
            Some(origin_ms) => origin_ms + self.started_at.elapsed().as_millis() as i64,
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as i64)
                .unwrap_or_default(),
        }
    }
}

/// 고정 응답 한 줄. `urlPrefix`로 요청 주소 앞부분을 맞춘다.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NetworkResponse {
    #[serde(rename = "urlPrefix")]
    pub url_prefix: String,
    pub status: u16,
    #[serde(default)]
    pub body: Value,
}

/// 바깥 요청 응답 표. 표에 없는 주소는 QA 모드에서 즉시 실패해야 한다.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct NetworkResponseTable {
    pub responses: Vec<NetworkResponse>,
}

impl NetworkResponseTable {
    pub fn load_file(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn find(&self, url: &str) -> Option<&NetworkResponse> {
        self.responses
            .iter()
            .find(|response| url.starts_with(&response.url_prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_starts_at_the_origin_and_moves_forward() {
        let clock = AppClock::fixed(1_768_446_000_000);
        let now = clock.now_ms();
        assert!((1_768_446_000_000..1_768_446_060_000).contains(&now));
    }

    #[test]
    fn network_table_matches_by_url_prefix_and_rejects_unknown_urls() {
        let table: NetworkResponseTable = serde_json::from_str(
            r#"{"responses":[{"urlPrefix":"https://api.github.com/repos/x","status":200,"body":[]}]}"#,
        )
        .unwrap();

        assert_eq!(
            table
                .find("https://api.github.com/repos/x/releases")
                .map(|r| r.status),
            Some(200)
        );
        assert!(table.find("https://example.com").is_none());
    }
}
