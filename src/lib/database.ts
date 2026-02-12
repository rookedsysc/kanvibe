import "reflect-metadata";
import { DataSource, type Repository } from "typeorm";
import { KanbanTask } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";

/**
 * TypeORM DataSource 싱글턴.
 * Next.js hot-reload 시 재연결을 방지하기 위해 global 객체에 캐싱한다.
 */
const globalForDb = globalThis as unknown as {
  dataSource: DataSource | undefined;
};

function createDataSource(): DataSource {
  return new DataSource({
    type: "postgres",
    url: "postgresql://kanvibe:kanvibe@localhost:5432/kanvibe",
    entities: [KanbanTask, Project],
    synchronize: true,
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
 * KanbanTask 리포지토리를 반환한다.
 * 엔티티 이름 문자열로 조회하여 tsx/cjs와 Turbopack 간 모듈 인스턴스 차이를 우회한다.
 */
export async function getTaskRepository(): Promise<Repository<KanbanTask>> {
  const ds = await getDataSource();
  return ds.getRepository("KanbanTask") as Repository<KanbanTask>;
}

export async function getProjectRepository(): Promise<Repository<Project>> {
  const ds = await getDataSource();
  return ds.getRepository("Project") as Repository<Project>;
}
