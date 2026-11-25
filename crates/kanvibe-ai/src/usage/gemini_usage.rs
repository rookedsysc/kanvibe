//! Gemini Code Assist 쿼터. 갱신한 토큰은 `oauth_creds.json`에 되쓰지 않는다.
//! 그 파일은 Gemini CLI의 것이고, KanVibe가 남의 도구 설정을 고치면 CLI 쪽 상태와 어긋날 수 있다.

use serde_json::Value;

use crate::javascript::{js_trim, parse_json};
use crate::node_path;
use crate::usage::context::{HttpRequest, UsageContext};
use crate::usage::gemini_oauth_client::{
    load_gemini_project_id, project_request_body, refresh_gemini_access_token,
    resolve_gemini_oauth_client,
};
use crate::usage::shared::{
    AI_USAGE_REQUEST_TIMEOUT_MS, UsageFailure, classify_usage_http_failure, create_error_usage,
    create_unavailable_usage, create_usage_window, finish_usage_result, read_local_text_file,
    read_usage_payload,
};
use crate::usage::types::{
    AiUsageAccount, AiUsageAccountResult, AiUsageFailureReason, AiUsageWindow, AiUsageWindowKind,
};

pub const RETRIEVE_USER_QUOTA_URL: &str =
    "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";

/// 응답의 모델 id는 기계용이라 그대로 두면 패널이 읽기 어렵다
fn mapped_model_display_name(model_id: &str) -> Option<&'static str> {
    match model_id {
        "gemini-3.1-pro" => Some("3.1 Pro"),
        "gemini-3.1-flash" => Some("3.1 Flash"),
        "gemini-3.0-pro" => Some("3.0 Pro"),
        "gemini-3.0-flash" => Some("3.0 Flash"),
        "gemini-2.5-pro" => Some("Pro"),
        "gemini-2.5-flash" => Some("Flash"),
        "gemini-2.5-flash-lite" => Some("Flash Lite"),
        _ => None,
    }
}

struct GeminiCredentials {
    access_token: String,
    refresh_token: String,
    expiry_date: f64,
}

struct GeminiQuotaBucket {
    remaining_fraction: f64,
    model_id: String,
    reset_time: String,
}

fn capitalize_first(part: &str) -> String {
    let mut characters = part.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

fn to_model_display_name(model_id: &str) -> String {
    if let Some(mapped_name) = mapped_model_display_name(model_id) {
        return mapped_name.to_string();
    }
    let has_prefix = model_id
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("gemini-"));
    let without_prefix = if has_prefix { &model_id[7..] } else { model_id };
    without_prefix
        .split('-')
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ")
}

/// JS `Math.round`. .5는 +∞ 쪽으로 올린다
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// 응답은 남은 비율을 주는데 다른 provider와 나란히 보이려면 사용한 비율이어야 한다
fn to_gemini_usage_window(bucket: &GeminiQuotaBucket) -> Option<AiUsageWindow> {
    let used_percent = js_round((1.0 - bucket.remaining_fraction) * 100.0);
    create_usage_window(
        AiUsageWindowKind::Model,
        &serde_json::json!(used_percent),
        &Value::String(bucket.reset_time.clone()),
        Some(to_model_display_name(&bucket.model_id)),
    )
}

fn to_quota_bucket(value: &Value) -> Option<GeminiQuotaBucket> {
    let remaining_fraction = value
        .get("remainingFraction")?
        .as_f64()
        .filter(|fraction| fraction.is_finite())?;
    Some(GeminiQuotaBucket {
        remaining_fraction,
        model_id: value.get("modelId")?.as_str()?.to_string(),
        reset_time: value.get("resetTime")?.as_str()?.to_string(),
    })
}

/// 이 엔드포인트는 버킷 배열을 최상위로 주기도 하고 buckets 키에 담아 주기도 한다
fn parse_quota_buckets(payload: &Value) -> Vec<GeminiQuotaBucket> {
    let buckets = match payload {
        Value::Array(items) => Some(items),
        other => other.get("buckets").and_then(Value::as_array),
    };
    buckets
        .map(|items| items.iter().filter_map(to_quota_bucket).collect())
        .unwrap_or_default()
}

fn read_gemini_credentials(config_dir: &str) -> Option<GeminiCredentials> {
    let raw_credentials = read_local_text_file(&node_path::join(&[config_dir, "oauth_creds.json"]));
    if raw_credentials.is_empty() {
        return None;
    }
    let parsed = parse_json(&raw_credentials)?;
    let access_token = parsed
        .get("access_token")?
        .as_str()
        .filter(|token| !js_trim(token).is_empty())?;
    Some(GeminiCredentials {
        access_token: access_token.to_string(),
        refresh_token: parsed
            .get("refresh_token")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        expiry_date: parsed
            .get("expiry_date")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
    })
}

fn resolve_access_token(
    context: &UsageContext,
    account: &AiUsageAccount,
    credentials: &GeminiCredentials,
) -> Result<String, UsageFailure> {
    if credentials.expiry_date > context.now_ms() {
        return Ok(credentials.access_token.clone());
    }
    let Some(oauth_client) = resolve_gemini_oauth_client(context) else {
        return Err(Box::new(create_unavailable_usage(
            account,
            AiUsageFailureReason::GeminiCliNotFound,
            context.now_iso(),
        )));
    };
    match refresh_gemini_access_token(context, &credentials.refresh_token, &oauth_client) {
        Some(refreshed) => Ok(refreshed.access_token),
        None => Err(Box::new(create_unavailable_usage(
            account,
            AiUsageFailureReason::ExpiredCredentials,
            context.now_iso(),
        ))),
    }
}

fn quota_request(access_token: &str, project_id: &str) -> HttpRequest {
    HttpRequest {
        method: "POST".to_string(),
        url: RETRIEVE_USER_QUOTA_URL.to_string(),
        headers: vec![
            (
                "Authorization".to_string(),
                format!("Bearer {access_token}"),
            ),
            ("Content-Type".to_string(), "application/json".to_string()),
        ],
        body: Some(project_request_body(project_id)),
        timeout_ms: AI_USAGE_REQUEST_TIMEOUT_MS,
    }
}

fn read_gemini_quota(
    context: &UsageContext,
    account: &AiUsageAccount,
) -> Result<AiUsageAccountResult, UsageFailure> {
    let Some(credentials) = read_gemini_credentials(&account.config_dir) else {
        return Ok(create_unavailable_usage(
            account,
            AiUsageFailureReason::MissingCredentials,
            context.now_iso(),
        ));
    };
    let access_token = resolve_access_token(context, account, &credentials)?;
    let Some(project_id) = load_gemini_project_id(context, &access_token) else {
        return Ok(create_error_usage(
            account,
            AiUsageFailureReason::FetchFailed,
            context.now_iso(),
        ));
    };
    let response = context
        .http
        .send(&quota_request(&access_token, &project_id));
    let payload = read_usage_payload(context, account, response, |rejected| {
        classify_usage_http_failure(account, rejected.status, context.now_iso())
    })?;
    let windows = parse_quota_buckets(&payload)
        .iter()
        .filter_map(to_gemini_usage_window)
        .collect();
    Ok(finish_usage_result(context, account, windows, None))
}

pub fn read_gemini_usage(context: &UsageContext, account: &AiUsageAccount) -> AiUsageAccountResult {
    read_gemini_quota(context, account).unwrap_or_else(|failure| *failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::javascript::stringify;
    use crate::test_support::{temp_home, write_file};
    use crate::usage::gemini_oauth_client::LOAD_CODE_ASSIST_URL;
    use crate::usage::test_context::{TestUsage, json_response};
    use crate::usage::types::{AiUsageProvider, AiUsageStatus};
    use serde_json::json;

    const ONE_HOUR_MS: f64 = 60.0 * 60.0 * 1000.0;

    struct GeminiFixture {
        _home: tempfile::TempDir,
        usage: TestUsage,
        account: AiUsageAccount,
    }

    fn fixture(expiry_offset_ms: Option<f64>) -> GeminiFixture {
        let (home, root) = temp_home();
        let config_dir = format!("{root}/.gemini");
        let usage = TestUsage::new(&root);
        if let Some(offset) = expiry_offset_ms {
            let credentials = json!({"access_token": "access-token", "refresh_token": "refresh-token", "expiry_date": usage.context.now_ms() + offset});
            write_file(
                &format!("{config_dir}/oauth_creds.json"),
                &stringify(&credentials),
            );
        }
        GeminiFixture {
            _home: home,
            usage,
            account: AiUsageAccount {
                provider: AiUsageProvider::Gemini,
                account_id: config_dir.clone(),
                label: "Gemini".to_string(),
                config_dir: config_dir.clone(),
                account_root: config_dir,
            },
        }
    }

    /// 프로젝트 조회는 `project-id`로, 쿼터 조회는 주어진 본문으로 답한다
    fn respond_with_quota(fixture: &GeminiFixture, quota: Value) {
        let quota = quota.to_string();
        fixture.usage.http.respond(move |request| {
            if request.url == LOAD_CODE_ASSIST_URL {
                return Ok(json_response(
                    200,
                    r#"{"cloudaicompanionProject":"project-id"}"#,
                ));
            }
            Ok(json_response(200, &quota))
        });
    }

    fn quota_requests(fixture: &GeminiFixture) -> usize {
        fixture
            .usage
            .http
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.url == RETRIEVE_USER_QUOTA_URL)
            .count()
    }

    // parity: TS-aiUsage-readGeminiUsage-d01581
    #[test]
    fn missing_credentials_are_unavailable() {
        let fixture = fixture(None);
        let result = read_gemini_usage(&fixture.usage.context, &fixture.account);
        assert_eq!(result.status, AiUsageStatus::Unavailable);
        assert_eq!(
            result.reason,
            Some(AiUsageFailureReason::MissingCredentials)
        );
        assert_eq!(fixture.usage.http.request_count(), 0);
    }

    // parity: TS-aiUsage-readGeminiUsage-e0e627
    #[test]
    fn expired_token_without_installed_cli_gives_up() {
        let fixture = fixture(Some(-ONE_HOUR_MS));
        let result = read_gemini_usage(&fixture.usage.context, &fixture.account);
        assert_eq!(result.status, AiUsageStatus::Unavailable);
        assert_eq!(result.reason, Some(AiUsageFailureReason::GeminiCliNotFound));
        assert_eq!(fixture.usage.http.request_count(), 0);
    }

    // parity: TS-aiUsage-readGeminiUsage-24a064
    #[test]
    fn unknown_project_skips_quota_call() {
        let fixture = fixture(Some(ONE_HOUR_MS));
        fixture.usage.http.respond_json(200, json!({}));
        let result = read_gemini_usage(&fixture.usage.context, &fixture.account);
        assert_eq!(result.status, AiUsageStatus::Error);
        assert_eq!(result.reason, Some(AiUsageFailureReason::FetchFailed));
        assert_eq!(quota_requests(&fixture), 0);
    }

    // parity: TS-aiUsage-readGeminiUsage-812c16
    #[test]
    fn remaining_fraction_becomes_used_percent_with_display_name() {
        let fixture = fixture(Some(ONE_HOUR_MS));
        respond_with_quota(
            &fixture,
            json!({"buckets": [{"remainingFraction": 0.25, "modelId": "gemini-2.5-pro", "resetTime": "2026-08-10T07:00:00.000Z"}]}),
        );
        let result = read_gemini_usage(&fixture.usage.context, &fixture.account);
        assert_eq!(result.status, AiUsageStatus::Ok);
        assert_eq!(
            result.windows,
            [AiUsageWindow {
                kind: AiUsageWindowKind::Model,
                model_name: Some("Pro".to_string()),
                used_percent: 75.0,
                resets_at: Some("2026-08-10T07:00:00.000Z".to_string()),
            }]
        );
        let quota_request = fixture.usage.http.request(1);
        assert_eq!(
            quota_request.body.as_deref(),
            Some(r#"{"project":"project-id"}"#)
        );
    }

    // parity: TS-aiUsage-readGeminiUsage-16ae2f
    #[test]
    fn unmapped_model_id_gets_readable_name() {
        let fixture = fixture(Some(ONE_HOUR_MS));
        respond_with_quota(
            &fixture,
            json!({"buckets": [{"remainingFraction": 1, "modelId": "gemini-4.0-ultra", "resetTime": "2026-08-10T07:00:00.000Z"}]}),
        );
        let result = read_gemini_usage(&fixture.usage.context, &fixture.account);
        assert_eq!(result.windows[0].model_name.as_deref(), Some("4.0 Ultra"));
        assert_eq!(result.windows[0].used_percent, 0.0);
    }
}
