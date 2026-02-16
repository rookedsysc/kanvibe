import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * branch_name 컬럼의 UNIQUE 제약을 제거한다.
 * 서로 다른 프로젝트에서 동일한 브랜치명(예: main)을 가진 태스크를 허용한다.
 */
export class RemoveBranchNameUnique1771257600000 implements MigrationInterface {
  name = "RemoveBranchNameUnique1771257600000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TABLE "kanban_tasks" DROP CONSTRAINT IF EXISTS "UQ_kanban_tasks_branch_name"`,
    );
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      DO $$ BEGIN
        ALTER TABLE "kanban_tasks" ADD CONSTRAINT "UQ_kanban_tasks_branch_name" UNIQUE ("branch_name");
      EXCEPTION WHEN duplicate_object THEN NULL;
      END $$
    `);
  }
}
