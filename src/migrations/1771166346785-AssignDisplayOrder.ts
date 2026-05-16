import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * 기존 태스크에 status별 created_at 순서 기반으로 display_order를 부여한다.
 * SQLite는 UPDATE...FROM을 지원하지 않으므로 상관 서브쿼리로 0-based 순서를 계산한다.
 */
export class AssignDisplayOrder1771166346785 implements MigrationInterface {
  name = "AssignDisplayOrder1771166346785";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      UPDATE kanban_tasks
      SET display_order = (
        SELECT COUNT(*)
        FROM kanban_tasks t2
        WHERE t2.status = kanban_tasks.status
          AND t2.created_at < kanban_tasks.created_at
      )
      WHERE display_order = 0
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`UPDATE kanban_tasks SET display_order = 0`);
  }
}
