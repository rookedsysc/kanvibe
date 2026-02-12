import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * kanban_tasks 테이블에 pr_url 컬럼을 추가한다.
 */
export class AddPrUrlToKanbanTasks1770854400001 implements MigrationInterface {
  name = "AddPrUrlToKanbanTasks1770854400001";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TABLE "kanban_tasks" ADD COLUMN IF NOT EXISTS "pr_url" character varying(500)`,
    );
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TABLE "kanban_tasks" DROP COLUMN IF EXISTS "pr_url"`,
    );
  }
}
