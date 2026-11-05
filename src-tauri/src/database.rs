use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};

pub struct Database(pub Mutex<Connection>);
pub type Result<T> = std::result::Result<T, String>;
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}
pub fn nullable(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let connection = Connection::open(path).map_err(|e| e.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS projects(id varchar(36) PRIMARY KEY NOT NULL,name varchar(255) NOT NULL,repo_path varchar(500) NOT NULL,default_branch varchar(255) NOT NULL DEFAULT 'main',ssh_host varchar(255),is_worktree boolean NOT NULL DEFAULT 0,color varchar(7),icon_data_url text,created_at datetime NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS kanban_tasks(id varchar(36) PRIMARY KEY NOT NULL,title varchar(255) NOT NULL,description text,status varchar NOT NULL DEFAULT 'todo',branch_name varchar(255),worktree_path varchar(500),session_type varchar,session_name varchar(255),ssh_host varchar(255),agent_type varchar(50),project_id varchar(36),base_branch varchar(255),pr_url varchar(500),priority varchar,created_at datetime NOT NULL DEFAULT (datetime('now')),updated_at datetime NOT NULL DEFAULT (datetime('now')),FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE SET NULL);
CREATE TABLE IF NOT EXISTS app_settings(id varchar(36) PRIMARY KEY NOT NULL,key varchar(100) NOT NULL UNIQUE,value text NOT NULL,created_at datetime NOT NULL DEFAULT (datetime('now')),updated_at datetime NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS pane_layout_configs(id varchar(36) PRIMARY KEY NOT NULL,layout_type varchar(50) NOT NULL,panes text NOT NULL,project_id varchar(36) UNIQUE,is_global boolean NOT NULL DEFAULT 0,created_at datetime NOT NULL DEFAULT (datetime('now')),updated_at datetime NOT NULL DEFAULT (datetime('now')),FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS task_diff_stats(task_id varchar(36) PRIMARY KEY NOT NULL,file_count integer NOT NULL,additions integer NOT NULL,deletions integer NOT NULL,updated_at datetime NOT NULL DEFAULT (datetime('now')),FOREIGN KEY(task_id) REFERENCES kanban_tasks(id) ON DELETE CASCADE);
").map_err(|e|e.to_string())?;
        // 배포 이력이 다른 DB에도 누락된 부가 컬럼만 보충하고 기존 행은 보존한다.
        for (table, column, definition) in [
            ("projects", "is_worktree", "boolean NOT NULL DEFAULT 0"),
            ("projects", "color", "varchar(7)"),
            ("projects", "icon_data_url", "text"),
            ("kanban_tasks", "pr_url", "varchar(500)"),
            ("kanban_tasks", "priority", "varchar"),
            ("kanban_tasks", "base_branch", "varchar(255)"),
            ("kanban_tasks", "project_id", "varchar(36)"),
            ("pane_layout_configs", "panes", "text NOT NULL DEFAULT '[]'"),
        ] {
            let mut stmt = connection
                .prepare_cached(&format!("PRAGMA table_info({table})"))
                .map_err(|e| e.to_string())?;
            let names = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| e.to_string())?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            if !names.iter().any(|s| s == column) {
                connection
                    .execute_batch(&format!(
                        "ALTER TABLE {table} ADD COLUMN {column} {definition}"
                    ))
                    .map_err(|e| e.to_string())?;
            }
        }
        migrate_legacy_constraints(&connection)?;
        connection.execute_batch("DROP INDEX IF EXISTS UQ_kanban_tasks_branch_name; CREATE INDEX IF NOT EXISTS idx_tasks_status_updated ON kanban_tasks(status,updated_at DESC); CREATE INDEX IF NOT EXISTS idx_tasks_project_branch ON kanban_tasks(project_id,branch_name); UPDATE kanban_tasks SET base_branch=(SELECT default_branch FROM projects WHERE id=kanban_tasks.project_id) WHERE project_id IS NOT NULL AND (base_branch IS NULL OR base_branch='');").map_err(|e|e.to_string())?;
        Ok(Self(Mutex::new(connection)))
    }
    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0.lock().map_err(|e| e.to_string())
    }
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        self.lock()?
            .query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())
    }
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.lock()?.execute("INSERT INTO app_settings(id,key,value,created_at,updated_at) VALUES(?1,?2,?3,?4,?4) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at",params![id(),key,value,now()]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn projects(&self) -> Result<Vec<Value>> {
        let c = self.lock()?;
        let mut s=c.prepare_cached("SELECT id,name,repo_path,default_branch,ssh_host,is_worktree,color,icon_data_url,created_at FROM projects ORDER BY created_at ASC").map_err(|e|e.to_string())?;
        let rows = s
            .query_map([], project_row)
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }
    pub fn project(&self, id: &str) -> Result<Option<Value>> {
        self.lock()?.query_row("SELECT id,name,repo_path,default_branch,ssh_host,is_worktree,color,icon_data_url,created_at FROM projects WHERE id=?1",[id],project_row).optional().map_err(|e|e.to_string())
    }
    pub fn tasks(&self) -> Result<Vec<Value>> {
        let c = self.lock()?;
        let mut s=c.prepare_cached("SELECT id,title,description,status,branch_name,worktree_path,session_type,session_name,ssh_host,agent_type,project_id,base_branch,pr_url,priority,created_at,updated_at FROM kanban_tasks ORDER BY updated_at DESC,created_at DESC").map_err(|e|e.to_string())?;
        let rows = s
            .query_map([], task_row)
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok(rows)
    }
    pub fn board_tasks(&self) -> Result<(Vec<Value>, i64)> {
        let c = self.lock()?;
        let done_total = c
            .query_row(
                "SELECT COUNT(*) FROM kanban_tasks WHERE status='done'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let mut statement=c.prepare_cached("SELECT id,title,description,status,branch_name,worktree_path,session_type,session_name,ssh_host,agent_type,project_id,base_branch,pr_url,priority,created_at,updated_at FROM kanban_tasks WHERE status!='done' OR id IN (SELECT id FROM kanban_tasks WHERE status='done' ORDER BY updated_at DESC LIMIT 20) ORDER BY updated_at DESC,created_at DESC").map_err(|e|e.to_string())?;
        let tasks = statement
            .query_map([], task_row)
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok((tasks, done_total))
    }
    pub fn done_tasks(&self, offset: i64, limit: i64) -> Result<(Vec<Value>, i64)> {
        let c = self.lock()?;
        let count = c
            .query_row(
                "SELECT COUNT(*) FROM kanban_tasks WHERE status='done'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let mut statement=c.prepare_cached("SELECT id,title,description,status,branch_name,worktree_path,session_type,session_name,ssh_host,agent_type,project_id,base_branch,pr_url,priority,created_at,updated_at FROM kanban_tasks WHERE status='done' ORDER BY updated_at DESC,created_at DESC LIMIT ?1 OFFSET ?2").map_err(|e|e.to_string())?;
        let tasks = statement
            .query_map(params![limit, offset], task_row)
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        Ok((tasks, count))
    }
    pub fn task(&self, id: &str) -> Result<Option<Value>> {
        let c = self.lock()?;
        c.query_row("SELECT id,title,description,status,branch_name,worktree_path,session_type,session_name,ssh_host,agent_type,project_id,base_branch,pr_url,priority,created_at,updated_at FROM kanban_tasks WHERE id=?1",[id],task_row).optional().map_err(|e|e.to_string())
    }
    pub fn save_task(&self, v: &Value) -> Result<Value> {
        let task_id = nullable(v, "id").unwrap_or_else(id);
        let session_name = nullable(v, "sessionName").map(|name| {
            if text(v, "sessionType") == "zellij" {
                name.chars().take(60).collect::<String>()
            } else {
                name
            }
        });
        let timestamp = now();
        self.lock()?.execute("INSERT INTO kanban_tasks(id,title,description,status,branch_name,worktree_path,session_type,session_name,ssh_host,agent_type,project_id,base_branch,pr_url,priority,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16) ON CONFLICT(id) DO UPDATE SET title=excluded.title,description=excluded.description,status=excluded.status,branch_name=excluded.branch_name,worktree_path=excluded.worktree_path,session_type=excluded.session_type,session_name=excluded.session_name,ssh_host=excluded.ssh_host,agent_type=excluded.agent_type,project_id=excluded.project_id,base_branch=excluded.base_branch,pr_url=excluded.pr_url,priority=excluded.priority,updated_at=excluded.updated_at",params![task_id,text(v,"title"),v.get("description").and_then(Value::as_str),if text(v,"status").is_empty(){"todo"}else{text(v,"status")},nullable(v,"branchName"),nullable(v,"worktreePath"),nullable(v,"sessionType"),session_name,nullable(v,"sshHost"),nullable(v,"agentType"),nullable(v,"projectId"),nullable(v,"baseBranch"),nullable(v,"prUrl"),nullable(v,"priority"),v.get("createdAt").and_then(Value::as_str).unwrap_or(&timestamp),timestamp]).map_err(|e|e.to_string())?;
        self.task(&task_id)?
            .ok_or_else(|| "저장된 작업을 찾을 수 없습니다".into())
    }
    pub fn save_project(&self, v: &Value) -> Result<Value> {
        let project_id = nullable(v, "id").unwrap_or_else(id);
        self.lock()?.execute("INSERT INTO projects(id,name,repo_path,default_branch,ssh_host,is_worktree,color,icon_data_url,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET name=excluded.name,repo_path=excluded.repo_path,default_branch=excluded.default_branch,ssh_host=excluded.ssh_host,is_worktree=excluded.is_worktree,color=excluded.color,icon_data_url=excluded.icon_data_url",params![project_id,text(v,"name"),text(v,"repoPath"),if text(v,"defaultBranch").is_empty(){"main"}else{text(v,"defaultBranch")},nullable(v,"sshHost"),v["isWorktree"].as_bool().unwrap_or(false),nullable(v,"color"),nullable(v,"iconDataUrl"),nullable(v,"createdAt").unwrap_or_else(now)]).map_err(|e|e.to_string())?;
        self.project(&project_id)?
            .ok_or_else(|| "저장된 프로젝트를 찾을 수 없습니다".into())
    }
}
fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
fn migrate_legacy_constraints(connection: &Connection) -> Result<()> {
    let indexes = {
        let mut statement = connection
            .prepare("PRAGMA index_list(projects)")
            .map_err(|e| e.to_string())?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, bool>(2)?)))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };
    let mut old_unique = false;
    for (name, unique) in indexes {
        if !unique {
            continue;
        }
        let mut statement = connection
            .prepare(&format!("PRAGMA index_info({})", identifier(&name)))
            .map_err(|e| e.to_string())?;
        let columns = statement
            .query_map([], |r| r.get::<_, String>(2))
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        if columns == ["name"] {
            old_unique = true;
            break;
        }
    }
    if !old_unique {
        return Ok(());
    }
    let columns = {
        let mut statement = connection
            .prepare("PRAGMA table_info(projects)")
            .map_err(|e| e.to_string())?;
        let columns = statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, bool>(5)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        columns
    };
    let definitions = columns
        .iter()
        .map(|(name, kind, required, default, primary)| {
            format!(
                "{} {}{}{}{}",
                identifier(name),
                if kind.is_empty() { "TEXT" } else { kind },
                if *primary { " PRIMARY KEY" } else { "" },
                if *required { " NOT NULL" } else { "" },
                default
                    .as_ref()
                    .map(|v| format!(" DEFAULT {v}"))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let names = columns
        .iter()
        .map(|(name, _, _, _, _)| identifier(name))
        .collect::<Vec<_>>()
        .join(",");
    connection
        .execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")
        .map_err(|e| e.to_string())?;
    let rebuilt=connection.execute_batch(&format!("CREATE TABLE projects_rebuilt({definitions}); INSERT INTO projects_rebuilt({names}) SELECT {names} FROM projects; DROP TABLE projects; ALTER TABLE projects_rebuilt RENAME TO projects;"));
    match rebuilt {
        Ok(()) => {
            connection
                .execute_batch("COMMIT; PRAGMA foreign_keys=ON;")
                .map_err(|e| e.to_string())?;
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK; PRAGMA foreign_keys=ON;");
            return Err(error.to_string());
        }
    }
    connection.flush_prepared_statement_cache();
    Ok(())
}

fn date(r: &rusqlite::Row<'_>, i: usize) -> rusqlite::Result<String> {
    let s: String = r.get(i)?;
    Ok(if s.contains('T') {
        s
    } else {
        format!("{}Z", s.replace(' ', "T"))
    })
}
fn project_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"repoPath":r.get::<_,String>(2)?,"defaultBranch":r.get::<_,String>(3)?,"sshHost":r.get::<_,Option<String>>(4)?,"isWorktree":r.get::<_,bool>(5)?,"color":r.get::<_,Option<String>>(6)?,"iconDataUrl":r.get::<_,Option<String>>(7)?,"createdAt":date(r,8)?}),
    )
}
fn task_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(
        json!({"id":r.get::<_,String>(0)?,"title":r.get::<_,String>(1)?,"description":r.get::<_,Option<String>>(2)?,"status":r.get::<_,String>(3)?,"branchName":r.get::<_,Option<String>>(4)?,"worktreePath":r.get::<_,Option<String>>(5)?,"sessionType":r.get::<_,Option<String>>(6)?,"sessionName":r.get::<_,Option<String>>(7)?,"sshHost":r.get::<_,Option<String>>(8)?,"agentType":r.get::<_,Option<String>>(9)?,"projectId":r.get::<_,Option<String>>(10)?,"baseBranch":r.get::<_,Option<String>>(11)?,"prUrl":r.get::<_,Option<String>>(12)?,"priority":r.get::<_,Option<String>>(13)?,"createdAt":date(r,14)?,"updatedAt":date(r,15)?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zellij_names_are_limited_at_every_task_save_while_other_types_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("app.db")).unwrap();
        let name = format!("repository-feature-{}", "branch".repeat(20));
        for session_type in ["zellij", "tmux", "terminal"] {
            let mut task = db
                .save_task(&json!({"title":"create","sessionType":session_type,"sessionName":name}))
                .unwrap();
            let expected = if session_type == "zellij" {
                name.chars().take(60).collect::<String>()
            } else {
                name.clone()
            };
            assert_eq!(task["sessionName"], expected);
            task["sessionName"] = json!(name);
            task["title"] = json!("branch/connect/root/recovery");
            let saved = db.save_task(&task).unwrap();
            assert_eq!(saved["sessionName"], expected);
        }
    }
    #[test]
    fn existing_electron_schema_and_rows_survive_open_and_partial_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE projects(id varchar PRIMARY KEY,name varchar NOT NULL,repo_path varchar NOT NULL,default_branch varchar NOT NULL,ssh_host varchar,created_at datetime NOT NULL); CREATE TABLE kanban_tasks(id varchar PRIMARY KEY,title varchar NOT NULL,description text,status varchar NOT NULL,branch_name varchar,worktree_path varchar,session_type varchar,session_name varchar,ssh_host varchar,agent_type varchar,project_id varchar,base_branch varchar,created_at datetime NOT NULL,updated_at datetime NOT NULL); INSERT INTO projects VALUES('p','project','/repo','main',NULL,'2026-01-01 00:00:00'); INSERT INTO kanban_tasks VALUES('t','한글 작업','설명','progress','feature',NULL,'tmux','project-feature',NULL,NULL,'p','main','2026-01-01 00:00:00','2026-01-02 00:00:00');").unwrap();
        drop(c);
        let db = Database::open(&path).unwrap();
        let mut task = db.task("t").unwrap().unwrap();
        assert_eq!(task["title"], "한글 작업");
        assert_eq!(task["description"], "설명");
        assert_eq!(task["createdAt"], "2026-01-01T00:00:00Z");
        assert_eq!(task["priority"], Value::Null);
        task["status"] = json!("review");
        let saved = db.save_task(&task).unwrap();
        assert_eq!(saved["status"], "review");
        assert_eq!(saved["createdAt"], task["createdAt"]);
        assert_eq!(saved["description"], "설명");
        assert_eq!(db.projects().unwrap().len(), 1);
    }
    #[test]
    fn settings_upsert_retains_one_row_and_db_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.db");
        {
            let db = Database::open(&path).unwrap();
            db.set_setting("sidebar_default_collapsed", "true").unwrap();
            db.set_setting("sidebar_default_collapsed", "false")
                .unwrap();
            assert_eq!(
                db.lock()
                    .unwrap()
                    .query_row::<i64, _, _>("SELECT count(*) FROM app_settings", [], |r| r.get(0))
                    .unwrap(),
                1
            );
        }
        assert_eq!(
            Database::open(&path)
                .unwrap()
                .setting("sidebar_default_collapsed")
                .unwrap()
                .as_deref(),
            Some("false")
        );
    }
    #[test]
    fn project_deletion_nulls_tasks_and_cascades_layouts() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("app.db")).unwrap();
        let p = db
            .save_project(&json!({"name":"p","repoPath":"/repo","defaultBranch":"main"}))
            .unwrap();
        let t = db
            .save_task(&json!({"title":"t","status":"todo","projectId":p["id"]}))
            .unwrap();
        db.lock()
            .unwrap()
            .execute("DELETE FROM projects WHERE id=?1", [text(&p, "id")])
            .unwrap();
        assert!(db.task(text(&t, "id")).unwrap().unwrap()["projectId"].is_null());
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;
    #[test]
    fn board_reads_only_first_done_page_and_preserves_total() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(&dir.path().join("app.db")).unwrap();
        for n in 0..25 {
            db.save_task(
                &json!({"id":format!("done-{n}"),"title":format!("done {n}"),"status":"done"}),
            )
            .unwrap();
        }
        db.save_task(&json!({"title":"active","status":"progress"}))
            .unwrap();
        let (tasks, total) = db.board_tasks().unwrap();
        assert_eq!(total, 25);
        assert_eq!(tasks.len(), 21);
        assert_eq!(
            tasks
                .iter()
                .filter(|t| text(t, "status") == "progress")
                .count(),
            1
        );
        let (page, total) = db.done_tasks(20, 20).unwrap();
        assert_eq!(page.len(), 5);
        assert_eq!(total, 25);
    }
}

#[cfg(test)]
mod electron_fixture_tests {
    use super::*;
    #[test]
    fn real_electron_fixture_preserves_all_rows_and_nested_task_contract() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../tests/fixtures/electron-v1.4.sql"))
            .unwrap();
        let task_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM kanban_tasks", [], |r| r.get(0))
            .unwrap();
        let settings: Vec<(String, String)> = {
            let mut s = connection
                .prepare("SELECT key,value FROM app_settings ORDER BY key")
                .unwrap();
            let values = s
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            values
        };
        drop(connection);
        let db = Database::open(&path).unwrap();
        assert_eq!(db.tasks().unwrap().len() as i64, task_count);
        for (key, value) in settings {
            assert_eq!(db.setting(&key).unwrap().as_deref(), Some(value.as_str()));
        }
        let tasks = db.tasks().unwrap();
        let old = tasks
            .iter()
            .find(|t| text(t, "title") == "Tauri UI parity")
            .unwrap();
        let project = db.project(text(old, "projectId")).unwrap().unwrap();
        assert_eq!(project["name"], "Tauri migration QA");
        assert_eq!(old["sessionType"], Value::Null);
        let terminal_task=db.save_task(&json!({"title":"터미널 CRUD", "projectId":project["id"],"branchName":"feature","worktreePath":"/fixture/worktree","baseBranch":"main","sessionType":"terminal","sessionName":"fixture-feature","description":"설명","priority":"medium","status":"progress"})).unwrap();
        assert_eq!(terminal_task["sessionType"], "terminal");
        assert_eq!(terminal_task["projectId"], project["id"]);
        assert_eq!(terminal_task["priority"], "medium");
        assert_eq!(terminal_task["description"], "설명");
        let mut changed = old.clone();
        changed["title"] = json!("Updated title");
        changed["description"] = json!("");
        changed["priority"] = json!("high");
        let saved = db.save_task(&changed).unwrap();
        assert_eq!(saved["description"], "");
        assert_eq!(saved["priority"], "high");
        assert_eq!(saved["createdAt"], old["createdAt"]);
        assert_eq!(saved["worktreePath"], old["worktreePath"]);
        assert_eq!(saved["projectId"], project["id"]);
        db.lock()
            .unwrap()
            .execute("DELETE FROM kanban_tasks WHERE id=?1", [text(&saved, "id")])
            .unwrap();
        assert!(db.task(text(&saved, "id")).unwrap().is_none());
        assert!(db.project(text(&project, "id")).unwrap().is_some());
    }
    #[test]
    fn initial_schema_global_name_unique_is_removed_without_unlinking_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY NOT NULL,name TEXT NOT NULL UNIQUE,repo_path TEXT NOT NULL,default_branch TEXT NOT NULL DEFAULT 'main',ssh_host TEXT,created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,legacy_extra TEXT DEFAULT 'keep'); INSERT INTO projects(id,name,repo_path)VALUES('p','same','/local');").unwrap();
        drop(c);
        let db = Database::open(&path).unwrap();
        let task = db
            .save_task(&json!({"title":"main","projectId":"p","status":"todo"}))
            .unwrap();
        db.save_project(&json!({"name":"same","repoPath":"/remote","sshHost":"server"}))
            .unwrap();
        assert_eq!(db.projects().unwrap().len(), 2);
        assert_eq!(
            db.task(text(&task, "id")).unwrap().unwrap()["projectId"],
            "p"
        );
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row::<String, _, _>(
                    "SELECT legacy_extra FROM projects WHERE id='p'",
                    [],
                    |r| r.get(0)
                )
                .unwrap(),
            "keep"
        );
    }
}
