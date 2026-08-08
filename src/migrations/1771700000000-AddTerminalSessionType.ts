import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * session_type 컬럼에 'terminal' 값을 허용한다.
 * SQLite는 ENUM 타입이 없고 TEXT로 저장하므로 스키마 변경이 불필요하다.
 */
export class AddTerminalSessionType1771700000000 implements MigrationInterface {
  name = "AddTerminalSessionType1771700000000";

  public async up(_queryRunner: QueryRunner): Promise<void> {
    // SQLite는 TEXT 컬럼이 모든 문자열 값을 허용하므로 no-op
  }

  /**
   * 되돌린 버전은 'terminal'을 모르고 tmux가 아닌 값을 zellij로 취급한다.
   * 붙을 zellij 세션이 실제로는 없으므로, 세션 연결을 지워 사용자가 다시 연결하게 한다.
   */
  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `UPDATE "kanban_tasks" SET "session_type" = NULL, "session_name" = NULL WHERE "session_type" = 'terminal'`,
    );
  }
}
