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

export type BoardEventPayload =
  | BoardUpdatedPayload
  | ({ type: "task-status-changed" } & TaskStatusChangedPayload)
  | ({ type: "hook-status-target-missing" } & HookStatusTargetMissingPayload)
  | ({ type: "task-hook-install-failed" } & TaskHookInstallFailedPayload);

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
