use serde_json::{Value, json};

use super::*;
use crate::javascript::{parse_date_ms, stringify};
use crate::test_support::{temp_home, write_file};
use crate::usage::context::UsagePlatform;
use crate::usage::test_context::{TestUsage, exited, json_response};
use crate::usage::types::{AiUsageProvider, AiUsageStatus};

fn unexpired_at_ms() -> f64 {
    parse_date_ms("2999-01-01T00:00:00.000Z").unwrap()
}
const EXPIRED_AT_MS: f64 = 1_000.0;

fn create_credentials_json(access_token: &str, expires_at: Option<f64>) -> String {
    let mut oauth = json!({"accessToken": access_token, "refreshToken": "refresh-token"});
    if let Some(expires_at) = expires_at {
        oauth["expiresAt"] = json!(expires_at);
    }
    stringify(&json!({"claudeAiOauth": oauth}))
}

struct ClaudeFixture {
    _home: tempfile::TempDir,
    usage: TestUsage,
    account: AiUsageAccount,
}

impl ClaudeFixture {
    fn new(file_credentials: &str) -> Self {
        Self::on(UsagePlatform::Linux, file_credentials)
    }

    /// Keychain이 있는 macOS. `keychain_credentials`가 없으면 모든 조회가 거부된다
    fn with_keychain(file_credentials: &str, keychain_credentials: Option<String>) -> Self {
        let fixture = Self::on(UsagePlatform::Darwin, file_credentials);
        fixture
            .usage
            .processes
            .respond(move |_| match &keychain_credentials {
                Some(credentials) => exited(0, credentials),
                None => exited(36, ""),
            });
        fixture
    }

    fn on(platform: UsagePlatform, file_credentials: &str) -> Self {
        let (home, root) = temp_home();
        let config_dir = format!("{root}/.claude");
        if !file_credentials.is_empty() {
            write_file(&format!("{config_dir}/.credentials.json"), file_credentials);
        }
        let usage = TestUsage::with(&root, platform, &[]);
        let account = AiUsageAccount {
            provider: AiUsageProvider::Claude,
            account_id: "account-uuid".to_string(),
            label: "me@example.com".to_string(),
            config_dir: config_dir.clone(),
            account_root: config_dir,
        };
        Self {
            _home: home,
            usage,
            account,
        }
    }

    fn respond(&self, payload: Value) {
        self.usage.http.respond_json(200, payload);
    }

    fn read(&self) -> AiUsageAccountResult {
        read_claude_usage(&self.usage.context, &self.account)
    }

    fn sent_authorization(&self, index: usize) -> String {
        let request = self.usage.http.request(index);
        request
            .headers
            .iter()
            .find(|(name, _)| name == "Authorization")
            .unwrap()
            .1
            .clone()
    }

    fn stub_rate_limited(&self, retry_after: Option<&'static str>) {
        self.usage.http.respond(move |_| {
            let mut response = json_response(429, "{}");
            if let Some(seconds) = retry_after {
                response
                    .headers
                    .push(("Retry-After".to_string(), seconds.to_string()));
            }
            Ok(response)
        });
    }
}

fn usage_payload() -> Value {
    json!({"five_hour": {"utilization": 5, "resets_at": null}})
}

fn window(
    kind: AiUsageWindowKind,
    model_name: Option<&str>,
    used_percent: f64,
    resets_at: Option<&str>,
) -> AiUsageWindow {
    AiUsageWindow {
        kind,
        model_name: model_name.map(str::to_string),
        used_percent,
        resets_at: resets_at.map(str::to_string),
    }
}

// parity: TS-aiUsage-readClaudeUsage-95881b
#[test]
fn reads_credentials_from_account_config_dir() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.respond(usage_payload());
    let result = fixture.read();
    assert_eq!(fixture.sent_authorization(0), "Bearer access-token");
    assert_eq!(result.account_id, "account-uuid");
    assert_eq!(result.label, "me@example.com");
}

// parity: TS-aiUsage-readClaudeUsage-0646cd
#[test]
fn maps_five_hour_and_seven_day_windows() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.respond(json!({
        "five_hour": {"utilization": 22, "resets_at": "2026-08-10T06:20:00.000Z"},
        "seven_day": {"utilization": 61, "resets_at": "2026-08-13T02:00:00.000Z"}
    }));
    let result = fixture.read();
    assert_eq!(result.status, AiUsageStatus::Ok);
    assert_eq!(
        result.windows,
        [
            window(
                AiUsageWindowKind::Session,
                None,
                22.0,
                Some("2026-08-10T06:20:00.000Z")
            ),
            window(
                AiUsageWindowKind::Weekly,
                None,
                61.0,
                Some("2026-08-13T02:00:00.000Z")
            ),
        ]
    );
}

// parity: TS-aiUsage-readClaudeUsage-db3864
#[test]
fn expired_token_with_live_refresh_waits_for_cli() {
    let fixture = ClaudeFixture::new(&create_credentials_json(
        "expired-token",
        Some(EXPIRED_AT_MS),
    ));
    assert_eq!(
        fixture.read().reason,
        Some(AiUsageFailureReason::AwaitingCliRefresh)
    );
    assert_eq!(fixture.usage.http.request_count(), 0);
}

// parity: TS-aiUsage-readClaudeUsage-600ab2
#[test]
fn expired_token_without_refresh_needs_sign_in() {
    let fixture = ClaudeFixture::new(&stringify(
        &json!({"claudeAiOauth": {"accessToken": "expired-token", "expiresAt": EXPIRED_AT_MS}}),
    ));
    assert_eq!(
        fixture.read().reason,
        Some(AiUsageFailureReason::ExpiredCredentials)
    );
    assert_eq!(fixture.usage.http.request_count(), 0);
}

// parity: TS-aiUsage-readClaudeUsage-5a9387
#[test]
fn both_tokens_expired_needs_sign_in() {
    let fixture = ClaudeFixture::new(&stringify(&json!({"claudeAiOauth": {
        "accessToken": "expired-token",
        "refreshToken": "expired-refresh-token",
        "expiresAt": EXPIRED_AT_MS,
        "refreshTokenExpiresAt": EXPIRED_AT_MS
    }})));
    assert_eq!(
        fixture.read().reason,
        Some(AiUsageFailureReason::ExpiredCredentials)
    );
    assert_eq!(fixture.usage.http.request_count(), 0);
}

// parity: TS-aiUsage-readClaudeUsage-3a3d2a
#[test]
fn unknown_expiry_is_queried() {
    let fixture = ClaudeFixture::new(&create_credentials_json("token-without-expiry", None));
    fixture.respond(usage_payload());
    assert_eq!(fixture.read().status, AiUsageStatus::Ok);
    assert_eq!(fixture.usage.http.request_count(), 1);
}

// parity: TS-aiUsage-readClaudeUsage-f4c2cf
#[test]
fn soon_expiring_token_is_sent_without_refreshing() {
    let fixture = ClaudeFixture::new("");
    let soon = fixture.usage.context.now_ms() + 60_000.0;
    write_file(
        &format!("{}/.credentials.json", fixture.account.config_dir),
        &create_credentials_json("soon-expiring-token", Some(soon)),
    );
    fixture.respond(usage_payload());
    fixture.read();
    assert_eq!(fixture.usage.http.request_count(), 1);
    assert_eq!(fixture.usage.http.request(0).url, CLAUDE_USAGE_URL);
    assert_eq!(fixture.sent_authorization(0), "Bearer soon-expiring-token");
}

// parity: TS-aiUsage-readClaudeUsage-cda686
#[test]
fn no_file_and_no_keychain_is_missing_credentials() {
    let fixture = ClaudeFixture::new("");
    let result = fixture.read();
    assert_eq!(result.status, AiUsageStatus::Unavailable);
    assert_eq!(
        result.reason,
        Some(AiUsageFailureReason::MissingCredentials)
    );
    assert_eq!(fixture.usage.http.request_count(), 0);
}

// parity: TS-aiUsage-readClaudeUsage-8005a0
#[test]
fn keychain_token_is_used_without_file() {
    let fixture =
        ClaudeFixture::with_keychain("", Some(create_credentials_json("keychain-token", None)));
    fixture.respond(usage_payload());
    assert_eq!(fixture.read().status, AiUsageStatus::Ok);
    assert_eq!(fixture.sent_authorization(0), "Bearer keychain-token");
}

// parity: TS-aiUsage-readClaudeUsage-bef5b9
#[test]
fn fresher_keychain_wins_over_stale_file() {
    let fixture = ClaudeFixture::with_keychain(
        &create_credentials_json("stale-file-token", Some(unexpired_at_ms())),
        Some(create_credentials_json(
            "fresh-keychain-token",
            Some(unexpired_at_ms() + 1_000.0),
        )),
    );
    fixture.respond(usage_payload());
    fixture.read();
    assert_eq!(fixture.sent_authorization(0), "Bearer fresh-keychain-token");
}

// parity: TS-aiUsage-readClaudeUsage-5e97d2
#[test]
fn fresher_file_wins_over_stale_keychain() {
    let fixture = ClaudeFixture::with_keychain(
        &create_credentials_json("fresh-file-token", Some(unexpired_at_ms() + 1_000.0)),
        Some(create_credentials_json(
            "stale-keychain-token",
            Some(unexpired_at_ms()),
        )),
    );
    fixture.respond(usage_payload());
    fixture.read();
    assert_eq!(fixture.sent_authorization(0), "Bearer fresh-file-token");
}

// parity: TS-aiUsage-readClaudeUsage-86b0c9
#[test]
fn side_with_known_expiry_wins() {
    let fixture = ClaudeFixture::with_keychain(
        &create_credentials_json("file-token-without-expiry", None),
        Some(create_credentials_json(
            "keychain-token",
            Some(unexpired_at_ms()),
        )),
    );
    fixture.respond(usage_payload());
    fixture.read();
    assert_eq!(fixture.sent_authorization(0), "Bearer keychain-token");
}

// parity: TS-aiUsage-readClaudeUsage-224017
#[test]
fn unreadable_keychain_is_not_reported_as_signed_out() {
    let fixture = ClaudeFixture::with_keychain("", None);
    let result = fixture.read();
    assert_eq!(result.status, AiUsageStatus::Unavailable);
    assert_eq!(
        result.reason,
        Some(AiUsageFailureReason::KeychainUnreadable)
    );
    assert_eq!(fixture.usage.http.request_count(), 0);
}

// parity: TS-aiUsage-readClaudeUsage-e81617
#[test]
fn rejected_token_with_live_refresh_waits_for_cli() {
    let fixture = ClaudeFixture::new(&create_credentials_json(
        "rejected-token",
        Some(unexpired_at_ms()),
    ));
    fixture.usage.http.respond_json(401, json!({}));
    let result = fixture.read();
    assert_eq!(result.status, AiUsageStatus::Unavailable);
    assert_eq!(
        result.reason,
        Some(AiUsageFailureReason::AwaitingCliRefresh)
    );
}

// parity: TS-aiUsage-readClaudeUsage-55ca8d
#[test]
fn rejected_token_without_refresh_needs_sign_in() {
    let fixture = ClaudeFixture::new(&stringify(
        &json!({"claudeAiOauth": {"accessToken": "rejected-token", "expiresAt": unexpired_at_ms()}}),
    ));
    fixture.usage.http.respond_json(401, json!({}));
    let result = fixture.read();
    assert_eq!(result.status, AiUsageStatus::Unavailable);
    assert_eq!(
        result.reason,
        Some(AiUsageFailureReason::ExpiredCredentials)
    );
}

// parity: TS-aiUsage-readClaudeUsage-90f2ac
#[test]
fn rate_limit_blocks_calls_until_retry_after() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.stub_rate_limited(Some("290"));
    let first = fixture.read();
    let second = fixture.read();
    assert_eq!(first.reason, Some(AiUsageFailureReason::RateLimited));
    assert_eq!(second.reason, Some(AiUsageFailureReason::RateLimited));
    assert_eq!(fixture.usage.http.request_count(), 1);
}

// parity: TS-aiUsage-readClaudeUsage-a4fa8b
#[test]
fn calls_again_after_retry_after() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.usage.clock.set_iso("2026-08-15T10:00:00.000Z");
    fixture.stub_rate_limited(Some("60"));
    fixture.read();
    fixture.usage.clock.set_iso("2026-08-15T10:01:01.000Z");
    fixture.respond(usage_payload());
    assert_eq!(fixture.read().status, AiUsageStatus::Ok);
    assert_eq!(fixture.usage.http.request_count(), 2);
}

// parity: TS-aiUsage-readClaudeUsage-61edf7
#[test]
fn missing_retry_after_still_backs_off() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.usage.clock.set_iso("2026-08-15T10:00:00.000Z");
    fixture.stub_rate_limited(None);
    fixture.read();
    fixture.usage.clock.set_iso("2026-08-15T10:04:59.000Z");
    fixture.read();
    assert_eq!(fixture.usage.http.request_count(), 1);
}

// parity: TS-aiUsage-readClaudeUsage-d9a468
#[test]
fn model_weekly_limits_join_weekly_kind_with_model_name() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.respond(json!({
        "five_hour": {"utilization": 9, "resets_at": "2026-08-15T15:09:59.000Z"},
        "seven_day": {"utilization": 1, "resets_at": "2026-08-16T17:59:59.000Z"},
        "limits": [
            {"kind": "session", "percent": 9, "resets_at": "2026-08-15T15:09:59.000Z", "scope": null},
            {"kind": "weekly_all", "percent": 1, "resets_at": "2026-08-16T17:59:59.000Z", "scope": null},
            {"kind": "weekly_scoped", "percent": 4, "resets_at": null, "scope": {"model": {"id": null, "display_name": "Fable"}}}
        ]
    }));
    assert_eq!(
        fixture.read().windows,
        [
            window(
                AiUsageWindowKind::Session,
                None,
                9.0,
                Some("2026-08-15T15:09:59.000Z")
            ),
            window(
                AiUsageWindowKind::Weekly,
                None,
                1.0,
                Some("2026-08-16T17:59:59.000Z")
            ),
            window(AiUsageWindowKind::Weekly, Some("Fable"), 4.0, None),
        ]
    );
}

// parity: TS-aiUsage-readClaudeUsage-b30fbd
#[test]
fn unnamed_model_and_unknown_windows_are_dropped() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.respond(json!({"limits": [
        {"kind": "session", "percent": 9, "resets_at": null, "scope": null},
        {"kind": "weekly_scoped", "percent": 4, "resets_at": null, "scope": {"model": null}},
        {"kind": "nimbus_quill", "percent": 7, "resets_at": null, "scope": null}
    ]}));
    assert_eq!(
        fixture.read().windows,
        [window(AiUsageWindowKind::Session, None, 9.0, None)]
    );
}

// parity: TS-aiUsage-readClaudeUsage-0df62e
#[test]
fn subscription_type_becomes_plan_name() {
    let fixture = ClaudeFixture::new(&stringify(
        &json!({"claudeAiOauth": {"accessToken": "access-token", "subscriptionType": "max"}}),
    ));
    fixture.respond(usage_payload());
    assert_eq!(fixture.read().plan_name.as_deref(), Some("max"));
}

// parity: TS-aiUsage-readClaudeUsage-24b287
#[test]
fn empty_windows_are_an_empty_response_error() {
    let fixture = ClaudeFixture::new(&create_credentials_json("access-token", None));
    fixture.respond(json!({}));
    let result = fixture.read();
    assert_eq!(result.status, AiUsageStatus::Error);
    assert_eq!(result.reason, Some(AiUsageFailureReason::EmptyResponse));
}

#[test]
fn request_carries_cli_contract_headers() {
    let fixture = ClaudeFixture::new(&create_credentials_json(" access-token ", None));
    fixture.respond(usage_payload());
    fixture.read();
    let request = fixture.usage.http.request(0);
    assert_eq!(request.method, "GET");
    assert_eq!(request.timeout_ms, 10_000);
    assert_eq!(
        request.headers,
        [
            (
                "Authorization".to_string(),
                "Bearer access-token".to_string()
            ),
            ("anthropic-beta".to_string(), "oauth-2025-04-20".to_string()),
            ("User-Agent".to_string(), "claude-code/2.1.0".to_string()),
        ]
    );
}
