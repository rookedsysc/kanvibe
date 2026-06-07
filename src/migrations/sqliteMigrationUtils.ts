import type { QueryRunner } from "typeorm";

/** PRAGMA table_info 결과 행 타입 */
interface ColumnInfo {
  name: string;
}

/** 컬럼이 없을 때만 ADD COLUMN을 실행한다. SQLite는 ADD COLUMN IF NOT EXISTS를 지원하지 않는다. */
export async function addColumnIfNotExists(
  queryRunner: QueryRunner,
  table: string,
  column: string,
  definition: string,
): Promise<void> {
  const columns: ColumnInfo[] = await queryRunner.query(`PRAGMA table_info("${table}")`);
  if (!columns.some((c) => c.name === column)) {
    await queryRunner.query(`ALTER TABLE "${table}" ADD COLUMN ${definition}`);
  }
}

/** 컬럼이 있을 때만 DROP COLUMN을 실행한다. SQLite 3.35+에서 지원된다. */
export async function dropColumnIfExists(
  queryRunner: QueryRunner,
  table: string,
  column: string,
): Promise<void> {
  const columns: ColumnInfo[] = await queryRunner.query(`PRAGMA table_info("${table}")`);
  if (columns.some((c) => c.name === column)) {
    await queryRunner.query(`ALTER TABLE "${table}" DROP COLUMN "${column}"`);
  }
}
