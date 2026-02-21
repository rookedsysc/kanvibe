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
}

const activeTerminals = new Map<string, TerminalEntry>();

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

  /** 세션이 없으면 자동 생성한다 */
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
    if (!isZellijSessionAlive(sessionName)) {
      try {
        const dir = cwd || process.env.HOME || "/";
        execSync(
          `cd "${dir}" && zellij --session "${sessionName}" &`,
          { timeout: 5000, shell: "/bin/sh" },
        );
        /** zellij 초기화 대기 */
        await new Promise((resolve) => setTimeout(resolve, 2000));
      } catch (error) {
        console.error(`[터미널] zellij 세션 자동 생성 실패:`, error);
        ws.close(1008, "zellij 세션 생성에 실패했습니다.");
        return;
      }
    }
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

  /** 세션에 직접 attach한다 */
  const args: string[] =
    sessionType === SessionType.TMUX
      ? ["attach-session", "-t", sessionName]
      : ["attach", sessionName];

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

  const entry: TerminalEntry = { pty: ptyProcess, clients: new Set([ws]), sessionType, sessionName };
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

/** SSH를 통해 원격 세션에 attach하여 WebSocket과 연결한다 */
export async function attachRemoteSession(
  taskId: string,
  sshHost: string,
  sessionType: SessionType,
  sessionName: string,
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
    /** 세션에 직접 attach한다 */
    const command =
      sessionType === SessionType.TMUX
        ? `tmux attach-session -t "${sessionName}"`
        : `zellij attach "${sessionName}"`;

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

/** 탭 전환 시 해당 태스크의 세션으로 포커스를 이동한다 */
export function focusSession(taskId: string): void {
  const entry = activeTerminals.get(taskId);
  if (!entry) return;

  try {
    if (entry.sessionType === SessionType.TMUX) {
      execSync(`tmux switch-client -t "${entry.sessionName}"`, { timeout: 3000, stdio: "ignore" });
    }
    // zellij는 외부에서 세션 전환이 불가능하므로 무시
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
