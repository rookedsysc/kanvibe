//! GitHub 릴리스에서 더 새 버전을 찾는다(TS `releaseUpdateService.ts`). 자동 업데이트는 없고 화면이 배너만 띄운다.
//! 성공한 확인 결과는 1시간 재사용하고, 동시에 들어온 확인은 진행 중인 요청 하나를 함께 기다린다.
//! 창이 많아도 한 버전의 배너는 처음 요청한 창 하나만 띄우도록 `claim_release_update_version`으로 나눠 갖는다.

use std::collections::HashSet;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::release_updates::{
    ReleaseVersion, compare_release_versions, is_release_version_greater_than,
    parse_release_version,
};
use serde::Serialize;
use serde_json::Value;

const KANVIBE_RELEASES_API_URL: &str = "https://api.github.com/repos/rookedsysc/kanvibe/releases";
const DEVELOPMENT_APP_VERSION: &str = "0.0.0";
const RELEASE_UPDATE_CHECK_CACHE: Duration = Duration::from_secs(60 * 60);

/// 릴리스 목록 요청. 응답 본문 JSON을 그대로 돌려주고, 실패는 사용자에게 보일 메시지로 돌려준다
pub trait ReleaseFeedClient: Send + Sync {
    fn fetch_releases(&self) -> Result<Value, String>;
}

pub struct GitHubReleaseFeedClient;

impl ReleaseFeedClient for GitHubReleaseFeedClient {
    fn fetch_releases(&self) -> Result<Value, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent
            .get(KANVIBE_RELEASES_API_URL)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "KanVibe")
            .call()
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!(
                "GitHub releases request failed with status {}",
                response.status().as_u16()
            ));
        }
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        serde_json::from_str(&body).map_err(|error| error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseUpdate {
    pub version: String,
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub html_url: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseUpdateCheckResult {
    pub current_version: String,
    pub is_update_available: bool,
    pub release: Option<ReleaseUpdate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ReleaseUpdateCheckResult {
    fn without_release(&self) -> Self {
        Self {
            is_update_available: false,
            release: None,
            ..self.clone()
        }
    }
}

fn trimmed_string_field(release: &Value, field: &str) -> String {
    release
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// 초안·사전 릴리스, 버전 태그나 주소가 없는 항목은 업데이트 후보가 아니다
fn to_release_update(release: &Value) -> Option<ReleaseUpdate> {
    let is_flagged = |field: &str| release.get(field) == Some(&Value::Bool(true));
    if is_flagged("draft") || is_flagged("prerelease") {
        return None;
    }
    let tag_name = trimmed_string_field(release, "tag_name");
    let html_url = trimmed_string_field(release, "html_url");
    parse_release_version(&tag_name)?;
    if html_url.is_empty() {
        return None;
    }
    let name = trimmed_string_field(release, "name");
    let published_at = trimmed_string_field(release, "published_at");
    Some(ReleaseUpdate {
        version: tag_name.strip_prefix('v').unwrap_or(&tag_name).to_string(),
        name: if name.is_empty() {
            tag_name.clone()
        } else {
            name
        },
        body: trimmed_string_field(release, "body"),
        html_url,
        published_at: (!published_at.is_empty()).then_some(published_at),
        tag_name,
    })
}

/// 지금 버전보다 큰 릴리스 중 가장 높은 버전. 같은 버전이 여럿이면 먼저 나온 것을 고른다
pub fn select_latest_release_update(
    releases: &[Value],
    current_version: &str,
) -> Option<ReleaseUpdate> {
    let mut comparable: Vec<(ReleaseVersion, ReleaseUpdate)> = releases
        .iter()
        .filter_map(to_release_update)
        .filter(|release| is_release_version_greater_than(&release.tag_name, current_version))
        .filter_map(|release| Some((parse_release_version(&release.tag_name)?, release)))
        .collect();
    comparable.sort_by(|left, right| compare_release_versions(right.0, left.0));
    comparable.into_iter().next().map(|(_, release)| release)
}

/// 진행 중인 확인 하나. 먼저 온 호출이 요청을 보내고 뒤에 온 호출은 결과를 기다린다
#[derive(Default)]
struct InFlightCheck {
    result: Mutex<Option<ReleaseUpdateCheckResult>>,
    is_ready: Condvar,
}

impl InFlightCheck {
    fn publish(&self, result: ReleaseUpdateCheckResult) {
        *self.result.lock().unwrap() = Some(result);
        self.is_ready.notify_all();
    }

    fn wait(&self) -> ReleaseUpdateCheckResult {
        let guard = self.result.lock().unwrap();
        let guard = self
            .is_ready
            .wait_while(guard, |result| result.is_none())
            .unwrap();
        guard.clone().expect("기다림이 끝나면 결과가 있다")
    }
}

#[derive(Default)]
struct CheckState {
    cached: Option<(Instant, ReleaseUpdateCheckResult)>,
    in_flight: Option<Arc<InFlightCheck>>,
    shown_versions: HashSet<String>,
}

pub struct ReleaseUpdateService {
    client: Arc<dyn ReleaseFeedClient>,
    current_version: String,
    clock: Box<dyn Fn() -> Instant + Send + Sync>,
    state: Mutex<CheckState>,
}

impl ReleaseUpdateService {
    /// `is_development_run`은 TS가 `KANVIBE_RENDERER_URL`로 판별하던 개발 실행 여부다. 개발 실행은 늘 0.0.0으로 본다
    pub fn new(
        client: Arc<dyn ReleaseFeedClient>,
        app_version: &str,
        is_development_run: bool,
    ) -> Self {
        Self::with_clock(client, app_version, is_development_run, Instant::now)
    }

    pub fn with_clock(
        client: Arc<dyn ReleaseFeedClient>,
        app_version: &str,
        is_development_run: bool,
        clock: impl Fn() -> Instant + Send + Sync + 'static,
    ) -> Self {
        let current_version = if is_development_run {
            DEVELOPMENT_APP_VERSION
        } else {
            app_version
        };
        Self {
            client,
            current_version: current_version.to_string(),
            clock: Box::new(clock),
            state: Mutex::new(CheckState::default()),
        }
    }

    pub fn current_release_version(&self) -> &str {
        &self.current_version
    }

    /// 이 버전의 배너를 띄울 창을 정한다. 처음 요청한 창만 `true`를 받는다
    pub fn claim_release_update_version(&self, version: &str) -> bool {
        if version.is_empty() {
            return false;
        }
        self.state
            .lock()
            .unwrap()
            .shown_versions
            .insert(version.to_string())
    }

    pub fn check_for_release_update(&self) -> ReleaseUpdateCheckResult {
        let (in_flight, is_leader) = match self.reuse_cached_or_join() {
            Ok(cached) => return self.hide_shown_release(cached),
            Err(joined) => joined,
        };
        if !is_leader {
            return self.hide_shown_release(in_flight.wait());
        }
        let result = self.create_release_update_check();
        self.finish_check(&result);
        in_flight.publish(result.clone());
        self.hide_shown_release(result)
    }

    /// 1시간 안의 결과가 있으면 돌려주고, 없으면 진행 중인 확인에 합류하거나 새로 시작한다
    fn reuse_cached_or_join(&self) -> Result<ReleaseUpdateCheckResult, (Arc<InFlightCheck>, bool)> {
        let now = (self.clock)();
        let mut state = self.state.lock().unwrap();
        match &state.cached {
            Some((checked_at, result))
                if now.duration_since(*checked_at) < RELEASE_UPDATE_CHECK_CACHE =>
            {
                return Ok(result.clone());
            }
            Some(_) => state.cached = None,
            None => {}
        }
        if let Some(in_flight) = &state.in_flight {
            return Err((Arc::clone(in_flight), false));
        }
        let in_flight = Arc::new(InFlightCheck::default());
        state.in_flight = Some(Arc::clone(&in_flight));
        Err((in_flight, true))
    }

    /// 실패한 확인은 기억하지 않아 다음 요청이 다시 시도한다
    fn finish_check(&self, result: &ReleaseUpdateCheckResult) {
        let checked_at = (self.clock)();
        let mut state = self.state.lock().unwrap();
        if result.error.is_none() {
            state.cached = Some((checked_at, result.clone()));
        }
        state.in_flight = None;
    }

    fn create_release_update_check(&self) -> ReleaseUpdateCheckResult {
        let current_version = self.current_version.clone();
        let releases = self.client.fetch_releases().and_then(|body| match body {
            Value::Array(releases) => Ok(releases),
            _ => Err("GitHub releases response was not an array".to_string()),
        });
        match releases {
            Ok(releases) => {
                let release = select_latest_release_update(&releases, &current_version);
                ReleaseUpdateCheckResult {
                    current_version,
                    is_update_available: release.is_some(),
                    release,
                    error: None,
                }
            }
            Err(error) => ReleaseUpdateCheckResult {
                current_version,
                is_update_available: false,
                release: None,
                error: Some(error),
            },
        }
    }

    fn hide_shown_release(&self, result: ReleaseUpdateCheckResult) -> ReleaseUpdateCheckResult {
        let Some(release) = result
            .release
            .as_ref()
            .filter(|_| result.is_update_available)
        else {
            return result;
        };
        if self
            .state
            .lock()
            .unwrap()
            .shown_versions
            .contains(&release.version)
        {
            return result.without_release();
        }
        result
    }
}

#[cfg(test)]
mod tests;
