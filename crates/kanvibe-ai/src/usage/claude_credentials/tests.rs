use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::javascript::{parse_date_ms, stringify};
use crate::usage::context::ProcessOutput;
use crate::usage::test_context::{TestUsage, exited};

/// `/Users/tester/.claude`를 sha256으로 줄인 Claude Code 2.1+ 서비스 이름
const TESTER_SCOPED_SERVICE: &str = "Claude Code-credentials-ee16a9f4";
const LEGACY_SERVICE: &str = "Claude Code-credentials";
const TESTER_CONFIG_DIR: &str = "/Users/tester/.claude";

fn create_credentials_json(access_token: &str) -> String {
    stringify(
        &json!({"claudeAiOauth": {"accessToken": access_token, "refreshToken": "refresh-token"}}),
    )
}

/// Claude Code 2.1.x가 접미사 없는 항목에 MCP 서버 토큰만 남겨 둔 모양
fn mcp_only_credentials() -> String {
    stringify(&json!({"mcpOAuth": {"slack|38801a7d": {"serverName": "slack", "accessToken": ""}}}))
}

fn darwin() -> TestUsage {
    TestUsage::with("/Users/tester", UsagePlatform::Darwin, &[])
}

fn config_dirs(dirs: &[&str]) -> Vec<String> {
    dirs.iter().map(|dir| dir.to_string()).collect()
}

/// 서비스 이름별로 응답을 정하고, 목록에 없는 이름은 "항목 없음"으로 답한다
fn stub_keychain_items(
    usage: &TestUsage,
    items: &[(&str, Result<String, i32>)],
    dump_output: Option<&str>,
) {
    let items: HashMap<String, Result<String, i32>> = items
        .iter()
        .map(|(service, item)| (service.to_string(), item.clone()))
        .collect();
    let dump_output = dump_output.map(str::to_string);
    usage.processes.respond(move |request| {
        if request.args[0] == "dump-keychain" {
            return dump_output
                .as_deref()
                .map_or(exited(1, ""), |output| exited(0, output));
        }
        let service_index = request.args.iter().position(|arg| arg == "-s").unwrap() + 1;
        match items.get(&request.args[service_index]) {
            None => exited(44, ""),
            Some(Ok(credentials)) => exited(0, credentials),
            Some(Err(exit_code)) => exited(*exit_code, ""),
        }
    });
}

fn requested_services(usage: &TestUsage) -> Vec<String> {
    usage
        .processes
        .arguments()
        .into_iter()
        .filter(|args| args[0] == "find-generic-password")
        .map(|args| args[args.iter().position(|arg| arg == "-s").unwrap() + 1].clone())
        .collect()
}

fn read(usage: &TestUsage) -> ClaudeKeychainReadResult {
    read_claude_keychain_credentials(&usage.context, &config_dirs(&[TESTER_CONFIG_DIR]))
}

fn found(credentials: String) -> ClaudeKeychainReadResult {
    ClaudeKeychainReadResult::Found { credentials }
}

// parity: TS-aiUsage-claudeCredentials-1428d2
#[test]
fn reads_access_token_from_credentials() {
    assert_eq!(
        read_claude_access_token(&create_credentials_json("access-token")).as_deref(),
        Some("access-token")
    );
}

// parity: TS-aiUsage-claudeCredentials-5f85d4
#[test]
fn empty_broken_or_tokenless_credentials_have_no_token() {
    assert_eq!(read_claude_access_token(""), None);
    assert_eq!(read_claude_access_token("{not json"), None);
    assert_eq!(
        read_claude_access_token(&stringify(&json!({"claudeAiOauth": {}}))),
        None
    );
    assert_eq!(
        read_claude_access_token(&stringify(&json!({"claudeAiOauth": {"accessToken": "  "}}))),
        None
    );
}

// parity: TS-aiUsage-claudeCredentials-d575b9
#[test]
fn mcp_only_credentials_are_not_login_tokens() {
    assert_eq!(read_claude_access_token(&mcp_only_credentials()), None);
}

// parity: TS-aiUsage-claudeCredentials-75a6f4
#[test]
fn reads_expiry_from_credentials() {
    let credentials = stringify(&json!({"claudeAiOauth": {"expiresAt": 1_787_110_963_172_i64}}));
    assert_eq!(
        read_claude_token_expires_at(&credentials),
        Some(1_787_110_963_172.0)
    );
}

// parity: TS-aiUsage-claudeCredentials-6135b2
#[test]
fn missing_or_non_numeric_expiry_is_none() {
    assert_eq!(
        read_claude_token_expires_at(&create_credentials_json("access-token")),
        None
    );
    assert_eq!(read_claude_token_expires_at("{not json"), None);
    assert_eq!(
        read_claude_token_expires_at(&stringify(&json!({"claudeAiOauth": {"expiresAt": "곧"}}))),
        None
    );
}

// parity: TS-aiUsage-claudeCredentials-83c9c9
#[test]
fn token_is_expired_from_the_expiry_instant() {
    let now_ms = parse_date_ms("2026-08-15T10:00:00.000Z").unwrap();
    let at = |expires_at: f64| stringify(&json!({"claudeAiOauth": {"expiresAt": expires_at}}));
    assert!(is_claude_access_token_expired(&at(now_ms), now_ms));
    assert!(!is_claude_access_token_expired(&at(now_ms + 1.0), now_ms));
}

// parity: TS-aiUsage-claudeCredentials-fdc96a
#[test]
fn unknown_expiry_is_not_expired() {
    assert!(!is_claude_access_token_expired(
        &create_credentials_json("access-token"),
        0.0
    ));
    assert!(!is_claude_access_token_expired("{not json", 0.0));
}

// parity: TS-aiUsage-claudeCredentials-999bc4
#[test]
fn refresh_token_is_usable_without_or_before_expiry() {
    let now_ms = 1_000_000.0;
    let unexpired = stringify(
        &json!({"claudeAiOauth": {"refreshToken": "refresh-token", "refreshTokenExpiresAt": now_ms + 60_000.0}}),
    );
    assert!(has_usable_claude_refresh_token(
        &create_credentials_json("access-token"),
        now_ms
    ));
    assert!(has_usable_claude_refresh_token(&unexpired, now_ms));
}

// parity: TS-aiUsage-claudeCredentials-813494
#[test]
fn missing_or_expired_refresh_token_is_unusable() {
    let now_ms = 1_000_000.0;
    let expired = stringify(
        &json!({"claudeAiOauth": {"refreshToken": "refresh-token", "refreshTokenExpiresAt": now_ms - 1.0}}),
    );
    let access_only = stringify(&json!({"claudeAiOauth": {"accessToken": "access-token"}}));
    assert!(!has_usable_claude_refresh_token(&access_only, now_ms));
    assert!(!has_usable_claude_refresh_token(&expired, now_ms));
    assert!(!has_usable_claude_refresh_token("", now_ms));
}

// parity: TS-aiUsage-claudeCredentials-0691c9
#[test]
fn keychain_is_untouched_outside_macos() {
    let usage = TestUsage::new("/Users/tester");
    assert_eq!(read(&usage), ClaudeKeychainReadResult::Absent);
    assert_eq!(usage.processes.request_count(), 0);
}

// parity: TS-aiUsage-claudeCredentials-5ef9e2
#[test]
fn security_runs_by_absolute_path() {
    let usage = darwin();
    let credentials = create_credentials_json("keychain-token");
    usage.processes.respond(move |_| exited(0, &credentials));
    read(&usage);
    assert_eq!(
        usage.processes.requests.lock().unwrap()[0].program,
        "/usr/bin/security"
    );
}

// parity: TS-aiUsage-claudeCredentials-e95961
#[test]
fn scoped_service_is_read_with_current_user_first() {
    let usage = darwin();
    let credentials = create_credentials_json("scoped-token");
    let response = credentials.clone();
    usage.processes.respond(move |_| exited(0, &response));
    assert_eq!(read(&usage), found(credentials));
    assert_eq!(
        usage.processes.arguments()[0],
        [
            "find-generic-password",
            "-a",
            "tester",
            "-s",
            TESTER_SCOPED_SERVICE,
            "-w"
        ]
    );
    assert_eq!(usage.processes.request_count(), 1);
}

// parity: TS-aiUsage-claudeCredentials-34442d
#[test]
fn falls_back_to_legacy_service() {
    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[(LEGACY_SERVICE, Ok(create_credentials_json("legacy-token")))],
        None,
    );
    assert_eq!(read(&usage), found(create_credentials_json("legacy-token")));
    assert!(requested_services(&usage).contains(&LEGACY_SERVICE.to_string()));
}

// parity: TS-aiUsage-claudeCredentials-2ee599
#[test]
fn tokenless_item_does_not_hide_later_candidates() {
    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[
            (TESTER_SCOPED_SERVICE, Ok(mcp_only_credentials())),
            (LEGACY_SERVICE, Ok(create_credentials_json("legacy-token"))),
        ],
        None,
    );
    assert_eq!(read(&usage), found(create_credentials_json("legacy-token")));
}

// parity: TS-aiUsage-claudeCredentials-1b1f85
#[test]
fn no_login_token_anywhere_is_absent() {
    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[
            (TESTER_SCOPED_SERVICE, Ok(mcp_only_credentials())),
            (LEGACY_SERVICE, Ok(mcp_only_credentials())),
        ],
        None,
    );
    assert_eq!(read(&usage), ClaudeKeychainReadResult::Absent);
}

// parity: TS-aiUsage-claudeCredentials-e6913e
#[test]
fn retries_same_service_without_account() {
    let usage = darwin();
    let credentials = create_credentials_json("other-account-token");
    let response = credentials.clone();
    usage.processes.respond(move |request| {
        if request.args.iter().any(|arg| arg == "-a") {
            exited(44, "")
        } else {
            exited(0, &response)
        }
    });
    assert_eq!(read(&usage), found(credentials));
    assert_eq!(
        usage.processes.arguments()[1],
        ["find-generic-password", "-s", TESTER_SCOPED_SERVICE, "-w"]
    );
}

// parity: TS-aiUsage-claudeCredentials-ad4ae2
#[test]
fn every_config_dir_candidate_gets_a_scoped_lookup() {
    let usage = darwin();
    stub_keychain_items(&usage, &[], None);
    read_claude_keychain_credentials(
        &usage.context,
        &config_dirs(&[TESTER_CONFIG_DIR, "/Users/tester/.claude-work"]),
    );
    let services = requested_services(&usage);
    assert!(services.contains(&TESTER_SCOPED_SERVICE.to_string()));
    assert!(services.contains(&"Claude Code-credentials-c4394a73".to_string()));
}

// parity: TS-aiUsage-claudeCredentials-0dcdb0
#[test]
fn unguessed_names_are_found_in_the_stored_list() {
    let usage = darwin();
    let stored_service = "Claude Code-credentials-deadbeef";
    let dump = format!("    \"acct\"<blob>=\"tester\"\n    \"svce\"<blob>=\"{stored_service}\"\n");
    stub_keychain_items(
        &usage,
        &[(stored_service, Ok(create_credentials_json("stored-token")))],
        Some(&dump),
    );
    assert_eq!(read(&usage), found(create_credentials_json("stored-token")));
    assert!(requested_services(&usage).contains(&stored_service.to_string()));
}

// parity: TS-aiUsage-claudeCredentials-63b6ce
#[test]
fn listing_never_decrypts_and_is_skipped_when_guess_hits() {
    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[(
            TESTER_SCOPED_SERVICE,
            Ok(create_credentials_json("scoped-token")),
        )],
        None,
    );
    read(&usage);
    assert!(
        !usage
            .processes
            .arguments()
            .iter()
            .any(|args| args[0] == "dump-keychain")
    );

    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[],
        Some("    \"svce\"<blob>=\"Claude Code-credentials-deadbeef\"\n"),
    );
    read(&usage);
    let dump_args = usage
        .processes
        .arguments()
        .into_iter()
        .find(|args| args[0] == "dump-keychain");
    assert_eq!(dump_args, Some(vec!["dump-keychain".to_string()]));
}

// parity: TS-aiUsage-claudeCredentials-8d38c0
#[test]
fn other_apps_in_the_list_are_not_queried() {
    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[],
        Some(
            "    \"svce\"<blob>=\"com.apple.Safari\"\n    \"svce\"<blob>=\"GitHub - api.github.com\"\n",
        ),
    );
    read(&usage);
    assert_eq!(
        requested_services(&usage),
        [
            TESTER_SCOPED_SERVICE,
            TESTER_SCOPED_SERVICE,
            LEGACY_SERVICE,
            LEGACY_SERVICE
        ]
    );
}

// parity: TS-aiUsage-claudeCredentials-646288
#[test]
fn missing_item_means_not_logged_in() {
    let usage = darwin();
    usage.processes.respond(|_| exited(44, ""));
    assert_eq!(read(&usage), ClaudeKeychainReadResult::Absent);
}

// parity: TS-aiUsage-claudeCredentials-020cab
#[test]
fn denied_access_is_unreadable() {
    let usage = darwin();
    usage.processes.respond(|_| exited(36, ""));
    assert_eq!(read(&usage), ClaudeKeychainReadResult::Unreadable);
}

// parity: TS-aiUsage-claudeCredentials-bab60c
#[test]
fn unreadable_item_does_not_block_other_candidates() {
    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[
            (TESTER_SCOPED_SERVICE, Err(36)),
            (LEGACY_SERVICE, Ok(create_credentials_json("legacy-token"))),
        ],
        None,
    );
    assert_eq!(read(&usage), found(create_credentials_json("legacy-token")));
}

// parity: TS-aiUsage-claudeCredentials-f2b66f
#[test]
fn unreadable_item_is_not_retried_without_account() {
    let usage = darwin();
    stub_keychain_items(&usage, &[(TESTER_SCOPED_SERVICE, Err(36))], None);
    read(&usage);
    let scoped_lookups: Vec<String> = requested_services(&usage)
        .into_iter()
        .filter(|service| service == TESTER_SCOPED_SERVICE)
        .collect();
    assert_eq!(scoped_lookups, [TESTER_SCOPED_SERVICE]);
}

// parity: TS-aiUsage-claudeCredentials-ceab9d
#[test]
fn miss_report_lists_services_and_outcomes_only() {
    let usage = darwin();
    stub_keychain_items(
        &usage,
        &[(LEGACY_SERVICE, Ok(mcp_only_credentials()))],
        None,
    );
    read(&usage);
    let message = usage.warnings.lock().unwrap()[0].clone();
    assert!(message.contains(&format!("{TESTER_SCOPED_SERVICE}=absent")));
    assert!(message.contains(&format!("{LEGACY_SERVICE}=found")));
    assert!(!message.contains("mcpOAuth"));
}

// parity: TS-aiUsage-claudeCredentials-818051
#[test]
fn unknown_failures_like_timeouts_are_unreadable() {
    let usage = darwin();
    usage.processes.respond(|_| ProcessOutput::default());
    assert_eq!(read(&usage), ClaudeKeychainReadResult::Unreadable);
}

// parity: TS-aiUsage-claudeCredentials-5c40ea
#[test]
fn lookups_are_time_limited() {
    let usage = darwin();
    usage.processes.respond(|_| exited(0, ""));
    read(&usage);
    assert!(usage.processes.requests.lock().unwrap()[0].timeout_ms > 0);
}

#[test]
fn keychain_child_gets_only_the_narrow_environment() {
    let usage = TestUsage::with(
        "/Users/tester",
        UsagePlatform::Darwin,
        &[("KANVIBE_SECRET", "x"), ("PORT", "3000")],
    );
    usage.processes.respond(|_| exited(44, ""));
    read(&usage);
    let environment = usage.processes.requests.lock().unwrap()[0]
        .environment
        .clone();
    let names: Vec<&str> = environment.keys().map(String::as_str).collect();
    assert_eq!(names, ["HOME", "LOGNAME", "PATH", "USER"]);
    assert_eq!(environment["HOME"], "/Users/tester");
}
