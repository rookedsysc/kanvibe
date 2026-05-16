import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * branch_name 컬럼의 UNIQUE 인덱스를 제거한다.
 * 서로 다른 프로젝트에서 동일한 브랜치명(예: main)을 가진 태스크를 허용한다.
 * SQLite는 DROP CONSTRAINT를 지원하지 않으므로 인덱스를 직접 삭제한다.
 */
export class RemoveBranchNameUnique1771257600000 implements MigrationInterface {
  name = "RemoveBranchNameUnique1771257600000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`DROP INDEX IF EXISTS "UQ_kanban_tasks_branch_name"`);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE UNIQUE INDEX IF NOT EXISTS "UQ_kanban_tasks_branch_name"
        ON kanban_tasks(branch_name)
    `);
  }
}
