//! pane 레이아웃 설정 서비스(TS `paneLayoutService.ts`). 프로젝트별 설정이 없으면 전역 기본값을 쓴다.

use kanvibe_db::repositories::pane_layouts::{
    NewPaneLayoutConfig, find_global_pane_layout, find_project_pane_layout, insert_pane_layout,
    update_pane_layout,
};
use kanvibe_db::{Database, DatabaseError, DatabaseResult};
use kanvibe_domain::pane_layout::{PaneCommand, PaneLayoutConfig, PaneLayoutType};
use rusqlite::Row;

#[cfg(test)]
mod tests;

/// 레이아웃 저장 요청. `is_global`이면 전역 기본값을, 아니면 `project_id`의 설정을 고친다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePaneLayoutInput {
    pub layout_type: PaneLayoutType,
    pub panes: Vec<PaneCommand>,
    pub project_id: Option<String>,
    pub is_global: bool,
}

/// 글로벌 기본 pane 레이아웃 조회
pub fn get_global_pane_layout(database: &Database) -> DatabaseResult<Option<PaneLayoutConfig>> {
    find_global_pane_layout(&database.connection())
}

/// 프로젝트별 pane 레이아웃 조회 (프로젝트 설정만, fallback 없음)
pub fn get_project_pane_layout(
    database: &Database,
    project_id: &str,
) -> DatabaseResult<Option<PaneLayoutConfig>> {
    find_project_pane_layout(&database.connection(), project_id)
}

/// 프로젝트에 적용될 실제 pane 레이아웃 조회 (프로젝트 → 글로벌 fallback)
pub fn get_effective_pane_layout(
    database: &Database,
    project_id: Option<&str>,
) -> DatabaseResult<Option<PaneLayoutConfig>> {
    if let Some(project_id) = project_id.filter(|project_id| !project_id.is_empty())
        && let Some(project_config) = get_project_pane_layout(database, project_id)?
    {
        return Ok(Some(project_config));
    }
    get_global_pane_layout(database)
}

/// 저장소에 없는 전체 조회라 같은 컬럼 모양을 여기서 읽는다
fn pane_layout_from_row(row: &Row<'_>) -> rusqlite::Result<PaneLayoutConfig> {
    let stored_value_error = |column_index: usize, error: DatabaseError| {
        rusqlite::Error::FromSqlConversionFailure(
            column_index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    };
    let layout_type_text: String = row.get("layout_type")?;
    let layout_type = PaneLayoutType::parse(&layout_type_text).ok_or_else(|| {
        let unknown_value = DatabaseError::UnknownStoredValue {
            column: "pane_layout_configs.layout_type",
            value: layout_type_text.clone(),
        };
        stored_value_error(1, unknown_value)
    })?;
    let panes_json: String = row.get("panes")?;
    let panes = serde_json::from_str(&panes_json).map_err(|source| {
        let stored_json = DatabaseError::StoredJson {
            column: "pane_layout_configs.panes",
            source,
        };
        stored_value_error(2, stored_json)
    })?;
    Ok(PaneLayoutConfig {
        id: row.get("id")?,
        layout_type,
        panes,
        project_id: row.get("project_id")?,
        is_global: row.get::<_, i64>("is_global")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// 모든 pane 레이아웃 설정 조회 (글로벌 + 프로젝트별)
pub fn get_all_pane_layouts(database: &Database) -> DatabaseResult<Vec<PaneLayoutConfig>> {
    let connection = database.connection();
    let mut statement = connection.prepare(
        "SELECT id, layout_type, panes, project_id, is_global, created_at, updated_at \
         FROM pane_layout_configs ORDER BY is_global DESC, created_at ASC",
    )?;
    let layouts = statement
        .query_map([], pane_layout_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(layouts)
}

/// 고칠 대상이 이미 있으면 그 행을, 없으면 `None`을 돌려준다
fn find_existing_pane_layout(
    database: &Database,
    input: &SavePaneLayoutInput,
) -> DatabaseResult<Option<PaneLayoutConfig>> {
    if input.is_global {
        return get_global_pane_layout(database);
    }
    match input
        .project_id
        .as_deref()
        .filter(|project_id| !project_id.is_empty())
    {
        Some(project_id) => get_project_pane_layout(database, project_id),
        None => Ok(None),
    }
}

/// pane 레이아웃 저장 (upsert 패턴: 기존 설정이 있으면 업데이트, 없으면 생성)
pub fn save_pane_layout(
    database: &Database,
    input: SavePaneLayoutInput,
) -> DatabaseResult<PaneLayoutConfig> {
    if let Some(mut existing) = find_existing_pane_layout(database, &input)? {
        existing.layout_type = input.layout_type;
        existing.panes = input.panes.clone();
        update_pane_layout(&database.connection(), &existing)?;
        // 갱신 시각은 DB가 정하므로 저장한 행을 다시 읽어 돌려준다
        let saved = find_existing_pane_layout(database, &input)?;
        return Ok(saved.unwrap_or(existing));
    }

    let new_layout = NewPaneLayoutConfig {
        layout_type: input.layout_type,
        panes: input.panes,
        project_id: input.project_id.filter(|project_id| !project_id.is_empty()),
        is_global: input.is_global,
    };
    insert_pane_layout(&database.connection(), &new_layout)
}

/// pane 레이아웃 삭제. 지운 행이 있었는지 돌려준다
pub fn delete_pane_layout(database: &Database, layout_id: &str) -> DatabaseResult<bool> {
    let affected_rows = database
        .connection()
        .execute("DELETE FROM pane_layout_configs WHERE id = ?1", [layout_id])?;
    Ok(affected_rows > 0)
}
