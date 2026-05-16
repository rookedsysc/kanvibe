import { syncRegisteredProjectWorktrees } from "@/desktop/main/services/projectService";
import { syncActiveTaskPullRequests, syncActiveTaskPulls } from "@/desktop/main/services/kanbanService";
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
        const worktreeSyncResult = await syncRegisteredProjectWorktrees();
        const prSyncResult = await syncActiveTaskPullRequests(emittedMergeEventKeys);
        const pullSyncResult = await syncActiveTaskPulls();
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
