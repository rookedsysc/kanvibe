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
  buildKanvibeTaskStateContent,
  getKanvibeTaskStatePath,
  parseKanvibeTaskState,
  parseTaskStatus,
  readKanvibeTaskState,
  writeKanvibeTaskState,
} from "@/lib/kanvibeProjectState";

describe("kanvibeProjectState", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockReadTextFile.mockResolvedValue("");
    mockWriteTextFile.mockResolvedValue(undefined);
  });

  it("builds status.md content with only the shared task status", () => {
    const content = buildKanvibeTaskStateContent({
      taskId: "task-1",
      status: TaskStatus.REVIEW,
    });

    expect(content).toBe("review\n");
    expect(content).not.toContain("task-1");
    expect(content).not.toContain("taskId");
    expect(content).not.toContain("updatedAt");
  });

  it("normalizes valid persisted task statuses and rejects unusable state files", () => {
    expect(parseTaskStatus("PROGRESS")).toBe(TaskStatus.PROGRESS);
    expect(parseTaskStatus(TaskStatus.PENDING)).toBe(TaskStatus.PENDING);
    expect(parseTaskStatus(42)).toBeNull();
    expect(parseTaskStatus("blocked")).toBeNull();

    expect(parseKanvibeTaskState("")).toBeNull();
    expect(parseKanvibeTaskState("not-a-status")).toBeNull();
    expect(parseKanvibeTaskState("DONE\n")).toEqual({
      version: 1,
      status: TaskStatus.DONE,
    });
    expect(parseKanvibeTaskState("  review  \n")).toEqual({
      version: 1,
      status: TaskStatus.REVIEW,
    });
  });

  it("uses host-aware .kanvibe/status.md paths when reading and writing project status", async () => {
    mockReadTextFile.mockResolvedValueOnce("pending\n");
    await expect(readKanvibeTaskState("/remote/repo", "ssh-host")).resolves.toEqual({
      version: 1,
      status: TaskStatus.PENDING,
    });
    expect(mockReadTextFile).toHaveBeenCalledWith("/remote/repo/.kanvibe/status.md", "ssh-host");

    await writeKanvibeTaskState("/remote/repo", { status: TaskStatus.DONE, taskId: "task-2" }, "ssh-host");
    expect(mockWriteTextFile).toHaveBeenLastCalledWith(
      "/remote/repo/.kanvibe/status.md",
      "done\n",
      "ssh-host",
    );

    expect(getKanvibeTaskStatePath("/local/repo")).toBe("/local/repo/.kanvibe/status.md");
  });
});
