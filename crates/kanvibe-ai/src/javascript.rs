//! TS 리더가 기대는 JavaScript 내장 동작(`Date.parse`, `toISOString`, `JSON.stringify`, UTF-16 길이, 공백, 참·거짓 판정)을
//! 바이트 단위로 같게 흉내 낸다. 세션 제목 자르기나 시각 정렬이 TS와 한 글자라도 달라지면 화면과 모바일 계약이 어긋난다.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// JS 정규식 `\s`와 `String.prototype.trim`이 공백으로 보는 글자들. 정규식 문자 클래스 안에 그대로 넣어 쓴다
pub const JS_WHITESPACE_CLASS: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

/// JS 정규식 `.`(s 플래그 없음)이 맞추는 글자. 줄 끝 글자 네 개만 뺀다
pub const JS_DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

/// JS 공백 판정. Rust `char::is_whitespace`와 달리 U+FEFF를 넣고 U+0085를 뺀다
pub fn is_js_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

pub fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

pub fn js_trim_start(value: &str) -> &str {
    value.trim_start_matches(is_js_whitespace)
}

pub fn js_trim_end(value: &str) -> &str {
    value.trim_end_matches(is_js_whitespace)
}

/// JS `string.length`. BMP 밖 글자는 둘로 센다
pub fn utf16_length(value: &str) -> usize {
    value.chars().map(char::len_utf16).sum()
}

/// JS `string.slice(0, units)`. 대리 쌍 한가운데에서 잘리면 JS는 반쪽 글자를 남기지만 Rust 문자열은 담을 수 없어 그 글자를 뺀다
pub fn utf16_prefix(value: &str, units: usize) -> &str {
    let mut consumed_units = 0;
    for (byte_index, character) in value.char_indices() {
        consumed_units += character.len_utf16();
        if consumed_units > units {
            return &value[..byte_index];
        }
    }
    value
}

/// JS `string.slice(start)`(start ≥ 0)
pub fn utf16_suffix_from(value: &str, start_units: usize) -> &str {
    let prefix = utf16_prefix(value, start_units);
    &value[prefix.len()..]
}

/// JS `string.slice(-units)`. 끝에서 UTF-16 단위로 센다
pub fn utf16_last(value: &str, units: usize) -> &str {
    let total = utf16_length(value);
    utf16_suffix_from(value, total.saturating_sub(units))
}

/// JS 참·거짓 판정. 빈 배열과 빈 객체는 참이다
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// 값이 있으면 참인지 본다. 없는 필드(`undefined`)는 거짓이다
pub fn is_present_truthy(value: Option<&Value>) -> bool {
    value.is_some_and(is_truthy)
}

/// 값이 비어 있지 않은 문자열이면 그 문자열. `typeof x === "string" && x`에 해당한다
pub fn non_empty_str(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// JS `String(value)`
pub fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number_to_string(number.as_f64().unwrap_or(0.0)),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// JS `Number.prototype.toString()`. 가장 짧게 되돌아오는 자릿수를 JS의 지수 표기 규칙으로 늘어놓는다
pub fn number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_string();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_string();
    }
    if number == 0.0 {
        return "0".to_string();
    }
    let sign = if number < 0.0 { "-" } else { "" };
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust 지수 표기에는 항상 e가 있다");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().expect("지수는 정수다");
    format!("{sign}{}", place_decimal_digits(&digits, exponent + 1))
}

/// 자릿수 `digits`가 0.digits × 10^point_position 일 때 JS가 쓰는 모양
fn place_decimal_digits(digits: &str, point_position: i32) -> String {
    let digit_count = digits.len() as i32;
    if digit_count <= point_position && point_position <= 21 {
        return format!(
            "{digits}{}",
            "0".repeat((point_position - digit_count) as usize)
        );
    }
    if 0 < point_position && point_position <= 21 {
        let (integer, fraction) = digits.split_at(point_position as usize);
        return format!("{integer}.{fraction}");
    }
    if -6 < point_position && point_position <= 0 {
        return format!("0.{}{digits}", "0".repeat((-point_position) as usize));
    }
    let exponent = point_position - 1;
    let exponent_sign = if exponent < 0 { "-" } else { "+" };
    let (first, rest) = digits.split_at(1);
    let mantissa = if rest.is_empty() {
        first.to_string()
    } else {
        format!("{first}.{rest}")
    };
    format!("{mantissa}e{exponent_sign}{}", exponent.abs())
}

/// JS `JSON.stringify(value)`. 키 순서는 읽은 순서를 따르고 숫자는 JS 숫자 표기를 쓴다
pub fn stringify(value: &Value) -> String {
    let mut output = String::new();
    write_json(value, &mut output);
    output
}

fn write_json(value: &Value, output: &mut String) {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(flag) => output.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => output.push_str(&number_to_string(number.as_f64().unwrap_or(0.0))),
        Value::String(text) => output.push_str(&quote_json_string(text)),
        Value::Array(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_json(item, output);
            }
            output.push(']');
        }
        Value::Object(entries) => {
            output.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(&quote_json_string(key));
                output.push(':');
                write_json(item, output);
            }
            output.push('}');
        }
    }
}

/// JSON 문자열 따옴표 처리. serde_json과 JS는 같은 글자를 같은 모양으로 이스케이프한다
pub fn quote_json_string(text: &str) -> String {
    serde_json::to_string(text).expect("문자열 직렬화는 실패하지 않는다")
}

/// JS `JSON.parse`. 해석할 수 없으면 `None`이다
pub fn parse_json(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok()
}

const MAX_TIME_VALUE_MS: f64 = 8.64e15;

static ISO_DATE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^([+-][0-9]{6}|[0-9]{4})(?:-([0-9]{2})(?:-([0-9]{2}))?)?",
        r"(?:[Tt ]([0-9]{2}):([0-9]{2})(?::([0-9]{2})(?:[.,]([0-9]+))?)?)?",
        r" *([Zz]|[+-][0-9]{2}(?::?[0-9]{2})?)?$"
    ))
    .expect("ISO 날짜 패턴")
});

static SLASH_DATE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^([0-9]{4})/([0-9]{1,2})/([0-9]{1,2})(?: ([0-9]{1,2}):([0-9]{2})(?::([0-9]{2}))?)?$",
    )
    .expect("슬래시 날짜 패턴")
});

/// JS `Date.parse`. ISO 8601 꼴(날짜만 있으면 UTC, 시각이 있고 오프셋이 없으면 현지 시각)과 V8이 받아 주는
/// `YYYY/MM/DD hh:mm:ss`(현지 시각)를 해석한다. 해석할 수 없으면 `None`(JS의 NaN)이다
pub fn parse_date_ms(value: &str) -> Option<f64> {
    let trimmed = js_trim(value);
    if let Some(captures) = ISO_DATE_PATTERN.captures(trimmed) {
        return parse_iso_captures(&captures);
    }
    let captures = SLASH_DATE_PATTERN.captures(trimmed)?;
    let number = |index: usize| -> Option<u32> {
        captures
            .get(index)
            .map_or(Some(0), |found| found.as_str().parse().ok())
    };
    let date = NaiveDate::from_ymd_opt(captures[1].parse().ok()?, number(2)?, number(3)?)?;
    let time = NaiveTime::from_hms_opt(number(4)?, number(5)?, number(6)?)?;
    local_time_ms(date.and_time(time))
}

fn parse_iso_captures(captures: &regex::Captures<'_>) -> Option<f64> {
    let field = |index: usize, default: u32| -> Option<u32> {
        captures
            .get(index)
            .map_or(Some(default), |found| found.as_str().parse().ok())
    };
    let year: i32 = captures[1].parse().ok()?;
    let date = NaiveDate::from_ymd_opt(year, field(2, 1)?, field(3, 1)?)?;
    let hour = field(4, 0)?;
    let minute = field(5, 0)?;
    let second = field(6, 0)?;
    let millisecond = captures.get(7).map_or(Some(0), |found| {
        let digits = format!("{:0<3}", found.as_str());
        digits[..3].parse::<u32>().ok()
    })?;
    let is_end_of_day = hour == 24 && minute == 0 && second == 0 && millisecond == 0;
    let time = if is_end_of_day {
        NaiveTime::from_hms_opt(0, 0, 0)?
    } else {
        NaiveTime::from_hms_milli_opt(hour, minute, second, millisecond)?
    };
    let mut date_time = date.and_time(time);
    if is_end_of_day {
        date_time += chrono::Duration::days(1);
    }
    let has_time = captures.get(4).is_some();
    let offset_minutes = match captures.get(8).map(|found| found.as_str()) {
        Some("Z" | "z") => Some(0),
        Some(offset) => Some(parse_offset_minutes(offset)?),
        None if has_time => None,
        None => Some(0),
    };
    match offset_minutes {
        Some(minutes) => {
            let utc_ms = date_time.and_utc().timestamp_millis() - i64::from(minutes) * 60_000;
            clip_time_value(utc_ms as f64)
        }
        None => local_time_ms(date_time),
    }
}

fn parse_offset_minutes(offset: &str) -> Option<i32> {
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let digits: String = offset[1..].chars().filter(char::is_ascii_digit).collect();
    let hours: i32 = digits.get(..2)?.parse().ok()?;
    let minutes: i32 = digits
        .get(2..)
        .filter(|rest| !rest.is_empty())
        .map_or(Some(0), |rest| rest.parse().ok())?;
    (hours <= 23 && minutes <= 59).then_some(sign * (hours * 60 + minutes))
}

fn local_time_ms(date_time: NaiveDateTime) -> Option<f64> {
    let local = Local.from_local_datetime(&date_time).earliest()?;
    clip_time_value(local.timestamp_millis() as f64)
}

fn clip_time_value(milliseconds: f64) -> Option<f64> {
    (milliseconds.abs() <= MAX_TIME_VALUE_MS).then_some(milliseconds)
}

/// JS `new Date(ms).toISOString()`. 소수 밀리초는 버리고, 범위를 벗어나면(JS는 RangeError) `None`이다
pub fn iso_string_from_ms(milliseconds: f64) -> Option<String> {
    if !milliseconds.is_finite() || milliseconds.abs() > MAX_TIME_VALUE_MS {
        return None;
    }
    let date_time: DateTime<Utc> = DateTime::from_timestamp_millis(milliseconds.trunc() as i64)?;
    let year = date_time.format("%Y").to_string().parse::<i32>().ok()?;
    let rest = date_time.format("-%m-%dT%H:%M:%S%.3fZ").to_string();
    let year_text = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else if year < 0 {
        format!("-{:06}", -year)
    } else {
        format!("+{year:06}")
    };
    Some(format!("{year_text}{rest}"))
}

/// JS `Date.now()`
pub fn now_ms() -> f64 {
    Utc::now().timestamp_millis() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn number_to_string_follows_js_notation() {
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(1.5), "1.5");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(
            number_to_string(123456789012345680000.0),
            "123456789012345680000"
        );
        assert_eq!(number_to_string(0.000001), "0.000001");
        assert_eq!(number_to_string(0.0000001), "1e-7");
        assert_eq!(number_to_string(1.25e-7), "1.25e-7");
    }

    #[test]
    fn stringify_keeps_insertion_order_and_js_numbers() {
        let value: Value = serde_json::from_str(r#"{"b":1.0,"a":[1e2,"x\n"],"c":null}"#).unwrap();
        assert_eq!(stringify(&value), r#"{"b":1,"a":[100,"x\n"],"c":null}"#);
        assert_eq!(stringify(&json!({})), "{}");
    }

    #[test]
    fn date_parse_matches_js_for_iso_forms() {
        assert_eq!(
            parse_date_ms("2026-03-13T09:00:00.000Z"),
            Some(1_773_392_400_000.0)
        );
        assert_eq!(
            parse_date_ms("2026-11-12T19:00:00.000+09:00"),
            Some(1_794_477_600_000.0)
        );
        assert_eq!(parse_date_ms("2026-03-13"), Some(1_773_360_000_000.0));
        assert_eq!(
            parse_date_ms("2026-03-13T09:00:00.123456Z"),
            Some(1_773_392_400_123.0)
        );
        assert_eq!(parse_date_ms("not a date"), None);
        assert_eq!(parse_date_ms(""), None);
    }

    #[test]
    fn iso_string_truncates_fractional_milliseconds() {
        assert_eq!(
            iso_string_from_ms(1_773_392_400_123.9).as_deref(),
            Some("2026-03-13T09:00:00.123Z")
        );
        assert_eq!(iso_string_from_ms(9e15), None);
    }

    #[test]
    fn utf16_helpers_count_surrogate_pairs() {
        assert_eq!(utf16_length("a🙂"), 3);
        assert_eq!(utf16_prefix("a🙂b", 3), "a🙂");
        assert_eq!(utf16_prefix("a🙂b", 2), "a");
        assert_eq!(utf16_last("abc", 2), "bc");
        assert_eq!(js_trim("\u{FEFF} x \u{3000}"), "x");
    }
}
