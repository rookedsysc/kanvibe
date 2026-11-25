//! JS `Date`의 현지 시각 읽기(`getHours()` 등). TS 화면은 날짜를 `toLocaleString()`처럼 시스템 시간대로 보였다.
//! 앱은 시간대 라이브러리를 직접 쓰지 않으므로, JS `Date.parse` 규칙(오프셋 없는 ISO 시각은 현지 시각)을 따르는
//! `parse_date_ms`로 같은 벽시계 값을 현지로 다시 읽어 시간대 차이를 구한다.

use kanvibe_ai::javascript::{iso_string_from_ms, parse_date_ms};

/// 현지 시간대의 날짜·시각 조각
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDateTime {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalDateTime {
    /// epoch ms를 현지 벽시계로 읽는다
    pub fn from_epoch_ms(epoch_ms: f64) -> Option<Self> {
        let offset_ms = local_offset_ms(epoch_ms)?;
        let iso = iso_string_from_ms(epoch_ms + offset_ms)?;
        let number = |range: std::ops::Range<usize>| iso.get(range)?.parse::<u32>().ok();
        Some(Self {
            year: number(0..4)?,
            month: number(5..7)?,
            day: number(8..10)?,
            hour: number(11..13)?,
            minute: number(14..16)?,
            second: number(17..19)?,
        })
    }

    /// `5:07 AM`(en-US `hour: numeric, minute: 2-digit`)
    pub fn twelve_hour_minute(&self) -> String {
        format!(
            "{}:{:02} {}",
            self.twelve_hour(),
            self.minute,
            self.meridiem()
        )
    }

    /// `5:07:09 AM`
    pub fn twelve_hour_second(&self) -> String {
        format!(
            "{}:{:02}:{:02} {}",
            self.twelve_hour(),
            self.minute,
            self.second,
            self.meridiem()
        )
    }

    fn twelve_hour(&self) -> u32 {
        match self.hour % 12 {
            0 => 12,
            other => other,
        }
    }

    fn meridiem(&self) -> &'static str {
        if self.hour < 12 { "AM" } else { "PM" }
    }
}

/// 그 순간의 현지 시간대 오프셋. UTC 벽시계 값을 현지 시각으로 읽은 순간과의 차이다
fn local_offset_ms(epoch_ms: f64) -> Option<f64> {
    let utc_iso = iso_string_from_ms(epoch_ms)?;
    // `2026-09-08T00:00:00.000Z`에서 `Z`를 떼면 같은 벽시계 값의 현지 시각이다
    let wall_clock = utc_iso.strip_suffix('Z')?;
    Some(epoch_ms - parse_date_ms(wall_clock)?)
}
