//! dayjs 날짜 형식(`dateFormat`)으로 읽고 쓴다. customParseFormat의 엄격 모드처럼, 읽은 시각을 같은 형식으로
//! 다시 써서 입력과 같을 때만 받아들인다.
//!
//! 지원 토큰: `YYYY YY M MM MMM MMMM D DD Do H HH h hh m mm s ss S SS SSS A a X x`, `[...]` 글자 그대로.
//! 나머지 글자는 그대로 둔다.

use super::calendar::{
    CivilTime, MILLIS_PER_DAY, MILLIS_PER_HOUR, MILLIS_PER_MINUTE, MILLIS_PER_SECOND, MONTH_NAMES,
    days_from_civil, now_millis,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum FormatToken {
    Literal(String),
    /// 토큰 글자 그대로(`YYYY`, `MM` …)
    Field(&'static str),
}

const FIELD_TOKENS: [&str; 24] = [
    "YYYY", "YY", "MMMM", "MMM", "MM", "M", "Do", "DD", "D", "HH", "H", "hh", "h", "mm", "m", "ss",
    "s", "SSS", "SS", "S", "A", "a", "X", "x",
];

fn tokenize(format: &str) -> Vec<FormatToken> {
    let mut tokens = Vec::new();
    let mut rest = format;
    while let Some(character) = rest.chars().next() {
        if character == '[' {
            let end = rest.find(']').unwrap_or(rest.len() - 1);
            tokens.push(FormatToken::Literal(rest[1..end].to_string()));
            rest = &rest[(end + 1).min(rest.len())..];
            continue;
        }
        match FIELD_TOKENS.iter().find(|token| rest.starts_with(**token)) {
            Some(token) => {
                tokens.push(FormatToken::Field(token));
                rest = &rest[token.len()..];
            }
            None => {
                tokens.push(FormatToken::Literal(character.to_string()));
                rest = &rest[character.len_utf8()..];
            }
        }
    }
    tokens
}

/// 읽어 낸 칸들. 없는 칸은 dayjs처럼 오늘 날짜나 0으로 채운다
#[derive(Default)]
struct ParsedFields {
    year: Option<i64>,
    month: Option<u32>,
    day: Option<u32>,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
    is_afternoon: Option<bool>,
    timestamp: Option<i64>,
}

/// 엄격 모드로 읽는다. 형식과 맞지 않으면 `None`
pub fn parse_strict(input: &str, format: &str) -> Option<i64> {
    let tokens = tokenize(format);
    let mut fields = ParsedFields::default();
    let mut rest = input;
    for token in &tokens {
        rest = match token {
            FormatToken::Literal(text) => rest.get(text.len()..).unwrap_or(""),
            FormatToken::Field(name) => read_field(rest, name, &mut fields)?,
        };
    }
    let millis = fields_to_millis(&fields);
    (format_tokens(millis, &tokens) == input).then_some(millis)
}

fn take_digits(input: &str, min: usize, max: usize) -> Option<(&str, &str)> {
    let count = input
        .bytes()
        .take(max)
        .take_while(u8::is_ascii_digit)
        .count();
    (count >= min).then(|| input.split_at(count))
}

fn take_signed(input: &str) -> Option<(i64, &str)> {
    let sign_length = usize::from(input.starts_with(['+', '-']));
    let (digits, rest) = take_digits(&input[sign_length..], 1, usize::MAX)?;
    let value: i64 = digits.parse().ok()?;
    let value = if input.starts_with('-') {
        -value
    } else {
        value
    };
    Some((value, rest))
}

fn take_word(input: &str) -> (&str, &str) {
    let end = input
        .char_indices()
        .find(|(_, character)| !character.is_alphabetic())
        .map_or(input.len(), |(index, _)| index);
    input.split_at(end)
}

fn read_field<'a>(input: &'a str, name: &str, fields: &mut ParsedFields) -> Option<&'a str> {
    match name {
        "MMM" | "MMMM" => return read_month_name(input, fields),
        "A" | "a" => return read_meridiem(input, fields),
        "X" | "x" => {
            let (value, rest) = take_signed(input)?;
            let unit = if name == "X" { MILLIS_PER_SECOND } else { 1 };
            fields.timestamp = Some(value * unit);
            return Some(rest);
        }
        _ => {}
    }
    let (min, max) = match name {
        "YYYY" => (4, 4),
        "YY" | "MM" | "DD" | "HH" | "hh" | "mm" | "ss" | "SS" => (2, 2),
        "SSS" => (3, 3),
        "S" => (1, 1),
        _ => (1, 2),
    };
    let (digits, rest) = take_digits(input, min, max)?;
    let value: u32 = digits.parse().ok()?;
    store_number(name, value, fields);
    Some(rest)
}

fn store_number(name: &str, value: u32, fields: &mut ParsedFields) {
    match name {
        "YYYY" => fields.year = Some(i64::from(value)),
        "YY" => fields.year = Some(i64::from(value) + if value > 68 { 1900 } else { 2000 }),
        "M" | "MM" => fields.month = Some(value),
        "D" | "DD" | "Do" => fields.day = Some(value),
        "H" | "HH" | "h" | "hh" => fields.hour = value,
        "m" | "mm" => fields.minute = value,
        "s" | "ss" => fields.second = value,
        "S" => fields.millisecond = value * 100,
        "SS" => fields.millisecond = value * 10,
        "SSS" => fields.millisecond = value,
        _ => {}
    }
}

fn read_month_name<'a>(input: &'a str, fields: &mut ParsedFields) -> Option<&'a str> {
    let (word, rest) = take_word(input);
    let index = MONTH_NAMES
        .iter()
        .position(|name| *name == word || name[..3] == *word)?;
    fields.month = Some(index as u32 + 1);
    Some(rest)
}

fn read_meridiem<'a>(input: &'a str, fields: &mut ParsedFields) -> Option<&'a str> {
    let (word, rest) = take_word(input);
    fields.is_afternoon = Some(word.eq_ignore_ascii_case("pm"));
    Some(rest)
}

fn fields_to_millis(fields: &ParsedFields) -> i64 {
    if let Some(timestamp) = fields.timestamp {
        return timestamp;
    }
    let today = CivilTime::from_millis(now_millis());
    let has_date = fields.year.is_some() || fields.month.is_some();
    let year = fields.year.unwrap_or(today.year);
    let month = match (fields.year, fields.month) {
        (Some(_), None) => 1,
        (_, month) => month.unwrap_or(today.month),
    };
    let day = fields.day.unwrap_or(if has_date { 1 } else { today.day });
    let hour = match fields.is_afternoon {
        Some(true) if fields.hour < 12 => fields.hour + 12,
        Some(false) if fields.hour == 12 => 0,
        _ => fields.hour,
    };
    let month_index = year * 12 + i64::from(month) - 1;
    let first_day = days_from_civil(
        month_index.div_euclid(12),
        month_index.rem_euclid(12) as u32 + 1,
        1,
    );
    (first_day + i64::from(day) - 1) * MILLIS_PER_DAY
        + i64::from(hour) * MILLIS_PER_HOUR
        + i64::from(fields.minute) * MILLIS_PER_MINUTE
        + i64::from(fields.second) * MILLIS_PER_SECOND
        + i64::from(fields.millisecond)
}

/// dayjs `format()`
pub fn format(millis: i64, format: &str) -> String {
    format_tokens(millis, &tokenize(format))
}

fn format_tokens(millis: i64, tokens: &[FormatToken]) -> String {
    let civil = CivilTime::from_millis(millis);
    tokens
        .iter()
        .map(|token| match token {
            FormatToken::Literal(text) => text.clone(),
            FormatToken::Field(name) => format_field(millis, civil, name),
        })
        .collect()
}

fn format_field(millis: i64, civil: CivilTime, name: &str) -> String {
    let hour12 = if civil.hour.is_multiple_of(12) {
        12
    } else {
        civil.hour % 12
    };
    match name {
        "YYYY" => format!("{:04}", civil.year),
        "YY" => format!("{:02}", civil.year.rem_euclid(100)),
        "MMMM" => MONTH_NAMES[civil.month as usize - 1].to_string(),
        "MMM" => MONTH_NAMES[civil.month as usize - 1][..3].to_string(),
        "MM" => format!("{:02}", civil.month),
        "M" => civil.month.to_string(),
        "Do" => format!("{}{}", civil.day, ordinal_suffix(civil.day)),
        "DD" => format!("{:02}", civil.day),
        "D" => civil.day.to_string(),
        "HH" => format!("{:02}", civil.hour),
        "H" => civil.hour.to_string(),
        "hh" => format!("{hour12:02}"),
        "h" => hour12.to_string(),
        "mm" => format!("{:02}", civil.minute),
        "m" => civil.minute.to_string(),
        "ss" => format!("{:02}", civil.second),
        "s" => civil.second.to_string(),
        "SSS" => format!("{:03}", civil.millisecond),
        "SS" => format!("{:02}", civil.millisecond / 10),
        "S" => (civil.millisecond / 100).to_string(),
        "A" => if civil.hour < 12 { "AM" } else { "PM" }.to_string(),
        "a" => if civil.hour < 12 { "am" } else { "pm" }.to_string(),
        "X" => millis.div_euclid(MILLIS_PER_SECOND).to_string(),
        "x" => millis.to_string(),
        other => other.to_string(),
    }
}

fn ordinal_suffix(day: u32) -> &'static str {
    match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 형식대로_읽고_엄격하게_검사한다() {
        let expected = CivilTime::date(2024, 3, 5).to_millis();
        assert_eq!(parse_strict("2024-03-05", "YYYY-MM-DD"), Some(expected));
        assert_eq!(parse_strict("2024-3-5", "YYYY-MM-DD"), None);
        assert_eq!(parse_strict("2024-02-30", "YYYY-MM-DD"), None);
        assert_eq!(parse_strict("05/03/2024", "DD/MM/YYYY"), Some(expected));
        assert_eq!(parse_strict("10d", "YYYY-MM-DD"), None);
    }

    #[test]
    fn 시각과_오전_오후를_읽는다() {
        let expected = CivilTime::date(2024, 3, 5).to_millis() + 13 * MILLIS_PER_HOUR;
        assert_eq!(
            parse_strict("2024-03-05 01:00 PM", "YYYY-MM-DD hh:mm A"),
            Some(expected)
        );
        assert_eq!(format(expected, "MMM Do, HH:mm"), "Mar 5th, 13:00");
    }
}
