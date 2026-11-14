/** @vitest-environment node */
import { execFileSync, execSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  /** 갱신을 예약할 때 조회하는 task의 호스트. 적지 않은 task는 로컬이고, missingTaskIds에 든 task는 DB에 없다 */
  const taskHosts = {
    hostByTaskId: {} as Record<string, string>,
    missingTaskIds: new Set<string>(),
  };

  return {
    taskHosts,
    taskRepo: {
      findOne: vi.fn(),
      find: async ({ where }: { where: { id: { value: string[] } } }) => where.id.value
        .filter((taskId) => !taskHosts.missingTaskIds.has(taskId))
        .map((taskId) => ({ id: taskId, sshHost: taskHosts.hostByTaskId[taskId] ?? null })),
    },
    taskDiffStatsRepo: {
      find: vi.fn(),
      save: vi.fn(),
    },
    execGit: vi.fn(),
    /** execGit 호출 순서대로 그 명령이 받은 원격 명령 우선순위 */
    execGitPriorities: [] as Array<"background" | "foreground">,
    readTextFile: vi.fn(),
    writeTextFile: vi.fn(),
  };
});

vi.mock("@/lib/database", () => ({
  getTaskRepository: vi.fn(async () => mocks.taskRepo),
  getTaskDiffStatsRepository: vi.fn(async () => mocks.taskDiffStatsRepo),
}));

vi.mock("@/lib/gitOperations", async () => {
  const { AsyncLocalStorage } = await import("node:async_hooks");
  const backgroundPriority = new AsyncLocalStorage<true>();

  return {
    execGit: (...args: unknown[]) => {
      mocks.execGitPriorities.push(backgroundPriority.getStore() ? "background" : "foreground");
      return mocks.execGit(...args);
    },
    runWithBackgroundSSHCommands: <T>(operation: () => Promise<T>) => backgroundPriority.run(true, operation),
  };
});

beforeEach(() => {
  mocks.taskHosts.hostByTaskId = {};
  mocks.taskHosts.missingTaskIds.clear();
  mocks.execGitPriorities.length = 0;
});

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

    await expect(getTaskDiffStats(["task-progress"])).resolves.toEqual({});
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalled());
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
      "task-progress": { fileCount: 1, additions: 1, deletions: 1 },
    });
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalled());
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
      await service.getGitDiffFiles("fixture");
      expect(mocks.taskDiffStatsRepo.save).not.toHaveBeenCalled();
      task.baseBranch = "main";
      expect(await service.getGitDiffFiles("fixture")).toEqual([]);
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


describe("readTaskWorkspaceFile", () => {
  const MAX_FILE_BYTES = 1024 * 1024;
  let directory: string;
  let outsideDirectory: string;

  async function loadService(sshHost: string | null = null) {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.taskRepo.findOne.mockResolvedValue({ worktreePath: directory, branchName: "main", sshHost });
    mocks.execGit.mockImplementation(async (command: string) => execSync(command, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 4 * MAX_FILE_BYTES }).trim());
    return import("@/desktop/main/services/diffService");
  }

  beforeEach(() => {
    directory = mkdtempSync(path.join(tmpdir(), "kanvibe-workspace-file-"));
    outsideDirectory = mkdtempSync(path.join(tmpdir(), "kanvibe-workspace-outside-"));
    mkdirSync(path.join(directory, "docs"));
    writeFileSync(path.join(directory, "docs/plan.md"), "# plan\n본문\n");
    writeFileSync(path.join(outsideDirectory, "secret.txt"), "secret");
  });

  afterEach(() => {
    rmSync(directory, { recursive: true, force: true });
    rmSync(outsideDirectory, { recursive: true, force: true });
  });

  it("worktree 안의 상대 경로는 내용을 읽어 돌려준다", async () => {
    const service = await loadService();

    await expect(service.readTaskWorkspaceFile("task-1", "docs/plan.md")).resolves.toEqual({ path: "docs/plan.md", content: "# plan\n본문\n" });
  });

  it("worktree 안의 절대 경로는 상대 경로로 바꿔 돌려준다", async () => {
    const service = await loadService();

    await expect(service.readTaskWorkspaceFile("task-1", path.join(directory, "docs/plan.md")))
      .resolves.toEqual({ path: path.join("docs", "plan.md"), content: "# plan\n본문\n" });
  });

  it.each([null, "remote-host"])("상위 경로와 worktree 밖 절대 경로는 읽지 않고 outside-worktree로 거절한다 (host=%s)", async (sshHost) => {
    const service = await loadService(sshHost);
    const outsideFile = path.join(outsideDirectory, "secret.txt");

    await expect(service.readTaskWorkspaceFile("task-1", `../${path.basename(outsideDirectory)}/secret.txt`))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "outside-worktree", message: "outside-worktree" });
    await expect(service.readTaskWorkspaceFile("task-1", outsideFile))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "outside-worktree" });
    await expect(service.readTaskWorkspaceFile("task-1", `${directory}-sibling/secret.txt`))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "outside-worktree" });
    expect(mocks.execGit).not.toHaveBeenCalled();
  });

  it.each([null, "remote-host"])("1MiB를 넘으면 too-large로 거절하고 정확히 1MiB는 읽는다 (host=%s)", async (sshHost) => {
    writeFileSync(path.join(directory, "limit.txt"), "a".repeat(MAX_FILE_BYTES));
    writeFileSync(path.join(directory, "over.txt"), "a".repeat(MAX_FILE_BYTES + 1));
    const service = await loadService(sshHost);

    const atLimit = await service.readTaskWorkspaceFile("task-1", "limit.txt");
    expect(atLimit.path).toBe("limit.txt");
    expect(Buffer.byteLength(atLimit.content)).toBe(MAX_FILE_BYTES);
    await expect(service.readTaskWorkspaceFile("task-1", "over.txt"))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "too-large" });
  });

  it.each([null, "remote-host"])("없는 파일과 디렉터리는 not-found로 알린다 (host=%s)", async (sshHost) => {
    const service = await loadService(sshHost);

    await expect(service.readTaskWorkspaceFile("task-1", "docs/missing.md"))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "not-found" });
    await expect(service.readTaskWorkspaceFile("task-1", "docs"))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "not-found" });
  });

  it("없는 태스크는 not-found로 알린다", async () => {
    const service = await loadService();
    mocks.taskRepo.findOne.mockResolvedValue(null);

    await expect(service.readTaskWorkspaceFile("missing-task", "docs/plan.md"))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "not-found" });
  });

  it("원격 태스크는 sshHost로 크기를 먼저 재고 내용을 읽는다", async () => {
    const service = await loadService("remote-host");

    await expect(service.readTaskWorkspaceFile("task-1", path.join(directory, "docs/plan.md")))
      .resolves.toEqual({ path: "docs/plan.md", content: "# plan\n본문" });
    expect(mocks.execGit).toHaveBeenCalledWith(
      expect.stringContaining(`wc -c < '${directory}/docs/plan.md'`),
      "remote-host",
      { maxBufferBytes: MAX_FILE_BYTES + 4096 },
    );
  });

  it("원격 명령이 실패하면 내부 오류 대신 unreadable로 알린다", async () => {
    const service = await loadService("remote-host");
    mocks.execGit.mockRejectedValue(new Error("ssh: connect to host remote-host port 22: Connection refused"));
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});

    await expect(service.readTaskWorkspaceFile("task-1", "docs/plan.md"))
      .rejects.toMatchObject({ name: "TaskWorkspaceFileError", reason: "unreadable", message: "unreadable" });
    consoleError.mockRestore();
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


describe("cached lazy diff refresh", () => {
  const output = (additions: number) => ["__KANVIBE_DIFF_NAME_STATUS__", "M\0a.ts", "__KANVIBE_DIFF_NUMSTAT__", `${additions}\t0\ta.ts`, "__KANVIBE_DIFF_WORKING_TREE__"].join("\0") + "\0";
  beforeEach(() => {
    vi.resetModules(); vi.resetAllMocks();
    const cache = new Map<string, object>([["slow", { taskId: "slow", fileCount: 1, additions: 9, deletions: 0 }]]);
    mocks.taskDiffStatsRepo.find.mockImplementation(async () => [...cache.values()]);
    mocks.taskDiffStatsRepo.save.mockImplementation(async (record) => { cache.set(record.taskId, record); });
    mocks.taskRepo.findOne.mockImplementation(async ({ where }) => ({ id: where.id, worktreePath: `/repo/${where.id}`, branchName: where.id, baseBranch: "dev" }));
  });

  it("느린 git을 기다리지 않고 cache를 반환하고 완료된 항목부터 다음 조회에 보여준다", async () => {
    let finish!: (value: string) => void;
    mocks.execGit.mockImplementation((command: string) => command.includes("/repo/slow") ? new Promise<string>((resolve) => { finish = resolve; }) : Promise.resolve(output(3)));
    const service = await import("@/desktop/main/services/diffService");
    let initial: unknown;
    const pending = service.getTaskDiffStats(["slow", "fast"]).then((value) => { initial = value; });
    try {
      await vi.waitFor(() => expect(initial).toEqual({ slow: { fileCount: 1, additions: 9, deletions: 0 } }), { timeout: 200 });
      await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledWith({ taskId: "fast", fileCount: 1, additions: 3, deletions: 0 }));
      expect(await service.getTaskDiffStats([])).toEqual({ slow: { fileCount: 1, additions: 9, deletions: 0 }, fast: { fileCount: 1, additions: 3, deletions: 0 } });
    } finally { finish(output(7)); await pending; }
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledWith({ taskId: "slow", fileCount: 1, additions: 7, deletions: 0 }));
  });

  it("refresh를 두 개로 제한하고 진행/대기 중인 task 중복은 합친다", async () => {
    const finish: Array<(value: string) => void> = [];
    mocks.execGit.mockImplementation(() => new Promise<string>((resolve) => { finish.push(resolve); }));
    const service = await import("@/desktop/main/services/diffService");
    const first = service.getTaskDiffStats(["one", "two", "three", "one"]);
    const duplicate = service.getTaskDiffStats(["one", "two", "three"]);
    try {
      await vi.waitFor(() => expect(mocks.execGit).toHaveBeenCalledTimes(2), { timeout: 200 });
      finish[1](output(2));
      await vi.waitFor(() => expect(mocks.execGit).toHaveBeenCalledTimes(3));
      expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledTimes(1);
    } finally {
      for (const resolve of finish) resolve(output(1));
      await Promise.all([first, duplicate]);
    }
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledTimes(3));
  });

  it("한 호스트의 갱신이 막혀도 다른 호스트와 로컬의 갱신은 바로 시작하고, 호스트마다 두 개까지만 동시에 돈다", async () => {
    mocks.taskHosts.hostByTaskId = { "slow-1": "slow-host", "slow-2": "slow-host", "slow-3": "slow-host", "other-1": "other-host" };
    const finish = new Map<string, (value: string) => void>();
    mocks.execGit.mockImplementation((command: string) => new Promise<string>((resolve) => {
      finish.set(/\/repo\/([a-z0-9-]+)/.exec(command)![1], resolve);
    }));
    const service = await import("@/desktop/main/services/diffService");

    await service.getTaskDiffStats(["slow-1", "slow-2", "slow-3", "other-1", "local-1"]);

    try {
      // 앱 전체 제한 2였다면 slow-host의 두 갱신이 끝날 때까지 other-1과 local-1은 시작하지 못한다.
      await vi.waitFor(() => expect([...finish.keys()].sort()).toEqual(["local-1", "other-1", "slow-1", "slow-2"]), { timeout: 200 });
      finish.get("slow-1")!(output(1));
      await vi.waitFor(() => expect(finish.has("slow-3")).toBe(true));
    } finally {
      for (const resolve of finish.values()) resolve(output(1));
    }
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledTimes(5));
    expect(mocks.execGit).toHaveBeenCalledTimes(5);
  });

  it("보드 배지 갱신은 background 우선순위로, 상세의 직접 조회는 화면 요청 우선순위로 원격 명령을 낸다", async () => {
    mocks.taskHosts.hostByTaskId = { board: "remote-host", detail: "remote-host" };
    mocks.execGit.mockResolvedValue(output(1));
    const service = await import("@/desktop/main/services/diffService");

    await service.getTaskDiffStats(["board"]);
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledTimes(1));
    await service.getGitDiffFiles("detail");

    expect(mocks.execGitPriorities).toEqual(["background", "foreground"]);
  });

  it("DB에서 사라진 task는 갱신을 예약하지 않아 폴링마다 실패하는 조회를 내지 않는다", async () => {
    mocks.taskHosts.missingTaskIds.add("deleted");
    mocks.execGit.mockResolvedValue(output(4));
    const service = await import("@/desktop/main/services/diffService");

    await service.getTaskDiffStats(["deleted", "alive"]);
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledTimes(1));

    expect(mocks.execGit).toHaveBeenCalledTimes(1);
    expect(mocks.taskRepo.findOne).toHaveBeenCalledTimes(1);
    expect(mocks.taskRepo.findOne).toHaveBeenCalledWith({ where: { id: "alive" } });
  });

  it("상세와 board가 같은 task 읽기와 저장을 공유하고 옛 요청의 역전을 막는다", async () => {
    let finish!: (value: string) => void;
    mocks.execGit.mockImplementationOnce(() => new Promise<string>((resolve) => { finish = resolve; }));
    const service = await import("@/desktop/main/services/diffService");
    const detail = service.getGitDiffFiles("same");
    const duplicateDetail = service.getGitDiffFiles("same");
    const board = service.getTaskDiffStats(["same", "same"]);
    await vi.waitFor(() => expect(mocks.execGit).toHaveBeenCalled());
    const count = mocks.execGit.mock.calls.length;
    finish(output(1));
    await Promise.all([detail, duplicateDetail, board]);
    expect(count).toBe(1);
    expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledTimes(1);
    mocks.execGit.mockResolvedValue(output(8));
    await service.getGitDiffFiles("same");
    expect(await service.getTaskDiffStats([])).toMatchObject({ same: { additions: 8 } });
  });

  it("대기 중인 board task를 상세에서 먼저 읽으면 queue에서 제거해 다시 읽지 않는다", async () => {
    const finish = new Map<string, (value: string) => void>();
    mocks.execGit.mockImplementation((command: string) => new Promise<string>((resolve) => {
      const taskId = ["one", "two", "three"].find((id) => command.includes(`/repo/${id}`))!;
      finish.set(taskId, resolve);
    }));
    const service = await import("@/desktop/main/services/diffService");
    await service.getTaskDiffStats(["one", "two", "three"]);
    await vi.waitFor(() => expect(finish.size).toBe(2));
    const detail = service.getGitDiffFiles("three");
    await vi.waitFor(() => expect(finish.has("three")).toBe(true));
    finish.get("three")!(output(3));
    await detail;
    finish.get("one")!(output(1));
    finish.get("two")!(output(2));
    await vi.waitFor(() => expect(mocks.taskDiffStatsRepo.save).toHaveBeenCalledTimes(3));
    expect(mocks.execGit).toHaveBeenCalledTimes(3);
    expect(await service.getTaskDiffStats([])).toMatchObject({ three: { additions: 3 } });
  });

});
