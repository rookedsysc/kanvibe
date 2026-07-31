import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: { findBy: vi.fn() },
  projectRepo: { findOneBy: vi.fn() },
  getTaskRepository: vi.fn(),
  getProjectRepository: vi.fn(),
  addAiToolPatternsToGitExclude: vi.fn(),
  readKanvibeProjectColor: vi.fn(),
  writeKanvibeProjectColor: vi.fn(),
  writeKanvibeProjectColorIfAbsent: vi.fn(),
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: mocks.getTaskRepository,
  getProjectRepository: mocks.getProjectRepository,
}));

vi.mock("@/lib/gitExclude", () => ({
  addAiToolPatternsToGitExclude: mocks.addAiToolPatternsToGitExclude,
}));

vi.mock("@/lib/kanvibeProjectState", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/kanvibeProjectState")>()),
  readKanvibeProjectColor: mocks.readKanvibeProjectColor,
  writeKanvibeProjectColor: mocks.writeKanvibeProjectColor,
  writeKanvibeProjectColorIfAbsent: mocks.writeKanvibeProjectColorIfAbsent,
}));

import type { Project } from "@/entities/Project";
import {
  persistProjectColorToKanvibeState,
  readSharedProjectColor,
  syncProjectColorWithKanvibeState,
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
    mocks.getProjectRepository.mockResolvedValue(mocks.projectRepo);
    mocks.taskRepo.findBy.mockResolvedValue([]);
    mocks.projectRepo.findOneBy.mockResolvedValue(null);
    mocks.addAiToolPatternsToGitExclude.mockResolvedValue(undefined);
    mocks.writeKanvibeProjectColor.mockResolvedValue(undefined);
    mocks.writeKanvibeProjectColorIfAbsent.mockResolvedValue(undefined);
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

  it("다른 client가 바꾼 색상만 DB 반영 대상으로 돌려준다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue("#0064FF");
    const project = createProject();

    await expect(syncProjectColorWithKanvibeState(project)).resolves.toBe("#0064FF");

    mocks.readKanvibeProjectColor.mockResolvedValue("#0064FF");
    await expect(syncProjectColorWithKanvibeState(createProject({ color: "#0064FF" }))).resolves.toBeNull();
  });

  it("프로젝트 색상을 직접 바꾸지 않아 호출한 쪽이 반영 여부를 판단할 수 있다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue("#0064FF");
    const project = createProject();

    await syncProjectColorWithKanvibeState(project);

    expect(project.color).toBe("#65D08A");
  });

  it("이미 같은 색상이 기록된 경로는 다시 쓰지 않는다", async () => {
    mocks.taskRepo.findBy.mockResolvedValue([
      { worktreePath: "/workspace/kanvibe__worktrees/feature" },
    ]);
    mocks.readKanvibeProjectColor.mockResolvedValue("#65D08A");

    await expect(syncProjectColorWithKanvibeState(createProject())).resolves.toBeNull();

    expect(mocks.writeKanvibeProjectColor).not.toHaveBeenCalled();
    expect(mocks.addAiToolPatternsToGitExclude).not.toHaveBeenCalled();
  });

  it("색상 확정 이후 생긴 worktree에도 공유 색상을 뒤늦게 기록한다", async () => {
    mocks.taskRepo.findBy.mockResolvedValue([
      { worktreePath: "/workspace/kanvibe__worktrees/feature" },
    ]);
    /** 프로젝트 루트에는 색상이 있지만 나중에 생긴 worktree에는 아직 없다 */
    mocks.readKanvibeProjectColor.mockImplementation(async (repoPath: string) =>
      repoPath === "/workspace/kanvibe" ? "#65D08A" : null,
    );

    await expect(syncProjectColorWithKanvibeState(createProject())).resolves.toBeNull();

    expect(mocks.writeKanvibeProjectColor.mock.calls).toEqual([
      ["/workspace/kanvibe__worktrees/feature", "#65D08A", null],
    ]);
  });

  it("sync 중 사용자가 색을 바꾸면 낡은 색이 아니라 루트의 새 색상을 퍼뜨린다", async () => {
    mocks.taskRepo.findBy.mockResolvedValue([
      { worktreePath: "/workspace/kanvibe__worktrees/feature" },
    ]);
    /** sync가 시작된 뒤 사용자가 #0064FF로 바꿔 루트와 worktree에 이미 기록된 상태 */
    mocks.readKanvibeProjectColor.mockResolvedValue("#0064FF");
    /** 메모리의 project.color는 sync 시작 시점의 낡은 값 */
    const project = createProject({ color: "#65D08A" });

    await expect(syncProjectColorWithKanvibeState(project)).resolves.toBe("#0064FF");

    expect(mocks.writeKanvibeProjectColor).not.toHaveBeenCalled();
  });

  it("프로젝트 루트 색상 파일은 sync 전파 대상에서 제외한다", async () => {
    mocks.taskRepo.findBy.mockResolvedValue([
      { worktreePath: "/workspace/kanvibe" },
      { worktreePath: "/workspace/kanvibe__worktrees/feature" },
    ]);
    /** 루트는 새 색상, worktree는 아직 옛 색상인 전파 직전 상태 */
    mocks.readKanvibeProjectColor.mockImplementation(async (repoPath: string) =>
      repoPath === "/workspace/kanvibe" ? "#0064FF" : "#65D08A",
    );

    await syncProjectColorWithKanvibeState(createProject({ color: "#0064FF" }));

    expect(mocks.writeKanvibeProjectColor.mock.calls).toEqual([
      ["/workspace/kanvibe__worktrees/feature", "#0064FF", null],
    ]);
  });

  it("공유 색상이 없으면 기존 색상을 유지한 채 씨앗으로 기록한다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue(null);
    mocks.projectRepo.findOneBy.mockResolvedValue({ color: "#65D08A" });
    const project = createProject();

    await expect(syncProjectColorWithKanvibeState(project)).resolves.toBeNull();

    expect(project.color).toBe("#65D08A");
    expect(mocks.writeKanvibeProjectColorIfAbsent)
      .toHaveBeenCalledWith("/workspace/kanvibe", "#65D08A", null);
  });

  it("씨앗은 sync 시작 시점의 낡은 색이 아니라 DB의 현재 색을 기록한다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue(null);
    /** sync가 시작된 뒤 사용자가 #0064FF로 바꿔 DB에는 이미 새 색이 저장된 상태 */
    mocks.projectRepo.findOneBy.mockResolvedValue({ color: "#0064FF" });

    await syncProjectColorWithKanvibeState(createProject({ color: "#65D08A" }));

    expect(mocks.writeKanvibeProjectColorIfAbsent)
      .toHaveBeenCalledWith("/workspace/kanvibe", "#0064FF", null);
  });

  it("씨앗은 권위 파일인 루트에만 기록하고 worktree는 전파에 맡긴다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue(null);
    mocks.projectRepo.findOneBy.mockResolvedValue({ color: "#65D08A" });
    mocks.taskRepo.findBy.mockResolvedValue([
      { worktreePath: "/workspace/kanvibe__worktrees/feature" },
    ]);

    await syncProjectColorWithKanvibeState(createProject());

    expect(mocks.writeKanvibeProjectColorIfAbsent.mock.calls).toEqual([
      ["/workspace/kanvibe", "#65D08A", null],
    ]);
    expect(mocks.writeKanvibeProjectColor).not.toHaveBeenCalled();
  });

  it("씨앗 기록이 실패해도 sync 흐름을 막지 않는다", async () => {
    mocks.readKanvibeProjectColor.mockResolvedValue(null);
    mocks.projectRepo.findOneBy.mockResolvedValue({ color: "#65D08A" });
    mocks.writeKanvibeProjectColorIfAbsent.mockRejectedValue(new Error("read-only"));

    await expect(syncProjectColorWithKanvibeState(createProject())).resolves.toBeNull();
  });

  it("색상 읽기 실패는 등록 흐름을 막지 않는다", async () => {
    mocks.readKanvibeProjectColor.mockRejectedValue(new Error("permission denied"));

    await expect(readSharedProjectColor("/workspace/kanvibe", null)).resolves.toBeNull();
  });
});
