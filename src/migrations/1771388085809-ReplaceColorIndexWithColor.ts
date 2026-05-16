import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists } from "./sqliteMigrationUtils";

interface ColumnInfo {
  name: string;
}

/**
 * color_index(INTEGER)를 color(TEXT)로 교체한다.
 * SQLite는 타입 변경이 불가하므로 컬럼명 변경으로 처리한다.
 * 이미 color 컬럼이 있는 DB(sqliteSchema.ts로 생성된 경우)는 no-op.
 */
export class ReplaceColorIndexWithColor1771388085809 implements MigrationInterface {
  name = "ReplaceColorIndexWithColor1771388085809";

  public async up(queryRunner: QueryRunner): Promise<void> {
    const columns: ColumnInfo[] = await queryRunner.query(`PRAGMA table_info("projects")`);
    const hasColorIndex = columns.some((c) => c.name === "color_index");
    const hasColor = columns.some((c) => c.name === "color");

    if (hasColorIndex && !hasColor) {
      await queryRunner.query(`ALTER TABLE "projects" RENAME COLUMN "color_index" TO "color"`);
    } else if (!hasColor) {
      await addColumnIfNotExists(queryRunner, "projects", "color", `"color" TEXT DEFAULT NULL`);
    }
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    const columns: ColumnInfo[] = await queryRunner.query(`PRAGMA table_info("projects")`);
    const hasColor = columns.some((c) => c.name === "color");
    const hasColorIndex = columns.some((c) => c.name === "color_index");

    if (hasColor && !hasColorIndex) {
      await queryRunner.query(`ALTER TABLE "projects" RENAME COLUMN "color" TO "color_index"`);
    }
  }
}
