import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { BackgroundTaskSyncStatus } from "@/desktop/shared/backgroundTaskSync";

export function getBackgroundTaskSyncStatus(): Promise<BackgroundTaskSyncStatus> {
  return invokeDesktop("backgroundTaskSync", "getBackgroundTaskSyncStatus");
}

export function startBackgroundTaskSyncNow(): Promise<BackgroundTaskSyncStatus> {
  return invokeDesktop("backgroundTaskSync", "startBackgroundTaskSyncNow");
}
