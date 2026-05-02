import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  syncRegisteredProjectWorktrees: vi.fn(),
  syncActiveTaskPullRequests: vi.fn(),
  syncActiveTaskPulls: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
  broadcastBackgroundSyncReviewNeeded: vi.fn(),
}));

vi.mock("@/desktop/main/services/projectService", () => ({
  syncRegisteredProjectWorktrees: mocks.syncRegisteredProjectWorktrees,
}));

vi.mock("@/desktop/main/services/kanbanService", () => ({
  syncActiveTaskPullRequests: mocks.syncActiveTaskPullRequests,
  syncActiveTaskPulls: mocks.syncActiveTaskPulls,
}));

vi.mock("@/lib/boardNotifier", () => ({
  broadcastBoardUpdate: mocks.broadcastBoardUpdate,
  broadcastBackgroundSyncReviewNeeded: mocks.broadcastBackgroundSyncReviewNeeded,
}));

describe("backgroundTaskSyncService", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.useFakeTimers();
    mocks.syncRegisteredProjectWorktrees.mockResolvedValue({
      worktreeTasks: [],
      registeredWorktrees: [],
      hooksSetup: [],
      errors: [],
      changed: false,
    });
    mocks.syncActiveTaskPullRequests.mockResolvedValue({
      updatedTaskIds: [],
      mergeEventKeys: [],
      mergedPullRequests: [],
    });
    mocks.syncActiveTaskPulls.mockResolvedValue({
      pulledTasks: [],
    });
  });

  it("background sync review 대상이 있으면 통합 review event를 브로드캐스트한다", async () => {
    mocks.syncRegisteredProjectWorktrees.mockResolvedValue({
      worktreeTasks: ["feature-sync"],
      registeredWorktrees: [
        {
          taskId: "task-worktree",
          projectName: "api",
          branchName: "feature-sync",
          worktreePath: "/workspace/api__worktrees/feature-sync",
          sshHost: null,
        },
      ],
      hooksSetup: [],
      errors: [],
      changed: true,
    });
    mocks.syncActiveTaskPullRequests.mockResolvedValue({
      updatedTaskIds: [],
      mergeEventKeys: ["task-10:https://github.com/kanvibe/kanvibe/pull/211:2026-04-30T02:00:00Z"],
      mergedPullRequests: [
        {
          taskId: "task-10",
          taskTitle: "Merged PR task",
          branchName: "feature/merged-pr",
          prUrl: "https://github.com/kanvibe/kanvibe/pull/211",
          mergedAt: "2026-04-30T02:00:00Z",
        },
      ],
    });

    const { startBackgroundTaskSync } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);

    expect(mocks.broadcastBackgroundSyncReviewNeeded).toHaveBeenCalledWith({
      registeredWorktrees: [
        {
          taskId: "task-worktree",
          projectName: "api",
          branchName: "feature-sync",
          worktreePath: "/workspace/api__worktrees/feature-sync",
          sshHost: null,
        },
      ],
      mergedPullRequests: [
        {
          taskId: "task-10",
          taskTitle: "Merged PR task",
          branchName: "feature/merged-pr",
          prUrl: "https://github.com/kanvibe/kanvibe/pull/211",
          mergedAt: "2026-04-30T02:00:00Z",
        },
      ],
      pulledTasks: [],
    });

    stop();
  });

  it("background task sync는 여러 번 시작해도 하나의 loop만 실행한다", async () => {
    const { startBackgroundTaskSync } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stopA = startBackgroundTaskSync();
    const stopB = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(1);

    stopA();
    stopB();
  });

  it("task pull 결과가 있으면 background sync review event에 포함한다", async () => {
    mocks.syncActiveTaskPulls.mockResolvedValue({
      pulledTasks: [
        {
          taskId: "task-pull",
          taskTitle: "Pull target",
          branchName: "feature/pull",
          worktreePath: "/workspace/repo__worktrees/feature-pull",
          sshHost: null,
          status: "updated",
          summary: "Fast-forward",
        },
      ],
    });

    const { startBackgroundTaskSync } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);

    expect(mocks.broadcastBackgroundSyncReviewNeeded).toHaveBeenCalledWith({
      registeredWorktrees: [],
      mergedPullRequests: [],
      pulledTasks: [
        {
          taskId: "task-pull",
          taskTitle: "Pull target",
          branchName: "feature/pull",
          worktreePath: "/workspace/repo__worktrees/feature-pull",
          sshHost: null,
          status: "updated",
          summary: "Fast-forward",
        },
      ],
    });

    stop();
  });
});
