import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * base_branch가 비어있는 태스크를 소속 프로젝트의 default_branch로 채운다.
 * SQLite는 UPDATE...FROM을 지원하지 않으므로 상관 서브쿼리를 사용한다.
 */
export class FillEmptyBaseBranch1771400000000 implements MigrationInterface {
  name = "FillEmptyBaseBranch1771400000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      UPDATE kanban_tasks
      SET base_branch = (
        SELECT p.default_branch
        FROM projects p
        WHERE p.id = kanban_tasks.project_id
      )
      WHERE project_id IS NOT NULL
        AND (base_branch IS NULL OR base_branch = '')
    `);
  }

  public async down(_queryRunner: QueryRunner): Promise<void> {
    // 보정 전 원본 값을 복원할 수 없으므로 rollback은 no-op
  }
}
