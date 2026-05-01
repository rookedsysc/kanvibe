import { syncRegisteredProjectWorktrees } from "@/desktop/main/services/projectService";
import { syncActiveTaskPullRequests } from "@/desktop/main/services/kanbanService";
import { broadcastBackgroundSyncReviewNeeded, broadcastBoardUpdate } from "@/lib/boardNotifier";

const INITIAL_SYNC_DELAY_MS = 20_000;
const SYNC_INTERVAL_MS = 90_000;

export function startBackgroundTaskSync() {
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
    if (disposed || running) {
      scheduleNext(SYNC_INTERVAL_MS);
      return;
    }

    running = true;

    try {
      const worktreeSyncResult = await syncRegisteredProjectWorktrees();
      const prSyncResult = await syncActiveTaskPullRequests(emittedMergeEventKeys);

      if (worktreeSyncResult.registeredWorktrees.length > 0 || prSyncResult.mergedPullRequests.length > 0) {
        broadcastBackgroundSyncReviewNeeded({
          registeredWorktrees: worktreeSyncResult.registeredWorktrees,
          mergedPullRequests: prSyncResult.mergedPullRequests,
        });
      }

      if (worktreeSyncResult.changed || prSyncResult.updatedTaskIds.length > 0) {
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

  return () => {
    disposed = true;
    if (timeoutHandle) {
      clearTimeout(timeoutHandle);
      timeoutHandle = null;
    }
  };
}
