//! 날짜·시각을 사용자 locale로 적는다(TS `toLocaleString()`·`toLocaleDateString()`·`toLocaleTimeString()`).
//! TS는 locale 인자 없이 불러 브라우저 기본 locale(시스템 locale)과 시스템 시간대를 썼다. 앱 화면 언어(`/ko`)와는 상관없다.
//!
//! locale은 POSIX 규칙대로 `LC_ALL` → `LC_TIME` → `LANG` 순서로 읽고, 읽지 못하면 en-US다. 값을 읽기만 하고 바꾸지 않는다.
//! 표기는 화면 목록 정렬(`icu_collator`)과 같은 ICU4X 데이터(`icu_datetime`)로 만든다.
//! 시간대는 JS `Date`와 같은 현지 벽시계(`LocalDateTime`)로 바꾼 뒤 적는다.

use icu_calendar::Iso;
use icu_datetime::fieldsets::enums::{
    CompositeDateTimeFieldSet, DateAndTimeFieldSet, DateFieldSet, TimeFieldSet,
};
use icu_datetime::fieldsets::{MD, T, YMD};
use icu_datetime::input::{Date, DateTime, Time};
use icu_datetime::options::YearStyle;
use icu_datetime::{DateTimeFormatter, DateTimeFormatterPreferences};
use icu_locale_core::Locale;
use kanvibe_ai::javascript::parse_date_ms;

use crate::views::settings::local_time::LocalDateTime;

/// locale을 읽지 못하면 쓰는 값(Electron `--lang=en-US`와 같다)
const FALLBACK_LOCALE: &str = "en-US";

/// Chromium은 CLDR 42가 시각과 AM/PM 사이에 넣은 좁은 공백(U+202F)을 웹 호환 때문에 보통 공백으로 되돌려 보인다
const NARROW_NO_BREAK_SPACE: char = '\u{202f}';

/// `en_US.UTF-8`·`ko_KR@euro` 같은 POSIX locale 값을 BCP 47로 읽는다. `C`·`POSIX`는 locale이 없다는 뜻이다
pub fn locale_from_posix(value: &str) -> Option<Locale> {
    let language_tag = value.split(['.', '@']).next()?.replace('_', "-");
    if language_tag.is_empty() || language_tag == "C" || language_tag == "POSIX" {
        return None;
    }
    language_tag.parse().ok()
}

/// 날짜·시각 표기에 쓸 시스템 locale
pub fn system_locale() -> Locale {
    ["LC_ALL", "LC_TIME", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .and_then(|value| locale_from_posix(&value))
        .unwrap_or_else(|| FALLBACK_LOCALE.parse().expect("fallback locale"))
}

/// ISO 시각 글자를 현지 벽시계로 읽는다(JS `new Date(iso)`)
fn parse_local(iso: &str) -> Option<LocalDateTime> {
    LocalDateTime::from_epoch_ms(parse_date_ms(iso)?)
}

fn to_input(local: LocalDateTime) -> Option<DateTime<Iso>> {
    Some(DateTime {
        date: Date::try_new_iso(
            i32::try_from(local.year).ok()?,
            u8::try_from(local.month).ok()?,
            u8::try_from(local.day).ok()?,
        )
        .ok()?,
        time: Time::try_new(
            u8::try_from(local.hour).ok()?,
            u8::try_from(local.minute).ok()?,
            u8::try_from(local.second).ok()?,
            0,
        )
        .ok()?,
    })
}

fn formatter(
    locale: &Locale,
    field_set: CompositeDateTimeFieldSet,
) -> Option<DateTimeFormatter<CompositeDateTimeFieldSet>> {
    DateTimeFormatter::try_new(DateTimeFormatterPreferences::from(locale), field_set).ok()
}

fn format_with(
    locale: &Locale,
    field_set: CompositeDateTimeFieldSet,
    local: LocalDateTime,
) -> Option<String> {
    let text = formatter(locale, field_set)?
        .format(&to_input(local)?)
        .to_string();
    Some(text.replace(NARROW_NO_BREAK_SPACE, " "))
}

/// `toLocaleString()` 필드: 연(네 자리)·월·일·시·분·초
fn date_time_fields() -> CompositeDateTimeFieldSet {
    let field_set = YMD::short()
        .with_year_style(YearStyle::Full)
        .with_time_hms();
    CompositeDateTimeFieldSet::DateTime(DateAndTimeFieldSet::YMDT(field_set))
}

/// `toLocaleDateString()` 필드: 연(네 자리)·월·일
fn date_fields() -> CompositeDateTimeFieldSet {
    CompositeDateTimeFieldSet::Date(DateFieldSet::YMD(
        YMD::short().with_year_style(YearStyle::Full),
    ))
}

/// `new Date(iso).toLocaleString()`: en-US는 `1/15/2026, 2:57:00 AM`. 읽지 못하면 받은 글자 그대로다
pub fn format_date_time(iso: &str) -> String {
    parse_local(iso)
        .and_then(|local| format_with(&system_locale(), date_time_fields(), local))
        .unwrap_or_else(|| iso.to_string())
}

/// `new Date(iso).toLocaleDateString()`: en-US는 `1/15/2026`
pub fn format_date(iso: &str) -> String {
    parse_local(iso)
        .and_then(|local| format_with(&system_locale(), date_fields(), local))
        .unwrap_or_else(|| iso.to_string())
}

/// `toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })`: en-US는 `5:07 AM`
pub fn format_time_hm(local: LocalDateTime) -> Option<String> {
    format_with(
        &system_locale(),
        CompositeDateTimeFieldSet::Time(TimeFieldSet::T(T::hm())),
        local,
    )
}

/// `toLocaleDateString(undefined, { month, day, hour, minute })`: en-US는 `1/20, 12:00 AM`
pub fn format_month_day_time_hm(local: LocalDateTime) -> Option<String> {
    let field_set = DateAndTimeFieldSet::MDT(MD::short().with_time_hm());
    format_with(
        &system_locale(),
        CompositeDateTimeFieldSet::DateTime(field_set),
        local,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(tag: &str) -> Locale {
        tag.parse().unwrap()
    }

    fn wall_clock(hour: u32, minute: u32, second: u32) -> LocalDateTime {
        LocalDateTime {
            year: 2026,
            month: 1,
            day: 15,
            hour,
            minute,
            second,
        }
    }

    #[test]
    fn posix_locale_values_become_bcp47() {
        assert_eq!(locale_from_posix("en_US.UTF-8"), Some(locale("en-US")));
        assert_eq!(locale_from_posix("ko_KR@euro"), Some(locale("ko-KR")));
        assert_eq!(locale_from_posix("C.UTF-8"), None);
        assert_eq!(locale_from_posix("POSIX"), None);
    }

    // 시험 프로세스의 시간대가 UTC가 아닐 수 있어 현지 벽시계를 거치지 않은 값으로 확인한다
    #[test]
    fn en_us_matches_the_chromium_to_locale_output() {
        let en_us = locale("en-US");
        let time = wall_clock(2, 57, 0);
        assert_eq!(
            format_with(&en_us, date_time_fields(), time).as_deref(),
            Some("1/15/2026, 2:57:00 AM")
        );
        assert_eq!(
            format_with(&en_us, date_fields(), time).as_deref(),
            Some("1/15/2026")
        );
        let afternoon = wall_clock(14, 5, 9);
        assert_eq!(
            format_with(&en_us, date_time_fields(), afternoon).as_deref(),
            Some("1/15/2026, 2:05:09 PM")
        );
    }

    // ICU4X에 실린 CLDR이 Chromium(ICU 74)보다 새것이라 en-US 밖의 locale은 표기가 조금 다를 수 있다(ko: `2026. 1. 15.` 대신 `2026/1/15`)
    #[test]
    fn other_locales_use_their_own_pattern() {
        assert_eq!(
            format_with(&locale("ko-KR"), date_time_fields(), wall_clock(14, 5, 9)).as_deref(),
            Some("2026/1/15 오후 2:05:09")
        );
        assert_eq!(
            format_with(&locale("de-DE"), date_fields(), wall_clock(14, 5, 9)).as_deref(),
            Some("15.01.2026")
        );
    }

    #[test]
    fn unreadable_values_are_shown_as_they_are() {
        assert_eq!(format_date_time("not a date"), "not a date");
    }
}
