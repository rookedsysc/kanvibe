use std::sync::Arc;

use super::*;
use crate::test_support::{TestDatabase, open_test_database};

const DAY_MS: i64 = 24 * 60 * 60_000;
const NOW: i64 = 100 * DAY_MS;

fn usage() -> (LanPlaintextUsage, TestDatabase) {
    let test_database = open_test_database();
    let usage = LanPlaintextUsage::new(Arc::clone(&test_database.database), false);
    (usage, test_database)
}

fn set_observed_since(test_database: &TestDatabase, observed_since: i64) {
    open_app_settings(&test_database.database)
        .get_or_start_lan_plaintext_observation(observed_since)
        .unwrap();
}

fn last_request_at(test_database: &TestDatabase) -> i64 {
    open_app_settings(&test_database.database)
        .get_lan_plaintext_last_request_at()
        .unwrap()
}

// parity: TS-main-lanPlaintextService-627d92
#[test]
fn first_run_opens_lan_while_measuring_usage() {
    let (usage, test_database) = usage();

    assert_eq!(usage.resolve_hook_server_bind_host(NOW), LAN_BIND_HOST);
    assert_eq!(
        open_app_settings(&test_database.database)
            .get_or_start_lan_plaintext_observation(NOW + DAY_MS)
            .unwrap(),
        NOW
    );
}

// parity: TS-main-lanPlaintextService-2cfc93
#[test]
fn fourteen_idle_days_close_to_loopback() {
    let (usage, test_database) = usage();
    set_observed_since(&test_database, NOW - 14 * DAY_MS);

    assert_eq!(usage.resolve_hook_server_bind_host(NOW), LOOPBACK_BIND_HOST);
}

// parity: TS-main-lanPlaintextService-6cb7c6
#[test]
fn recent_lan_request_keeps_lan_open() {
    let (usage, test_database) = usage();
    set_observed_since(&test_database, NOW - 30 * DAY_MS);
    open_app_settings(&test_database.database)
        .set_lan_plaintext_last_request_at(NOW - 13 * DAY_MS)
        .unwrap();

    assert_eq!(usage.resolve_hook_server_bind_host(NOW), LAN_BIND_HOST);
}

// parity: TS-main-lanPlaintextService-bf22ff
#[test]
fn user_allowance_opens_lan_regardless_of_usage() {
    let (usage, test_database) = usage();
    set_observed_since(&test_database, NOW - 30 * DAY_MS);
    open_app_settings(&test_database.database)
        .set_lan_plaintext_allowed(true)
        .unwrap();

    assert_eq!(usage.resolve_hook_server_bind_host(NOW), LAN_BIND_HOST);
}

// parity: TS-main-lanPlaintextService-ef4ac4
#[test]
fn unreadable_settings_keep_lan_open() {
    let (usage, test_database) = usage();
    test_database
        .database
        .connection()
        .execute_batch("DROP TABLE app_settings")
        .unwrap();

    assert_eq!(usage.resolve_hook_server_bind_host(NOW), LAN_BIND_HOST);
}

// parity: TS-main-lanPlaintextService-001d93
#[test]
fn loopback_requests_are_not_lan_usage() {
    for address in ["127.0.0.1", "::1", "::ffff:127.0.0.1"] {
        let (usage, test_database) = usage();

        usage.record_lan_plaintext_response(Some(address), Some("/api/hooks/status"), 200, NOW);

        assert_eq!(last_request_at(&test_database), 0, "{address}");
    }
}

// parity: TS-main-lanPlaintextService-905437
#[test]
fn lan_requests_are_recorded_at_most_once_an_hour() {
    let (usage, test_database) = usage();
    let address = Some("192.168.0.20");
    let path = Some("/api/hooks/status");

    usage.record_lan_plaintext_response(address, path, 200, NOW);
    assert_eq!(last_request_at(&test_database), NOW);
    usage.record_lan_plaintext_response(address, path, 200, NOW + 1000);
    assert_eq!(last_request_at(&test_database), NOW);
    usage.record_lan_plaintext_response(address, path, 200, NOW + 60 * 60_000);
    assert_eq!(last_request_at(&test_database), NOW + 60 * 60_000);
}

// parity: TS-main-lanPlaintextService-157bf6
#[test]
fn failed_and_health_requests_do_not_keep_the_port_open() {
    let cases = [
        ("/wp-login.php", 404),
        ("/api/mobile/board", 401),
        ("/api/hooks/health", 200),
    ];
    for (request_url, status_code) in cases {
        let (usage, test_database) = usage();

        usage.record_lan_plaintext_response(
            Some("192.168.0.20"),
            Some(request_url),
            status_code,
            NOW,
        );

        assert_eq!(last_request_at(&test_database), 0, "{request_url}");
    }
}
