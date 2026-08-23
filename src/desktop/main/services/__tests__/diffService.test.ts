import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  taskRepo: {
    findOne: vi.fn(),
  },
  execGit: vi.fn(),
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
}));

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
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
  });

  it("여러 태스크의 변경 집계를 한 번에 돌려준다", async () => {
    mocks.taskRepo.findOne.mockImplementation(async ({ where }: { where: { id: string } }) => ({
      id: where.id,
      worktreePath: `/repo__worktrees/${where.id}`,
      branchName: `feat/${where.id}`,
      baseBranch: "main",
      sshHost: null,
    }));
    mocks.execGit.mockImplementation(async (command: string) => {
      const isFirstTask = command.includes("/repo__worktrees/task-1");
      return isFirstTask
        ? [
            "__KANVIBE_DIFF_NAME_STATUS__",
            "M\tsrc/app.ts",
            "A\tsrc/new.ts",
            "__KANVIBE_DIFF_NUMSTAT__",
            "12\t3\tsrc/app.ts",
            "88\t0\tsrc/new.ts",
            "__KANVIBE_DIFF_WORKING_TREE__",
          ].join("\n")
        : [
            "__KANVIBE_DIFF_NAME_STATUS__",
            "D\tscripts/old.cjs",
            "__KANVIBE_DIFF_NUMSTAT__",
            "0\t41\tscripts/old.cjs",
            "__KANVIBE_DIFF_WORKING_TREE__",
          ].join("\n");
    });

    const { getTaskDiffStats } = await import("@/desktop/main/services/diffService");

    await expect(getTaskDiffStats(["task-1", "task-2"])).resolves.toEqual({
      "task-1": { fileCount: 2, additions: 100, deletions: 3 },
      "task-2": { fileCount: 1, additions: 0, deletions: 41 },
    });
  });

  it("조회에 실패한 태스크는 빈 집계로 남기고 나머지는 그대로 돌려준다", async () => {
    mocks.taskRepo.findOne.mockImplementation(async ({ where }: { where: { id: string } }) => (
      where.id === "task-without-worktree"
        ? { id: where.id, worktreePath: null, branchName: null, baseBranch: "main", sshHost: null }
        : { id: where.id, worktreePath: "/repo__worktrees/ok", branchName: "feat/ok", baseBranch: "main", sshHost: null }
    ));
    mocks.execGit.mockResolvedValue([
      "__KANVIBE_DIFF_NAME_STATUS__",
      "M\tsrc/app.ts",
      "__KANVIBE_DIFF_NUMSTAT__",
      "5\t2\tsrc/app.ts",
      "__KANVIBE_DIFF_WORKING_TREE__",
    ].join("\n"));

    const { getTaskDiffStats } = await import("@/desktop/main/services/diffService");

    await expect(getTaskDiffStats(["task-without-worktree", "task-ok"])).resolves.toEqual({
      "task-without-worktree": { fileCount: 0, additions: 0, deletions: 0 },
      "task-ok": { fileCount: 1, additions: 5, deletions: 2 },
    });
  });

  it("조회할 태스크가 없으면 git을 돌리지 않는다", async () => {
    const { getTaskDiffStats } = await import("@/desktop/main/services/diffService");

    await expect(getTaskDiffStats([])).resolves.toEqual({});
    expect(mocks.execGit).not.toHaveBeenCalled();
  });
});
