//! d3 시간 축: `scaleTime().ticks()`의 눈금 간격 고르기, 간격별 눈금 시각, `timeFormat()`(strftime) 글자.

use super::calendar::{
    CivilTime, MILLIS_PER_DAY, MILLIS_PER_HOUR, MILLIS_PER_MINUTE, MILLIS_PER_SECOND,
    MILLIS_PER_WEEK, MONTH_NAMES, WEEKDAY_NAMES, add_months, start_of_day, weekday_of,
};

/// d3 기본 눈금 개수
const DEFAULT_TICK_COUNT: f64 = 10.0;
/// mermaid `MAX_TICK_COUNT`: 사용자 눈금 간격이 이보다 많은 눈금을 만들면 기본 눈금으로 돌아간다
pub const MAX_TICK_COUNT: i64 = 10_000;
const MILLIS_PER_MONTH: i64 = 30 * MILLIS_PER_DAY;
const MILLIS_PER_YEAR: i64 = 365 * MILLIS_PER_DAY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeUnit {
    Millisecond,
    Second,
    Minute,
    Hour,
    Day,
    /// 주의 시작 요일(0 = 일요일)
    Week(u32),
    Month,
    Year,
}

/// `unit.every(step)`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickInterval {
    pub unit: TimeUnit,
    pub step: i64,
}

/// d3-time `tickIntervals`
const TICK_INTERVALS: [(TimeUnit, i64, i64); 18] = [
    (TimeUnit::Second, 1, MILLIS_PER_SECOND),
    (TimeUnit::Second, 5, 5 * MILLIS_PER_SECOND),
    (TimeUnit::Second, 15, 15 * MILLIS_PER_SECOND),
    (TimeUnit::Second, 30, 30 * MILLIS_PER_SECOND),
    (TimeUnit::Minute, 1, MILLIS_PER_MINUTE),
    (TimeUnit::Minute, 5, 5 * MILLIS_PER_MINUTE),
    (TimeUnit::Minute, 15, 15 * MILLIS_PER_MINUTE),
    (TimeUnit::Minute, 30, 30 * MILLIS_PER_MINUTE),
    (TimeUnit::Hour, 1, MILLIS_PER_HOUR),
    (TimeUnit::Hour, 3, 3 * MILLIS_PER_HOUR),
    (TimeUnit::Hour, 6, 6 * MILLIS_PER_HOUR),
    (TimeUnit::Hour, 12, 12 * MILLIS_PER_HOUR),
    (TimeUnit::Day, 1, MILLIS_PER_DAY),
    (TimeUnit::Day, 2, 2 * MILLIS_PER_DAY),
    (TimeUnit::Week(0), 1, MILLIS_PER_WEEK),
    (TimeUnit::Month, 1, MILLIS_PER_MONTH),
    (TimeUnit::Month, 3, 3 * MILLIS_PER_MONTH),
    (TimeUnit::Year, 1, MILLIS_PER_YEAR),
];

/// d3-array `tickStep`: 1·2·5·10 × 10의 거듭제곱 중 가까운 간격
fn tick_step(start: f64, stop: f64, count: f64) -> f64 {
    let step = (stop - start) / count;
    let power = step.log10().floor();
    let error = step / 10f64.powf(power);
    let factor = if error >= 50f64.sqrt() {
        10.0
    } else if error >= 10f64.sqrt() {
        5.0
    } else if error >= 2f64.sqrt() {
        2.0
    } else {
        1.0
    };
    factor * 10f64.powf(power)
}

/// d3-time `tickInterval(start, stop, 10)`. 간격을 만들 수 없으면 `None`
fn default_interval(start: i64, stop: i64) -> Option<TickInterval> {
    let target = (stop - start) as f64 / DEFAULT_TICK_COUNT;
    let index = TICK_INTERVALS
        .iter()
        .take_while(|(_, _, duration)| *duration as f64 <= target)
        .count();
    if index == TICK_INTERVALS.len() {
        let year = MILLIS_PER_YEAR as f64;
        let step = tick_step(start as f64 / year, stop as f64 / year, DEFAULT_TICK_COUNT);
        return every(TimeUnit::Year, step);
    }
    if index == 0 {
        let step = tick_step(start as f64, stop as f64, DEFAULT_TICK_COUNT).max(1.0);
        return every(TimeUnit::Millisecond, step);
    }
    let (_, _, previous) = TICK_INTERVALS[index - 1];
    let (_, _, next) = TICK_INTERVALS[index];
    let is_previous_closer = target / (previous as f64) < (next as f64) / target;
    let chosen = if is_previous_closer { index - 1 } else { index };
    let (unit, step, _) = TICK_INTERVALS[chosen];
    Some(TickInterval { unit, step })
}

fn every(unit: TimeUnit, step: f64) -> Option<TickInterval> {
    let step = step.floor();
    (step.is_finite() && step >= 1.0).then_some(TickInterval {
        unit,
        step: step as i64,
    })
}

/// `scaleTime().ticks()` 또는 `.ticks(interval)`: 정의역 양 끝을 포함한 눈금 시각
pub fn ticks(start: i64, stop: i64, interval: Option<TickInterval>) -> Vec<i64> {
    if stop <= start {
        return Vec::new();
    }
    match interval.or_else(|| default_interval(start, stop)) {
        Some(interval) => interval_range(interval, start, stop + 1),
        None => Vec::new(),
    }
}

/// 간격이 정의역에 만들 눈금 수 어림(mermaid `getEstimatedTickCount`)
pub fn estimated_tick_count(start: i64, stop: i64, interval: TickInterval) -> i64 {
    let unit_millis = match interval.unit {
        TimeUnit::Millisecond => 1,
        TimeUnit::Second => MILLIS_PER_SECOND,
        TimeUnit::Minute => MILLIS_PER_MINUTE,
        TimeUnit::Hour => MILLIS_PER_HOUR,
        TimeUnit::Day => MILLIS_PER_DAY,
        TimeUnit::Week(_) => MILLIS_PER_WEEK,
        TimeUnit::Month => MILLIS_PER_MONTH,
        TimeUnit::Year => MILLIS_PER_YEAR,
    };
    let interval_millis = unit_millis * interval.step;
    ((stop - start).max(0) + interval_millis - 1) / interval_millis
}

/// `interval.range(start, stop)`: [start, stop) 안의 눈금. `every(step)`은 칸 값이 step의 배수인 눈금만 남긴다
fn interval_range(interval: TickInterval, start: i64, stop: i64) -> Vec<i64> {
    let mut tick = floor(interval.unit, start - 1);
    tick = floor(interval.unit, advance(interval.unit, tick));
    let mut result = Vec::new();
    while tick < stop && (result.len() as i64) < MAX_TICK_COUNT {
        if field(interval.unit, tick).rem_euclid(interval.step) == 0 {
            result.push(tick);
        }
        tick = floor(interval.unit, advance(interval.unit, tick));
    }
    result
}

fn floor(unit: TimeUnit, millis: i64) -> i64 {
    let unit_floor = |size: i64| millis.div_euclid(size) * size;
    match unit {
        TimeUnit::Millisecond => millis,
        TimeUnit::Second => unit_floor(MILLIS_PER_SECOND),
        TimeUnit::Minute => unit_floor(MILLIS_PER_MINUTE),
        TimeUnit::Hour => unit_floor(MILLIS_PER_HOUR),
        TimeUnit::Day => unit_floor(MILLIS_PER_DAY),
        TimeUnit::Week(first_day) => {
            let back = (weekday_of(millis) + 7 - first_day) % 7;
            start_of_day(millis) - i64::from(back) * MILLIS_PER_DAY
        }
        TimeUnit::Month => {
            let civil = CivilTime::from_millis(millis);
            CivilTime::date(civil.year, civil.month, 1).to_millis()
        }
        TimeUnit::Year => CivilTime::date(CivilTime::from_millis(millis).year, 1, 1).to_millis(),
    }
}

fn advance(unit: TimeUnit, millis: i64) -> i64 {
    match unit {
        TimeUnit::Millisecond => millis + 1,
        TimeUnit::Second => millis + MILLIS_PER_SECOND,
        TimeUnit::Minute => millis + MILLIS_PER_MINUTE,
        TimeUnit::Hour => millis + MILLIS_PER_HOUR,
        TimeUnit::Day => millis + MILLIS_PER_DAY,
        TimeUnit::Week(_) => millis + MILLIS_PER_WEEK,
        TimeUnit::Month => add_months(millis, 1),
        TimeUnit::Year => add_months(millis, 12),
    }
}

/// d3 `every()`가 배수를 따지는 칸 값
fn field(unit: TimeUnit, millis: i64) -> i64 {
    let civil = CivilTime::from_millis(millis);
    match unit {
        TimeUnit::Millisecond => millis,
        TimeUnit::Second => i64::from(civil.second),
        TimeUnit::Minute => i64::from(civil.minute),
        TimeUnit::Hour => i64::from(civil.hour),
        TimeUnit::Day => i64::from(civil.day) - 1,
        TimeUnit::Week(_) => (millis - floor(unit, 0)).div_euclid(MILLIS_PER_WEEK),
        TimeUnit::Month => i64::from(civil.month) - 1,
        TimeUnit::Year => civil.year,
    }
}

/// d3 `timeFormat(specifier)`. `%-`·`%_`·`%0` 채움 수식어를 받는다
pub fn format_time(millis: i64, specifier: &str) -> String {
    let civil = CivilTime::from_millis(millis);
    let mut output = String::new();
    let mut characters = specifier.chars();
    while let Some(character) = characters.next() {
        if character != '%' {
            output.push(character);
            continue;
        }
        let mut directive = characters.next().unwrap_or('%');
        let mut pad = None;
        if matches!(directive, '-' | '_' | '0') {
            pad = Some(directive);
            directive = characters.next().unwrap_or('%');
        }
        output.push_str(&format_directive(civil, directive, pad));
    }
    output
}

fn format_directive(civil: CivilTime, directive: char, pad: Option<char>) -> String {
    let default_pad = if directive == 'e' { '_' } else { '0' };
    let padded = |value: i64, width: usize| pad_number(value, width, pad.unwrap_or(default_pad));
    let hour12 = if civil.hour.is_multiple_of(12) {
        12
    } else {
        civil.hour % 12
    };
    match directive {
        'a' => WEEKDAY_NAMES[civil.weekday() as usize][..3].to_string(),
        'A' => WEEKDAY_NAMES[civil.weekday() as usize].to_string(),
        'b' => MONTH_NAMES[civil.month as usize - 1][..3].to_string(),
        'B' => MONTH_NAMES[civil.month as usize - 1].to_string(),
        'd' | 'e' => padded(i64::from(civil.day), 2),
        'H' => padded(i64::from(civil.hour), 2),
        'I' => padded(i64::from(hour12), 2),
        'j' => padded(civil.day_of_year(), 3),
        'L' => padded(i64::from(civil.millisecond), 3),
        'm' => padded(i64::from(civil.month), 2),
        'M' => padded(i64::from(civil.minute), 2),
        'p' => if civil.hour < 12 { "AM" } else { "PM" }.to_string(),
        'S' => padded(i64::from(civil.second), 2),
        'u' => (if civil.weekday() == 0 {
            7
        } else {
            civil.weekday()
        })
        .to_string(),
        'w' => civil.weekday().to_string(),
        'y' => padded(civil.year.rem_euclid(100), 2),
        'Y' => padded(civil.year.rem_euclid(10_000), 4),
        other => other.to_string(),
    }
}

/// `-`는 채우지 않고, `_`는 공백, `0`은 0으로 채운다
fn pad_number(value: i64, width: usize, pad: char) -> String {
    let digits = value.to_string();
    match pad {
        '-' => digits,
        '_' => format!("{digits:>width$}"),
        _ => format!("{digits:0>width$}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i64, month: u32, day: u32) -> i64 {
        CivilTime::date(year, month, day).to_millis()
    }

    #[test]
    fn 이십육일_정의역은_홀수_날에_이틀_간격_눈금이다() {
        let ticks = ticks(date(2024, 1, 1), date(2024, 1, 27), None);
        assert_eq!(ticks.len(), 14);
        assert_eq!(ticks[1], date(2024, 1, 3));
        assert_eq!(*ticks.last().unwrap(), date(2024, 1, 27));
    }

    #[test]
    fn 몇_달_정의역은_매달_첫날_눈금이다() {
        let ticks = ticks(date(2024, 1, 15), date(2024, 9, 1), None);
        assert_eq!(ticks.first(), Some(&date(2024, 2, 1)));
        assert_eq!(ticks.len(), 8);
    }

    #[test]
    fn strftime_형식으로_쓴다() {
        let millis = date(2024, 3, 5) + 14 * MILLIS_PER_HOUR;
        assert_eq!(format_time(millis, "%Y-%m-%d"), "2024-03-05");
        assert_eq!(format_time(millis, "%b %e, %I %p"), "Mar  5, 02 PM");
        assert_eq!(format_time(millis, "%-m/%-d %a"), "3/5 Tue");
    }
}
