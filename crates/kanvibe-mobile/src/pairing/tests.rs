use kanvibe_process::request_scope::parse_bearer_token;
use serde_json::json;

use super::*;

const NOW: i64 = 1_800_000_000_000;

fn codes() -> PairingCodes {
    PairingCodes::default()
}

/// 발급된 코드와 반드시 다른 여섯 자리
fn wrong_code_for(code: &str) -> &'static str {
    if code == "000000" { "111111" } else { "000000" }
}

fn paired_device(device_id: &str, token: &str, device_name: &str) -> PairedDevice {
    PairedDevice {
        device_id: device_id.to_string(),
        token: token.to_string(),
        device_name: device_name.to_string(),
        paired_at: "2026-09-08T00:00:00.000Z".to_string(),
        notification: None,
    }
}

fn default_device() -> PairedDevice {
    paired_device("d1", &"a".repeat(64), "iPhone")
}

#[test]
fn pairing_code_is_six_digits() {
    let code = codes().issue(NOW).code;

    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|character| character.is_ascii_digit()));
}

#[test]
fn issuing_again_kills_the_previous_code() {
    let codes = codes();
    let first = codes.issue(NOW).code;
    let second = codes.issue(NOW).code;

    assert_eq!(codes.redeem(&first, NOW), first == second);
}

#[test]
fn matching_code_is_redeemed() {
    let codes = codes();
    let code = codes.issue(NOW).code;

    assert!(codes.redeem(&code, NOW));
}

#[test]
fn redeemed_code_cannot_be_used_again() {
    let codes = codes();
    let code = codes.issue(NOW).code;
    codes.redeem(&code, NOW);

    assert!(!codes.redeem(&code, NOW));
}

#[test]
fn wrong_code_does_not_consume_the_code() {
    let codes = codes();
    let code = codes.issue(NOW).code;

    assert!(!codes.redeem(wrong_code_for(&code), NOW));
    assert!(codes.redeem(&code, NOW));
}

#[test]
fn code_expires_five_minutes_after_issue() {
    let codes = codes();
    let code = codes.issue(NOW).code;

    assert_eq!(codes.read_pending(NOW + PAIRING_CODE_TTL_MS), None);
    assert!(!codes.redeem(&code, NOW + PAIRING_CODE_TTL_MS));
}

#[test]
fn closing_the_pairing_screen_revokes_the_code() {
    let codes = codes();
    let code = codes.issue(NOW).code;
    codes.revoke();

    assert!(!codes.redeem(&code, NOW));
}

#[test]
fn nothing_passes_without_an_issued_code() {
    assert!(!codes().redeem("000000", NOW));
}

#[test]
fn correct_code_still_works_just_below_the_failure_limit() {
    let codes = codes();
    let code = codes.issue(NOW).code;

    for _ in 0..PAIRING_CODE_MAX_FAILURES - 1 {
        assert!(!codes.redeem(wrong_code_for(&code), NOW));
    }

    assert!(codes.redeem(&code, NOW));
}

#[test]
fn five_failures_discard_the_code() {
    let codes = codes();
    let code = codes.issue(NOW).code;

    for _ in 0..PAIRING_CODE_MAX_FAILURES {
        assert!(!codes.redeem(wrong_code_for(&code), NOW));
    }

    assert_eq!(codes.read_pending(NOW), None);
    assert!(!codes.redeem(&code, NOW));
}

#[test]
fn new_code_restarts_the_failure_count() {
    let codes = codes();
    let stale = codes.issue(NOW).code;
    for _ in 0..PAIRING_CODE_MAX_FAILURES - 1 {
        codes.redeem(wrong_code_for(&stale), NOW);
    }

    let code = codes.issue(NOW).code;
    assert!(!codes.redeem(wrong_code_for(&code), NOW));
    assert!(codes.redeem(&code, NOW));
}

#[test]
fn device_token_is_long_hex() {
    let token = mint_device_token();

    assert_eq!(token.len(), 64);
    assert!(
        token
            .chars()
            .all(|character| matches!(character, '0'..='9' | 'a'..='f'))
    );
}

#[test]
fn two_device_tokens_differ() {
    assert_ne!(mint_device_token(), mint_device_token());
}

#[test]
fn device_ids_minted_at_the_same_moment_differ() {
    assert_ne!(mint_device_id(), mint_device_id());
}

#[test]
fn bearer_header_yields_only_the_token() {
    assert_eq!(parse_bearer_token(Some("Bearer abc123")), Some("abc123"));
}

#[test]
fn bearer_scheme_is_case_insensitive() {
    assert_eq!(parse_bearer_token(Some("bearer abc123")), Some("abc123"));
}

#[test]
fn missing_bearer_header_yields_none() {
    assert_eq!(parse_bearer_token(None), None);
    assert_eq!(parse_bearer_token(Some("")), None);
}

#[test]
fn non_bearer_scheme_is_rejected() {
    assert_eq!(parse_bearer_token(Some("Basic abc123")), None);
}

#[test]
fn finds_the_owner_of_a_stored_token() {
    let device = paired_device("d2", &"b".repeat(64), "iPad");
    let devices = [default_device(), device.clone()];

    assert_eq!(
        find_paired_device(&devices, Some(&"b".repeat(64))),
        Some(&device)
    );
}

#[test]
fn unknown_token_is_rejected() {
    assert_eq!(
        find_paired_device(&[default_device()], Some(&"c".repeat(64))),
        None
    );
}

#[test]
fn missing_token_is_rejected() {
    assert_eq!(find_paired_device(&[default_device()], None), None);
}

#[test]
fn token_of_a_different_length_is_rejected_without_panicking() {
    assert_eq!(find_paired_device(&[default_device()], Some("short")), None);
}

#[test]
fn serialized_device_list_reads_back_the_same() {
    let devices = vec![
        default_device(),
        paired_device("d1", &"d".repeat(64), "iPad"),
    ];

    assert_eq!(
        parse_paired_devices(Some(&serialize_paired_devices(&devices))),
        devices
    );
}

#[test]
fn missing_stored_value_is_an_empty_list() {
    assert_eq!(parse_paired_devices(None), Vec::new());
}

#[test]
fn broken_stored_value_does_not_fail() {
    assert_eq!(parse_paired_devices(Some("{not json")), Vec::new());
}

#[test]
fn malformed_entries_are_dropped_and_the_rest_kept() {
    let device = default_device();
    let mut empty_id = serde_json::to_value(&device).unwrap();
    empty_id["deviceId"] = json!("");
    let raw = json!([{ "token": 1 }, empty_id, device]).to_string();

    assert_eq!(parse_paired_devices(Some(&raw)), vec![default_device()]);
}
