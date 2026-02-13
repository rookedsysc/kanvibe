import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * projects 테이블에 is_worktree 컬럼을 추가한다.
 */
export class AddIsWorktreeToProjects1770854400002 implements MigrationInterface {
  name = "AddIsWorktreeToProjects1770854400002";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TABLE "projects" ADD COLUMN IF NOT EXISTS "is_worktree" boolean NOT NULL DEFAULT false`,
    );
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TABLE "projects" DROP COLUMN IF EXISTS "is_worktree"`,
    );
  }
}
