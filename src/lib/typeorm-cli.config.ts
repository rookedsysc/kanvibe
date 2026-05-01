import "reflect-metadata";
import { DataSource } from "typeorm";
import { KanbanTask } from "../entities/KanbanTask";
import { Project } from "../entities/Project";
import { PaneLayoutConfig } from "../entities/PaneLayoutConfig";
import { AppSettings } from "../entities/AppSettings";
import { getRuntimeDatabasePath } from "./databasePaths";

/**
 * TypeORM CLI 전용 DataSource 설정.
 * 내장 SQLite DB를 조회하거나 ad-hoc 점검할 때 사용한다.
 */
export default new DataSource({
  type: "better-sqlite3",
  database: getRuntimeDatabasePath(),
  entities: [KanbanTask, Project, PaneLayoutConfig, AppSettings],
  synchronize: false,
  logging: process.env.TYPEORM_LOGGING === "true",
  prepareDatabase: (database) => {
    database.pragma("journal_mode = WAL");
    database.pragma("foreign_keys = ON");
  },
});
