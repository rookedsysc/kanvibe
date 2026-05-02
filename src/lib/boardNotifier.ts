import { EventEmitter } from "node:events";

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

export interface TaskHookInstallFailedPayload {
  taskId: string;
  taskTitle: string;
  error: string;
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
  operation: "worktree-sync" | "pull-request-sync";
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
  | ({ type: "task-pr-merged-detected" } & TaskPrMergedDetectedPayload)
  | ({ type: "task-pr-merged-detected-batch" } & TaskPrMergedDetectedBatchPayload)
  | ({ type: "background-sync-review-needed" } & BackgroundSyncReviewPayload);

const boardEventEmitter = new EventEmitter();

export function subscribeToBoardEvents(
  listener: (payload: BoardEventPayload) => void,
): () => void {
  boardEventEmitter.on("event", listener);
  return () => {
    boardEventEmitter.off("event", listener);
  };
}

function emitBoardEvent(payload: BoardEventPayload) {
  boardEventEmitter.emit("event", payload);
}

/** 연결된 모든 데스크톱 렌더러에 업데이트 알림을 전송한다 */
export function broadcastBoardUpdate() {
  emitBoardEvent({ type: "board-updated" });
}

/** hooks 경유 상태 변경 시 task 상세 정보를 포함하여 브로드캐스트한다 */
export function broadcastTaskStatusChanged(payload: TaskStatusChangedPayload) {
  emitBoardEvent({ type: "task-status-changed", ...payload });
}

/** hooks 대상 조회 실패 시 미매칭 상황을 브로드캐스트한다 */
export function broadcastHookStatusTargetMissing(payload: HookStatusTargetMissingPayload) {
  emitBoardEvent({ type: "hook-status-target-missing", ...payload });
}

/** 새 태스크 생성 후 hooks 자동 설치가 실패하면 렌더러에 안내를 브로드캐스트한다 */
export function broadcastTaskHookInstallFailed(payload: TaskHookInstallFailedPayload) {
  emitBoardEvent({ type: "task-hook-install-failed", ...payload });
}

/** background PR sync가 merge 완료를 감지하면 보드 확인 알럿으로 전달한다 */
export function broadcastTaskPrMergedDetected(payload: TaskPrMergedDetectedPayload) {
  emitBoardEvent({ type: "task-pr-merged-detected", ...payload });
}

/** background PR sync 한 사이클에서 감지된 merged PR 목록을 한 번에 전달한다 */
export function broadcastTaskPrMergedDetectedBatch(payload: TaskPrMergedDetectedBatchPayload) {
  emitBoardEvent({ type: "task-pr-merged-detected-batch", ...payload });
}

/** background sync가 사용자 검토가 필요한 변경을 발견하면 review payload를 브로드캐스트한다 */
export function broadcastBackgroundSyncReviewNeeded(payload: BackgroundSyncReviewPayload) {
  emitBoardEvent({ type: "background-sync-review-needed", ...payload });
}
