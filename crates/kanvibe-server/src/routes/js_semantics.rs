//! TS 라우트가 기대던 브라우저·Node 표준 동작(WHATWG URL, `decodeURIComponent`, 속성 읽기)을 같은 결과로 옮긴 것.

use kanvibe_ai::javascript::js_trim;
use serde_json::Value;

/// `new URL(request.url, base)`의 `pathname`과 `searchParams`
#[derive(Debug, Clone)]
pub struct MobileUrl {
    pub pathname: String,
    query_pairs: Vec<(String, String)>,
}

impl MobileUrl {
    /// 해석하지 못하면 TS의 `new URL` 예외처럼 `None`이다
    pub fn parse(raw_url: &str) -> Option<Self> {
        let base = url::Url::parse("http://127.0.0.1/").ok()?;
        let parsed = base.join(raw_url).ok()?;
        Some(Self {
            pathname: parsed.path().to_string(),
            query_pairs: parsed.query_pairs().into_owned().collect(),
        })
    }

    /// `searchParams.get`: 같은 이름의 첫 값
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query_pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn has_param(&self, name: &str) -> bool {
        self.query_pairs.iter().any(|(key, _)| key == name)
    }

    /// `pattern`의 `*`는 TS 정규식의 `([^/]+)` 한 칸이다. 맞으면 잡은 칸을 순서대로 돌려준다(디코드 전)
    pub fn capture(&self, pattern: &str) -> Option<Vec<String>> {
        match_path(&self.pathname, pattern)
    }
}

pub fn match_path(pathname: &str, pattern: &str) -> Option<Vec<String>> {
    let path_segments: Vec<&str> = pathname.split('/').collect();
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    if path_segments.len() != pattern_segments.len() {
        return None;
    }
    let mut captured = Vec::new();
    for (segment, expected) in path_segments.iter().zip(&pattern_segments) {
        if *expected == "*" {
            if segment.is_empty() {
                return None;
            }
            captured.push((*segment).to_string());
        } else if segment != expected {
            return None;
        }
    }
    Some(captured)
}

/// `decodeURIComponent`. 잘못된 퍼센트 열이나 UTF-8이 아니면 `URIError`처럼 실패한다
pub fn decode_uri_component(encoded: &str) -> Result<String, UriError> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let hex = bytes.get(index + 1..index + 3).ok_or(UriError)?;
        let text = std::str::from_utf8(hex).map_err(|_| UriError)?;
        decoded.push(u8::from_str_radix(text, 16).map_err(|_| UriError)?);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| UriError)
}

/// TS `URIError: URI malformed`
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("URI malformed")]
pub struct UriError;

pub fn decode_all(segments: &[String]) -> Result<Vec<String>, UriError> {
    segments
        .iter()
        .map(|segment| decode_uri_component(segment))
        .collect()
}

/// JS에서 `null`·`undefined`의 속성을 읽으면 TypeError다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyOfNullish;

/// `body.<name>`. 없는 속성은 `None`(undefined), `null`/`undefined` 본문은 TypeError다
pub fn property<'a>(body: &'a Value, name: &str) -> Result<Option<&'a Value>, PropertyOfNullish> {
    match body {
        Value::Null => Err(PropertyOfNullish),
        Value::Object(fields) => Ok(fields.get(name)),
        _ => Ok(None),
    }
}

/// `body?.<name>`
pub fn optional_property<'a>(body: &'a Value, name: &str) -> Option<&'a Value> {
    property(body, name).ok().flatten()
}

/// TS `readText`: 문자열이고 공백을 지운 뒤 비어 있지 않을 때만 그 값
pub fn read_text(value: Option<&Value>) -> Option<String> {
    let Some(Value::String(text)) = value else {
        return None;
    };
    let trimmed = js_trim(text);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// `typeof value === "string" && value` (비어 있지 않은 문자열)
pub fn non_empty_string(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// `/^\d+$/.test(text)` — JS `\d`는 ASCII 숫자만이다
pub fn is_ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// `Number(text)`가 안전한 정수인 십진 숫자열이면 그 값
pub fn safe_integer(text: &str) -> Option<u64> {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
    if !is_ascii_digits(text) {
        return None;
    }
    let value: f64 = text.parse().ok()?;
    (value <= MAX_SAFE_INTEGER as f64).then_some(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_uri_component_matches_js_including_malformed_sequences() {
        assert_eq!(decode_uri_component("%250").unwrap(), "%0");
        assert_eq!(decode_uri_component("a%2Fb").unwrap(), "a/b");
        assert_eq!(decode_uri_component("%E0%A4%A"), Err(UriError));
        assert_eq!(decode_uri_component("%FF"), Err(UriError));
    }

    #[test]
    fn mobile_url_keeps_raw_path_segments_and_decodes_query() {
        let url = MobileUrl::parse("/api/mobile/tasks/a%2Fb/surfaces?metadata=minimal&tabId=%401")
            .unwrap();
        assert_eq!(
            url.capture("/api/mobile/tasks/*/surfaces"),
            Some(vec!["a%2Fb".to_string()])
        );
        assert_eq!(url.param("tabId"), Some("@1"));
        assert!(url.capture("/api/mobile/tasks/*").is_none());
    }

    #[test]
    fn property_reads_follow_js_nullish_rules() {
        assert_eq!(property(&Value::Null, "code"), Err(PropertyOfNullish));
        assert_eq!(property(&serde_json::json!(3), "code"), Ok(None));
        assert_eq!(
            read_text(Some(&serde_json::json!("  a "))),
            Some("a".to_string())
        );
        assert_eq!(read_text(Some(&serde_json::json!("   "))), None);
    }
}
