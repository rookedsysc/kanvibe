import { EventEmitter } from "node:events";

function getInternalBroadcastUrl() {
  const port = process.env.PORT || "4885";
  const configuredHost = process.env.KANVIBE_HOST;
  const host = configuredHost && configuredHost !== "0.0.0.0" ? configuredHost : "127.0.0.1";

  return `http://${host}:${port}/_internal/broadcast`;
}

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
  projectName: string;
  branchName: string;
  requestedStatus: string;
  reason: "project-not-found" | "task-not-found";
}

export type BoardEventPayload =
  | BoardUpdatedPayload
  | ({ type: "task-status-changed" } & TaskStatusChangedPayload)
  | ({ type: "hook-status-target-missing" } & HookStatusTargetMissingPayload);

const boardEventEmitter = new EventEmitter();
interface LegacyBoardClient {
  readyState: number;
  OPEN: number;
  send: (message: string) => void;
}

const legacyBoardClients = new Set<LegacyBoardClient>();

export function addBoardClient(client: LegacyBoardClient) {
  legacyBoardClients.add(client);
}

export function removeBoardClient(client: LegacyBoardClient) {
  legacyBoardClients.delete(client);
}

export function getBoardClients(): Set<LegacyBoardClient> {
  return legacyBoardClients;
}

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

  fetch(getInternalBroadcastUrl(), {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  }).catch(() => {
    // 보드 업데이트 전파는 실패해도 호출부를 깨뜨리지 않는다.
  });
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
