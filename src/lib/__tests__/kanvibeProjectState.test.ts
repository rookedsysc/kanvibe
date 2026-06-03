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

  it("builds extensible status.json content without task/url mappings", () => {
    const content = buildKanvibeTaskStateContent({
      status: TaskStatus.REVIEW,
    });
    const parsed = JSON.parse(content) as { schemaVersion?: unknown; status?: unknown; updatedAt?: unknown };

    expect(parsed).toEqual({
      schemaVersion: 1,
      status: TaskStatus.REVIEW,
      updatedAt: expect.any(String),
    });
    expect(Number.isNaN(Date.parse(String(parsed.updatedAt)))).toBe(false);
    expect(content).not.toContain("task-1");
    expect(content).not.toContain("taskId");
    expect(content).not.toContain("url");
  });

  it("normalizes valid persisted task statuses and rejects unusable state files", () => {
    expect(parseTaskStatus("PROGRESS")).toBe(TaskStatus.PROGRESS);
    expect(parseTaskStatus(TaskStatus.PENDING)).toBe(TaskStatus.PENDING);
    expect(parseTaskStatus(42)).toBeNull();
    expect(parseTaskStatus("blocked")).toBeNull();

    expect(parseKanvibeTaskState("")).toBeNull();
    expect(parseKanvibeTaskState("not-a-status")).toBeNull();
    expect(parseKanvibeTaskState(JSON.stringify({ schemaVersion: 1, status: "review", updatedAt: "2026-06-03T00:00:00.000Z" }))).toEqual({
      schemaVersion: 1,
      status: TaskStatus.REVIEW,
      updatedAt: "2026-06-03T00:00:00.000Z",
    });
    expect(parseKanvibeTaskState("DONE\n")).toEqual({
      schemaVersion: 1,
      status: TaskStatus.DONE,
    });
    expect(parseKanvibeTaskState("  review  \n")).toEqual({
      schemaVersion: 1,
      status: TaskStatus.REVIEW,
    });
  });

  it("uses host-aware .kanvibe/status.json paths when reading and writing project status", async () => {
    mockReadTextFile.mockResolvedValueOnce(JSON.stringify({ schemaVersion: 1, status: "pending" }));
    await expect(readKanvibeTaskState("/remote/repo", "ssh-host")).resolves.toEqual({
      schemaVersion: 1,
      status: TaskStatus.PENDING,
    });
    expect(mockReadTextFile).toHaveBeenCalledWith("/remote/repo/.kanvibe/status.json", "ssh-host");

    await writeKanvibeTaskState("/remote/repo", { status: TaskStatus.DONE }, "ssh-host");
    expect(mockWriteTextFile).toHaveBeenLastCalledWith(
      "/remote/repo/.kanvibe/status.json",
      expect.stringContaining('"status": "done"'),
      "ssh-host",
    );
    const writtenContent = mockWriteTextFile.mock.calls.at(-1)?.[1] as string;
    expect(JSON.parse(writtenContent)).toEqual({
      schemaVersion: 1,
      status: TaskStatus.DONE,
      updatedAt: expect.any(String),
    });
    expect(writtenContent).not.toContain("task-2");
    expect(writtenContent).not.toContain("taskId");
    expect(writtenContent).not.toContain("url");

    expect(getKanvibeTaskStatePath("/local/repo")).toBe("/local/repo/.kanvibe/status.json");
  });
});
