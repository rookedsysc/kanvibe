import { createServer } from "http";
import { parse } from "url";
import { WebSocketServer, type WebSocket } from "ws";
import { getTaskRepository } from "../database";
import { attachLocalSession, attachRemoteSession } from "@/lib/terminal";
import { parseSSHConfig } from "@/lib/sshConfig";

/** 보드 업데이트 알림을 수신할 WebSocket 클라이언트 목록 */
const boardClients = new Set<WebSocket>();

export function addBoardClient(ws: WebSocket): void {
  boardClients.add(ws);
}

export function removeBoardClient(ws: WebSocket): void {
  boardClients.delete(ws);
}

export function getBoardClients(): Set<WebSocket> {
  return boardClients;
}

/**
 * 터미널 + 보드 알림 WebSocket 서버를 시작한다.
 * 기존 server.ts의 WebSocket 로직을 그대로 이전한 것이다.
 * 데스크탑 앱이므로 인증 검증은 제거되었다.
 */
export function startTerminalServer(port: number): void {
  const wsHttpServer = createServer();
  const wss = new WebSocketServer({ server: wsHttpServer });

  wss.on("connection", async (ws: WebSocket, request) => {
    const parsed = parse(request.url || "", true);

    /** 보드 알림 WebSocket 연결 */
    if (parsed.pathname === "/api/board/events") {
      addBoardClient(ws);
      ws.on("close", () => removeBoardClient(ws));
      return;
    }

    /** 터미널 WebSocket 연결: /api/terminal/{taskId} */
    const taskIdMatch = parsed.pathname?.match(/^\/api\/terminal\/([a-f0-9-]+)$/);
    if (!taskIdMatch) {
      ws.close(1008, "잘못된 경로");
      return;
    }

    const taskId = taskIdMatch[1];
    const initialCols = parseInt(parsed.query.cols as string, 10) || 120;
    const initialRows = parseInt(parsed.query.rows as string, 10) || 30;

    console.log(`[WS] 터미널 연결 요청: taskId=${taskId}`);

    try {
      const repo = getTaskRepository();
      const task = await repo.findOneBy({ id: taskId });

      if (!task || !task.sessionType || !task.sessionName) {
        ws.close(1008, "작업에 연결된 세션이 없습니다.");
        return;
      }

      if (task.sshHost) {
        const sshHosts = await parseSSHConfig();
        const hostConfig = sshHosts.find((h) => h.host === task.sshHost);

        if (!hostConfig) {
          ws.close(1008, `SSH 호스트를 찾을 수 없습니다: ${task.sshHost}`);
          return;
        }

        await attachRemoteSession(
          taskId,
          task.sshHost,
          task.sessionType,
          task.sessionName,
          ws,
          hostConfig,
          initialCols,
          initialRows,
        );
      } else {
        await attachLocalSession(
          taskId,
          task.sessionType,
          task.sessionName,
          ws,
          task.worktreePath,
          initialCols,
          initialRows,
        );
      }
    } catch (error) {
      console.error("터미널 연결 오류:", error);
      ws.close(1011, "터미널 연결 실패");
    }
  });

  wsHttpServer.listen(port, "0.0.0.0", () => {
    console.log(`> 터미널 WebSocket 서버: ws://0.0.0.0:${port}`);
  });
}
