//! Claude 자격증명 해석과 macOS Keychain 조회.
//!
//! 네이티브 Keychain API가 아니라 `/usr/bin/security`를 실행하는 것은 의도된 선택이다. Claude Code가 항목을 만들 때
//! 접근 제어를 지정하지 않아 ACL의 신뢰 앱이 `security` 하나뿐이라, 같은 도구로 읽으면 승인 창 없이 통과한다.
//! 출력에는 토큰이 들어 있으므로 성공·실패 어느 쪽도 값을 로그로 남기지 않는다.

use std::collections::BTreeMap;

use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

use crate::javascript::{js_trim, parse_json};
use crate::usage::context::{ProcessRequest, UsageContext, UsagePlatform};
use crate::usage::shared::read_non_empty_string;

/// Claude Code가 macOS Keychain에 자격증명을 넣을 때 쓰는 서비스 이름
pub const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const CLAUDE_KEYCHAIN_CONFIG_HASH_LENGTH: usize = 8;

/// Finder나 Dock에서 띄운 앱은 로그인 셸의 PATH를 물려받지 못해 시스템 도구를 절대경로로 부른다
pub const SECURITY_COMMAND_PATH: &str = "/usr/bin/security";

/// Keychain 조회는 접근 승인 프롬프트에서 사용자를 기다릴 수 있어 상한을 둔다
const KEYCHAIN_READ_TIMEOUT_MS: u64 = 10_000;
const KEYCHAIN_OUTPUT_MAX_BYTES: usize = 1024 * 1024;

/// `security(1)`가 항목을 찾지 못했을 때 내는 종료 상태
const SECURITY_ITEM_NOT_FOUND_EXIT_CODE: i32 = 44;

fn read_claude_oauth_section(credentials_json: &str) -> Option<Value> {
    if credentials_json.is_empty() {
        return None;
    }
    parse_json(credentials_json)?
        .get("claudeAiOauth")
        .filter(|section| !section.is_null())
        .cloned()
}

pub fn read_claude_access_token(credentials_json: &str) -> Option<String> {
    read_non_empty_string(read_claude_oauth_section(credentials_json)?.get("accessToken"))
}

/// 액세스 토큰이 언제까지 유효한지 알려주는 epoch 밀리초. 자격증명이 두 곳에 있을 때 어느 쪽이 더 신선한지 가리는 근거다
pub fn read_claude_token_expires_at(credentials_json: &str) -> Option<f64> {
    read_claude_oauth_section(credentials_json)?
        .get("expiresAt")?
        .as_f64()
        .filter(|expires_at| expires_at.is_finite())
}

/// 만료 시각을 모르는 토큰은 만료됐다고 단정하지 않는다. 보내 보면 서버가 답해 준다
pub fn is_claude_access_token_expired(credentials_json: &str, now_ms: f64) -> bool {
    read_claude_token_expires_at(credentials_json).is_some_and(|expires_at| expires_at <= now_ms)
}

/// Claude Code가 다음에 실행될 때 액세스 토큰을 스스로 갱신할 수 있는지. 아니면 다시 로그인해야만 풀린다
pub fn has_usable_claude_refresh_token(credentials_json: &str, now_ms: f64) -> bool {
    let Some(oauth_section) = read_claude_oauth_section(credentials_json) else {
        return false;
    };
    if read_non_empty_string(oauth_section.get("refreshToken")).is_none() {
        return false;
    }
    match oauth_section
        .get("refreshTokenExpiresAt")
        .and_then(Value::as_f64)
    {
        Some(refresh_expires_at) => refresh_expires_at > now_ms,
        None => true,
    }
}

/// 구독 등급 표시값. 사용량 응답에는 계정 정보가 없어 자격증명이 유일한 출처다
pub fn read_claude_subscription_type(credentials_json: &str) -> Option<String> {
    read_non_empty_string(read_claude_oauth_section(credentials_json)?.get("subscriptionType"))
}

/// Keychain 조회 결과. "로그인하지 않음"과 "읽지 못함"을 가르는 것이 이 타입의 존재 이유다
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeKeychainReadResult {
    Found { credentials: String },
    Absent,
    Unreadable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecurityOutcome {
    Found,
    Absent,
    Unreadable,
}

impl SecurityOutcome {
    fn label(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::Absent => "absent",
            Self::Unreadable => "unreadable",
        }
    }
}

/// Keychain 조회 자식에게 넘기는 환경. TS는 앱 환경 전체를 물려줬지만 여기서는 `security`가 사용자 Keychain을
/// 찾는 데 필요한 `HOME`, `USER`, `LOGNAME`과 시스템 도구만 담은 `PATH`만 넘긴다
pub fn create_keychain_environment(context: &UsageContext) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".to_string(), context.home_directory.clone()),
        ("USER".to_string(), context.user_name.clone()),
        ("LOGNAME".to_string(), context.user_name.clone()),
        (
            "PATH".to_string(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
        ),
    ])
}

/// 사유를 모르는 실패는 "읽지 못함"으로 둔다. 로그인 여부를 단정하지 않는 쪽이 안전하다
fn run_security(context: &UsageContext, args: &[&str]) -> (SecurityOutcome, String) {
    let output = context.processes.run(&ProcessRequest {
        program: SECURITY_COMMAND_PATH.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        environment: create_keychain_environment(context),
        timeout_ms: KEYCHAIN_READ_TIMEOUT_MS,
        max_output_bytes: KEYCHAIN_OUTPUT_MAX_BYTES,
    });
    match output.exit_code {
        Some(0) => (SecurityOutcome::Found, js_trim(&output.stdout).to_string()),
        Some(SECURITY_ITEM_NOT_FOUND_EXIT_CODE) => (SecurityOutcome::Absent, String::new()),
        _ => (SecurityOutcome::Unreadable, String::new()),
    }
}

/// 현재 사용자 계정으로 먼저 찾고, 없으면 계정을 지정하지 않고 한 번 더 찾는다.
/// 항목의 계정명이 OS 사용자명과 다르게 저장된 사례가 있어 계정을 가정하면 있는 항목을 놓친다
fn read_service_credentials(context: &UsageContext, service: &str) -> (SecurityOutcome, String) {
    let account_scoped = run_security(
        context,
        &[
            "find-generic-password",
            "-a",
            &context.user_name,
            "-s",
            service,
            "-w",
        ],
    );
    if account_scoped.0 != SecurityOutcome::Absent {
        return account_scoped;
    }
    run_security(context, &["find-generic-password", "-s", service, "-w"])
}

fn to_scoped_service(config_dir: &str) -> String {
    let digest = Sha256::digest(config_dir.as_bytes());
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{CLAUDE_KEYCHAIN_SERVICE}-{}",
        &hex[..CLAUDE_KEYCHAIN_CONFIG_HASH_LENGTH]
    )
}

/// Claude Code 2.1+는 config dir로 스코프한 서비스에 쓰고, 그 이전 버전은 접미사 없는 서비스에 썼다
fn build_guessed_services(config_dirs: &[String]) -> Vec<String> {
    let mut services: Vec<String> = Vec::new();
    let candidates = config_dirs
        .iter()
        .map(|config_dir| to_scoped_service(config_dir))
        .chain([CLAUDE_KEYCHAIN_SERVICE.to_string()]);
    for service in candidates {
        if !services.contains(&service) {
            services.push(service);
        }
    }
    services
}

static STORED_SERVICE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""svce"<blob>="([^"]*)""#).expect("Keychain 서비스 패턴"));

/// 추측한 이름이 모두 빗나갔을 때 Keychain에 실제로 있는 이름을 훑는다. `-d` 없이 실행하면 비밀값을 복호화하지 않는다
fn list_stored_claude_services(context: &UsageContext) -> Vec<String> {
    let (outcome, output) = run_security(context, &["dump-keychain"]);
    if outcome != SecurityOutcome::Found {
        return Vec::new();
    }
    let mut services: Vec<String> = Vec::new();
    for found in STORED_SERVICE_PATTERN.captures_iter(&output) {
        let service = found[1].to_string();
        if service.starts_with(CLAUDE_KEYCHAIN_SERVICE) && !services.contains(&service) {
            services.push(service);
        }
    }
    services
}

struct KeychainSearch {
    credentials: Option<String>,
    attempts: Vec<(String, SecurityOutcome)>,
}

/// 로그인 토큰이 실제로 들어 있는 첫 항목을 고른다. Claude Code 2.1.x는 접미사 없는 항목에 MCP 서버 토큰만 남기기도 한다
fn find_credentials_with_access_token(
    context: &UsageContext,
    services: &[String],
) -> KeychainSearch {
    let mut attempts = Vec::new();
    for service in services {
        let (outcome, output) = read_service_credentials(context, service);
        attempts.push((service.clone(), outcome));
        if outcome == SecurityOutcome::Found && read_claude_access_token(&output).is_some() {
            return KeychainSearch {
                credentials: Some(output),
                attempts,
            };
        }
    }
    KeychainSearch {
        credentials: None,
        attempts,
    }
}

/// 맥에서만 나는 실패라 로그가 유일한 단서다. 항목 값에는 토큰이 들어 있으므로 이름과 결과만 남긴다
fn report_keychain_miss(context: &UsageContext, attempts: &[(String, SecurityOutcome)]) {
    let summary = attempts
        .iter()
        .map(|(service, outcome)| format!("{service}={}", outcome.label()))
        .collect::<Vec<_>>()
        .join(", ");
    (context.warn)(&format!(
        "[ai-usage] Keychain에서 Claude 로그인 토큰을 찾지 못했습니다: {summary}"
    ));
}

/// macOS의 Claude Code는 자격증명을 Keychain에 두고 `.credentials.json`은 폴백으로만 쓴다.
/// 이름을 추측해 먼저 찾고, 모두 빗나갔을 때만 실제 항목 목록을 훑는다
pub fn read_claude_keychain_credentials(
    context: &UsageContext,
    config_dirs: &[String],
) -> ClaudeKeychainReadResult {
    if context.platform != UsagePlatform::Darwin {
        return ClaudeKeychainReadResult::Absent;
    }
    let guessed_services = build_guessed_services(config_dirs);
    let guessed_search = find_credentials_with_access_token(context, &guessed_services);
    if let Some(credentials) = guessed_search.credentials {
        return ClaudeKeychainReadResult::Found { credentials };
    }
    let stored_services: Vec<String> = list_stored_claude_services(context)
        .into_iter()
        .filter(|service| !guessed_services.contains(service))
        .collect();
    let stored_search = find_credentials_with_access_token(context, &stored_services);
    if let Some(credentials) = stored_search.credentials {
        return ClaudeKeychainReadResult::Found { credentials };
    }
    let attempts = [guessed_search.attempts, stored_search.attempts].concat();
    report_keychain_miss(context, &attempts);
    if attempts
        .iter()
        .any(|(_, outcome)| *outcome == SecurityOutcome::Unreadable)
    {
        ClaudeKeychainReadResult::Unreadable
    } else {
        ClaudeKeychainReadResult::Absent
    }
}

#[cfg(test)]
mod tests;
