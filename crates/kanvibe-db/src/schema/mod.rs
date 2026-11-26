//! 이미 작업 테이블이 있는 DB의 스키마를 멱등하게 최신으로 맞추는 복구. TS `sqliteSchema.ts`를 옮겼다.
//! 마이그레이션 기록 없이 baseline 처리되는 옛 DB는 마이그레이션이 돌지 않으므로 이 복구가 스키마를 책임진다.

mod sql;

use std::path::Path;

use rusqlite::{Connection, TransactionBehavior};

use crate::error::{DatabaseError, DatabaseResult};
use crate::table_introspection::{
    ColumnDefinition, add_column_if_missing, find_unique_project_name_index, index_columns,
    quote_identifier, table_columns,
};

/// 앱 시작 시 DB를 처음 열 때와 같은 연결 설정
pub(crate) fn configure_connection(connection: &Connection) -> DatabaseResult<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn ensure_columns(connection: &Connection) -> DatabaseResult<()> {
    let missing_column_definitions = [
        (
            "projects",
            "is_worktree",
            "is_worktree INTEGER NOT NULL DEFAULT 0",
        ),
        ("projects", "color", "color TEXT DEFAULT NULL"),
        (
            "projects",
            "icon_data_url",
            "icon_data_url TEXT DEFAULT NULL",
        ),
        ("kanban_tasks", "project_id", "project_id TEXT"),
        ("kanban_tasks", "base_branch", "base_branch TEXT"),
        ("kanban_tasks", "pr_url", "pr_url TEXT"),
        ("kanban_tasks", "priority", "priority TEXT DEFAULT NULL"),
        (
            "pane_layout_configs",
            "panes",
            "panes TEXT NOT NULL DEFAULT '[]'",
        ),
    ];
    for (table_name, column_name, definition) in missing_column_definitions {
        add_column_if_missing(connection, table_name, column_name, definition)?;
    }
    Ok(())
}

/// `CREATE INDEX IF NOT EXISTS`는 이름이 같으면 정의가 달라도 넘어가므로, 컬럼 구성이 달라진 인덱스는 먼저 지운다.
/// 지우지 않으면 baseline DB의 인덱스가 옛 컬럼에 남아 정렬이 매번 임시 B-tree로 떨어진다.
fn drop_index_with_different_columns(
    connection: &Connection,
    index_name: &str,
    expected_columns: &[&str],
) -> DatabaseResult<()> {
    let actual_columns = index_columns(connection, index_name)?;
    if actual_columns.is_empty() {
        return Ok(());
    }
    let is_expected = actual_columns.len() == expected_columns.len()
        && actual_columns
            .iter()
            .zip(expected_columns)
            .all(|(actual, expected)| actual.as_deref() == Some(*expected));
    if !is_expected {
        connection.execute(
            &format!("DROP INDEX IF EXISTS {}", quote_identifier(index_name)),
            [],
        )?;
    }
    Ok(())
}

fn ensure_indexes(connection: &Connection) -> DatabaseResult<()> {
    drop_index_with_different_columns(
        connection,
        "idx_kanban_tasks_status_order",
        &["status", "updated_at"],
    )?;
    drop_index_with_different_columns(
        connection,
        "idx_kanban_tasks_project_branch",
        &["project_id", "branch_name"],
    )?;
    connection.execute_batch(sql::ENSURE_INDEXES)?;
    Ok(())
}

/// 기존 컬럼 정의를 그대로 옮긴다. 정의를 새로 적으면 이 파일이 모르는 레거시 컬럼이 재생성 중에 사라진다
fn format_column_definition(column: &ColumnDefinition) -> String {
    let declared_type = if column.declared_type.is_empty() {
        "TEXT"
    } else {
        &column.declared_type
    };
    let mut definition = vec![quote_identifier(&column.name), declared_type.to_string()];
    if column.is_primary_key {
        definition.push("PRIMARY KEY".to_string());
    }
    if column.is_not_null {
        definition.push("NOT NULL".to_string());
    }
    if let Some(default_value) = &column.default_value {
        definition.push(format!("DEFAULT {default_value}"));
    }
    definition.join(" ")
}

fn rebuild_projects_without_unique_name(connection: &Connection) -> DatabaseResult<()> {
    let columns = table_columns(connection, "projects")?;
    let column_definitions: Vec<String> = columns.iter().map(format_column_definition).collect();
    let quoted_names: Vec<String> = columns
        .iter()
        .map(|column| quote_identifier(&column.name))
        .collect();
    let column_names = quoted_names.join(", ");
    let rebuild_sql = format!(
        "\n        CREATE TABLE projects_rebuilt (\n          {}\n        );\n\n        INSERT INTO projects_rebuilt ({column_names}) SELECT {column_names} FROM projects;\n\n        DROP TABLE projects;\n\n        ALTER TABLE projects_rebuilt RENAME TO projects;\n      ",
        column_definitions.join(",\n          ")
    );

    let transaction =
        rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Deferred)?;
    transaction.execute_batch(&rebuild_sql)?;
    transaction.commit()?;
    Ok(())
}

/// 표시 이름 유일성은 PC 단위로만 따지므로 옛 DB에 남은 projects.name 전역 UNIQUE를 떨어뜨린다.
/// 인라인 UNIQUE는 테이블을 다시 만들어야 지워지고, DROP TABLE이 작업의 project_id를 끊지 않도록 foreign_keys를 끈다.
fn drop_global_unique_project_name(connection: &Connection) -> DatabaseResult<()> {
    if find_unique_project_name_index(connection)?.is_none() {
        return Ok(());
    }
    connection.pragma_update(None, "foreign_keys", "OFF")?;
    let rebuild_result = rebuild_projects_without_unique_name(connection);
    connection.pragma_update(None, "foreign_keys", "ON")?;
    rebuild_result
}

fn create_parent_directory(database_path: &Path) -> DatabaseResult<()> {
    let Some(parent_directory) = database_path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent_directory).map_err(|source| DatabaseError::File {
        path: parent_directory.to_path_buf(),
        source,
    })
}

pub fn ensure_sqlite_database_ready(database_path: &Path) -> DatabaseResult<()> {
    create_parent_directory(database_path)?;
    let connection = Connection::open(database_path)?;
    configure_connection(&connection)?;

    let transaction =
        rusqlite::Transaction::new_unchecked(&connection, TransactionBehavior::Deferred)?;
    transaction.execute_batch(sql::ENSURE_BASE_TABLES)?;
    ensure_columns(&transaction)?;
    ensure_indexes(&transaction)?;
    transaction.commit()?;

    drop_global_unique_project_name(&connection)
}

/// 패키지에 넣을 시드 DB를 새로 만든다. 첫 실행은 이 파일을 복사해서 시작한다
pub fn build_seed_database(output_path: &Path) -> DatabaseResult<()> {
    create_parent_directory(output_path)?;
    if output_path.exists() {
        std::fs::remove_file(output_path).map_err(|source| DatabaseError::File {
            path: output_path.to_path_buf(),
            source,
        })?;
    }
    ensure_sqlite_database_ready(output_path)
}
