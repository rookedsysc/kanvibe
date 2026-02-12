import type { WebSocket } from "ws";
import { SessionType } from "@/entities/KanbanTask";

/**
 * 활성 터미널 세션을 관리하는 레지스트리.
 * taskId를 키로 PTY 프로세스를 추적한다.
 */
interface TerminalEntry {
  pty: import("node-pty").IPty;
  ws: WebSocket;
}

const activeTerminals = new Map<string, TerminalEntry>();

/** 로컬 tmux window / zellij tab에 attach하여 WebSocket과 연결한다 */
export async function attachLocalSession(
  taskId: string,
  sessionType: SessionType,
  sessionName: string,
  windowName: string,
  ws: WebSocket
): Promise<void> {
  const pty = await import("node-pty");

  const shell =
    sessionType === SessionType.TMUX
      ? "tmux"
      : "zellij";

  /** tmux는 session:window 형식으로 특정 window에 직접 연결한다 */
  const args =
    sessionType === SessionType.TMUX
      ? ["attach-session", "-t", `${sessionName}:${windowName}`]
      : ["attach", sessionName];

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

  const ptyProcess = pty.spawn(shell, args, {
    name: "xterm-256color",
    cols: 120,
    rows: 30,
    cwd: process.env.HOME || "/",
    env: process.env as Record<string, string>,
  });

  activeTerminals.set(taskId, { pty: ptyProcess, ws });

  ptyProcess.onData((data) => {
    if (ws.readyState === ws.OPEN) {
      ws.send(data);
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
    detachSession(taskId);
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
  sshConfig: { hostname: string; port: number; username: string; privateKeyPath: string }
): Promise<void> {
  const { Client } = await import("ssh2");
  const fs = await import("fs");

  const conn = new Client();

  conn.on("ready", () => {
    /** tmux는 session:window 타겟으로, zellij는 tab 이동 후 attach한다 */
    const command =
      sessionType === SessionType.TMUX
        ? `tmux attach-session -t "${sessionName}:${windowName}"`
        : `zellij action --session "${sessionName}" go-to-tab-name "${windowName}" 2>/dev/null; zellij attach "${sessionName}"`;

    conn.shell({ term: "xterm-256color", cols: 120, rows: 30 }, (err, stream) => {
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

/** 터미널 세션을 분리하고 PTY 프로세스를 종료한다 */
export function detachSession(taskId: string): void {
  const entry = activeTerminals.get(taskId);
  if (!entry) return;

  try {
    entry.pty.kill();
  } catch {
    // 이미 종료된 경우 무시
  }

  if (entry.ws.readyState === entry.ws.OPEN) {
    entry.ws.close();
  }

  activeTerminals.delete(taskId);
}

/** 활성 터미널 수를 반환한다 */
export function getActiveTerminalCount(): number {
  return activeTerminals.size;
}
