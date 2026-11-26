//! Codex(ChatGPT) 사용량. 창의 종류는 primary·secondary 순서가 아니라 창 길이가 정한다.

use serde_json::Value;

use crate::node_path;
use crate::usage::context::{HttpRequest, UsageContext};
use crate::usage::shared::{
    AI_USAGE_REQUEST_TIMEOUT_MS, classify_usage_http_failure, create_unavailable_usage,
    create_usage_window, finish_usage_result, read_local_text_file, read_usage_payload,
};
use crate::usage::types::{
    AiUsageAccount, AiUsageAccountResult, AiUsageFailureReason, AiUsageWindow, AiUsageWindowKind,
};

pub const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

const CODEX_SESSION_WINDOW_SECONDS: f64 = 18_000.0;
const CODEX_WEEKLY_WINDOW_SECONDS: f64 = 604_800.0;
/// 무료 등급은 5시간·7일 창 대신 30일 창 하나만 받는다
const CODEX_MONTHLY_WINDOW_SECONDS: f64 = 2_592_000.0;
/// 예전 응답에서 관측된 1분 남짓의 창 길이 흔들림만 흡수한다
const CODEX_WINDOW_TOLERANCE_SECONDS: f64 = 60.0;

struct CodexCredentials {
    access_token: String,
    account_id: Option<String>,
}

fn read_codex_credentials(config_dir: &str) -> Option<CodexCredentials> {
    let raw_auth = read_local_text_file(&node_path::join(&[config_dir, "auth.json"]));
    if raw_auth.is_empty() {
        return None;
    }
    let parsed = crate::javascript::parse_json(&raw_auth)?;
    let tokens = parsed.get("tokens")?;
    let access_token = tokens.get("access_token")?.as_str()?;
    if crate::javascript::js_trim(access_token).is_empty() {
        return None;
    }
    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|account_id| !crate::javascript::js_trim(account_id).is_empty())
        .map(str::to_string);
    Some(CodexCredentials {
        access_token: access_token.to_string(),
        account_id,
    })
}

/// 실제 응답에서 primary_window가 7일 창으로 오는 계정이 있어, 순서로 판정하면 5시간 한도를 잘못 표시한다
fn classify_codex_window_kind(limit_window_seconds: Option<&Value>) -> Option<AiUsageWindowKind> {
    let seconds = limit_window_seconds?
        .as_f64()
        .filter(|seconds| seconds.is_finite())?;
    [
        (CODEX_SESSION_WINDOW_SECONDS, AiUsageWindowKind::Session),
        (CODEX_WEEKLY_WINDOW_SECONDS, AiUsageWindowKind::Weekly),
        (CODEX_MONTHLY_WINDOW_SECONDS, AiUsageWindowKind::Monthly),
    ]
    .into_iter()
    .find(|(window_seconds, _)| (seconds - window_seconds).abs() <= CODEX_WINDOW_TOLERANCE_SECONDS)
    .map(|(_, kind)| kind)
}

fn to_codex_usage_window(raw: Option<&Value>) -> Option<AiUsageWindow> {
    let raw = raw.filter(|raw| crate::javascript::is_truthy(raw))?;
    let kind = classify_codex_window_kind(raw.get("limit_window_seconds"))?;
    create_usage_window(
        kind,
        raw.get("used_percent").unwrap_or(&Value::Null),
        raw.get("reset_at").unwrap_or(&Value::Null),
        None,
    )
}

/// 응답이 어느 순서로 오든 짧은 창을 먼저 보여 줘야 어느 한도가 먼저 닫히는지 바로 읽힌다
fn order_codex_windows(payload: &Value) -> Vec<AiUsageWindow> {
    let rate_limit = payload.get("rate_limit");
    let mapped: Vec<AiUsageWindow> = [
        to_codex_usage_window(rate_limit.and_then(|limit| limit.get("primary_window"))),
        to_codex_usage_window(rate_limit.and_then(|limit| limit.get("secondary_window"))),
    ]
    .into_iter()
    .flatten()
    .collect();
    [
        AiUsageWindowKind::Session,
        AiUsageWindowKind::Weekly,
        AiUsageWindowKind::Monthly,
    ]
    .into_iter()
    .filter_map(|kind| mapped.iter().find(|window| window.kind == kind).cloned())
    .collect()
}

fn codex_usage_request(credentials: CodexCredentials) -> HttpRequest {
    let mut headers = vec![(
        "Authorization".to_string(),
        format!("Bearer {}", credentials.access_token),
    )];
    if let Some(account_id) = credentials.account_id {
        headers.push(("ChatGPT-Account-Id".to_string(), account_id));
    }
    HttpRequest {
        method: "GET".to_string(),
        url: CODEX_USAGE_URL.to_string(),
        headers,
        body: None,
        timeout_ms: AI_USAGE_REQUEST_TIMEOUT_MS,
    }
}

pub fn read_codex_usage(context: &UsageContext, account: &AiUsageAccount) -> AiUsageAccountResult {
    let Some(credentials) = read_codex_credentials(&account.config_dir) else {
        return create_unavailable_usage(
            account,
            AiUsageFailureReason::MissingCredentials,
            context.now_iso(),
        );
    };
    let response = context.http.send(&codex_usage_request(credentials));
    let payload = match read_usage_payload(context, account, response, |rejected| {
        classify_usage_http_failure(account, rejected.status, context.now_iso())
    }) {
        Ok(payload) => payload,
        Err(failure) => return *failure,
    };
    let plan_name = payload
        .get("plan_type")
        .and_then(Value::as_str)
        .map(str::to_string);
    finish_usage_result(context, account, order_codex_windows(&payload), plan_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::javascript::{iso_string_from_ms, stringify};
    use crate::test_support::{temp_home, write_file};
    use crate::usage::test_context::TestUsage;
    use crate::usage::types::{AiUsageProvider, AiUsageStatus};
    use serde_json::json;

    struct CodexFixture {
        _home: tempfile::TempDir,
        usage: TestUsage,
        account: AiUsageAccount,
    }

    fn fixture(auth_json: &str) -> CodexFixture {
        let (home, root) = temp_home();
        let config_dir = format!("{root}/.codex");
        write_file(&format!("{config_dir}/auth.json"), auth_json);
        CodexFixture {
            _home: home,
            usage: TestUsage::new(&root),
            account: AiUsageAccount {
                provider: AiUsageProvider::Codex,
                account_id: "account-id".to_string(),
                label: "codex@example.com".to_string(),
                config_dir: config_dir.clone(),
                account_root: config_dir,
            },
        }
    }

    fn auth_json() -> String {
        stringify(&json!({"tokens": {"access_token": "access-token", "account_id": "account-id"}}))
    }

    fn read_with(payload: Value) -> AiUsageAccountResult {
        let fixture = fixture(&auth_json());
        fixture.usage.http.respond_json(200, payload);
        read_codex_usage(&fixture.usage.context, &fixture.account)
    }

    fn reset_iso(seconds: f64) -> Option<String> {
        iso_string_from_ms(seconds * 1000.0)
    }

    #[test]
    fn week_long_primary_window_is_weekly() {
        let result = read_with(json!({"plan_type": "pro", "rate_limit": {
            "primary_window": {"used_percent": 35, "limit_window_seconds": 604800, "reset_at": 1786840544},
            "secondary_window": null
        }}));
        assert_eq!(result.status, AiUsageStatus::Ok);
        assert_eq!(result.plan_name.as_deref(), Some("pro"));
        assert_eq!(
            result.windows,
            [AiUsageWindow {
                kind: AiUsageWindowKind::Weekly,
                model_name: None,
                used_percent: 35.0,
                resets_at: reset_iso(1_786_840_544.0),
            }]
        );
    }

    #[test]
    fn session_and_weekly_windows_map_by_length() {
        let result = read_with(json!({"plan_type": "plus", "rate_limit": {
            "primary_window": {"used_percent": 10, "limit_window_seconds": 18000, "reset_at": 1786800000},
            "secondary_window": {"used_percent": 80, "limit_window_seconds": 604800, "reset_at": 1786900000}
        }}));
        let kinds: Vec<_> = result.windows.iter().map(|window| window.kind).collect();
        let percents: Vec<_> = result
            .windows
            .iter()
            .map(|window| window.used_percent)
            .collect();
        assert_eq!(
            kinds,
            [AiUsageWindowKind::Session, AiUsageWindowKind::Weekly]
        );
        assert_eq!(percents, [10.0, 80.0]);
    }

    #[test]
    fn thirty_day_window_is_monthly() {
        let result = read_with(json!({"plan_type": "free", "rate_limit": {
            "primary_window": {"used_percent": 29, "limit_window_seconds": 2592000, "reset_at": 1789261591},
            "secondary_window": null
        }}));
        assert_eq!(result.status, AiUsageStatus::Ok);
        assert_eq!(
            result.windows,
            [AiUsageWindow {
                kind: AiUsageWindowKind::Monthly,
                model_name: None,
                used_percent: 29.0,
                resets_at: reset_iso(1_789_261_591.0),
            }]
        );
    }

    #[test]
    fn unknown_window_length_is_empty_response() {
        let result = read_with(json!({"plan_type": "free", "rate_limit": {
            "primary_window": {"used_percent": 29, "limit_window_seconds": 86400, "reset_at": 1789261591},
            "secondary_window": null
        }}));
        assert_eq!(result.status, AiUsageStatus::Error);
        assert_eq!(result.reason, Some(AiUsageFailureReason::EmptyResponse));
    }

    #[test]
    fn missing_access_token_skips_network() {
        let fixture = fixture(&stringify(&json!({"auth_mode": "apikey", "tokens": null})));
        let result = read_codex_usage(&fixture.usage.context, &fixture.account);
        assert_eq!(result.status, AiUsageStatus::Unavailable);
        assert_eq!(
            result.reason,
            Some(AiUsageFailureReason::MissingCredentials)
        );
        assert_eq!(fixture.usage.http.request_count(), 0);
    }

    #[test]
    fn sends_account_id_header() {
        let fixture = fixture(&auth_json());
        fixture.usage.http.respond_json(
            200,
            json!({"plan_type": "pro", "rate_limit": {
                "primary_window": {"used_percent": 1, "limit_window_seconds": 604800, "reset_at": 1}
            }}),
        );
        read_codex_usage(&fixture.usage.context, &fixture.account);
        let request = fixture.usage.http.request(0);
        assert_eq!(request.url, CODEX_USAGE_URL);
        assert!(
            request
                .headers
                .contains(&("ChatGPT-Account-Id".to_string(), "account-id".to_string()))
        );
    }
}
