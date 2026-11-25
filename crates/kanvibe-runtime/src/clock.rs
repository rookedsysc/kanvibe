//! 앱과 서비스가 함께 보는 현재 시각. QA 모드에서는 고정 시각에서 출발해 실제 경과 시간만큼만 흐른다.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use kanvibe_ai::usage::context::UsageClock;

#[derive(Debug, Clone, Copy)]
pub struct AppClock {
    fixed_origin_ms: Option<i64>,
    started_at: Instant,
}

impl AppClock {
    pub fn system() -> Self {
        Self {
            fixed_origin_ms: None,
            started_at: Instant::now(),
        }
    }

    pub fn fixed(origin_ms: i64) -> Self {
        Self {
            fixed_origin_ms: Some(origin_ms),
            started_at: Instant::now(),
        }
    }

    pub fn now_ms(&self) -> i64 {
        match self.fixed_origin_ms {
            Some(origin_ms) => origin_ms + self.started_at.elapsed().as_millis() as i64,
            None => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as i64)
                .unwrap_or_default(),
        }
    }
}

/// AI 사용량 조회의 `Date.now()`. QA 모드에서 사용량 초기화까지 남은 시간이 고정 시각 기준으로 계산된다
impl UsageClock for AppClock {
    fn now_ms(&self) -> f64 {
        AppClock::now_ms(self) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_starts_at_the_origin_and_moves_forward() {
        let clock = AppClock::fixed(1_768_446_000_000);
        let now = clock.now_ms();
        assert!((1_768_446_000_000..1_768_446_060_000).contains(&now));
    }
}
