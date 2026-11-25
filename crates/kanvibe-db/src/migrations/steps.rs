//! TS 마이그레이션 17개의 `up`. 실행하는 SQL과 순서를 TS와 같게 둬서 같은 스키마 텍스트가 남게 한다.

use std::collections::{HashMap, HashSet};

use kanvibe_domain::project::{posix_basename, resolve_unique_project_name};
use rusqlite::{Connection, params};

use super::sql;
use crate::error::{DatabaseError, DatabaseResult};
use crate::table_introspection::{
    add_column_if_missing, column_names, find_unique_project_name_index,
};

pub(crate) fn initial_schema(connection: &Connection) -> DatabaseResult<()> {
    for statement in [
        sql::CREATE_LEGACY_PROJECTS,
        sql::CREATE_LEGACY_KANBAN_TASKS,
        sql::CREATE_BRANCH_NAME_UNIQUE_INDEX,
        sql::CREATE_LEGACY_STATUS_ORDER_INDEX,
        sql::CREATE_PROJECT_BRANCH_INDEX,
    ] {
        connection.execute(statement, [])?;
    }
    Ok(())
}

pub(crate) fn add_pr_url_to_kanban_tasks(connection: &Connection) -> DatabaseResult<()> {
    add_column_if_missing(connection, "kanban_tasks", "pr_url", r#""pr_url" TEXT"#)
}

pub(crate) fn add_is_worktree_to_projects(connection: &Connection) -> DatabaseResult<()> {
    let definition = r#""is_worktree" INTEGER NOT NULL DEFAULT 0"#;
    add_column_if_missing(connection, "projects", "is_worktree", definition)
}

pub(crate) fn add_pane_layout_config(connection: &Connection) -> DatabaseResult<()> {
    connection.execute(sql::CREATE_PANE_LAYOUT_CONFIGS, [])?;
    Ok(())
}

/// 같은 시각에 만든 작업도 id 순으로 서로 다른 순번을 받는다
pub(crate) fn assign_display_order(connection: &Connection) -> DatabaseResult<()> {
    connection.execute(sql::ASSIGN_DISPLAY_ORDER, [])?;
    Ok(())
}

pub(crate) fn add_app_settings(connection: &Connection) -> DatabaseResult<()> {
    connection.execute(sql::CREATE_APP_SETTINGS, [])?;
    Ok(())
}

/// SQLite TEXT 컬럼은 어떤 문자열이든 받으므로 상태 값을 늘려도 바꿀 스키마가 없다
pub(crate) fn schema_unchanged(_connection: &Connection) -> DatabaseResult<()> {
    Ok(())
}

pub(crate) fn remove_branch_name_unique(connection: &Connection) -> DatabaseResult<()> {
    connection.execute(r#"DROP INDEX IF EXISTS "UQ_kanban_tasks_branch_name""#, [])?;
    Ok(())
}

pub(crate) fn add_color_index_to_projects(connection: &Connection) -> DatabaseResult<()> {
    add_column_if_missing(
        connection,
        "projects",
        "color_index",
        r#""color_index" INTEGER"#,
    )
}

pub(crate) fn add_priority_to_kanban_tasks(connection: &Connection) -> DatabaseResult<()> {
    add_column_if_missing(
        connection,
        "kanban_tasks",
        "priority",
        r#""priority" TEXT DEFAULT NULL"#,
    )
}

pub(crate) fn replace_color_index_with_color(connection: &Connection) -> DatabaseResult<()> {
    let columns = column_names(connection, "projects")?;
    let has_color_index = columns.iter().any(|name| name == "color_index");
    let has_color = columns.iter().any(|name| name == "color");

    if has_color_index && !has_color {
        connection.execute(
            r#"ALTER TABLE "projects" RENAME COLUMN "color_index" TO "color""#,
            [],
        )?;
    } else if !has_color {
        add_column_if_missing(
            connection,
            "projects",
            "color",
            r#""color" TEXT DEFAULT NULL"#,
        )?;
    }
    Ok(())
}

pub(crate) fn fill_empty_base_branch(connection: &Connection) -> DatabaseResult<()> {
    connection.execute(sql::FILL_EMPTY_BASE_BRANCH, [])?;
    Ok(())
}

pub(crate) fn add_icon_data_url_to_projects(connection: &Connection) -> DatabaseResult<()> {
    let definition = r#""icon_data_url" TEXT DEFAULT NULL"#;
    add_column_if_missing(connection, "projects", "icon_data_url", definition)
}

/// 재생성한 테이블이 갖는 컬럼. 여기에 없는 컬럼이 실제 테이블에 있으면 재생성이 그 데이터를 지운다
const REBUILT_PROJECT_COLUMNS: [&str; 9] = [
    "id",
    "name",
    "repo_path",
    "default_branch",
    "ssh_host",
    "is_worktree",
    "color",
    "icon_data_url",
    "created_at",
];

/// 재생성이 받아줄 수 없는 컬럼이 있으면 조용히 잃는 대신 멈춘다
fn resolve_project_copy_columns(connection: &Connection) -> DatabaseResult<String> {
    let columns = column_names(connection, "projects")?;
    let dropped_columns: Vec<String> = columns
        .iter()
        .filter(|name| !REBUILT_PROJECT_COLUMNS.contains(&name.as_str()))
        .cloned()
        .collect();
    if !dropped_columns.is_empty() {
        return Err(DatabaseError::UnsupportedProjectColumns(dropped_columns));
    }

    let quoted_columns: Vec<String> = columns.iter().map(|name| format!("\"{name}\"")).collect();
    Ok(quoted_columns.join(", "))
}

/// SQLite는 인라인 UNIQUE 제약을 DROP INDEX로 지울 수 없어 테이블을 다시 만든다.
/// 마이그레이션 실행 중에는 foreign_keys가 꺼져 있어 DROP TABLE이 kanban_tasks의 project_id를 지우지 않는다.
fn rebuild_projects_table(connection: &Connection) -> DatabaseResult<()> {
    let copy_columns = resolve_project_copy_columns(connection)?;
    connection.execute(sql::CREATE_PROJECTS_REBUILT, [])?;
    let copy_sql = format!(
        r#"INSERT INTO "projects_rebuilt" ({copy_columns}) SELECT {copy_columns} FROM "projects""#
    );
    connection.execute(&copy_sql, [])?;
    connection.execute(r#"DROP TABLE "projects""#, [])?;
    connection.execute(r#"ALTER TABLE "projects_rebuilt" RENAME TO "projects""#, [])?;
    Ok(())
}

struct ProjectNameRow {
    id: String,
    name: String,
    repo_path: String,
    ssh_host: Option<String>,
}

/// 전역 유일 규칙 때문에 붙었던 상위 폴더와 숫자를 같은 PC 기준으로 다시 계산한다.
/// 등록 순서를 지켜야 먼저 등록한 프로젝트가 계속 짧은 이름을 갖는다.
fn rescope_project_names(connection: &Connection) -> DatabaseResult<()> {
    let mut statement = connection.prepare(
        r#"SELECT id, name, repo_path, ssh_host FROM "projects" ORDER BY created_at, id"#,
    )?;
    let projects = statement
        .query_map([], |row| {
            Ok(ProjectNameRow {
                id: row.get(0)?,
                name: row.get(1)?,
                repo_path: row.get(2)?,
                ssh_host: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut names_by_host: HashMap<String, HashSet<String>> = HashMap::new();

    for project in projects {
        let names_on_same_host = names_by_host
            .entry(project.ssh_host.unwrap_or_default())
            .or_default();
        let preferred_name = posix_basename(&project.repo_path);
        let Some(rescoped_name) =
            resolve_unique_project_name(preferred_name, &project.repo_path, names_on_same_host)
        else {
            // 이미 등록된 프로젝트는 구분할 이름이 없다고 지울 수 없으므로 기존 이름을 그대로 둔다
            names_on_same_host.insert(project.name);
            continue;
        };
        if rescoped_name != project.name {
            connection.execute(
                r#"UPDATE "projects" SET name = ? WHERE id = ?"#,
                params![rescoped_name, project.id],
            )?;
        }
    }
    Ok(())
}

pub(crate) fn rescope_project_names_per_host(connection: &Connection) -> DatabaseResult<()> {
    if find_unique_project_name_index(connection)?.is_some() {
        rebuild_projects_table(connection)?;
    }
    rescope_project_names(connection)
}

/// 기본 순서가 최근 수정순이 되면서 수동 순번을 떨어뜨리고 인덱스도 그 순서를 덮게 바꾼다
pub(crate) fn drop_display_order_from_kanban_tasks(connection: &Connection) -> DatabaseResult<()> {
    connection.execute(
        r#"DROP INDEX IF EXISTS "idx_kanban_tasks_status_order""#,
        [],
    )?;
    if column_names(connection, "kanban_tasks")?
        .iter()
        .any(|name| name == "display_order")
    {
        connection.execute(
            r#"ALTER TABLE "kanban_tasks" DROP COLUMN "display_order""#,
            [],
        )?;
    }
    connection.execute(sql::CREATE_STATUS_UPDATED_INDEX, [])?;
    Ok(())
}

pub(crate) fn add_task_diff_stats(connection: &Connection) -> DatabaseResult<()> {
    connection.execute(sql::CREATE_TASK_DIFF_STATS, [])?;
    Ok(())
}
