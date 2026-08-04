import { MigrationInterface, QueryRunner } from "typeorm";
import path from "path";
import { resolveUniqueProjectName } from "../lib/projectName";

/** PRAGMA index_list 결과 행 타입 */
interface IndexListEntry {
  name: string;
  unique: number;
}

/** PRAGMA index_info 결과 행 타입 */
interface IndexInfoEntry {
  name: string;
}

/** 이름을 다시 계산하는 데 필요한 projects 행 타입 */
interface ProjectNameRow {
  id: string;
  name: string;
  repo_path: string;
  ssh_host: string | null;
}

const PROJECT_COLUMNS = "id, name, repo_path, default_branch, ssh_host, is_worktree, color, icon_data_url, created_at";

/** name 컬럼 하나만 덮는 UNIQUE 인덱스를 찾는다. 인라인 UNIQUE 제약은 이름 없는 autoindex로 잡히므로 구성 컬럼으로 판별한다 */
async function findUniqueNameIndex(queryRunner: QueryRunner): Promise<string | null> {
  const indexes: IndexListEntry[] = await queryRunner.query(`PRAGMA index_list("projects")`);

  for (const index of indexes) {
    if (!index.unique) {
      continue;
    }

    const columns: IndexInfoEntry[] = await queryRunner.query(`PRAGMA index_info("${index.name}")`);
    if (columns.length === 1 && columns[0]?.name === "name") {
      return index.name;
    }
  }

  return null;
}

/**
 * projects 테이블을 지정한 name 컬럼 정의로 다시 만든다.
 * SQLite는 인라인 UNIQUE 제약을 DROP INDEX로 제거할 수 없어 테이블을 재생성해야 한다.
 * 마이그레이션 실행 중에는 TypeORM이 foreign_keys를 꺼두므로 DROP TABLE이 kanban_tasks의 project_id를 지우지 않는다.
 */
async function rebuildProjectsTable(queryRunner: QueryRunner, nameColumnDefinition: string): Promise<void> {
  await queryRunner.query(`
    CREATE TABLE "projects_rebuilt" (
      id TEXT PRIMARY KEY NOT NULL,
      ${nameColumnDefinition},
      repo_path TEXT NOT NULL,
      default_branch TEXT NOT NULL DEFAULT 'main',
      ssh_host TEXT,
      is_worktree INTEGER NOT NULL DEFAULT 0,
      color TEXT DEFAULT NULL,
      icon_data_url TEXT DEFAULT NULL,
      created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
    )
  `);
  await queryRunner.query(
    `INSERT INTO "projects_rebuilt" (${PROJECT_COLUMNS}) SELECT ${PROJECT_COLUMNS} FROM "projects"`,
  );
  await queryRunner.query(`DROP TABLE "projects"`);
  await queryRunner.query(`ALTER TABLE "projects_rebuilt" RENAME TO "projects"`);
}

/**
 * 전역 유일 규칙으로 지어진 기존 이름을 같은 PC 기준으로 다시 계산한다.
 * 다른 PC 때문에 붙었던 상위 폴더와 숫자 접미사가 이 단계에서 사라진다.
 * 프로젝트 이름은 등록 시 경로에서만 만들어지고 사용자가 바꿀 수 없으므로, 다시 계산해도 사용자가 지은 이름을 잃지 않는다.
 * 등록 순서를 유지해야 먼저 등록한 프로젝트가 계속 짧은 이름을 갖는다.
 */
async function rescopeProjectNamesPerHost(queryRunner: QueryRunner): Promise<void> {
  const projects: ProjectNameRow[] = await queryRunner.query(
    `SELECT id, name, repo_path, ssh_host FROM "projects" ORDER BY created_at, id`,
  );
  const namesByHost = new Map<string, Set<string>>();

  for (const project of projects) {
    const hostKey = project.ssh_host ?? "";
    const namesOnSameHost = namesByHost.get(hostKey) ?? new Set<string>();
    namesByHost.set(hostKey, namesOnSameHost);

    const rescopedName = resolveUniqueProjectName(
      path.basename(project.repo_path),
      project.repo_path,
      namesOnSameHost,
    );

    /** 이미 등록된 프로젝트는 구분할 이름이 없다고 지울 수 없으므로 기존 이름을 그대로 둔다 */
    if (!rescopedName) {
      namesOnSameHost.add(project.name);
      continue;
    }

    if (rescopedName !== project.name) {
      await queryRunner.query(`UPDATE "projects" SET name = ? WHERE id = ?`, [rescopedName, project.id]);
    }
  }
}

/**
 * 프로젝트 표시 이름을 전역 유일에서 PC(sshHost) 단위 유일로 바꾼다.
 * name의 UNIQUE 제약을 제거하고, 그 제약 때문에 상위 폴더나 숫자가 붙었던 기존 이름을 다시 계산한다.
 * 이미 UNIQUE가 없는 DB(sqliteSchema.ts로 생성된 경우)는 테이블 재생성을 건너뛴다.
 */
export class RescopeProjectNamesPerHost1771600000000 implements MigrationInterface {
  name = "RescopeProjectNamesPerHost1771600000000";

  public async up(queryRunner: QueryRunner): Promise<void> {
    if (await findUniqueNameIndex(queryRunner)) {
      await rebuildProjectsTable(queryRunner, "name TEXT NOT NULL");
    }

    await rescopeProjectNamesPerHost(queryRunner);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    if (await findUniqueNameIndex(queryRunner)) {
      return;
    }

    await rebuildProjectsTable(queryRunner, "name TEXT NOT NULL UNIQUE");
  }
}
