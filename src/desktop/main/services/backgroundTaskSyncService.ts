import { syncRegisteredProjectWorktrees, type RegisteredProjectWorktreeSyncResult } from "@/desktop/main/services/projectService";
import {
  syncActiveTaskPullRequests,
  syncActiveTaskPulls,
  type ActiveTaskPullRequestSyncResult,
  type ActiveTaskPullSyncResult,
} from "@/desktop/main/services/kanbanService";
import {
  broadcastBackgroundSyncReviewNeeded,
  broadcastBoardUpdate,
  type BackgroundSyncFailurePayload,
} from "@/lib/boardNotifier";
import {
  getBackgroundSyncEnabled,
  getBackgroundSyncIntervalMs,
  registerBackgroundSyncIntervalChangedCallback,
  registerBackgroundSyncEnabledChangedCallback,
} from "@/desktop/main/services/appSettingsService";

const INITIAL_SYNC_DELAY_MS = 20_000;
const FALLBACK_SYNC_INTERVAL_MS = 10 * 60_000;

let activeBackgroundTaskSyncStop: (() => void) | null = null;

interface BackgroundSyncStageResult<T> {
  result: T;
  failure: BackgroundSyncFailurePayload | null;
}

function createEmptyWorktreeSyncResult(): RegisteredProjectWorktreeSyncResult {
  return {
    worktreeTasks: [],
    registeredWorktrees: [],
    hooksSetup: [],
    errors: [],
    changed: false,
  };
}

function createEmptyPullRequestSyncResult(): ActiveTaskPullRequestSyncResult {
  return {
    updatedTaskIds: [],
    mergeEventKeys: [],
    mergedPullRequests: [],
  };
}

function createEmptyTaskPullSyncResult(): ActiveTaskPullSyncResult {
  return {
    pulledTasks: [],
  };
}

function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === "string") {
    return error;
  }

  return String(error);
}

async function runBackgroundSyncStage<T>(
  stageName: string,
  operation: () => Promise<T>,
  createFallbackResult: () => T,
  createFailure: (reason: string) => BackgroundSyncFailurePayload,
): Promise<BackgroundSyncStageResult<T>> {
  try {
    return {
      result: await operation(),
      failure: null,
    };
  } catch (error) {
    console.error(`[background-task-sync] ${stageName} failed:`, error);

    return {
      result: createFallbackResult(),
      failure: createFailure(getErrorMessage(error)),
    };
  }
}

export function startBackgroundTaskSync() {
  if (activeBackgroundTaskSyncStop) {
    return activeBackgroundTaskSyncStop;
  }

  let disposed = false;
  let running = false;
  let timeoutHandle: ReturnType<typeof setTimeout> | null = null;
  const emittedMergeEventKeys = new Set<string>();

  function scheduleNext(delayMs: number) {
    if (disposed) {
      return;
    }

    timeoutHandle = setTimeout(() => {
      void runSyncCycle();
    }, delayMs);
  }

  async function runSyncCycle() {
    if (disposed) {
      return;
    }

    if (running) {
      return;
    }

    running = true;

    try {
      const isEnabled = await getBackgroundSyncEnabled();
      if (isEnabled) {
        const worktreeSync = await runBackgroundSyncStage(
          "worktree sync",
          syncRegisteredProjectWorktrees,
          createEmptyWorktreeSyncResult,
          (reason) => ({
            operation: "worktree-sync",
            target: "등록 프로젝트 worktree sync",
            reason,
          }),
        );
        const prSync = await runBackgroundSyncStage(
          "pull request sync",
          () => syncActiveTaskPullRequests(emittedMergeEventKeys),
          createEmptyPullRequestSyncResult,
          (reason) => ({
            operation: "pull-request-sync",
            target: "PR 상태 sync",
            reason,
          }),
        );
        const pullSync = await runBackgroundSyncStage(
          "task pull sync",
          syncActiveTaskPulls,
          createEmptyTaskPullSyncResult,
          (reason) => ({
            operation: "task-pull-sync",
            target: "active task pull sync",
            reason,
          }),
        );
        const worktreeSyncResult = worktreeSync.result;
        const prSyncResult = prSync.result;
        const pullSyncResult = pullSync.result;
        const failures: BackgroundSyncFailurePayload[] = [
          ...(worktreeSync.failure ? [worktreeSync.failure] : []),
          ...worktreeSyncResult.errors.map((reason) => ({
            operation: "worktree-sync" as const,
            target: "등록 프로젝트 worktree sync",
            reason,
          })),
          ...(prSync.failure ? [prSync.failure] : []),
          ...(prSyncResult.failures ?? []),
          ...(pullSync.failure ? [pullSync.failure] : []),
        ];

        if (
          worktreeSyncResult.registeredWorktrees.length > 0
          || prSyncResult.mergedPullRequests.length > 0
          || pullSyncResult.pulledTasks.length > 0
          || failures.length > 0
        ) {
          broadcastBackgroundSyncReviewNeeded({
            registeredWorktrees: worktreeSyncResult.registeredWorktrees,
            mergedPullRequests: prSyncResult.mergedPullRequests,
            pulledTasks: pullSyncResult.pulledTasks,
            ...(failures.length > 0 ? { failures } : {}),
          });
        }

        const hasUpdatedPulledTasks = pullSyncResult.pulledTasks.some((task) => task.status === "updated");
        if (worktreeSyncResult.changed || prSyncResult.updatedTaskIds.length > 0 || hasUpdatedPulledTasks) {
          broadcastBoardUpdate();
        }
      }
    } catch (error) {
      console.error("[background-task-sync] sync failed:", error);
    } finally {
      running = false;
      const intervalMs = await getBackgroundSyncIntervalMs().catch(() => FALLBACK_SYNC_INTERVAL_MS);
      scheduleNext(intervalMs);
    }
  }

  function reschedule(newIntervalMs: number) {
    if (disposed) return;
    // 사이클 실행 중이면 건너뜀 — finally 블록이 최신 주기를 읽어 스케줄링함
    if (running) return;
    if (timeoutHandle) {
      clearTimeout(timeoutHandle);
      timeoutHandle = null;
    }
    scheduleNext(newIntervalMs);
  }

  function rescheduleOnEnable(enabled: boolean) {
    if (!enabled) return; // 비활성화 시: 루프는 다음 웨이크업에서 작업을 건너뜀
    if (disposed) return;
    if (running) return; // 사이클 진행 중: finally 블록이 이어서 스케줄링함
    if (timeoutHandle) {
      clearTimeout(timeoutHandle);
      timeoutHandle = null;
    }
    scheduleNext(INITIAL_SYNC_DELAY_MS);
  }

  registerBackgroundSyncIntervalChangedCallback(reschedule);
  registerBackgroundSyncEnabledChangedCallback(rescheduleOnEnable);
  scheduleNext(INITIAL_SYNC_DELAY_MS);

  function stopBackgroundTaskSync() {
    disposed = true;
    if (timeoutHandle) {
      clearTimeout(timeoutHandle);
      timeoutHandle = null;
    }
    if (activeBackgroundTaskSyncStop === stopBackgroundTaskSync) {
      activeBackgroundTaskSyncStop = null;
    }
  }

  activeBackgroundTaskSyncStop = stopBackgroundTaskSync;
  return activeBackgroundTaskSyncStop;
}
