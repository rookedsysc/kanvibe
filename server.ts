import "reflect-metadata";
import { createServer } from "http";
import { parse } from "url";
import next from "next";
import { WebSocketServer, type WebSocket } from "ws";
import { validateSessionFromCookie } from "@/lib/auth";
import { getTaskRepository } from "@/lib/database";
import { attachLocalSession, attachRemoteSession } from "@/lib/terminal";
import { parseSSHConfig } from "@/lib/sshConfig";

const dev = process.env.NODE_ENV !== "production";
const hostname = "0.0.0.0";
const port = parseInt(process.env.PORT || "4885", 10);

const app = next({ dev, hostname, port });
const handle = app.getRequestHandler();

app.prepare().then(() => {
  /**
   * Next.js가 getRequestHandler 내부에서 자동으로 upgrade 핸들러를 등록하지 못하게 차단한다.
   * 대신 getUpgradeHandler()로 직접 라우팅하여 이중 등록을 방지한다.
   */
  // @ts-expect-error didWebSocketSetup은 내부 프로퍼티
  app.didWebSocketSetup = true;

  const nextUpgrade = app.getUpgradeHandler();

  const server = createServer((req, res) => {
    const parsedUrl = parse(req.url!, true);
    handle(req, res, parsedUrl);
  });

  const wss = new WebSocketServer({ noServer: true });

  server.on("upgrade", (request, socket, head) => {
    const { pathname } = parse(request.url || "");

    /** 터미널 WebSocket만 직접 처리, 나머지(HMR 등)는 Next.js에 위임 */
    const terminalMatch = pathname?.match(/^\/api\/terminal\/([a-f0-9-]+)$/);
    if (!terminalMatch) {
      nextUpgrade(request, socket, head);
      return;
    }

    const cookieHeader = request.headers.cookie || "";
    const isAuthed = validateSessionFromCookie(cookieHeader);
    console.log(`[WS] 터미널 연결 요청: taskId=${terminalMatch[1]}, auth=${isAuthed}`);

    if (!isAuthed) {
      console.log("[WS] 인증 실패 — 쿠키:", cookieHeader ? "있음" : "없음");
      socket.write("HTTP/1.1 401 Unauthorized\r\n\r\n");
      socket.destroy();
      return;
    }

    wss.handleUpgrade(request, socket, head, (ws) => {
      wss.emit("connection", ws, request, terminalMatch[1]);
    });
  });

  wss.on("connection", async (ws: WebSocket, _request: unknown, taskId: string) => {
    try {
      const repo = await getTaskRepository();
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
          hostConfig
        );
      } else {
        await attachLocalSession(taskId, task.sessionType, task.sessionName, ws);
      }
    } catch (error) {
      console.error("터미널 연결 오류:", error);
      ws.close(1011, "터미널 연결 실패");
    }
  });

  server.listen(port, hostname, () => {
    console.log(`> KanVibe 서버 시작: http://${hostname}:${port}`);
  });
});
