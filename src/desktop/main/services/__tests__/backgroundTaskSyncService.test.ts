import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  syncRegisteredProjectWorktrees: vi.fn(),
  syncActiveTaskPullRequests: vi.fn(),
  syncActiveTaskPulls: vi.fn(),
  broadcastBoardUpdate: vi.fn(),
  broadcastBackgroundSyncReviewNeeded: vi.fn(),
  getBackgroundSyncEnabled: vi.fn().mockResolvedValue(true),
  getBackgroundSyncIntervalMs: vi.fn().mockResolvedValue(10 * 60_000),
  registerBackgroundSyncIntervalChangedCallback: vi.fn(),
  registerBackgroundSyncEnabledChangedCallback: vi.fn(),
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

vi.mock("@/desktop/main/services/appSettingsService", () => ({
  getBackgroundSyncEnabled: mocks.getBackgroundSyncEnabled,
  getBackgroundSyncIntervalMs: mocks.getBackgroundSyncIntervalMs,
  registerBackgroundSyncIntervalChangedCallback: mocks.registerBackgroundSyncIntervalChangedCallback,
  registerBackgroundSyncEnabledChangedCallback: mocks.registerBackgroundSyncEnabledChangedCallback,
}));

async function flushBackgroundSyncCycle() {
  for (let i = 0; i < 5; i += 1) {
    await vi.advanceTimersByTimeAsync(0);
  }
}

describe("backgroundTaskSyncService", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.useFakeTimers();
    mocks.getBackgroundSyncEnabled.mockResolvedValue(true);
    mocks.getBackgroundSyncIntervalMs.mockResolvedValue(10 * 60_000);
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

  it("manual background sync는 설정 비활성화와 무관하게 동일한 sync cycle을 실행한다", async () => {
    mocks.getBackgroundSyncEnabled.mockResolvedValue(false);

    const { runBackgroundTaskSyncNow } = await import("@/desktop/main/services/backgroundTaskSyncService");

    await runBackgroundTaskSyncNow();

    expect(mocks.getBackgroundSyncEnabled).not.toHaveBeenCalled();
    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(1);
  });

  it("manual background sync는 진행 중인 scheduled cycle과 같은 in-flight 작업을 공유한다", async () => {
    let resolveWorktreeSync: (result: {
      worktreeTasks: string[];
      registeredWorktrees: never[];
      hooksSetup: never[];
      errors: never[];
      changed: boolean;
    }) => void = () => {};
    mocks.syncRegisteredProjectWorktrees.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveWorktreeSync = resolve;
      }),
    );

    const { startBackgroundTaskSync, runBackgroundTaskSyncNow } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);

    const manualSync = runBackgroundTaskSyncNow();

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).not.toHaveBeenCalled();
    expect(mocks.syncActiveTaskPulls).not.toHaveBeenCalled();

    resolveWorktreeSync({
      worktreeTasks: [],
      registeredWorktrees: [],
      hooksSetup: [],
      errors: [],
      changed: false,
    });
    await manualSync;

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(1);

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

  it("실행 중인 background task sync cycle이 있으면 추가 cycle을 실행하지 않는다", async () => {
    let resolveWorktreeSync: (result: {
      worktreeTasks: string[];
      registeredWorktrees: never[];
      hooksSetup: never[];
      errors: never[];
      changed: boolean;
    }) => void = () => {};
    mocks.syncRegisteredProjectWorktrees.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveWorktreeSync = resolve;
      }),
    );

    const { startBackgroundTaskSync } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).not.toHaveBeenCalled();
    expect(mocks.syncActiveTaskPulls).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(10 * 60_000);

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).not.toHaveBeenCalled();
    expect(mocks.syncActiveTaskPulls).not.toHaveBeenCalled();

    resolveWorktreeSync({
      worktreeTasks: [],
      registeredWorktrees: [],
      hooksSetup: [],
      errors: [],
      changed: false,
    });
    await flushBackgroundSyncCycle();

    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(10 * 60_000);
    await flushBackgroundSyncCycle();

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(2);
    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(2);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(2);

    stop();
  });

  it("background task sync cycle은 고비용 sync 작업을 직렬로 실행한다", async () => {
    const calls: string[] = [];
    let resolveWorktreeSync: (result: {
      worktreeTasks: string[];
      registeredWorktrees: never[];
      hooksSetup: never[];
      errors: never[];
      changed: boolean;
    }) => void = () => {};
    let resolvePrSync: (result: {
      updatedTaskIds: string[];
      mergeEventKeys: string[];
      mergedPullRequests: never[];
    }) => void = () => {};

    mocks.syncRegisteredProjectWorktrees.mockImplementationOnce(
      () => new Promise((resolve) => {
        calls.push("worktree:start");
        resolveWorktreeSync = (result) => {
          calls.push("worktree:end");
          resolve(result);
        };
      }),
    );
    mocks.syncActiveTaskPullRequests.mockImplementationOnce(
      () => new Promise((resolve) => {
        calls.push("pr:start");
        resolvePrSync = (result) => {
          calls.push("pr:end");
          resolve(result);
        };
      }),
    );
    mocks.syncActiveTaskPulls.mockImplementationOnce(async () => {
      calls.push("pull:start");
      return { pulledTasks: [] };
    });

    const { startBackgroundTaskSync } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);
    await flushBackgroundSyncCycle();

    expect(calls).toEqual(["worktree:start"]);

    resolveWorktreeSync({
      worktreeTasks: [],
      registeredWorktrees: [],
      hooksSetup: [],
      errors: [],
      changed: false,
    });
    await vi.advanceTimersByTimeAsync(0);

    expect(calls).toEqual(["worktree:start", "worktree:end", "pr:start"]);

    resolvePrSync({
      updatedTaskIds: [],
      mergeEventKeys: [],
      mergedPullRequests: [],
    });
    await vi.advanceTimersByTimeAsync(0);

    expect(calls).toEqual(["worktree:start", "worktree:end", "pr:start", "pr:end", "pull:start"]);

    stop();
  });

  it("worktree sync 단계가 예외로 실패해도 PR sync와 pull sync를 계속 실행한다", async () => {
    mocks.syncRegisteredProjectWorktrees.mockRejectedValue(new Error("project repository unavailable"));
    mocks.syncActiveTaskPullRequests.mockResolvedValue({
      updatedTaskIds: ["task-pr"],
      mergeEventKeys: [],
      mergedPullRequests: [],
    });
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
    await flushBackgroundSyncCycle();

    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(1);
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
      failures: [
        {
          operation: "worktree-sync",
          target: "등록 프로젝트 worktree sync",
          reason: "project repository unavailable",
        },
      ],
    });
    expect(mocks.broadcastBoardUpdate).toHaveBeenCalledTimes(1);

    stop();
  });

  it("manual sync 진행 중에 주기가 바뀌면 새 주기로 다음 sync를 다시 예약한다", async () => {
    const { startBackgroundTaskSync, runBackgroundTaskSyncNow } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);
    await flushBackgroundSyncCycle();

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);

    let resolveManualWorktreeSync: () => void = () => {};
    mocks.syncRegisteredProjectWorktrees.mockReturnValueOnce(
      new Promise((resolve) => {
        resolveManualWorktreeSync = () => resolve({
          worktreeTasks: [],
          registeredWorktrees: [],
          hooksSetup: [],
          errors: [],
          changed: false,
        });
      }),
    );

    const manualSync = runBackgroundTaskSyncNow();
    await flushBackgroundSyncCycle();

    const notifyIntervalChanged = mocks.registerBackgroundSyncIntervalChangedCallback.mock.calls[0][0];
    mocks.getBackgroundSyncIntervalMs.mockResolvedValue(60_000);
    notifyIntervalChanged(60_000);

    resolveManualWorktreeSync();
    await manualSync;
    await flushBackgroundSyncCycle();

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(2);

    await vi.advanceTimersByTimeAsync(60_000);
    await flushBackgroundSyncCycle();

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(3);

    stop();
  });

  it("background task sync 갱신 주기는 최초 실행 후 10분이다", async () => {
    const { startBackgroundTaskSync } = await import("@/desktop/main/services/backgroundTaskSyncService");

    const stop = startBackgroundTaskSync();
    await vi.advanceTimersByTimeAsync(20_000);
    await flushBackgroundSyncCycle();

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(10 * 60_000 - 1);

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(1);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1);
    await flushBackgroundSyncCycle();

    expect(mocks.syncRegisteredProjectWorktrees).toHaveBeenCalledTimes(2);
    expect(mocks.syncActiveTaskPullRequests).toHaveBeenCalledTimes(2);
    expect(mocks.syncActiveTaskPulls).toHaveBeenCalledTimes(2);

    stop();
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
    await flushBackgroundSyncCycle();

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
      pulledTasks: [],
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
