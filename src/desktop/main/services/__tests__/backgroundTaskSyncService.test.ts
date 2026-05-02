import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  syncRegisteredProjectWorktrees: vi.fn(),
  syncActiveTaskPullRequests: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
  broadcastBackgroundSyncReviewNeeded: vi.fn(),
}));

vi.mock("@/desktop/main/services/projectService", () => ({
  syncRegisteredProjectWorktrees: mocks.syncRegisteredProjectWorktrees,
}));

vi.mock("@/desktop/main/services/kanbanService", () => ({
  syncActiveTaskPullRequests: mocks.syncActiveTaskPullRequests,
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
    });

    stop();
  });

  it("background sync 실패만 있어도 대상과 이유를 review event로 브로드캐스트한다", async () => {
    mocks.syncRegisteredProjectWorktrees.mockResolvedValue({
      worktreeTasks: [],
      registeredWorktrees: [],
      hooksSetup: [],
      errors: ["api worktree 스캔 실패: git fetch failed"],
      changed: false,
    });
    mocks.syncActiveTaskPullRequests.mockResolvedValue({
      updatedTaskIds: [],
      mergeEventKeys: [],
      mergedPullRequests: [],
      failures: [
        {
          operation: "pull-request-sync",
          target: "PR sync target (feature/pr-fail)",
          reason: "gh auth failed",
          taskId: "task-11",
          branchName: "feature/pr-fail",
        },
      ],
    });

    const { startBackgroundTaskSync } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);

    expect(mocks.broadcastBackgroundSyncReviewNeeded).toHaveBeenCalledWith({
      registeredWorktrees: [],
      mergedPullRequests: [],
      failures: [
        {
          operation: "worktree-sync",
          target: "등록 프로젝트 worktree sync",
          reason: "api worktree 스캔 실패: git fetch failed",
        },
        {
          operation: "pull-request-sync",
          target: "PR sync target (feature/pr-fail)",
          reason: "gh auth failed",
          taskId: "task-11",
          branchName: "feature/pr-fail",
        },
      ],
    });

    stop();
  });
});
