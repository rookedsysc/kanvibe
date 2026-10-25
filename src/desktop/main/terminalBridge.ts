import { EventEmitter } from "node:events";
import type { WebContents } from "electron";
import { getTaskRepository } from "@/lib/database";
import { SessionType, type KanbanTask } from "@/entities/KanbanTask";
import { attachLocalSession, attachRemoteSession, focusSession } from "@/lib/terminal";
import { parseSSHConfig, type SSHHostConfig } from "@/lib/sshConfig";
import { ensureRemoteSessionDependency } from "@/lib/remoteSessionDependency";
import { getEffectivePaneLayout } from "@/desktop/main/services/paneLayoutService";
import { decodeDataUrlToBuffer, transferImageToRemoteHost } from "@/lib/remoteImagePaste";

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
    private readonly tabId: string | null,
  ) {
    super();
  }

  send(data: string | Buffer) {
    if (this.readyState !== OPEN || this.webContents.isDestroyed()) {
      return;
    }

    this.webContents.send("kanvibe:terminal-data", {
      taskId: this.taskId,
      tabId: this.tabId,
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
        tabId: this.tabId,
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

/**
 * 화면 없이 세션에 붙는 클라이언트. 받은 출력을 콜백으로 넘긴다.
 *
 * 이미 떠 있는 PTY에 붙을 때는 지금 화면을 먼저 그려야 하는데, 그 화면이 준비되는 사이에도 출력이 들어온다.
 * [holdOutput]으로 그 출력을 모아 두었다가 [releaseHeldOutput]이 화면 뒤에 이어 보낸다.
 */
export class CallbackTerminalClient extends EventEmitter {
  readonly OPEN = OPEN;
  readyState = OPEN;
  private heldChunks: string[] | null = null;

  constructor(private readonly onChunk: (chunk: string) => void) {
    super();
  }

  send(data: string | Buffer) {
    const chunk = typeof data === "string" ? data : data.toString();
    if (this.heldChunks) {
      this.heldChunks.push(chunk);
      return;
    }
    this.onChunk(chunk);
  }

  holdOutput() {
    this.heldChunks ??= [];
  }

  /** 화면을 먼저 보내고 모아 둔 출력을 이어 보낸다. 이후 출력은 곧바로 흘린다 */
  releaseHeldOutput(screen: string) {
    const heldChunks = this.heldChunks ?? [];
    this.heldChunks = null;

    if (screen) {
      this.onChunk(screen);
    }
    heldChunks.forEach((chunk) => this.onChunk(chunk));
  }

  close() {
    if (this.readyState !== OPEN) {
      return;
    }

    this.readyState = CLOSED;
    this.emit("close");
  }
}

const terminalClients = new Map<string, ElectronTerminalClient>();

/**
 * 렌더러는 멀티플렉서 세션에 null을, terminal 세션에 탭 식별자를 넘긴다.
 * 그래야 tmux·zellij는 창 하나가 PTY 하나를 공유하고, terminal 세션만 탭마다 스트림이 갈린다.
 */
function buildClientKey(webContentsId: number, taskId: string, tabId: string | null): string {
  return `${webContentsId}:${taskId}:${tabId ?? ""}`;
}

function getClient(
  webContentsId: number,
  taskId: string,
  tabId: string | null,
): ElectronTerminalClient | null {
  return terminalClients.get(buildClientKey(webContentsId, taskId, tabId)) ?? null;
}

/** taskId로 원격 세션 여부와 매칭되는 SSH 접속 정보를 함께 조회한다 */
async function resolveRemoteSshConfig(
  taskId: string,
): Promise<{ ok: true; sshConfig: SSHHostConfig } | { ok: false; error: string }> {
  const taskRepo = await getTaskRepository();
  const task = await taskRepo.findOneBy({ id: taskId });

  if (!task || !task.sshHost) {
    return { ok: false, error: "원격 세션이 아닙니다." };
  }

  const sshHosts = await parseSSHConfig();
  const sshConfig = sshHosts.find((host) => host.host === task.sshHost);

  if (!sshConfig) {
    return { ok: false, error: `SSH 호스트를 찾을 수 없습니다: ${task.sshHost}` };
  }

  return { ok: true, sshConfig };
}

/** 클립보드 이미지를 원격 세션에 scp로 전달하고, 성공하면 원격 경로를 반환한다 */
export async function pasteImageToRemoteTerminal(
  taskId: string,
  imageDataUrl: string,
): Promise<{ ok: true; remotePath: string } | { ok: false; error: string }> {
  const resolved = await resolveRemoteSshConfig(taskId);
  if (!resolved.ok) {
    return resolved;
  }

  try {
    const imageBuffer = decodeDataUrlToBuffer(imageDataUrl);
    const remotePath = await transferImageToRemoteHost(resolved.sshConfig, imageBuffer);
    return { ok: true, remotePath };
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : "이미지 전송 실패" };
  }
}

/** 세션에 붙는 클라이언트. 터미널 레지스트리는 WebSocket 중 이 부분만 쓴다 */
export interface TerminalSessionClient extends EventEmitter {
  readonly OPEN: number;
  readyState: number;
  send(data: string | Buffer): void;
  close(code?: number, reason?: string): void;
}

/**
 * 태스크의 세션에 클라이언트를 붙인다. 세션이 없으면 데스크탑이 처음 열 때와 똑같이 만든다.
 *
 * 데스크탑 화면과 모바일이 같은 함수를 지나야 pane 레이아웃, 원격 의존성 확인, tmux 부트스트랩이 한쪽에서만 달라지지 않는다.
 * 원격 접속 정보를 찾지 못하면 클라이언트를 닫고 던진다.
 */
export async function attachTaskTerminal(
  task: KanbanTask,
  tabId: string | null,
  client: TerminalSessionClient,
  cols?: number,
  rows?: number,
): Promise<void> {
  const sessionType = task.sessionType as SessionType;
  const sessionName = task.sessionName as string;
  const tmuxPaneLayout = sessionType === SessionType.TMUX
    ? await getEffectivePaneLayout(task.projectId ?? undefined)
    : null;

  if (!task.sshHost) {
    await attachLocalSession(task.id, tabId, sessionType, sessionName, client as never, task.worktreePath, cols, rows, tmuxPaneLayout);
    return;
  }

  await ensureRemoteSessionDependency(sessionType, task.sshHost);

  const resolved = await resolveRemoteSshConfig(task.id);
  if (!resolved.ok) {
    client.close(1008, resolved.error);
    throw new Error(resolved.error);
  }

  await attachRemoteSession(
    task.id,
    tabId,
    task.sshHost,
    sessionType,
    sessionName,
    client as never,
    resolved.sshConfig,
    cols,
    rows,
    task.worktreePath,
    tmuxPaneLayout,
  );
}

export async function openTerminal(
  webContents: WebContents,
  taskId: string,
  tabId: string | null,
  cols: number,
  rows: number,
) {
  const existingClient = getClient(webContents.id, taskId, tabId);
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

  const client = new ElectronTerminalClient(webContents, taskId, tabId);
  terminalClients.set(buildClientKey(webContents.id, taskId, tabId), client);

  const finalizeClient = () => {
    terminalClients.delete(buildClientKey(webContents.id, taskId, tabId));
  };

  client.once("close", finalizeClient);

  try {
    await attachTaskTerminal(task, tabId, client, cols, rows);
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

export function writeTerminal(
  webContentsId: number,
  taskId: string,
  tabId: string | null,
  data: string,
) {
  getClient(webContentsId, taskId, tabId)?.emitMessage(data);
}

export function resizeTerminal(
  webContentsId: number,
  taskId: string,
  tabId: string | null,
  cols: number,
  rows: number,
) {
  getClient(webContentsId, taskId, tabId)
    ?.emitMessage(`\x01${JSON.stringify({ type: "resize", cols, rows })}`);
}

export function focusTerminal(taskId: string) {
  focusSession(taskId);
}

export function closeTerminal(webContentsId: number, taskId: string, tabId: string | null) {
  const client = getClient(webContentsId, taskId, tabId);
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
