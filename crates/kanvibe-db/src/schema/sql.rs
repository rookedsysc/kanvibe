//! TS `sqliteSchema.ts`가 실행하던 SQL 원문. 공백까지 그대로 옮겨 sqlite_master 텍스트를 맞춘다.

pub(super) const ENSURE_BASE_TABLES: &str = r#"
    CREATE TABLE IF NOT EXISTS projects (
      id TEXT PRIMARY KEY NOT NULL,
      name TEXT NOT NULL,
      repo_path TEXT NOT NULL,
      default_branch TEXT NOT NULL DEFAULT 'main',
      ssh_host TEXT,
      is_worktree INTEGER NOT NULL DEFAULT 0,
      color TEXT DEFAULT NULL,
      icon_data_url TEXT DEFAULT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE TABLE IF NOT EXISTS kanban_tasks (
      id TEXT PRIMARY KEY NOT NULL,
      title TEXT NOT NULL,
      description TEXT,
      status TEXT NOT NULL DEFAULT 'todo',
      branch_name TEXT,
      worktree_path TEXT,
      session_type TEXT,
      session_name TEXT,
      ssh_host TEXT,
      agent_type TEXT,
      project_id TEXT,
      base_branch TEXT,
      pr_url TEXT,
      priority TEXT DEFAULT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
    );

    CREATE TABLE IF NOT EXISTS pane_layout_configs (
      id TEXT PRIMARY KEY NOT NULL,
      layout_type TEXT NOT NULL,
      panes TEXT NOT NULL,
      project_id TEXT UNIQUE,
      is_global INTEGER NOT NULL DEFAULT 0,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS task_diff_stats (
      task_id TEXT PRIMARY KEY NOT NULL,
      file_count INTEGER NOT NULL DEFAULT 0,
      additions INTEGER NOT NULL DEFAULT 0,
      deletions INTEGER NOT NULL DEFAULT 0,
      updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      FOREIGN KEY (task_id) REFERENCES kanban_tasks(id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS app_settings (
      id TEXT PRIMARY KEY NOT NULL,
      key TEXT NOT NULL UNIQUE,
      value TEXT NOT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

  "#;

pub(super) const ENSURE_INDEXES: &str = r#"
    CREATE INDEX IF NOT EXISTS idx_kanban_tasks_status_order
      ON kanban_tasks(status, updated_at);

    CREATE INDEX IF NOT EXISTS idx_kanban_tasks_project_branch
      ON kanban_tasks(project_id, branch_name);
  "#;
