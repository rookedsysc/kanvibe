//! 설정에 따라 다음 sync를 예약하는 루프(TS `startBackgroundTaskSync`). 켜져 있을 때만 타이머를 두고,
//! 꺼져 있는 동안은 타이머도 반복 설정 조회도 없이 기다린다.

use std::time::Duration;

use super::timer::TimerId;
use super::{BackgroundTaskSync, lock};

const INITIAL_SYNC_DELAY: Duration = Duration::from_millis(20_000);
const FALLBACK_SYNC_INTERVAL_MS: i64 = 10 * 60_000;

/// 시작한 루프의 상태. `generation`으로 멈춘 뒤 다시 시작한 루프와 앞선 루프의 예약을 가른다
pub(super) struct SchedulerState {
    generation: u64,
    is_enabled: bool,
    /// 루프가 직접 돌린 사이클이 진행 중이다. 그동안의 주기 변경은 사이클이 끝날 때 최신 주기를 읽어 반영한다
    is_cycle_running: bool,
    pending_timer: Option<TimerId>,
}

/// JS `setTimeout`처럼 음수 지연은 바로 실행한다
fn delay_of(interval_ms: i64) -> Duration {
    Duration::from_millis(interval_ms.max(0) as u64)
}

impl BackgroundTaskSync {
    /// 예약 루프를 시작한다. 이미 돌고 있으면 그 루프를 그대로 쓴다.
    /// 켜짐 여부는 시작할 때 한 번 읽고, 이후 변경은 `set_background_sync_enabled`가 알려 준다
    pub fn start_background_task_sync(&self) {
        let mut scheduler = lock(&self.shared.scheduler);
        if scheduler.is_some() {
            return;
        }
        let generation = self.next_generation();
        *scheduler = Some(SchedulerState {
            generation,
            is_enabled: false,
            is_cycle_running: false,
            pending_timer: None,
        });
        drop(scheduler);
        match self.shared.settings.is_enabled() {
            Ok(is_enabled) => self.apply_enabled_change(is_enabled),
            Err(error) => eprintln!("[background-task-sync] settings load failed: {error}"),
        }
    }

    fn next_generation(&self) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static GENERATION: AtomicU64 = AtomicU64::new(0);
        GENERATION.fetch_add(1, Ordering::SeqCst)
    }

    pub fn stop_background_task_sync(&self) {
        let stopped = lock(&self.shared.scheduler).take();
        if let Some(timer_id) = stopped.and_then(|state| state.pending_timer) {
            self.shared.timer.cancel(timer_id);
        }
    }

    /// 켜짐 여부를 저장하고 루프에 바로 반영한다
    pub fn set_background_sync_enabled(&self, is_enabled: bool) -> Result<(), String> {
        self.shared.settings.save_enabled(is_enabled)?;
        self.apply_enabled_change(is_enabled);
        Ok(())
    }

    /// 주기를 저장하고, 루프가 기다리는 중이면 새 주기로 다시 예약한다
    pub fn set_background_sync_interval_ms(&self, interval_ms: i64) -> Result<(), String> {
        self.shared.settings.save_interval_ms(interval_ms)?;
        self.apply_interval_change(interval_ms);
        Ok(())
    }

    /// 켜면 20초 뒤 첫 sync를 예약하고, 끄면 예약을 취소한다. 루프 사이클이 도는 중이면 끝날 때 판단한다
    fn apply_enabled_change(&self, is_enabled: bool) {
        let mut scheduler = lock(&self.shared.scheduler);
        let Some(state) = scheduler.as_mut() else {
            return;
        };
        state.is_enabled = is_enabled;
        if let Some(timer_id) = state.pending_timer.take() {
            self.shared.timer.cancel(timer_id);
        }
        if is_enabled && !state.is_cycle_running {
            state.pending_timer = Some(self.schedule_cycle(state.generation, INITIAL_SYNC_DELAY));
        }
    }

    fn apply_interval_change(&self, interval_ms: i64) {
        let mut scheduler = lock(&self.shared.scheduler);
        let Some(state) = scheduler
            .as_mut()
            .filter(|state| state.is_enabled && !state.is_cycle_running)
        else {
            return;
        };
        if let Some(timer_id) = state.pending_timer.take() {
            self.shared.timer.cancel(timer_id);
        }
        state.pending_timer = Some(self.schedule_cycle(state.generation, delay_of(interval_ms)));
    }

    fn schedule_cycle(&self, generation: u64, delay: Duration) -> TimerId {
        let service = self.clone();
        self.shared.timer.schedule(
            delay,
            Box::new(move || service.run_scheduled_cycle(generation)),
        )
    }

    /// 예약이 발화하면 같은 루프가 켜져 있을 때만 사이클을 돌리고, 끝난 뒤 최신 주기로 다음 사이클을 예약한다
    fn run_scheduled_cycle(&self, generation: u64) {
        if !self.mark_cycle_running(generation) {
            return;
        }
        if let Err(error) = self.run_shared_cycle() {
            eprintln!("[background-task-sync] sync failed: {error}");
        }
        let should_schedule = lock(&self.shared.scheduler)
            .as_ref()
            .is_some_and(|state| state.generation == generation && state.is_enabled);
        let interval_ms = should_schedule.then(|| {
            self.shared
                .settings
                .interval_ms()
                .unwrap_or(FALLBACK_SYNC_INTERVAL_MS)
        });
        let mut scheduler = lock(&self.shared.scheduler);
        let Some(state) = scheduler
            .as_mut()
            .filter(|state| state.generation == generation)
        else {
            return;
        };
        state.is_cycle_running = false;
        if let Some(interval_ms) = interval_ms.filter(|_| state.is_enabled) {
            state.pending_timer = Some(self.schedule_cycle(generation, delay_of(interval_ms)));
        }
    }

    fn mark_cycle_running(&self, generation: u64) -> bool {
        let mut scheduler = lock(&self.shared.scheduler);
        let Some(state) = scheduler
            .as_mut()
            .filter(|state| state.generation == generation && state.is_enabled)
        else {
            return false;
        };
        state.pending_timer = None;
        state.is_cycle_running = true;
        true
    }
}
