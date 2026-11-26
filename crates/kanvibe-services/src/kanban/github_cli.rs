//! task 브랜치의 PR을 `gh pr list`로 조회한다. 등록 경로가 저장소의 부모면 유일한 하위 저장소로 내려가고,
//! `.envrc`가 있으면 direnv 환경으로 gh를 부른다. gh가 저장소를 판별하지 못하면 origin remote로 다시 조회한다.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use kanvibe_domain::project::Project;
use kanvibe_domain::task::KanbanTask;
use kanvibe_process::exec::{ExecGitError, ExecGitOptions, ShellCommandRunner};
use kanvibe_process::github_project_icon::parse_github_repository_reference;
use regex::Regex;

/// 저장소 경로가 사라졌을 때 셸이 stdout으로 내보내는 표식.
/// 경로 없음은 재시도해도 회복되지 않아 sync 실패로 올리지 않고 조용히 건너뛴다
pub(super) const MISSING_REPOSITORY_MARKER: &str = "kanvibe-missing-repository";
const GITHUB_CLI_TIMEOUT: Duration = Duration::from_millis(10_000);
/// gh 인증은 사용자가 그 호스트에서 직접 고쳐야 풀린다. 그 전까지 같은 저장소의 자동 조회는 기억한 실패를 그대로 쓴다
const GITHUB_CLI_AUTH_FAILURE_COOLDOWN: Duration = Duration::from_secs(5 * 60);

static MISSING_GITHUB_CLI_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)(?:^|\s)gh:.*not found").expect("유효한 정규식"),
        Regex::new(r"(?i)command not found.*\bgh\b").expect("유효한 정규식"),
    ]
});
static AUTHENTICATION_ERROR_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)gh auth login|GH_TOKEN|HTTP 401|Bad credentials").expect("유효한 정규식")
});
static UNRESOLVED_REPOSITORY_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)Could not resolve to a Repository|Could not determine base repository")
        .expect("유효한 정규식")
});

/// 작은따옴표 셸 인용(TS kanbanService `quoteForShell`)
pub(super) fn quote_for_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// gh와 Git fallback이 같은 디렉터리와 direnv 환경을 쓰도록 저장소 안에서 명령을 실행한다
pub(super) fn build_repository_command(repo_path: &str, command: &str) -> String {
    [
        format!("repo={}", quote_for_shell(repo_path)),
        r#"if [ ! -e "$repo/.git" ]; then nested=""; nestedCount=0; for candidate in "$repo"/*/; do if [ -e "$candidate.git" ]; then nested="${candidate%/}"; nestedCount=$((nestedCount + 1)); fi; done; if [ "$nestedCount" -eq 1 ]; then repo="$nested"; fi; fi"#.to_string(),
        format!(
            r#"cd "$repo" || {{ printf %s {}; exit 0; }}"#,
            quote_for_shell(MISSING_REPOSITORY_MARKER)
        ),
        r#"envrcDirectory=""; directory="$PWD"; while :; do if [ -f "$directory/.envrc" ]; then envrcDirectory="$directory"; break; fi; if [ "$directory" = "/" ]; then break; fi; directory=$(dirname "$directory"); done"#.to_string(),
        format!(
            r#"if [ -n "$envrcDirectory" ] && command -v direnv >/dev/null; then direnv exec "$repo" {command} || {command}; else {command}; fi"#
        ),
    ]
    .join("; ")
}

pub(super) fn is_missing_repository_output(output: &str) -> bool {
    output.trim() == MISSING_REPOSITORY_MARKER
}

fn is_missing_github_cli(error: &ExecGitError) -> bool {
    error.error_code == Some("ENOENT")
        || MISSING_GITHUB_CLI_PATTERNS
            .iter()
            .any(|pattern| pattern.is_match(&error.message))
}

/// 로컬만 명령 자체에 제한 시간을 걸고, 원격은 SSH 계층의 제한 시간을 그대로 따른다
pub(super) fn github_cli_exec_options(ssh_host: Option<&str>) -> ExecGitOptions {
    ExecGitOptions {
        timeout: ssh_host.is_none().then_some(GITHUB_CLI_TIMEOUT),
        ..ExecGitOptions::default()
    }
}

/// 조회할 저장소 경로와 호스트. task worktree가 프로젝트 경로보다 먼저다
pub(super) fn resolve_task_git_context(
    task: &KanbanTask,
    project: Option<&Project>,
) -> (String, Option<String>) {
    let task_ssh_host = task.ssh_host.clone().filter(|host| !host.is_empty());
    let ssh_host = task_ssh_host.or_else(|| project.and_then(|project| project.ssh_host.clone()));
    // 둘 다 없으면 TS `process.cwd()`처럼 앱이 실행된 디렉터리에서 조회한다
    let cwd = task
        .worktree_path
        .clone()
        .or_else(|| project.map(|project| project.repo_path.clone()))
        .unwrap_or_else(|| ".".to_string());
    (cwd, ssh_host)
}

/// 최근 gh 인증 실패. 인증은 `.envrc`로 경로마다 다를 수 있어 호스트와 저장소 경로를 함께 키로 쓴다
#[derive(Default)]
pub(super) struct GitHubCliAuthFailures {
    failures: Mutex<HashMap<String, (Instant, ExecGitError)>>,
}

fn auth_failure_key(cwd: &str, ssh_host: Option<&str>) -> String {
    serde_json::json!([ssh_host, cwd]).to_string()
}

impl GitHubCliAuthFailures {
    /// 이 저장소에서 쿨다운 안에 기억한 인증 실패
    pub(super) fn find_recent(
        &self,
        cwd: &str,
        ssh_host: Option<&str>,
        now: Instant,
    ) -> Option<ExecGitError> {
        let key = auth_failure_key(cwd, ssh_host);
        let mut failures = self.failures.lock().unwrap();
        let (until, error) = failures.get(&key)?;
        if now >= *until {
            failures.remove(&key);
            return None;
        }
        Some(error.clone())
    }

    fn record(
        &self,
        cwd: &str,
        ssh_host: Option<&str>,
        result: &Result<String, ExecGitError>,
        now: Instant,
    ) {
        let key = auth_failure_key(cwd, ssh_host);
        let mut failures = self.failures.lock().unwrap();
        match result {
            Ok(_) => {
                failures.remove(&key);
            }
            Err(error) if AUTHENTICATION_ERROR_PATTERN.is_match(&error.message) => {
                failures.insert(key, (now + GITHUB_CLI_AUTH_FAILURE_COOLDOWN, error.clone()));
            }
            Err(_) => {}
        }
    }
}

/// 한 저장소의 gh PR 조회. 시계는 인증 실패 기억에만 쓴다
pub(super) struct GitHubCliQuery<'a> {
    pub runner: &'a dyn ShellCommandRunner,
    pub auth_failures: &'a GitHubCliAuthFailures,
    pub now: &'a dyn Fn() -> Instant,
    pub cwd: &'a str,
    pub ssh_host: Option<&'a str>,
}

impl GitHubCliQuery<'_> {
    fn exec(&self, command: &str) -> Result<String, ExecGitError> {
        let options = github_cli_exec_options(self.ssh_host);
        self.runner.exec_git(
            &build_repository_command(self.cwd, command),
            self.ssh_host,
            &options,
        )
    }

    /// gh가 저장소를 판별하지 못하면 origin remote의 GitHub 저장소를 명시해 다시 조회한다
    fn query_with_repository_fallback(&self, gh_arguments: &str) -> Result<String, ExecGitError> {
        let error = match self.exec(&format!("gh {gh_arguments}")) {
            Ok(output) => return Ok(output),
            Err(error) => error,
        };
        if !UNRESOLVED_REPOSITORY_PATTERN.is_match(&error.message) {
            return Err(error);
        }
        let Ok(remote_url) = self.exec("git remote get-url origin") else {
            return Err(error);
        };
        let Some(repository) = parse_github_repository_reference(&remote_url) else {
            return Err(error);
        };
        let reference = format!("{}/{}", repository.owner, repository.repository);
        self.exec(&format!(
            "gh {gh_arguments} --repo {}",
            quote_for_shell(&reference)
        ))
    }

    /// 인증 실패를 저장소 단위로 기억한다. `should_reuse_recent_auth_failure`면 기억한 실패를 명령 없이 돌려준다
    fn query_pull_requests(
        &self,
        gh_arguments: &str,
        should_reuse_recent_auth_failure: bool,
    ) -> Result<String, ExecGitError> {
        if should_reuse_recent_auth_failure
            && let Some(error) =
                self.auth_failures
                    .find_recent(self.cwd, self.ssh_host, (self.now)())
        {
            return Err(error);
        }
        let result = self.query_with_repository_fallback(gh_arguments);
        self.auth_failures
            .record(self.cwd, self.ssh_host, &result, (self.now)());
        result
    }

    /// 브랜치의 첫 PR URL. PR이 없거나 gh가 없으면 `None`이다
    pub(super) fn pr_url(&self, branch_name: &str) -> Result<Option<String>, ExecGitError> {
        let gh_arguments = format!(
            "pr list --head {} --json url -q '.[0].url'",
            quote_for_shell(branch_name)
        );
        match self.query_pull_requests(&gh_arguments, false) {
            Ok(output) if is_missing_repository_output(&output) => Ok(None),
            Ok(output) => Ok(Some(output.trim().to_string()).filter(|url| !url.is_empty())),
            Err(error) if is_missing_github_cli(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// 브랜치의 가장 최근 PR 상태. 백그라운드 동기화는 같은 저장소의 task마다 부르므로 첫 인증 실패를 나머지가 함께 쓴다
    pub(super) fn pr_status(
        &self,
        branch_name: &str,
    ) -> Result<Option<PullRequestStatus>, ExecGitError> {
        let gh_arguments = format!(
            "pr list --head {} --state all --json url,state,mergedAt,updatedAt",
            quote_for_shell(branch_name)
        );
        match self.query_pull_requests(&gh_arguments, true) {
            Ok(output) if output.trim().is_empty() || is_missing_repository_output(&output) => {
                Ok(None)
            }
            Ok(output) => parse_pull_request_status(&output),
            Err(error) if is_missing_github_cli(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PullRequestStatus {
    pub url: Option<String>,
    pub state: Option<String>,
    pub merged_at: Option<String>,
    pub updated_at: Option<String>,
}

fn updated_at_millis(status: &PullRequestStatus) -> i64 {
    status
        .updated_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map_or(0, |time| time.timestamp_millis())
}

/// 가장 최근에 수정된 PR 하나를 고른다
fn parse_pull_request_status(output: &str) -> Result<Option<PullRequestStatus>, ExecGitError> {
    let mut statuses: Vec<PullRequestStatus> =
        serde_json::from_str(output).map_err(|error| ExecGitError::new(error.to_string()))?;
    statuses.sort_by_key(|status| std::cmp::Reverse(updated_at_millis(status)));
    Ok(statuses.into_iter().next())
}
