//! DB 시각 문자열을 TS가 내보내던 ISO 형식으로 바꾼다.
//!
//! 도메인 모델은 SQLite의 `2026-11-25 03:24:31` 원문을 그대로 들고 다닌다(다시 쓸 때 원본이 바뀌지 않게).
//! TS는 TypeORM이 그 값을 UTC `Date`로 읽어 `JSON.stringify`가 `2026-11-25T03:24:31.000Z`를 냈으므로,
//! 네트워크로 나가는 JSON에서만 같은 변환을 한다. 대상은 이름이 `At`으로 끝나는 칸의 SQLite 시각 문자열뿐이다.

use serde_json::Value;

pub fn normalize_database_dates(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(normalize_database_dates),
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                if key.ends_with("At")
                    && let Value::String(text) = field
                    && let Some(iso) = to_iso_timestamp(text)
                {
                    *text = iso;
                    continue;
                }
                normalize_database_dates(field);
            }
        }
        _ => {}
    }
}

/// `YYYY-MM-DD HH:MM[:SS[.fff]]`(구분자 공백 또는 T, 시간대 없음 또는 `Z`)만 바꾼다. 다른 오프셋은 그대로 둔다
fn to_iso_timestamp(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let is_date = bytes.len() >= 16
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && matches!(bytes[10], b' ' | b'T')
        && bytes[13] == b':';
    if !is_date || !text.is_ascii() {
        return None;
    }
    let (date, time) = (&text[..10], text[11..].trim_end_matches('Z'));
    let (clock, fraction) = time.split_once('.').unwrap_or((time, ""));
    let clock = match clock.len() {
        5 => format!("{clock}:00"),
        8 => clock.to_string(),
        _ => return None,
    };
    let digits_only = |part: &str| {
        part.bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b':' || byte == b'-')
    };
    if !digits_only(date)
        || !digits_only(&clock)
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let milliseconds = format!("{fraction:0<3}");
    Some(format!("{date}T{clock}.{}Z", &milliseconds[..3]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn database_timestamps_under_at_keys_become_typeorm_iso_strings() {
        let mut value = json!({
            "createdAt": "2026-11-25 03:24:31",
            "updatedAt": "2026-11-25 03:24:31.9",
            "task": { "createdAt": "2026-11-25T03:24:31.906Z" },
            "pairedAt": "2026-11-25T03:24:31+09:00",
            "title": "2026-11-25 03:24:31"
        });
        normalize_database_dates(&mut value);
        assert_eq!(value["createdAt"], "2026-11-25T03:24:31.000Z");
        assert_eq!(value["updatedAt"], "2026-11-25T03:24:31.900Z");
        assert_eq!(value["task"]["createdAt"], "2026-11-25T03:24:31.906Z");
        assert_eq!(value["pairedAt"], "2026-11-25T03:24:31+09:00");
        assert_eq!(value["title"], "2026-11-25 03:24:31");
    }
}
