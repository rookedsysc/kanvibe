import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists, dropColumnIfExists } from "./sqliteMigrationUtils";

/** kanban_tasks 테이블에 pr_url 컬럼을 추가한다. */
export class AddPrUrlToKanbanTasks1770854400001 implements MigrationInterface {
  name = "AddPrUrlToKanbanTasks1770854400001";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "kanban_tasks", "pr_url", `"pr_url" TEXT`);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await dropColumnIfExists(queryRunner, "kanban_tasks", "pr_url");
  }
}
