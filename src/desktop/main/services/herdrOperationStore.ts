import type { EntityManager } from "typeorm";
import { getDataSource } from "@/lib/database";
import { SessionType, TaskStatus, type KanbanTask } from "@/entities/KanbanTask";
import {
  HerdrOperationPhase,
  type HerdrSessionOperation,
  type HerdrTaskBinding,
} from "@/desktop/shared/herdrSessions";

interface StoredHerdrOperation {
  operation_id: string;
  phase: string;
  payload: string;
  created_at: string;
  updated_at: string;
}

const supportedPhases = new Set<string>(Object.values(HerdrOperationPhase));
const UNSUPPORTED_PHASE_CODE = "OPERATION_PHASE_UNSUPPORTED";
let pendingWrite: Promise<void> = Promise.resolve();

/** SQLite의 단일 연결에서 별도 작업의 BEGIN/ROLLBACK이 서로 섞이지 않게 한다. */
function serializeWrite<T>(write: () => Promise<T>): Promise<T> {
  const queued = pendingWrite.then(write);
  pendingWrite = queued.then(() => undefined, () => undefined);
  return queued;
}

function readOperation(row: StoredHerdrOperation): HerdrSessionOperation {
  const operation = JSON.parse(row.payload) as HerdrSessionOperation;
  if (!supportedPhases.has(row.phase)) {
    return {
      ...operation,
      phase: HerdrOperationPhase.ATTENTION,
      error: {
        code: UNSUPPORTED_PHASE_CODE,
        message: "지원하지 않는 Herdr 작업 상태입니다. 앱을 업데이트한 뒤 다시 확인하세요.",
        retryable: false,
        operationId: row.operation_id,
      },
    };
  }
  return {
    ...operation,
    phase: row.phase as HerdrOperationPhase,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

function assertWritableOperation(operation: HerdrSessionOperation): void {
  if (!supportedPhases.has(operation.phase) || operation.error?.code === UNSUPPORTED_PHASE_CODE) {
    throw new Error("지원하지 않는 Herdr 작업 상태는 저장할 수 없습니다.");
  }
}

async function writeOperation(manager: EntityManager, operation: HerdrSessionOperation): Promise<void> {
  const existing: { phase: string }[] = await manager.query(
    "SELECT phase FROM herdr_session_operations WHERE operation_id = ?",
    [operation.operationId],
  );
  if (existing[0] && !supportedPhases.has(existing[0].phase)) {
    throw new Error("지원하지 않는 Herdr 작업 상태는 덮어쓸 수 없습니다.");
  }

  await manager.query(`
    INSERT INTO herdr_session_operations
      (operation_id, task_id, phase, payload, created_at, updated_at)
    VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT(operation_id) DO UPDATE SET
      phase = excluded.phase,
      payload = excluded.payload,
      updated_at = excluded.updated_at
  `, [
    operation.operationId,
    operation.plan.taskId,
    operation.phase,
    JSON.stringify(operation),
    operation.createdAt,
    operation.updatedAt,
  ]);
}

export async function getHerdrOperation(operationId: string): Promise<HerdrSessionOperation | null> {
  const dataSource = await getDataSource();
  const rows: StoredHerdrOperation[] = await dataSource.query(
    "SELECT * FROM herdr_session_operations WHERE operation_id = ?",
    [operationId],
  );
  return rows[0] ? readOperation(rows[0]) : null;
}

export async function listHerdrOperations(): Promise<HerdrSessionOperation[]> {
  const dataSource = await getDataSource();
  const rows: StoredHerdrOperation[] = await dataSource.query(
    "SELECT * FROM herdr_session_operations ORDER BY updated_at DESC, operation_id ASC",
  );
  return rows.map(readOperation);
}

export async function saveHerdrOperation(operation: HerdrSessionOperation): Promise<void> {
  assertWritableOperation(operation);
  return serializeWrite(async () => {
    const dataSource = await getDataSource();
    await dataSource.transaction((manager) => writeOperation(manager, operation));
  });
}

/** 원격 생성과는 별개로, 로컬 연결과 작업 기록만 한 트랜잭션에서 반영한다. */
export async function commitHerdrBinding(
  operation: HerdrSessionOperation,
  expectedBinding: HerdrTaskBinding,
  nextBinding: HerdrTaskBinding,
): Promise<boolean> {
  assertWritableOperation(operation);
  return serializeWrite(async () => {
    const dataSource = await getDataSource();
    return dataSource.transaction(async (manager) => {
      const result = await manager.createQueryBuilder()
        .update("kanban_tasks")
        .set({ ...nextBinding, updatedAt: () => "CURRENT_TIMESTAMP" })
        .where("id = :taskId", { taskId: operation.plan.taskId })
        .andWhere("ssh_host IS :sshHost", { sshHost: expectedBinding.sshHost })
        .andWhere("session_type = :sessionType", { sessionType: expectedBinding.sessionType })
        .andWhere("session_name = :sessionName", { sessionName: expectedBinding.sessionName })
        .andWhere("worktree_path IS :worktreePath", { worktreePath: expectedBinding.worktreePath })
        .execute();
      if (result.affected !== 1) return false;
      await writeOperation(manager, operation);
      return true;
    });
  });
}

/** 로컬 보드에 없던 Space는 태스크 생성과 작업 기록을 함께 확정한다. */
export async function commitHerdrImportedTask(
  operation: HerdrSessionOperation,
  nextBinding: HerdrTaskBinding,
): Promise<boolean> {
  assertWritableOperation(operation);
  if (!operation.plan.importTask?.title) throw new Error("가져올 태스크 제목이 필요합니다.");
  return serializeWrite(async () => {
    const dataSource = await getDataSource();
    const metadata = dataSource.entityMetadatas.find((entity) => entity.tableName === "kanban_tasks");
    if (!metadata) throw new Error("태스크 테이블의 메타데이터를 찾을 수 없습니다.");
    return dataSource.transaction(async (manager) => {
      const tasks = manager.getRepository<KanbanTask>(metadata.target);
      if (await tasks.existsBy({ id: operation.plan.taskId })) return false;
      await tasks.insert({
        id: operation.plan.taskId,
        title: operation.plan.importTask!.title,
        ...nextBinding,
        sessionType: SessionType.HERDR,
        status: TaskStatus.PROGRESS,
        projectId: null,
      });
      await writeOperation(manager, operation);
      return true;
    });
  });
}
