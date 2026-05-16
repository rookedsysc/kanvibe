import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import Database from "better-sqlite3";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ensureSqliteDatabaseReady } from "@/lib/sqliteSchema";

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
});
