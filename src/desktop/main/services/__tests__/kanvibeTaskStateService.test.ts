import { beforeEach, describe, expect, it, vi } from "vitest";

const entityMocks = vi.hoisted(() => ({
  TaskStatus: {
    TODO: "todo",
    PROGRESS: "progress",
    PENDING: "pending",
    REVIEW: "review",
    DONE: "done",
  },
}));

const mocks = vi.hoisted(() => ({
  projectRepo: {
    findOneBy: vi.fn(),
  },
  getProjectRepository: vi.fn(),
  readKanvibeTaskState: vi.fn(),
  writeKanvibeTaskStatus: vi.fn(),
  addAiToolPatternsToGitExclude: vi.fn(),
}));

vi.mock("@/entities/KanbanTask", () => entityMocks);

vi.mock("@/lib/database", () => ({
  getProjectRepository: mocks.getProjectRepository,
}));

vi.mock("@/lib/kanvibeProjectState", () => ({
  readKanvibeTaskState: mocks.readKanvibeTaskState,
  writeKanvibeTaskStatus: mocks.writeKanvibeTaskStatus,
}));

vi.mock("@/lib/gitExclude", () => ({
  addAiToolPatternsToGitExclude: mocks.addAiToolPatternsToGitExclude,
}));

const TaskStatus = entityMocks.TaskStatus as unknown as typeof import("@/entities/KanbanTask").TaskStatus;

describe("kanvibeTaskStateService", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.getProjectRepository.mockReset();
    mocks.projectRepo.findOneBy.mockReset();
    mocks.readKanvibeTaskState.mockReset();
    mocks.writeKanvibeTaskStatus.mockReset();
    mocks.addAiToolPatternsToGitExclude.mockReset();
    mocks.getProjectRepository.mockResolvedValue(mocks.projectRepo);
  });

  it("path 기반 task 상태 저장은 누락된 path를 무시한다", async () => {
    const { persistTaskStateAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");

    await persistTaskStateAtPath(null, { id: "task-1", status: TaskStatus.PROGRESS }, "ssh-host");

    expect(mocks.writeKanvibeTaskStatus).not.toHaveBeenCalled();
    expect(mocks.addAiToolPatternsToGitExclude).not.toHaveBeenCalled();
  });

  it("path 기반 task 상태 저장은 status 파일 작성 전에 git exclude를 갱신한다", async () => {
    const { persistTaskStateAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");

    await persistTaskStateAtPath(
      "/remote/repo__worktrees/feature-task",
      { id: "task-1", status: TaskStatus.REVIEW },
      "remote-host",
    );

    expect(mocks.addAiToolPatternsToGitExclude).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-task",
      "remote-host",
    );
    expect(mocks.addAiToolPatternsToGitExclude.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.writeKanvibeTaskStatus.mock.invocationCallOrder[0],
    );
    expect(mocks.writeKanvibeTaskStatus).toHaveBeenCalledWith(
      "/remote/repo__worktrees/feature-task",
      TaskStatus.REVIEW,
      "remote-host",
    );
  });

  it("git exclude 갱신 실패는 status 파일 저장을 막지 않는다", async () => {
    const consoleWarn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    mocks.addAiToolPatternsToGitExclude.mockRejectedValue(new Error("not a git repo"));
    const { persistTaskStateAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");

    await expect(persistTaskStateAtPath(
      "/workspace/repo",
      { id: "task-1", status: TaskStatus.PROGRESS },
      null,
    )).resolves.toBeUndefined();

    expect(mocks.writeKanvibeTaskStatus).toHaveBeenCalledWith(
      "/workspace/repo",
      TaskStatus.PROGRESS,
      null,
    );
    expect(consoleWarn).toHaveBeenCalledWith(
      ".kanvibe 상태 디렉터리 git exclude 갱신 실패:",
      expect.objectContaining({
        repoPath: "/workspace/repo",
        sshHost: null,
        error: "not a git repo",
      }),
    );
    consoleWarn.mockRestore();
  });

  it("에이전트 hook 이벤트가 이어져도 같은 저장소의 git exclude는 한 번만 확인하고 상태는 매번 기록한다", async () => {
    const { persistTaskStateAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");
    const repoPath = "/remote/repo__worktrees/feature-task";

    await persistTaskStateAtPath(repoPath, { id: "task-1", status: TaskStatus.PROGRESS }, "remote-host");
    await persistTaskStateAtPath(repoPath, { id: "task-1", status: TaskStatus.PENDING }, "remote-host");
    await persistTaskStateAtPath(repoPath, { id: "task-1", status: TaskStatus.REVIEW }, "remote-host");

    // exclude 확인은 원격에서 검증 접속을 두세 번 쓰므로 이벤트마다 되풀이하지 않는다.
    expect(mocks.addAiToolPatternsToGitExclude).toHaveBeenCalledTimes(1);
    expect(mocks.writeKanvibeTaskStatus.mock.calls.map(([, status]) => status)).toEqual([
      TaskStatus.PROGRESS,
      TaskStatus.PENDING,
      TaskStatus.REVIEW,
    ]);
  });

  it("git exclude 확인은 저장소와 호스트마다 따로 기억하고, 동시에 온 기록은 진행 중인 확인을 함께 기다린다", async () => {
    let finishExcludeCheck!: () => void;
    mocks.addAiToolPatternsToGitExclude.mockImplementationOnce(() => new Promise<void>((resolve) => { finishExcludeCheck = resolve; }));
    const { persistTaskStateAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");

    const firstWrite = persistTaskStateAtPath("/repo/a", { id: "task-1", status: TaskStatus.PROGRESS }, "host-a");
    const overlappingWrite = persistTaskStateAtPath("/repo/a", { id: "task-1", status: TaskStatus.REVIEW }, "host-a");
    await Promise.resolve();

    // 확인이 끝나기 전에는 어느 쪽도 상태 파일을 쓰지 않아 exclude 뒤에 기록하는 순서가 유지된다.
    expect(mocks.addAiToolPatternsToGitExclude).toHaveBeenCalledTimes(1);
    expect(mocks.writeKanvibeTaskStatus).not.toHaveBeenCalled();
    finishExcludeCheck();
    await Promise.all([firstWrite, overlappingWrite]);
    expect(mocks.writeKanvibeTaskStatus).toHaveBeenCalledTimes(2);

    await persistTaskStateAtPath("/repo/a", { id: "task-2", status: TaskStatus.PROGRESS }, "host-b");
    await persistTaskStateAtPath("/repo/b", { id: "task-3", status: TaskStatus.PROGRESS }, "host-a");
    expect(mocks.addAiToolPatternsToGitExclude).toHaveBeenCalledTimes(3);
  });

  it("실패한 git exclude 확인은 기억하지 않아 다음 기록에서 다시 시도하고, 기억한 확인도 시간이 지나면 다시 한다", async () => {
    vi.useFakeTimers();
    const consoleWarn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    try {
      mocks.addAiToolPatternsToGitExclude.mockRejectedValueOnce(new Error("Connection reset by peer"));
      const { persistTaskStateAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");
      const persist = () => persistTaskStateAtPath("/repo/a", { id: "task-1", status: TaskStatus.PROGRESS }, "host-a");

      await persist();
      await persist();
      expect(mocks.addAiToolPatternsToGitExclude).toHaveBeenCalledTimes(2);

      await persist();
      expect(mocks.addAiToolPatternsToGitExclude).toHaveBeenCalledTimes(2);

      // 누군가 exclude를 지워도 이 시간 뒤의 기록이 복구한다.
      vi.advanceTimersByTime(10 * 60_000);
      await persist();
      expect(mocks.addAiToolPatternsToGitExclude).toHaveBeenCalledTimes(3);
    } finally {
      consoleWarn.mockRestore();
      vi.useRealTimers();
    }
  });

  it("path 기반 task 상태 저장은 실패해도 호출자 흐름을 막지 않는다", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    mocks.writeKanvibeTaskStatus.mockRejectedValue(new Error("disk full"));
    const { persistTaskStateAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");

    await expect(persistTaskStateAtPath(
      "/workspace/repo",
      { id: "task-1", status: TaskStatus.REVIEW },
      "ssh-host",
    )).resolves.toBeUndefined();

    expect(mocks.writeKanvibeTaskStatus).toHaveBeenCalledWith(
      "/workspace/repo",
      TaskStatus.REVIEW,
      "ssh-host",
    );
    expect(consoleError).toHaveBeenCalledWith(
      ".kanvibe task 상태 저장 실패:",
      expect.objectContaining({
        targetPath: "/workspace/repo",
        taskId: "task-1",
        status: TaskStatus.REVIEW,
        sshHost: "ssh-host",
        error: "disk full",
      }),
    );
    consoleError.mockRestore();
  });

  it("task worktreePath가 있으면 project 조회 없이 해당 path에 상태를 저장한다", async () => {
    const { persistTaskStateForTask } = await import("@/desktop/main/services/kanvibeTaskStateService");

    await persistTaskStateForTask({
      id: "task-1",
      status: TaskStatus.PENDING,
      worktreePath: "/workspace/repo__worktrees/task-1",
      sshHost: "ssh-host",
      projectId: "project-1",
      branchName: "feature/sync",
    });

    expect(mocks.getProjectRepository).not.toHaveBeenCalled();
    expect(mocks.writeKanvibeTaskStatus).toHaveBeenCalledWith(
      "/workspace/repo__worktrees/task-1",
      TaskStatus.PENDING,
      "ssh-host",
    );
  });

  it("worktreePath가 없는 default branch task는 project root에 상태를 저장한다", async () => {
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: "project-ssh",
    });
    const { persistTaskStateForTask } = await import("@/desktop/main/services/kanvibeTaskStateService");

    await persistTaskStateForTask({
      id: "task-main",
      status: TaskStatus.TODO,
      worktreePath: null,
      sshHost: null,
      projectId: "project-1",
      branchName: "main",
    });

    expect(mocks.projectRepo.findOneBy).toHaveBeenCalledWith({ id: "project-1" });
    expect(mocks.writeKanvibeTaskStatus).toHaveBeenCalledWith(
      "/workspace/repo",
      TaskStatus.TODO,
      "project-ssh",
    );
  });

  it("worktreePath 없는 non-default branch task는 상태 파일을 쓰지 않는다", async () => {
    mocks.projectRepo.findOneBy.mockResolvedValue({
      id: "project-1",
      repoPath: "/workspace/repo",
      defaultBranch: "main",
      sshHost: null,
    });
    const { persistTaskStateForTask } = await import("@/desktop/main/services/kanvibeTaskStateService");

    await persistTaskStateForTask({
      id: "task-feature",
      status: TaskStatus.PROGRESS,
      worktreePath: null,
      sshHost: null,
      projectId: "project-1",
      branchName: "feature/sync",
    });

    expect(mocks.projectRepo.findOneBy).toHaveBeenCalledWith({ id: "project-1" });
    expect(mocks.writeKanvibeTaskStatus).not.toHaveBeenCalled();
  });

  it("저장된 task 상태만 읽어 반환한다", async () => {
    mocks.readKanvibeTaskState.mockResolvedValue({
      schemaVersion: 1,
      status: TaskStatus.DONE,
      updatedAt: "2026-06-03T00:00:00.000Z",
    });
    const { readPersistedTaskStatusAtPath } = await import("@/desktop/main/services/kanvibeTaskStateService");

    const status = await readPersistedTaskStatusAtPath("/workspace/repo", "ssh-host");

    expect(status).toBe(TaskStatus.DONE);
    expect(mocks.readKanvibeTaskState).toHaveBeenCalledWith("/workspace/repo", "ssh-host");
  });
});
