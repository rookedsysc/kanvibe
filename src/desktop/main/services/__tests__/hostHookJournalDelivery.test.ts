import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DataSource } from "typeorm";
import { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";
const state = vi.hoisted(() => ({ repository: undefined as unknown, execute: vi.fn(), board: vi.fn(), status: vi.fn(), missing: vi.fn() }));
vi.mock("@/lib/database", () => ({ getTaskRepository: async () => state.repository, getProjectRepository: vi.fn() }));
vi.mock("@/lib/gitOperations", () => ({ execGit: state.execute, resolvePathForShell: (value: string) => value }));
vi.mock("@/lib/boardNotifier", () => ({ broadcastBoardUpdate: state.board, broadcastTaskStatusChanged: state.status, broadcastHookStatusTargetMissing: state.missing }));
import { updateJournalHookTaskStatus } from "../hookService";

describe("journal hook delivery with current remote state and SQLite CAS", () => {
  let database: DataSource;
  const source = new Set(["host"]);
  beforeEach(async () => {
    vi.clearAllMocks();
    database = new DataSource({ type: "better-sqlite3", database: ":memory:", entities: [KanbanTask, Project], synchronize: true });
    await database.initialize(); state.repository = database.getRepository(KanbanTask);
    state.execute.mockResolvedValue('{"schemaVersion":1,"status":"review"}');
    await database.getRepository(KanbanTask).insert({ id: "fixture", title: "fixture", status: TaskStatus.PROGRESS, sshHost: "host", worktreePath: "/fixture" });
  });
  afterEach(async () => { await database?.destroy(); });
  const current = () => database.getRepository(KanbanTask).findOneByOrFail({ id: "fixture" });

  it("applies the current status once without rewriting the remote snapshot", async () => {
    const input = { taskId: "fixture", status: "review" };
    expect(await updateJournalHookTaskStatus(input, source)).toMatchObject({ success: true, delivery: "applied" });
    expect((await current()).status).toBe(TaskStatus.REVIEW);
    expect(await updateJournalHookTaskStatus(input, source)).toMatchObject({ success: true, delivery: "ignored" });
    expect(state.board).toHaveBeenCalledTimes(1);
    expect(state.status).toHaveBeenCalledTimes(1);
    expect(state.execute.mock.calls.every(([command]) => !/printf|mkdir|status.json.*>/.test(command))).toBe(true);
  });

  it("acknowledges an older offline status without changing the latest task or sending alerts", async () => {
    expect(await updateJournalHookTaskStatus({ taskId: "fixture", status: "done", agentSession: { provider: "codex", hookPid: 1234 } }, source)).toMatchObject({ success: true, delivery: "ignored" });
    expect((await current()).status).toBe(TaskStatus.PROGRESS);
    expect(state.board).not.toHaveBeenCalled();
    expect(state.status).not.toHaveBeenCalled();
    expect(state.execute).toHaveBeenCalledTimes(1);
  });

  it("protects a newer UI update made while reading the remote file", async () => {
    state.execute.mockImplementation(async () => {
      await database.getRepository(KanbanTask).update("fixture", { status: TaskStatus.DONE, updatedAt: new Date("2026-11-14T11:00:00.123Z") });
      return '{"schemaVersion":1,"status":"review"}';
    });
    expect(await updateJournalHookTaskStatus({ taskId: "fixture", status: "review" }, source)).toMatchObject({ success: true, delivery: "ignored" });
    expect((await current()).status).toBe(TaskStatus.DONE);
    expect(state.board).not.toHaveBeenCalled();
  });

  it("uses the same seconds-precision SQLite timestamp comparison as reconnect sync", async () => {
    await database.query("UPDATE kanban_tasks SET updated_at = ? WHERE id = ?", ["2026-11-14 00:00:00", "fixture"]);
    expect(await updateJournalHookTaskStatus({ taskId: "fixture", status: "review" }, source)).toMatchObject({ success: true, delivery: "applied" });
    expect((await current()).status).toBe(TaskStatus.REVIEW);
  });

  it("rejects another host before looking at any remote file", async () => {
    expect(await updateJournalHookTaskStatus({ taskId: "fixture", status: "review" }, new Set(["other"]))).toMatchObject({ success: false, status: 404 });
    expect(state.execute).not.toHaveBeenCalled();
    expect((await current()).status).toBe(TaskStatus.PROGRESS);
  });

  it("keeps an event retryable when its remote status is unavailable", async () => {
    state.execute.mockRejectedValue(new Error("offline"));
    expect(await updateJournalHookTaskStatus({ taskId: "fixture", status: "review" }, source)).toMatchObject({ success: false, status: 503 });
    expect((await current()).status).toBe(TaskStatus.PROGRESS);
    expect(state.board).not.toHaveBeenCalled();
  });

  it.each([{ taskId: "", status: "review" }, { taskId: "fixture", status: "invalid" }, { taskId: "missing", status: "review" }])("permanently rejects invalid input or a removed target %j", async input => {
    const result = await updateJournalHookTaskStatus(input, source);
    expect(result.success).toBe(false);
    expect(result.status).toBeGreaterThanOrEqual(400);
    expect(result.status).toBeLessThan(500);
    expect(state.execute).not.toHaveBeenCalled();
    expect(state.board).not.toHaveBeenCalled();
  });
});
