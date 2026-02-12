import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * 초기 스키마 마이그레이션.
 * projects, kanban_tasks 테이블과 관련 enum 타입을 생성한다.
 * synchronize: true로 이미 생성된 DB에서도 안전하게 실행되도록 IF NOT EXISTS를 사용한다.
 */
export class InitialSchema1770854400000 implements MigrationInterface {
  name = "InitialSchema1770854400000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      DO $$ BEGIN
        CREATE TYPE "kanban_tasks_status_enum" AS ENUM('todo', 'progress', 'review', 'done');
      EXCEPTION WHEN duplicate_object THEN NULL;
      END $$
    `);
    await queryRunner.query(`
      DO $$ BEGIN
        CREATE TYPE "kanban_tasks_session_type_enum" AS ENUM('tmux', 'zellij');
      EXCEPTION WHEN duplicate_object THEN NULL;
      END $$
    `);

    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "projects" (
        "id" uuid NOT NULL DEFAULT uuid_generate_v4(),
        "name" character varying(255) NOT NULL,
        "repo_path" character varying(500) NOT NULL,
        "default_branch" character varying(255) NOT NULL DEFAULT 'main',
        "ssh_host" character varying(255),
        "created_at" TIMESTAMP NOT NULL DEFAULT now(),
        CONSTRAINT "PK_projects" PRIMARY KEY ("id")
      )
    `);

    await queryRunner.query(`
      DO $$ BEGIN
        ALTER TABLE "projects" ADD CONSTRAINT "UQ_projects_name" UNIQUE ("name");
      EXCEPTION WHEN duplicate_object THEN NULL;
      END $$
    `);

    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "kanban_tasks" (
        "id" uuid NOT NULL DEFAULT uuid_generate_v4(),
        "title" character varying(255) NOT NULL,
        "description" text,
        "status" "kanban_tasks_status_enum" NOT NULL DEFAULT 'todo',
        "branch_name" character varying(255),
        "worktree_path" character varying(500),
        "session_type" "kanban_tasks_session_type_enum",
        "session_name" character varying(255),
        "ssh_host" character varying(255),
        "agent_type" character varying(50),
        "project_id" uuid,
        "base_branch" character varying(255),
        "display_order" integer NOT NULL DEFAULT 0,
        "created_at" TIMESTAMP NOT NULL DEFAULT now(),
        "updated_at" TIMESTAMP NOT NULL DEFAULT now(),
        CONSTRAINT "PK_kanban_tasks" PRIMARY KEY ("id")
      )
    `);

    await queryRunner.query(`
      DO $$ BEGIN
        ALTER TABLE "kanban_tasks" ADD CONSTRAINT "UQ_kanban_tasks_branch_name" UNIQUE ("branch_name");
      EXCEPTION WHEN duplicate_object THEN NULL;
      END $$
    `);

    await queryRunner.query(`
      DO $$ BEGIN
        ALTER TABLE "kanban_tasks"
          ADD CONSTRAINT "FK_kanban_tasks_project_id"
          FOREIGN KEY ("project_id") REFERENCES "projects"("id") ON DELETE SET NULL;
      EXCEPTION WHEN duplicate_object THEN NULL;
      END $$
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(
      `ALTER TABLE "kanban_tasks" DROP CONSTRAINT IF EXISTS "FK_kanban_tasks_project_id"`,
    );
    await queryRunner.query(`DROP TABLE IF EXISTS "kanban_tasks"`);
    await queryRunner.query(`DROP TABLE IF EXISTS "projects"`);
    await queryRunner.query(`DROP TYPE IF EXISTS "kanban_tasks_session_type_enum"`);
    await queryRunner.query(`DROP TYPE IF EXISTS "kanban_tasks_status_enum"`);
  }
}
