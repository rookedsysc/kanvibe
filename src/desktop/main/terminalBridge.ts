import { EventEmitter } from "node:events";
import type { WebContents } from "electron";
import { getTaskRepository } from "@/lib/database";
import { SessionType } from "@/entities/KanbanTask";
import { attachLocalSession, attachRemoteSession, focusSession } from "@/lib/terminal";
import { parseSSHConfig } from "@/lib/sshConfig";
import { ensureRemoteSessionDependency } from "@/lib/remoteSessionDependency";
import { getEffectivePaneLayout } from "@/desktop/main/services/paneLayoutService";

const OPEN = 1;
const CLOSED = 3;

class ElectronTerminalClient extends EventEmitter {
  readonly OPEN = OPEN;
  readyState = OPEN;
  /**
   * 동일 (webContentsId, taskId) 조합으로 openTerminal이 중첩 호출되는 횟수를 추적한다.
   * React StrictMode dev 환경에서 useEffect가 mount → cleanup → remount로 두 번 도는 경우,
   * 첫 mount의 cleanup이 두 번째 mount가 사용 중인 PTY를 죽이지 않도록 보호한다.
   */
  refCount = 1;

  constructor(
    private readonly webContents: WebContents,
    private readonly taskId: string,
  ) {
    super();
  }

  send(data: string | Buffer) {
    if (this.readyState !== OPEN || this.webContents.isDestroyed()) {
      return;
    }

    this.webContents.send("kanvibe:terminal-data", {
      taskId: this.taskId,
      data: typeof data === "string" ? data : data.toString(),
    });
  }

  close(_code?: number, reason?: string) {
    if (this.readyState !== OPEN) {
      return;
    }

    this.readyState = CLOSED;
    this.emit("close");

    if (!this.webContents.isDestroyed()) {
      this.webContents.send("kanvibe:terminal-close", {
        taskId: this.taskId,
        reason: reason ?? null,
      });
    }
  }

  emitMessage(message: string) {
    if (this.readyState !== OPEN) {
      return;
    }

    this.emit("message", Buffer.from(message));
  }
}

const terminalClients = new Map<string, ElectronTerminalClient>();

function buildClientKey(webContentsId: number, taskId: string): string {
  return `${webContentsId}:${taskId}`;
}

function getClient(webContentsId: number, taskId: string): ElectronTerminalClient | null {
  return terminalClients.get(buildClientKey(webContentsId, taskId)) ?? null;
}

export async function openTerminal(
  webContents: WebContents,
  taskId: string,
  cols: number,
  rows: number,
) {
  const existingClient = getClient(webContents.id, taskId);
  if (existingClient) {
    existingClient.refCount += 1;
    existingClient.emitMessage(`\x01${JSON.stringify({ type: "resize", cols, rows })}`);
    return { ok: true };
  }

  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOneBy({ id: taskId });

  if (!task || !task.sessionType || !task.sessionName) {
    return { ok: false, error: "작업에 연결된 세션이 없습니다." };
  }

  const client = new ElectronTerminalClient(webContents, taskId);
  terminalClients.set(buildClientKey(webContents.id, taskId), client);

  const finalizeClient = () => {
    terminalClients.delete(buildClientKey(webContents.id, taskId));
  };

  client.once("close", finalizeClient);

  try {
    const tmuxPaneLayout = task.sessionType === SessionType.TMUX
      ? await getEffectivePaneLayout(task.projectId ?? undefined)
      : null;

    if (task.sshHost) {
      await ensureRemoteSessionDependency(task.sessionType as SessionType, task.sshHost);

      const sshHosts = await parseSSHConfig();
      const sshConfig = sshHosts.find((host) => host.host === task.sshHost);

      if (!sshConfig) {
        client.close(1008, `SSH 호스트를 찾을 수 없습니다: ${task.sshHost}`);
        return { ok: false, error: `SSH 호스트를 찾을 수 없습니다: ${task.sshHost}` };
      }

      await attachRemoteSession(
        taskId,
        task.sshHost,
        task.sessionType as SessionType,
        task.sessionName,
        client as never,
        sshConfig,
        cols,
        rows,
        task.worktreePath,
        tmuxPaneLayout,
      );
    } else {
      await attachLocalSession(
        taskId,
        task.sessionType as SessionType,
        task.sessionName,
        client as never,
        task.worktreePath,
        cols,
        rows,
        tmuxPaneLayout,
      );
    }

    return { ok: true };
  } catch (error) {
    finalizeClient();
    client.close(1011, "터미널 연결 실패");
    return {
      ok: false,
      error: error instanceof Error ? error.message : "터미널 연결 실패",
    };
  }
}

export function writeTerminal(webContentsId: number, taskId: string, data: string) {
  getClient(webContentsId, taskId)?.emitMessage(data);
}

export function resizeTerminal(webContentsId: number, taskId: string, cols: number, rows: number) {
  getClient(webContentsId, taskId)?.emitMessage(`\x01${JSON.stringify({ type: "resize", cols, rows })}`);
}

export function focusTerminal(taskId: string) {
  focusSession(taskId);
}

export function closeTerminal(webContentsId: number, taskId: string) {
  const client = getClient(webContentsId, taskId);
  if (!client) {
    return;
  }

  client.refCount -= 1;
  if (client.refCount > 0) {
    return;
  }

  client.close();
}

/** 윈도우(webContents) 자체가 destroy되는 경로이므로 refCount와 무관하게 모든 client를 강제 정리한다 */
export function closeWindowTerminals(webContentsId: number) {
  for (const [key, client] of terminalClients.entries()) {
    if (key.startsWith(`${webContentsId}:`)) {
      client.refCount = 0;
      client.close();
    }
  }
}
