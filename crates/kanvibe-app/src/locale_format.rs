//! 날짜·시각을 사용자 locale로 적는다(TS `toLocaleString()`·`toLocaleDateString()`·`toLocaleTimeString()`).
//! TS는 locale 인자 없이 불러 브라우저 기본 locale(시스템 locale)과 시스템 시간대를 썼다. 앱 화면 언어(`/ko`)와는 상관없다.
//!
//! locale은 POSIX 규칙대로 `LC_ALL` → `LC_TIME` → `LANG` 순서로 읽고, 읽지 못하면 en-US다. 값을 읽기만 하고 바꾸지 않는다.
//! 앱 언어 ko·en·zh는 TS 앱의 Electron(ICU 74.2, CLDR 44.1)이 낸 표기를 글자 그대로 옮긴 패턴으로 적고(`chromium_cldr44`),
//! 나머지 locale은 화면 목록 정렬(`icu_collator`)과 같은 ICU4X 데이터(`icu_datetime`)로 만든다.
//! 시간대는 JS `Date`와 같은 현지 벽시계(`LocalDateTime`)로 바꾼 뒤 적는다.

use icu_calendar::Iso;
use icu_datetime::fieldsets::enums::{
    CompositeDateTimeFieldSet, DateAndTimeFieldSet, DateFieldSet, TimeFieldSet,
};
use icu_datetime::fieldsets::{MD, T, YMD};
use icu_datetime::input::{Date, DateTime, Time};
use icu_datetime::options::{Alignment, YearStyle};
use icu_datetime::{DateTimeFormatter, DateTimeFormatterPreferences};
use icu_locale_core::Locale;
use kanvibe_ai::javascript::parse_date_ms;

use crate::views::settings::local_time::LocalDateTime;
use chromium_cldr44::ChromiumLocale;

mod chromium_cldr44;

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

/// TS 화면이 부르는 날짜·시각 옵션 묶음
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateTimeStyle {
    /// `toLocaleString()`
    DateTime,
    /// `toLocaleDateString()`
    Date,
    /// `toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })`
    TimeHourMinute,
    /// `toLocaleDateString(undefined, { month: "numeric", day: "numeric", hour: "numeric", minute: "2-digit" })`
    MonthDayTimeHourMinute,
    /// `toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })`
    TimeTwoDigitHourMinute,
}

impl DateTimeStyle {
    /// ICU4X가 맡는 locale에 쓰는 필드 조합
    fn icu_field_set(self) -> CompositeDateTimeFieldSet {
        match self {
            Self::DateTime => CompositeDateTimeFieldSet::DateTime(DateAndTimeFieldSet::YMDT(
                YMD::short()
                    .with_year_style(YearStyle::Full)
                    .with_time_hms(),
            )),
            Self::Date => CompositeDateTimeFieldSet::Date(DateFieldSet::YMD(
                YMD::short().with_year_style(YearStyle::Full),
            )),
            Self::TimeHourMinute => CompositeDateTimeFieldSet::Time(TimeFieldSet::T(T::hm())),
            Self::MonthDayTimeHourMinute => CompositeDateTimeFieldSet::DateTime(
                DateAndTimeFieldSet::MDT(MD::short().with_time_hm()),
            ),
            Self::TimeTwoDigitHourMinute => CompositeDateTimeFieldSet::Time(TimeFieldSet::T(
                T::hm().with_alignment(Alignment::Column),
            )),
        }
    }
}

/// 앱 언어(ko·en·zh)는 Electron 표기를 그대로 쓰고, 나머지 locale은 ICU4X 데이터로 적는다
fn format_with(locale: &Locale, style: DateTimeStyle, local: LocalDateTime) -> Option<String> {
    if let Some(chromium_locale) = ChromiumLocale::from_locale(locale) {
        return Some(chromium_locale.format(style, local));
    }
    let formatter = DateTimeFormatter::try_new(
        DateTimeFormatterPreferences::from(locale),
        style.icu_field_set(),
    )
    .ok()?;
    let text = formatter.format(&to_input(local)?).to_string();
    Some(text.replace(NARROW_NO_BREAK_SPACE, " "))
}

/// `new Date(iso).toLocaleString()`: en-US는 `1/15/2026, 2:57:00 AM`. 읽지 못하면 받은 글자 그대로다
pub fn format_date_time(iso: &str) -> String {
    parse_local(iso)
        .and_then(|local| format_with(&system_locale(), DateTimeStyle::DateTime, local))
        .unwrap_or_else(|| iso.to_string())
}

/// `new Date(iso).toLocaleDateString()`: en-US는 `1/15/2026`
pub fn format_date(iso: &str) -> String {
    parse_local(iso)
        .and_then(|local| format_with(&system_locale(), DateTimeStyle::Date, local))
        .unwrap_or_else(|| iso.to_string())
}

/// `toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })`: en-US는 `5:07 AM`
pub fn format_time_hm(local: LocalDateTime) -> Option<String> {
    format_with(&system_locale(), DateTimeStyle::TimeHourMinute, local)
}

/// `toLocaleDateString(undefined, { month, day, hour, minute })`: en-US는 `1/20, 12:00 AM`
pub fn format_month_day_time_hm(local: LocalDateTime) -> Option<String> {
    format_with(
        &system_locale(),
        DateTimeStyle::MonthDayTimeHourMinute,
        local,
    )
}

/// `toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })`(채팅 메시지 시각): en-US는 `02:45 AM`
pub fn format_time_two_digit_hm(local: LocalDateTime) -> Option<String> {
    format_with(
        &system_locale(),
        DateTimeStyle::TimeTwoDigitHourMinute,
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

    /// Electron 32(ICU 74.2, CLDR 44.1)에서 받아 둔 출력. 받는 도구는 TS 코드와 함께 지웠다(커밋 a73f3498까지 capture_intl.cjs)
    const ELECTRON_INTL: &str = include_str!("../../../parity/fixtures/locale/electron-intl.json");

    const STYLES: [(&str, DateTimeStyle); 5] = [
        ("dateTime", DateTimeStyle::DateTime),
        ("date", DateTimeStyle::Date),
        ("timeHourMinute", DateTimeStyle::TimeHourMinute),
        (
            "monthDayTimeHourMinute",
            DateTimeStyle::MonthDayTimeHourMinute,
        ),
        (
            "timeTwoDigitHourMinute",
            DateTimeStyle::TimeTwoDigitHourMinute,
        ),
    ];

    // 시험 프로세스의 시간대가 UTC가 아닐 수 있어 현지 벽시계를 거치지 않은 값으로 확인한다
    #[test]
    fn app_locales_match_the_electron_golden_output() {
        let golden: serde_json::Value = serde_json::from_str(ELECTRON_INTL).unwrap();
        assert_eq!(golden["cldr"], "44.1");
        let wall_clocks: Vec<LocalDateTime> = golden["wallClocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|clock| {
                let part = |index: usize| u32::try_from(clock[index].as_u64().unwrap()).unwrap();
                LocalDateTime {
                    year: part(0),
                    month: part(1),
                    day: part(2),
                    hour: part(3),
                    minute: part(4),
                    second: part(5),
                }
            })
            .collect();
        let outputs = golden["outputs"].as_object().unwrap();
        let mut checked = 0;
        for (tag, styles) in outputs {
            for (key, style) in STYLES {
                let expected = styles[key].as_array().unwrap();
                for (clock, expected) in wall_clocks.iter().zip(expected) {
                    assert_eq!(
                        format_with(&locale(tag), style, *clock).as_deref(),
                        expected.as_str(),
                        "{tag} {key} {clock:?}"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 6 * 5 * 6);
    }

    #[test]
    fn app_locale_matching_keeps_other_regions_on_icu4x() {
        assert_eq!(
            ChromiumLocale::from_locale(&locale("zh-Hans-CN")),
            Some(ChromiumLocale::SimplifiedChinese)
        );
        assert_eq!(ChromiumLocale::from_locale(&locale("zh-TW")), None);
        assert_eq!(ChromiumLocale::from_locale(&locale("en-GB")), None);
    }

    #[test]
    fn other_locales_use_their_own_pattern() {
        assert_eq!(
            format_with(&locale("de-DE"), DateTimeStyle::Date, wall_clock(14, 5, 9)).as_deref(),
            Some("15.01.2026")
        );
    }

    #[test]
    fn unreadable_values_are_shown_as_they_are() {
        assert_eq!(format_date_time("not a date"), "not a date");
    }
}
