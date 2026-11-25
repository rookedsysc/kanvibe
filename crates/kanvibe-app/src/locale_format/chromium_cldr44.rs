//! TS 앱의 Electron 32(Chromium 128, ICU 74.2, CLDR 44.1)가 앱 언어 ko·en·zh에서 내던 날짜·시각 표기.
//!
//! ICU4X 2.4에 실린 CLDR은 49라 ko가 `2026. 1. 15.` 대신 `2026/1/15`로 나온다. CLDR 44로 데이터를 다시 만들어도
//! ICU4X는 ICU4C의 skeleton 패턴 생성기(`yMdjms` 등)가 아닌 의미 기반 필드 조합으로 패턴을 고르므로 같은 글자를
//! 보장하지 못한다(zh는 날짜와 함께 적을 때만 시를 두 자리로 채운다). 그래서 세 언어는 Electron이 실제로 낸 패턴을 옮겨 적는다.
//! 기준값은 parity/tools/locale/capture_intl.cjs로 받은 parity/fixtures/locale/electron-intl.json이다.

use icu_locale_core::Locale;

use super::DateTimeStyle;
use crate::views::settings::local_time::LocalDateTime;

/// Electron 표기를 그대로 옮긴 locale
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChromiumLocale {
    /// ko, ko-KR
    Korean,
    /// en, en-US
    AmericanEnglish,
    /// zh, zh-CN, zh-Hans
    SimplifiedChinese,
}

impl ChromiumLocale {
    /// 지역이나 문자가 달라 표기가 바뀌는 locale(en-GB, zh-TW 등)은 ICU4X에 맡긴다
    pub(super) fn from_locale(locale: &Locale) -> Option<Self> {
        let language = locale.id.language.as_str();
        let region = locale.id.region.as_ref().map(|region| region.as_str());
        let script = locale.id.script.as_ref().map(|script| script.as_str());
        match (language, script, region) {
            ("ko", None, None | Some("KR")) => Some(Self::Korean),
            ("en", None, None | Some("US")) => Some(Self::AmericanEnglish),
            ("zh", None | Some("Hans"), None | Some("CN")) => Some(Self::SimplifiedChinese),
            _ => None,
        }
    }

    pub(super) fn format(self, style: DateTimeStyle, local: LocalDateTime) -> String {
        match self {
            Self::Korean => format_korean(style, local),
            Self::AmericanEnglish => format_american_english(style, local),
            Self::SimplifiedChinese => format_simplified_chinese(style, local),
        }
    }
}

/// 12시간제 시: 0시와 12시는 12다
fn twelve_hour(local: LocalDateTime) -> u32 {
    match local.hour % 12 {
        0 => 12,
        hour => hour,
    }
}

fn is_afternoon(local: LocalDateTime) -> bool {
    local.hour >= 12
}

/// ko: `2026. 1. 15. 오후 2:05:09`
fn format_korean(style: DateTimeStyle, local: LocalDateTime) -> String {
    let LocalDateTime {
        year,
        month,
        day,
        minute,
        second,
        ..
    } = local;
    let day_period = if is_afternoon(local) {
        "오후"
    } else {
        "오전"
    };
    let hour = twelve_hour(local);
    match style {
        DateTimeStyle::DateTime => {
            format!("{year}. {month}. {day}. {day_period} {hour}:{minute:02}:{second:02}")
        }
        DateTimeStyle::Date => format!("{year}. {month}. {day}."),
        DateTimeStyle::TimeHourMinute => format!("{day_period} {hour}:{minute:02}"),
        DateTimeStyle::MonthDayTimeHourMinute => {
            format!("{month}. {day}. {day_period} {hour}:{minute:02}")
        }
        DateTimeStyle::TimeTwoDigitHourMinute => format!("{day_period} {hour:02}:{minute:02}"),
    }
}

/// en-US: `1/15/2026, 2:05:09 PM`. Chromium이 CLDR 42의 좁은 공백(U+202F)을 보통 공백으로 되돌린 표기다
fn format_american_english(style: DateTimeStyle, local: LocalDateTime) -> String {
    let LocalDateTime {
        year,
        month,
        day,
        minute,
        second,
        ..
    } = local;
    let day_period = if is_afternoon(local) { "PM" } else { "AM" };
    let hour = twelve_hour(local);
    match style {
        DateTimeStyle::DateTime => {
            format!("{month}/{day}/{year}, {hour}:{minute:02}:{second:02} {day_period}")
        }
        DateTimeStyle::Date => format!("{month}/{day}/{year}"),
        DateTimeStyle::TimeHourMinute => format!("{hour}:{minute:02} {day_period}"),
        DateTimeStyle::MonthDayTimeHourMinute => {
            format!("{month}/{day}, {hour}:{minute:02} {day_period}")
        }
        DateTimeStyle::TimeTwoDigitHourMinute => format!("{hour:02}:{minute:02} {day_period}"),
    }
}

/// zh-CN: `2026/1/15 14:05:09`. 24시간제이고, 날짜와 함께 적는 기본 표기만 시를 두 자리로 채운다
fn format_simplified_chinese(style: DateTimeStyle, local: LocalDateTime) -> String {
    let LocalDateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    } = local;
    match style {
        DateTimeStyle::DateTime => {
            format!("{year}/{month}/{day} {hour:02}:{minute:02}:{second:02}")
        }
        DateTimeStyle::Date => format!("{year}/{month}/{day}"),
        DateTimeStyle::TimeHourMinute => format!("{hour}:{minute:02}"),
        DateTimeStyle::MonthDayTimeHourMinute => format!("{month}/{day} {hour}:{minute:02}"),
        DateTimeStyle::TimeTwoDigitHourMinute => format!("{hour:02}:{minute:02}"),
    }
}
