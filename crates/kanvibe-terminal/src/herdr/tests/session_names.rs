//! Herdr 세션 이름 규칙. 규칙은 `kanvibe_process::worktree`에 있고, Herdr 연동이 기대하는 결과를 여기서 함께 확인한다.

use kanvibe_process::worktree::sanitize_herdr_session_name;
use regex::Regex;

// parity: TS-lib-herdr-0c27a3
#[test]
fn valid_session_names_are_kept() {
    for name in ["normal-branch".to_string(), "a".repeat(40)] {
        assert_eq!(sanitize_herdr_session_name(&name), name);
    }
}

// parity: TS-lib-herdr-b0b3ec
#[test]
fn out_of_rule_names_become_deterministic_ascii_names() {
    let allowed = Regex::new("^[a-zA-Z0-9._-]{1,40}$").unwrap();
    for name in [
        String::new(),
        ".".into(),
        "..".into(),
        "한글/브랜치".into(),
        "a".repeat(65),
    ] {
        let sanitized = sanitize_herdr_session_name(&name);
        assert!(allowed.is_match(&sanitized), "{name:?} → {sanitized}");
        assert_eq!(sanitize_herdr_session_name(&name), sanitized);
    }
}

// parity: TS-lib-herdr-bd475c
#[test]
fn names_sharing_a_truncated_prefix_stay_distinct() {
    let first = format!("{}x", "a".repeat(80));
    let second = format!("{}y", "a".repeat(80));
    assert_ne!(
        sanitize_herdr_session_name(&first),
        sanitize_herdr_session_name(&second)
    );
}
