import fs from "node:fs";
import path from "node:path";
import Database from "better-sqlite3";

function quoteSqliteIdentifier(identifier: string): string {
  return `"${identifier.replaceAll("\"", "\"\"")}"`;
}

function getColumnNames(database: Database.Database, tableName: string): Set<string> {
  const rows = database.prepare(`PRAGMA table_info(${quoteSqliteIdentifier(tableName)})`).all() as Array<{ name: string }>;
  return new Set(rows.map((row) => row.name));
}

function ensureColumn(database: Database.Database, tableName: string, columnName: string, definition: string): void {
  const columns = getColumnNames(database, tableName);
  if (columns.has(columnName)) {
    return;
  }

  database.exec(`ALTER TABLE ${quoteSqliteIdentifier(tableName)} ADD COLUMN ${definition}`);
}

function ensureBaseTables(database: Database.Database): void {
  database.exec(`
    CREATE TABLE IF NOT EXISTS projects (
      id TEXT PRIMARY KEY NOT NULL,
      name TEXT NOT NULL UNIQUE,
      repo_path TEXT NOT NULL,
      default_branch TEXT NOT NULL DEFAULT 'main',
      ssh_host TEXT,
      is_worktree INTEGER NOT NULL DEFAULT 0,
      color TEXT DEFAULT NULL,
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
      display_order INTEGER NOT NULL DEFAULT 0,
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

    CREATE TABLE IF NOT EXISTS app_settings (
      id TEXT PRIMARY KEY NOT NULL,
      key TEXT NOT NULL UNIQUE,
      value TEXT NOT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

    CREATE INDEX IF NOT EXISTS idx_kanban_tasks_status_order
      ON kanban_tasks(status, display_order, created_at);

    CREATE INDEX IF NOT EXISTS idx_kanban_tasks_project_branch
      ON kanban_tasks(project_id, branch_name);
  `);
}

function ensureColumns(database: Database.Database): void {
  ensureColumn(database, "projects", "is_worktree", "is_worktree INTEGER NOT NULL DEFAULT 0");
  ensureColumn(database, "projects", "color", "color TEXT DEFAULT NULL");

  ensureColumn(database, "kanban_tasks", "project_id", "project_id TEXT");
  ensureColumn(database, "kanban_tasks", "base_branch", "base_branch TEXT");
  ensureColumn(database, "kanban_tasks", "pr_url", "pr_url TEXT");
  ensureColumn(database, "kanban_tasks", "priority", "priority TEXT DEFAULT NULL");
  ensureColumn(database, "kanban_tasks", "display_order", "display_order INTEGER NOT NULL DEFAULT 0");

  ensureColumn(database, "pane_layout_configs", "panes", "panes TEXT NOT NULL DEFAULT '[]'");
}

export function ensureSqliteDatabaseReady(databasePath: string): void {
  fs.mkdirSync(path.dirname(databasePath), { recursive: true });

  const database = new Database(databasePath);

  try {
    database.pragma("journal_mode = WAL");
    database.pragma("foreign_keys = ON");

    const transaction = database.transaction(() => {
      ensureBaseTables(database);
      ensureColumns(database);
    });

    transaction();
  } finally {
    database.close();
  }
}

export function buildSeedDatabase(outputPath: string): void {
  fs.mkdirSync(path.dirname(outputPath), { recursive: true });

  if (fs.existsSync(outputPath)) {
    fs.rmSync(outputPath, { force: true });
  }

  ensureSqliteDatabaseReady(outputPath);
}
