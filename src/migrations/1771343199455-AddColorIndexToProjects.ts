import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists, dropColumnIfExists } from "./sqliteMigrationUtils";

/** projects 테이블에 color_index 컬럼을 추가한다. SQLite에서 smallint는 INTEGER로 저장된다. */
export class AddColorIndexToProjects1771343199455 implements MigrationInterface {
  name = "AddColorIndexToProjects1771343199455";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "projects", "color_index", `"color_index" INTEGER`);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await dropColumnIfExists(queryRunner, "projects", "color_index");
  }
}
