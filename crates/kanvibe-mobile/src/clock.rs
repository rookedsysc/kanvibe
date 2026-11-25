//! 지금 시각. 만료·기록 간격을 재는 테스트가 시각을 고정할 수 있게 주입한다.

use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};

/// 유닉스 epoch 밀리초를 돌려주는 시계(TS `Date.now()`)
pub type EpochClock = dyn Fn() -> i64 + Send + Sync;

pub fn system_clock() -> Arc<EpochClock> {
    Arc::new(|| Utc::now().timestamp_millis())
}

/// JS `new Date(ms).toISOString()`과 같은 밀리초 UTC 형식
pub(crate) fn iso_timestamp(epoch_ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(epoch_ms)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}
