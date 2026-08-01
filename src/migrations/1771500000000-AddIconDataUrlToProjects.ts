import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists } from "./sqliteMigrationUtils";

interface ColumnInfo {
  name: string;
}

/** 프로젝트 제목 앞에 표시할 GitHub repo/org 아이콘을 data URL로 보관한다 */
export class AddIconDataUrlToProjects1771500000000 implements MigrationInterface {
  name = "AddIconDataUrlToProjects1771500000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "projects", "icon_data_url", `"icon_data_url" TEXT DEFAULT NULL`);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    const columns: ColumnInfo[] = await queryRunner.query(`PRAGMA table_info("projects")`);
    if (columns.some((column) => column.name === "icon_data_url")) {
      await queryRunner.query(`ALTER TABLE "projects" DROP COLUMN "icon_data_url"`);
    }
  }
}
