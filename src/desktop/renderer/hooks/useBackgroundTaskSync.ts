import { useSyncExternalStore } from "react";
import { getBackgroundTaskSyncStatus, startBackgroundTaskSyncNow } from "@/desktop/renderer/actions/backgroundTaskSync";
import type { BackgroundTaskSyncStatus } from "@/desktop/shared/backgroundTaskSync";
import type { BoardEventPayload } from "@/lib/boardNotifier";

interface SyncSnapshot {
  status: BackgroundTaskSyncStatus | null;
  error: string | null;
  isStarting: boolean;
}

const initialSnapshot: SyncSnapshot = { status: null, error: null, isStarting: false };
const listeners = new Set<() => void>();
let snapshot = initialSnapshot;
let revision = 0;
let subscriptionEpoch = 0;
let disposeEvents: (() => void) | undefined;

function updateSnapshot(update: Partial<SyncSnapshot>) {
  snapshot = { ...snapshot, ...update };
  for (const listener of listeners) listener();
}

async function refresh() {
  const requestRevision = ++revision;
  try {
    const status = await getBackgroundTaskSyncStatus();
    if (listeners.size > 0 && requestRevision === revision) {
      updateSnapshot({ status, error: null });
    }
  } catch (reason) {
    if (listeners.size > 0 && requestRevision === revision) {
      updateSnapshot({ error: reason instanceof Error ? reason.message : String(reason) });
    }
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) {
    const epoch = ++subscriptionEpoch;
    disposeEvents = window.kanvibeDesktop?.onBoardEvent?.((event: BoardEventPayload) => {
      if (event.type !== "background-sync-status-changed" || epoch !== subscriptionEpoch) return;
      revision += 1;
      updateSnapshot({ status: event.status, error: null });
    });
    void refresh();
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      disposeEvents?.();
      disposeEvents = undefined;
      subscriptionEpoch += 1;
      revision += 1;
      snapshot = initialSnapshot;
    }
  };
}

function getSnapshot() {
  return snapshot;
}

async function start() {
  if (snapshot.isStarting || snapshot.status?.isRunning) return;
  updateSnapshot({ isStarting: true });
  const requestRevision = ++revision;
  const epoch = subscriptionEpoch;
  try {
    const status = await startBackgroundTaskSyncNow();
    if (listeners.size > 0 && requestRevision === revision) {
      updateSnapshot({ status, error: null });
    }
  } catch (reason) {
    if (listeners.size > 0 && requestRevision === revision) {
      updateSnapshot({ error: reason instanceof Error ? reason.message : String(reason) });
    }
  } finally {
    if (listeners.size > 0 && epoch === subscriptionEpoch) updateSnapshot({ isStarting: false });
  }
}

export function useBackgroundTaskSync() {
  const current = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  return { ...current, start, refresh };
}
