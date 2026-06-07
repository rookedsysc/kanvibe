import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import Database from "better-sqlite3";
import { DataSource } from "typeorm";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ensureSqliteDatabaseReady } from "@/lib/sqliteSchema";
import { AssignDisplayOrder1771166346785 } from "@/migrations/1771166346785-AssignDisplayOrder";

const originalKanvibeDbPath = process.env.KANVIBE_DB_PATH;

function createDatabasePath(): string {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "kanvibe-db-test-"));
  return path.join(directory, "kanvibe.db");
}

function insertLegacyBootstrapData(databasePath: string): void {
  ensureSqliteDatabaseReady(databasePath);

  const database = new Database(databasePath);
  try {
    database.exec(`
      INSERT INTO projects (id, name, repo_path, default_branch)
      VALUES
        ('project-1', 'Project 1', '/workspace/project-1', 'main'),
        ('project-2', 'Project 2', '/workspace/project-2', 'main');

      INSERT INTO kanban_tasks (
        id,
        title,
        status,
        branch_name,
        project_id,
        display_order,
        created_at,
        updated_at
      )
      VALUES
        ('task-1', 'Top card', 'todo', 'main', 'project-1', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
        ('task-2', 'Older card', 'todo', 'main', 'project-2', 1, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
    `);
  } finally {
    database.close();
  }
}

function insertOlderLegacyBootstrapData(databasePath: string): void {
  fs.mkdirSync(path.dirname(databasePath), { recursive: true });

  const database = new Database(databasePath);
  try {
    database.exec(`
      CREATE TABLE projects (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL UNIQUE,
        repo_path TEXT NOT NULL,
        default_branch TEXT NOT NULL DEFAULT 'main',
        ssh_host TEXT,
        created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      );

      CREATE TABLE kanban_tasks (
        id TEXT PRIMARY KEY NOT NULL,
        title TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'todo',
        branch_name TEXT,
        created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      );

      CREATE TABLE pane_layout_configs (
        id TEXT PRIMARY KEY NOT NULL,
        layout_type TEXT NOT NULL,
        project_id TEXT UNIQUE,
        is_global INTEGER NOT NULL DEFAULT 0,
        created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      );

      INSERT INTO projects (id, name, repo_path, default_branch)
      VALUES ('project-1', 'Project 1', '/workspace/project-1', 'main');

      INSERT INTO kanban_tasks (id, title, status, branch_name)
      VALUES ('task-1', 'Task 1', 'todo', 'main');
    `);
  } finally {
    database.close();
  }
}

afterEach(() => {
  if (originalKanvibeDbPath === undefined) {
    delete process.env.KANVIBE_DB_PATH;
  } else {
    process.env.KANVIBE_DB_PATH = originalKanvibeDbPath;
  }
  vi.resetModules();
});

describe("database migrations", () => {
  it("baselines databases created by the legacy SQLite bootstrap", async () => {
    const databasePath = createDatabasePath();
    insertLegacyBootstrapData(databasePath);
    process.env.KANVIBE_DB_PATH = databasePath;
    vi.resetModules();

    const { getDataSource } = await import("@/lib/database");
    const dataSource = await getDataSource();

    try {
      const tasks = await dataSource.query(`
        SELECT id, branch_name, display_order
        FROM kanban_tasks
        ORDER BY id
      `);
      const migrations = await dataSource.query(`SELECT name FROM migrations ORDER BY timestamp`);

      expect(tasks).toEqual([
        { id: "task-1", branch_name: "main", display_order: 0 },
        { id: "task-2", branch_name: "main", display_order: 1 },
      ]);
      expect(migrations).toHaveLength(12);
      expect(migrations[0]).toEqual({ name: "InitialSchema1770854400000" });
    } finally {
      await dataSource.destroy();
    }
  });

  it("backfills older legacy SQLite columns before baselining migrations", async () => {
    const databasePath = createDatabasePath();
    insertOlderLegacyBootstrapData(databasePath);
    process.env.KANVIBE_DB_PATH = databasePath;
    vi.resetModules();

    const { getDataSource } = await import("@/lib/database");
    const dataSource = await getDataSource();

    try {
      const taskColumns = await dataSource.query(`PRAGMA table_info("kanban_tasks")`);
      const paneColumns = await dataSource.query(`PRAGMA table_info("pane_layout_configs")`);
      const tasks = await dataSource.query(`
        SELECT id, project_id, base_branch, pr_url, priority, display_order
        FROM kanban_tasks
      `);
      const migrations = await dataSource.query(`SELECT name FROM migrations ORDER BY timestamp`);

      expect(taskColumns.map((row: { name: string }) => row.name)).toEqual(
        expect.arrayContaining(["project_id", "base_branch", "pr_url", "priority", "display_order"]),
      );
      expect(paneColumns.map((row: { name: string }) => row.name)).toContain("panes");
      expect(tasks).toEqual([
        {
          id: "task-1",
          project_id: null,
          base_branch: null,
          pr_url: null,
          priority: null,
          display_order: 0,
        },
      ]);
      expect(migrations).toHaveLength(12);
    } finally {
      await dataSource.destroy();
    }
  });

  it("runs TypeORM migrations for fresh database files", async () => {
    const databasePath = createDatabasePath();
    process.env.KANVIBE_DB_PATH = databasePath;
    vi.resetModules();

    const { getDataSource } = await import("@/lib/database");
    const dataSource = await getDataSource();

    try {
      const tables = await dataSource.query(`
        SELECT name
        FROM sqlite_master
        WHERE type = 'table'
        ORDER BY name
      `);
      const indexes = await dataSource.query(`PRAGMA index_list("kanban_tasks")`);
      const migrations = await dataSource.query(`SELECT name FROM migrations ORDER BY timestamp`);

      expect(tables.map((row: { name: string }) => row.name)).toEqual(
        expect.arrayContaining(["app_settings", "kanban_tasks", "migrations", "pane_layout_configs", "projects"]),
      );
      expect(indexes.map((row: { name: string }) => row.name)).not.toContain(
        "UQ_kanban_tasks_branch_name",
      );
      expect(migrations).toHaveLength(12);
    } finally {
      await dataSource.destroy();
    }
  });

  it("assigns distinct display orders when task timestamps are tied", async () => {
    const databasePath = createDatabasePath();
    const dataSource = new DataSource({
      type: "better-sqlite3",
      database: databasePath,
    });

    await dataSource.initialize();

    try {
      await dataSource.query(`
        CREATE TABLE kanban_tasks (
          id TEXT PRIMARY KEY NOT NULL,
          status TEXT NOT NULL,
          display_order INTEGER NOT NULL DEFAULT 0,
          created_at DATETIME NOT NULL
        )
      `);
      await dataSource.query(`
        INSERT INTO kanban_tasks (id, status, display_order, created_at)
        VALUES
          ('task-a', 'todo', 0, '2026-01-01T00:00:00.000Z'),
          ('task-b', 'todo', 0, '2026-01-01T00:00:00.000Z'),
          ('task-c', 'todo', 0, '2026-01-01T00:00:00.000Z')
      `);

      const queryRunner = dataSource.createQueryRunner();
      try {
        await new AssignDisplayOrder1771166346785().up(queryRunner);
      } finally {
        await queryRunner.release();
      }

      const tasks = await dataSource.query(`
        SELECT id, display_order
        FROM kanban_tasks
        ORDER BY id
      `);

      expect(tasks).toEqual([
        { id: "task-a", display_order: 0 },
        { id: "task-b", display_order: 1 },
        { id: "task-c", display_order: 2 },
      ]);
    } finally {
      await dataSource.destroy();
    }
  });
});
