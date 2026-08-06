import path from "path";
import { existsSync } from "fs";
import { SessionType } from "@/entities/KanbanTask";
import { PaneLayoutType } from "@/entities/PaneLayoutConfig";
import { execSync } from "child_process";
import type { WebSocket } from "ws";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, hasLocalX11Display } from "@/lib/sshConfig";
import {
  buildTmuxSessionBootstrapCommand,
  buildTmuxUserClipboardDirectiveTestCommand,
  buildTmuxUserClipboardPreferenceCommand,
  buildZellijAliveSessionCheckCommand,
  hasUserTmuxClipboardPreference,
  parseAliveZellijSessionNames,
  quoteForPosixShell,
  type TmuxPaneLayoutConfig,
  ZELLIJ_LAYOUT_FILENAME,
} from "@/lib/worktree";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";

/**
 * 활성 터미널 세션을 관리하는 레지스트리.
 * 이 모듈이 PTY의 유일한 소유자다. 탭 조작도 여기 있는 PTY를 통해서만 이뤄진다.
 */
interface TerminalEntry {
  pty: import("node-pty").IPty;
  clients: Set<WebSocket>;
  sessionType: SessionType;
  sessionName: string;
  taskId: string;
  tabId: string | null;
}

const activeTerminals = new Map<string, TerminalEntry>();

/**
 * tmux와 zellij는 태스크당 PTY 하나를 멀티플렉서에 붙이고 탭은 멀티플렉서 안에 있다.
 * terminal 세션만 탭마다 PTY를 따로 가지므로 탭 식별자까지 키에 넣는다.
 */
function buildTerminalKey(taskId: string, tabId: string | null): string {
  return tabId ? `${taskId}#${tabId}` : taskId;
}

/**
 * terminal 세션은 화면을 벗어나도 셸을 유지한다.
 * tmux와 zellij는 PTY를 끊어도 세션이 서버에 남지만, terminal 세션의 PTY는 곧 작업 그 자체다.
 */
function shouldSurviveWithoutClients(sessionType: SessionType): boolean {
  return sessionType === SessionType.TERMINAL;
}

/**
 * terminal 세션의 탭 목록.
 * 탭은 PTY보다 먼저 생기고(렌더러가 붙기 전) PTY보다 늦게까지 남을 수 있어(셸이 죽어도 탭이 보임)
 * `activeTerminals`와 별도로 관리하되, 소유자는 이 모듈 하나로 유지한다.
 */
interface LocalTerminalTabState {
  tabs: { id: string; name: string }[];
  activeTabId: string;
  nextTabNumber: number;
}

const localTerminalTabs = new Map<string, LocalTerminalTabState>();

/** tmux가 window 이름에 실행 중인 명령을 쓰는 것과 같은 방식으로, 기본 탭 이름은 셸 이름으로 둔다 */
function getDefaultTabName(): string {
  const shellPath = process.env.SHELL;
  return shellPath ? path.basename(shellPath) : "shell";
}

function createTabState(taskId: string): LocalTerminalTabState {
  const state: LocalTerminalTabState = {
    tabs: [{ id: `${taskId}-1`, name: getDefaultTabName() }],
    activeTabId: `${taskId}-1`,
    nextTabNumber: 2,
  };
  localTerminalTabs.set(taskId, state);
  return state;
}

function getTabState(taskId: string): LocalTerminalTabState {
  return localTerminalTabs.get(taskId) ?? createTabState(taskId);
}

function toTerminalTabs(state: LocalTerminalTabState): TerminalTab[] {
  return state.tabs.map((tab, index) => ({
    id: tab.id,
    index,
    name: tab.name,
    isActive: tab.id === state.activeTabId,
  }));
}

/** terminal 세션의 탭 목록을 반환한다. 처음 열리는 태스크는 탭 하나를 만들어 준다 */
export function listLocalTerminalTabs(taskId: string): TerminalTab[] {
  return toTerminalTabs(getTabState(taskId));
}

/** 새 탭을 목록 끝에 추가하고 활성으로 만든다. PTY는 렌더러가 붙을 때 생긴다 */
export function createLocalTerminalTab(taskId: string): TerminalTab {
  const state = getTabState(taskId);
  const createdTab = { id: `${taskId}-${state.nextTabNumber}`, name: getDefaultTabName() };

  state.nextTabNumber += 1;
  state.tabs.push(createdTab);
  state.activeTabId = createdTab.id;

  return {
    id: createdTab.id,
    index: state.tabs.length - 1,
    name: createdTab.name,
    isActive: true,
  };
}

/** 탭을 닫고 남은 탭 수를 반환한다. 활성 탭을 닫으면 그 자리를 이어받는 탭이 활성이 된다 */
export function closeLocalTerminalTab(taskId: string, tabId: string): number {
  const state = getTabState(taskId);
  const closedIndex = state.tabs.findIndex((tab) => tab.id === tabId);
  if (closedIndex === -1) {
    return state.tabs.length;
  }

  destroyTerminal(buildTerminalKey(taskId, tabId), "close-terminal-tab");
  state.tabs.splice(closedIndex, 1);

  if (state.tabs.length === 0) {
    localTerminalTabs.delete(taskId);
    return 0;
  }

  if (state.activeTabId === tabId) {
    state.activeTabId = state.tabs[Math.min(closedIndex, state.tabs.length - 1)].id;
  }

  return state.tabs.length;
}

export function renameLocalTerminalTab(taskId: string, tabId: string, name: string): void {
  const targetTab = getTabState(taskId).tabs.find((tab) => tab.id === tabId);
  if (targetTab) {
    targetTab.name = name;
  }
}

export function selectLocalTerminalTab(taskId: string, tabId: string): void {
  const state = getTabState(taskId);
  if (state.tabs.some((tab) => tab.id === tabId)) {
    state.activeTabId = tabId;
  }
}

export function moveLocalTerminalTab(taskId: string, tabId: string, targetIndex: number): void {
  const state = getTabState(taskId);
  const currentIndex = state.tabs.findIndex((tab) => tab.id === tabId);
  if (currentIndex === -1 || targetIndex < 0 || targetIndex >= state.tabs.length) {
    return;
  }

  const [movedTab] = state.tabs.splice(currentIndex, 1);
  state.tabs.splice(targetIndex, 0, movedTab);
}

/** 앱 종료 경로. terminal 세션은 KanVibe가 소유하므로 남겨두면 고아 프로세스가 된다 */
export function killAllTerminalSessions(): void {
  for (const terminalKey of [...activeTerminals.keys()]) {
    destroyTerminal(terminalKey, "app-quit");
  }
  localTerminalTabs.clear();
}

/** WebSocket 하나를 기존 PTY에 연결한다. 같은 PTY를 여러 창이 공유할 수 있다 */
function attachClientToEntry(terminalKey: string, entry: TerminalEntry, ws: WebSocket): void {
  entry.clients.add(ws);

  ws.on("message", (message) => {
    handleTerminalMessage(entry.pty, message.toString());
  });

  ws.on("close", () => {
    debugLog("ws.close 발생", { terminalKey, remainingClients: entry.clients.size - 1 });
    releaseTerminalClient(terminalKey, ws, "ws-close");
  });
}

/**
 * 클라이언트 하나를 떼어낸다.
 * 마지막 클라이언트가 떠나도 terminal 세션은 살려 두고, 멀티플렉서 세션만 PTY를 끊는다.
 */
function releaseTerminalClient(terminalKey: string, ws: WebSocket, triggerLabel: string): void {
  const entry = activeTerminals.get(terminalKey);
  if (!entry) {
    return;
  }

  entry.clients.delete(ws);
  if (entry.clients.size > 0) {
    return;
  }

  if (shouldSurviveWithoutClients(entry.sessionType)) {
    debugLog("클라이언트가 모두 떠났지만 terminal 세션은 유지한다", { terminalKey, triggerLabel });
    return;
  }

  destroyTerminal(terminalKey, triggerLabel);
}

/** PTY를 죽이고 붙어 있던 클라이언트를 모두 닫는다 */
function destroyTerminal(terminalKey: string, triggerLabel?: string): void {
  const entry = activeTerminals.get(terminalKey);
  if (!entry) {
    debugLog("destroyTerminal 호출 (entry 없음)", { terminalKey, triggerLabel });
    return;
  }

  debugLog("destroyTerminal 진입", {
    terminalKey,
    triggerLabel,
    sessionName: entry.sessionName,
    clients: entry.clients.size,
  });

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

  activeTerminals.delete(terminalKey);
}

/** PTY 출력을 붙어 있는 모든 클라이언트에 흘리고, PTY가 죽으면 레지스트리에서 지운다 */
function registerTerminalEntry(
  terminalKey: string,
  entry: TerminalEntry,
  ws: WebSocket,
  debugLabel: string,
): void {
  activeTerminals.set(terminalKey, entry);

  let firstDataLogged = false;
  entry.pty.onData((data) => {
    if (!firstDataLogged) {
      firstDataLogged = true;
      debugLog(`${debugLabel} PTY 첫 데이터 수신`, { terminalKey, sample: data.slice(0, 200) });
    }
    for (const client of entry.clients) {
      if (client.readyState === client.OPEN) {
        client.send(data);
      }
    }
  });

  entry.pty.onExit(({ exitCode, signal }) => {
    debugLog(`${debugLabel} PTY onExit`, { terminalKey, exitCode, signal });
    destroyTerminal(terminalKey, `${debugLabel}-pty-exit`);
    /** 셸이 스스로 끝난 탭은 목록에서도 사라져야 사용자가 죽은 탭을 클릭하지 않는다 */
    if (entry.tabId) {
      removeClosedTab(entry.taskId, entry.tabId);
    }
  });

  attachClientToEntry(terminalKey, entry, ws);
}

/** PTY가 스스로 끝났을 때 탭 목록만 정리한다. PTY는 이미 없으므로 다시 죽이지 않는다 */
function removeClosedTab(taskId: string, tabId: string): void {
  const state = localTerminalTabs.get(taskId);
  if (!state) {
    return;
  }

  const closedIndex = state.tabs.findIndex((tab) => tab.id === tabId);
  if (closedIndex === -1) {
    return;
  }

  state.tabs.splice(closedIndex, 1);
  if (state.tabs.length === 0) {
    localTerminalTabs.delete(taskId);
    return;
  }

  if (state.activeTabId === tabId) {
    state.activeTabId = state.tabs[Math.min(closedIndex, state.tabs.length - 1)].id;
  }
}

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
 * 원격은 왕복을 없애려고 같은 판정을 원격 셸 안에서 직접 수행한다(`buildRemoteTmuxAttachCommand` 참고).
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

/** 로그인 셸을 그대로 띄워, 사용자가 평소 쓰는 셸 설정을 받게 한다 */
function resolveLoginShell(): string {
  return process.env.SHELL || "/bin/sh";
}

interface LocalPtySpawnPlan {
  shell: string;
  args: string[];
  cwd: string;
}

/**
 * 세션 타입별로 어떤 프로세스를 띄울지 정한다.
 * tmux는 이미 만들어 둔 세션에 attach하고, zellij는 세션이 없으면 생성까지 겸하며,
 * terminal은 멀티플렉서 없이 로그인 셸을 바로 띄운다.
 */
function buildLocalPtySpawnPlan(
  sessionType: SessionType,
  sessionName: string,
  cwd: string | null | undefined,
  zellijNeedsCreation: boolean,
): LocalPtySpawnPlan {
  const homeDirectory = process.env.HOME || "/";

  if (sessionType === SessionType.TERMINAL) {
    return { shell: resolveLoginShell(), args: ["-l"], cwd: cwd || homeDirectory };
  }

  if (sessionType === SessionType.TMUX) {
    return { shell: "tmux", args: ["attach-session", "-t", sessionName], cwd: homeDirectory };
  }

  if (!zellijNeedsCreation) {
    return { shell: "zellij", args: ["attach", sessionName], cwd: homeDirectory };
  }

  /** 세션이 없으면 --session으로 생성과 attach를 동시에 처리한다 */
  const args = ["--session", sessionName];

  /** worktree 디렉토리에 KDL 레이아웃 파일이 있으면 새 세션 생성 시 적용한다 */
  if (cwd) {
    const layoutFile = path.join(cwd, ZELLIJ_LAYOUT_FILENAME);
    if (existsSync(layoutFile)) {
      args.push("--new-session-with-layout", layoutFile);
    }
  }

  return { shell: "zellij", args, cwd: cwd || homeDirectory };
}

/** 로컬 tmux / zellij / terminal 세션에 attach하여 WebSocket과 연결한다 */
export async function attachLocalSession(
  taskId: string,
  tabId: string | null,
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
  const terminalKey = buildTerminalKey(taskId, tabId);

  /** 같은 터미널에 이미 활성 PTY가 있으면 기존 PTY를 공유한다 */
  const existing = activeTerminals.get(terminalKey);
  if (existing) {
    attachClientToEntry(terminalKey, existing, ws);
    return;
  }

  /**
   * tmux: 세션이 없으면 execSync으로 detached 세션을 먼저 생성한다 (TTY 불필요).
   * zellij: TTY 없이 실행 불가하므로, node-pty가 PTY를 제공하며 세션 생성을 처리한다.
   * terminal: 멀티플렉서가 없으므로 준비할 세션도 없다.
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
  } else if (sessionType === SessionType.ZELLIJ) {
    zellijNeedsCreation = !isZellijSessionAlive(sessionName);
    console.log(`[터미널] zellij sessionName="${sessionName}", needsCreation=${zellijNeedsCreation}, cwd=${cwd}`);
  }

  const pty = await import("node-pty");
  const spawnPlan = buildLocalPtySpawnPlan(sessionType, sessionName, cwd, zellijNeedsCreation);

  if (shouldLogTerminalSpawn()) {
    console.log(
      `[터미널] PTY spawn: shell=${spawnPlan.shell}, args=${JSON.stringify(spawnPlan.args)}, cwd=${spawnPlan.cwd}`,
    );
  }

  let ptyProcess: import("node-pty").IPty;
  try {
    ptyProcess = pty.spawn(spawnPlan.shell, spawnPlan.args, {
      name: "xterm-256color",
      cols: initialCols,
      rows: initialRows,
      cwd: spawnPlan.cwd,
      env: terminalEnvironment,
    });
  } catch (error) {
    console.error("[터미널] PTY spawn 실패:", error);
    ws.close(1011, "터미널 프로세스 생성 실패");
    throw new Error("터미널 프로세스 생성 실패");
  }

  registerTerminalEntry(
    terminalKey,
    { pty: ptyProcess, clients: new Set([ws]), sessionType, sessionName, taskId, tabId },
    ws,
    "Local",
  );
}

/** SSH를 통해 원격 세션에 attach하여 WebSocket과 연결한다 */
export async function attachRemoteSession(
  taskId: string,
  tabId: string | null,
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
  const terminalKey = buildTerminalKey(taskId, tabId);

  const existing = activeTerminals.get(terminalKey);
  if (existing) {
    attachClientToEntry(terminalKey, existing, ws);
    return;
  }

  const pty = await import("node-pty");
  const attachCommand = buildRemoteAttachCommand(sessionType, sessionName, worktreePath, tmuxPaneLayout);
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
  debugLog("Remote PTY attachCommand 인자 전달 완료", { taskId, byteLength: attachCommand.length });

  registerTerminalEntry(
    terminalKey,
    { pty: ptyProcess, clients: new Set([ws]), sessionType, sessionName, taskId, tabId },
    ws,
    "Remote",
  );
}

function buildRemoteShellCommand(command: string): string {
  return `sh -lc ${quoteForPosixShell(command)}`;
}

function buildRemoteAttachCommand(
  sessionType: SessionType,
  sessionName: string,
  worktreePath?: string | null,
  tmuxPaneLayout?: TmuxPaneLayoutConfig | null,
): string {
  if (sessionType === SessionType.TMUX) {
    return buildRemoteTmuxAttachCommand(sessionName, worktreePath, tmuxPaneLayout);
  }

  if (sessionType === SessionType.ZELLIJ) {
    return buildRemoteZellijAttachCommand(sessionName, worktreePath);
  }

  return buildRemotePlainShellCommand(worktreePath);
}

/**
 * 멀티플렉서 없이 원격 로그인 셸만 띄운다.
 * 이 셸은 SSH 연결에 매여 있어 연결이 끊기면 실행 중이던 작업도 함께 사라진다.
 * 작업을 살려두려면 tmux나 zellij 세션을 써야 한다는 점을 UI가 사용자에게 알린다.
 */
function buildRemotePlainShellCommand(worktreePath?: string | null): string {
  const launchLoginShell = 'exec "${SHELL:-/bin/sh}" -l';
  if (!worktreePath) {
    return launchLoginShell;
  }

  /** 작업 디렉터리로 못 들어가도 셸은 열어 주되, 왜 다른 위치인지 보이도록 남긴다 */
  return [
    `cd ${quoteForPosixShell(worktreePath)} ||`,
    `printf '%s\\n' ${quoteForPosixShell(
      `KanVibe: cannot enter ${worktreePath}; starting the shell in the default directory.`,
    )} >&2;`,
    launchLoginShell,
  ].join(" ");
}

/**
 * 원격 tmux 세션에 붙는 명령을 만든다.
 * 세션이 이미 있으면 그대로 attach하고, 없으면 생성부터 attach까지를 tmux 호출 한 번으로 처리한다.
 * 사용자 설정 때문에 실패하면 소켓은 그대로 둔 채 설정 파일 없이 한 번 더 시도한다.
 * 재시도는 원격에 tmux 서버가 아직 없을 때만 설정을 실제로 건너뛴다(`TmuxSessionBootstrapOptions` 참고).
 *
 * OSC 52 판정은 원격 설정 파일을 읽는 셸 한 줄이라, 앱에서 미리 SSH로 물어보면
 * 세션이 살아 있어 결과를 쓰지도 않는 재attach에까지 왕복 지연이 붙는다.
 * 그래서 판정을 세션 생성 분기 안으로 내려 사용 지점과 같은 원격 셸에서 수행한다.
 */
function buildRemoteTmuxAttachCommand(
  sessionName: string,
  worktreePath?: string | null,
  tmuxPaneLayout?: TmuxPaneLayoutConfig | null,
): string {
  const quotedSessionName = quoteForPosixShell(sessionName);
  const buildBootstrapWithFallback = (enableClipboard: boolean) => {
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

    return `${bootstrapWithRetry} || { ${buildRemoteInteractiveShellFallbackCommand()}; };`;
  };

  return [
    `if tmux has-session -t ${quotedSessionName} 2>/dev/null; then`,
    `exec tmux attach-session -t ${quotedSessionName};`,
    `elif ${buildTmuxUserClipboardDirectiveTestCommand()}; then ${buildBootstrapWithFallback(false)}`,
    `else ${buildBootstrapWithFallback(true)}`,
    "fi",
  ].join(" ");
}

/**
 * 원격 zellij 세션에 붙는 명령을 만든다.
 * 로컬과 달리 attach만 하면 세션이 없을 때 SSH가 그대로 끊기므로, 로컬처럼 생성과 레이아웃 적용까지 처리한다.
 *
 * 존재 확인과 attach를 나눈 이유가 두 가지다.
 * attach를 조건문에 두면 stderr 리다이렉트가 대화형 세션이 사는 내내 유지되어 zellij 오류가 전부 사라지고,
 * attach의 종료 코드를 "세션 없음" 신호로 재사용하게 되어 사용자가 비정상 코드로 빠져나오면 새 세션이 생긴다.
 */
function buildRemoteZellijAttachCommand(
  sessionName: string,
  worktreePath?: string | null,
): string {
  const quotedSessionName = quoteForPosixShell(sessionName);
  const createArguments = ["--session", quotedSessionName];
  const attachWhenAlive = [
    `if ${buildZellijAliveSessionCheckCommand(sessionName)}; then`,
    `exec zellij attach ${quotedSessionName};`,
    "fi;",
  ].join(" ");

  if (worktreePath) {
    const quotedWorktreePath = quoteForPosixShell(worktreePath);
    const layoutFile = quoteForPosixShell(path.posix.join(worktreePath, ZELLIJ_LAYOUT_FILENAME));
    /** 레이아웃 파일 경로가 절대경로라 cd 실패는 조용히 통과한다. 작업 디렉터리가 어긋난 이유가 보이도록 남긴다 */
    const changeDirectory = [
      `cd ${quotedWorktreePath} ||`,
      `printf '%s\\n' ${quoteForPosixShell(
        `KanVibe: cannot enter ${worktreePath}; starting the zellij session in the default directory.`,
      )} >&2;`,
    ].join(" ");

    return [
      attachWhenAlive,
      changeDirectory,
      `if [ -f ${layoutFile} ]; then`,
      `exec zellij ${createArguments.join(" ")} --new-session-with-layout ${layoutFile};`,
      "else",
      `exec zellij ${createArguments.join(" ")};`,
      "fi",
    ].join(" ");
  }

  return `${attachWhenAlive} exec zellij ${createArguments.join(" ")}`;
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
  void taskId;
}

/**
 * 태스크에 딸린 터미널을 모두 종료한다. 탭이 여러 개인 terminal 세션도 함께 정리한다.
 * @param triggerLabel 누가 detach를 호출했는지 표시 (진단용). 예: "cleanup-task-resources", "closeWindowTerminals"
 */
export function detachSession(taskId: string, triggerLabel?: string): void {
  const taskTerminalKeys = [...activeTerminals.entries()]
    .filter(([, entry]) => entry.taskId === taskId)
    .map(([terminalKey]) => terminalKey);

  if (taskTerminalKeys.length === 0) {
    debugLog("detachSession 호출 (entry 없음)", { taskId, triggerLabel });
  }

  for (const terminalKey of taskTerminalKeys) {
    destroyTerminal(terminalKey, triggerLabel);
  }

  localTerminalTabs.delete(taskId);
}

/** 활성 터미널 수를 반환한다 */
export function getActiveTerminalCount(): number {
  return activeTerminals.size;
}
