import { MigrationInterface, QueryRunner } from "typeorm";

/**
 * SQLite 초기 스키마.
 * 기존 PostgreSQL 12개 migration을 통합한 단일 migration이다.
 */
export class InitialSqliteSchema1770854400000 implements MigrationInterface {
  name = "InitialSqliteSchema1770854400000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE TABLE "projects" (
        "id" varchar PRIMARY KEY NOT NULL,
        "name" varchar(255) NOT NULL,
        "repo_path" varchar(500) NOT NULL,
        "default_branch" varchar(255) NOT NULL DEFAULT 'main',
        "ssh_host" varchar(255),
        "is_worktree" boolean NOT NULL DEFAULT (0),
        "color" varchar(7),
        "created_at" datetime NOT NULL DEFAULT (datetime('now')),
        CONSTRAINT "UQ_projects_name" UNIQUE ("name")
      )
    `);

    await queryRunner.query(`
      CREATE TABLE "kanban_tasks" (
        "id" varchar PRIMARY KEY NOT NULL,
        "title" varchar(255) NOT NULL,
        "description" text,
        "status" varchar NOT NULL DEFAULT 'todo',
        "branch_name" varchar(255),
        "worktree_path" varchar(500),
        "session_type" varchar,
        "session_name" varchar(255),
        "ssh_host" varchar(255),
        "agent_type" varchar(50),
        "project_id" varchar,
        "base_branch" varchar(255),
        "pr_url" varchar(500),
        "priority" varchar,
        "display_order" integer NOT NULL DEFAULT (0),
        "created_at" datetime NOT NULL DEFAULT (datetime('now')),
        "updated_at" datetime NOT NULL DEFAULT (datetime('now')),
        CONSTRAINT "FK_kanban_tasks_project" FOREIGN KEY ("project_id") REFERENCES "projects" ("id") ON DELETE SET NULL
      )
    `);

    await queryRunner.query(`
      CREATE TABLE "pane_layout_configs" (
        "id" varchar PRIMARY KEY NOT NULL,
        "layout_type" varchar(50) NOT NULL,
        "panes" text NOT NULL,
        "project_id" varchar,
        "is_global" boolean NOT NULL DEFAULT (0),
        "created_at" datetime NOT NULL DEFAULT (datetime('now')),
        "updated_at" datetime NOT NULL DEFAULT (datetime('now')),
        CONSTRAINT "UQ_pane_layout_configs_project_id" UNIQUE ("project_id"),
        CONSTRAINT "FK_pane_layout_configs_project" FOREIGN KEY ("project_id") REFERENCES "projects" ("id") ON DELETE CASCADE
      )
    `);

    await queryRunner.query(`
      CREATE TABLE "app_settings" (
        "id" varchar PRIMARY KEY NOT NULL,
        "key" varchar(100) NOT NULL,
        "value" text NOT NULL,
        "created_at" datetime NOT NULL DEFAULT (datetime('now')),
        "updated_at" datetime NOT NULL DEFAULT (datetime('now')),
        CONSTRAINT "UQ_app_settings_key" UNIQUE ("key")
      )
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`DROP TABLE IF EXISTS "app_settings"`);
    await queryRunner.query(`DROP TABLE IF EXISTS "pane_layout_configs"`);
    await queryRunner.query(`DROP TABLE IF EXISTS "kanban_tasks"`);
    await queryRunner.query(`DROP TABLE IF EXISTS "projects"`);
  }
}
