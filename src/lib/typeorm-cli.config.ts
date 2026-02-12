import "reflect-metadata";
import { DataSource } from "typeorm";
import { KanbanTask } from "../entities/KanbanTask";
import { Project } from "../entities/Project";

/**
 * TypeORM CLI 전용 DataSource 설정.
 * migration:generate, migration:run, migration:revert 명령에서 사용한다.
 * Next.js path alias(@/*)가 CLI에서 동작하지 않으므로 상대 경로를 사용한다.
 */
export default new DataSource({
  type: "postgres",
  url: process.env.DATABASE_URL ?? "postgresql://kanvibe:kanvibe@localhost:5432/kanvibe",
  entities: [KanbanTask, Project],
  migrations: ["src/migrations/*.ts"],
  synchronize: false,
  logging: true,
});
