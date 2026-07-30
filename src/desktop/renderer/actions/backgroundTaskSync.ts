import { invokeDesktop } from "@/desktop/renderer/ipc";
import type { BackgroundTaskSyncRunResult } from "@/desktop/main/services/backgroundTaskSyncService";

export function runBackgroundTaskSyncNow(): Promise<BackgroundTaskSyncRunResult> {
  return invokeDesktop("backgroundTaskSync", "runBackgroundTaskSyncNow");
}
