//! 설치된 hook의 서버 주소가 기대값과 같은지, 그 서버에 실제로 닿는지 검사한다(TS `hookServerStatus.ts`).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;

use super::client_runtime::{HookClientRuntime, HookInstallContext};
use super::installer_jobs::SharedJobs;
use super::socket_path::build_shell_owned_hook_socket_assignment;
use crate::exec::ExecGitOptions;
use crate::host_file_access::quote_shell_argument;

/// 앱에서 쓰는 캐시 유지 시간. TS는 테스트(VITEST)에서만 0으로 두었다
pub const HOOK_SERVER_CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookServerValidation {
    pub has_expected_hook_server_url: bool,
    pub has_reachable_hook_server: bool,
    pub expected_hook_server_url: Option<String>,
    pub configured_hook_server_url: Option<String>,
}

struct CachedValue<T> {
    expires_at: Instant,
    value: T,
}

/// 기대 hook 서버 주소와 도달 여부를 잠시 기억해, 여러 provider 상태를 한꺼번에 볼 때 같은 검사를 반복하지 않는다.
/// 실패한 주소 계산은 기억하지 않는다. 캐시가 빈 채로 동시에 들어온 검사는 진행 중인 검사 하나의 결과를 함께 쓴다.
pub struct HookServerStatusCache {
    time_to_live: Duration,
    expected_urls: Mutex<HashMap<String, CachedValue<String>>>,
    reachability: Mutex<HashMap<String, CachedValue<bool>>>,
    expected_url_lookups: SharedJobs<Option<String>>,
    reachability_checks: SharedJobs<bool>,
}

impl HookServerStatusCache {
    pub fn new(time_to_live: Duration) -> Self {
        Self {
            time_to_live,
            expected_urls: Mutex::new(HashMap::new()),
            reachability: Mutex::new(HashMap::new()),
            expected_url_lookups: SharedJobs::new(),
            reachability_checks: SharedJobs::new(),
        }
    }
}

fn read_cached<T: Clone>(cache: &Mutex<HashMap<String, CachedValue<T>>>, key: &str) -> Option<T> {
    let entries = cache.lock().unwrap();
    entries
        .get(key)
        .filter(|entry| entry.expires_at > Instant::now())
        .map(|entry| entry.value.clone())
}

fn store_cached<T>(
    cache: &Mutex<HashMap<String, CachedValue<T>>>,
    key: &str,
    value: T,
    time_to_live: Duration,
) {
    let entry = CachedValue {
        expires_at: Instant::now() + time_to_live,
        value,
    };
    cache.lock().unwrap().insert(key.to_string(), entry);
}

static SHELL_HOOK_SERVER_URL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^KANVIBE_URL="([^"]+)""#).expect("유효한 정규식"));
static PLUGIN_HOOK_SERVER_URL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"const KANVIBE_URL = "([^"]+)";"#).expect("유효한 정규식"));
/// WHATWG URL에서 이 판정에 필요한 부분(scheme, host, port)만 읽는다
static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*([A-Za-z][A-Za-z0-9+.-]*):(?://)?(?:[^@/?#]*@)?(\[[^\]/?#]*\]|[^:/?#]+)(?::([0-9]*))?(?:[/?#].*)?\s*$")
        .expect("유효한 정규식")
});

pub fn extract_shell_hook_server_url(content: &str) -> Option<String> {
    SHELL_HOOK_SERVER_URL_PATTERN
        .captures(content)
        .map(|captures| captures[1].to_string())
}

pub fn extract_plugin_hook_server_url(content: &str) -> Option<String> {
    PLUGIN_HOOK_SERVER_URL_PATTERN
        .captures(content)
        .map(|captures| captures[1].to_string())
}

/// `new URL(...)`의 protocol·hostname·port. 기본 포트는 WHATWG처럼 빈 문자열이다
#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedHookServerUrl {
    protocol: String,
    hostname: String,
    port: String,
}

impl ParsedHookServerUrl {
    fn origin(&self) -> String {
        match self.port.as_str() {
            "" => format!("{}//{}", self.protocol, self.hostname),
            port => format!("{}//{}:{port}", self.protocol, self.hostname),
        }
    }
}

fn default_port(protocol: &str) -> Option<&'static str> {
    match protocol {
        "http:" | "ws:" => Some("80"),
        "https:" | "wss:" => Some("443"),
        "ftp:" => Some("21"),
        _ => None,
    }
}

fn parse_hook_server_url(url: &str) -> Option<ParsedHookServerUrl> {
    let captures = URL_PATTERN.captures(url)?;
    let protocol = format!("{}:", captures[1].to_ascii_lowercase());
    let raw_port = captures.get(3).map_or("", |port| port.as_str());
    let port = match raw_port.trim_start_matches('0') {
        "" if raw_port.is_empty() => String::new(),
        "" => "0".to_string(),
        trimmed => trimmed.to_string(),
    };
    if port.parse::<u32>().is_ok_and(|number| number > 65_535) {
        return None;
    }
    let port = if default_port(&protocol) == Some(port.as_str()) {
        String::new()
    } else {
        port
    };
    Some(ParsedHookServerUrl {
        protocol,
        hostname: captures[2].to_ascii_lowercase(),
        port,
    })
}

fn normalize_loopback_hostname(hostname: &str) -> &str {
    if hostname == "127.0.0.1" {
        "localhost"
    } else {
        hostname
    }
}

fn is_expected_hook_server_url(configured_url: &str, expected_url: &str, is_remote: bool) -> bool {
    let (Some(configured), Some(expected)) = (
        parse_hook_server_url(configured_url),
        parse_hook_server_url(expected_url),
    ) else {
        return false;
    };
    if configured.protocol != expected.protocol || configured.port != expected.port {
        return false;
    }
    is_remote
        || normalize_loopback_hostname(&configured.hostname)
            == normalize_loopback_hostname(&expected.hostname)
}

fn is_remote_hook_server_url_on_active_port(configured_url: &str, active_port: u16) -> bool {
    parse_hook_server_url(configured_url).is_some_and(|configured| {
        configured.protocol == "http:" && configured.port == active_port.to_string()
    })
}

fn build_remote_hook_health_check_command(
    health_check_url: &str,
    client_id: &str,
    namespace: Option<&str>,
) -> Option<String> {
    let client_header = match namespace {
        Some(_) => format!(
            " -H {}",
            quote_shell_argument(&format!("x-kanvibe-client-id: {client_id}"))
        ),
        None => String::new(),
    };
    let socket_assignment =
        build_shell_owned_hook_socket_assignment("__kanvibe_socket", client_id, namespace).ok()?;
    Some(
        [
            "command -v curl >/dev/null 2>&1".to_string(),
            socket_assignment,
            "[ -n \"$__kanvibe_socket\" ]".to_string(),
            format!(
                "curl --unix-socket \"$__kanvibe_socket\" -fsS --max-time 2{client_header} {} >/dev/null",
                quote_shell_argument(health_check_url)
            ),
        ]
        .join(" && "),
    )
}

/// 원격은 그 호스트에서 hook 소켓으로 health 경로를 부르고, SSH 실패나 소켓 부재는 도달 실패로 본다
fn perform_reachability_check(
    context: &HookInstallContext,
    health_check_url: &str,
    ssh_host: Option<&str>,
) -> bool {
    let Some(host) = ssh_host else {
        return context
            .runtime
            .is_local_hook_server_healthy(health_check_url);
    };
    let runtime: &dyn HookClientRuntime = context.runtime.as_ref();
    let Ok(client_id) = runtime.hook_client_id() else {
        return false;
    };
    let Ok(namespace) = runtime.remote_host_namespace(host) else {
        return false;
    };
    let Some(command) =
        build_remote_hook_health_check_command(health_check_url, &client_id, namespace.as_deref())
    else {
        return false;
    };
    let options = ExecGitOptions {
        should_check_host_key_strictly: true,
        ..ExecGitOptions::default()
    };
    context
        .files
        .runner()
        .exec_git(&command, Some(host), &options)
        .is_ok()
}

fn is_hook_server_reachable(
    context: &HookInstallContext,
    base_url: &str,
    ssh_host: Option<&str>,
) -> bool {
    let Some(parsed) = parse_hook_server_url(base_url) else {
        return false;
    };
    let health_check_url = format!("{}/api/hooks/health", parsed.origin());
    let cache_key = format!("{}:{health_check_url}", ssh_host.unwrap_or("local"));
    let cache = &context.server_status;
    if let Some(is_reachable) = read_cached(&cache.reachability, &cache_key) {
        return is_reachable;
    }
    let shared_check = cache.reachability_checks.run(&cache_key, || {
        if let Some(is_reachable) = read_cached(&cache.reachability, &cache_key) {
            return Ok(is_reachable);
        }
        let is_reachable = perform_reachability_check(context, &health_check_url, ssh_host);
        store_cached(
            &cache.reachability,
            &cache_key,
            is_reachable,
            cache.time_to_live,
        );
        Ok(is_reachable)
    });
    shared_check.unwrap_or(false)
}

fn expected_hook_server_url(
    context: &HookInstallContext,
    ssh_host: Option<&str>,
) -> Option<String> {
    let cache = &context.server_status;
    let cache_key = ssh_host.unwrap_or("local");
    if let Some(expected_url) = read_cached(&cache.expected_urls, cache_key) {
        return Some(expected_url);
    }
    let shared_lookup = cache.expected_url_lookups.run(cache_key, || {
        if let Some(expected_url) = read_cached(&cache.expected_urls, cache_key) {
            return Ok(Some(expected_url));
        }
        let Ok(expected_url) = context.runtime.hook_server_url(ssh_host) else {
            return Ok(None);
        };
        store_cached(
            &cache.expected_urls,
            cache_key,
            expected_url.clone(),
            cache.time_to_live,
        );
        Ok(Some(expected_url))
    });
    shared_lookup.ok().flatten()
}

fn unvalidated(configured_hook_server_url: Option<String>, is_valid: bool) -> HookServerValidation {
    HookServerValidation {
        has_expected_hook_server_url: is_valid,
        has_reachable_hook_server: is_valid,
        expected_hook_server_url: None,
        configured_hook_server_url,
    }
}

/// 원격 주소를 다시 계산하지 못하면, 지금 열린 hook 서버 포트를 쓰는 http 주소인지로만 판정한다
fn validate_without_expected_url(
    context: &HookInstallContext,
    defined_urls: &[&str],
    ssh_host: Option<&str>,
) -> HookServerValidation {
    let configured_hook_server_url = defined_urls.first().map(|url| url.to_string());
    let Some(host) = ssh_host else {
        return unvalidated(configured_hook_server_url, false);
    };
    let active_port = context.runtime.hook_server_port();
    let has_expected_hook_server_url = defined_urls
        .iter()
        .all(|url| is_remote_hook_server_url_on_active_port(url, active_port));
    let has_reachable_hook_server = has_expected_hook_server_url
        && is_hook_server_reachable(context, defined_urls[0], Some(host));
    HookServerValidation {
        has_expected_hook_server_url,
        has_reachable_hook_server,
        expected_hook_server_url: None,
        configured_hook_server_url,
    }
}

/// 설치된 hook들이 가리키는 서버 주소가 모두 기대값과 맞고 그 서버에 닿는지 검사한다.
/// `should_validate`가 거짓이면 검사하지 않고 통과로 본다.
pub fn validate_hook_server_configuration(
    context: &HookInstallContext,
    configured_urls: &[Option<String>],
    should_validate: bool,
    ssh_host: Option<&str>,
) -> HookServerValidation {
    let ssh_host = ssh_host.filter(|host| !host.is_empty());
    let defined_urls: Vec<&str> = configured_urls
        .iter()
        .filter_map(|url| url.as_deref().filter(|url| !url.is_empty()))
        .collect();
    let configured_hook_server_url = defined_urls.first().map(|url| url.to_string());
    if !should_validate {
        return unvalidated(configured_hook_server_url, true);
    }
    if defined_urls.len() != configured_urls.len() || defined_urls.is_empty() {
        return unvalidated(configured_hook_server_url, false);
    }
    let Some(expected_url) = expected_hook_server_url(context, ssh_host) else {
        return validate_without_expected_url(context, &defined_urls, ssh_host);
    };
    let has_expected_hook_server_url = defined_urls
        .iter()
        .all(|url| is_expected_hook_server_url(url, &expected_url, ssh_host.is_some()));
    let has_reachable_hook_server = has_expected_hook_server_url
        && is_hook_server_reachable(context, defined_urls[0], ssh_host);
    HookServerValidation {
        has_expected_hook_server_url,
        has_reachable_hook_server,
        expected_hook_server_url: Some(expected_url),
        configured_hook_server_url,
    }
}
