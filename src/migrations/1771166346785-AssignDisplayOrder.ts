import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * 기존 태스크에 status별 createdAt 순서 기반으로 display_order를 부여한다.
 * 모든 기존 태스크가 display_order = 0이므로, ROW_NUMBER로 0-based 순서를 할당한다.
 */
export class AssignDisplayOrder1771166346785 implements MigrationInterface {
  name = "AssignDisplayOrder1771166346785";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      UPDATE kanban_tasks
      SET display_order = sub.rn
      FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY status ORDER BY created_at ASC) - 1 AS rn
        FROM kanban_tasks
      ) sub
      WHERE kanban_tasks.id = sub.id
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`UPDATE kanban_tasks SET display_order = 0`);
  }
}
