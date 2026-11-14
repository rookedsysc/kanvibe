import { createHash, randomUUID } from "crypto";
import path from "path";
import { SessionType } from "@/entities/KanbanTask";
import { getTaskRepository } from "@/lib/database";
import { bindHerdrWorkspace, ensureHerdrRemoteMachine, ensureHerdrServer, listHerdrSessions, parseHerdrSessionTarget, runHerdrCommand } from "@/lib/herdr";
import { decodeHerdrSnapshot, type HerdrSnapshot } from "@/lib/herdrSnapshot";
import { getHerdrWorkspaceRebuildBlockers, rebuildHerdrWorkspace } from "@/lib/herdrWorkspaceRebuild";
import { getHomeDirectory, pathExists, readTextFile, updateTextFile } from "@/lib/hostFileAccess";
import { broadcastBoardUpdate, broadcastTaskSessionChanged } from "@/lib/boardNotifier";
import {
  HerdrOperationPhase as Phase, HerdrSessionMode as Mode,
  type HerdrCreatePlanRequest, type HerdrExecutePlanRequest, type HerdrSessionOperation,
  type HerdrSessionPlan, type HerdrSessionResult, type HerdrSessionScan,
  type HerdrScannedSession, type HerdrTaskBinding,
} from "@/desktop/shared/herdrSessions";
import { commitHerdrBinding, commitHerdrImportedTask, getHerdrOperation, listHerdrOperations, saveHerdrOperation } from "./herdrOperationStore";

interface Inspection { session: HerdrScannedSession; raw: string; snapshot: HerdrSnapshot | null }
interface ScanCache { scan: HerdrSessionScan; sources: Inspection[]; expiresAt: number }
interface PreparedPlan { plan: HerdrSessionPlan; source: Inspection; expiresAt: number }
const scans = new Map<string, ScanCache>();
const plans = new Map<string, PreparedPlan>();
const inFlight = new Map<string, { request: HerdrExecutePlanRequest; promise: Promise<HerdrSessionResult<HerdrSessionOperation>> }>();
const taskLocks = new Set<string>();
const cancelled = new Set<string>();
const CACHE_TTL = 15 * 60_000;
const MAX_CACHE = 100;
const validSession = (name: string) => /^[A-Za-z0-9._-]{1,64}$/.test(name) && name !== "." && name !== "..";

class TransitionError extends Error {
  constructor(readonly code: string, message: string) { super(message); }
}
function fail<T>(error: unknown, operationId: string | null = null): HerdrSessionResult<T> {
  return { ok: false, error: { code: error instanceof TransitionError ? error.code : "HERDR_SESSION_ERROR",
    message: error instanceof Error ? error.message : String(error), retryable: false, operationId } };
}
async function result<T>(action: () => Promise<T>, operationId: string | null = null): Promise<HerdrSessionResult<T>> {
  try { return { ok: true, value: await action() }; } catch (error) { return fail(error, operationId); }
}
function assert(condition: unknown, code: string, message: string): asserts condition {
  if (!condition) throw new TransitionError(code, message);
}
function prune<T extends { expiresAt: number }>(cache: Map<string, T>) {
  for (const [key, item] of cache) if (item.expiresAt < Date.now()) cache.delete(key);
  while (cache.size >= MAX_CACHE) cache.delete(cache.keys().next().value!);
}
function binding(task: { sshHost: string | null; sessionName: string | null; worktreePath: string | null }): HerdrTaskBinding {
  return { sshHost: task.sshHost ?? null, sessionType: "herdr", sessionName: task.sessionName!, worktreePath: task.worktreePath ?? null };
}
function digest(value: unknown): string { return createHash("sha256").update(JSON.stringify(value)).digest("hex"); }
function fingerprint(source: Inspection, workspaceId: string, previous: HerdrTaskBinding): string {
  return digest({ name: source.session.name, running: source.session.running, raw: source.raw,
    workspace: source.session.workspaces.find((item) => item.workspaceId === workspaceId), previous });
}

async function inspect(sshHost: string | null): Promise<Inspection[]> {
  const sessions = await listHerdrSessions(sshHost);
  const output: Inspection[] = [];
  for (const entry of sessions) {
    if (!validSession(entry.name)) continue;
    const session: HerdrScannedSession = { name: entry.name, running: entry.running, sessionDir: entry.session_dir ?? null,
      snapshotState: "missing", workspaces: [], taskIds: [], error: null };
    let raw = "";
    let snapshot: HerdrSnapshot | null = null;
    if (entry.session_dir) {
      try {
        raw = await readTextFile(path.posix.join(entry.session_dir, "session.json"), sshHost);
        if (raw) {
          try { snapshot = decodeHerdrSnapshot(raw); session.snapshotState = "valid"; }
          catch (error) { session.snapshotState = "unsupported"; session.error = String(error); }
        }
      } catch (error) { session.snapshotState = "unreadable"; session.error = String(error); }
    }
    session.workspaces = snapshot?.workspaces.map((space) => ({ workspaceId: space.sourceWorkspaceId,
      label: space.label, cwd: space.cwd, tabCount: space.tabs.length })) ?? [];
    if (entry.running) {
      try {
        const live = await runHerdrCommand<{ workspaces: { workspace_id: string; label?: string; cwd?: string; tab_count?: number }[] }>(entry.name, ["workspace", "list"], sshHost);
        session.workspaces = await Promise.all(live.workspaces.map(async (space) => {
          assert(/^w[0-9A-Za-z]+$/.test(space.workspace_id), "INVALID_WORKSPACE", "Herdr Space 식별자가 올바르지 않습니다.");
          const persisted = snapshot?.workspaces.find((item) => item.sourceWorkspaceId === space.workspace_id);
          let cwd = persisted?.cwd ?? space.cwd ?? "";
          if (!cwd) {
            const panes = await runHerdrCommand<{ panes: { cwd?: string }[] }>(entry.name, ["pane", "list", "--workspace", space.workspace_id], sshHost);
            cwd = panes.panes[0]?.cwd ?? "";
          }
          return { workspaceId: space.workspace_id, label: space.label ?? persisted?.label ?? space.workspace_id,
            cwd, tabCount: space.tab_count ?? persisted?.tabs.length ?? 0 };
        }));
      } catch (error) {
        // 실패한 live 조회에 snapshot을 대신 보여주면 사라진 Space를 연결할 수 있다.
        session.workspaces = []; session.error = String(error);
      }
    }
    output.push({ session, raw, snapshot });
  }
  return output;
}

export async function scanSessions(request: { sshHost: string | null }): Promise<HerdrSessionResult<HerdrSessionScan>> {
  return result(async () => {
    assert(request && (request.sshHost === null || (typeof request.sshHost === "string" && /^[A-Za-z0-9_.@-]+$/.test(request.sshHost))), "INVALID_HOST", "호스트가 올바르지 않습니다.");
    const sources = await inspect(request.sshHost);
    const repository = await getTaskRepository();
    const tasks = (await repository.find()).filter((task) => task.sessionType === SessionType.HERDR && task.sessionName && (task.sshHost ?? null) === request.sshHost);
    for (const source of sources) source.session.taskIds = tasks.filter((task) => parseHerdrSessionTarget(task.sessionName!).serverName === source.session.name).map((task) => task.id);
    const scan: HerdrSessionScan = { scanId: randomUUID(), sshHost: request.sshHost, observedAt: new Date().toISOString(),
      fingerprint: digest(sources), sessions: sources.map((source) => source.session),
      tasks: tasks.map((task) => ({ id: task.id, title: task.title, sessionName: task.sessionName!, worktreePath: task.worktreePath })) };
    prune(scans);
    for (const [key, cached] of scans) if (cached.scan.sshHost === request.sshHost) scans.delete(key);
    scans.set(scan.scanId, { scan, sources, expiresAt: Date.now() + CACHE_TTL });
    return scan;
  });
}

async function normalizedCwd(cwd: string, sshHost: string | null): Promise<string> {
  if (cwd === "~" || cwd.startsWith("~/")) return path.posix.resolve(await getHomeDirectory(sshHost), cwd.slice(2));
  return cwd.startsWith("/") ? path.posix.normalize(cwd) : "";
}

export async function createPlan(request: HerdrCreatePlanRequest): Promise<HerdrSessionResult<HerdrSessionPlan>> {
  return result(async () => {
    prune(scans); prune(plans);
    const cached = scans.get(request.scanId);
    assert(cached, "SCAN_EXPIRED", "목록을 다시 조회하세요.");
    assert(Object.values(Mode).includes(request.mode), "INVALID_MODE", "전환 방식을 선택하세요.");
    const source = cached.sources.find((item) => item.session.name === request.sourceSession);
    assert(source, "SOURCE_MISSING", "원본 세션을 찾을 수 없습니다.");
    const workspace = source.session.workspaces.find((space) => space.workspaceId === request.sourceWorkspaceId);
    const importing = request.taskId === "__import__";
    const title = (workspace && workspace.label !== workspace.workspaceId ? workspace.label : path.posix.basename(workspace?.cwd ?? "") || source.session.name).slice(0, 255);
    const task = importing ? { id: randomUUID(), title, sessionType: SessionType.HERDR,
      sessionName: `herdr:${source.session.name}:${request.sourceWorkspaceId}`, sshHost: cached.scan.sshHost, worktreePath: workspace?.cwd ?? null }
      : await (await getTaskRepository()).findOneBy({ id: request.taskId });
    assert(task?.sessionType === SessionType.HERDR && task.sessionName && (task.sshHost ?? null) === cached.scan.sshHost,
      "TASK_MISMATCH", "같은 호스트의 Herdr 태스크를 선택하세요.");
    const blockers: string[] = [];
    if (!workspace) blockers.push("WORKSPACE_MISSING: 원본 Space를 확인할 수 없습니다.");
    if (workspace && (!workspace.cwd || !task.worktreePath || await normalizedCwd(workspace.cwd, cached.scan.sshHost) !== await normalizedCwd(task.worktreePath, cached.scan.sshHost))) {
      blockers.push("CWD_MISMATCH: 태스크와 원본 Space의 프로젝트 경로가 일치하지 않습니다.");
    }
    if (request.mode === Mode.ATTACH_EXISTING && !source.session.running) blockers.push("SOURCE_STOPPED: 실행 중인 세션에만 연결할 수 있습니다.");
    if (request.mode === Mode.REBUILD_STOPPED) {
      if (source.session.name === "kanvibe") blockers.push("SOURCE_IS_TARGET: 공유 kanvibe 자체는 원본으로 재구성할 수 없습니다. 먼저 기존 Space로 연결하세요.");
      if (source.session.running) blockers.push("SOURCE_RUNNING: 실행 중인 프로세스는 다른 서버로 옮길 수 없습니다.");
      if (source.session.snapshotState !== "valid") blockers.push("SNAPSHOT_UNSUPPORTED: 지원하는 snapshot이 필요합니다.");
      const savedSpace = source.snapshot?.workspaces.find((space) => space.sourceWorkspaceId === request.sourceWorkspaceId);
      if (savedSpace) {
        blockers.push(...getHerdrWorkspaceRebuildBlockers(savedSpace));
        for (const cwd of new Set(savedSpace.tabs.flatMap((tab) => tab.panes.map((pane) => pane.cwd)))) {
          if (cwd.startsWith("/") && !await pathExists(cwd, cached.scan.sshHost)) blockers.push(`CWD_MISSING: 작업 경로가 없습니다: ${cwd}`);
        }
      }
      if (parseHerdrSessionTarget(task.sessionName).workspaceKey && task.sessionName.startsWith("kanvibe:")) blockers.push("TARGET_BOUND: 태스크가 이미 공유 Space에 연결되어 있습니다.");
    }
    const previousBinding = binding(task);
    const plan: HerdrSessionPlan = { planId: randomUUID(), fingerprint: fingerprint(source, request.sourceWorkspaceId, previousBinding),
      taskId: task.id, mode: request.mode, source: { sshHost: cached.scan.sshHost, sessionName: source.session.name, workspaceId: request.sourceWorkspaceId },
      targetSession: request.mode === Mode.ATTACH_EXISTING ? source.session.name : "kanvibe", eligibility: blockers.length ? "blocked" : "ready", previousBinding,
      ...(importing ? { importTask: { title } } : {}),
      preserved: ["원본 세션과 파일", "기존 machine 설정", ...(request.mode === Mode.ATTACH_EXISTING ? ["실행 중인 프로세스와 대화"] : ["원본 snapshot 백업"])],
      recreated: request.mode === Mode.REBUILD_STOPPED ? ["Space, 탭, 분할, 비율, 이름, 작업 경로", "새 셸 프로세스"] : ["KanVibe 태스크 연결"],
      unsupported: ["실행 중인 프로세스의 서버 간 이동", "다른 PC로 파일 복사", ...(request.mode === Mode.REBUILD_STOPPED ? ["셸 메모리와 스크롤백", "에이전트와 시작 명령의 자동 실행"] : []), "외부 Herdr 창의 machine 자동 선택"], blockers };
    for (const [key, item] of plans) if (item.plan.taskId === task.id) plans.delete(key);
    plans.set(plan.planId, { plan, source, expiresAt: Date.now() + CACHE_TTL });
    return plan;
  });
}

function projectOperation(operation: HerdrSessionOperation): HerdrSessionOperation {
  if ([Phase.APPLYING, Phase.VERIFYING, Phase.COMMITTED].includes(operation.phase) && !inFlight.has(operation.operationId)) {
    return { ...operation, phase: Phase.ATTENTION, error: { code: "INTERRUPTED", message: "실행이 중단되었습니다. 원본과 생성된 Space를 확인하세요. 자동 재실행하지 않습니다.", retryable: false, operationId: operation.operationId } };
  }
  return operation;
}

function nextBinding(operation: HerdrSessionOperation): HerdrTaskBinding {
  return { ...operation.plan.previousBinding,
    sessionName: operation.plan.mode === Mode.ATTACH_EXISTING
      ? `herdr:${operation.plan.source.sessionName}:${operation.targetWorkspaceId}` : `kanvibe:${operation.plan.taskId}` };
}
async function reconnect(taskId: string) {
  const { detachSession } = await import("@/lib/terminal");
  detachSession(taskId, "herdr-session-transition");
  broadcastTaskSessionChanged({ taskId });
  broadcastBoardUpdate();
}

async function apply(request: HerdrExecutePlanRequest): Promise<HerdrSessionResult<HerdrSessionOperation>> {
  let operation: HerdrSessionOperation | null = null;
  let lockedTask: string | null = null;
  let creationAttempted = false;
  try {
    const existing = await getHerdrOperation(request.operationId);
    if (existing) {
      assert(existing.plan.planId === request.planId && existing.plan.fingerprint === request.expectedFingerprint, "OPERATION_CONFLICT", "이 작업 ID는 다른 요청에 사용되었습니다.");
      return { ok: true, value: projectOperation(existing) };
    }
    prune(plans);
    const prepared = plans.get(request.planId);
    assert(prepared, "PLAN_EXPIRED", "미리보기를 다시 만드세요.");
    const { plan } = prepared;
    assert(plan.fingerprint === request.expectedFingerprint, "SOURCE_CHANGED", "미리보기 정보가 달라졌습니다. 다시 조회하세요.");
    assert(plan.eligibility === "ready", "PLAN_BLOCKED", plan.blockers.join("\n"));
    assert(!taskLocks.has(plan.taskId), "TASK_BUSY", "이 태스크의 다른 전환이 진행 중입니다.");
    taskLocks.add(plan.taskId); lockedTask = plan.taskId;
    const task = await (await getTaskRepository()).findOneBy({ id: plan.taskId });
    const fresh = (await inspect(plan.source.sshHost)).find((item) => item.session.name === plan.source.sessionName);
    const taskUnchanged = plan.importTask ? !task : task?.sessionType === SessionType.HERDR;
    assert(taskUnchanged && fresh && fingerprint(fresh, plan.source.workspaceId, plan.importTask ? plan.previousBinding : binding(task!)) === plan.fingerprint,
      "SOURCE_CHANGED", "원본 또는 태스크 연결이 변경되었습니다. 다시 조회하세요.");
    const now = new Date().toISOString();
    operation = { operationId: request.operationId, plan, phase: Phase.APPLYING, targetWorkspaceId: null, backupLocation: null, error: null, createdAt: now, updatedAt: now };
    await saveHerdrOperation(operation);
    const persist = async () => { operation!.updatedAt = new Date().toISOString(); await saveHerdrOperation(operation!); };
    const checkCancelled = () => assert(!cancelled.has(request.operationId), "CANCELLED", "사용자가 실행을 취소했습니다.");
    checkCancelled();
    if (plan.mode === Mode.ATTACH_EXISTING) {
      if (plan.source.sshHost) await ensureHerdrRemoteMachine(plan.source.sessionName, plan.source.sshHost, { requireEnabled: true });
      operation.targetWorkspaceId = plan.source.workspaceId;
    } else {
      assert(!fresh.session.running && fresh.snapshot, "SOURCE_RUNNING", "중단된 snapshot만 재구성할 수 있습니다.");
      const savedSpace = fresh.snapshot.workspaces.find((space) => space.sourceWorkspaceId === plan.source.workspaceId)!;
      operation.backupLocation = path.posix.join(fresh.session.sessionDir!, "kanvibe-backups", `${request.operationId}.json`);
      await updateTextFile(operation.backupLocation, (content) => {
        assert(!content || content === fresh.raw, "BACKUP_CONFLICT", "기존 백업 내용이 다릅니다.");
        return fresh.raw;
      }, plan.source.sshHost, 0o600);
      assert(await readTextFile(operation.backupLocation, plan.source.sshHost) === fresh.raw, "BACKUP_FAILED", "원본 백업을 검증할 수 없습니다.");
      await persist(); checkCancelled();
      await ensureHerdrServer("kanvibe", plan.source.sshHost);
      checkCancelled();
      const sourceStatus = (await listHerdrSessions(plan.source.sshHost)).find((item) => item.name === plan.source.sessionName);
      assert(sourceStatus && !sourceStatus.running, "SOURCE_RUNNING", "원본이 다시 실행되었습니다. 재구성을 중단합니다.");
      assert(await readTextFile(path.posix.join(fresh.session.sessionDir!, "session.json"), plan.source.sshHost) === fresh.raw,
        "SOURCE_CHANGED", "원본 snapshot이 변경되었습니다. 다시 조회하세요.");
      creationAttempted = true;
      await rebuildHerdrWorkspace({ source: savedSpace, sshHost: plan.source.sshHost, operationId: request.operationId,
        onCreated: async (workspaceId) => { operation!.targetWorkspaceId = workspaceId; await persist(); }, checkCancelled });
      if (plan.source.sshHost) await ensureHerdrRemoteMachine("kanvibe", plan.source.sshHost, { requireEnabled: true });
      checkCancelled();
      const finalSource = (await listHerdrSessions(plan.source.sshHost)).find((item) => item.name === plan.source.sessionName);
      assert(finalSource && !finalSource.running && await readTextFile(path.posix.join(fresh.session.sessionDir!, "session.json"), plan.source.sshHost) === fresh.raw,
        "SOURCE_CHANGED", "재구성 중 원본이 변경되었습니다. 새 Space는 보존하고 연결 변경을 중단합니다.");
      await bindHerdrWorkspace(plan.taskId, operation.targetWorkspaceId!, plan.source.sshHost);
    }
    checkCancelled();
    operation.phase = Phase.VERIFYING; await persist();
    const live = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(plan.targetSession, ["workspace", "list"], plan.source.sshHost);
    assert(live.workspaces.some((space) => space.workspace_id === operation!.targetWorkspaceId), "TARGET_MISSING", "대상 Space를 확인할 수 없습니다.");
    checkCancelled();
    operation.phase = Phase.COMMITTED;
    operation.bindingCommitted = true;
    const committed = plan.importTask ? await commitHerdrImportedTask(operation, nextBinding(operation))
      : await commitHerdrBinding(operation, plan.previousBinding, nextBinding(operation));
    if (!committed) operation.bindingCommitted = false;
    assert(committed, "BINDING_CHANGED", "다른 창에서 연결을 변경했습니다. 생성된 Space는 보존했습니다.");
    await reconnect(plan.taskId);
    operation.phase = Phase.COMPLETED; await persist();
    plans.delete(plan.planId);
    return { ok: true, value: operation };
  } catch (error) {
    if (!operation) return fail(error, request.operationId);
    const failure = fail<never>(error, request.operationId);
    operation.phase = operation.targetWorkspaceId || creationAttempted ? Phase.ATTENTION : cancelled.has(request.operationId) ? Phase.CANCELLED : Phase.FAILED;
    operation.error = !failure.ok ? failure.error : null;
    operation.updatedAt = new Date().toISOString();
    await saveHerdrOperation(operation);
    return { ok: true, value: operation };
  } finally {
    if (lockedTask) taskLocks.delete(lockedTask);
    cancelled.delete(request.operationId);
  }
}

export async function executePlan(request: HerdrExecutePlanRequest): Promise<HerdrSessionResult<HerdrSessionOperation>> {
  if (!request || !/^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(request.operationId)) return fail(new TransitionError("INVALID_OPERATION_ID", "작업 ID가 올바르지 않습니다."));
  const pending = inFlight.get(request.operationId);
  if (pending) {
    if (pending.request.planId !== request.planId || pending.request.expectedFingerprint !== request.expectedFingerprint) return fail(new TransitionError("OPERATION_CONFLICT", "같은 작업 ID로 다른 요청을 실행할 수 없습니다."), request.operationId);
    return pending.promise;
  }
  const promise = apply(request).finally(() => inFlight.delete(request.operationId));
  inFlight.set(request.operationId, { request, promise });
  return promise;
}

export async function getOperation(request: { operationId: string }): Promise<HerdrSessionResult<HerdrSessionOperation>> {
  return result(async () => {
    const operation = await getHerdrOperation(request.operationId);
    assert(operation, "OPERATION_MISSING", "작업을 찾을 수 없습니다.");
    return projectOperation(operation);
  }, request.operationId);
}
export async function listOperations(): Promise<HerdrSessionResult<HerdrSessionOperation[]>> {
  return result(async () => (await listHerdrOperations()).map(projectOperation));
}
export async function controlOperation(request: { operationId: string; action: "cancel" | "revertBinding" }): Promise<HerdrSessionResult<HerdrSessionOperation>> {
  return result(async () => {
    assert(request.action === "cancel" || request.action === "revertBinding", "INVALID_ACTION", "지원하지 않는 작업입니다.");
    const operation = await getHerdrOperation(request.operationId);
    assert(operation, "OPERATION_MISSING", "작업을 찾을 수 없습니다.");
    if (request.action === "cancel") {
      assert(inFlight.has(request.operationId), "NOT_RUNNING", "진행 중인 작업만 취소할 수 있습니다.");
      cancelled.add(request.operationId); return operation;
    }
    assert(!inFlight.has(request.operationId) && !taskLocks.has(operation.plan.taskId) && operation.bindingCommitted && [Phase.COMPLETED, Phase.COMMITTED, Phase.ATTENTION].includes(operation.phase) && operation.targetWorkspaceId,
      "REVERT_BLOCKED", "연결 변경이 반영된 작업만 되돌릴 수 있습니다.");
    assert(!operation.plan.importTask || operation.plan.mode !== Mode.ATTACH_EXISTING, "NO_PREVIOUS_BINDING", "새로 가져온 태스크에는 이전 연결이 없습니다. 원본 Space와 가져온 태스크는 유지합니다.");
    taskLocks.add(operation.plan.taskId);
    try {
      const previous = parseHerdrSessionTarget(operation.plan.previousBinding.sessionName);
      const servers = await listHerdrSessions(operation.plan.previousBinding.sshHost);
      assert(servers.some((server) => server.name === previous.serverName && server.running), "REVERT_SOURCE_STOPPED", "이전 서버가 중단되어 있습니다. 자동 시작을 피하기 위해 연결을 유지합니다.");
      operation.phase = Phase.REVERTED; operation.error = null; operation.updatedAt = new Date().toISOString();
      assert(await commitHerdrBinding(operation, nextBinding(operation), operation.plan.previousBinding), "BINDING_CHANGED", "현재 연결이 달라서 되돌릴 수 없습니다.");
      await reconnect(operation.plan.taskId);
    } finally { taskLocks.delete(operation.plan.taskId); }
    return operation;
  }, request.operationId);
}
