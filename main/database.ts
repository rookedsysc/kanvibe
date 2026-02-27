import "reflect-metadata";
import path from "path";
import { app } from "electron";
import { DataSource, type ObjectLiteral, type Repository } from "typeorm";
import { KanbanTask } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";
import { PaneLayoutConfig } from "@/entities/PaneLayoutConfig";
import { AppSettings } from "@/entities/AppSettings";
import { InitialSqliteSchema1770854400000 } from "@/migrations/0001-InitialSqliteSchema";

let dataSource: DataSource | undefined;

function getDatabasePath(): string {
  const userDataPath = app.getPath("userData");
  const dbFileName = app.isPackaged ? "kanvibe.sqlite" : "kanvibe.dev.sqlite";
  return path.join(userDataPath, dbFileName);
}

function createDataSource(): DataSource {
  return new DataSource({
    type: "better-sqlite3",
    database: getDatabasePath(),
    entities: [KanbanTask, Project, PaneLayoutConfig, AppSettings],
    migrations: [InitialSqliteSchema1770854400000],
    synchronize: false,
    logging: !app.isPackaged,
    prepareDatabase: (db) => {
      db.pragma("journal_mode = WAL");
    },
  });
}

/** DB를 초기화하고 미실행 migration을 적용한다 */
export async function setupDatabase(): Promise<void> {
  const ds = createDataSource();
  await ds.initialize();
  await ds.runMigrations();
  dataSource = ds;
  console.log(`[DB] SQLite initialized: ${getDatabasePath()}`);
}

export function getDataSource(): DataSource {
  if (!dataSource?.isInitialized) {
    throw new Error("Database not initialized. Call setupDatabase() first.");
  }
  return dataSource;
}

/**
 * 테이블 이름으로 엔티티 메타데이터를 찾아 리포지토리를 반환한다.
 * 프로덕션 빌드에서 SWC가 클래스명을 minify하면 문자열 조회("KanbanTask")가 실패하고,
 * 테이블 이름은 @Entity 데코레이터의 문자열 리터럴이므로 양쪽 모두에서 안전하다.
 */
function getRepositoryByTable<T extends ObjectLiteral>(tableName: string): Repository<T> {
  const ds = getDataSource();
  const metadata = ds.entityMetadatas.find((m) => m.tableName === tableName);
  if (!metadata) {
    const available = ds.entityMetadatas.map((m) => m.tableName).join(", ");
    throw new Error(
      `Entity metadata not found for table "${tableName}". Available tables: [${available}]`,
    );
  }
  return ds.getRepository(metadata.target) as Repository<T>;
}

export function getTaskRepository(): Repository<KanbanTask> {
  return getRepositoryByTable<KanbanTask>("kanban_tasks");
}

export function getProjectRepository(): Repository<Project> {
  return getRepositoryByTable<Project>("projects");
}

export function getPaneLayoutConfigRepository(): Repository<PaneLayoutConfig> {
  return getRepositoryByTable<PaneLayoutConfig>("pane_layout_configs");
}

export function getAppSettingsRepository(): Repository<AppSettings> {
  return getRepositoryByTable<AppSettings>("app_settings");
}
