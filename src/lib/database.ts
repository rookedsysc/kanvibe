import "reflect-metadata";
import { DataSource, type ObjectLiteral, type Repository } from "typeorm";
import { KanbanTask } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";
import { InitialSchema1770854400000 } from "@/migrations/1770854400000-InitialSchema";
import { AddPrUrlToKanbanTasks1770854400001 } from "@/migrations/1770854400001-AddPrUrlToKanbanTasks";
import { AddIsWorktreeToProjects1770854400002 } from "@/migrations/1770854400002-AddIsWorktreeToProjects";

/**
 * TypeORM DataSource 싱글턴.
 * Next.js hot-reload 시 재연결을 방지하기 위해 global 객체에 캐싱한다.
 */
const globalForDb = globalThis as unknown as {
  dataSource: DataSource | undefined;
};

function buildDatabaseUrl(): string {
  const user = encodeURIComponent(process.env.KANVIBE_USER || "admin");
  const password = encodeURIComponent(process.env.KANVIBE_PASSWORD || "changeme");
  const port = process.env.DB_PORT || "4886";
  return `postgresql://${user}:${password}@localhost:${port}/kanvibe`;
}

function createDataSource(): DataSource {
  return new DataSource({
    type: "postgres",
    url: process.env.DATABASE_URL ?? buildDatabaseUrl(),
    entities: [KanbanTask, Project],
    migrations: [InitialSchema1770854400000, AddPrUrlToKanbanTasks1770854400001, AddIsWorktreeToProjects1770854400002],
    synchronize: false,
    logging: process.env.NODE_ENV !== "production",
  });
}

export async function getDataSource(): Promise<DataSource> {
  if (globalForDb.dataSource?.isInitialized) {
    return globalForDb.dataSource;
  }

  const ds = createDataSource();
  await ds.initialize();
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
