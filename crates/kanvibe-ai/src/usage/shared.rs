//! 사용량 창 정규화, 실패 결과 생성, HTTP 실패 분류처럼 provider 리더가 함께 쓰는 규칙.

use serde_json::Value;

use crate::javascript::{iso_string_from_ms, js_trim, parse_date_ms};
use crate::usage::context::{HttpResponse, UsageContext};
use crate::usage::types::{
    AiUsageAccount, AiUsageAccountResult, AiUsageFailureReason, AiUsageStatus, AiUsageWindow,
    AiUsageWindowKind,
};

/// provider 응답이 느릴 때 패널 전체가 멈추지 않도록 각 호출에 거는 상한
pub const AI_USAGE_REQUEST_TIMEOUT_MS: u64 = 10_000;

/// 화면을 보는 동안 사용량을 다시 받아 오는 주기이자, 받아 둔 결과를 여러 창이 나눠 쓰는 기간.
/// Claude 사용량 엔드포인트가 5분에 대여섯 번이면 잠기므로 그 창에 한 번만 부른다
pub const AI_USAGE_REFRESH_INTERVAL_MS: f64 = 5.0 * 60_000.0;

/// 앱 안에서 로그인하면 풀리는 사유들. 사람이 다시 로그인하기 전까지 계속 실패하므로 잠시 뒤 풀리는 실패와 달리 다룬다
pub const SIGN_IN_FAILURE_REASONS: [AiUsageFailureReason; 3] = [
    AiUsageFailureReason::MissingCredentials,
    AiUsageFailureReason::ExpiredCredentials,
    AiUsageFailureReason::GeminiCliNotFound,
];

/// 초 단위 epoch와 밀리초 단위 epoch를 가르는 경계. 1e10초는 서기 2286년이고 1e10밀리초는 1970년이다
const EPOCH_SECONDS_UPPER_BOUND: f64 = 10_000_000_000.0;

/// JS `Number(text)`. 앞뒤 공백을 걷고, 빈 글은 0, 숫자로 읽을 수 없으면 NaN이다
pub fn js_number(text: &str) -> f64 {
    let trimmed = js_trim(text);
    if trimmed.is_empty() {
        return 0.0;
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = trimmed.strip_prefix(prefix) {
            return parse_radix_digits(digits, radix);
        }
    }
    let is_decimal_literal = trimmed
        .chars()
        .all(|character| character.is_ascii_digit() || "+-.eE".contains(character));
    if !is_decimal_literal {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

fn parse_radix_digits(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    digits
        .chars()
        .try_fold(0.0_f64, |total, character| {
            character
                .to_digit(radix)
                .map(|digit| total * f64::from(radix) + f64::from(digit))
        })
        .unwrap_or(f64::NAN)
}

fn clamp_used_percent(value: &Value) -> Option<f64> {
    let number = value.as_f64().filter(|number| number.is_finite())?;
    Some(number.clamp(0.0, 100.0))
}

fn to_epoch_milliseconds(value: f64) -> f64 {
    if value > EPOCH_SECONDS_UPPER_BOUND {
        value
    } else {
        value * 1000.0
    }
}

/// provider마다 reset 시각을 ISO 문자열, epoch 초, epoch 밀리초 중 아무 형태로나 준다. 어느 쪽이든 ISO 문자열 하나로 누른다
fn to_iso_reset_time(value: &Value) -> Option<String> {
    if let Some(number) = value.as_f64().filter(|number| number.is_finite()) {
        return iso_string_from_ms(to_epoch_milliseconds(number));
    }
    let text = value.as_str().filter(|text| !js_trim(text).is_empty())?;
    let numeric_value = js_number(text);
    if numeric_value.is_finite() {
        return iso_string_from_ms(to_epoch_milliseconds(numeric_value));
    }
    iso_string_from_ms(parse_date_ms(text)?)
}

/// 사용률이 숫자가 아니면 창을 만들지 않는다
pub fn create_usage_window(
    kind: AiUsageWindowKind,
    used_percent: &Value,
    resets_at: &Value,
    model_name: Option<String>,
) -> Option<AiUsageWindow> {
    let used_percent = clamp_used_percent(used_percent)?;
    Some(AiUsageWindow {
        kind,
        model_name,
        used_percent,
        resets_at: to_iso_reset_time(resets_at),
    })
}

pub fn create_usage_result(
    context: &UsageContext,
    account: &AiUsageAccount,
    windows: Vec<AiUsageWindow>,
    plan_name: Option<String>,
) -> AiUsageAccountResult {
    AiUsageAccountResult {
        provider: account.provider,
        account_id: account.account_id.clone(),
        label: account.label.clone(),
        status: AiUsageStatus::Ok,
        plan_name,
        windows,
        reason: None,
        fetched_at: context.now_iso(),
    }
}

/// 아직 로그인하지 않았거나 CLI가 없는 정상적인 부재
pub fn create_unavailable_usage(
    account: &AiUsageAccount,
    reason: AiUsageFailureReason,
    fetched_at: String,
) -> AiUsageAccountResult {
    create_failed_usage(account, AiUsageStatus::Unavailable, reason, fetched_at)
}

/// 호출이 실패했거나 응답을 해석할 수 없는 비정상 상태
pub fn create_error_usage(
    account: &AiUsageAccount,
    reason: AiUsageFailureReason,
    fetched_at: String,
) -> AiUsageAccountResult {
    create_failed_usage(account, AiUsageStatus::Error, reason, fetched_at)
}

fn create_failed_usage(
    account: &AiUsageAccount,
    status: AiUsageStatus,
    reason: AiUsageFailureReason,
    fetched_at: String,
) -> AiUsageAccountResult {
    AiUsageAccountResult {
        provider: account.provider,
        account_id: account.account_id.clone(),
        label: account.label.clone(),
        status,
        plan_name: None,
        windows: Vec::new(),
        reason: Some(reason),
        fetched_at,
    }
}

/// 인증 실패는 재로그인으로 풀리는 부재이고 429는 잠시 뒤 재시도할 문제라 서로 다른 상태로 가른다.
/// 나머지 비 2xx는 스키마가 바뀌었을 수도 있으므로 error로만 떨어뜨린다
pub fn classify_usage_http_failure(
    account: &AiUsageAccount,
    status_code: u16,
    fetched_at: String,
) -> AiUsageAccountResult {
    match status_code {
        401 | 403 => create_unavailable_usage(
            account,
            AiUsageFailureReason::ExpiredCredentials,
            fetched_at,
        ),
        429 => create_error_usage(account, AiUsageFailureReason::RateLimited, fetched_at),
        _ => create_error_usage(account, AiUsageFailureReason::FetchFailed, fetched_at),
    }
}

/// 리더 흐름을 `?`로 끊을 때 쓰는 실패 결과
pub type UsageFailure = Box<AiUsageAccountResult>;

/// 응답 본문을 JSON으로 읽는다. 연결 실패와 해석 실패는 fetch-failed이고, 비 2xx는 `on_rejected`가 결과를 정한다
pub fn read_usage_payload(
    context: &UsageContext,
    account: &AiUsageAccount,
    response: Result<HttpResponse, String>,
    on_rejected: impl FnOnce(&HttpResponse) -> AiUsageAccountResult,
) -> Result<Value, UsageFailure> {
    let fetch_failed = || {
        Box::new(create_error_usage(
            account,
            AiUsageFailureReason::FetchFailed,
            context.now_iso(),
        ))
    };
    let response = response.map_err(|_| fetch_failed())?;
    if !response.is_ok() {
        return Err(Box::new(on_rejected(&response)));
    }
    response.json().ok_or_else(fetch_failed)
}

/// 창이 하나도 없으면 응답 형식이 바뀐 것으로 보고 empty-response, 있으면 성공 결과다
pub fn finish_usage_result(
    context: &UsageContext,
    account: &AiUsageAccount,
    windows: Vec<AiUsageWindow>,
    plan_name: Option<String>,
) -> AiUsageAccountResult {
    if windows.is_empty() {
        return create_error_usage(
            account,
            AiUsageFailureReason::EmptyResponse,
            context.now_iso(),
        );
    }
    create_usage_result(context, account, windows, plan_name)
}

/// TS `readTextFile`(로컬). 읽지 못하면 빈 문자열이다
pub fn read_local_text_file(file_path: &str) -> String {
    std::fs::read(file_path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// `typeof value === "string" && value.trim()`이면 앞뒤 공백을 걷은 값
pub fn read_non_empty_string(value: Option<&Value>) -> Option<String> {
    let trimmed = js_trim(value?.as_str()?);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn js_number_follows_number_constructor() {
        assert_eq!(js_number(" 290 "), 290.0);
        assert_eq!(js_number(""), 0.0);
        assert_eq!(js_number("0x10"), 16.0);
        assert_eq!(js_number("1e3"), 1000.0);
        assert!(js_number("inf").is_nan());
        assert!(js_number("2026-08-10T00:00:00Z").is_nan());
    }

    #[test]
    fn reset_time_accepts_seconds_milliseconds_and_iso_text() {
        let window =
            |value: Value| create_usage_window(AiUsageWindowKind::Session, &json!(5), &value, None);
        assert_eq!(
            window(json!(1_786_840_544)).unwrap().resets_at.as_deref(),
            Some("2026-08-16T00:35:44.000Z")
        );
        assert_eq!(
            window(json!(1_786_840_544_000_i64))
                .unwrap()
                .resets_at
                .as_deref(),
            Some("2026-08-16T00:35:44.000Z")
        );
        assert_eq!(
            window(json!("1786840544")).unwrap().resets_at.as_deref(),
            Some("2026-08-16T00:35:44.000Z")
        );
        assert_eq!(
            window(json!("2026-08-10T06:20:00Z"))
                .unwrap()
                .resets_at
                .as_deref(),
            Some("2026-08-10T06:20:00.000Z")
        );
        assert_eq!(window(json!("  ")).unwrap().resets_at, None);
        assert_eq!(
            create_usage_window(AiUsageWindowKind::Session, &json!(140), &Value::Null, None)
                .unwrap()
                .used_percent,
            100.0
        );
        assert!(
            create_usage_window(AiUsageWindowKind::Session, &json!("5"), &Value::Null, None)
                .is_none()
        );
    }
}
