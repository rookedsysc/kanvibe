//! treemap `valueFormat`(d3-format 기본 로캘 en-US)을 옮겼다. Mermaid는 `$0,0`, `$`와 쉼표가 함께 있는 꼴,
//! 나머지 `$` 꼴을 따로 다루고 나머지는 d3 `format()`에 넘긴다. 읽지 못하는 지정자는 `,`로 대신한다.
//!
//! 지정자: `[[채움]정렬][부호][$#][0][폭][,][.정밀도][~][형식]`. 형식은 `% d e f g p r s`와 없음(`.12~g`)을 다룬다.
//! `b c o x X n`처럼 진법·글자로 바꾸는 형식은 지원하지 않고 없음과 같이 다룬다.

/// en-US `minus`(U+2212)
const MINUS: char = '\u{2212}';
const SI_PREFIXES: [&str; 17] = [
    "y", "z", "a", "f", "p", "n", "\u{b5}", "m", "", "k", "M", "G", "T", "P", "E", "Z", "Y",
];

#[derive(Clone, Debug, PartialEq)]
struct Specifier {
    fill: char,
    align: char,
    sign: char,
    is_currency: bool,
    width: usize,
    has_comma: bool,
    precision: Option<usize>,
    should_trim: bool,
    kind: char,
}

/// Mermaid가 만든 값 글자 함수(`$` 접두 + d3 형식)
#[derive(Clone, Debug, PartialEq)]
pub struct ValueFormat {
    dollar_prefix: bool,
    specifier: Specifier,
}

impl ValueFormat {
    /// treemap renderer.ts의 `valueFormat` 분기
    pub fn new(format: &str) -> Self {
        let comma = || parse_specifier(",").unwrap_or_else(default_specifier);
        let (dollar_prefix, specifier) = if format == "$0,0" {
            (true, Some(comma()))
        } else if let Some(rest) = format.strip_prefix('$') {
            if format.contains(',') {
                let precision = precision_text(format);
                (true, parse_specifier(&format!(",{precision}")))
            } else {
                (true, parse_specifier(rest))
            }
        } else {
            (false, parse_specifier(format))
        };
        match specifier {
            Some(specifier) => Self {
                dollar_prefix,
                specifier,
            },
            None => Self {
                dollar_prefix: false,
                specifier: comma(),
            },
        }
    }

    pub fn format(&self, value: f64) -> String {
        let text = format_number(&self.specifier, value);
        if self.dollar_prefix {
            format!("${text}")
        } else {
            text
        }
    }
}

/// `/\.\d+/`의 첫 일치
fn precision_text(format: &str) -> String {
    let Some(start) = format.find('.') else {
        return String::new();
    };
    let digits: String = format[start + 1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        return precision_text(&format[start + 1..]);
    }
    format!(".{digits}")
}

fn default_specifier() -> Specifier {
    Specifier {
        fill: ' ',
        align: '>',
        sign: '-',
        is_currency: false,
        width: 0,
        has_comma: false,
        precision: None,
        should_trim: false,
        kind: ' ',
    }
}

/// d3 `formatSpecifier` 정규식을 손으로 읽는다. 맞지 않으면 `None`
fn parse_specifier(text: &str) -> Option<Specifier> {
    let characters: Vec<char> = text.chars().collect();
    let mut position = 0;
    let mut specifier = default_specifier();
    let is_align = |c: char| matches!(c, '<' | '>' | '=' | '^');
    if characters.len() > 1 && is_align(characters[1]) {
        (specifier.fill, specifier.align) = (characters[0], characters[1]);
        position = 2;
    } else if characters.first().copied().is_some_and(is_align) {
        specifier.align = characters[0];
        position = 1;
    }
    let mut next_if = |predicate: &dyn Fn(char) -> bool| {
        let found = characters.get(position).copied().filter(|c| predicate(*c));
        position += usize::from(found.is_some());
        found
    };
    if let Some(sign) = next_if(&|c| matches!(c, '+' | '-' | '(' | ' ')) {
        specifier.sign = sign;
    }
    specifier.is_currency = next_if(&|c| matches!(c, '$' | '#')) == Some('$');
    let has_zero = next_if(&|c| c == '0').is_some();
    let mut width = String::new();
    while let Some(digit) = next_if(&|c| c.is_ascii_digit()) {
        width.push(digit);
    }
    specifier.has_comma = next_if(&|c| c == ',').is_some();
    if next_if(&|c| c == '.').is_some() {
        let mut digits = String::new();
        while let Some(digit) = next_if(&|c| c.is_ascii_digit()) {
            digits.push(digit);
        }
        specifier.precision = Some(digits.parse().ok()?);
    }
    specifier.should_trim = next_if(&|c| c == '~').is_some();
    if let Some(kind) = next_if(&|c| c.is_ascii_alphabetic() || c == '%') {
        specifier.kind = kind;
    }
    if position != characters.len() {
        return None;
    }
    specifier.width = width.parse().unwrap_or(0);
    if has_zero || (specifier.fill == '0' && specifier.align == '=') {
        (specifier.fill, specifier.align) = ('0', '=');
    }
    Some(normalize_kind(specifier))
}

/// 없는 형식은 `.12~g`, `n`은 `,g`. 정밀도 기본 6과 범위 제한
fn normalize_kind(mut specifier: Specifier) -> Specifier {
    match specifier.kind {
        'n' => {
            specifier.has_comma = true;
            specifier.kind = 'g';
        }
        '%' | 'd' | 'e' | 'f' | 'g' | 'p' | 'r' | 's' => {}
        _ => {
            specifier.precision = Some(specifier.precision.unwrap_or(12));
            specifier.should_trim = true;
            specifier.kind = 'g';
        }
    }
    let precision = specifier.precision.unwrap_or(6);
    specifier.precision = Some(if matches!(specifier.kind, 'g' | 'p' | 'r' | 's') {
        precision.clamp(1, 21)
    } else {
        precision.min(20)
    });
    specifier
}

/// d3 locale `format(value)`
fn format_number(specifier: &Specifier, value: f64) -> String {
    let precision = specifier.precision.unwrap_or(6);
    let is_negative = value < 0.0 || (value == 0.0 && value.is_sign_negative());
    let (mut body, si_suffix) = format_kind(specifier.kind, value.abs(), precision);
    if specifier.should_trim {
        body = trim_insignificant(&body);
    }
    let is_negative = is_negative && !(body.parse::<f64>() == Ok(0.0) && specifier.sign != '+');
    let mut prefix = match (is_negative, specifier.sign) {
        (true, '(') => "(".to_string(),
        (true, _) => MINUS.to_string(),
        (false, '-' | '(') => String::new(),
        (false, sign) => sign.to_string(),
    };
    if specifier.is_currency {
        prefix.push('$');
    }
    let mut suffix = si_suffix;
    if matches!(specifier.kind, '%' | 'p') {
        suffix.push('%');
    }
    if is_negative && specifier.sign == '(' {
        suffix.push(')');
    }
    let split = body
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(body.len());
    let (integer, fraction) = body.split_at(split);
    let suffix = format!("{fraction}{suffix}");
    pad_and_group(specifier, &prefix, integer, &suffix)
}

/// 폭을 채우고 쉼표로 세 자리씩 묶는다(0 채움이면 채운 뒤 묶는다)
fn pad_and_group(specifier: &Specifier, prefix: &str, integer: &str, suffix: &str) -> String {
    let is_zero_fill = specifier.fill == '0' && specifier.align == '=';
    let mut integer = integer.to_string();
    if specifier.has_comma && !is_zero_fill {
        integer = group_thousands(&integer, usize::MAX);
    }
    let length = prefix.chars().count() + integer.chars().count() + suffix.chars().count();
    let mut padding: String =
        std::iter::repeat_n(specifier.fill, specifier.width.saturating_sub(length)).collect();
    if specifier.has_comma && is_zero_fill {
        let limit = if padding.is_empty() {
            usize::MAX
        } else {
            specifier.width - suffix.chars().count()
        };
        integer = group_thousands(&format!("{padding}{integer}"), limit);
        padding.clear();
    }
    match specifier.align {
        '<' => format!("{prefix}{integer}{suffix}{padding}"),
        '=' => format!("{prefix}{padding}{integer}{suffix}"),
        '^' => {
            let half = padding.chars().count() / 2;
            let (left, right) = padding.split_at(half);
            format!("{left}{prefix}{integer}{suffix}{right}")
        }
        _ => format!("{padding}{prefix}{integer}{suffix}"),
    }
}

/// d3 `formatGroup([3], ",")`
fn group_thousands(digits: &str, width: usize) -> String {
    let characters: Vec<char> = digits.chars().collect();
    let mut groups: Vec<String> = Vec::new();
    let (mut end, mut length) = (characters.len(), 0usize);
    while end > 0 {
        let mut size = 3;
        if length + size + 1 > width {
            size = width.saturating_sub(length).max(1);
        }
        let start = end.saturating_sub(size);
        groups.push(characters[start..end].iter().collect());
        end = start;
        length += size + 1;
        if length > width {
            break;
        }
    }
    groups.reverse();
    groups.join(",")
}

/// 형식별 숫자 글자와 SI 접두(`s`만)
fn format_kind(kind: char, value: f64, precision: usize) -> (String, String) {
    match kind {
        '%' => (to_fixed(value * 100.0, precision), String::new()),
        'd' => (format!("{}", (value + 0.5).floor()), String::new()),
        'e' => (to_exponential(value, Some(precision)), String::new()),
        'f' => (to_fixed(value, precision), String::new()),
        'p' => (format_rounded(value * 100.0, precision), String::new()),
        'r' => (format_rounded(value, precision), String::new()),
        's' => format_prefix_auto(value, precision),
        _ => (to_precision(value, precision), String::new()),
    }
}

/// JS `toFixed`: 같은 거리면 큰 쪽으로 반올림한다
fn to_fixed(value: f64, digits: usize) -> String {
    let scale = 10f64.powi(digits as i32);
    let scaled = value * scale;
    if (scaled - scaled.trunc() - 0.5).abs() < f64::EPSILON * scaled.abs().max(1.0) {
        return format!("{:.digits$}", (scaled.trunc() + 1.0) / scale);
    }
    format!("{value:.digits$}")
}

/// JS `toExponential(p)`(정밀도가 없으면 가장 짧은 표현)
fn to_exponential(value: f64, digits: Option<usize>) -> String {
    let text = match digits {
        Some(digits) => format!("{value:.digits$e}"),
        None => format!("{value:e}"),
    };
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{}", exponent.abs())
}

/// JS `toPrecision(p)`
fn to_precision(value: f64, digits: usize) -> String {
    if value == 0.0 {
        return to_fixed(0.0, digits.saturating_sub(1));
    }
    let (_, exponent) = decimal_parts(value, Some(digits));
    if exponent < -6 || exponent >= digits as i32 {
        return to_exponential(value, Some(digits - 1));
    }
    to_fixed(value, (digits as i32 - 1 - exponent).max(0) as usize)
}

/// d3 `formatDecimalParts`: 소수점 없는 유효 숫자와 십진 지수
fn decimal_parts(value: f64, digits: Option<usize>) -> (String, i32) {
    let text = to_exponential(value, digits.map(|digits| digits.saturating_sub(1)));
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    (mantissa.replace('.', ""), exponent.parse().unwrap_or(0))
}

/// d3 `formatRounded`
fn format_rounded(value: f64, digits: usize) -> String {
    if value == 0.0 || !value.is_finite() {
        return format!("{value}");
    }
    let (coefficient, exponent) = decimal_parts(value, Some(digits));
    if exponent < 0 {
        return format!("0.{}{coefficient}", "0".repeat((-exponent - 1) as usize));
    }
    let integer_length = exponent as usize + 1;
    if coefficient.len() > integer_length {
        return format!(
            "{}.{}",
            &coefficient[..integer_length],
            &coefficient[integer_length..]
        );
    }
    format!(
        "{coefficient}{}",
        "0".repeat(integer_length - coefficient.len())
    )
}

/// d3 `formatPrefixAuto`: 지수를 3의 배수로 묶고 SI 접두를 붙인다
fn format_prefix_auto(value: f64, digits: usize) -> (String, String) {
    if value == 0.0 || !value.is_finite() {
        return (to_precision(value, digits), String::new());
    }
    let (coefficient, exponent) = decimal_parts(value, Some(digits));
    let prefix_exponent = (exponent as f64 / 3.0).floor().clamp(-8.0, 8.0) as i32;
    let prefix = SI_PREFIXES[(8 + prefix_exponent) as usize].to_string();
    let integer_length = exponent - prefix_exponent * 3 + 1;
    let length = coefficient.len() as i32;
    let body = if integer_length == length {
        coefficient
    } else if integer_length > length {
        format!(
            "{coefficient}{}",
            "0".repeat((integer_length - length) as usize)
        )
    } else if integer_length > 0 {
        let split = integer_length as usize;
        format!("{}.{}", &coefficient[..split], &coefficient[split..])
    } else {
        let (small, _) = decimal_parts(
            value,
            Some((digits as i32 + integer_length - 1).max(0) as usize),
        );
        format!("0.{}{small}", "0".repeat((-integer_length) as usize))
    };
    (body, prefix)
}

/// d3 `formatTrim`: 소수부 끝의 0과 남은 소수점을 지운다
fn trim_insignificant(text: &str) -> String {
    let characters: Vec<char> = text.chars().collect();
    let (mut start, mut end): (Option<usize>, usize) = (None, 0);
    for (index, character) in characters.iter().enumerate().skip(1) {
        match character {
            '.' => {
                start = Some(index);
                end = index;
            }
            '0' => {
                if start == Some(0) {
                    start = Some(index);
                }
                end = index;
            }
            c if c.is_ascii_digit() => {
                if start.is_some_and(|value| value > 0) {
                    start = Some(0);
                }
            }
            _ => break,
        }
    }
    match start {
        Some(start) if start > 0 => {
            let head: String = characters[..start].iter().collect();
            let tail: String = characters[end + 1..].iter().collect();
            format!("{head}{tail}")
        }
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기본_쉼표_형식은_세_자리마다_묶는다() {
        let format = ValueFormat::new(",");
        assert_eq!(format.format(5300.0), "5,300");
        assert_eq!(format.format(1234567.5), "1,234,567.5");
    }

    #[test]
    fn 달러와_쉼표가_있으면_정밀도만_남긴다() {
        // treemap-config: "$,.2f"는 Mermaid가 ",.2"(유효 숫자 2개 ~g)로 바꿔 지수 표기가 된다
        let format = ValueFormat::new("$,.2f");
        assert_eq!(format.format(87000.75), "$8.7e+4");
        assert_eq!(format.format(4000.0), "$4e+3");
        assert_eq!(ValueFormat::new("$0,0").format(1500.0), "$1,500");
    }

    #[test]
    fn 여러_d3_형식을_옮긴다() {
        assert_eq!(ValueFormat::new(".1%").format(0.256), "25.6%");
        assert_eq!(ValueFormat::new(".3s").format(12500.0), "12.5k");
        assert_eq!(ValueFormat::new(",.2f").format(1234.5), "1,234.50");
        assert_eq!(ValueFormat::new("d").format(2.5), "3");
        assert_eq!(ValueFormat::new(".2r").format(1234.0), "1200");
        assert_eq!(ValueFormat::new("$").format(12.0), "$12");
    }

    #[test]
    fn 읽지_못하는_지정자는_쉼표로_대신한다() {
        assert_eq!(ValueFormat::new("bogus!").format(1500.0), "1,500");
    }
}
