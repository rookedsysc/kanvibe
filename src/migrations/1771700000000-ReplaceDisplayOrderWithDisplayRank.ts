import { MigrationInterface, QueryRunner } from "typeorm";
import { addColumnIfNotExists, dropColumnIfExists } from "./sqliteMigrationUtils";
import { buildSequentialRanks } from "../desktop/shared/displayRank";

interface TaskOrderRow {
  id: string;
  status: string;
}

/**
 * 카드 순서를 정수 display_order에서 16진 fractional rank인 display_rank로 옮긴다.
 * 정수 순번은 카드 하나를 옮길 때마다 컬럼 전체를 다시 번호 매겨야 하지만 rank는 옮긴 행만 갱신하면 된다.
 *
 * 카드 자리는 rank 하나로만 표현한다. 순서 자체는 display_order에서 그대로 보존한다.
 */
export class ReplaceDisplayOrderWithDisplayRank1771700000000 implements MigrationInterface {
  name = "ReplaceDisplayOrderWithDisplayRank1771700000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "kanban_tasks", "display_rank", `"display_rank" TEXT NOT NULL DEFAULT '8'`);

    // 새로 만든 DB에는 display_order가 아예 없다. 그때는 옮길 순서도 없으므로 백필을 건너뛴다
    if (await hasColumn(queryRunner, "kanban_tasks", "display_order")) {
      const orderedRows: TaskOrderRow[] = await queryRunner.query(`
        SELECT id, status FROM kanban_tasks
        ORDER BY status ASC, display_order ASC, created_at ASC, id ASC
      `);
      await assignRanksByStatus(queryRunner, orderedRows);
    }

    await queryRunner.query(`DROP INDEX IF EXISTS "idx_kanban_tasks_status_order"`);
    await dropColumnIfExists(queryRunner, "kanban_tasks", "display_order");
    await queryRunner.query(`
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, display_rank, created_at)
    `);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await addColumnIfNotExists(queryRunner, "kanban_tasks", "display_order", `"display_order" INTEGER NOT NULL DEFAULT 0`);

    const orderedRows: TaskOrderRow[] = await queryRunner.query(`
      SELECT id, status FROM kanban_tasks
      ORDER BY status ASC, display_rank ASC, created_at ASC, id ASC
    `);
    const positionByStatus = new Map<string, number>();
    for (const row of orderedRows) {
      const position = positionByStatus.get(row.status) ?? 0;
      positionByStatus.set(row.status, position + 1);
      await queryRunner.query(`UPDATE kanban_tasks SET display_order = ? WHERE id = ?`, [position, row.id]);
    }

    await queryRunner.query(`DROP INDEX IF EXISTS "idx_kanban_tasks_status_order"`);
    await dropColumnIfExists(queryRunner, "kanban_tasks", "display_rank");
    await queryRunner.query(`
      CREATE INDEX IF NOT EXISTS "idx_kanban_tasks_status_order"
        ON kanban_tasks(status, display_order, created_at)
    `);
  }
}

async function hasColumn(queryRunner: QueryRunner, table: string, column: string): Promise<boolean> {
  const columns: { name: string }[] = await queryRunner.query(`PRAGMA table_info("${table}")`);
  return columns.some((candidate) => candidate.name === column);
}

/** 상태별로 끊어 그 안의 순서대로 rank를 부여한다 */
async function assignRanksByStatus(queryRunner: QueryRunner, orderedRows: TaskOrderRow[]): Promise<void> {
  const idsByStatus = new Map<string, string[]>();
  for (const row of orderedRows) {
    const ids = idsByStatus.get(row.status) ?? [];
    ids.push(row.id);
    idsByStatus.set(row.status, ids);
  }

  for (const ids of idsByStatus.values()) {
    const ranks = buildSequentialRanks(ids.length);
    for (let index = 0; index < ids.length; index += 1) {
      await queryRunner.query(`UPDATE kanban_tasks SET display_rank = ? WHERE id = ?`, [ranks[index], ids[index]]);
    }
  }
}
