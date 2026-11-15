import { useSyncExternalStore } from "react";
import { deleteTask } from "@/desktop/renderer/actions/kanban";
import type { AppNotification } from "@/desktop/shared/notifications";

type DeleteResult = { status: "pending" | "succeeded" } | { status: "failed"; reason: string };

interface ReviewSnapshot {
  notification: AppNotification | null;
  isOpen: boolean;
  isCleaning: boolean;
  selectedTaskIds: string[];
  results: Record<string, DeleteResult>;
}

let snapshot: ReviewSnapshot = { notification: null, isOpen: false, isCleaning: false, selectedTaskIds: [], results: {} };
const listeners = new Set<() => void>();

function updateSnapshot(update: Partial<ReviewSnapshot>) {
  snapshot = { ...snapshot, ...update };
  for (const listener of listeners) listener();
}

export function getBackgroundSyncReviewSnapshot() {
  return snapshot;
}

export function openBackgroundSyncReview(notification: AppNotification) {
  if (notification.action?.type !== "background-sync-review") return;
  if (snapshot.notification?.id !== notification.id && !snapshot.isCleaning) {
    updateSnapshot({ notification, selectedTaskIds: [...new Set(notification.action.payload.mergedPullRequests.map((task) => task.taskId))], results: {}, isOpen: true });
  } else {
    updateSnapshot({ isOpen: true });
  }
}

export function closeBackgroundSyncReview() {
  updateSnapshot({ isOpen: false });
}

export function toggleBackgroundSyncReviewTask(taskId: string) {
  if (snapshot.isCleaning || Object.keys(snapshot.results).length > 0) return;
  const selectedTaskIds = snapshot.selectedTaskIds.includes(taskId)
    ? snapshot.selectedTaskIds.filter((id) => id !== taskId)
    : [...snapshot.selectedTaskIds, taskId];
  updateSnapshot({ selectedTaskIds });
}

export function getBackgroundSyncReviewCount(notification: AppNotification | null) {
  if (notification?.action?.type !== "background-sync-review") return 0;
  const review = notification.action.payload;
  const deleted = snapshot.notification?.id === notification.id
    ? review.mergedPullRequests.filter((task) => snapshot.results[task.taskId]?.status === "succeeded").length
    : 0;
  return review.mergedPullRequests.length + review.registeredWorktrees.length + (review.pulledTasks?.length ?? 0) + (review.failures?.length ?? 0) - deleted;
}

export async function deleteBackgroundSyncReviewTasks(retryFailed = false) {
  if (snapshot.isCleaning || !snapshot.notification) return;
  const taskIds = retryFailed
    ? Object.entries(snapshot.results).filter(([, result]) => result.status === "failed").map(([taskId]) => taskId)
    : snapshot.selectedTaskIds.filter((id) => snapshot.results[id]?.status !== "succeeded");
  if (taskIds.length === 0) { closeBackgroundSyncReview(); return; }
  const results = { ...snapshot.results };
  for (const taskId of taskIds) results[taskId] = { status: "pending" };
  updateSnapshot({ isOpen: false, isCleaning: true, results });
  try {
    for (const taskId of taskIds) {
      let result: DeleteResult;
      try {
        await deleteTask(taskId);
        result = { status: "succeeded" };
      } catch (error) {
        result = { status: "failed", reason: error instanceof Error ? error.message : String(error) };
      }
      updateSnapshot({ results: { ...snapshot.results, [taskId]: result } });
    }
  } finally {
    updateSnapshot({ isCleaning: false });
  }
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function useBackgroundSyncReview() {
  return useSyncExternalStore(subscribe, getBackgroundSyncReviewSnapshot, getBackgroundSyncReviewSnapshot);
}
