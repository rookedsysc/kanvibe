/** @vitest-environment node */
import { execFileSync, execSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
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
        "M\0src/app.ts",
        "__KANVIBE_DIFF_NUMSTAT__",
        "3\t1\tsrc/app.ts",
        "__KANVIBE_DIFF_WORKING_TREE__",
        "?? docs/new.md",
      ].join("\0") + "\0";
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
      "M\0src/app.ts",
      "__KANVIBE_DIFF_NUMSTAT__",
      `${additions}\t${deletions}\tsrc/app.ts`,
      "__KANVIBE_DIFF_WORKING_TREE__",
    ].join("\0") + "\0";
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


describe("실제 Git 파일명", () => {
  it("공백·한글·탭·개행과 커밋/작업 트리 rename의 목적지 경로를 읽고 저장한다", async () => {
    const directory = mkdtempSync(path.join(tmpdir(), "kanvibe-diff-path-"));
    const git = (...args: string[]) => execFileSync("git", ["-C", directory, ...args], { encoding: "utf8" });
    try {
      git("init", "-b", "main");
      git("config", "user.email", "fixture@example.invalid");
      git("config", "user.name", "fixture");
      writeFileSync(path.join(directory, "committed old.txt"), "committed rename\n");
      writeFileSync(path.join(directory, "working old.txt"), "working rename\n");
      git("add", ".");
      git("commit", "-m", "initial");
      git("checkout", "-b", "feature");
      git("mv", "committed old.txt", "committed 한글\tnew.txt");
      writeFileSync(path.join(directory, "stats\n\t한글.txt"), "first\nsecond\n");
      git("add", ".");
      git("commit", "-m", "rename");
      git("mv", "working old.txt", "working 새 파일.txt");
      const untracked = ["hello world.txt", "한글.txt", "tab\tfile.txt", "line\nfile.txt"];
      for (const name of untracked) writeFileSync(path.join(directory, name), "original\n");
      vi.resetModules();
      vi.clearAllMocks();
      mocks.taskRepo.findOne.mockResolvedValue({ worktreePath: directory, branchName: "feature", baseBranch: "main", sshHost: null });
      mocks.taskDiffStatsRepo.save.mockResolvedValue(undefined);
      mocks.execGit.mockImplementation(async (command: string) => execSync(command, { encoding: "utf8" }));
      mocks.readTextFile.mockImplementation(async (filePath: string) => readFileSync(filePath, "utf8"));
      mocks.writeTextFile.mockImplementation(async (filePath: string, contents: string) => writeFileSync(filePath, contents));
      const service = await import("@/desktop/main/services/diffService");
      const files = await service.getGitDiffFiles("fixture");
      const expected = [...untracked, "committed 한글\tnew.txt", "working 새 파일.txt", "stats\n\t한글.txt"];
      expect(files.find((file) => file.path === "stats\n\t한글.txt")).toEqual({ path: "stats\n\t한글.txt", status: "added", additions: 2, deletions: 0 });
      expect(files.map((file) => file.path).sort()).toEqual(expected.sort());
      expect(files.filter((file) => file.status === "renamed").map((file) => file.path).sort()).toEqual(["committed 한글\tnew.txt", "working 새 파일.txt"].sort());
      for (const name of expected) {
        expect(await service.getFileContent("fixture", name)).not.toBe("");
        expect(await service.saveFileContent("fixture", name, "updated\n")).toEqual({ success: true });
        expect(readFileSync(path.join(directory, name), "utf8")).toBe("updated\n");
      }
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});


describe("diff 실패와 경로 경계", () => {
  it("잘못된 revision은 캐시를 보존하고 정상 빈 diff만 0으로 저장한다", async () => {
    const directory = mkdtempSync(path.join(tmpdir(), "kanvibe-diff-failure-"));
    const git = (...args: string[]) => execFileSync("git", ["-C", directory, ...args], { encoding: "utf8" });
    try {
      git("init", "-b", "main");
      git("config", "user.email", "fixture@example.invalid");
      git("config", "user.name", "fixture");
      writeFileSync(path.join(directory, "tracked.txt"), "initial\n");
      git("add", ".");
      git("commit", "-m", "initial");
      vi.resetModules();
      vi.clearAllMocks();
      const task = { worktreePath: directory, branchName: "main", baseBranch: "missing-base", sshHost: null };
      mocks.taskRepo.findOne.mockResolvedValue(task);
      mocks.taskDiffStatsRepo.find.mockResolvedValue([{ taskId: "fixture", fileCount: 8, additions: 21, deletions: 5 }]);
      mocks.execGit.mockImplementation(async (command: string) => execSync(command, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }));
      const service = await import("@/desktop/main/services/diffService");

      expect(await service.getTaskDiffStats(["fixture"])).toEqual({ fixture: { fileCount: 8, additions: 21, deletions: 5 } });
      expect(mocks.taskDiffStatsRepo.save).not.toHaveBeenCalled();
      task.baseBranch = "main";
      expect(await service.getTaskDiffStats(["fixture"])).toEqual({ fixture: { fileCount: 0, additions: 0, deletions: 0 } });
      expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledWith({ taskId: "fixture", fileCount: 0, additions: 0, deletions: 0 });
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it.each([null, "remote-host"])("점 두 개로 시작하는 내부 파일은 읽고 저장하며 상위 경로는 차단한다 (host=%s)", async (sshHost) => {
    const directory = mkdtempSync(path.join(tmpdir(), "kanvibe-dotdot-file-"));
    try {
      mkdirSync(path.join(directory, "..directory"));
      writeFileSync(path.join(directory, "..notes"), "notes\n");
      writeFileSync(path.join(directory, "..directory/file"), "nested\n");
      vi.resetModules();
      vi.clearAllMocks();
      mocks.taskRepo.findOne.mockResolvedValue({ worktreePath: directory, branchName: "main", sshHost });
      mocks.readTextFile.mockImplementation(async (filePath: string) => readFileSync(filePath, "utf8"));
      mocks.writeTextFile.mockImplementation(async (filePath: string, contents: string) => writeFileSync(filePath, contents));
      const service = await import("@/desktop/main/services/diffService");

      expect(await service.getFileContent("fixture", "..notes")).toBe("notes\n");
      expect(await service.getFileContent("fixture", "..directory/file")).toBe("nested\n");
      expect(await service.saveFileContent("fixture", "..notes", "updated\n")).toEqual({ success: true });
      expect(readFileSync(path.join(directory, "..notes"), "utf8")).toBe("updated\n");
      expect(await service.saveFileContent("fixture", "..directory/file", "nested updated\n")).toEqual({ success: true });
      expect(readFileSync(path.join(directory, "..directory/file"), "utf8")).toBe("nested updated\n");
      expect(await service.saveFileContent("fixture", "../outside", "blocked")).toMatchObject({ success: false });
      expect(await service.saveFileContent("fixture", "/outside", "blocked")).toMatchObject({ success: false });
      expect(mocks.writeTextFile).toHaveBeenCalledTimes(2);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});


describe("rename 원본 내용", () => {
  it.each([
    ["committed", false],
    ["committed", true],
    ["staged", false],
    ["staged", true],
    ["chained", false],
    ["chained", true],
  ] as const)("%s rename에서 수정 여부=%s와 무관하게 이전 경로의 원본을 읽는다", async (kind, edited) => {
    const directory = mkdtempSync(path.join(tmpdir(), "kanvibe-rename-original-"));
    const git = (...args: string[]) => execFileSync("git", ["-C", directory, ...args], { encoding: "utf8" });
    const oldPath = "old 한글\tfile.txt";
    const newPath = "new 한글\nfile.txt";
    const original = Array.from({ length: 12 }, (_, index) => `original line ${index}\n`).join("");
    const current = edited ? original.replace("original line 5", "changed line 5") : original;
    try {
      git("init", "-b", "main");
      git("config", "user.email", "fixture@example.invalid");
      git("config", "user.name", "fixture");
      writeFileSync(path.join(directory, oldPath), original);
      git("add", ".");
      git("commit", "-m", "initial");
      git("checkout", "-b", "feature");
      git("mv", oldPath, newPath);
      writeFileSync(path.join(directory, newPath), current);
      git("add", ".");
      if (kind !== "staged") git("commit", "-m", "rename");
      const finalPath = kind === "chained" ? "final.txt" : newPath;
      if (kind === "chained") git("mv", newPath, finalPath);
      vi.resetModules();
      vi.clearAllMocks();
      mocks.taskRepo.findOne.mockResolvedValue({ worktreePath: directory, branchName: "feature", baseBranch: "main", sshHost: null });
      mocks.taskDiffStatsRepo.save.mockResolvedValue(undefined);
      mocks.execGit.mockImplementation(async (command: string) => execSync(command, { encoding: "utf8" }));
      mocks.readTextFile.mockImplementation(async (filePath: string) => readFileSync(filePath, "utf8"));
      const service = await import("@/desktop/main/services/diffService");
      const files = await service.getGitDiffFiles("fixture");
      expect(files).toHaveLength(1);
      expect(files[0]).toMatchObject({ path: finalPath, originalPath: oldPath, status: "renamed" });
      expect(await service.getOriginalFileContent("fixture", files[0].originalPath ?? files[0].path)).toBe(original);
      expect(await service.getFileContent("fixture", files[0].path)).toBe(current);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
