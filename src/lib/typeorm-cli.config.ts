import "reflect-metadata";
import { DataSource } from "typeorm";
import { KanbanTask } from "../entities/KanbanTask";
import { Project } from "../entities/Project";
import { PaneLayoutConfig } from "../entities/PaneLayoutConfig";

/**
 * TypeORM CLI 전용 DataSource 설정.
 * migration:generate, migration:run, migration:revert 명령에서 사용한다.
 * Next.js path alias(@/*)가 CLI에서 동작하지 않으므로 상대 경로를 사용한다.
 */
function buildDatabaseUrl(): string {
  const user = encodeURIComponent(process.env.KANVIBE_USER || "admin");
  const password = encodeURIComponent(process.env.KANVIBE_PASSWORD || "changeme");
  const port = process.env.DB_PORT || "4886";
  return `postgresql://${user}:${password}@localhost:${port}/kanvibe`;
}

export default new DataSource({
  type: "postgres",
  url: process.env.DATABASE_URL ?? buildDatabaseUrl(),
  entities: [KanbanTask, Project, PaneLayoutConfig],
  migrations: ["src/migrations/*.ts"],
  synchronize: false,
  logging: true,
});
