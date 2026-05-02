import { syncRegisteredProjectWorktrees } from "@/desktop/main/services/projectService";
import { syncActiveTaskPullRequests, syncActiveTaskPulls } from "@/desktop/main/services/kanbanService";
import {
  broadcastBackgroundSyncReviewNeeded,
  broadcastBoardUpdate,
  type BackgroundSyncFailurePayload,
} from "@/lib/boardNotifier";

const INITIAL_SYNC_DELAY_MS = 20_000;
const SYNC_INTERVAL_MS = 10 * 60_000;

let activeBackgroundTaskSyncStop: (() => void) | null = null;

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
      const [
        worktreeSyncResult,
        prSyncResult,
        pullSyncResult,
      ] = await Promise.all([
        syncRegisteredProjectWorktrees(),
        syncActiveTaskPullRequests(emittedMergeEventKeys),
        syncActiveTaskPulls(),
      ]);
      const failures: BackgroundSyncFailurePayload[] = [
        ...worktreeSyncResult.errors.map((reason) => ({
          operation: "worktree-sync" as const,
          target: "등록 프로젝트 worktree sync",
          reason,
        })),
        ...(prSyncResult.failures ?? []),
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
    } catch (error) {
      console.error("[background-task-sync] sync failed:", error);
    } finally {
      running = false;
      scheduleNext(SYNC_INTERVAL_MS);
    }
  }

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
