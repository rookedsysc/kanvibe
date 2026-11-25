//! Claude OAuth 사용량. 자격증명은 파일과 Keychain 중 신선한 쪽을 읽기만 하고, 갱신은 Claude Code에게 맡긴다.
//! refresh 토큰은 한 번 쓰면 회전돼서, KanVibe가 대신 갱신하면 Claude Code가 들고 있던 값이 무효가 된다.

use serde_json::Value;

use crate::node_path;
use crate::usage::claude_credentials::{
    ClaudeKeychainReadResult, has_usable_claude_refresh_token, is_claude_access_token_expired,
    read_claude_access_token, read_claude_keychain_credentials, read_claude_subscription_type,
    read_claude_token_expires_at,
};
use crate::usage::context::{HttpRequest, HttpResponse, UsageContext};
use crate::usage::shared::{
    AI_USAGE_REQUEST_TIMEOUT_MS, UsageFailure, classify_usage_http_failure, create_error_usage,
    create_unavailable_usage, create_usage_window, finish_usage_result, js_number,
    read_local_text_file, read_usage_payload,
};
use crate::usage::types::{
    AiUsageAccount, AiUsageAccountResult, AiUsageFailureReason, AiUsageWindow, AiUsageWindowKind,
};

pub const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// Claude Code CLI가 이 엔드포인트에 붙이는 헤더를 그대로 맞춘다. 값이 다르면 서버가 응답 형식을 바꾸거나 거절한다
const CLAUDE_OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";
const CLAUDE_CODE_USER_AGENT: &str = "claude-code/2.1.0";

const TOO_MANY_REQUESTS_STATUS: u16 = 429;

/// `retry-after`가 비어 있을 때 쓸 간격. 실측 응답이 290초를 줬다
const CLAUDE_RATE_LIMIT_FALLBACK_SECONDS: f64 = 300.0;

struct FreshClaudeCredentials {
    credentials: String,
    is_keychain_unreadable: bool,
}

/// 두 저장소 중 더 나중까지 쓸 수 있는 쪽을 가른다. 만료 시각을 모르는 쪽은 비교에서 지고, 양쪽 다 모르면 파일을 쓴다
fn is_keychain_fresher(file_credentials: &str, keychain_credentials: &str) -> bool {
    if keychain_credentials.is_empty() {
        return false;
    }
    if file_credentials.is_empty() {
        return true;
    }
    let keychain_expires_at = read_claude_token_expires_at(keychain_credentials);
    let file_expires_at = read_claude_token_expires_at(file_credentials);
    match (keychain_expires_at, file_expires_at) {
        (Some(keychain), Some(file)) => keychain > file,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

fn read_fresh_claude_credentials(
    context: &UsageContext,
    config_dir: &str,
) -> FreshClaudeCredentials {
    let file_credentials =
        read_local_text_file(&node_path::join(&[config_dir, ".credentials.json"]));
    let keychain_result = read_claude_keychain_credentials(context, &[config_dir.to_string()]);
    let is_keychain_unreadable = keychain_result == ClaudeKeychainReadResult::Unreadable;
    let keychain_credentials = match keychain_result {
        ClaudeKeychainReadResult::Found { credentials } => credentials,
        _ => String::new(),
    };
    let credentials = if is_keychain_fresher(&file_credentials, &keychain_credentials) {
        keychain_credentials
    } else {
        file_credentials
    };
    FreshClaudeCredentials {
        credentials,
        is_keychain_unreadable,
    }
}

/// 액세스 토큰을 쓸 수 없을 때, Claude Code가 풀 수 있는 부재와 다시 로그인해야 하는 부재를 가른다
fn create_unusable_token_usage(
    context: &UsageContext,
    account: &AiUsageAccount,
    credentials: &str,
) -> AiUsageAccountResult {
    let reason = if has_usable_claude_refresh_token(credentials, context.now_ms()) {
        AiUsageFailureReason::AwaitingCliRefresh
    } else {
        AiUsageFailureReason::ExpiredCredentials
    };
    create_unavailable_usage(account, reason, context.now_iso())
}

fn to_claude_usage_window(raw: Option<&Value>, kind: AiUsageWindowKind) -> Option<AiUsageWindow> {
    let raw = raw.filter(|raw| crate::javascript::is_truthy(raw))?;
    let utilization = raw.get("utilization").filter(|value| value.is_number());
    let used_percent = utilization
        .or_else(|| raw.get("used_percentage"))
        .unwrap_or(&Value::Null);
    create_usage_window(
        kind,
        used_percent,
        raw.get("resets_at").unwrap_or(&Value::Null),
        None,
    )
}

/// 응답의 창 이름을 화면이 아는 종류로 옮긴다. `weekly_scoped`는 7일 한도 안에서 모델 몫만 따로 센 창이라 7일로 묶는다
fn claude_limit_kind(kind: &str) -> Option<(AiUsageWindowKind, bool)> {
    match kind {
        "session" => Some((AiUsageWindowKind::Session, false)),
        "weekly_all" => Some((AiUsageWindowKind::Weekly, false)),
        "weekly_scoped" => Some((AiUsageWindowKind::Weekly, true)),
        _ => None,
    }
}

fn to_claude_limit_window(limit: &Value) -> Option<AiUsageWindow> {
    let (kind, is_model_scoped) = claude_limit_kind(limit.get("kind")?.as_str()?)?;
    let percent = limit.get("percent").unwrap_or(&Value::Null);
    let resets_at = limit.get("resets_at").unwrap_or(&Value::Null);
    if !is_model_scoped {
        return create_usage_window(kind, percent, resets_at, None);
    }
    // 모델 창은 모델 이름이 곧 라벨이라, 이름이 없으면 무엇의 한도인지 알릴 방법이 없다
    let model_name = limit
        .pointer("/scope/model/display_name")
        .and_then(Value::as_str)?;
    let trimmed = crate::javascript::js_trim(model_name);
    (!trimmed.is_empty())
        .then(|| create_usage_window(kind, percent, resets_at, Some(trimmed.to_string())))?
}

/// 새 응답은 `limits` 배열에 창을 담고, 배열이 없는 옛 응답은 상위 필드를 폴백으로 쓴다
fn to_claude_usage_windows(payload: &Value) -> Vec<AiUsageWindow> {
    let limit_windows: Vec<AiUsageWindow> = payload
        .get("limits")
        .and_then(Value::as_array)
        .map(|limits| limits.iter().filter_map(to_claude_limit_window).collect())
        .unwrap_or_default();
    if !limit_windows.is_empty() {
        return limit_windows;
    }
    [
        to_claude_usage_window(payload.get("five_hour"), AiUsageWindowKind::Session),
        to_claude_usage_window(payload.get("seven_day"), AiUsageWindowKind::Weekly),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn to_retry_allowed_at_ms(context: &UsageContext, response: &HttpResponse) -> f64 {
    let retry_after_seconds = response.header("retry-after").map_or(0.0, js_number);
    let backoff_seconds = if retry_after_seconds.is_finite() && retry_after_seconds > 0.0 {
        retry_after_seconds
    } else {
        CLAUDE_RATE_LIMIT_FALLBACK_SECONDS
    };
    context.now_ms() + backoff_seconds * 1000.0
}

fn fetch_claude_usage(context: &UsageContext, access_token: &str) -> Result<HttpResponse, String> {
    context.http.send(&HttpRequest {
        method: "GET".to_string(),
        url: CLAUDE_USAGE_URL.to_string(),
        headers: vec![
            (
                "Authorization".to_string(),
                format!("Bearer {access_token}"),
            ),
            (
                "anthropic-beta".to_string(),
                CLAUDE_OAUTH_BETA_HEADER.to_string(),
            ),
            ("User-Agent".to_string(), CLAUDE_CODE_USER_AGENT.to_string()),
        ],
        body: None,
        timeout_ms: AI_USAGE_REQUEST_TIMEOUT_MS,
    })
}

fn is_rate_limited(context: &UsageContext) -> bool {
    context.now_ms()
        < *context
            .claude_retry_allowed_at_ms
            .lock()
            .expect("재시도 시각")
}

fn read_claude_windows(
    context: &UsageContext,
    account: &AiUsageAccount,
) -> Result<AiUsageAccountResult, UsageFailure> {
    let fresh = read_fresh_claude_credentials(context, &account.config_dir);
    let Some(access_token) = read_claude_access_token(&fresh.credentials) else {
        let reason = if fresh.is_keychain_unreadable {
            AiUsageFailureReason::KeychainUnreadable
        } else {
            AiUsageFailureReason::MissingCredentials
        };
        return Ok(create_unavailable_usage(account, reason, context.now_iso()));
    };
    // 만료된 토큰은 보내도 거절될 뿐이라, 몇 번 안 되는 호출 예산을 거기에 쓰지 않는다
    if is_claude_access_token_expired(&fresh.credentials, context.now_ms()) {
        return Ok(create_unusable_token_usage(
            context,
            account,
            &fresh.credentials,
        ));
    }
    if is_rate_limited(context) {
        return Ok(create_error_usage(
            account,
            AiUsageFailureReason::RateLimited,
            context.now_iso(),
        ));
    }
    let response = fetch_claude_usage(context, &access_token);
    let payload = read_usage_payload(context, account, response, |rejected| {
        classify_rejected_response(context, account, rejected, &fresh.credentials)
    })?;
    let plan_name = read_claude_subscription_type(&fresh.credentials);
    Ok(finish_usage_result(
        context,
        account,
        to_claude_usage_windows(&payload),
        plan_name,
    ))
}

pub fn read_claude_usage(context: &UsageContext, account: &AiUsageAccount) -> AiUsageAccountResult {
    read_claude_windows(context, account).unwrap_or_else(|failure| *failure)
}

fn classify_rejected_response(
    context: &UsageContext,
    account: &AiUsageAccount,
    response: &HttpResponse,
    credentials: &str,
) -> AiUsageAccountResult {
    if response.status == TOO_MANY_REQUESTS_STATUS {
        *context
            .claude_retry_allowed_at_ms
            .lock()
            .expect("재시도 시각") = to_retry_allowed_at_ms(context, response);
    }
    let failure = classify_usage_http_failure(account, response.status, context.now_iso());
    if failure.reason == Some(AiUsageFailureReason::ExpiredCredentials) {
        create_unusable_token_usage(context, account, credentials)
    } else {
        failure
    }
}

#[cfg(test)]
mod tests;
