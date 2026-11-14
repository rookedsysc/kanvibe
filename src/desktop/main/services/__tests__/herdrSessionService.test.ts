/** @vitest-environment node */
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import type { DataSource } from "typeorm";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SessionType } from "@/entities/KanbanTask";
import {
  HerdrOperationPhase,
  HerdrSessionMode,
  type HerdrSessionOperation,
  type HerdrSessionPlan,
  type HerdrSessionResult,
} from "@/desktop/shared/herdrSessions";
import officialSnapshot from "@/lib/__tests__/fixtures/herdr/current-session-v3.json";

const { execGit } = vi.hoisted(() => ({ execGit: vi.fn() }));
vi.mock("@/lib/gitOperations", () => ({ execGit, isSSHConnectionError: () => false }));
// 테스트가 실제 Electron 터미널을 열거나 닫지 않도록 native 경계만 대체한다.
vi.mock("@/lib/terminal", () => ({ detachSession: vi.fn() }));

let directory: string;
let sessionDirectory: string;
let workspaceDirectory: string;
let snapshotFile: string;
let dataSource: DataSource;
let sourceRunning: boolean;
let taskHost: string | null;
let machineProfiles: { target: string; session: string; enabled: boolean }[];
const taskId = "44444444-4444-4444-8444-444444444444";
const sourceSession = "dotfiles-main";
const sourceWorkspaceId = "wproj1";
const operationId = "33333333-3333-4333-8333-333333333333";

function unwrap<T>(result: HerdrSessionResult<T>): T {
  if (!result.ok) throw new Error(`${result.error.code}: ${result.error.message}`);
  return result.value;
}

async function tasks() {
  return (await import("@/lib/database")).getTaskRepository();
}

function commandCount(fragment: string): number {
  return execGit.mock.calls.filter(([command]) => String(command).includes(fragment)).length;
}

async function writeSnapshot(version = 3) {
  const snapshot = JSON.parse(JSON.stringify(officialSnapshot).replaceAll("/home/test/projects/project-a", workspaceDirectory)
    .replaceAll("/home/test/projects/project-b", workspaceDirectory));
  snapshot.version = version;
  await writeFile(snapshotFile, JSON.stringify(snapshot));
}

async function createPlan(mode = HerdrSessionMode.ATTACH_EXISTING): Promise<HerdrSessionPlan> {
  const service = await import("../herdrSessionService");
  const scan = unwrap(await service.scanSessions({ sshHost: taskHost }));
  return unwrap(await service.createPlan({ scanId: scan.scanId, taskId, sourceSession, sourceWorkspaceId, mode }));
}

beforeEach(async () => {
  directory = await mkdtemp(path.join(os.tmpdir(), "kanvibe-herdr-service-"));
  sessionDirectory = path.join(directory, "sessions", sourceSession);
  workspaceDirectory = path.join(directory, "dotfiles");
  snapshotFile = path.join(sessionDirectory, "session.json");
  await Promise.all([mkdir(sessionDirectory, { recursive: true }), mkdir(workspaceDirectory)]);
  await writeSnapshot();
  sourceRunning = true;
  taskHost = null;
  machineProfiles = [];
  vi.stubEnv("KANVIBE_DESKTOP", "");
  vi.stubEnv("KANVIBE_APP_DATA_DIR", directory);
  vi.stubEnv("KANVIBE_DB_PATH", path.join(directory, "kanvibe.db"));
  vi.resetModules();
  execGit.mockReset();
  execGit.mockImplementation(async (command: string) => {
    if (command.includes("'session' 'list'")) return JSON.stringify({ sessions: [{
      name: sourceSession, running: sourceRunning, session_dir: sessionDirectory, socket_path: path.join(sessionDirectory, "herdr.sock"),
    }] });
    if (command.includes("'machine' 'list'")) return JSON.stringify(machineProfiles);
    if (command.includes("'machine' 'add'")) return "";
    if (command.includes("'workspace' 'list'")) return JSON.stringify({ result: { workspaces: [{
      workspace_id: sourceWorkspaceId, cwd: workspaceDirectory, label: "dotfiles", focused: true,
    }] } });
    if (command.includes("'tab' 'list'")) return JSON.stringify({ result: { tabs: [
      { tab_id: "wproj1:t1", number: 1, label: "separate-pane", focused: true },
      { tab_id: "wproj1:t2", number: 2, label: "p", focused: false },
    ] } });
    if (command.includes("test -f") && command.includes("cat") && command.includes(sessionDirectory)) return readFile(snapshotFile, "utf8");
    if (command.includes("test -d") && command.includes(workspaceDirectory)) return "1";
    throw new Error(`허용하지 않은 외부 명령: ${command}`);
  });
  dataSource = await (await import("@/lib/database")).getDataSource();
  await (await tasks()).save({ id: taskId, title: "도트파일", sessionType: SessionType.HERDR, sessionName: sourceSession,
    worktreePath: workspaceDirectory, sshHost: null });
});

afterEach(async () => {
  if (dataSource?.isInitialized) await dataSource.destroy();
  vi.unstubAllEnvs();
  await rm(directory, { recursive: true, force: true });
});

describe("Herdr 세션 전환 보존 계약", () => {
  it("scan은 원본 서버를 시작하거나 태스크 연결을 바꾸지 않는다", async () => {
    sourceRunning = false;
    const before = await (await tasks()).findOneBy({ id: taskId });
    const service = await import("../herdrSessionService");

    const scan = unwrap(await service.scanSessions({ sshHost: null }));

    expect(scan.sessions).toEqual(expect.arrayContaining([expect.objectContaining({ name: sourceSession, running: false, snapshotState: "valid" })]));
    expect(scan.tasks).toEqual(expect.arrayContaining([expect.objectContaining({ id: taskId })]));
    expect(await (await tasks()).findOneBy({ id: taskId })).toEqual(before);
    expect(commandCount("'server'")).toBe(0);
    expect(commandCount("'session' 'start'")).toBe(0);
  });

  it("실행 중인 원본은 재구성할 수 없고 차단 사유를 계획에 보여준다", async () => {
    const plan = await createPlan(HerdrSessionMode.REBUILD_STOPPED);
    expect(plan).toMatchObject({ eligibility: "blocked" });
    expect(plan.blockers.join(" ")).toContain("SOURCE_RUNNING");
  });

  it("지원하지 않는 snapshot은 재구성 계획에서 차단한다", async () => {
    sourceRunning = false;
    await writeSnapshot(999);
    const plan = await createPlan(HerdrSessionMode.REBUILD_STOPPED);
    expect(plan.eligibility).toBe("blocked");
    expect(plan.blockers.length).toBeGreaterThan(0);
  });

  it("로컬 기존 Space 연결은 원본 서버와 Space를 보존한다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    expect(plan.eligibility).toBe("ready");

    const operation = unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));

    expect(operation.phase).toBe(HerdrOperationPhase.COMPLETED);
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sessionName: "herdr:dotfiles-main:wproj1" });
    expect(commandCount("'machine' 'add'")).toBe(0);
    expect(commandCount("'workspace' 'close'")).toBe(0);
    expect(commandCount("'session' 'delete'")).toBe(0);
  });

  it("원격 기존 Space 연결은 명시한 서버 프로필을 로컬 machines에 등록한다", async () => {
    taskHost = "dev-vm";
    await (await tasks()).update(taskId, { sshHost: taskHost });
    const service = await import("../herdrSessionService");
    const plan = await createPlan();

    const operation = unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));

    expect(operation.phase).toBe(HerdrOperationPhase.COMPLETED);
    expect(execGit.mock.calls).toEqual(expect.arrayContaining([
      [expect.stringContaining("'machine' 'add' 'dev-vm' '--remote-session' 'dotfiles-main'"), null, expect.anything()],
    ]));
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sshHost: "dev-vm", sessionName: "herdr:dotfiles-main:wproj1" });
  });

  it("로컬 태스크가 없는 원격 Space는 실행할 때 한 번만 가져온다", async () => {
    taskHost = "dev-vm";
    const repository = await tasks();
    await repository.delete(taskId);
    const service = await import("../herdrSessionService");

    const scan = unwrap(await service.scanSessions({ sshHost: taskHost }));
    expect(scan.tasks).toEqual([]);
    const plan = unwrap(await service.createPlan({ scanId: scan.scanId, taskId: "__import__", sourceSession, sourceWorkspaceId,
      mode: HerdrSessionMode.ATTACH_EXISTING }));
    expect(plan).toMatchObject({ eligibility: "ready", importTask: { title: "dotfiles" } });
    expect(await repository.count()).toBe(0);
    expect(unwrap(await service.listOperations())).toEqual([]);

    const request = { planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId };
    const imported = unwrap(await service.executePlan(request));
    const retried = unwrap(await service.executePlan(request));

    expect(imported.phase).toBe(HerdrOperationPhase.COMPLETED);
    expect(retried).toEqual(imported);
    expect(await repository.count()).toBe(1);
    expect(await repository.findOneBy({ id: plan.taskId })).toMatchObject({ title: "dotfiles", sshHost: "dev-vm",
      sessionType: SessionType.HERDR, sessionName: "herdr:dotfiles-main:wproj1", status: "progress", projectId: null,
      worktreePath: workspaceDirectory });
    expect(unwrap(await service.listOperations())).toHaveLength(1);
    expect(commandCount("'machine' 'add'")).toBe(1);
    expect(commandCount("'workspace' 'close'")).toBe(0);
  });

  it("기존 비활성 machine을 자동 추가하거나 활성화하지 않고 연결을 보존한다", async () => {
    taskHost = "dev-vm";
    machineProfiles = [{ target: "dev-vm", session: sourceSession, enabled: false }];
    await (await tasks()).update(taskId, { sshHost: taskHost });
    const service = await import("../herdrSessionService");
    const plan = await createPlan();

    const failed = unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));

    expect(failed.phase).toBe(HerdrOperationPhase.FAILED);
    expect(failed.bindingCommitted).not.toBe(true);
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sshHost: taskHost, sessionName: sourceSession });
    expect(commandCount("'machine' 'add'")).toBe(0);
    expect(commandCount("'machine' 'enable'")).toBe(0);
    expect(commandCount("'workspace' 'close'")).toBe(0);
    expect(commandCount("'session' 'delete'")).toBe(0);
  });

  it.each([{ sshHost: "other-host" }, { sessionType: SessionType.TMUX }])("호스트 또는 종류가 다른 태스크는 연결하지 않는다: %j", async (change) => {
    const service = await import("../herdrSessionService");
    const scan = unwrap(await service.scanSessions({ sshHost: null }));
    await (await tasks()).update(taskId, change);
    const result = await service.createPlan({ scanId: scan.scanId, taskId, sourceSession, sourceWorkspaceId, mode: HerdrSessionMode.ATTACH_EXISTING });
    expect(result.ok ? result.value.eligibility === "blocked" : true).toBe(true);
  });

  it("같은 operationId의 동시 요청은 연결 작업을 한 번만 실행한다", async () => {
    taskHost = "dev-vm";
    await (await tasks()).update(taskId, { sshHost: taskHost });
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    const request = { planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId };

    const [first, second] = await Promise.all([service.executePlan(request), service.executePlan(request)]);

    expect(unwrap(first)).toEqual(unwrap(second));
    expect(commandCount("'machine' 'add'")).toBe(1);
    expect(unwrap(await service.listOperations())).toHaveLength(1);
  });

  it("기존 operationId에 다른 계획을 입력하면 충돌로 거부한다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));

    const result = await service.executePlan({ planId: "another-plan", expectedFingerprint: plan.fingerprint, operationId });

    expect(result).toMatchObject({ ok: false, error: { code: "OPERATION_CONFLICT" } });
  });

  it("사용자가 본 fingerprint가 다르면 실행하지 않는다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    const result = await service.executePlan({ planId: plan.planId, expectedFingerprint: "stale-fingerprint", operationId });
    expect(result).toMatchObject({ ok: false, error: { code: "SOURCE_CHANGED" } });
    expect(commandCount("'machine' 'add'")).toBe(0);
  });

  it("계획을 본 뒤 snapshot이 바뀌면 원격 변경 전에 중단한다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    await writeSnapshot(999);

    const result = await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId });

    expect(result).toMatchObject({ ok: false, error: { code: "SOURCE_CHANGED" } });
    expect(commandCount("'machine' 'add'")).toBe(0);
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sessionName: sourceSession });
  });

  it("다른 프로젝트의 cwd는 같은 호스트여도 연결하지 않는다", async () => {
    await (await tasks()).update(taskId, { worktreePath: path.join(directory, "another-project") });

    const plan = await createPlan();

    expect(plan.eligibility).toBe("blocked");
    expect(plan.blockers.length).toBeGreaterThan(0);
  });

  it("재구성 실행 직전에 원본 서버 실행 상태를 다시 확인한다", async () => {
    sourceRunning = false;
    const service = await import("../herdrSessionService");
    const plan = await createPlan(HerdrSessionMode.REBUILD_STOPPED);
    expect(plan.eligibility).toBe("ready");
    sourceRunning = true;

    const result = await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId });

    expect(result.ok).toBe(false);
    expect(commandCount("'workspace' 'create'")).toBe(0);
    expect(commandCount("'server'")).toBe(0);
  });

  it("프로세스 중단 후 남은 진행 상태는 다시 실행하지 않고 attention으로 조회한다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    const pending: HerdrSessionOperation = { operationId, plan, phase: HerdrOperationPhase.APPLYING,
      targetWorkspaceId: null, backupLocation: null, error: null,
      createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() };
    await (await import("../herdrOperationStore")).saveHerdrOperation(pending);

    expect(unwrap(await service.getOperation({ operationId })).phase).toBe(HerdrOperationPhase.ATTENTION);
    expect(unwrap(await service.listOperations())).toEqual(expect.arrayContaining([expect.objectContaining({ phase: HerdrOperationPhase.ATTENTION })]));
    expect(commandCount("'workspace' 'create'")).toBe(0);
  });

  it("대상 서버 준비 중 snapshot이 바뀌면 Space 생성 전에 멈추고 백업만 남긴다", async () => {
    sourceRunning = false;
    const service = await import("../herdrSessionService");
    const plan = await createPlan(HerdrSessionMode.REBUILD_STOPPED);
    const original = execGit.getMockImplementation()!;
    execGit.mockImplementation(async (command: string, ...args: unknown[]) => {
      if (command.includes("--session 'kanvibe'") && command.includes("'workspace' 'list'")) {
        await writeSnapshot(999);
        return JSON.stringify({ result: { workspaces: [] } });
      }
      return original(command, ...args);
    });

    const operation = unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));

    expect(operation).toMatchObject({ phase: HerdrOperationPhase.FAILED, error: { code: "SOURCE_CHANGED" }, targetWorkspaceId: null });
    expect(operation.backupLocation).not.toBeNull();
    expect(JSON.parse(await readFile(operation.backupLocation!, "utf8")).version).toBe(3);
    expect(commandCount("'workspace' 'create'")).toBe(0);
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sessionName: sourceSession });
  });

  it("연결 되돌리기는 다른 창에서 바뀐 태스크 연결을 덮어쓰지 않는다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));
    await (await tasks()).update(taskId, { sessionName: "manually-selected-session" });

    const result = await service.controlOperation({ operationId, action: "revertBinding" });

    expect(result.ok).toBe(false);
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sessionName: "manually-selected-session" });
    expect(commandCount("'workspace' 'close'")).toBe(0);
  });

  it("연결을 되돌려도 원본과 등록된 Space는 삭제하지 않는다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));

    const reverted = unwrap(await service.controlOperation({ operationId, action: "revertBinding" }));

    expect(reverted.phase).toBe(HerdrOperationPhase.REVERTED);
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sessionName: sourceSession });
    expect(commandCount("'workspace' 'close'")).toBe(0);
    expect(commandCount("'session' 'stop'")).toBe(0);
    expect(commandCount("'session' 'delete'")).toBe(0);
  });

  it("연결 commit 전의 attention 기록으로 태스크를 되돌리지 않는다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    const attention: HerdrSessionOperation = {
      operationId, plan, phase: HerdrOperationPhase.ATTENTION, bindingCommitted: false,
      targetWorkspaceId: sourceWorkspaceId, backupLocation: null, error: null,
      createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(),
    };
    await (await import("../herdrOperationStore")).saveHerdrOperation(attention);
    await (await tasks()).update(taskId, { sessionName: "herdr:dotfiles-main:wproj1" });

    const result = await service.controlOperation({ operationId, action: "revertBinding" });

    expect(result).toMatchObject({ ok: false, error: { code: "REVERT_BLOCKED" } });
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sessionName: "herdr:dotfiles-main:wproj1" });
  });

  it("이전 서버가 중단되면 자동 시작하지 않고 현재 연결을 유지한다", async () => {
    const service = await import("../herdrSessionService");
    const plan = await createPlan();
    unwrap(await service.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId }));
    sourceRunning = false;

    const result = await service.controlOperation({ operationId, action: "revertBinding" });

    expect(result).toMatchObject({ ok: false, error: { code: "REVERT_SOURCE_STOPPED" } });
    expect(await (await tasks()).findOneBy({ id: taskId })).toMatchObject({ sessionName: "herdr:dotfiles-main:wproj1" });
    expect(commandCount("'server'")).toBe(0);
    expect(commandCount("'session' 'start'")).toBe(0);
  });
});
