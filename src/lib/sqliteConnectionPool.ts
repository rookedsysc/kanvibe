import Database from "better-sqlite3";
import { existsSync } from "fs";

/** dbPath → Database 인스턴스를 보관하는 모듈 레벨 커넥션 풀 */
const connectionPool = new Map<string, Database.Database>();

/**
 * 주어진 SQLite DB 파일에 대한 읽기 전용 커넥션을 반환한다.
 * 동일 경로에 대한 커넥션은 재사용되어 프로세스 생애 동안 유지된다.
 * @param dbPath SQLite DB 파일 경로
 * @returns 읽기 전용 Database 인스턴스, 파일이 없으면 null
 */
export function getSqliteConnection(dbPath: string): Database.Database | null {
  if (!existsSync(dbPath)) return null;

  const existing = connectionPool.get(dbPath);
  if (existing) return existing;

  const db = new Database(dbPath, { readonly: true });
  connectionPool.set(dbPath, db);
  return db;
}

/**
 * SQL 쿼리를 실행하고 결과 행 배열을 반환한다.
 * @param db Database 인스턴스
 * @param sql 실행할 SQL 쿼리
 */
export function querySqlite<T>(db: Database.Database, sql: string): T[] {
  return db.prepare(sql).all() as T[];
}
