//! TS 앱의 TypeORM 마이그레이션을 같은 이름·순서로 옮긴 것.
//! 실행 여부는 TypeORM이 쓰던 `migrations` 테이블의 `name`으로 판정하므로 TS 앱이 이미 실행한 마이그레이션은 다시 돌지 않는다.

mod sql;
pub(crate) mod steps;

use std::collections::HashSet;

use rusqlite::{Connection, TransactionBehavior, params};

use crate::error::DatabaseResult;
use crate::table_introspection::has_table;

pub struct Migration {
    /// TypeORM 클래스 이름. 끝 13자리가 timestamp다
    pub name: &'static str,
    up: fn(&Connection) -> DatabaseResult<()>,
}

impl Migration {
    pub fn timestamp(&self) -> i64 {
        let digits = &self.name[self.name.len() - 13..];
        digits
            .parse()
            .expect("마이그레이션 이름은 13자리 timestamp로 끝난다")
    }
}

/// TS `MIGRATIONS` 배열과 같은 순서. 이미 timestamp 오름차순이다
pub static MIGRATIONS: [Migration; 17] = [
    Migration {
        name: "InitialSchema1770854400000",
        up: steps::initial_schema,
    },
    Migration {
        name: "AddPrUrlToKanbanTasks1770854400001",
        up: steps::add_pr_url_to_kanban_tasks,
    },
    Migration {
        name: "AddIsWorktreeToProjects1770854400002",
        up: steps::add_is_worktree_to_projects,
    },
    Migration {
        name: "AddPaneLayoutConfig1771048256887",
        up: steps::add_pane_layout_config,
    },
    Migration {
        name: "AssignDisplayOrder1771166346785",
        up: steps::assign_display_order,
    },
    Migration {
        name: "AddAppSettings1771166907165",
        up: steps::add_app_settings,
    },
    Migration {
        name: "AddPendingStatus1771171200000",
        up: steps::schema_unchanged,
    },
    Migration {
        name: "RemoveBranchNameUnique1771257600000",
        up: steps::remove_branch_name_unique,
    },
    Migration {
        name: "AddColorIndexToProjects1771343199455",
        up: steps::add_color_index_to_projects,
    },
    Migration {
        name: "AddPriorityToKanbanTasks1771344000000",
        up: steps::add_priority_to_kanban_tasks,
    },
    Migration {
        name: "ReplaceColorIndexWithColor1771388085809",
        up: steps::replace_color_index_with_color,
    },
    Migration {
        name: "FillEmptyBaseBranch1771400000000",
        up: steps::fill_empty_base_branch,
    },
    Migration {
        name: "AddIconDataUrlToProjects1771500000000",
        up: steps::add_icon_data_url_to_projects,
    },
    Migration {
        name: "RescopeProjectNamesPerHost1771600000000",
        up: steps::rescope_project_names_per_host,
    },
    Migration {
        name: "AddTerminalSessionType1771700000000",
        up: steps::schema_unchanged,
    },
    Migration {
        name: "DropDisplayOrderFromKanbanTasks1771800000000",
        up: steps::drop_display_order_from_kanban_tasks,
    },
    Migration {
        name: "AddTaskDiffStats1771900000000",
        up: steps::add_task_diff_stats,
    },
];

/// 작업 테이블은 있는데 마이그레이션 기록이 비어 있는 옛 DB를 모든 마이그레이션이 실행된 것으로 기록한다.
/// 이런 DB의 스키마는 앞서 `schema::ensure_sqlite_database_ready`가 이미 최신으로 맞춰 둔다.
pub fn baseline_existing_database(connection: &Connection) -> DatabaseResult<()> {
    if !has_table(connection, "kanban_tasks")? {
        return Ok(());
    }

    let transaction =
        rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Deferred)?;
    transaction.execute(sql::CREATE_BASELINE_MIGRATIONS_TABLE, [])?;
    let recorded_count: i64 = transaction.query_row(
        r#"SELECT COUNT(1) AS "count" FROM "migrations""#,
        [],
        |row| row.get(0),
    )?;
    if recorded_count == 0 {
        for migration in &MIGRATIONS {
            record_migration(&transaction, migration)?;
        }
    }
    transaction.commit()?;
    Ok(())
}

fn record_migration(connection: &Connection, migration: &Migration) -> DatabaseResult<()> {
    connection.execute(
        r#"INSERT INTO "migrations"("timestamp", "name") VALUES (?, ?)"#,
        params![migration.timestamp(), migration.name],
    )?;
    Ok(())
}

fn executed_migration_names(connection: &Connection) -> DatabaseResult<HashSet<String>> {
    let mut statement = connection.prepare(r#"SELECT "name" FROM "migrations""#)?;
    let names = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<HashSet<String>, _>>()?;
    Ok(names)
}

/// 아직 실행하지 않은 마이그레이션을 TypeORM `transaction: "all"`과 같게 한 트랜잭션에서 실행하고 이름을 돌려준다.
/// 테이블 재생성 중 DROP TABLE이 다른 테이블의 연결을 지우지 않도록 그동안 foreign_keys를 끈다.
pub fn run_pending_migrations(connection: &Connection) -> DatabaseResult<Vec<&'static str>> {
    if !has_table(connection, "migrations")? {
        connection.execute(sql::CREATE_TYPEORM_MIGRATIONS_TABLE, [])?;
    }
    let executed_names = executed_migration_names(connection)?;
    let pending_migrations: Vec<&Migration> = MIGRATIONS
        .iter()
        .filter(|migration| !executed_names.contains(migration.name))
        .collect();
    if pending_migrations.is_empty() {
        return Ok(Vec::new());
    }

    connection.pragma_update(None, "foreign_keys", "OFF")?;
    let migration_result = apply_in_one_transaction(connection, &pending_migrations);
    connection.pragma_update(None, "foreign_keys", "ON")?;
    migration_result?;

    Ok(pending_migrations
        .iter()
        .map(|migration| migration.name)
        .collect())
}

fn apply_in_one_transaction(
    connection: &Connection,
    migrations: &[&Migration],
) -> DatabaseResult<()> {
    let transaction =
        rusqlite::Transaction::new_unchecked(connection, TransactionBehavior::Deferred)?;
    for migration in migrations {
        (migration.up)(&transaction)?;
        record_migration(&transaction, migration)?;
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests;
