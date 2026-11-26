//! 테이블별 조회·저장. 시각 컬럼은 TypeORM처럼 DB의 `datetime('now')`로 채워 TS 앱이 쓰던 형식을 지킨다.

pub mod app_settings;
pub mod pane_layouts;
pub mod projects;
pub mod task_diff_stats;
pub mod tasks;

use rusqlite::types::Type;

use crate::error::DatabaseError;

/// 저장된 문자열을 어휘로 읽는다. 모르는 값이면 행 변환 오류로 돌려준다
pub(crate) fn parse_stored_value<T>(
    column: &'static str,
    raw_value: String,
    parse: impl Fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    parse(&raw_value).ok_or_else(|| {
        let unknown_value = DatabaseError::UnknownStoredValue {
            column,
            value: raw_value,
        };
        rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(unknown_value))
    })
}

pub(crate) fn new_record_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
