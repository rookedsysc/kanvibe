import { describe, expect, it, vi, beforeEach } from "vitest";
import { TaskStatus } from "@/entities/KanbanTask";

const { mockReadTextFile, mockWriteTextFile } = vi.hoisted(() => ({
  mockReadTextFile: vi.fn(),
  mockWriteTextFile: vi.fn(),
}));

vi.mock("@/lib/hostFileAccess", () => ({
  readTextFile: (...args: unknown[]) => mockReadTextFile(...args),
  writeTextFile: (...args: unknown[]) => mockWriteTextFile(...args),
}));

import {
  buildKanvibeHookTargetsContent,
  buildKanvibeTaskStateContent,
  getKanvibeHookTargetsPath,
  getKanvibeTaskStatePath,
  parseKanvibeTaskState,
  parseTaskStatus,
  readKanvibeTaskState,
  upsertKanvibeHookTarget,
  writeKanvibeTaskState,
} from "@/lib/kanvibeProjectState";

describe("kanvibeProjectState", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReadTextFile.mockResolvedValue("");
    mockWriteTextFile.mockResolvedValue(undefined);
  });

  it("adds the current hook target after existing valid targets and removes only exact duplicates", () => {
    const current = JSON.stringify({
      version: 1,
      targets: [
        { url: "http://old-client:9736", taskId: "old-task" },
        { url: "http://local:9736", taskId: "task-1" },
        { url: "http://same-url-different-task:9736", taskId: "task-1" },
        { url: "http://local:9736", taskId: "other-task" },
      ],
    });

    const updated = JSON.parse(buildKanvibeHookTargetsContent(current, {
      url: "http://local:9736",
      taskId: "task-1",
    }));

    expect(updated).toEqual({
      version: 1,
      targets: [
        { url: "http://old-client:9736", taskId: "old-task" },
        { url: "http://same-url-different-task:9736", taskId: "task-1" },
        { url: "http://local:9736", taskId: "other-task" },
        { url: "http://local:9736", taskId: "task-1" },
      ],
    });
  });

  it("drops malformed hook target entries instead of writing broken fan-out targets", () => {
    const updated = JSON.parse(buildKanvibeHookTargetsContent(JSON.stringify({
      version: 1,
      targets: [
        null,
        "not-object",
        { url: 42, taskId: "task-1" },
        { url: "http://missing-task:9736" },
        { url: "", taskId: "task-1" },
        { url: "http://empty-task:9736", taskId: "" },
        { url: "http://valid:9736", taskId: "valid-task" },
      ],
    }), {
      url: "http://local:9736",
      taskId: "task-1",
    }));

    expect(updated.targets).toEqual([
      { url: "http://valid:9736", taskId: "valid-task" },
      { url: "http://local:9736", taskId: "task-1" },
    ]);
  });

  it("falls back to a fresh hook target document when existing content is empty, invalid, or missing the target array", () => {
    const target = { url: "http://local:9736", taskId: "task-1" };

    expect(JSON.parse(buildKanvibeHookTargetsContent("", target))).toEqual({
      version: 1,
      targets: [target],
    });
    expect(JSON.parse(buildKanvibeHookTargetsContent("{not-json", target))).toEqual({
      version: 1,
      targets: [target],
    });
    expect(JSON.parse(buildKanvibeHookTargetsContent(JSON.stringify({ targets: "not-array" }), target))).toEqual({
      version: 1,
      targets: [target],
    });
  });

  it("builds and parses task state while omitting absent optional fields", () => {
    const withTask = parseKanvibeTaskState(buildKanvibeTaskStateContent({
      taskId: "task-1",
      status: TaskStatus.REVIEW,
    }));
    const withoutTask = parseKanvibeTaskState(buildKanvibeTaskStateContent({
      taskId: null,
      status: TaskStatus.TODO,
    }));

    expect(withTask).toEqual(expect.objectContaining({
      version: 1,
      taskId: "task-1",
      status: TaskStatus.REVIEW,
      updatedAt: expect.any(String),
    }));
    expect(withoutTask).toEqual(expect.objectContaining({
      version: 1,
      status: TaskStatus.TODO,
      updatedAt: expect.any(String),
    }));
    expect(withoutTask).not.toHaveProperty("taskId");
  });

  it("normalizes valid persisted task statuses and rejects unusable state files", () => {
    expect(parseTaskStatus("PROGRESS")).toBe(TaskStatus.PROGRESS);
    expect(parseTaskStatus(TaskStatus.PENDING)).toBe(TaskStatus.PENDING);
    expect(parseTaskStatus(42)).toBeNull();
    expect(parseTaskStatus("blocked")).toBeNull();

    expect(parseKanvibeTaskState("")).toBeNull();
    expect(parseKanvibeTaskState("not-json")).toBeNull();
    expect(parseKanvibeTaskState(JSON.stringify({ status: "blocked" }))).toBeNull();
    expect(parseKanvibeTaskState(JSON.stringify({ status: 42 }))).toBeNull();
    expect(parseKanvibeTaskState(JSON.stringify({ status: "DONE", taskId: "", updatedAt: "" }))).toEqual({
      version: 1,
      status: TaskStatus.DONE,
    });
  });

  it("uses host-aware .kanvibe paths when reading and writing project sync files", async () => {
    mockReadTextFile.mockResolvedValue(JSON.stringify({
      version: 1,
      targets: [{ url: "http://old:9736", taskId: "old-task" }],
    }));

    await upsertKanvibeHookTarget("/remote/repo", { url: "http://local:9736", taskId: "task-1" }, "ssh-host");

    expect(mockReadTextFile).toHaveBeenCalledWith("/remote/repo/.kanvibe/hooks-targets.json", "ssh-host");
    expect(mockWriteTextFile).toHaveBeenCalledWith(
      "/remote/repo/.kanvibe/hooks-targets.json",
      expect.stringContaining("http://local:9736"),
      "ssh-host",
    );

    mockReadTextFile.mockResolvedValueOnce(JSON.stringify({ status: "pending", taskId: "task-2" }));
    await expect(readKanvibeTaskState("/remote/repo", "ssh-host")).resolves.toEqual({
      version: 1,
      status: TaskStatus.PENDING,
      taskId: "task-2",
    });

    await writeKanvibeTaskState("/remote/repo", { status: TaskStatus.DONE, taskId: "task-2" }, "ssh-host");
    expect(mockWriteTextFile).toHaveBeenLastCalledWith(
      "/remote/repo/.kanvibe/task-state.json",
      expect.stringContaining('"status": "done"'),
      "ssh-host",
    );

    expect(getKanvibeHookTargetsPath("/local/repo")).toBe("/local/repo/.kanvibe/hooks-targets.json");
    expect(getKanvibeTaskStatePath("/local/repo")).toBe("/local/repo/.kanvibe/task-state.json");
  });
});
