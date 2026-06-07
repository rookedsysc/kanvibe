import { MigrationInterface, QueryRunner } from "typeorm";

/** projects, kanban_tasks 초기 테이블을 생성한다. */
export class InitialSchema1770854400000 implements MigrationInterface {
  name = "InitialSchema1770854400000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "projects" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "name" TEXT NOT NULL UNIQUE,
        "repo_path" TEXT NOT NULL,
        "default_branch" TEXT NOT NULL DEFAULT 'main',
        "ssh_host" TEXT,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
      )
    `);

    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "kanban_tasks" (
        "id" TEXT PRIMARY KEY NOT NULL,
        "title" TEXT NOT NULL,
        "description" TEXT,
        "status" TEXT NOT NULL DEFAULT 'todo',
        "branch_name" TEXT,
        "worktree_path" TEXT,
        "session_type" TEXT,
        "session_name" TEXT,
        "ssh_host" TEXT,
        "agent_type" TEXT,
        "project_id" TEXT,
        "base_branch" TEXT,
        "display_order" INTEGER NOT NULL DEFAULT 0,
        "created_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        "updated_at" DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE SET NULL
      )
    `);

    await queryRunner.query(`
      CREATE UNIQUE INDEX IF NOT EXISTS "UQ_kanban_tasks_branch_name"
        ON kanban_tasks(branch_name)
    `);

    await queryRunner.query(`
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, display_order, created_at)
    `);

    await queryRunner.query(`
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_project_branch"
        ON kanban_tasks(project_id, branch_name)
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`DROP TABLE IF EXISTS "kanban_tasks"`);
    await queryRunner.query(`DROP TABLE IF EXISTS "projects"`);
  }
}
