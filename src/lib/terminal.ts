import path from "path";
import { existsSync } from "fs";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType } from "@/entities/PaneLayoutConfig";
import { ZELLIJ_LAYOUT_FILENAME } from "@/lib/worktree";
import { execSync } from "child_process";
import type { WebSocket } from "ws";
import { buildSSHArgs, getKanvibeSSHConnectionReuseOptions } from "@/lib/sshConfig";
import { buildTmuxSessionBootstrapCommands, type TmuxPaneLayoutConfig } from "@/lib/worktree";

/**
 * 활성 터미널 세션을 관리하는 레지스트리.
 * taskId를 키로 PTY 프로세스를 추적한다.
 */
interface TerminalEntry {
  pty: import("node-pty").IPty;
  clients: Set<WebSocket>;
  sessionType: SessionType;
  sessionName: string;
}

const activeTerminals = new Map<string, TerminalEntry>();

function shouldLogTerminalSpawn(): boolean {
  return process.env.KANVIBE_DEBUG_TERMINAL === "true";
}

/** 진단 로그 헬퍼. KANVIBE_DEBUG_TERMINAL=true일 때만 출력된다 */
function debugLog(message: string, payload?: Record<string, unknown>): void {
  if (!shouldLogTerminalSpawn()) return;
  if (payload === undefined) {
    console.log(`[터미널-진단] ${message}`);
    return;
  }
  console.log(`[터미널-진단] ${message}`, JSON.stringify(payload));
}

/** tmux 세션이 존재하는지 확인한다 */
function isTmuxSessionAlive(sessionName: string): boolean {
  try {
    execSync(`tmux has-session -t "${sessionName}" 2>/dev/null`, { timeout: 3000 });
    return true;
  } catch {
    return false;
  }
}

/** zellij 세션이 존재하는지 확인한다 */
function isZellijSessionAlive(sessionName: string): boolean {
  try {
    const output = execSync("zellij list-sessions", {
      encoding: "utf-8",
      timeout: 3000,
    });
    return output.split("\n").some((s) => s.trim().startsWith(sessionName));
  } catch {
    return false;
  }
}

/** 로컬 tmux / zellij 세션에 attach하여 WebSocket과 연결한다 */
export async function attachLocalSession(
  taskId: string,
  sessionType: SessionType,
  sessionName: string,
  ws: WebSocket,
  cwd?: string | null,
  cols?: number,
  rows?: number,
): Promise<void> {
  const initialCols = cols ?? 120;
  const initialRows = rows ?? 30;

  /** 동일 taskId로 이미 활성 PTY가 있으면 기존 PTY를 공유한다 */
  const existing = activeTerminals.get(taskId);
  if (existing) {
    existing.clients.add(ws);

    ws.on("message", (message) => {
      const data = message.toString();
      if (data.startsWith("\x01")) {
        try {
          const parsed = JSON.parse(data.slice(1));
          if (parsed.type === "resize" && parsed.cols && parsed.rows) {
            existing.pty.resize(parsed.cols, parsed.rows);
          }
        } catch {
          existing.pty.write(data);
        }
        return;
      }
      existing.pty.write(data);
    });

    ws.on("close", () => {
      debugLog("Local ws.close 발생 (기존 세션)", { taskId, remainingClients: existing.clients.size - 1 });
      existing.clients.delete(ws);
      if (existing.clients.size === 0) {
        detachSession(taskId, "local-ws-close-existing");
      }
    });

    return;
  }

  /**
   * tmux: 세션이 없으면 execSync으로 detached 세션을 먼저 생성한다 (TTY 불필요).
   * zellij: TTY 없이 실행 불가하므로, node-pty가 PTY를 제공하며 세션 생성을 처리한다.
   */
  let zellijNeedsCreation = false;

  if (sessionType === SessionType.TMUX) {
    if (!isTmuxSessionAlive(sessionName)) {
      try {
        const dir = cwd || process.env.HOME || "/";
        execSync(
          `tmux new-session -d -s "${sessionName}" -c "${dir}"`,
          { timeout: 5000 },
        );
      } catch (error) {
        console.error(`[터미널] tmux 세션 자동 생성 실패:`, error);
        ws.close(1008, "tmux 세션 생성에 실패했습니다.");
        return;
      }
    }
  } else {
    zellijNeedsCreation = !isZellijSessionAlive(sessionName);
    console.log(`[터미널] zellij sessionName="${sessionName}", needsCreation=${zellijNeedsCreation}, cwd=${cwd}`);
  }

  /** 웹 터미널 크기가 다른 클라이언트에 제한되지 않도록 최근 활성 클라이언트 기준으로 설정 */
  if (sessionType === SessionType.TMUX) {
    try {
      execSync("tmux set-option -g window-size latest", { stdio: "ignore" });
    } catch {
      // tmux 구버전에서는 window-size 옵션이 없을 수 있음
    }
  }

  const pty = await import("node-pty");

  let shell: string;
  let args: string[];
  let ptyCwd: string;

  if (sessionType === SessionType.TMUX) {
    shell = "tmux";
    args = ["attach-session", "-t", sessionName];
    ptyCwd = process.env.HOME || "/";
  } else if (zellijNeedsCreation) {
    /** 세션이 없으면 --session으로 생성과 attach를 동시에 처리한다 */
    shell = "zellij";
    args = ["--session", sessionName];
    ptyCwd = cwd || process.env.HOME || "/";

    /** worktree 디렉토리에 KDL 레이아웃 파일이 있으면 새 세션 생성 시 적용한다 */
    if (cwd) {
      const layoutFile = path.join(cwd, ZELLIJ_LAYOUT_FILENAME);
      if (existsSync(layoutFile)) {
        args.push("--new-session-with-layout", layoutFile);
      }
    }
  } else {
    /** 기존 세션에 attach한다 */
    shell = "zellij";
    args = ["attach", sessionName];
    ptyCwd = process.env.HOME || "/";
  }

  if (shouldLogTerminalSpawn()) {
    console.log(`[터미널] PTY spawn: shell=${shell}, args=${JSON.stringify(args)}, cwd=${ptyCwd}`);
  }

  let ptyProcess: import("node-pty").IPty;
  try {
    ptyProcess = pty.spawn(shell, args, {
      name: "xterm-256color",
      cols: initialCols,
      rows: initialRows,
      cwd: ptyCwd,
      env: {
        ...process.env,
        LANG: process.env.LANG || "en_US.UTF-8",
        LC_ALL: process.env.LC_ALL || "en_US.UTF-8",
      } as Record<string, string>,
    });
  } catch (error) {
    console.error("[터미널] PTY spawn 실패:", error);
    ws.close(1011, "터미널 프로세스 생성 실패");
    return;
  }

  const entry: TerminalEntry = { pty: ptyProcess, clients: new Set([ws]), sessionType, sessionName };
  activeTerminals.set(taskId, entry);

  let firstLocalDataLogged = false;
  ptyProcess.onData((data) => {
    if (!firstLocalDataLogged) {
      firstLocalDataLogged = true;
      debugLog("Local PTY 첫 데이터 수신", { taskId, sample: data.slice(0, 200) });
    }
    for (const client of entry.clients) {
      if (client.readyState === client.OPEN) {
        client.send(data);
      }
    }
  });

  ptyProcess.onExit(({ exitCode, signal }) => {
    debugLog("Local PTY onExit", { taskId, exitCode, signal });
    detachSession(taskId, "local-pty-exit");
  });

  ws.on("message", (message) => {
    const data = message.toString();

    if (data.startsWith("\x01")) {
      try {
        const parsed = JSON.parse(data.slice(1));
        if (parsed.type === "resize" && parsed.cols && parsed.rows) {
          ptyProcess.resize(parsed.cols, parsed.rows);
        }
      } catch {
        ptyProcess.write(data);
      }
      return;
    }

    ptyProcess.write(data);
  });

  ws.on("close", () => {
    debugLog("Local ws.close 발생", { taskId, remainingClients: entry.clients.size - 1 });
    entry.clients.delete(ws);
    if (entry.clients.size === 0) {
      detachSession(taskId, "local-ws-close");
    }
  });
}

/** SSH를 통해 원격 세션에 attach하여 WebSocket과 연결한다 */
export async function attachRemoteSession(
  taskId: string,
  sshHost: string,
  sessionType: SessionType,
  sessionName: string,
  ws: WebSocket,
  sshConfig: {
    host: string;
    hostname: string;
    port: number;
    username: string;
    privateKeyPath: string;
  },
  cols?: number,
  rows?: number,
  worktreePath?: string | null,
  tmuxPaneLayout?: TmuxPaneLayoutConfig | null,
): Promise<void> {
  const initialCols = cols ?? 120;
  const initialRows = rows ?? 30;

  const existing = activeTerminals.get(taskId);
  if (existing) {
    existing.clients.add(ws);
    ws.on("message", (message) => handleTerminalMessage(existing.pty, message.toString()));
    ws.on("close", () => {
      debugLog("Remote ws.close 발생 (기존 세션)", { taskId, remainingClients: existing.clients.size - 1 });
      existing.clients.delete(ws);
      if (existing.clients.size === 0) {
        detachSession(taskId, "remote-ws-close-existing");
      }
    });
    return;
  }

  const pty = await import("node-pty");
  const attachCommand = sessionType === SessionType.TMUX
    ? buildRemoteTmuxAttachCommand(sessionName, worktreePath, tmuxPaneLayout)
    : `exec zellij attach "${sessionName}"`;
  const args = buildSSHArgs(sshConfig, {
    forceTty: true,
    trustedX11Forwarding: true,
    connectionReuse: getKanvibeSSHConnectionReuseOptions(),
  });

  if (shouldLogTerminalSpawn()) {
    console.log(`[터미널] Remote PTY spawn: shell=ssh, args=${JSON.stringify(args)}`);
  }
  debugLog("attachRemoteSession 시작", { taskId, sshHost, sessionName, sessionType, worktreePath, attachCommand });

  let ptyProcess: import("node-pty").IPty;
  try {
    ptyProcess = pty.spawn("ssh", args, {
      name: "xterm-256color",
      cols: initialCols,
      rows: initialRows,
      cwd: process.env.HOME || "/",
      env: {
        ...process.env,
        LANG: process.env.LANG || "en_US.UTF-8",
        LC_ALL: process.env.LC_ALL || "en_US.UTF-8",
      } as Record<string, string>,
    });
  } catch (error) {
    console.error("[터미널] Remote PTY spawn 실패:", error);
    ws.close(1011, "SSH 연결 실패");
    return;
  }

  debugLog("Remote PTY spawn 성공", { taskId, pid: ptyProcess.pid });

  const entry: TerminalEntry = { pty: ptyProcess, clients: new Set([ws]), sessionType, sessionName };
  activeTerminals.set(taskId, entry);

  let firstDataLogged = false;
  ptyProcess.onData((data) => {
    if (!firstDataLogged) {
      firstDataLogged = true;
      debugLog("Remote PTY 첫 데이터 수신", { taskId, sample: data.slice(0, 200) });
    }
    for (const client of entry.clients) {
      if (client.readyState === client.OPEN) {
        client.send(data);
      }
    }
  });

  ptyProcess.onExit(({ exitCode, signal }) => {
    debugLog("Remote PTY onExit", { taskId, exitCode, signal });
    detachSession(taskId, "remote-pty-exit");
  });

  ptyProcess.write(`${attachCommand}\r`);
  debugLog("Remote PTY attachCommand 전송 완료", { taskId, byteLength: attachCommand.length });

  ws.on("message", (message) => {
    handleTerminalMessage(ptyProcess, message.toString());
  });

  ws.on("close", () => {
    debugLog("Remote ws.close 발생", { taskId, remainingClients: entry.clients.size - 1 });
    entry.clients.delete(ws);
    if (entry.clients.size === 0) {
      detachSession(taskId, "remote-ws-close");
    }
  });
}

function buildRemoteTmuxAttachCommand(
  sessionName: string,
  worktreePath?: string | null,
  tmuxPaneLayout?: TmuxPaneLayoutConfig | null,
): string {
  const attachCommand = `exec tmux attach-session -t "${sessionName}"`;
  const bootstrapCommands = worktreePath
    ? buildTmuxSessionBootstrapCommands(
        sessionName,
        worktreePath,
        tmuxPaneLayout && tmuxPaneLayout.layoutType !== PaneLayoutType.SINGLE
          ? tmuxPaneLayout
          : null,
      )
    : [`tmux new-session -d -s "${sessionName}"`];

  return [
    `if tmux has-session -t "${sessionName}" 2>/dev/null; then`,
    `  ${attachCommand}`,
    "fi",
    ...bootstrapCommands,
    attachCommand,
  ].join("; ");
}

function handleTerminalMessage(ptyProcess: import("node-pty").IPty, data: string): void {
  if (data.startsWith("\x01")) {
    try {
      const parsed = JSON.parse(data.slice(1));
      if (parsed.type === "resize" && parsed.cols && parsed.rows) {
        ptyProcess.resize(parsed.cols, parsed.rows);
      }
    } catch {
      ptyProcess.write(data);
    }
    return;
  }

  ptyProcess.write(data);
}

/** 렌더러의 입력 포커스는 xterm DOM에서만 처리한다. 호스트 tmux 클라이언트 전환은 수행하지 않는다 */
export function focusSession(taskId: string): void {
  if (!activeTerminals.has(taskId)) {
    return;
  }

  return;
}

/**
 * 터미널 세션을 분리하고 PTY 프로세스를 종료한다. 모든 연결된 클라이언트를 닫는다.
 * @param triggerLabel 누가 detach를 호출했는지 표시 (진단용). 예: "remote-pty-exit", "remote-ws-close", "closeWindowTerminals"
 */
export function detachSession(taskId: string, triggerLabel?: string): void {
  const entry = activeTerminals.get(taskId);
  if (!entry) {
    debugLog("detachSession 호출 (entry 없음)", { taskId, triggerLabel });
    return;
  }

  debugLog("detachSession 진입", { taskId, triggerLabel, sessionName: entry.sessionName, clients: entry.clients.size });

  try {
    entry.pty.kill();
  } catch {
    // 이미 종료된 경우 무시
  }

  for (const client of entry.clients) {
    if (client.readyState === client.OPEN) {
      client.close();
    }
  }
  entry.clients.clear();

  activeTerminals.delete(taskId);
}

/** 활성 터미널 수를 반환한다 */
export function getActiveTerminalCount(): number {
  return activeTerminals.size;
}
