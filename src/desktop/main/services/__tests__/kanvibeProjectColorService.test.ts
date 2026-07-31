import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: { findBy: vi.fn() },
  getTaskRepository: vi.fn(),
  addAiToolPatternsToGitExclude: vi.fn(),
  readKanvibeProjectColor: vi.fn(),
  writeKanvibeProjectColor: vi.fn(),
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: mocks.getTaskRepository,
}));

vi.mock("@/lib/gitExclude", () => ({
  addAiToolPatternsToGitExclude: mocks.addAiToolPatternsToGitExclude,
}));

vi.mock("@/lib/kanvibeProjectState", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/kanvibeProjectState")>()),
  readKanvibeProjectColor: mocks.readKanvibeProjectColor,
  writeKanvibeProjectColor: mocks.writeKanvibeProjectColor,
}));

import type { Project } from "@/entities/Project";
import {
  applySharedProjectColor,
  persistProjectColorToKanvibeState,
  readSharedProjectColor,
} from "@/desktop/main/services/kanvibeProjectColorService";

function createProject(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1",
    name: "kanvibe",
    repoPath: "/workspace/kanvibe",
    defaultBranch: "main",
    sshHost: null,
    isWorktree: false,
    color: "#65D08A",
    iconDataUrl: null,
    createdAt: new Date(),
    ...overrides,
  };
}

describe("kanvibeProjectColorService", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getTaskRepository.mockResolvedValue(mocks.taskRepo);
    mocks.taskRepo.findBy.mockResolvedValue([]);
    mocks.addAiToolPatternsToGitExclude.mockResolvedValue(undefined);
    mocks.writeKanvibeProjectColor.mockResolvedValue(undefined);
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  it("프로젝트 루트와 소속 task worktree에 모두 색상을 기록한다", async () => {
    mocks.taskRepo.findBy.mockResolvedValue([
      { worktreePath: "/workspace/kanvibe__worktrees/feature" },
      { worktreePath: null },
      { worktreePath: "/workspace/kanvibe__worktrees/feature" },
    ]);

    await persistProjectColorToKanvibeState(createProject({ sshHost: "remote-host" }));

    expect(mocks.writeKanvibeProjectColor.mock.calls).toEqual([
      ["/workspace/kanvibe", "#65D08A", "remote-host"],
      ["/workspace/kanvibe__worktrees/feature", "#65D08A", "remote-host"],
    ]);
  });

  it("색상이 없거나 형식이 어긋나면 기록하지 않는다", async () => {
    await persistProjectColorToKanvibeState(createProject({ color: null }));
    await persistProjectColorToKanvibeState(createProject({ color: "not-a-color" }));

    expect(mocks.writeKanvibeProjectColor).not.toHaveBeenCalled();
  });

  it("일부 경로 기록이 실패해도 나머지 경로 기록을 계속한다", async () => {
    mocks.taskRepo.findBy.mockResolvedValue([{ worktreePath: "/workspace/kanvibe__worktrees/feature" }]);
    mocks.writeKanvibeProjectColor.mockRejectedValueOnce(new Error("read-only"));

    await expect(persistProjectColorToKanvibeState(createProject())).resolves.toBeUndefined();

    expect(mocks.writeKanvibeProjectColor).toHaveBeenCalledTimes(2);
  });

  it("다른 client가 바꾼 색상만 프로젝트에 반영한다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue("#0064FF");
    const project = createProject();

    await expect(applySharedProjectColor(project)).resolves.toBe(true);
    expect(project.color).toBe("#0064FF");

    mocks.readKanvibeProjectColor.mockResolvedValue("#0064FF");
    await expect(applySharedProjectColor(project)).resolves.toBe(false);
  });

  it("공유 색상이 없으면 기존 색상을 유지한다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue(null);
    const project = createProject();

    await expect(applySharedProjectColor(project)).resolves.toBe(false);
    expect(project.color).toBe("#65D08A");
  });

  it("색상 읽기 실패는 등록 흐름을 막지 않는다", async () => {
    mocks.readKanvibeProjectColor.mockRejectedValue(new Error("permission denied"));

    await expect(readSharedProjectColor("/workspace/kanvibe", null)).resolves.toBeNull();
  });
});
