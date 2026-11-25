//! 달력 계산. 시각은 1970-01-01 00:00 벽시계에서 잰 밀리초(i64)다.
//!
//! TS 쪽 dayjs·d3는 현지 시간대 `Date`를 쓰지만 기준 이미지는 서머타임이 없는 시간대에서 만들어졌으므로,
//! 여기서는 시간대 없이 벽시계 시각 그대로 더하고 뺀다. 그래서 하루는 언제나 24시간이다. 지금 시각만은
//! 현지 시간대 벽시계로 옮겨 이 기준에 맞춘다.

pub const MILLIS_PER_SECOND: i64 = 1_000;
pub const MILLIS_PER_MINUTE: i64 = 60 * MILLIS_PER_SECOND;
pub const MILLIS_PER_HOUR: i64 = 60 * MILLIS_PER_MINUTE;
pub const MILLIS_PER_DAY: i64 = 24 * MILLIS_PER_HOUR;
pub const MILLIS_PER_WEEK: i64 = 7 * MILLIS_PER_DAY;

pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub const WEEKDAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// 벽시계 시각을 칸별로 나눈 값. `month`는 1..=12
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millisecond: u32,
}

impl CivilTime {
    pub fn date(year: i64, month: u32, day: u32) -> Self {
        Self {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
        }
    }

    pub fn to_millis(self) -> i64 {
        let days = days_from_civil(self.year, self.month, self.day);
        let time_of_day = i64::from(self.hour) * MILLIS_PER_HOUR
            + i64::from(self.minute) * MILLIS_PER_MINUTE
            + i64::from(self.second) * MILLIS_PER_SECOND
            + i64::from(self.millisecond);
        days * MILLIS_PER_DAY + time_of_day
    }

    pub fn from_millis(millis: i64) -> Self {
        let days = millis.div_euclid(MILLIS_PER_DAY);
        let time_of_day = millis.rem_euclid(MILLIS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (time_of_day / MILLIS_PER_HOUR) as u32,
            minute: (time_of_day % MILLIS_PER_HOUR / MILLIS_PER_MINUTE) as u32,
            second: (time_of_day % MILLIS_PER_MINUTE / MILLIS_PER_SECOND) as u32,
            millisecond: (time_of_day % MILLIS_PER_SECOND) as u32,
        }
    }

    /// 0 = 일요일
    pub fn weekday(self) -> u32 {
        weekday_of(self.to_millis())
    }

    /// 1월 1일이 1인 연중 날짜
    pub fn day_of_year(self) -> i64 {
        days_from_civil(self.year, self.month, self.day) - days_from_civil(self.year, 1, 1) + 1
    }
}

/// 0 = 일요일. 1970-01-01은 목요일이다
pub fn weekday_of(millis: i64) -> u32 {
    (millis.div_euclid(MILLIS_PER_DAY) + 4).rem_euclid(7) as u32
}

pub fn start_of_day(millis: i64) -> i64 {
    millis.div_euclid(MILLIS_PER_DAY) * MILLIS_PER_DAY
}

pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// 그레고리력 날짜 → 1970-01-01부터 센 날 수(Howard Hinnant의 days_from_civil)
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// dayjs `set('month'|'year')`: 1일로 옮겨 달을 바꾼 뒤 날짜를 그 달 마지막 날로 자른다
pub fn add_months(millis: i64, months: i64) -> i64 {
    let civil = CivilTime::from_millis(millis);
    let month_index = civil.year * 12 + i64::from(civil.month) - 1 + months;
    let year = month_index.div_euclid(12);
    let month = month_index.rem_euclid(12) as u32 + 1;
    let day = civil.day.min(days_in_month(year, month));
    CivilTime {
        year,
        month,
        day,
        ..civil
    }
    .to_millis()
}

/// 지금 시각을 현지 시간대 벽시계로 잰 값. TS의 `new Date()`·`dayjs()`는 현지 시간대이고 작업 날짜도 현지
/// 벽시계로 읽으므로, 오늘 선도 같은 기준에서 잰다
pub fn now_millis() -> i64 {
    wall_clock_millis(&chrono::Local::now())
}

/// 시각을 그 시간대의 벽시계 밀리초로 바꾼다
pub fn wall_clock_millis<Zone: chrono::TimeZone>(instant: &chrono::DateTime<Zone>) -> i64 {
    instant.naive_local().and_utc().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 날짜와_날_수를_오간다() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 3, 1), 19_783);
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
        assert_eq!(CivilTime::date(2024, 1, 1).weekday(), 1);
    }

    #[test]
    fn 지금_시각은_현지_시간대_날짜로_센다() {
        use chrono::{FixedOffset, TimeZone};
        let seoul = FixedOffset::east_opt(9 * 3600).unwrap();
        // UTC로는 1월 1일 20시지만 서울(UTC+9)에서는 이미 1월 2일 오전 5시다
        let instant = seoul.with_ymd_and_hms(2024, 1, 2, 5, 0, 0).unwrap();
        let wall = CivilTime::from_millis(wall_clock_millis(&instant));
        assert_eq!(
            (wall.year, wall.month, wall.day, wall.hour),
            (2024, 1, 2, 5)
        );
        let new_york = FixedOffset::west_opt(5 * 3600).unwrap();
        let same_instant = instant.with_timezone(&new_york);
        let wall = CivilTime::from_millis(wall_clock_millis(&same_instant));
        assert_eq!((wall.month, wall.day, wall.hour), (1, 1, 15));
    }

    #[test]
    fn 달을_더하면_날짜를_달_끝으로_자른다() {
        let january_end = CivilTime::date(2024, 1, 31).to_millis();
        assert_eq!(
            add_months(january_end, 1),
            CivilTime::date(2024, 2, 29).to_millis()
        );
    }
}
