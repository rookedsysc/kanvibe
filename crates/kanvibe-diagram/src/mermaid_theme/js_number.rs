//! JS 숫자 규칙: `Math.round`와 `Number.prototype.toString()`.

/// `Math.round`: .5는 +∞ 쪽으로 올린다
pub(crate) fn js_round(value: f64) -> f64 {
    if !value.is_finite() {
        return value;
    }
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `String(number)`: 가장 짧게 되읽히는 자릿수, 1e-6 ≤ |x| < 1e21 밖은 지수 표기
pub(crate) fn number_to_js_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs();
    if (1e-6..1e21).contains(&magnitude) {
        return format!("{value}");
    }
    let text = format!("{value:e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if !exponent.starts_with('-') => {
            format!("{mantissa}e+{exponent}")
        }
        _ => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js와_같은_숫자_글자를_만든다() {
        assert_eq!(number_to_js_string(96.2745098039), "96.2745098039");
        assert_eq!(number_to_js_string(80.0), "80");
        assert_eq!(number_to_js_string(-0.0), "0");
        assert_eq!(number_to_js_string(1e-7), "1e-7");
        assert_eq!(number_to_js_string(1e21), "1e+21");
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(2.5), 3.0);
    }
}
