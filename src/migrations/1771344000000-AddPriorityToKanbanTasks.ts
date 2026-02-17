import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * kanban_tasks 테이블에 priority 컬럼을 추가한다.
 * priority는 low/medium/high enum 값을 가지며 nullable이다.
 */
export class AddPriorityToKanbanTasks1771344000000 implements MigrationInterface {
  name = "AddPriorityToKanbanTasks1771344000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `CREATE TYPE "kanban_tasks_priority_enum" AS ENUM('low', 'medium', 'high')`,
    );
    await queryRunner.query(
      `ALTER TABLE "kanban_tasks" ADD "priority" "kanban_tasks_priority_enum" DEFAULT NULL`,
    );
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TABLE "kanban_tasks" DROP COLUMN "priority"`,
    );
    await queryRunner.query(
      `DROP TYPE "kanban_tasks_priority_enum"`,
    );
  }
}
