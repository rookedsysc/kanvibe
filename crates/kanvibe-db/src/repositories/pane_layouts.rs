use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutConfig, PaneLayoutType};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{new_record_id, parse_stored_value};
use crate::error::{DatabaseError, DatabaseResult};

const PANE_LAYOUT_COLUMNS: &str =
    "id, layout_type, panes, project_id, is_global, created_at, updated_at";

/// 새로 저장할 레이아웃. 프로젝트가 없고 `is_global`이면 전역 기본값이다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPaneLayoutConfig {
    pub layout_type: PaneLayoutType,
    pub panes: Vec<PaneCommand>,
    pub project_id: Option<String>,
    pub is_global: bool,
}

fn pane_layout_from_row(row: &Row<'_>) -> rusqlite::Result<PaneLayoutConfig> {
    let panes_json: String = row.get("panes")?;
    let panes = serde_json::from_str(&panes_json).map_err(|source| {
        let stored_json = DatabaseError::StoredJson {
            column: "pane_layout_configs.panes",
            source,
        };
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(stored_json),
        )
    })?;
    Ok(PaneLayoutConfig {
        id: row.get("id")?,
        layout_type: parse_stored_value(
            "pane_layout_configs.layout_type",
            row.get("layout_type")?,
            PaneLayoutType::parse,
        )?,
        panes,
        project_id: row.get("project_id")?,
        is_global: row.get::<_, i64>("is_global")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// TypeORM `simple-json` 컬럼과 같게 공백 없는 JSON으로 저장한다
fn serialize_panes(panes: &[PaneCommand]) -> String {
    serde_json::to_string(panes).expect("pane 명령 목록은 항상 JSON으로 직렬화된다")
}

pub fn insert_pane_layout(
    connection: &Connection,
    layout: &NewPaneLayoutConfig,
) -> DatabaseResult<PaneLayoutConfig> {
    let layout_id = new_record_id();
    connection.execute(
        "INSERT INTO pane_layout_configs (id, layout_type, panes, project_id, is_global) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![layout_id, layout.layout_type.as_str(), serialize_panes(&layout.panes), layout.project_id, layout.is_global],
    )?;
    let sql = format!("SELECT {PANE_LAYOUT_COLUMNS} FROM pane_layout_configs WHERE id = ?1");
    Ok(connection.query_row(&sql, [&layout_id], pane_layout_from_row)?)
}

/// 프로젝트가 따로 정하지 않았을 때 쓰는 전역 기본 레이아웃
pub fn find_global_pane_layout(
    connection: &Connection,
) -> DatabaseResult<Option<PaneLayoutConfig>> {
    let sql = format!(
        "SELECT {PANE_LAYOUT_COLUMNS} FROM pane_layout_configs WHERE project_id IS NULL AND is_global = 1 LIMIT 1"
    );
    Ok(connection
        .query_row(&sql, [], pane_layout_from_row)
        .optional()?)
}

pub fn find_project_pane_layout(
    connection: &Connection,
    project_id: &str,
) -> DatabaseResult<Option<PaneLayoutConfig>> {
    let sql =
        format!("SELECT {PANE_LAYOUT_COLUMNS} FROM pane_layout_configs WHERE project_id = ?1");
    Ok(connection
        .query_row(&sql, [project_id], pane_layout_from_row)
        .optional()?)
}

pub fn update_pane_layout(
    connection: &Connection,
    layout: &PaneLayoutConfig,
) -> DatabaseResult<()> {
    connection.execute(
        "UPDATE pane_layout_configs SET layout_type = ?2, panes = ?3, project_id = ?4, is_global = ?5, \
         updated_at = datetime('now') WHERE id = ?1",
        params![layout.id, layout.layout_type.as_str(), serialize_panes(&layout.panes), layout.project_id, layout.is_global],
    )?;
    Ok(())
}

pub fn delete_pane_layout(connection: &Connection, layout_id: &str) -> DatabaseResult<()> {
    connection.execute("DELETE FROM pane_layout_configs WHERE id = ?1", [layout_id])?;
    Ok(())
}
