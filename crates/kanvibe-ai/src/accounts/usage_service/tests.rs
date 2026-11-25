use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::*;
use crate::accounts::settings_store::memory::MemorySettings;
use crate::usage::test_context::FakeClock;
use crate::usage::types::{
    AiUsageFailureReason, AiUsageProvider, AiUsageStatus, AiUsageWindowKind,
};

/// 정해 둔 스냅샷을 돌려주고 몇 번 집계했는지 센다. 집계가 느린 조회를 흉내 낼 수 있다
#[derive(Default)]
struct ScriptedAggregator {
    snapshot: Mutex<Option<AiUsageSnapshot>>,
    calls: AtomicUsize,
    delay_ms: u64,
}

impl ScriptedAggregator {
    fn set(&self, snapshot: AiUsageSnapshot) {
        *self.snapshot.lock().unwrap() = Some(snapshot);
    }
}

impl UsageSnapshotAggregator for ScriptedAggregator {
    fn aggregate(&self, _: &[AiAccountRegistration]) -> AiUsageSnapshot {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(self.delay_ms));
        self.snapshot
            .lock()
            .unwrap()
            .clone()
            .expect("집계 결과가 정해지지 않았습니다")
    }
}

struct ServiceFixture {
    service: AiUsageService,
    store: Arc<MemorySettings>,
    aggregator: Arc<ScriptedAggregator>,
    clock: Arc<FakeClock>,
    warnings: Arc<Mutex<Vec<String>>>,
}

fn fixture_with_delay(delay_ms: u64) -> ServiceFixture {
    let store = Arc::new(MemorySettings::default());
    let aggregator = Arc::new(ScriptedAggregator {
        delay_ms,
        ..Default::default()
    });
    let usage = crate::usage::test_context::TestUsage::new("/home/tester");
    let clock = usage.clock.clone();
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let recorded = warnings.clone();
    let service = AiUsageService::new(
        store.clone(),
        aggregator.clone(),
        clock.clone(),
        Arc::new(move |message: &str| recorded.lock().unwrap().push(message.to_string())),
    );
    ServiceFixture {
        service,
        store,
        aggregator,
        clock,
        warnings,
    }
}

fn fixture() -> ServiceFixture {
    fixture_with_delay(0)
}

fn create_snapshot(used_percent: f64, resets_at: Option<&str>) -> AiUsageSnapshot {
    AiUsageSnapshot {
        fetched_at: "2026-08-10T06:00:00.000Z".to_string(),
        accounts: vec![AiUsageAccountResult {
            provider: AiUsageProvider::Claude,
            account_id: "account-uuid".to_string(),
            label: "me@example.com".to_string(),
            status: AiUsageStatus::Ok,
            plan_name: Some("max".to_string()),
            windows: vec![AiUsageWindow {
                kind: AiUsageWindowKind::Session,
                model_name: None,
                used_percent,
                resets_at: resets_at.map(str::to_string),
            }],
            reason: None,
            fetched_at: "2026-08-10T06:00:00.000Z".to_string(),
        }],
    }
}

/// 값을 하나도 얻지 못한 조회. 같은 계정이지만 창이 비어 있다
fn create_failed_snapshot(fetched_at: &str, reason: AiUsageFailureReason) -> AiUsageSnapshot {
    AiUsageSnapshot {
        fetched_at: fetched_at.to_string(),
        accounts: vec![AiUsageAccountResult {
            provider: AiUsageProvider::Claude,
            account_id: "account-uuid".to_string(),
            label: "me@example.com".to_string(),
            status: AiUsageStatus::Error,
            plan_name: None,
            windows: Vec::new(),
            reason: Some(reason),
            fetched_at: fetched_at.to_string(),
        }],
    }
}

const FAR_FUTURE: Option<&str> = Some("2999-01-01T00:00:00.000Z");

/// 직전 조회를 남긴 뒤 실패한 조회를 돌린 결과
fn carried_after_failure(
    first: AiUsageSnapshot,
    reason: AiUsageFailureReason,
) -> Arc<AiUsageSnapshot> {
    let fixture = fixture();
    fixture.aggregator.set(first);
    fixture.service.get_ai_usage_snapshot(false).unwrap();
    fixture
        .aggregator
        .set(create_failed_snapshot("2026-08-10T06:05:00.000Z", reason));
    fixture.service.get_ai_usage_snapshot(false).unwrap()
}

// parity: TS-main-aiUsageService-ab370d
#[test]
fn cache_is_empty_before_any_fetch() {
    assert_eq!(
        fixture().service.get_cached_ai_usage_snapshot().unwrap(),
        None
    );
}

// parity: TS-main-aiUsageService-66f004
#[test]
fn fetched_snapshot_is_stored_in_settings() {
    let fixture = fixture();
    fixture.aggregator.set(create_snapshot(22.0, None));
    fixture.service.get_ai_usage_snapshot(false).unwrap();
    let cached = fixture
        .service
        .get_cached_ai_usage_snapshot()
        .unwrap()
        .unwrap();
    assert_eq!(cached.accounts[0].windows[0].used_percent, 22.0);
    assert_eq!(cached.accounts[0].label, "me@example.com");
}

// parity: TS-main-aiUsageService-73ba2b
#[test]
fn cache_never_holds_raw_account_id() {
    let fixture = fixture();
    fixture.aggregator.set(create_snapshot(22.0, None));
    fixture.service.get_ai_usage_snapshot(false).unwrap();
    let stored: Vec<String> = fixture
        .store
        .values
        .lock()
        .unwrap()
        .values()
        .cloned()
        .collect();
    assert!(!stored.join(",").contains("account-uuid"));
}

// parity: TS-main-aiUsageService-7a8c14
#[test]
fn new_fetch_replaces_cache() {
    let fixture = fixture();
    fixture.aggregator.set(create_snapshot(22.0, None));
    fixture.service.get_ai_usage_snapshot(false).unwrap();
    fixture.aggregator.set(create_snapshot(80.0, None));
    fixture.service.get_ai_usage_snapshot(false).unwrap();
    let cached = fixture
        .service
        .get_cached_ai_usage_snapshot()
        .unwrap()
        .unwrap();
    assert_eq!(cached.accounts[0].windows[0].used_percent, 80.0);
}

// parity: TS-main-aiUsageService-2ba907
#[test]
fn failed_fetch_carries_last_values_and_keeps_reason() {
    let snapshot = carried_after_failure(
        create_snapshot(22.0, FAR_FUTURE),
        AiUsageFailureReason::RateLimited,
    );
    assert_eq!(snapshot.accounts[0].windows[0].used_percent, 22.0);
    assert_eq!(
        snapshot.accounts[0].reason,
        Some(AiUsageFailureReason::RateLimited)
    );
    assert_eq!(snapshot.accounts[0].plan_name.as_deref(), Some("max"));
}

// parity: TS-main-aiUsageService-092983
#[test]
fn sign_in_failures_are_not_carried() {
    let snapshot = carried_after_failure(
        create_snapshot(22.0, FAR_FUTURE),
        AiUsageFailureReason::ExpiredCredentials,
    );
    assert!(snapshot.accounts[0].windows.is_empty());
    assert_eq!(
        snapshot.accounts[0].reason,
        Some(AiUsageFailureReason::ExpiredCredentials)
    );
}

// parity: TS-main-aiUsageService-4f3e06
#[test]
fn awaiting_cli_refresh_carries_values_and_their_time() {
    let snapshot = carried_after_failure(
        create_snapshot(22.0, FAR_FUTURE),
        AiUsageFailureReason::AwaitingCliRefresh,
    );
    assert_eq!(snapshot.accounts[0].windows[0].used_percent, 22.0);
    assert_eq!(
        snapshot.accounts[0].reason,
        Some(AiUsageFailureReason::AwaitingCliRefresh)
    );
    assert_eq!(snapshot.accounts[0].fetched_at, "2026-08-10T06:00:00.000Z");
}

// parity: TS-main-aiUsageService-e1061e
#[test]
fn carried_values_keep_their_original_fetch_time() {
    let snapshot = carried_after_failure(
        create_snapshot(22.0, FAR_FUTURE),
        AiUsageFailureReason::RateLimited,
    );
    assert_eq!(snapshot.accounts[0].fetched_at, "2026-08-10T06:00:00.000Z");
    assert_eq!(snapshot.fetched_at, "2026-08-10T06:05:00.000Z");
}

// parity: TS-main-aiUsageService-832c02
#[test]
fn reset_windows_are_not_carried() {
    let snapshot = carried_after_failure(
        create_snapshot(22.0, Some("2026-08-10T06:01:00.000Z")),
        AiUsageFailureReason::RateLimited,
    );
    assert!(snapshot.accounts[0].windows.is_empty());
    assert_eq!(
        snapshot.accounts[0].reason,
        Some(AiUsageFailureReason::RateLimited)
    );
}

// parity: TS-main-aiUsageService-c8eb81
#[test]
fn failure_without_previous_value_stays_as_is() {
    let fixture = fixture();
    fixture.aggregator.set(create_failed_snapshot(
        "2026-08-10T06:05:00.000Z",
        AiUsageFailureReason::RateLimited,
    ));
    assert!(
        fixture
            .service
            .get_ai_usage_snapshot(false)
            .unwrap()
            .accounts[0]
            .windows
            .is_empty()
    );
}

// parity: TS-main-aiUsageService-b7cbdd
#[test]
fn overlapping_reusable_requests_share_one_aggregation() {
    let fixture = fixture_with_delay(50);
    fixture.aggregator.set(create_snapshot(22.0, None));
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.service.get_ai_usage_snapshot(true).unwrap());
        std::thread::sleep(Duration::from_millis(10));
        let second = scope.spawn(|| fixture.service.get_ai_usage_snapshot(true).unwrap());
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_eq!(fixture.aggregator.calls.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&first, &second));
}

// parity: TS-main-aiUsageService-44e56e
#[test]
fn reusable_requests_within_five_minutes_do_not_aggregate_again() {
    let fixture = fixture();
    fixture.clock.set_iso("2026-08-10T06:00:00.000Z");
    fixture.aggregator.set(create_snapshot(22.0, None));
    fixture.service.get_ai_usage_snapshot(true).unwrap();
    fixture.clock.set_iso("2026-08-10T06:04:59.000Z");
    fixture.aggregator.set(create_snapshot(80.0, None));
    let snapshot = fixture.service.get_ai_usage_snapshot(true).unwrap();
    assert_eq!(fixture.aggregator.calls.load(Ordering::SeqCst), 1);
    assert_eq!(snapshot.accounts[0].windows[0].used_percent, 22.0);
}

// parity: TS-main-aiUsageService-90e9b5
#[test]
fn reusable_requests_aggregate_again_after_five_minutes() {
    let fixture = fixture();
    fixture.clock.set_iso("2026-08-10T06:00:00.000Z");
    fixture.aggregator.set(create_snapshot(22.0, None));
    fixture.service.get_ai_usage_snapshot(true).unwrap();
    fixture.clock.set_iso("2026-08-10T06:05:00.000Z");
    fixture.aggregator.set(create_snapshot(80.0, None));
    let snapshot = fixture.service.get_ai_usage_snapshot(true).unwrap();
    assert_eq!(fixture.aggregator.calls.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot.accounts[0].windows[0].used_percent, 80.0);
}

// parity: TS-main-aiUsageService-4f2b64
#[test]
fn non_reusable_requests_always_aggregate() {
    let fixture = fixture();
    fixture.clock.set_iso("2026-08-10T06:00:00.000Z");
    fixture.aggregator.set(create_snapshot(22.0, None));
    fixture.service.get_ai_usage_snapshot(true).unwrap();
    fixture.aggregator.set(create_snapshot(80.0, None));
    let snapshot = fixture.service.get_ai_usage_snapshot(false).unwrap();
    assert_eq!(fixture.aggregator.calls.load(Ordering::SeqCst), 2);
    assert_eq!(snapshot.accounts[0].windows[0].used_percent, 80.0);
}

// parity: TS-main-aiUsageService-357104
#[test]
fn cache_write_failure_still_returns_result() {
    let fixture = fixture();
    fixture.aggregator.set(create_snapshot(22.0, None));
    fixture.store.is_read_only.store(true, Ordering::SeqCst);
    let snapshot = fixture.service.get_ai_usage_snapshot(false).unwrap();
    assert_eq!(snapshot.accounts[0].windows[0].used_percent, 22.0);
    assert!(!fixture.warnings.lock().unwrap().is_empty());
}
