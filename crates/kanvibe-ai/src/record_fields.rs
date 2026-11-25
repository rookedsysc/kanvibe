//! 기록 JSON에서 필드를 꺼내는 JS 관용구. `a ?? b ?? c`처럼 null·없음을 건너뛰는 선택을 한곳에 둔다.

use serde_json::Value;

/// `record.a ?? record.b ?? ...`. 모두 없거나 null이면 `None`이다
pub fn first_present<'a>(record: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .filter_map(|key| record.get(*key))
        .find(|value| !value.is_null())
}

/// [`first_present`]의 결과. 없으면 JSON null로 본다(JS `undefined`를 넘기는 자리)
pub fn first_present_or_null<'a>(record: &'a Value, keys: &[&str]) -> &'a Value {
    first_present(record, keys).unwrap_or(&Value::Null)
}

/// `typeof record[key] === "string" ? record[key] : null`
pub fn string_field<'a>(record: &'a Value, key: &str) -> Option<&'a str> {
    record.get(key).and_then(Value::as_str)
}

/// 검색어가 없거나, 값 중 하나라도 소문자로 바꿔 검색어를 담고 있으면 참이다
pub fn matches_query(query: Option<&str>, values: &[Option<&str>]) -> bool {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return true;
    };
    let normalized_query = query.to_lowercase();
    values
        .iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(&normalized_query))
}
