//! 보드 정렬 순서 설정. 기준을 고른 순서대로 쌓고 기준마다 방향을 뒤집을 수 있다.
//! 기준이 하나도 없으면 보드는 DB가 내려준 기본 순서인 최근 수정순을 그대로 쓴다.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BoardSortField {
    Priority,
    CreatedAt,
    UpdatedAt,
    Title,
    Project,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BoardSortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardSortKey {
    pub field: BoardSortField,
    pub direction: BoardSortDirection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardSortPreference {
    pub keys: Vec<BoardSortKey>,
}

/// 저장된 문자열을 정렬 설정으로 읽는다. 읽을 수 없으면 기본값(기준 없음)으로 되돌린다.
/// 알아볼 수 없는 기준은 버리고, 같은 기준이 두 번 나오면 앞의 것만 남기며, 수동 배치 시절의 `mode` 필드는 무시한다.
pub fn parse_board_sort_preference(raw_value: Option<&str>) -> BoardSortPreference {
    let Some(raw_value) = raw_value.filter(|value| !value.is_empty()) else {
        return BoardSortPreference::default();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(raw_value) else {
        return BoardSortPreference::default();
    };
    let raw_keys = parsed.get("keys").and_then(Value::as_array);

    BoardSortPreference {
        keys: raw_keys
            .map(|keys| parse_sort_keys(keys))
            .unwrap_or_default(),
    }
}

fn parse_sort_keys(raw_keys: &[Value]) -> Vec<BoardSortKey> {
    let mut seen_fields = HashSet::new();
    raw_keys
        .iter()
        .filter_map(|raw_key| {
            let field: BoardSortField =
                serde_json::from_value(raw_key.get("field")?.clone()).ok()?;
            let direction = raw_key
                .get("direction")
                .and_then(|direction| serde_json::from_value(direction.clone()).ok())
                .unwrap_or_default();
            seen_fields
                .insert(field)
                .then_some(BoardSortKey { field, direction })
        })
        .collect()
}

pub fn serialize_board_sort_preference(preference: &BoardSortPreference) -> String {
    serde_json::to_string(preference).expect("정렬 설정은 항상 JSON으로 직렬화된다")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_the_saved_keys() {
        let preference = BoardSortPreference {
            keys: vec![
                BoardSortKey {
                    field: BoardSortField::Priority,
                    direction: BoardSortDirection::Asc,
                },
                BoardSortKey {
                    field: BoardSortField::Title,
                    direction: BoardSortDirection::Desc,
                },
            ],
        };
        let serialized = serialize_board_sort_preference(&preference);

        assert_eq!(parse_board_sort_preference(Some(&serialized)), preference);
        assert_eq!(
            serialized,
            r#"{"keys":[{"field":"priority","direction":"asc"},{"field":"title","direction":"desc"}]}"#
        );
    }

    #[test]
    fn missing_value_gives_the_default() {
        assert_eq!(
            parse_board_sort_preference(None),
            BoardSortPreference::default()
        );
    }

    #[test]
    fn broken_json_gives_the_default() {
        assert_eq!(
            parse_board_sort_preference(Some("{ not json")),
            BoardSortPreference::default()
        );
    }

    #[test]
    fn unknown_fields_are_dropped_and_unknown_directions_become_asc() {
        let stored = r#"{"keys":[{"field":"unknownField","direction":"asc"},{"field":"createdAt","direction":"sideways"}]}"#;

        assert_eq!(
            parse_board_sort_preference(Some(stored)).keys,
            [BoardSortKey {
                field: BoardSortField::CreatedAt,
                direction: BoardSortDirection::Asc
            }]
        );
    }

    #[test]
    fn legacy_mode_field_is_dropped() {
        let stored = r#"{"keys":[{"field":"title","direction":"asc"}],"mode":"manual-first"}"#;

        assert_eq!(
            parse_board_sort_preference(Some(stored)),
            BoardSortPreference {
                keys: vec![BoardSortKey {
                    field: BoardSortField::Title,
                    direction: BoardSortDirection::Asc
                }]
            }
        );
    }

    #[test]
    fn repeated_field_keeps_only_the_first() {
        let stored = r#"{"keys":[{"field":"title","direction":"desc"},{"field":"title","direction":"asc"}]}"#;

        assert_eq!(
            parse_board_sort_preference(Some(stored)).keys,
            [BoardSortKey {
                field: BoardSortField::Title,
                direction: BoardSortDirection::Desc
            }]
        );
    }
}
