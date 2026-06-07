import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * status 컬럼에 'pending' 값을 허용한다.
 * SQLite는 ENUM 타입이 없고 TEXT로 저장하므로 스키마 변경이 불필요하다.
 */
export class AddPendingStatus1771171200000 implements MigrationInterface {
  name = "AddPendingStatus1771171200000";

  public async up(_queryRunner: QueryRunner): Promise<void> {
    // SQLite는 TEXT 컬럼이 모든 문자열 값을 허용하므로 no-op
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `UPDATE "kanban_tasks" SET "status" = 'review' WHERE "status" = 'pending'`,
    );
  }
}
