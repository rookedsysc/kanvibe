//! PRAGMA로 테이블·인덱스 구성을 읽는 도우미. 스키마 복구와 마이그레이션이 같은 판정을 쓴다.

use rusqlite::{Connection, OptionalExtension};

use crate::error::DatabaseResult;

pub(crate) fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub(crate) fn has_table(connection: &Connection, table_name: &str) -> DatabaseResult<bool> {
    let found = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            [table_name],
            |_| Ok(()),
        )
        .optional()?;
    Ok(found.is_some())
}

/// `PRAGMA table_info` 한 행 중 컬럼 정의를 다시 쓰는 데 필요한 값
#[derive(Debug, Clone)]
pub(crate) struct ColumnDefinition {
    pub name: String,
    pub declared_type: String,
    pub is_not_null: bool,
    pub default_value: Option<String>,
    pub is_primary_key: bool,
}

pub(crate) fn table_columns(
    connection: &Connection,
    table_name: &str,
) -> DatabaseResult<Vec<ColumnDefinition>> {
    let sql = format!("PRAGMA table_info({})", quote_identifier(table_name));
    let mut statement = connection.prepare(&sql)?;
    let columns = statement
        .query_map([], |row| {
            Ok(ColumnDefinition {
                name: row.get("name")?,
                declared_type: row.get("type")?,
                is_not_null: row.get::<_, i64>("notnull")? != 0,
                default_value: row.get("dflt_value")?,
                is_primary_key: row.get::<_, i64>("pk")? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns)
}

pub(crate) fn column_names(
    connection: &Connection,
    table_name: &str,
) -> DatabaseResult<Vec<String>> {
    let columns = table_columns(connection, table_name)?;
    Ok(columns.into_iter().map(|column| column.name).collect())
}

/// 인덱스가 덮는 컬럼. 식 인덱스의 자리는 `None`이다
pub(crate) fn index_columns(
    connection: &Connection,
    index_name: &str,
) -> DatabaseResult<Vec<Option<String>>> {
    let sql = format!("PRAGMA index_info({})", quote_identifier(index_name));
    let mut statement = connection.prepare(&sql)?;
    let columns = statement
        .query_map([], |row| row.get::<_, Option<String>>("name"))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns)
}

/// name 컬럼 하나만 덮는 UNIQUE 인덱스. 인라인 UNIQUE 제약은 이름 없는 autoindex로 잡혀 구성 컬럼으로 판별한다
pub(crate) fn find_unique_project_name_index(
    connection: &Connection,
) -> DatabaseResult<Option<String>> {
    let mut statement = connection.prepare(r#"PRAGMA index_list("projects")"#)?;
    let unique_indexes = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>("name")?, row.get::<_, i64>("unique")?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    for (index_name, _) in unique_indexes
        .into_iter()
        .filter(|(_, unique)| *unique != 0)
    {
        if index_columns(connection, &index_name)? == [Some("name".to_string())] {
            return Ok(Some(index_name));
        }
    }
    Ok(None)
}

/// 컬럼이 없을 때만 추가한다. SQLite는 `ADD COLUMN IF NOT EXISTS`를 지원하지 않는다
pub(crate) fn add_column_if_missing(
    connection: &Connection,
    table_name: &str,
    column_name: &str,
    definition: &str,
) -> DatabaseResult<()> {
    if column_names(connection, table_name)?
        .iter()
        .any(|name| name == column_name)
    {
        return Ok(());
    }
    let sql = format!(
        "ALTER TABLE {} ADD COLUMN {definition}",
        quote_identifier(table_name)
    );
    connection.execute(&sql, [])?;
    Ok(())
}
