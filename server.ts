import "reflect-metadata";
import { createServer } from "http";
import { parse } from "url";
import next from "next";
import { WebSocketServer, type WebSocket } from "ws";
import { validateSessionFromCookie } from "@/lib/auth";
import { getTaskRepository } from "@/lib/database";
import { attachLocalSession, attachRemoteSession } from "@/lib/terminal";
import { formatWindowName } from "@/lib/worktree";
import { parseSSHConfig } from "@/lib/sshConfig";

/**
 * node-pty의 ThreadSafeFunction 콜백이 실패하면 C++ 레벨에서
 * Napi::Error를 throw하므로 프로세스가 abort된다.
 * uncaughtException 핸들러로 가능한 범위의 에러를 로깅하고 프로세스 유지를 시도한다.
 */
process.on("uncaughtException", (error) => {
  console.error("[uncaughtException]", error);
});

const dev = process.env.NODE_ENV !== "production";
const hostname = "0.0.0.0";
const port = parseInt(process.env.PORT || "4885", 10);
const wsPort = port + 10000;

/**
 * Next.js에 httpServer를 전달하여 HMR WebSocket 등 내부 기능이
 * 커스텀 서버를 통해 동작하도록 한다.
 */
const server = createServer();
const app = next({ dev, hostname, port, httpServer: server });
const handle = app.getRequestHandler();

app.prepare().then(() => {
  /**
   * Next.js HTTP 요청 핸들러 등록.
   * 페이지, API, HMR 등 모든 HTTP 요청을 Next.js에 위임한다.
   */
  server.on("request", (req, res) => {
    const parsedUrl = parse(req.url!, true);
    handle(req, res, parsedUrl);
  });

  /**
   * 터미널 WebSocket 전용 서버.
   * Next.js 16 Turbopack이 같은 포트의 WebSocket upgrade를 내부적으로 가로채므로
   * 별도 포트(PORT + 1)에서 독립 운영한다.
   */
  const wsHttpServer = createServer();
  const wss = new WebSocketServer({ server: wsHttpServer });

  wss.on("connection", async (ws: WebSocket, request) => {
    const parsed = parse(request.url || "", true);
    const taskIdMatch = parsed.pathname?.match(/^\/api\/terminal\/([a-f0-9-]+)$/);

    if (!taskIdMatch) {
      ws.close(1008, "잘못된 경로");
      return;
    }

    const taskId = taskIdMatch[1];
    const initialCols = parseInt(parsed.query.cols as string, 10) || 120;
    const initialRows = parseInt(parsed.query.rows as string, 10) || 30;
    const cookieHeader = request.headers.cookie || "";
    const isAuthed = validateSessionFromCookie(cookieHeader);
    console.log(`[WS] 터미널 연결 요청: taskId=${taskId}, auth=${isAuthed}`);

    if (!isAuthed) {
      console.log("[WS] 인증 실패 — 쿠키:", cookieHeader ? "있음" : "없음");
      ws.close(1008, "인증 실패");
      return;
    }

    try {
      const repo = await getTaskRepository();
      const task = await repo.findOneBy({ id: taskId });

      if (!task || !task.sessionType || !task.sessionName) {
        ws.close(1008, "작업에 연결된 세션이 없습니다.");
        return;
      }

      /** branchName 또는 baseBranch에서 window/tab 이름을 파생한다 */
      const derivedBranch = task.branchName || task.baseBranch;
      const windowName = derivedBranch ? formatWindowName(derivedBranch) : "";

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
          windowName,
          ws,
          hostConfig,
          initialCols,
          initialRows
        );
      } else {
        await attachLocalSession(taskId, task.sessionType, task.sessionName, windowName, ws, task.worktreePath, initialCols, initialRows);
      }
    } catch (error) {
      console.error("터미널 연결 오류:", error);
      ws.close(1011, "터미널 연결 실패");
    }
  });

  server.listen(port, hostname, () => {
    console.log(`> KanVibe 서버 시작: http://${hostname}:${port}`);
  });

  wsHttpServer.listen(wsPort, hostname, () => {
    console.log(`> 터미널 WebSocket 서버: ws://${hostname}:${wsPort}`);
  });
});
