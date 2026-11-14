import type { MigrationInterface, QueryRunner } from "typeorm";

export class AddHerdrSessionOperations1772000000000 implements MigrationInterface {
  name = "AddHerdrSessionOperations1772000000000";

  async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`
      CREATE TABLE IF NOT EXISTS "herdr_session_operations" (
        "operation_id" VARCHAR(36) PRIMARY KEY NOT NULL,
        "task_id" VARCHAR(36) NOT NULL,
        "phase" VARCHAR(32) NOT NULL,
        "payload" TEXT NOT NULL,
        "created_at" VARCHAR(30) NOT NULL,
        "updated_at" VARCHAR(30) NOT NULL
      )
    `);
    await queryRunner.query(`
      CREATE INDEX IF NOT EXISTS "idx_herdr_session_operations_updated"
      ON "herdr_session_operations" ("updated_at")
    `);
  }

  async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query('DROP TABLE IF EXISTS "herdr_session_operations"');
  }
}
