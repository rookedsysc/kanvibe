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

    CREATE TABLE IF NOT EXISTS app_settings (
      id TEXT PRIMARY KEY NOT NULL,
      key TEXT NOT NULL UNIQUE,
      value TEXT NOT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
      updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    );

  `);
}

function ensureColumns(database: Database.Database): void {
  ensureColumn(database, "projects", "is_worktree", "is_worktree INTEGER NOT NULL DEFAULT 0");
  ensureColumn(database, "projects", "color", "color TEXT DEFAULT NULL");
  ensureColumn(database, "projects", "icon_data_url", "icon_data_url TEXT DEFAULT NULL");

  ensureColumn(database, "kanban_tasks", "project_id", "project_id TEXT");
  ensureColumn(database, "kanban_tasks", "base_branch", "base_branch TEXT");
  ensureColumn(database, "kanban_tasks", "pr_url", "pr_url TEXT");
  ensureColumn(database, "kanban_tasks", "priority", "priority TEXT DEFAULT NULL");

  ensureColumn(database, "pane_layout_configs", "panes", "panes TEXT NOT NULL DEFAULT '[]'");
}

/** PRAGMA table_info 결과 중 컬럼 정의를 다시 쓰는 데 필요한 필드 */
interface SqliteColumnInfo {
  name: string;
  type: string;
  notnull: number;
  dflt_value: string | null;
  pk: number;
}

/** name 컬럼 하나만 덮는 UNIQUE 인덱스를 찾는다. 인라인 UNIQUE 제약은 이름 없는 autoindex로 잡히므로 구성 컬럼으로 판별한다 */
function hasGlobalUniqueProjectName(database: Database.Database): boolean {
  const indexes = database.prepare(`PRAGMA index_list("projects")`).all() as Array<{ name: string; unique: number }>;

  return indexes.some((index) => {
    if (!index.unique) {
      return false;
    }

    const columns = database
      .prepare(`PRAGMA index_info(${quoteSqliteIdentifier(index.name)})`)
      .all() as Array<{ name: string }>;
    return columns.length === 1 && columns[0]?.name === "name";
  });
}

/** 기존 컬럼 정의를 그대로 옮긴다. 정의를 새로 적으면 이 파일이 모르는 레거시 컬럼이 재생성 과정에서 사라진다 */
function formatColumnDefinition(column: SqliteColumnInfo): string {
  const definition = [quoteSqliteIdentifier(column.name), column.type || "TEXT"];

  if (column.pk) {
    definition.push("PRIMARY KEY");
  }
  if (column.notnull) {
    definition.push("NOT NULL");
  }
  if (column.dflt_value !== null) {
    definition.push(`DEFAULT ${column.dflt_value}`);
  }

  return definition.join(" ");
}

/**
 * 표시 이름 유일성은 PC(ssh_host) 단위로만 따지므로 레거시 DB에 남아 있는 projects.name 전역 UNIQUE를 떨어뜨린다.
 * SQLite는 인라인 UNIQUE 제약을 DROP INDEX로 제거할 수 없어 테이블을 다시 만들어야 하고,
 * DROP TABLE이 kanban_tasks의 project_id 연결을 끊지 않도록 foreign_keys를 끈 상태에서 수행한다.
 * baseline 처리되는 오래된 DB는 마이그레이션을 건너뛰므로 이 복구가 없으면 제약만 남아 등록이 실패한다.
 */
function dropGlobalUniqueProjectName(database: Database.Database): void {
  if (!hasGlobalUniqueProjectName(database)) {
    return;
  }

  const columns = database.prepare(`PRAGMA table_info("projects")`).all() as SqliteColumnInfo[];
  const columnNames = columns.map((column) => quoteSqliteIdentifier(column.name)).join(", ");

  database.pragma("foreign_keys = OFF");
  try {
    const transaction = database.transaction(() => {
      database.exec(`
        CREATE TABLE projects_rebuilt (
          ${columns.map(formatColumnDefinition).join(",\n          ")}
        );

        INSERT INTO projects_rebuilt (${columnNames}) SELECT ${columnNames} FROM projects;

        DROP TABLE projects;

        ALTER TABLE projects_rebuilt RENAME TO projects;
      `);
    });

    transaction();
  } finally {
    database.pragma("foreign_keys = ON");
  }
}

/**
 * 컬럼 구성이 달라진 인덱스를 지운다.
 *
 * `CREATE INDEX IF NOT EXISTS`는 같은 이름이 이미 있으면 정의가 달라도 아무것도 하지 않는다.
 * 그래서 인덱스가 가리키는 컬럼을 바꿀 때는 먼저 지워야 한다.
 * migrations 테이블이 없어 baseline 처리되는 DB는 TypeORM 마이그레이션이 실행되지 않으므로
 * 여기서 지우지 않으면 인덱스가 영영 옛 컬럼에 남아 정렬이 매번 임시 B-tree로 떨어진다.
 */
function dropIndexWithDifferentColumns(
  database: Database.Database,
  indexName: string,
  expectedColumns: string[],
): void {
  const rows = database
    .prepare(`PRAGMA index_info(${quoteSqliteIdentifier(indexName)})`)
    .all() as Array<{ name: string | null }>;

  if (rows.length === 0) return;

  const actualColumns = rows.map((row) => row.name);
  const matchesExpected = actualColumns.length === expectedColumns.length
    && actualColumns.every((columnName, index) => columnName === expectedColumns[index]);

  if (matchesExpected) return;

  database.exec(`DROP INDEX IF EXISTS ${quoteSqliteIdentifier(indexName)}`);
}

function ensureIndexes(database: Database.Database): void {
  dropIndexWithDifferentColumns(database, "idx_kanban_tasks_status_order", [
    "status",
    "updated_at",
  ]);
  dropIndexWithDifferentColumns(database, "idx_kanban_tasks_project_branch", [
    "project_id",
    "branch_name",
  ]);

  database.exec(`
    CREATE INDEX IF NOT EXISTS idx_kanban_tasks_status_order
      ON kanban_tasks(status, updated_at);

    CREATE INDEX IF NOT EXISTS idx_kanban_tasks_project_branch
      ON kanban_tasks(project_id, branch_name);
  `);
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
      ensureIndexes(database);
    });

    transaction();
    dropGlobalUniqueProjectName(database);
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
