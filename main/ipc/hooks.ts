import { createServer, type IncomingMessage, type ServerResponse } from "http";
import { BrowserWindow } from "electron";
import { getTaskRepository, getProjectRepository } from "../database";
import { TaskStatus, SessionType } from "@/entities/KanbanTask";
import { createWorktreeWithSession } from "@/lib/worktree";
import { getBoardClients } from "./terminal";
import { cleanupTaskResources } from "./kanban";

const STATUS_MAP: Record<string, TaskStatus> = {
  todo: TaskStatus.TODO,
  progress: TaskStatus.PROGRESS,
  pending: TaskStatus.PENDING,
  review: TaskStatus.REVIEW,
  done: TaskStatus.DONE,
};

/** 모든 렌더러 윈도우에 보드 새로고침 이벤트를 전송한다 */
function notifyRefresh(): void {
  BrowserWindow.getAllWindows().forEach((win) => {
    win.webContents.send("board:refresh");
  });
}

/** 연결된 모든 보드 WebSocket 클라이언트에 메시지를 전송한다 */
function broadcastToWsClients(message: string): void {
  const clients = getBoardClients();
  clients.forEach((client) => {
    if (client.readyState === 1) {
      client.send(message);
    }
  });
}

/** Request body를 JSON으로 파싱한다 */
function parseBody(req: IncomingMessage): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    req.on("data", (chunk: Buffer) => chunks.push(chunk));
    req.on("end", () => {
      try {
        const body = JSON.parse(Buffer.concat(chunks).toString());
        resolve(body);
      } catch {
        reject(new Error("Invalid JSON"));
      }
    });
    req.on("error", reject);
  });
}

/** JSON 응답을 전송한다 */
function sendJson(res: ServerResponse, status: number, data: unknown): void {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify(data));
}

/**
 * Hook API: 작업 시작.
 * AI 에이전트가 작업을 시작할 때 호출하여 progress 카드를 자동 생성한다.
 */
async function handleHooksStart(
  body: Record<string, unknown>,
  res: ServerResponse,
): Promise<void> {
  const { title, branchName, agentType, sessionType, sshHost, projectId, baseBranch } = body;

  if (!title) {
    sendJson(res, 400, { success: false, error: "title은 필수입니다." });
    return;
  }

  const repo = getTaskRepository();

  const task = repo.create({
    title: title as string,
    branchName: (branchName as string) || null,
    agentType: (agentType as string) || null,
    sessionType: sessionType ? (sessionType as SessionType) : null,
    sshHost: (sshHost as string) || null,
    projectId: (projectId as string) || null,
    baseBranch: (baseBranch as string) || null,
    status: TaskStatus.PROGRESS,
  });

  if (branchName && sessionType && projectId) {
    try {
      const projectRepo = getProjectRepository();
      const project = await projectRepo.findOneBy({ id: projectId as string });

      if (project) {
        const base = (baseBranch as string) || project.defaultBranch;
        const session = await createWorktreeWithSession(
          project.repoPath,
          branchName as string,
          base,
          sessionType as SessionType,
          project.sshHost,
          projectId as string,
        );
        task.baseBranch = base;
        task.worktreePath = session.worktreePath;
        task.sessionName = session.sessionName;
        task.sshHost = project.sshHost;
      }
    } catch (error) {
      console.error("Worktree/세션 생성 실패:", error);
    }
  }

  const saved = await repo.save(task);

  notifyRefresh();
  broadcastToWsClients(JSON.stringify({ type: "board-updated" }));

  sendJson(res, 200, {
    success: true,
    data: {
      id: saved.id,
      status: saved.status,
      sessionName: saved.sessionName,
    },
  });
}

/**
 * Hook API: branchName + projectName 기반 작업 상태 업데이트.
 * Claude Code hooks에서 호출하여 현재 브랜치의 작업 상태를 자동 변경한다.
 */
async function handleHooksStatus(
  body: Record<string, unknown>,
  res: ServerResponse,
): Promise<void> {
  const { branchName, projectName, status } = body;

  if (!branchName || !projectName || !status) {
    sendJson(res, 400, {
      success: false,
      error: "branchName, projectName, status는 필수입니다.",
    });
    return;
  }

  const taskStatus = STATUS_MAP[(status as string).toLowerCase()];
  if (!taskStatus) {
    sendJson(res, 400, {
      success: false,
      error: `유효하지 않은 상태입니다: ${status}`,
    });
    return;
  }

  const projectRepo = getProjectRepository();
  const project = await projectRepo.findOneBy({ name: projectName as string });

  if (!project) {
    sendJson(res, 404, {
      success: false,
      error: `프로젝트를 찾을 수 없습니다: ${projectName}`,
    });
    return;
  }

  const taskRepo = getTaskRepository();
  const task = await taskRepo.findOneBy({
    branchName: branchName as string,
    projectId: project.id,
  });

  if (!task) {
    sendJson(res, 404, {
      success: false,
      error: `작업을 찾을 수 없습니다: ${projectName}/${branchName}`,
    });
    return;
  }

  if (taskStatus === TaskStatus.DONE) {
    await cleanupTaskResources(task);
    task.sessionType = null;
    task.sessionName = null;
    task.worktreePath = null;
    task.sshHost = null;
  }

  task.status = taskStatus;
  const saved = await taskRepo.save(task);

  notifyRefresh();
  broadcastToWsClients(JSON.stringify({ type: "board-updated" }));
  broadcastToWsClients(
    JSON.stringify({
      type: "task-status-changed",
      projectName,
      branchName,
      taskTitle: saved.title,
      description: saved.description,
      newStatus: taskStatus,
      taskId: saved.id,
    }),
  );

  sendJson(res, 200, {
    success: true,
    data: { id: saved.id, status: saved.status, branchName, projectName },
  });
}

/**
 * 내부 broadcast 엔드포인트.
 * boardNotifier.ts에서 HTTP로 메시지를 전송받아 WS 클라이언트에 중계한다.
 */
async function handleBroadcast(
  body: Record<string, unknown>,
  res: ServerResponse,
): Promise<void> {
  broadcastToWsClients(JSON.stringify(body));
  notifyRefresh();
  sendJson(res, 200, { ok: true });
}

/**
 * 외부 AI 에이전트용 hooks mini HTTP 서버를 시작한다.
 * CLI 도구(Claude Code, Gemini CLI 등)가 HTTP로 호출하므로 IPC가 아닌 HTTP를 사용한다.
 */
export function startHooksServer(port: number): void {
  const server = createServer(async (req: IncomingMessage, res: ServerResponse) => {
    /** CORS 허용 (로컬 CLI 도구 호환) */
    res.setHeader("Access-Control-Allow-Origin", "*");
    res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    res.setHeader("Access-Control-Allow-Headers", "Content-Type");

    if (req.method === "OPTIONS") {
      res.writeHead(204);
      res.end();
      return;
    }

    const url = new URL(req.url || "/", `http://localhost:${port}`);

    try {
      if (req.method === "POST" && url.pathname === "/api/hooks/start") {
        const body = await parseBody(req);
        await handleHooksStart(body, res);
        return;
      }

      if (req.method === "POST" && url.pathname === "/api/hooks/status") {
        const body = await parseBody(req);
        await handleHooksStatus(body, res);
        return;
      }

      if (req.method === "POST" && url.pathname === "/_internal/broadcast") {
        const body = await parseBody(req);
        await handleBroadcast(body, res);
        return;
      }

      sendJson(res, 404, { error: "Not Found" });
    } catch (error) {
      console.error("Hooks 서버 오류:", error);
      sendJson(res, 500, { success: false, error: "서버 오류" });
    }
  });

  server.listen(port, "127.0.0.1", () => {
    console.log(`> Hooks HTTP 서버: http://127.0.0.1:${port}`);
  });
}
