import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DataSource } from "typeorm";
import { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";
const state = vi.hoisted(() => ({ repository: undefined as unknown, execute: vi.fn() }));
vi.mock("@/lib/database", () => ({ getTaskRepository: async () => state.repository, getProjectRepository: vi.fn() }));
vi.mock("@/lib/gitOperations", () => ({ execGit: state.execute, resolvePathForShell: (value: string) => value }));
import { refreshHostTaskStatuses } from "@/desktop/main/services/kanvibeTaskStateService";

describe("reconnect SQLite compare and set in an isolated memory fixture", () => {
  let database: DataSource;
  beforeEach(async () => {
    database = new DataSource({ type: "better-sqlite3", database: ":memory:", entities: [KanbanTask, Project], synchronize: true });
    await database.initialize(); state.repository = database.getRepository(KanbanTask);
    state.execute.mockReset();
    state.execute.mockResolvedValue('{"schemaVersion":1,"status":"review"}');
    await database.getRepository(KanbanTask).insert({ id: "fixture", title: "fixture", status: TaskStatus.PROGRESS, sshHost: "host", worktreePath: "/fixture" });
  });
  afterEach(async () => { await database?.destroy(); });
  it("updates SQLite timestamps stored at second precision", async () => {
    await database.query("UPDATE kanban_tasks SET updated_at = ? WHERE id = ?", ["2026-11-14 00:00:00", "fixture"]);
    await refreshHostTaskStatuses(new Set(["host"]), () => true);
    expect((await database.getRepository(KanbanTask).findOneByOrFail({ id: "fixture" })).status).toBe(TaskStatus.REVIEW);
  });
  it("preserves a newer live update during the asynchronous file read", async () => {
    state.execute.mockImplementation(async () => {
      await database.getRepository(KanbanTask).update("fixture", { status: TaskStatus.DONE, updatedAt: new Date("2026-11-14T01:00:00.123Z") });
      return '{"schemaVersion":1,"status":"review"}';
    });
    await refreshHostTaskStatuses(new Set(["host"]), () => true);
    expect((await database.getRepository(KanbanTask).findOneByOrFail({ id: "fixture" })).status).toBe(TaskStatus.DONE);
  });
});
