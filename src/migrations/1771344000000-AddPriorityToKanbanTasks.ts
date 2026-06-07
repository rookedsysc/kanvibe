import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists, dropColumnIfExists } from "./sqliteMigrationUtils";

/**
 * kanban_tasks 테이블에 priority 컬럼을 추가한다.
 * SQLite는 ENUM 타입이 없으므로 TEXT로 저장한다 (low/medium/high).
 */
export class AddPriorityToKanbanTasks1771344000000 implements MigrationInterface {
  name = "AddPriorityToKanbanTasks1771344000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "kanban_tasks", "priority", `"priority" TEXT DEFAULT NULL`);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await dropColumnIfExists(queryRunner, "kanban_tasks", "priority");
  }
}
