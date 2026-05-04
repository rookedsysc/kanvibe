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
