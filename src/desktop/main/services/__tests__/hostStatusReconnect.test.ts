import { beforeEach, describe, expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({ find: vi.fn(), update: vi.fn(), execute: vi.fn(), broadcast: vi.fn() }));
vi.mock("@/lib/database", () => ({ getTaskRepository: async () => ({ find: mocks.find, update: mocks.update }), getProjectRepository: vi.fn() }));
vi.mock("@/lib/gitOperations", () => ({ execGit: mocks.execute, resolvePathForShell: (value: string) => value }));
vi.mock("@/lib/boardNotifier", () => ({ broadcastBoardUpdate: mocks.broadcast }));
import { refreshHostTaskStatuses } from "@/desktop/main/services/kanvibeTaskStateService";

describe("host reconnect current status snapshot", () => {
  const updatedAt = new Date("2026-11-14T00:00:00Z");
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.find.mockResolvedValue([{ id: "one", status: "progress", sshHost: "host-a", worktreePath: "/fixture/one", updatedAt }]);
    mocks.execute.mockResolvedValue('{"schemaVersion":1,"status":"review"}');
    mocks.update.mockResolvedValue({ affected: 1 });
  });
  it("reads only a current snapshot and broadcasts a changed status once", async () => {
    await refreshHostTaskStatuses(new Set(["host-a"]), () => true);
    expect(mocks.execute).toHaveBeenCalledWith(expect.stringContaining("/fixture/one/.kanvibe/status.json"), "host-a", expect.any(Object));
    expect(mocks.update).toHaveBeenCalledWith({ id: "one", status: "progress", updatedAt: expect.objectContaining({ objectLiteralParameters: { snapshotUpdatedAt: updatedAt.toISOString() } }) }, { status: "review" });
    expect(mocks.broadcast).toHaveBeenCalledTimes(1);
  });
  it("does not overwrite a live DB update that won while the file was read", async () => {
    mocks.update.mockResolvedValue({ affected: 0 });
    await refreshHostTaskStatuses(new Set(["host-a"]), () => true);
    expect(mocks.update.mock.calls[0][0]).toMatchObject({ status: "progress", updatedAt: expect.objectContaining({ objectLiteralParameters: { snapshotUpdatedAt: updatedAt.toISOString() } }) });
    expect(mocks.broadcast).not.toHaveBeenCalled();
  });
  it("drops a snapshot returned after this connection was replaced", async () => {
    let current = true;
    mocks.execute.mockImplementation(async () => { current = false; return '{"schemaVersion":1,"status":"done"}'; });
    await refreshHostTaskStatuses(new Set(["host-a"]), () => current);
    expect(mocks.update).not.toHaveBeenCalled(); expect(mocks.broadcast).not.toHaveBeenCalled();
  });
  it("ignores tasks from other hosts and tasks without an execution directory", async () => {
    mocks.find.mockResolvedValue([
      { id: "other", status: "todo", sshHost: "host-b", worktreePath: "/other" },
      { id: "missing", status: "todo", sshHost: "host-a", worktreePath: null },
    ]);
    await refreshHostTaskStatuses(new Set(["host-a"]), () => true);
    expect(mocks.execute).not.toHaveBeenCalled(); expect(mocks.update).not.toHaveBeenCalled();
  });
});
