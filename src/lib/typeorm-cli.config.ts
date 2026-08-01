import "reflect-metadata";
import { DataSource } from "typeorm";
import { KanbanTask } from "../entities/KanbanTask";
import { Project } from "../entities/Project";
import { PaneLayoutConfig } from "../entities/PaneLayoutConfig";
import { AppSettings } from "../entities/AppSettings";
import { getRuntimeDatabasePath } from "./databasePaths";
import { InitialSchema1770854400000 } from "../migrations/1770854400000-InitialSchema";
import { AddPrUrlToKanbanTasks1770854400001 } from "../migrations/1770854400001-AddPrUrlToKanbanTasks";
import { AddIsWorktreeToProjects1770854400002 } from "../migrations/1770854400002-AddIsWorktreeToProjects";
import { AddPaneLayoutConfig1771048256887 } from "../migrations/1771048256887-AddPaneLayoutConfig";
import { AssignDisplayOrder1771166346785 } from "../migrations/1771166346785-AssignDisplayOrder";
import { AddAppSettings1771166907165 } from "../migrations/1771166907165-AddAppSettings";
import { AddPendingStatus1771171200000 } from "../migrations/1771171200000-AddPendingStatus";
import { RemoveBranchNameUnique1771257600000 } from "../migrations/1771257600000-RemoveBranchNameUnique";
import { AddColorIndexToProjects1771343199455 } from "../migrations/1771343199455-AddColorIndexToProjects";
import { AddPriorityToKanbanTasks1771344000000 } from "../migrations/1771344000000-AddPriorityToKanbanTasks";
import { ReplaceColorIndexWithColor1771388085809 } from "../migrations/1771388085809-ReplaceColorIndexWithColor";
import { FillEmptyBaseBranch1771400000000 } from "../migrations/1771400000000-FillEmptyBaseBranch";
import { AddIconDataUrlToProjects1771500000000 } from "../migrations/1771500000000-AddIconDataUrlToProjects";

/** TypeORM CLI 전용 DataSource 설정. 내장 SQLite DB를 조회하거나 ad-hoc 점검할 때 사용한다. */
export default new DataSource({
  type: "better-sqlite3",
  database: getRuntimeDatabasePath(),
  entities: [KanbanTask, Project, PaneLayoutConfig, AppSettings],
  migrations: [
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
  ],
  synchronize: false,
  logging: process.env.TYPEORM_LOGGING === "true",
  prepareDatabase: (database) => {
    database.pragma("journal_mode = WAL");
    database.pragma("foreign_keys = ON");
  },
});
