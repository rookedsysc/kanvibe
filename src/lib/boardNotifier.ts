import type { WebSocket } from "ws";

/** 보드 업데이트 알림을 수신할 WebSocket 클라이언트 목록 */
const boardClients = new Set<WebSocket>();

export function addBoardClient(ws: WebSocket) {
  boardClients.add(ws);
}

export function removeBoardClient(ws: WebSocket) {
  boardClients.delete(ws);
}

/** server.ts에서 직접 WS 클라이언트에 접근할 때 사용한다 */
export function getBoardClients(): Set<WebSocket> {
  return boardClients;
}

const BROADCAST_URL = `http://localhost:${process.env.PORT || 4885}/_internal/broadcast`;

/**
 * 내부 HTTP 요청으로 custom server에 broadcast를 위임한다.
 * Turbopack이 API route를 별도 워커에서 실행하므로 in-memory Set이 공유되지 않는다.
 */
function sendBroadcast(message: string) {
  fetch(BROADCAST_URL, {
    method: "POST",
    body: message,
    headers: { "Content-Type": "application/json" },
  }).catch(() => {
    /* 연결 실패 무시 (서버 시작 전 등) */
  });
}

/** 연결된 모든 보드 클라이언트에 업데이트 알림을 전송한다 */
export function broadcastBoardUpdate() {
  sendBroadcast(JSON.stringify({ type: "board-updated" }));
}

export interface TaskStatusChangedPayload {
  projectName: string;
  branchName: string;
  taskTitle: string;
  description: string | null;
  newStatus: string;
  taskId: string;
}

/** hooks 경유 상태 변경 시 task 상세 정보를 포함하여 브로드캐스트한다 */
export function broadcastTaskStatusChanged(payload: TaskStatusChangedPayload) {
  sendBroadcast(JSON.stringify({ type: "task-status-changed", ...payload }));
}
