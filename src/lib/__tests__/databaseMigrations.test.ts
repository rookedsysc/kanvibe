import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import Database from "better-sqlite3";
import { DataSource } from "typeorm";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ensureSqliteDatabaseReady } from "@/lib/sqliteSchema";
import { AssignDisplayOrder1771166346785 } from "@/migrations/1771166346785-AssignDisplayOrder";
import { RescopeProjectNamesPerHost1771600000000 } from "@/migrations/1771600000000-RescopeProjectNamesPerHost";
import { ReplaceDisplayOrderWithDisplayRank1771700000000 } from "@/migrations/1771700000000-ReplaceDisplayOrderWithDisplayRank";

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
        display_rank,
        created_at,
        updated_at
      )
      VALUES
        ('task-1', 'Top card', 'todo', 'main', 'project-1', '2', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
        ('task-2', 'Older card', 'todo', 'main', 'project-2', '4', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z');
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

/** migrations 테이블이 없어 baseline 처리되는, 전역 UNIQUE가 살아 있는 오래된 DB */
function insertBaselinedLegacyProjectsData(databasePath: string): void {
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
        project_id TEXT,
        created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
      );

      INSERT INTO projects (id, name, repo_path, default_branch)
      VALUES ('project-local', 'kanvibe', '/workspace/kanvibe', 'main');

      INSERT INTO kanban_tasks (id, title, status, branch_name, project_id)
      VALUES ('task-1', 'main', 'todo', 'main', 'project-local');
    `);
  } finally {
    database.close();
  }
}

/** migrations 테이블이 없어 baseline 처리되는, 정수 순번만 들고 있던 오래된 DB */
function insertLegacyDisplayOrderData(databasePath: string): void {
  fs.mkdirSync(path.dirname(databasePath), { recursive: true });

  const database = new Database(databasePath);
  try {
    database.exec(`
      CREATE TABLE projects (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
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
        display_order INTEGER NOT NULL DEFAULT 0,
        created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      );

      INSERT INTO kanban_tasks (id, title, status, display_order, created_at, updated_at)
      VALUES
        ('task-last', 'Last', 'todo', 2, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
        ('task-first', 'First', 'todo', 0, '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
        ('task-middle', 'Middle', 'todo', 1, '2026-01-03T00:00:00.000Z', '2026-01-03T00:00:00.000Z');
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
        SELECT id, branch_name, display_rank
        FROM kanban_tasks
        ORDER BY id
      `);
      const migrations = await dataSource.query(`SELECT name FROM migrations ORDER BY timestamp`);

      expect(tasks).toEqual([
        { id: "task-1", branch_name: "main", display_rank: "2" },
        { id: "task-2", branch_name: "main", display_rank: "4" },
      ]);
      expect(migrations).toHaveLength(15);
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
        SELECT id, project_id, base_branch, pr_url, priority, display_rank, is_manually_ordered
        FROM kanban_tasks
      `);
      const migrations = await dataSource.query(`SELECT name FROM migrations ORDER BY timestamp`);

      expect(taskColumns.map((row: { name: string }) => row.name)).toEqual(
        expect.arrayContaining(["project_id", "base_branch", "pr_url", "priority", "display_rank"]),
      );
      expect(taskColumns.map((row: { name: string }) => row.name)).not.toContain("display_order");
      expect(paneColumns.map((row: { name: string }) => row.name)).toContain("panes");
      expect(tasks).toEqual([
        {
          id: "task-1",
          project_id: null,
          base_branch: null,
          pr_url: null,
          priority: null,
          display_rank: "1",
          is_manually_ordered: 0,
        },
      ]);
      expect(migrations).toHaveLength(15);
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
      expect(migrations).toHaveLength(15);

      await dataSource.query(`
        INSERT INTO projects (id, name, repo_path, ssh_host)
        VALUES
          ('project-local', 'kanvibe', '/workspace/kanvibe', NULL),
          ('project-remote', 'kanvibe', '/workspace/kanvibe', 'remote-host')
      `);
      const projectNames = await dataSource.query(`SELECT name FROM projects ORDER BY id`);

      expect(projectNames).toEqual([{ name: "kanvibe" }, { name: "kanvibe" }]);
    } finally {
      await dataSource.destroy();
    }
  });

  it("drops the legacy global unique project name without unlinking tasks", async () => {
    const databasePath = createDatabasePath();
    const dataSource = new DataSource({
      type: "better-sqlite3",
      database: databasePath,
      prepareDatabase: (database) => {
        database.pragma("foreign_keys = ON");
      },
    });

    await dataSource.initialize();

    try {
      await dataSource.query(`
        CREATE TABLE projects (
          id TEXT PRIMARY KEY NOT NULL,
          name TEXT NOT NULL UNIQUE,
          repo_path TEXT NOT NULL,
          default_branch TEXT NOT NULL DEFAULT 'main',
          ssh_host TEXT,
          is_worktree INTEGER NOT NULL DEFAULT 0,
          color TEXT DEFAULT NULL,
          icon_data_url TEXT DEFAULT NULL,
          created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
      `);
      await dataSource.query(`
        CREATE TABLE kanban_tasks (
          id TEXT PRIMARY KEY NOT NULL,
          title TEXT NOT NULL,
          status TEXT NOT NULL DEFAULT 'todo',
          project_id TEXT,
          created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
          updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
          FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL
        )
      `);
      await dataSource.query(`
        INSERT INTO projects (id, name, repo_path, ssh_host)
        VALUES ('project-local', 'kanvibe', '/workspace/kanvibe', NULL)
      `);
      await dataSource.query(`
        INSERT INTO kanban_tasks (id, title, project_id)
        VALUES ('task-1', 'main', 'project-local')
      `);

      /** 프로덕션과 같은 순서로 foreign_keys를 끄고 마이그레이션을 실행해 DROP TABLE이 task 연결을 지우지 않는지 확인한다 */
      const queryRunner = dataSource.createQueryRunner();
      try {
        await queryRunner.beforeMigration();
        await new RescopeProjectNamesPerHost1771600000000().up(queryRunner);
        await queryRunner.afterMigration();
      } finally {
        await queryRunner.release();
      }

      await dataSource.query(`
        INSERT INTO projects (id, name, repo_path, ssh_host)
        VALUES ('project-remote', 'kanvibe', '/workspace/kanvibe', 'remote-host')
      `);

      const projects = await dataSource.query(`SELECT id, name, ssh_host FROM projects ORDER BY id`);
      const tasks = await dataSource.query(`SELECT id, project_id FROM kanban_tasks`);

      expect(projects).toEqual([
        { id: "project-local", name: "kanvibe", ssh_host: null },
        { id: "project-remote", name: "kanvibe", ssh_host: "remote-host" },
      ]);
      expect(tasks).toEqual([{ id: "task-1", project_id: "project-local" }]);
    } finally {
      await dataSource.destroy();
    }
  });

  it("rescopes names that the global unique rule had padded with a parent folder or a number", async () => {
    const databasePath = createDatabasePath();
    const dataSource = new DataSource({
      type: "better-sqlite3",
      database: databasePath,
    });

    await dataSource.initialize();

    try {
      await dataSource.query(`
        CREATE TABLE projects (
          id TEXT PRIMARY KEY NOT NULL,
          name TEXT NOT NULL UNIQUE,
          repo_path TEXT NOT NULL,
          default_branch TEXT NOT NULL DEFAULT 'main',
          ssh_host TEXT,
          is_worktree INTEGER NOT NULL DEFAULT 0,
          color TEXT DEFAULT NULL,
          icon_data_url TEXT DEFAULT NULL,
          created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
      `);
      await dataSource.query(`
        INSERT INTO projects (id, name, repo_path, ssh_host, created_at)
        VALUES
          ('project-local', 'kanvibe', '/home/tester/Documents/kanvibe', NULL, '2026-01-01T00:00:00.000Z'),
          ('project-remote', 'Documents/kanvibe', '/home/tester/Documents/kanvibe', 'remote-host', '2026-01-02T00:00:00.000Z'),
          ('project-other-remote', 'kanvibe-2', '/home/tester/Documents/kanvibe', 'other-host', '2026-01-03T00:00:00.000Z'),
          ('project-local-second', 'work/kanvibe', '/home/tester/work/kanvibe', NULL, '2026-01-04T00:00:00.000Z')
      `);

      const queryRunner = dataSource.createQueryRunner();
      try {
        await queryRunner.beforeMigration();
        await new RescopeProjectNamesPerHost1771600000000().up(queryRunner);
        await queryRunner.afterMigration();
      } finally {
        await queryRunner.release();
      }

      const projects = await dataSource.query(`SELECT id, name FROM projects ORDER BY created_at`);

      expect(projects).toEqual([
        { id: "project-local", name: "kanvibe" },
        { id: "project-remote", name: "kanvibe" },
        { id: "project-other-remote", name: "kanvibe" },
        { id: "project-local-second", name: "work/kanvibe" },
      ]);
    } finally {
      await dataSource.destroy();
    }
  });

  it("drops the legacy global unique project name even when the database is baselined", async () => {
    const databasePath = createDatabasePath();
    insertBaselinedLegacyProjectsData(databasePath);
    process.env.KANVIBE_DB_PATH = databasePath;
    vi.resetModules();

    const { getDataSource } = await import("@/lib/database");
    const dataSource = await getDataSource();

    try {
      /** baseline은 모든 마이그레이션을 실행됨으로 기록하므로 스키마 복구가 없으면 UNIQUE가 그대로 남는다 */
      await dataSource.query(`
        INSERT INTO projects (id, name, repo_path, ssh_host)
        VALUES ('project-remote', 'kanvibe', '/workspace/kanvibe', 'remote-host')
      `);

      const projects = await dataSource.query(`SELECT id, name, ssh_host FROM projects ORDER BY id`);
      const tasks = await dataSource.query(`SELECT id, project_id FROM kanban_tasks`);

      expect(projects).toEqual([
        { id: "project-local", name: "kanvibe", ssh_host: null },
        { id: "project-remote", name: "kanvibe", ssh_host: "remote-host" },
      ]);
      expect(tasks).toEqual([{ id: "task-1", project_id: "project-local" }]);
    } finally {
      await dataSource.destroy();
    }
  });

  it("fails instead of silently dropping a projects column the rebuild cannot carry over", async () => {
    const databasePath = createDatabasePath();
    const dataSource = new DataSource({
      type: "better-sqlite3",
      database: databasePath,
    });

    await dataSource.initialize();

    try {
      await dataSource.query(`
        CREATE TABLE projects (
          id TEXT PRIMARY KEY NOT NULL,
          name TEXT NOT NULL UNIQUE,
          repo_path TEXT NOT NULL,
          default_branch TEXT NOT NULL DEFAULT 'main',
          ssh_host TEXT,
          is_worktree INTEGER NOT NULL DEFAULT 0,
          color TEXT DEFAULT NULL,
          icon_data_url TEXT DEFAULT NULL,
          created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
          pinned_at DATETIME DEFAULT NULL
        )
      `);
      await dataSource.query(`
        INSERT INTO projects (id, name, repo_path, pinned_at)
        VALUES ('project-local', 'kanvibe', '/workspace/kanvibe', '2026-01-01T00:00:00.000Z')
      `);

      const queryRunner = dataSource.createQueryRunner();
      try {
        await expect(
          new RescopeProjectNamesPerHost1771600000000().up(queryRunner),
        ).rejects.toThrow("pinned_at");
      } finally {
        await queryRunner.release();
      }

      const projects = await dataSource.query(`SELECT id, pinned_at FROM projects`);

      expect(projects).toEqual([{ id: "project-local", pinned_at: "2026-01-01T00:00:00.000Z" }]);
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
  it("정수 순번을 rank로 옮기면서 컬럼별 순서를 그대로 보존하고 display_order를 없앤다", async () => {
    // Given
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
        CREATE INDEX "idx_kanban_tasks_status_order"
          ON kanban_tasks(status, display_order, created_at)
      `);
      await dataSource.query(`
        INSERT INTO kanban_tasks (id, status, display_order, created_at)
        VALUES
          ('todo-second', 'todo', 1, '2026-01-01T00:00:00.000Z'),
          ('todo-first', 'todo', 0, '2026-01-02T00:00:00.000Z'),
          ('todo-third', 'todo', 2, '2026-01-03T00:00:00.000Z'),
          ('review-second', 'review', 1, '2026-01-01T00:00:00.000Z'),
          ('review-first', 'review', 0, '2026-01-02T00:00:00.000Z')
      `);

      // When
      const queryRunner = dataSource.createQueryRunner();
      try {
        await new ReplaceDisplayOrderWithDisplayRank1771700000000().up(queryRunner);
      } finally {
        await queryRunner.release();
      }

      // Then
      const columns = await dataSource.query(`PRAGMA table_info("kanban_tasks")`);
      const todoTasks = await dataSource.query(`
        SELECT id, is_manually_ordered FROM kanban_tasks
        WHERE status = 'todo' ORDER BY display_rank
      `);
      const reviewTasks = await dataSource.query(`
        SELECT id FROM kanban_tasks WHERE status = 'review' ORDER BY display_rank
      `);
      const indexes = await dataSource.query(`PRAGMA index_list("kanban_tasks")`);

      expect(columns.map((row: { name: string }) => row.name)).not.toContain("display_order");
      expect(todoTasks.map((row: { id: string }) => row.id)).toEqual(["todo-first", "todo-second", "todo-third"]);
      /** 상태마다 따로 번호가 매겨져 있었으므로 rank도 상태 안에서만 이어져야 한다 */
      expect(reviewTasks.map((row: { id: string }) => row.id)).toEqual(["review-first", "review-second"]);
      /** 자동으로 배정됐던 순번이므로 사용자가 직접 배치한 것으로 표시하지 않는다 */
      expect(todoTasks.every((row: { is_manually_ordered: number }) => row.is_manually_ordered === 0)).toBe(true);
      expect(indexes.map((row: { name: string }) => row.name)).toContain("idx_kanban_tasks_status_order");
    } finally {
      await dataSource.destroy();
    }
  });

  it("rank를 다시 정수 순번으로 되돌려 마이그레이션을 취소할 수 있다", async () => {
    // Given
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
          ('task-b', 'todo', 1, '2026-01-02T00:00:00.000Z')
      `);

      // When
      const queryRunner = dataSource.createQueryRunner();
      try {
        const migration = new ReplaceDisplayOrderWithDisplayRank1771700000000();
        await migration.up(queryRunner);
        await migration.down(queryRunner);
      } finally {
        await queryRunner.release();
      }

      // Then
      const columns = await dataSource.query(`PRAGMA table_info("kanban_tasks")`);
      const tasks = await dataSource.query(`SELECT id, display_order FROM kanban_tasks ORDER BY display_order`);

      expect(columns.map((row: { name: string }) => row.name)).not.toContain("display_rank");
      expect(tasks).toEqual([
        { id: "task-a", display_order: 0 },
        { id: "task-b", display_order: 1 },
      ]);
    } finally {
      await dataSource.destroy();
    }
  });
  it("baseline 처리되는 오래된 DB의 정수 순번도 rank로 옮겨 카드 순서를 지킨다", async () => {
    // Given
    const databasePath = createDatabasePath();
    insertLegacyDisplayOrderData(databasePath);
    process.env.KANVIBE_DB_PATH = databasePath;
    vi.resetModules();

    // When
    const { getDataSource } = await import("@/lib/database");
    const dataSource = await getDataSource();

    try {
      const tasks = await dataSource.query(`SELECT id FROM kanban_tasks ORDER BY display_rank`);
      const columns = await dataSource.query(`PRAGMA table_info("kanban_tasks")`);

      // Then
      /** baseline은 마이그레이션을 실행하지 않으므로 부트스트랩 쪽에서 순서를 옮겨 두어야 한다 */
      expect(tasks.map((row: { id: string }) => row.id)).toEqual(["task-first", "task-middle", "task-last"]);
      expect(columns.map((row: { name: string }) => row.name)).toContain("display_rank");
    } finally {
      await dataSource.destroy();
    }
  });
  it("정수 순번조차 없던 DB의 카드에도 서로 다른 rank를 줘서 사이에 끼워 넣을 수 있게 한다", async () => {
    // Given
    const databasePath = createDatabasePath();
    fs.mkdirSync(path.dirname(databasePath), { recursive: true });
    const legacyDatabase = new Database(databasePath);
    try {
      legacyDatabase.exec(`
        CREATE TABLE kanban_tasks (
          id TEXT PRIMARY KEY NOT NULL,
          title TEXT NOT NULL,
          status TEXT NOT NULL DEFAULT 'todo',
          branch_name TEXT,
          created_at DATETIME NOT NULL,
          updated_at DATETIME NOT NULL
        );

        INSERT INTO kanban_tasks (id, title, status, created_at, updated_at)
        VALUES
          ('task-1', 'First', 'todo', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z'),
          ('task-2', 'Second', 'todo', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z'),
          ('task-3', 'Third', 'todo', '2026-01-03T00:00:00.000Z', '2026-01-03T00:00:00.000Z');
      `);
    } finally {
      legacyDatabase.close();
    }

    // When
    ensureSqliteDatabaseReady(databasePath);

    // Then
    const database = new Database(databasePath);
    try {
      const tasks = database
        .prepare(`SELECT id, display_rank FROM kanban_tasks ORDER BY display_rank`)
        .all() as Array<{ id: string; display_rank: string }>;

      expect(tasks.map((task) => task.id)).toEqual(["task-1", "task-2", "task-3"]);
      /** 같은 값이 섞이면 두 카드 사이에 새 자리를 만들 수 없어 드래그가 엉뚱한 곳으로 간다 */
      expect(new Set(tasks.map((task) => task.display_rank)).size).toBe(3);
    } finally {
      database.close();
    }
  });
});
