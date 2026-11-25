//! 프로젝트 저장소의 GitHub owner 아바타를 data URL로 가져온다(TS `githubProjectIcon.ts`).
//! GitHub 저장소가 아니거나 아이콘을 받지 못하면 아이콘 없이 등록되게 `None`을 돌려준다.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use regex::Regex;

use crate::exec::{ExecGitOptions, ShellCommandRunner};
use crate::host_file_access::quote_shell_argument;

/// GitHub avatar 응답 크기. 보드 카드와 목록에서 쓰는 최대 크기의 2배(retina)로 맞춘다
const ICON_PIXEL_SIZE: u32 = 64;
const ICON_FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// data URL로 DB에 저장하므로 비정상적으로 큰 응답은 저장하지 않는다
const MAX_ICON_BYTES: usize = 512 * 1024;
const ALLOWED_ICON_CONTENT_TYPES: [&str; 4] =
    ["image/png", "image/jpeg", "image/gif", "image/webp"];
/// 같은 owner의 저장소를 연달아 등록할 때 아바타를 반복해서 내려받지 않도록 캐시한다.
/// 실패도 캐시해 오프라인에서 저장소마다 타임아웃을 다시 기다리지 않게 한다.
const OWNER_ICON_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

/// GitHub 사용자·조직 이름 규칙
static GITHUB_OWNER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$").expect("유효한 정규식")
});
static GITHUB_REMOTE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:https?://|ssh://)?(?:[^@/]+@)?github\.com[:/]+([^/]+)/([^/]+?)(?:\.git)?/?$",
    )
    .expect("유효한 정규식")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRepositoryReference {
    pub owner: String,
    pub repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconHttpResponse {
    /// HTTP 2xx 응답이다
    pub is_success: bool,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// 아바타를 내려받는 HTTP GET. 네트워크 실패는 오류 메시지로 돌려준다
pub trait IconHttpClient: Send + Sync {
    fn get(&self, url: &str, timeout: Duration) -> Result<IconHttpResponse, String>;
}

/// `ureq`로 실제 요청을 보낸다. 4xx·5xx도 오류가 아니라 응답으로 받는다
pub struct UreqIconHttpClient;

impl IconHttpClient for UreqIconHttpClient {
    fn get(&self, url: &str, timeout: Duration) -> Result<IconHttpResponse, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let mut response = agent.get(url).call().map_err(|error| error.to_string())?;
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = response
            .body_mut()
            .read_to_vec()
            .map_err(|error| error.to_string())?;
        Ok(IconHttpResponse {
            is_success: response.status().is_success(),
            content_type,
            body,
        })
    }
}

/// git remote URL에서 GitHub owner/repository를 뽑는다.
/// `https://github.com/o/r.git`, `git@github.com:o/r.git`, `ssh://git@github.com/o/r` 형식을 지원한다.
pub fn parse_github_repository_reference(remote_url: &str) -> Option<GitHubRepositoryReference> {
    let captures = GITHUB_REMOTE_PATTERN.captures(remote_url.trim())?;
    let owner = captures.get(1)?.as_str();
    if !GITHUB_OWNER_PATTERN.is_match(owner) {
        return None;
    }
    Some(GitHubRepositoryReference {
        owner: owner.to_string(),
        repository: captures.get(2)?.as_str().to_string(),
    })
}

/// 저장소의 origin remote를 읽어 GitHub owner/repository를 확인한다. remote가 없는 로컬 전용 저장소는 `None`이다
pub fn resolve_github_repository_reference(
    runner: &dyn ShellCommandRunner,
    repo_path: &str,
    ssh_host: Option<&str>,
) -> Option<GitHubRepositoryReference> {
    let command = format!(
        "git -C {} config --get remote.origin.url",
        quote_shell_argument(repo_path)
    );
    let remote_url = runner
        .exec_git(&command, ssh_host, &ExecGitOptions::default())
        .ok()?;
    parse_github_repository_reference(&remote_url)
}

struct CachedOwnerIcon {
    expires_at: Instant,
    icon_data_url: Arc<OnceLock<Option<String>>>,
}

/// owner 아바타 다운로드와 그 캐시. 동시에 같은 owner를 물어도 한 번만 내려받는다
pub struct GitHubProjectIconResolver {
    http_client: Box<dyn IconHttpClient>,
    owner_icons: Mutex<HashMap<String, CachedOwnerIcon>>,
}

impl GitHubProjectIconResolver {
    pub fn new(http_client: Box<dyn IconHttpClient>) -> Self {
        Self {
            http_client,
            owner_icons: Mutex::new(HashMap::new()),
        }
    }

    pub fn clear_owner_icon_cache(&self) {
        self.owner_icons.lock().unwrap().clear();
    }

    /// 캐시가 살아 있으면 그 값(진행 중인 다운로드 포함)을, 아니면 새 자리를 만들어 돌려준다
    fn owner_icon_slot(&self, owner: &str) -> Arc<OnceLock<Option<String>>> {
        let mut owner_icons = self.owner_icons.lock().unwrap();
        let now = Instant::now();
        if let Some(cached) = owner_icons
            .get(owner)
            .filter(|cached| cached.expires_at > now)
        {
            return Arc::clone(&cached.icon_data_url);
        }
        let slot = Arc::new(OnceLock::new());
        owner_icons.insert(
            owner.to_string(),
            CachedOwnerIcon {
                expires_at: now + OWNER_ICON_CACHE_TTL,
                icon_data_url: Arc::clone(&slot),
            },
        );
        slot
    }

    /// GitHub 사용자·조직 아바타를 data URL로 바꾼다. 네트워크 실패, 비정상 응답, 과도한 크기는 모두 아이콘 없음이다
    pub fn fetch_owner_icon_data_url(&self, owner: &str) -> Option<String> {
        if !GITHUB_OWNER_PATTERN.is_match(owner) {
            return None;
        }
        let slot = self.owner_icon_slot(owner);
        slot.get_or_init(|| self.download_owner_icon_data_url(owner))
            .clone()
    }

    fn download_owner_icon_data_url(&self, owner: &str) -> Option<String> {
        let url = format!("https://github.com/{owner}.png?size={ICON_PIXEL_SIZE}");
        let response = match self.http_client.get(&url, ICON_FETCH_TIMEOUT) {
            Ok(response) => response,
            Err(error) => {
                eprintln!("[project-icon] GitHub 아이콘 다운로드 실패 owner={owner} error={error}");
                return None;
            }
        };
        if !response.is_success {
            return None;
        }
        let content_type = response
            .content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let is_allowed_type = ALLOWED_ICON_CONTENT_TYPES.contains(&content_type.as_str());
        if !is_allowed_type || response.body.is_empty() || response.body.len() > MAX_ICON_BYTES {
            return None;
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(&response.body);
        Some(format!("data:{content_type};base64,{encoded}"))
    }

    /// 프로젝트 저장소의 GitHub 아이콘을 확보한다
    pub fn resolve_project_icon_data_url(
        &self,
        runner: &dyn ShellCommandRunner,
        repo_path: &str,
        ssh_host: Option<&str>,
    ) -> Option<String> {
        let repository_reference =
            resolve_github_repository_reference(runner, repo_path, ssh_host)?;
        self.fetch_owner_icon_data_url(&repository_reference.owner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::exec::ExecGitError;
    use crate::exec::testing::ScriptedCommandRunner;

    struct FakeIconHttpClient {
        response: Result<IconHttpResponse, String>,
        requested_urls: Arc<Mutex<Vec<String>>>,
        request_count: Arc<AtomicUsize>,
    }

    impl IconHttpClient for FakeIconHttpClient {
        fn get(&self, url: &str, _timeout: Duration) -> Result<IconHttpResponse, String> {
            self.request_count.fetch_add(1, Ordering::SeqCst);
            self.requested_urls.lock().unwrap().push(url.to_string());
            self.response.clone()
        }
    }

    struct Fixture {
        resolver: GitHubProjectIconResolver,
        requested_urls: Arc<Mutex<Vec<String>>>,
        request_count: Arc<AtomicUsize>,
    }

    fn fixture(response: Result<IconHttpResponse, String>) -> Fixture {
        let requested_urls = Arc::new(Mutex::new(Vec::new()));
        let request_count = Arc::new(AtomicUsize::new(0));
        let client = FakeIconHttpClient {
            response,
            requested_urls: Arc::clone(&requested_urls),
            request_count: Arc::clone(&request_count),
        };
        Fixture {
            resolver: GitHubProjectIconResolver::new(Box::new(client)),
            requested_urls,
            request_count,
        }
    }

    fn icon_response(content_type: &str, body: &[u8]) -> Result<IconHttpResponse, String> {
        Ok(IconHttpResponse {
            is_success: true,
            content_type: content_type.into(),
            body: body.to_vec(),
        })
    }

    fn reference(owner: &str, repository: &str) -> Option<GitHubRepositoryReference> {
        Some(GitHubRepositoryReference {
            owner: owner.into(),
            repository: repository.into(),
        })
    }

    fn data_url(body: &[u8]) -> Option<String> {
        Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(body)
        ))
    }

    // parity: TS-lib-githubProjectIcon-f84989
    #[test]
    fn reads_owner_and_repository_from_github_remote_formats() {
        for remote_url in [
            "https://github.com/rookedsysc/kanvibe.git",
            "git@github.com:rookedsysc/kanvibe.git\n",
            "ssh://git@github.com/rookedsysc/kanvibe",
            "https://github.com/rookedsysc/kanvibe/",
        ] {
            assert_eq!(
                parse_github_repository_reference(remote_url),
                reference("rookedsysc", "kanvibe")
            );
        }
    }

    // parity: TS-lib-githubProjectIcon-165e42
    #[test]
    fn non_github_or_invalid_owner_remotes_have_no_icon() {
        for remote_url in [
            "https://gitlab.com/team/app.git",
            "",
            "https://github.com/-bad-owner/app.git",
            "https://github.com/own er/app.git",
        ] {
            assert_eq!(parse_github_repository_reference(remote_url), None);
        }
    }

    // parity: TS-lib-githubProjectIcon-ca5211
    #[test]
    fn converts_the_avatar_to_a_data_url() {
        let icon_bytes = [137, 80, 78, 71];
        let fixture = fixture(icon_response("image/png", &icon_bytes));

        assert_eq!(
            fixture.resolver.fetch_owner_icon_data_url("rookedsysc"),
            data_url(&icon_bytes)
        );
        assert!(
            fixture.requested_urls.lock().unwrap()[0].contains("https://github.com/rookedsysc.png")
        );
    }

    // parity: TS-lib-githubProjectIcon-2b232d
    #[test]
    fn non_image_responses_and_network_failures_have_no_icon() {
        let html = fixture(icon_response("text/html", &[1]));
        assert_eq!(html.resolver.fetch_owner_icon_data_url("rookedsysc"), None);

        let offline = fixture(Err("offline".into()));
        assert_eq!(
            offline.resolver.fetch_owner_icon_data_url("rookedsysc"),
            None
        );
    }

    // parity: TS-lib-githubProjectIcon-0dc726
    #[test]
    fn repository_without_remote_registers_without_an_icon() {
        let fixture = fixture(icon_response("image/png", &[1]));
        let runner = ScriptedCommandRunner::new(|_| Err(ExecGitError::new("no origin")));

        assert_eq!(
            fixture
                .resolver
                .resolve_project_icon_data_url(&runner, "/repo", None),
            None
        );
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 0);
    }

    // parity: TS-lib-githubProjectIcon-3cdc98
    #[test]
    fn reads_the_origin_remote_to_get_the_icon() {
        let icon_bytes = [1, 2, 3];
        let fixture = fixture(icon_response("image/png", &icon_bytes));
        let runner = ScriptedCommandRunner::returning("git@github.com:rookedsysc/kanvibe.git\n");

        let icon = fixture.resolver.resolve_project_icon_data_url(
            &runner,
            "/remote/repo",
            Some("remote-host"),
        );

        assert_eq!(icon, data_url(&icon_bytes));
        let call = &runner.calls()[0];
        assert_eq!(
            call.command,
            "git -C '/remote/repo' config --get remote.origin.url"
        );
        assert_eq!(call.ssh_host.as_deref(), Some("remote-host"));
    }

    // parity: TS-lib-githubProjectIcon-dcf8a1
    #[test]
    fn downloads_an_owner_avatar_only_once() {
        let fixture = fixture(icon_response("image/png", &[1, 2, 3]));

        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| fixture.resolver.fetch_owner_icon_data_url("shared-owner"));
            let second = scope.spawn(|| fixture.resolver.fetch_owner_icon_data_url("shared-owner"));
            (first.join().unwrap(), second.join().unwrap())
        });
        fixture.resolver.fetch_owner_icon_data_url("shared-owner");

        assert_eq!(first, second);
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
    }

    // parity: TS-lib-githubProjectIcon-9c1825
    #[test]
    fn failures_are_cached_per_owner_too() {
        let fixture = fixture(Err("offline".into()));

        assert_eq!(
            fixture.resolver.fetch_owner_icon_data_url("offline-owner"),
            None
        );
        assert_eq!(
            fixture.resolver.fetch_owner_icon_data_url("offline-owner"),
            None
        );

        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn clearing_the_cache_downloads_again() {
        let fixture = fixture(Err("offline".into()));

        fixture.resolver.fetch_owner_icon_data_url("owner");
        fixture.resolver.clear_owner_icon_cache();
        fixture.resolver.fetch_owner_icon_data_url("owner");

        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2);
    }
}
