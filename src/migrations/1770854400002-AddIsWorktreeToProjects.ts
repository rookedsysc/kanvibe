import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists, dropColumnIfExists } from "./sqliteMigrationUtils";

/** projects 테이블에 is_worktree 컬럼을 추가한다. SQLite에서 boolean은 INTEGER(0/1)로 저장된다. */
export class AddIsWorktreeToProjects1770854400002 implements MigrationInterface {
  name = "AddIsWorktreeToProjects1770854400002";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "projects", "is_worktree", `"is_worktree" INTEGER NOT NULL DEFAULT 0`);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await dropColumnIfExists(queryRunner, "projects", "is_worktree");
  }
}
