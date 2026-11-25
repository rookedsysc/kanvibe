//! 없음과 `null`을 구별해야 하는 JSON 필드. 저장된 JSON을 읽었다가 다시 쓸 때 필드가 생기거나 사라지지 않게 한다.
//! `Option<Option<T>>`에서 바깥 `None`은 필드 없음, `Some(None)`은 `null`이다.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<S, T>(value: &Option<Option<T>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    match value {
        Some(inner_value) => inner_value.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
