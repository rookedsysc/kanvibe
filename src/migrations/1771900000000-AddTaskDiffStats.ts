import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * task_diff_stats 테이블을 생성한다.
 * 태스크가 지워지면 그 집계도 함께 사라져야 하므로 FK를 CASCADE로 건다.
 */
export class AddTaskDiffStats1771900000000 implements MigrationInterface {
  name = "AddTaskDiffStats1771900000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "task_diff_stats" (
        "task_id" TEXT PRIMARY KEY NOT NULL,
        "file_count" INTEGER NOT NULL DEFAULT 0,
        "additions" INTEGER NOT NULL DEFAULT 0,
        "deletions" INTEGER NOT NULL DEFAULT 0,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("task_id") REFERENCES "kanban_tasks"("id") ON DELETE CASCADE
      )
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`DROP TABLE IF EXISTS "task_diff_stats"`);
  }
}
