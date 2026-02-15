import type { WebSocket } from "ws";

/** 보드 업데이트 알림을 수신할 WebSocket 클라이언트 목록 */
const boardClients = new Set<WebSocket>();

export function addBoardClient(ws: WebSocket) {
  boardClients.add(ws);
}

export function removeBoardClient(ws: WebSocket) {
  boardClients.delete(ws);
}

/** 연결된 모든 보드 클라이언트에 업데이트 알림을 전송한다 */
export function broadcastBoardUpdate() {
  const message = JSON.stringify({ type: "board-updated" });
  for (const client of boardClients) {
    if (client.readyState === client.OPEN) {
      client.send(message);
    }
  }
}
