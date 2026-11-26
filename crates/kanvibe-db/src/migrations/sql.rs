//! TS 마이그레이션이 실행하던 SQL 원문. sqlite_master에는 실행한 문장의 공백까지 그대로 남으므로
//! 한 글자도 바꾸지 않고 옮겨야 TS 앱이 만든 DB와 같은 스키마 텍스트가 나온다.

pub(super) const CREATE_LEGACY_PROJECTS: &str = r#"
      CREATE TABLE IF NOT EXISTS "projects" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "name" TEXT NOT NULL UNIQUE,
        "repo_path" TEXT NOT NULL,
        "default_branch" TEXT NOT NULL DEFAULT 'main',
        "ssh_host" TEXT,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      )
    "#;

pub(super) const CREATE_LEGACY_KANBAN_TASKS: &str = r#"
      CREATE TABLE IF NOT EXISTS "kanban_tasks" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "title" TEXT NOT NULL,
        "description" TEXT,
        "status" TEXT NOT NULL DEFAULT 'todo',
        "branch_name" TEXT,
        "worktree_path" TEXT,
        "session_type" TEXT,
        "session_name" TEXT,
        "ssh_host" TEXT,
        "agent_type" TEXT,
        "project_id" TEXT,
        "base_branch" TEXT,
        "display_order" INTEGER NOT NULL DEFAULT 0,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE SET NULL
      )
    "#;

pub(super) const CREATE_BRANCH_NAME_UNIQUE_INDEX: &str = r#"
      CREATE UNIQUE INDEX IF NOT EXISTS "UQ_kanban_tasks_branch_name"
        ON kanban_tasks(branch_name)
    "#;

pub(super) const CREATE_LEGACY_STATUS_ORDER_INDEX: &str = r#"
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, display_order, created_at)
    "#;

pub(super) const CREATE_PROJECT_BRANCH_INDEX: &str = r#"
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_project_branch"
        ON kanban_tasks(project_id, branch_name)
    "#;

pub(super) const CREATE_PANE_LAYOUT_CONFIGS: &str = r#"
      CREATE TABLE IF NOT EXISTS "pane_layout_configs" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "layout_type" TEXT NOT NULL,
        "panes" TEXT NOT NULL,
        "project_id" TEXT UNIQUE,
        "is_global" INTEGER NOT NULL DEFAULT 0,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE CASCADE
      )
    "#;

pub(super) const ASSIGN_DISPLAY_ORDER: &str = r#"
      UPDATE kanban_tasks
      SET display_order = (
        SELECT COUNT(*)
        FROM kanban_tasks t2
        WHERE t2.status = kanban_tasks.status
          AND (
            t2.created_at < kanban_tasks.created_at
            OR (t2.created_at = kanban_tasks.created_at AND t2.id < kanban_tasks.id)
          )
      )
      WHERE display_order = 0
    "#;

pub(super) const CREATE_APP_SETTINGS: &str = r#"
      CREATE TABLE IF NOT EXISTS "app_settings" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "key" TEXT NOT NULL UNIQUE,
        "value" TEXT NOT NULL,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      )
    "#;

pub(super) const FILL_EMPTY_BASE_BRANCH: &str = r#"
      UPDATE kanban_tasks
      SET base_branch = (
        SELECT p.default_branch
        FROM projects p
        WHERE p.id = kanban_tasks.project_id
      )
      WHERE project_id IS NOT NULL
        AND (base_branch IS NULL OR base_branch = '')
    "#;

pub(super) const CREATE_PROJECTS_REBUILT: &str = r#"
    CREATE TABLE "projects_rebuilt" (
      id TEXT PRIMARY KEY NOT NULL,
      name TEXT NOT NULL,
      repo_path TEXT NOT NULL,
      default_branch TEXT NOT NULL DEFAULT 'main',
      ssh_host TEXT,
      is_worktree INTEGER NOT NULL DEFAULT 0,
      color TEXT DEFAULT NULL,
      icon_data_url TEXT DEFAULT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    )
  "#;

pub(super) const CREATE_STATUS_UPDATED_INDEX: &str = r#"
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, updated_at)
    "#;

pub(super) const CREATE_TASK_DIFF_STATS: &str = r#"
      CREATE TABLE IF NOT EXISTS "task_diff_stats" (
        "task_id" TEXT PRIMARY KEY NOT NULL,
        "file_count" INTEGER NOT NULL DEFAULT 0,
        "additions" INTEGER NOT NULL DEFAULT 0,
        "deletions" INTEGER NOT NULL DEFAULT 0,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("task_id") REFERENCES "kanban_tasks"("id") ON DELETE CASCADE
      )
    "#;
/// TypeORM이 migrations 표가 없을 때 만드는 문장
pub(super) const CREATE_TYPEORM_MIGRATIONS_TABLE: &str = r#"CREATE TABLE "migrations" ("id" integer PRIMARY KEY AUTOINCREMENT NOT NULL, "timestamp" bigint NOT NULL, "name" varchar NOT NULL)"#;

/// 마이그레이션 기록이 없는 옛 DB를 baseline 처리할 때 TS `database.ts`가 쓰던 문장
pub(super) const CREATE_BASELINE_MIGRATIONS_TABLE: &str = r#"
      CREATE TABLE IF NOT EXISTS "migrations" (
        "id" integer PRIMARY KEY AUTOINCREMENT NOT NULL,
        "timestamp" bigint NOT NULL,
        "name" varchar NOT NULL
      )
    "#;
