/** @vitest-environment node */
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import type { DataSource } from "typeorm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";
import {
  HerdrOperationPhase,
  HerdrSessionMode,
  type HerdrSessionOperation,
  type HerdrTaskBinding,
} from "@/desktop/shared/herdrSessions";

let directory: string;
let dataSource: DataSource;
const previousBinding: HerdrTaskBinding = {
  sshHost: null,
  sessionType: "herdr",
  sessionName: "dotfiles-main",
  worktreePath: "/workspace/dotfiles",
};
const nextBinding: HerdrTaskBinding = {
  ...previousBinding,
  sessionName: "kanvibe:dotfiles-main",
};

function operation(phase = HerdrOperationPhase.APPLYING): HerdrSessionOperation {
  return {
    operationId: "33333333-3333-4333-8333-333333333333",
    phase,
    plan: {
      planId: "22222222-2222-4222-8222-222222222222",
      fingerprint: "a".repeat(64),
      taskId: "44444444-4444-4444-8444-444444444444",
      mode: HerdrSessionMode.REBUILD_STOPPED,
      source: { sshHost: null, sessionName: "dotfiles-main", workspaceId: "w1" },
      targetSession: "kanvibe",
      eligibility: "ready",
      previousBinding,
      preserved: ["작업 파일"],
      recreated: ["배치"],
      unsupported: ["원래 PID"],
      blockers: [],
    },
    targetWorkspaceId: "w9",
    backupLocation: "/workspace/backups/session.json",
    error: null,
    createdAt: "2026-11-13T00:00:00Z",
    updatedAt: "2026-11-13T00:00:00Z",
  };
}

async function createTask(binding = previousBinding) {
  const { getTaskRepository } = await import("@/lib/database");
  const repository = await getTaskRepository();
  await repository.save({
    id: operation().plan.taskId,
    title: "dotfiles",
    ...binding,
    sessionType: SessionType.HERDR,
  });
  return repository;
}

beforeEach(async () => {
  directory = await mkdtemp(path.join(os.tmpdir(), "kanvibe-herdr-store-"));
  vi.stubEnv("KANVIBE_DESKTOP", "");
  vi.stubEnv("KANVIBE_APP_DATA_DIR", directory);
  vi.stubEnv("KANVIBE_DB_PATH", path.join(directory, "kanvibe.db"));
  vi.resetModules();
  const database = await import("@/lib/database");
  dataSource = await database.getDataSource();
});

afterEach(async () => {
  if (dataSource?.isInitialized) await dataSource.destroy();
  vi.unstubAllEnvs();
  await rm(directory, { recursive: true, force: true });
});

describe("Herdr 작업 기록 저장", () => {
  it("앱의 데이터 소스를 다시 열어도 저장한 작업을 조회한다", async () => {
    const store = await import("../herdrOperationStore");
    const saved = operation();
    await store.saveHerdrOperation(saved);
    await dataSource.destroy();

    dataSource = await (await import("@/lib/database")).getDataSource();

    expect(await store.getHerdrOperation(saved.operationId)).toEqual(saved);
    expect(await store.listHerdrOperations()).toEqual([saved]);
    expect(await store.getHerdrOperation("missing")).toBeNull();
  });

  it("예상 연결이 일치하면 태스크와 committed 기록을 함께 저장한다", async () => {
    const store = await import("../herdrOperationStore");
    const tasks = await createTask();
    const committed = operation(HerdrOperationPhase.COMMITTED);

    expect(await store.commitHerdrBinding(committed, previousBinding, nextBinding)).toBe(true);

    expect(await tasks.findOneBy({ id: committed.plan.taskId })).toMatchObject(nextBinding);
    expect(await store.getHerdrOperation(committed.operationId)).toEqual(committed);
  });

  it("기존 태스크 없이 가져온 Space의 진행 태스크와 작업 기록을 함께 만든다", async () => {
    const store = await import("../herdrOperationStore");
    const imported = operation(HerdrOperationPhase.COMMITTED);
    imported.plan = { ...imported.plan, importTask: { title: "원격 도트파일" } };

    expect(await store.commitHerdrImportedTask(imported, nextBinding)).toBe(true);

    expect(await (await import("@/lib/database")).getTaskRepository().then((tasks) => tasks.findOneBy({ id: imported.plan.taskId })))
      .toMatchObject({ id: imported.plan.taskId, title: "원격 도트파일", status: "progress", projectId: null, ...nextBinding });
    expect(await store.getHerdrOperation(imported.operationId)).toEqual(imported);
  });

  it("가져온 Space의 작업 기록 저장이 실패하면 새 태스크 생성도 롤백한다", async () => {
    const store = await import("../herdrOperationStore");
    const imported = operation(HerdrOperationPhase.COMMITTED);
    imported.plan = { ...imported.plan, importTask: { title: "원격 도트파일" } };
    await dataSource.query(`
      CREATE TRIGGER reject_herdr_import BEFORE INSERT ON herdr_session_operations
      BEGIN SELECT RAISE(ABORT, 'import operation rejected'); END
    `);

    await expect(store.commitHerdrImportedTask(imported, nextBinding)).rejects.toThrow("import operation rejected");

    expect(await (await import("@/lib/database")).getTaskRepository().then((tasks) => tasks.findOneBy({ id: imported.plan.taskId }))).toBeNull();
    expect(await store.getHerdrOperation(imported.operationId)).toBeNull();
  });

  it("가져오기용 태스크 ID가 이미 있으면 태스크와 작업 기록을 덮어쓰지 않는다", async () => {
    const store = await import("../herdrOperationStore");
    const tasks = await createTask();
    const saved = operation();
    await store.saveHerdrOperation(saved);
    const before = await tasks.findOneBy({ id: saved.plan.taskId });
    const imported = operation(HerdrOperationPhase.COMMITTED);
    imported.plan = { ...imported.plan, importTask: { title: "덮어쓰면 안 되는 제목" } };

    expect(await store.commitHerdrImportedTask(imported, nextBinding)).toBe(false);

    expect(await tasks.findOneBy({ id: saved.plan.taskId })).toEqual(before);
    expect(await store.getHerdrOperation(saved.operationId)).toEqual(saved);
  });

  it.each([
    { sshHost: "changed-host" },
    { sessionType: SessionType.TMUX },
    { sessionName: "another-session" },
    { worktreePath: "/workspace/changed" },
  ])("다른 창이 연결을 바꾸면 태스크와 작업 기록을 덮어쓰지 않는다: %j", async (changed) => {
    const store = await import("../herdrOperationStore");
    const tasks = await createTask();
    const saved = operation();
    await store.saveHerdrOperation(saved);
    await tasks.update(saved.plan.taskId, changed);
    const currentTask = await tasks.findOneBy({ id: saved.plan.taskId });

    expect(await store.commitHerdrBinding(operation(HerdrOperationPhase.COMMITTED), previousBinding, nextBinding)).toBe(false);

    expect(await tasks.findOneBy({ id: saved.plan.taskId })).toEqual(currentTask);
    expect(await store.getHerdrOperation(saved.operationId)).toEqual(saved);
  });

  it("작업 기록 저장에 실패하면 태스크 연결 갱신도 롤백한다", async () => {
    const store = await import("../herdrOperationStore");
    const tasks = await createTask();
    await dataSource.query(`
      CREATE TRIGGER reject_herdr_operation BEFORE INSERT ON herdr_session_operations
      BEGIN SELECT RAISE(ABORT, 'operation write rejected'); END
    `);

    await expect(store.commitHerdrBinding(operation(HerdrOperationPhase.COMMITTED), previousBinding, nextBinding))
      .rejects.toThrow("operation write rejected");

    expect(await tasks.findOneBy({ id: operation().plan.taskId })).toMatchObject(previousBinding);
    expect(await store.getHerdrOperation(operation().operationId)).toBeNull();
  });

  it("동시 저장과 바인딩 반영은 SQLite 트랜잭션을 중첩하지 않는다", async () => {
    const store = await import("../herdrOperationStore");
    const tasks = await createTask();
    const saved = operation();
    const committed = { ...operation(HerdrOperationPhase.COMMITTED), operationId: "55555555-5555-4555-8555-555555555555" };

    const [, bound] = await Promise.all([
      store.saveHerdrOperation(saved),
      store.commitHerdrBinding(committed, previousBinding, nextBinding),
    ]);

    expect(bound).toBe(true);
    expect(await store.getHerdrOperation(saved.operationId)).toEqual(saved);
    expect(await store.getHerdrOperation(committed.operationId)).toEqual(committed);
    expect(await tasks.findOneBy({ id: saved.plan.taskId })).toMatchObject(nextBinding);
  });

  it("실패한 저장 뒤에도 대기 중인 다른 작업을 실행한다", async () => {
    const store = await import("../herdrOperationStore");
    const saved = operation();
    await dataSource.query(`
      CREATE TRIGGER reject_first_herdr_operation BEFORE INSERT ON herdr_session_operations
      WHEN NEW.operation_id = '${saved.operationId}'
      BEGIN SELECT RAISE(ABORT, 'first operation rejected'); END
    `);
    const next = { ...saved, operationId: "55555555-5555-4555-8555-555555555555" };

    const results = await Promise.allSettled([store.saveHerdrOperation(saved), store.saveHerdrOperation(next)]);

    expect(results[0]).toMatchObject({ status: "rejected" });
    expect(results[1]).toEqual({ status: "fulfilled", value: undefined });
    expect(await store.getHerdrOperation(saved.operationId)).toBeNull();
    expect(await store.getHerdrOperation(next.operationId)).toEqual(next);
  });

  it("미지 상태는 읽기 전용 attention으로 표시하고 저장 원문을 보존한다", async () => {
    const store = await import("../herdrOperationStore");
    const saved = operation();
    await store.saveHerdrOperation(saved);
    await dataSource.query("UPDATE herdr_session_operations SET phase = ? WHERE operation_id = ?", ["future-phase", saved.operationId]);
    const before = await dataSource.query("SELECT phase, payload FROM herdr_session_operations");

    const read = await store.getHerdrOperation(saved.operationId);

    expect(read).toMatchObject({
      phase: HerdrOperationPhase.ATTENTION,
      error: { code: "OPERATION_PHASE_UNSUPPORTED", retryable: false },
    });
    expect(await store.listHerdrOperations()).toEqual([read]);
    await expect(store.saveHerdrOperation(read!)).rejects.toThrow("지원하지 않는");
    expect(await dataSource.query("SELECT phase, payload FROM herdr_session_operations")).toEqual(before);
  });

  it("미지 상태를 신규 작업으로 쓰지 않는다", async () => {
    const store = await import("../herdrOperationStore");
    const invalid = { ...operation(), phase: "future-phase" as HerdrOperationPhase };

    await expect(store.saveHerdrOperation(invalid)).rejects.toThrow("지원하지 않는");

    expect(await store.listHerdrOperations()).toEqual([]);
  });
});
