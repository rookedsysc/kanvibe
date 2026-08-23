import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: {
    findOne: vi.fn(),
  },
  taskDiffStatsRepo: {
    find: vi.fn(),
    save: vi.fn(),
  },
  execGit: vi.fn(),
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
  getTaskDiffStatsRepository: vi.fn(async () => mocks.taskDiffStatsRepo),
}));

vi.mock("@/lib/gitOperations", () => ({
  execGit: mocks.execGit,
}));

vi.mock("@/lib/hostFileAccess", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/hostFileAccess")>();
  return {
    ...actual,
    readTextFile: mocks.readTextFile,
    writeTextFile: mocks.writeTextFile,
  };
});

describe("diffService remote task support", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("원격 태스크의 diff 파일 목록을 sshHost로 조회한다", async () => {
    mocks.taskRepo.findOne.mockResolvedValue({
      id: "task-1",
      worktreePath: "/remote/worktrees/fix-qa",
      branchName: "fix/qa",
      baseBranch: "main",
      sshHost: "remote-host",
    });
    mocks.execGit.mockImplementation(async (command: string, sshHost?: string | null) => {
      expect(sshHost).toBe("remote-host");
      expect(command).toContain("git -C '/remote/worktrees/fix-qa' diff 'main...fix/qa' --name-status");
      expect(command).toContain("git -C '/remote/worktrees/fix-qa' diff 'main...fix/qa' --numstat");
      expect(command).toContain("git -C '/remote/worktrees/fix-qa' status --porcelain --untracked-files=all");
      return [
        "__KANVIBE_DIFF_NAME_STATUS__",
        "M\tsrc/app.ts",
        "__KANVIBE_DIFF_NUMSTAT__",
        "3\t1\tsrc/app.ts",
        "__KANVIBE_DIFF_WORKING_TREE__",
        "?? docs/new.md",
      ].join("\n");
    });

    const { getGitDiffFiles } = await import("@/desktop/main/services/diffService");

    await expect(getGitDiffFiles("task-1")).resolves.toEqual([
      { path: "src/app.ts", status: "modified", additions: 3, deletions: 1 },
      { path: "docs/new.md", status: "added", additions: 0, deletions: 0 },
    ]);
    expect(mocks.execGit).toHaveBeenCalledWith(
      expect.stringContaining("git -C '/remote/worktrees/fix-qa' diff 'main...fix/qa' --name-status"),
      "remote-host",
    );
    expect(mocks.execGit).toHaveBeenCalledTimes(1);
  });

  it("원격 태스크의 파일 읽기와 저장을 sshHost로 처리한다", async () => {
    mocks.taskRepo.findOne.mockResolvedValue({
      id: "task-1",
      worktreePath: "/remote/worktrees/fix-qa",
      branchName: "fix/qa",
      baseBranch: "main",
      sshHost: "remote-host",
    });
    mocks.execGit.mockResolvedValue("base content");
    mocks.readTextFile.mockResolvedValue("current content");
    mocks.writeTextFile.mockResolvedValue(undefined);

    const { getOriginalFileContent, getFileContent, saveFileContent } = await import("@/desktop/main/services/diffService");

    await expect(getOriginalFileContent("task-1", "src/app.ts")).resolves.toBe("base content");
    await expect(getFileContent("task-1", "src/app.ts")).resolves.toBe("current content");
    await expect(saveFileContent("task-1", "src/app.ts", "updated")).resolves.toEqual({ success: true });

    expect(mocks.execGit).toHaveBeenCalledWith(
      expect.stringContaining("git -C '/remote/worktrees/fix-qa' show 'main:src/app.ts'"),
      "remote-host",
    );
    expect(mocks.readTextFile).toHaveBeenCalledWith("/remote/worktrees/fix-qa/src/app.ts", "remote-host");
    expect(mocks.writeTextFile).toHaveBeenCalledWith("/remote/worktrees/fix-qa/src/app.ts", "updated", "remote-host");
  });
});

describe("getTaskDiffStats", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.taskDiffStatsRepo.find.mockResolvedValue([]);
    mocks.taskDiffStatsRepo.save.mockResolvedValue(undefined);
    mocks.taskRepo.findOne.mockImplementation(async ({ where }: { where: { id: string } }) => ({
      id: where.id,
      worktreePath: `/repo__worktrees/${where.id}`,
      branchName: `feat/${where.id}`,
      baseBranch: "main",
      sshHost: null,
    }));
  });

  function buildDiffOutput(additions: number, deletions: number) {
    return [
      "__KANVIBE_DIFF_NAME_STATUS__",
      "M\tsrc/app.ts",
      "__KANVIBE_DIFF_NUMSTAT__",
      `${additions}\t${deletions}\tsrc/app.ts`,
      "__KANVIBE_DIFF_WORKING_TREE__",
    ].join("\n");
  }

  it("넘긴 태스크만 git을 돌리고 그 집계를 저장한다", async () => {
    mocks.execGit.mockResolvedValue(buildDiffOutput(12, 3));

    const { getTaskDiffStats } = await import("@/desktop/main/services/diffService");

    await expect(getTaskDiffStats(["task-progress"])).resolves.toEqual({
      "task-progress": { fileCount: 1, additions: 12, deletions: 3 },
    });
    expect(mocks.execGit).toHaveBeenCalledTimes(1);
    expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledWith({
      taskId: "task-progress",
      fileCount: 1,
      additions: 12,
      deletions: 3,
    });
  });

  it("git을 돌리지 않은 태스크는 저장돼 있던 집계로 채운다", async () => {
    mocks.taskDiffStatsRepo.find.mockResolvedValue([
      { taskId: "task-review", fileCount: 4, additions: 40, deletions: 8 },
      { taskId: "task-progress", fileCount: 1, additions: 1, deletions: 1 },
    ]);
    mocks.execGit.mockResolvedValue(buildDiffOutput(12, 3));

    const { getTaskDiffStats } = await import("@/desktop/main/services/diffService");

    await expect(getTaskDiffStats(["task-progress"])).resolves.toEqual({
      "task-review": { fileCount: 4, additions: 40, deletions: 8 },
      "task-progress": { fileCount: 1, additions: 12, deletions: 3 },
    });
    expect(mocks.execGit).toHaveBeenCalledTimes(1);
  });

  it("다시 돌릴 태스크가 없으면 git 없이 저장된 집계만 돌려준다", async () => {
    mocks.taskDiffStatsRepo.find.mockResolvedValue([
      { taskId: "task-review", fileCount: 4, additions: 40, deletions: 8 },
    ]);

    const { getTaskDiffStats } = await import("@/desktop/main/services/diffService");

    await expect(getTaskDiffStats([])).resolves.toEqual({
      "task-review": { fileCount: 4, additions: 40, deletions: 8 },
    });
    expect(mocks.execGit).not.toHaveBeenCalled();
    expect(mocks.taskDiffStatsRepo.save).not.toHaveBeenCalled();
  });

  it("조회에 실패한 태스크는 저장된 집계를 덮어쓰지 않는다", async () => {
    mocks.taskDiffStatsRepo.find.mockResolvedValue([
      { taskId: "task-progress", fileCount: 4, additions: 40, deletions: 8 },
    ]);
    mocks.execGit.mockRejectedValue(new Error("git 실행 실패"));

    const { getTaskDiffStats } = await import("@/desktop/main/services/diffService");

    await expect(getTaskDiffStats(["task-progress"])).resolves.toEqual({
      "task-progress": { fileCount: 4, additions: 40, deletions: 8 },
    });
    expect(mocks.taskDiffStatsRepo.save).not.toHaveBeenCalled();
  });

  it("상세에서 파일 목록을 읽으면 그 태스크의 집계도 갱신된다", async () => {
    mocks.execGit.mockResolvedValue(buildDiffOutput(7, 2));

    const { getGitDiffFiles } = await import("@/desktop/main/services/diffService");

    await expect(getGitDiffFiles("task-review")).resolves.toEqual([
      { path: "src/app.ts", status: "modified", additions: 7, deletions: 2 },
    ]);
    expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledWith({
      taskId: "task-review",
      fileCount: 1,
      additions: 7,
      deletions: 2,
    });
  });
});
