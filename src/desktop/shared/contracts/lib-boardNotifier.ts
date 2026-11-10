
export interface BoardUpdatedPayload {
  type: "board-updated";
}

export interface TaskStatusChangedPayload {
  projectName: string;
  branchName: string;
  taskTitle: string;
  description: string | null;
  newStatus: string;
  taskId: string;
}

export interface HookStatusTargetMissingPayload {
  taskId: string;
  requestedStatus: string;
  reason: "task-not-found";
}

/** task가 쓰는 터미널 세션이 바뀌었다. 이 task를 비추던 화면은 새 세션으로 다시 붙어야 한다 */
export interface TaskSessionChangedPayload {
  taskId: string;
}

export interface TaskHookInstallFailedPayload {
  taskId: string;
  taskTitle: string;
  error: string;
}

export interface TaskPeerDeletionFailedPayload {
  taskId: string;
  taskTitle: string;
  failedPeerUrls: string[];
}

export interface TaskPrMergedDetectedPayload {
  taskId: string;
  taskTitle: string;
  branchName: string;
  prUrl: string;
  mergedAt: string;
}

export interface TaskPrMergedDetectedBatchPayload {
  mergedPullRequests: TaskPrMergedDetectedPayload[];
}

export interface BackgroundSyncRegisteredWorktreePayload {
  taskId: string;
  projectName: string;
  branchName: string;
  worktreePath: string;
  sshHost: string | null;
}

export interface BackgroundSyncPulledTaskPayload {
  taskId: string;
  taskTitle: string;
  branchName: string;
  worktreePath: string;
  sshHost: string | null;
  status: "updated" | "failed";
  summary: string;
}

export interface BackgroundSyncFailurePayload {
  operation: "worktree-sync" | "pull-request-sync" | "task-pull-sync";
  target: string;
  reason: string;
  taskId?: string;
  branchName?: string;
  sshHost?: string | null;
}

export interface BackgroundSyncReviewPayload {
  mergedPullRequests: TaskPrMergedDetectedPayload[];
  registeredWorktrees: BackgroundSyncRegisteredWorktreePayload[];
  pulledTasks?: BackgroundSyncPulledTaskPayload[];
  failures?: BackgroundSyncFailurePayload[];
}

export type BoardEventPayload =
  | BoardUpdatedPayload
  | ({ type: "task-status-changed" } & TaskStatusChangedPayload)
  | ({ type: "hook-status-target-missing" } & HookStatusTargetMissingPayload)
  | ({ type: "task-hook-install-failed" } & TaskHookInstallFailedPayload)
  | ({ type: "task-session-changed" } & TaskSessionChangedPayload)
  | ({ type: "task-peer-deletion-failed" } & TaskPeerDeletionFailedPayload)
  | ({ type: "task-pr-merged-detected" } & TaskPrMergedDetectedPayload)
  | ({ type: "task-pr-merged-detected-batch" } & TaskPrMergedDetectedBatchPayload)
  | ({ type: "background-sync-review-needed" } & BackgroundSyncReviewPayload);
