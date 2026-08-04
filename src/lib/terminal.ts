import path from "path";
import { existsSync } from "fs";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType } from "@/entities/PaneLayoutConfig";
import { execSync } from "child_process";
import type { WebSocket } from "ws";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, hasLocalX11Display } from "@/lib/sshConfig";
import {
  buildTmuxSessionBootstrapCommand,
  buildTmuxUserClipboardPreferenceCommand,
  hasUserTmuxClipboardPreference,
  parseAliveZellijSessionNames,
  quoteForPosixShell,
  type TmuxPaneLayoutConfig,
  ZELLIJ_LAYOUT_FILENAME,
} from "@/lib/worktree";
import { execGit } from "@/lib/gitOperations";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";

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
    execSync(`tmux has-session -t "${sessionName}" 2>/dev/null`, {
      env: createLocalShellEnvironment(),
      timeout: 3000,
    });
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
      env: createLocalShellEnvironment(),
      timeout: 3000,
    });
    return parseAliveZellijSessionNames(output).includes(sessionName);
  } catch {
    return false;
  }
}

/**
 * tmux 서버 기동은 사용자 설정에 달려 있어 오래 걸릴 수 있다.
 * 플러그인 관리자를 run-shell로 부르는 설정은 기동에만 수 초가 걸리므로 부트스트랩 상한을 넉넉히 둔다.
 */
const TMUX_BOOTSTRAP_TIMEOUT_MS = 30_000;

/** 설정 파일 grep 한 번이라 부트스트랩보다 훨씬 짧게 잡는다 */
const TMUX_CLIPBOARD_PREFERENCE_TIMEOUT_MS = 5_000;

/**
 * OSC 52 전달을 KanVibe가 켜도 되는지 판단한다.
 * `set-clipboard`는 서버 옵션이라 사용자의 다른 tmux 세션까지 함께 바뀌므로,
 * 사용자가 자기 설정에서 값을 정해 뒀으면 그 선택을 그대로 둔다.
 */
function shouldEnableLocalTmuxClipboard(terminalEnvironment: NodeJS.ProcessEnv): boolean {
  try {
    const output = execSync(buildTmuxUserClipboardPreferenceCommand(), {
      env: terminalEnvironment,
      encoding: "utf-8",
      timeout: TMUX_CLIPBOARD_PREFERENCE_TIMEOUT_MS,
    });
    return !hasUserTmuxClipboardPreference(output);
  } catch {
    /** 확인에 실패하면 사용자 설정을 덮지 않는 쪽으로 기운다 */
    return false;
  }
}

async function shouldEnableRemoteTmuxClipboard(sshHost: string): Promise<boolean> {
  try {
    const output = await execGit(buildTmuxUserClipboardPreferenceCommand(), sshHost, {
      timeoutMs: TMUX_CLIPBOARD_PREFERENCE_TIMEOUT_MS,
    });
    return !hasUserTmuxClipboardPreference(output);
  } catch {
    return false;
  }
}

/**
 * 로컬 tmux 세션을 만든다.
 * 사용자 설정이 세션 기동을 막으면 설정 파일 없이 한 번 더 시도해, 소켓을 바꾸지 않고도 세션을 살린다.
 * 재시도는 tmux 서버가 아직 없을 때만 설정을 실제로 건너뛴다(`TmuxSessionBootstrapOptions` 참고).
 */
function bootstrapLocalTmuxSession(
  sessionName: string,
  workingDir: string,
  tmuxPaneLayout: TmuxPaneLayoutConfig | null | undefined,
  terminalEnvironment: NodeJS.ProcessEnv,
): void {
  const paneLayout = tmuxPaneLayout && tmuxPaneLayout.layoutType !== PaneLayoutType.SINGLE
    ? tmuxPaneLayout
    : null;
  const enableClipboard = shouldEnableLocalTmuxClipboard(terminalEnvironment);
  const runBootstrap = (withoutUserConfigFile: boolean) => {
    execSync(
      buildTmuxSessionBootstrapCommand(sessionName, workingDir, paneLayout, {
        withoutUserConfigFile,
        enableClipboard,
      }),
      { env: terminalEnvironment, timeout: TMUX_BOOTSTRAP_TIMEOUT_MS },
    );
  };

  try {
    runBootstrap(false);
  } catch (error) {
    console.warn("[터미널] 사용자 tmux 설정으로 세션 생성에 실패해 설정 없이 재시도합니다:", error);
    execSync(`tmux kill-session -t ${quoteForPosixShell(sessionName)} 2>/dev/null || true`, {
      env: terminalEnvironment,
      timeout: TMUX_BOOTSTRAP_TIMEOUT_MS,
    });
    runBootstrap(true);
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
  tmuxPaneLayout?: TmuxPaneLayoutConfig | null,
): Promise<void> {
  const initialCols = cols ?? 120;
  const initialRows = rows ?? 30;
  const terminalEnvironment = createLocalShellEnvironment();

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
        bootstrapLocalTmuxSession(
          sessionName,
          cwd || process.env.HOME || "/",
          tmuxPaneLayout,
          terminalEnvironment,
        );
      } catch (error) {
        console.error(`[터미널] tmux 세션 자동 생성 실패:`, error);
        ws.close(1008, "tmux 세션 생성에 실패했습니다.");
        throw new Error("tmux 세션 생성에 실패했습니다.");
      }
    }
  } else {
    zellijNeedsCreation = !isZellijSessionAlive(sessionName);
    console.log(`[터미널] zellij sessionName="${sessionName}", needsCreation=${zellijNeedsCreation}, cwd=${cwd}`);
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
      env: terminalEnvironment,
    });
  } catch (error) {
    console.error("[터미널] PTY spawn 실패:", error);
    ws.close(1011, "터미널 프로세스 생성 실패");
    throw new Error("터미널 프로세스 생성 실패");
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
  const terminalEnvironment = createLocalShellEnvironment();

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
    ? buildRemoteTmuxAttachCommand(
        sessionName,
        worktreePath,
        tmuxPaneLayout,
        await shouldEnableRemoteTmuxClipboard(sshHost),
      )
    : buildRemoteZellijAttachCommand(sessionName, worktreePath);
  const args = [
    ...buildSSHArgs(sshConfig, {
      forceTty: true,
      trustedX11Forwarding: hasLocalX11Display(),
      connectionHealth: getKanvibeSSHConnectionHealthOptions(),
    }),
    buildRemoteShellCommand(attachCommand),
  ];

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
      env: terminalEnvironment,
    });
  } catch (error) {
    console.error("[터미널] Remote PTY spawn 실패:", error);
    ws.close(1011, "SSH 연결 실패");
    throw new Error("SSH 연결 실패");
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

  debugLog("Remote PTY attachCommand 인자 전달 완료", { taskId, byteLength: attachCommand.length });

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

function buildRemoteShellCommand(command: string): string {
  return `sh -lc ${quoteForPosixShell(command)}`;
}

/**
 * 원격 tmux 세션에 붙는 명령을 만든다.
 * 세션이 이미 있으면 그대로 attach하고, 없으면 생성부터 attach까지를 tmux 호출 한 번으로 처리한다.
 * 사용자 설정 때문에 실패하면 소켓은 그대로 둔 채 설정 파일 없이 한 번 더 시도한다.
 * 재시도는 원격에 tmux 서버가 아직 없을 때만 설정을 실제로 건너뛴다(`TmuxSessionBootstrapOptions` 참고).
 */
function buildRemoteTmuxAttachCommand(
  sessionName: string,
  worktreePath?: string | null,
  tmuxPaneLayout?: TmuxPaneLayoutConfig | null,
  enableClipboard = false,
): string {
  const quotedSessionName = quoteForPosixShell(sessionName);
  const buildBootstrapCommand = (withoutUserConfigFile: boolean) => buildTmuxSessionBootstrapCommand(
    sessionName,
    worktreePath ?? "",
    tmuxPaneLayout && tmuxPaneLayout.layoutType !== PaneLayoutType.SINGLE
      ? tmuxPaneLayout
      : null,
    { attachAfterBootstrap: true, withoutUserConfigFile, enableClipboard },
  );
  const bootstrapWithRetry = [
    buildBootstrapCommand(false),
    `{ tmux kill-session -t ${quotedSessionName} 2>/dev/null; ${buildBootstrapCommand(true)}; }`,
  ].join(" || ");

  return [
    `if tmux has-session -t ${quotedSessionName} 2>/dev/null; then`,
    `exec tmux attach-session -t ${quotedSessionName};`,
    `else ${bootstrapWithRetry} || { ${buildRemoteInteractiveShellFallbackCommand()}; };`,
    "fi",
  ].join(" ");
}

/**
 * 원격 zellij 세션에 붙는 명령을 만든다.
 * 로컬과 달리 attach만 하면 세션이 없을 때 SSH가 그대로 끊기므로, 로컬처럼 생성과 레이아웃 적용까지 처리한다.
 */
function buildRemoteZellijAttachCommand(
  sessionName: string,
  worktreePath?: string | null,
): string {
  const quotedSessionName = quoteForPosixShell(sessionName);
  const createArguments = ["--session", quotedSessionName];

  if (worktreePath) {
    const layoutFile = quoteForPosixShell(path.posix.join(worktreePath, ZELLIJ_LAYOUT_FILENAME));
    return [
      `if zellij attach ${quotedSessionName} 2>/dev/null; then exit 0; fi;`,
      `cd ${quoteForPosixShell(worktreePath)} 2>/dev/null || true;`,
      `if [ -f ${layoutFile} ]; then`,
      `exec zellij ${createArguments.join(" ")} --new-session-with-layout ${layoutFile};`,
      "else",
      `exec zellij ${createArguments.join(" ")};`,
      "fi",
    ].join(" ");
  }

  return `zellij attach ${quotedSessionName} 2>/dev/null || exec zellij ${createArguments.join(" ")}`;
}

function buildRemoteInteractiveShellFallbackCommand(): string {
  return [
    `printf '%s\\n' ${quoteForPosixShell(
      "KanVibe: tmux session setup failed; leaving the SSH shell open.",
    )} >&2`,
    'exec "${SHELL:-/bin/sh}" -l',
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
