use serde_json::json;

use super::*;

#[test]
fn decode_rejects_non_string_and_misaligned_payloads_before_decoding() {
    let invalid = Err("Invalid terminal payload".to_string());
    assert_eq!(decode_remote_terminal_bytes(None, 10), invalid);
    assert_eq!(decode_remote_terminal_bytes(Some(&json!(12)), 10), invalid);
    assert_eq!(
        decode_remote_terminal_bytes(Some(&json!("abc")), 10),
        invalid
    );
    // 상한 3바이트면 base64 4글자까지만 받는다
    assert_eq!(
        decode_remote_terminal_bytes(Some(&json!("YWJjZA==")), 3),
        invalid
    );
}

#[test]
fn decode_rejects_non_canonical_base64_and_oversized_bytes() {
    let too_large = Err("Terminal payload too large".to_string());
    assert_eq!(
        decode_remote_terminal_bytes(Some(&json!("YQ-_")), 10),
        too_large
    );
    assert_eq!(
        decode_remote_terminal_bytes(Some(&json!("YR==")), 10),
        too_large
    );
    assert_eq!(
        decode_remote_terminal_bytes(Some(&json!("YWI=")), 1),
        too_large
    );
}

#[test]
fn decode_returns_bytes_for_canonical_payload_within_limit() {
    assert_eq!(
        decode_remote_terminal_bytes(Some(&json!("YWJj")), 3),
        Ok(b"abc".to_vec())
    );
    assert_eq!(
        decode_remote_terminal_bytes(Some(&json!("")), 3),
        Ok(Vec::new())
    );
}

#[test]
fn remote_terminal_size_accepts_only_integers_within_dimension_limit() {
    assert!(is_remote_terminal_size(Some(&json!(1)), Some(&json!(500))));
    assert!(is_remote_terminal_size(
        Some(&json!(80.0)),
        Some(&json!(24))
    ));
    assert!(!is_remote_terminal_size(Some(&json!(0)), Some(&json!(24))));
    assert!(!is_remote_terminal_size(
        Some(&json!(80)),
        Some(&json!(501))
    ));
    assert!(!is_remote_terminal_size(
        Some(&json!(80.5)),
        Some(&json!(24))
    ));
    assert!(!is_remote_terminal_size(
        Some(&json!("80")),
        Some(&json!(24))
    ));
    assert!(!is_remote_terminal_size(None, Some(&json!(24))));
}

#[test]
fn herdr_terminal_record_marks_broken_lines_and_payloads_invalid() {
    assert_eq!(
        parse_herdr_terminal_record("{broken"),
        Some(HerdrTerminalRecord::Invalid)
    );
    assert_eq!(
        parse_herdr_terminal_record("null"),
        Some(HerdrTerminalRecord::Invalid)
    );
    assert_eq!(
        parse_herdr_terminal_record(r#"{"type":"terminal.frame","bytes":"abc"}"#),
        Some(HerdrTerminalRecord::Invalid)
    );
    assert_eq!(parse_herdr_terminal_record("7"), None);
}

#[test]
fn herdr_terminal_record_keeps_raw_bytes_and_size_only_when_both_are_numbers() {
    let record = parse_herdr_terminal_record(
        r#"{"type":"terminal.frame","bytes":"/w==","full":"yes","width":10}"#,
    );
    assert_eq!(
        record,
        Some(HerdrTerminalRecord::Frame {
            is_full_screen: false,
            bytes: vec![0xff],
            size: None,
        })
    );
}

#[test]
fn observe_record_replaces_invalid_utf8_like_node() {
    let record = parse_herdr_observe_record(r#"{"type":"terminal.frame","bytes":"Yf9i"}"#);
    assert_eq!(
        record,
        Some(HerdrObserveRecord::Frame {
            is_full_screen: false,
            output: "a\u{FFFD}b".to_string(),
            size: None,
        })
    );
}

#[test]
fn screen_redraw_keeps_trailing_carriage_return_not_followed_by_line_feed() {
    assert_eq!(build_screen_redraw("a\r", None), "\x1b[H\x1b[2J\x1b[3Ja\r");
}

#[test]
fn grant_serializes_with_camel_case_keys() {
    let grant = RemoteTerminalGrant {
        version: REMOTE_TERMINAL_GRANT_VERSION,
        grant_id: "g".to_string(),
        direct: RemoteTerminalDirectRoute {
            host: "h".to_string(),
            port: 1,
            path: "/stream/g".to_string(),
            token: "t".to_string(),
            certificate_fingerprint: "f".to_string(),
        },
        expires_at: 5,
    };
    assert_eq!(
        serde_json::to_string(&grant).unwrap(),
        r#"{"version":1,"grantId":"g","direct":{"host":"h","port":1,"path":"/stream/g","token":"t","certificateFingerprint":"f"},"expiresAt":5}"#
    );
}
