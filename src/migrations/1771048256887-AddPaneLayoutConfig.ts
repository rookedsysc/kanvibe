import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * pane_layout_configs 테이블을 생성한다.
 * SQLite는 ALTER TABLE로 FK 제약을 추가/삭제할 수 없으므로 FK 조작은 생략한다.
 */
export class AddPaneLayoutConfig1771048256887 implements MigrationInterface {
  name = "AddPaneLayoutConfig1771048256887";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "pane_layout_configs" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "layout_type" TEXT NOT NULL,
        "panes" TEXT NOT NULL,
        "project_id" TEXT UNIQUE,
        "is_global" INTEGER NOT NULL DEFAULT 0,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE CASCADE
      )
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`DROP TABLE IF EXISTS "pane_layout_configs"`);
  }
}
