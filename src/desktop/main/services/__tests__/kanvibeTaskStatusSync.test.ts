import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DataSource } from "typeorm";
import { KanbanTask, TaskStatus } from "@/entities/KanbanTask";
import { Project } from "@/entities/Project";

const mocks = vi.hoisted(() => ({ repository: undefined as unknown, readTextFiles: vi.fn(), writeTextFile: vi.fn() }));
vi.mock("@/lib/database", () => ({ getTaskRepository: async () => mocks.repository, getProjectRepository: vi.fn() }));
vi.mock("@/lib/hostFileAccess", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/hostFileAccess")>(),
  readTextFiles: mocks.readTextFiles,
  writeTextFile: mocks.writeTextFile,
}));
import { syncTaskStatusesFromFiles } from "../kanvibeTaskStateService";

describe("sync task statuses from files", () => {
  let database: DataSource;
  const snapshotTime = new Date("2026-11-15T00:00:00.000Z");
  beforeEach(async () => {
    vi.clearAllMocks();
    mocks.readTextFiles.mockReset();
    database = new DataSource({ type: "better-sqlite3", database: ":memory:", entities: [KanbanTask, Project], synchronize: true });
    await database.initialize();
    mocks.repository = database.getRepository(KanbanTask);
    mocks.readTextFiles.mockImplementation(async (paths: string[]) => new Map(paths.map((file) => [file, {
      exists: true, content: '{"schemaVersion":1,"status":"review"}',
    }])));
  });
  afterEach(async () => { await database?.destroy(); });

  async function createTask(id: string, fields: Partial<KanbanTask> = {}) {
    await database.getRepository(KanbanTask).insert({
      id, title: id, status: TaskStatus.PROGRESS, worktreePath: `/fixture/${id}`, updatedAt: snapshotTime, ...fields,
    });
  }
  const current = (id: string) => database.getRepository(KanbanTask).findOneByOrFail({ id });

  it.each(Object.values(TaskStatus))("Git 탐색에 의존하지 않고 기존 task를 %s 상태로 교정한다", async (status) => {
    await createTask("standalone", { status: status === TaskStatus.PROGRESS ? TaskStatus.DONE : TaskStatus.PROGRESS });
    mocks.readTextFiles.mockResolvedValue(new Map([["/fixture/standalone/.kanvibe/status.json", {
      exists: true, content: JSON.stringify({ status }),
    }]]));

    expect(await syncTaskStatusesFromFiles()).toEqual({ changed: true, errors: [] });
    expect((await current("standalone")).status).toBe(status);
    expect(mocks.writeTextFile).not.toHaveBeenCalled();
  });

  it("같은 호스트의 파일을 한 번에 읽고 프로젝트의 호스트와 기본 task 경로를 사용한다", async () => {
    await database.getRepository(Project).insert({ id: "project", name: "api", repoPath: "/remote/api", defaultBranch: "main", sshHost: "host-a" });
    await createTask("root", { projectId: "project", branchName: "main", worktreePath: null });
    await createTask("remote", { sshHost: "host-a" });
    await createTask("local");
    await createTask("same-path", { worktreePath: "/fixture/local" });

    expect(await syncTaskStatusesFromFiles()).toEqual({ changed: true, errors: [] });

    expect(mocks.readTextFiles).toHaveBeenCalledTimes(2);
    expect(mocks.readTextFiles).toHaveBeenCalledWith(["/remote/api/.kanvibe/status.json", "/fixture/remote/.kanvibe/status.json"], "host-a");
    expect(mocks.readTextFiles).toHaveBeenCalledWith(["/fixture/local/.kanvibe/status.json"], null);
    expect((await current("root")).status).toBe(TaskStatus.REVIEW);
    expect((await current("same-path")).status).toBe(TaskStatus.REVIEW);
  });

  it("파일 누락·손상·같은 상태는 DB를 쓰지 않고 경로 없는 task를 건너뛴다", async () => {
    await createTask("missing");
    await createTask("broken");
    await createTask("unchanged");
    await createTask("no-path", { worktreePath: null });
    mocks.readTextFiles.mockResolvedValue(new Map([
      ["/fixture/missing/.kanvibe/status.json", { exists: false, content: "" }],
      ["/fixture/broken/.kanvibe/status.json", { exists: true, content: '{"status":"unknown"}' }],
      ["/fixture/unchanged/.kanvibe/status.json", { exists: true, content: '{"status":"progress"}' }],
    ]));
    const update = vi.spyOn(database.getRepository(KanbanTask), "update");

    expect(await syncTaskStatusesFromFiles()).toEqual({ changed: false, errors: [] });
    expect(update).not.toHaveBeenCalled();
    expect(mocks.readTextFiles.mock.calls[0][0]).not.toContain("/fixture/no-path/.kanvibe/status.json");
  });

  it("한 호스트의 읽기가 실패해도 다른 호스트의 상태를 교정한다", async () => {
    await createTask("offline", { sshHost: "offline-host" });
    await createTask("local");
    mocks.readTextFiles.mockImplementation(async (paths: string[], host: string | null) => {
      if (host) throw new Error("offline");
      return new Map([[paths[0], { exists: true, content: '{"status":"pending"}' }]]);
    });

    const result = await syncTaskStatusesFromFiles();

    expect(result.changed).toBe(true);
    expect(result.errors).toEqual([expect.stringContaining("offline")]);
    expect((await current("offline")).status).toBe(TaskStatus.PROGRESS);
    expect((await current("local")).status).toBe(TaskStatus.PENDING);
  });

  it.each([TaskStatus.DONE, TaskStatus.PROGRESS])("읽는 도중 갱신된 DB 스냅샷을 덮어쓰지 않는다: %s", async (status) => {
    await createTask("concurrent");
    mocks.readTextFiles.mockImplementation(async (paths: string[]) => {
      await database.getRepository(KanbanTask).update("concurrent", { status, title: "newer", updatedAt: new Date("2026-11-15T00:00:01.123Z") });
      return new Map([[paths[0], { exists: true, content: '{"status":"review"}' }]]);
    });

    expect(await syncTaskStatusesFromFiles()).toEqual({ changed: false, errors: [] });
    expect(await current("concurrent")).toMatchObject({ status, title: "newer" });
  });

  it("읽는 동안 삭제된 task를 다시 생성하지 않는다", async () => {
    await createTask("deleted");
    mocks.readTextFiles.mockImplementation(async (paths: string[]) => {
      await database.getRepository(KanbanTask).delete("deleted");
      return new Map([[paths[0], { exists: true, content: '{"status":"review"}' }]]);
    });

    expect(await syncTaskStatusesFromFiles()).toEqual({ changed: false, errors: [] });
    expect(await database.getRepository(KanbanTask).count()).toBe(0);
  });

  it("task가 없으면 파일을 읽지 않는다", async () => {
    expect(await syncTaskStatusesFromFiles()).toEqual({ changed: false, errors: [] });
    expect(mocks.readTextFiles).not.toHaveBeenCalled();
  });
});
