import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * kanban_tasks의 status enum 타입에 'pending' 값을 추가한다.
 * PROGRESS와 REVIEW 사이에 위치하는 사용자 의사결정 대기 상태이다.
 */
export class AddPendingStatus1771171200000 implements MigrationInterface {
  name = "AddPendingStatus1771171200000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TYPE "kanban_tasks_status_enum" ADD VALUE IF NOT EXISTS 'pending' BEFORE 'review'`,
    );
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    /**
     * PostgreSQL에서는 ALTER TYPE ... DROP VALUE를 직접 지원하지 않는다.
     * 롤백이 필요한 경우 새로운 enum 타입을 만들어 교체해야 한다.
     */
    await queryRunner.query(`
      UPDATE "kanban_tasks" SET "status" = 'review' WHERE "status" = 'pending'
    `);
    await queryRunner.query(`
      ALTER TYPE "kanban_tasks_status_enum" RENAME TO "kanban_tasks_status_enum_old"
    `);
    await queryRunner.query(`
      CREATE TYPE "kanban_tasks_status_enum" AS ENUM('todo', 'progress', 'review', 'done')
    `);
    await queryRunner.query(`
      ALTER TABLE "kanban_tasks" ALTER COLUMN "status" TYPE "kanban_tasks_status_enum" USING "status"::text::"kanban_tasks_status_enum"
    `);
    await queryRunner.query(`
      DROP TYPE "kanban_tasks_status_enum_old"
    `);
  }
}
