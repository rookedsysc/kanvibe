import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists, dropColumnIfExists } from "./sqliteMigrationUtils";

/**
 * 카드를 손으로 배치하는 기능을 걷어내면서 display_order 컬럼을 떨어뜨린다.
 *
 * 보드 순서는 이제 사용자가 고른 정렬 기준으로만 정해지고, 기준이 없으면 최근 수정순을 기본으로 쓴다.
 * 저장해 둘 자리가 없으므로 컬럼과 그 컬럼을 가리키던 인덱스를 함께 정리하고,
 * 새 기본 순서가 임시 B-tree로 떨어지지 않도록 status + updated_at 인덱스를 다시 만든다.
 */
export class DropDisplayOrderFromKanbanTasks1771700000000 implements MigrationInterface {
  name = "DropDisplayOrderFromKanbanTasks1771700000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query(`DROP INDEX IF EXISTS "idx_kanban_tasks_status_order"`);
    await dropColumnIfExists(queryRunner, "kanban_tasks", "display_order");
    await queryRunner.query(`
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, updated_at)
    `);
  }

  /** 되돌릴 때는 컬럼만 되살린다. 사용자가 잡아 둔 순서는 이미 사라졌으므로 복원할 값이 없다 */
  public async down(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "kanban_tasks", "display_order", `"display_order" INTEGER NOT NULL DEFAULT 0`);

    await queryRunner.query(`DROP INDEX IF EXISTS "idx_kanban_tasks_status_order"`);
    await queryRunner.query(`
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, display_order, created_at)
    `);
  }
}
