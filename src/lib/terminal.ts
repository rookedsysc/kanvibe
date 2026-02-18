import type { WebSocket } from "ws";
import { execSync } from "child_process";
import { SessionType } from "@/entities/KanbanTask";

/**
 * 활성 터미널 세션을 관리하는 레지스트리.
 * taskId를 키로 PTY 프로세스를 추적한다.
 */
interface TerminalEntry {
  pty: import("node-pty").IPty;
  clients: Set<WebSocket>;
  sessionType: SessionType;
  sessionName: string;
  windowName: string;
}

const activeTerminals = new Map<string, TerminalEntry>();

/** tmux 세션에 해당 window가 존재하는지 확인한다 */
function isTmuxWindowAlive(sessionName: string, windowName: string): boolean {
  try {
    const output = execSync(
      `tmux list-windows -t "${sessionName}" -F "#{window_name}"`,
      { encoding: "utf-8", timeout: 5000 },
    );
    return output.split("\n").some((w) => w.trimEnd() === windowName);
  } catch {
    return false;
  }
}

/** tmux window의 이름으로 인덱스를 조회한다. 중복 이름이 있어도 첫 번째 매치를 반환한다 */
function getTmuxWindowIndex(sessionName: string, windowName: string): string | null {
  try {
    const output = execSync(
      `tmux list-windows -t "${sessionName}" -F "#{window_name}\t#{window_index}"`,
      { encoding: "utf-8", timeout: 5000 },
    );
    for (const line of output.split("\n")) {
      const [name, index] = line.split("\t");
      if (name?.trimEnd() === windowName && index) return index;
    }
    return null;
  } catch {
    return null;
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

/** 로컬 tmux window / zellij tab에 attach하여 WebSocket과 연결한다 */
export async function attachLocalSession(
  taskId: string,
  sessionType: SessionType,
  sessionName: string,
  windowName: string,
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
          } else if (parsed.type === "focus") {
            focusSession(taskId);
          }
        } catch {
          existing.pty.write(data);
        }
        return;
      }
      existing.pty.write(data);
    });

    ws.on("close", () => {
      existing.clients.delete(ws);
      if (existing.clients.size === 0) {
        detachSession(taskId);
      }
    });

    return;
  }

  /** tmux window가 없으면 세션과 window를 자동 생성한다 */
  if (sessionType === SessionType.TMUX) {
    if (!isTmuxWindowAlive(sessionName, windowName)) {
      try {
        const dir = cwd || process.env.HOME || "/";
        execSync(
          `tmux has-session -t "${sessionName}" 2>/dev/null || tmux new-session -d -s "${sessionName}"`,
          { timeout: 5000 },
        );
        /** 중복 방지: 다시 확인 후 없을 때만 생성한다 */
        if (!isTmuxWindowAlive(sessionName, windowName)) {
          execSync(
            `tmux new-window -t "${sessionName}" -n "${windowName}" -c "${dir}"`,
            { timeout: 5000 },
          );
        }
      } catch (error) {
        console.error(`[터미널] tmux 세션/window 자동 생성 실패:`, error);
        ws.close(1008, "tmux 세션 생성에 실패했습니다.");
        return;
      }
    }
  } else if (!isZellijSessionAlive(sessionName)) {
    console.error(`[터미널] zellij 세션을 찾을 수 없음: ${sessionName}`);
    ws.close(1008, "zellij 세션을 찾을 수 없습니다.");
    return;
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

  const shell =
    sessionType === SessionType.TMUX
      ? "tmux"
      : "zellij";

  /** tmux는 session:windowIndex 형식으로 특정 window에 직접 연결한다. 이름 중복 시에도 안전하도록 인덱스를 사용한다 */
  let args: string[];
  if (sessionType === SessionType.TMUX) {
    const windowIndex = getTmuxWindowIndex(sessionName, windowName);
    const target = windowIndex ? `${sessionName}:${windowIndex}` : `${sessionName}:${windowName}`;
    args = ["attach-session", "-t", target];
  } else {
    args = ["attach", sessionName];
  }

  /**
   * zellij는 attach 전에 해당 tab으로 이동해야 한다.
   * go-to-tab-name 액션으로 대상 tab을 선택한다.
   */
  if (sessionType === SessionType.ZELLIJ) {
    const { exec } = await import("child_process");
    const { promisify } = await import("util");
    const execAsync = promisify(exec);
    try {
      await execAsync(`zellij action --session "${sessionName}" go-to-tab-name "${windowName}"`);
    } catch {
      // tab 이동 실패 시 기본 탭으로 attach
    }
  }

  let ptyProcess: import("node-pty").IPty;
  try {
    ptyProcess = pty.spawn(shell, args, {
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
    console.error("[터미널] PTY spawn 실패:", error);
    ws.close(1011, "터미널 프로세스 생성 실패");
    return;
  }

  const entry: TerminalEntry = { pty: ptyProcess, clients: new Set([ws]), sessionType, sessionName, windowName };
  activeTerminals.set(taskId, entry);

  ptyProcess.onData((data) => {
    for (const client of entry.clients) {
      if (client.readyState === client.OPEN) {
        client.send(data);
      }
    }
  });

  ptyProcess.onExit(() => {
    detachSession(taskId);
  });

  ws.on("message", (message) => {
    const data = message.toString();

    if (data.startsWith("\x01")) {
      try {
        const parsed = JSON.parse(data.slice(1));
        if (parsed.type === "resize" && parsed.cols && parsed.rows) {
          ptyProcess.resize(parsed.cols, parsed.rows);
        } else if (parsed.type === "focus") {
          focusSession(taskId);
        }
      } catch {
        // 파싱 실패 시 일반 입력으로 처리
        ptyProcess.write(data);
      }
      return;
    }

    ptyProcess.write(data);
  });

  ws.on("close", () => {
    entry.clients.delete(ws);
    if (entry.clients.size === 0) {
      detachSession(taskId);
    }
  });
}

/** SSH를 통해 원격 tmux window / zellij tab에 attach하여 WebSocket과 연결한다 */
export async function attachRemoteSession(
  taskId: string,
  sshHost: string,
  sessionType: SessionType,
  sessionName: string,
  windowName: string,
  ws: WebSocket,
  sshConfig: { hostname: string; port: number; username: string; privateKeyPath: string },
  cols?: number,
  rows?: number,
): Promise<void> {
  const initialCols = cols ?? 120;
  const initialRows = rows ?? 30;

  const { Client } = await import("ssh2");
  const fs = await import("fs");

  const conn = new Client();

  conn.on("ready", () => {
    /** tmux는 session:window 타겟으로, zellij는 tab 이동 후 attach한다 */
    const command =
      sessionType === SessionType.TMUX
        ? `tmux attach-session -t "${sessionName}:${windowName}"`
        : `zellij action --session "${sessionName}" go-to-tab-name "${windowName}" 2>/dev/null; zellij attach "${sessionName}"`;

    conn.shell({ term: "xterm-256color", cols: initialCols, rows: initialRows }, (err, stream) => {
      if (err) {
        ws.close(1011, "SSH shell 오류");
        conn.end();
        return;
      }

      stream.write(command + "\n");

      stream.on("data", (data: Buffer) => {
        if (ws.readyState === ws.OPEN) {
          ws.send(data);
        }
      });

      stream.on("close", () => {
        ws.close();
        conn.end();
        activeTerminals.delete(taskId);
      });

      ws.on("message", (message) => {
        const data = message.toString();

        if (data.startsWith("\x01")) {
          try {
            const parsed = JSON.parse(data.slice(1));
            if (parsed.type === "resize" && parsed.cols && parsed.rows) {
              stream.setWindow(parsed.rows, parsed.cols, 0, 0);
            }
          } catch {
            stream.write(data);
          }
          return;
        }

        stream.write(data);
      });

      ws.on("close", () => {
        stream.close();
        conn.end();
        activeTerminals.delete(taskId);
      });
    });
  });

  conn.on("error", (err) => {
    console.error("SSH 연결 오류:", err.message);
    ws.close(1011, "SSH 연결 실패");
  });

  conn.connect({
    host: sshConfig.hostname,
    port: sshConfig.port,
    username: sshConfig.username,
    privateKey: fs.readFileSync(sshConfig.privateKeyPath),
  });
}

/** 탭 전환 시 해당 태스크의 tmux window / zellij tab으로 포커스를 이동한다 */
export function focusSession(taskId: string): void {
  const entry = activeTerminals.get(taskId);
  if (!entry) return;

  try {
    if (entry.sessionType === SessionType.TMUX) {
      const windowIndex = getTmuxWindowIndex(entry.sessionName, entry.windowName);
      const target = windowIndex
        ? `${entry.sessionName}:${windowIndex}`
        : `${entry.sessionName}:${entry.windowName}`;
      execSync(`tmux select-window -t "${target}"`, { timeout: 3000, stdio: "ignore" });
    } else {
      execSync(
        `zellij action --session "${entry.sessionName}" go-to-tab-name "${entry.windowName}"`,
        { timeout: 3000, stdio: "ignore" },
      );
    }
  } catch {
    // focus 실패 시 무시 (세션이 종료되었을 수 있음)
  }
}

/** 터미널 세션을 분리하고 PTY 프로세스를 종료한다. 모든 연결된 클라이언트를 닫는다 */
export function detachSession(taskId: string): void {
  const entry = activeTerminals.get(taskId);
  if (!entry) return;

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
