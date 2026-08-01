import "reflect-metadata";
import Database from "better-sqlite3";
import { DataSource, type ObjectLiteral, type Repository } from "typeorm";
import { KanbanTask } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";
import { PaneLayoutConfig } from "@/entities/PaneLayoutConfig";
import { AppSettings } from "@/entities/AppSettings";
import { ensureRuntimeDatabaseFile, getRuntimeDatabasePath } from "@/lib/databasePaths";
import { ensureSqliteDatabaseReady } from "@/lib/sqliteSchema";
import { InitialSchema1770854400000 } from "@/migrations/1770854400000-InitialSchema";
import { AddPrUrlToKanbanTasks1770854400001 } from "@/migrations/1770854400001-AddPrUrlToKanbanTasks";
import { AddIsWorktreeToProjects1770854400002 } from "@/migrations/1770854400002-AddIsWorktreeToProjects";
import { AddPaneLayoutConfig1771048256887 } from "@/migrations/1771048256887-AddPaneLayoutConfig";
import { AssignDisplayOrder1771166346785 } from "@/migrations/1771166346785-AssignDisplayOrder";
import { AddAppSettings1771166907165 } from "@/migrations/1771166907165-AddAppSettings";
import { AddPendingStatus1771171200000 } from "@/migrations/1771171200000-AddPendingStatus";
import { RemoveBranchNameUnique1771257600000 } from "@/migrations/1771257600000-RemoveBranchNameUnique";
import { AddColorIndexToProjects1771343199455 } from "@/migrations/1771343199455-AddColorIndexToProjects";
import { AddPriorityToKanbanTasks1771344000000 } from "@/migrations/1771344000000-AddPriorityToKanbanTasks";
import { ReplaceColorIndexWithColor1771388085809 } from "@/migrations/1771388085809-ReplaceColorIndexWithColor";
import { FillEmptyBaseBranch1771400000000 } from "@/migrations/1771400000000-FillEmptyBaseBranch";
import { AddIconDataUrlToProjects1771500000000 } from "@/migrations/1771500000000-AddIconDataUrlToProjects";

/** TypeORM DataSource 싱글턴. Vite HMR 시 재연결을 방지하기 위해 globalThis에 캐싱한다. */
const globalForDb = globalThis as unknown as {
  dataSource: DataSource | undefined;
};

const MIGRATIONS = [
  InitialSchema1770854400000,
  AddPrUrlToKanbanTasks1770854400001,
  AddIsWorktreeToProjects1770854400002,
  AddPaneLayoutConfig1771048256887,
  AssignDisplayOrder1771166346785,
  AddAppSettings1771166907165,
  AddPendingStatus1771171200000,
  RemoveBranchNameUnique1771257600000,
  AddColorIndexToProjects1771343199455,
  AddPriorityToKanbanTasks1771344000000,
  ReplaceColorIndexWithColor1771388085809,
  FillEmptyBaseBranch1771400000000,
  AddIconDataUrlToProjects1771500000000,
];

interface MigrationRecord {
  name: string;
  timestamp: number;
}

function getMigrationRecords(): MigrationRecord[] {
  return MIGRATIONS.map((MigrationClass) => {
    const migration = new MigrationClass();
    const name = migration.name || migration.constructor.name;
    const timestamp = Number.parseInt(name.slice(-13), 10);

    if (!timestamp || Number.isNaN(timestamp)) {
      throw new Error(`Invalid migration name: ${name}`);
    }

    return { name, timestamp };
  }).sort((a, b) => a.timestamp - b.timestamp);
}

function databaseHasTable(databasePath: string, tableName: string): boolean {
  const database = new Database(databasePath, { readonly: true, fileMustExist: true });

  try {
    const row = database
      .prepare(
        `
          SELECT 1 AS "exists"
          FROM sqlite_master
          WHERE type = 'table' AND name = ?
          LIMIT 1
        `,
      )
      .get(tableName);

    return row !== undefined;
  } finally {
    database.close();
  }
}

async function baselineExistingSqliteDatabase(ds: DataSource): Promise<void> {
  const queryRunner = ds.createQueryRunner();

  try {
    const hasKanbanTasksTable = await queryRunner.hasTable("kanban_tasks");
    if (!hasKanbanTasksTable) {
      return;
    }

    await queryRunner.startTransaction();

    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "migrations" (
        "id" integer PRIMARY KEY AUTOINCREMENT NOT NULL,
        "timestamp" bigint NOT NULL,
        "name" varchar NOT NULL
      )
    `);

    const rows: Array<{ count: number }> = await queryRunner.query(
      `SELECT COUNT(1) AS "count" FROM "migrations"`,
    );
    if (Number(rows[0]?.count ?? 0) > 0) {
      await queryRunner.commitTransaction();
      return;
    }

    for (const migration of getMigrationRecords()) {
      await queryRunner.query(
        `INSERT INTO "migrations"("timestamp", "name") VALUES (?, ?)`,
        [migration.timestamp, migration.name],
      );
    }

    await queryRunner.commitTransaction();
  } catch (error) {
    if (queryRunner.isTransactionActive) {
      await queryRunner.rollbackTransaction();
    }
    throw error;
  } finally {
    await queryRunner.release();
  }
}

function createDataSource(): DataSource {
  const databasePath = getRuntimeDatabasePath();
  const shouldLogSql = process.env.TYPEORM_LOGGING === "true";

  return new DataSource({
    type: "better-sqlite3",
    database: databasePath,
    entities: [KanbanTask, Project, PaneLayoutConfig, AppSettings],
    migrations: MIGRATIONS,
    synchronize: false,
    logging: shouldLogSql,
    prepareDatabase: (database) => {
      database.pragma("journal_mode = WAL");
      database.pragma("foreign_keys = ON");
    },
  });
}

export async function getDataSource(): Promise<DataSource> {
  if (globalForDb.dataSource?.isInitialized) {
    return globalForDb.dataSource;
  }

  const databasePath = ensureRuntimeDatabaseFile();
  if (databaseHasTable(databasePath, "kanban_tasks")) {
    ensureSqliteDatabaseReady(databasePath);
  }

  const ds = createDataSource();
  await ds.initialize();
  await baselineExistingSqliteDatabase(ds);
  await ds.runMigrations();
  globalForDb.dataSource = ds;
  return ds;
}

/**
 * 테이블 이름으로 엔티티 메타데이터를 찾아 리포지토리를 반환한다.
 * 프로덕션 빌드에서 SWC가 클래스명을 minify하면 문자열 조회("KanbanTask")가 실패하고,
 * tsx/cjs와 Turbopack 간 모듈 인스턴스가 다르면 클래스 참조 조회도 실패한다.
 * 테이블 이름은 @Entity 데코레이터의 문자열 리터럴이므로 양쪽 모두에서 안전하다.
 */
function getRepositoryByTable<T extends ObjectLiteral>(ds: DataSource, tableName: string): Repository<T> {
  const metadata = ds.entityMetadatas.find((m) => m.tableName === tableName);
  if (!metadata) {
    const available = ds.entityMetadatas.map((m) => m.tableName).join(", ");
    throw new Error(
      `Entity metadata not found for table "${tableName}". Available tables: [${available}]`,
    );
  }
  return ds.getRepository(metadata.target) as Repository<T>;
}

export async function getTaskRepository(): Promise<Repository<KanbanTask>> {
  const ds = await getDataSource();
  return getRepositoryByTable<KanbanTask>(ds, "kanban_tasks");
}

export async function getProjectRepository(): Promise<Repository<Project>> {
  const ds = await getDataSource();
  return getRepositoryByTable<Project>(ds, "projects");
}

export async function getPaneLayoutConfigRepository(): Promise<Repository<PaneLayoutConfig>> {
  const ds = await getDataSource();
  return getRepositoryByTable<PaneLayoutConfig>(ds, "pane_layout_configs");
}

export async function getAppSettingsRepository(): Promise<Repository<AppSettings>> {
  const ds = await getDataSource();
  return getRepositoryByTable<AppSettings>(ds, "app_settings");
}
