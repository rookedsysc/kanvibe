//! 사용량 패널이 부르는 서비스(TS `aiUsageService`). 창과 패널이 마지막 결과와 진행 중인 조회를 나눠 써서
//! 사용량 엔드포인트가 잠기지 않게 하고, 조회에 실패한 계정에는 직전 값을 이어 붙인다.

use std::sync::{Arc, Condvar, Mutex};

use crate::accounts::account_service::get_ai_account_registrations;
use crate::accounts::settings_store::AppSettingStore;
use crate::error::AiResult;
use crate::javascript::parse_date_ms;
use crate::usage::account_registry::AiAccountRegistration;
use crate::usage::context::UsageContext;
use crate::usage::context::{UsageClock, WarningSink};
use crate::usage::shared::{AI_USAGE_REFRESH_INTERVAL_MS, SIGN_IN_FAILURE_REASONS};
use crate::usage::types::{AiUsageAccountResult, AiUsageSnapshot, AiUsageWindow};
use crate::usage::usage_aggregation::aggregate_ai_usage;
use crate::usage::usage_snapshot_cache::{
    from_cached_snapshot, to_cacheable_snapshot, to_cached_account_id,
};

/// 앱을 껐다 켜도 패널이 빈 화면으로 열리지 않도록 마지막 조회 결과를 남겨 두는 자리
pub const AI_USAGE_SNAPSHOT_CACHE_KEY: &str = "ai_usage_snapshot_cache";

type SnapshotOutcome = AiResult<Arc<AiUsageSnapshot>>;

/// 진행 중인 조회 하나. 끝나면 결과를 담고 기다리던 호출을 깨운다
#[derive(Default)]
struct SnapshotRequest {
    outcome: Mutex<Option<SnapshotOutcome>>,
    finished: Condvar,
}

impl SnapshotRequest {
    fn wait(&self) -> SnapshotOutcome {
        let mut outcome = self
            .outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while outcome.is_none() {
            outcome = self
                .finished
                .wait(outcome)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        outcome.clone().expect("결과가 담긴 뒤에만 빠져나온다")
    }

    fn finish(&self, result: SnapshotOutcome) {
        *self
            .outcome
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
        self.finished.notify_all();
    }
}

/// 앱이 떠 있는 동안만 의미가 있는 값이라 저장하지 않고 서비스가 기억한다
#[derive(Default)]
struct SharedSnapshots {
    recent: Option<Arc<AiUsageSnapshot>>,
    in_flight: Option<Arc<SnapshotRequest>>,
}

/// 등록 계정 전체의 사용량을 모으는 쪽(TS `aggregateAiUsage`). 앱에서는 [`UsageContext`]가 맡는다
pub trait UsageSnapshotAggregator: Send + Sync {
    fn aggregate(&self, registrations: &[AiAccountRegistration]) -> AiUsageSnapshot;
}

impl UsageSnapshotAggregator for UsageContext {
    fn aggregate(&self, registrations: &[AiAccountRegistration]) -> AiUsageSnapshot {
        aggregate_ai_usage(self, registrations)
    }
}

pub struct AiUsageService {
    store: Arc<dyn AppSettingStore>,
    aggregator: Arc<dyn UsageSnapshotAggregator>,
    clock: Arc<dyn UsageClock>,
    warn: WarningSink,
    shared: Mutex<SharedSnapshots>,
}

/// 이미 초기화된 창의 사용률은 지금 값이 아니다. 이월할 수 있는 건 아직 살아 있는 창뿐이다
fn select_live_windows(windows: &[AiUsageWindow], now_ms: Option<f64>) -> Vec<AiUsageWindow> {
    windows
        .iter()
        .filter(|window| match window.resets_at.as_deref().filter(|resets_at| !resets_at.is_empty()) {
            None => true,
            Some(resets_at) => matches!((parse_date_ms(resets_at), now_ms), (Some(reset), Some(now)) if reset > now),
        })
        .cloned()
        .collect()
}

/// 다시 로그인하기 전까지 계속 실패하는 사유에는 이월이 옛 값을 새 값인 척하게만 만든다
fn is_sign_in_failure(account: &AiUsageAccountResult) -> bool {
    account
        .reason
        .is_some_and(|reason| SIGN_IN_FAILURE_REASONS.contains(&reason))
}

fn with_last_known_windows(
    account: AiUsageAccountResult,
    cached: &AiUsageSnapshot,
    now_ms: Option<f64>,
) -> AiUsageAccountResult {
    if !account.windows.is_empty() || is_sign_in_failure(&account) {
        return account;
    }
    let cached_account_id = to_cached_account_id(&account.account_id);
    let Some(cached_account) = cached.accounts.iter().find(|candidate| {
        candidate.provider == account.provider && candidate.account_id == cached_account_id
    }) else {
        return account;
    };
    let live_windows = select_live_windows(&cached_account.windows, now_ms);
    if live_windows.is_empty() {
        return account;
    }
    // 조회 시각은 값이 실제로 만들어진 때로 되돌린다. 실패한 조회의 시각을 붙이면 새 값처럼 보인다
    AiUsageAccountResult {
        plan_name: account
            .plan_name
            .clone()
            .or_else(|| cached_account.plan_name.clone()),
        windows: live_windows,
        fetched_at: cached_account.fetched_at.clone(),
        ..account
    }
}

/// 조회에 실패한 계정에 직전 조회의 값을 이어 붙인다. 실패 사유는 지우지 않아 화면이 옛 값을 새 값인 척하지 않게 한다
fn carry_last_known_usage(
    snapshot: AiUsageSnapshot,
    cached: Option<AiUsageSnapshot>,
) -> AiUsageSnapshot {
    let Some(cached) = cached else {
        return snapshot;
    };
    let now_ms = parse_date_ms(&snapshot.fetched_at);
    AiUsageSnapshot {
        accounts: snapshot
            .accounts
            .into_iter()
            .map(|account| with_last_known_windows(account, &cached, now_ms))
            .collect(),
        fetched_at: snapshot.fetched_at,
    }
}

impl AiUsageService {
    pub fn new(
        store: Arc<dyn AppSettingStore>,
        aggregator: Arc<dyn UsageSnapshotAggregator>,
        clock: Arc<dyn UsageClock>,
        warn: WarningSink,
    ) -> Self {
        Self {
            store,
            aggregator,
            clock,
            warn,
            shared: Mutex::new(SharedSnapshots::default()),
        }
    }

    /// 마지막으로 저장된 조회 결과. 저장된 값이 없거나 형식이 맞지 않으면 `None`
    pub fn get_cached_ai_usage_snapshot(&self) -> AiResult<Option<AiUsageSnapshot>> {
        Ok(from_cached_snapshot(
            self.store
                .get_app_setting(AI_USAGE_SNAPSHOT_CACHE_KEY)?
                .as_deref(),
        ))
    }

    fn fetch_ai_usage_snapshot(&self) -> SnapshotOutcome {
        let registrations = get_ai_account_registrations(self.store.as_ref())?;
        let aggregated = self.aggregator.aggregate(&registrations);
        let snapshot = carry_last_known_usage(aggregated, self.get_cached_ai_usage_snapshot()?);
        // 캐시는 다음 조회를 빠르게 보여 주기 위한 편의값이라 저장 실패로 조회까지 실패시키지 않는다
        if let Err(error) = self.store.set_app_setting(
            AI_USAGE_SNAPSHOT_CACHE_KEY,
            &to_cacheable_snapshot(&snapshot),
        ) {
            (self.warn)(&format!(
                "[ai-usage] 사용량 스냅샷을 캐시에 저장하지 못했습니다: {error}"
            ));
        }
        Ok(Arc::new(snapshot))
    }

    /// 시계가 뒤로 돌아가 조회 시각이 미래가 된 결과는 얼마나 묵었는지 알 수 없으므로 신선하다고 보지 않는다
    fn is_within_refresh_interval(&self, snapshot: &AiUsageSnapshot) -> bool {
        let Some(fetched_at) = parse_date_ms(&snapshot.fetched_at) else {
            return false;
        };
        let elapsed_ms = self.clock.now_ms() - fetched_at;
        (0.0..AI_USAGE_REFRESH_INTERVAL_MS).contains(&elapsed_ms)
    }

    /// `can_reuse_recent_snapshot`이 없으면 언제나 새로 조회하고 진행 중인 조회에도 합류하지 않는다.
    /// 새로고침이나 로그인 직후처럼 사람이 방금 바꾼 상태를 봐야 하는 호출이 이쪽이다
    pub fn get_ai_usage_snapshot(&self, can_reuse_recent_snapshot: bool) -> SnapshotOutcome {
        let request = {
            let mut shared = self
                .shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if can_reuse_recent_snapshot {
                if let Some(in_flight) = shared.in_flight.clone() {
                    drop(shared);
                    return in_flight.wait();
                }
                if let Some(recent) = shared
                    .recent
                    .clone()
                    .filter(|recent| self.is_within_refresh_interval(recent))
                {
                    return Ok(recent);
                }
            }
            let request = Arc::new(SnapshotRequest::default());
            shared.in_flight = Some(request.clone());
            request
        };
        let outcome = self.fetch_ai_usage_snapshot();
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Ok(snapshot) = &outcome {
            shared.recent = Some(snapshot.clone());
        }
        if shared
            .in_flight
            .as_ref()
            .is_some_and(|in_flight| Arc::ptr_eq(in_flight, &request))
        {
            shared.in_flight = None;
        }
        drop(shared);
        request.finish(outcome.clone());
        outcome
    }
}

#[cfg(test)]
mod tests;
