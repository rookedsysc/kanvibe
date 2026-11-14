export interface BackgroundTaskSyncStatus {
  phase: "idle" | "worktrees" | "pull-requests" | "pulls" | "completed" | "failed";
  isRunning: boolean;
  completedStages: number;
  reviewNeeded: boolean;
  boardUpdated: boolean;
  error: string | null;
}
