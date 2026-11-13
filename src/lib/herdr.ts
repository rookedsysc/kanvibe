import { connect } from "net";
import { spawn } from "child_process";
import { mkdtemp, rm, stat } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { setTimeout as delay } from "timers/promises";
import { PaneLayoutType } from "@/entities/PaneLayoutConfig";
import { execGit, isSSHTransportError } from "@/lib/gitOperations";
import type { TmuxPaneLayoutConfig } from "@/lib/worktree";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig } from "@/lib/sshConfig";
import { quoteShellArgument, readTextFile, updateTextFile } from "@/lib/hostFileAccess";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import type { MirrorPane } from "@/lib/paneMirror";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";
import type { AiSessionProvider, RunningAgentPane } from "@/lib/aiSessions/types";

const HERDR_COMMAND_TIMEOUT_MS = 5_000;
const HERDR_START_TIMEOUT_MS = 30_000;
const sessionBootstraps = new Map<string, Promise<void>>();
const sharedBootstraps = new Map<string, Promise<void>>();
const workspaceMappingPaths = new Map<string, string>();
let remoteMachineRegistration = Promise.resolve();

const HERDR_EXECUTABLE = `"$(command -v herdr || printf '%s' "$HOME/.local/bin/herdr")"`;

export function parseHerdrSessionTarget(sessionName: string): { serverName: string; workspaceKey: string | null } {
  if (!sessionName.startsWith("kanvibe:")) return { serverName: sessionName, workspaceKey: null };
  const workspaceKey = sessionName.slice("kanvibe:".length);
  if (!workspaceKey) throw new Error("Herdr Space 식별자가 비어 있습니다.");
  return { serverName: "kanvibe", workspaceKey };
}

export function buildHerdrCommand(sessionName: string, arguments_: string[]): string {
  const { serverName } = parseHerdrSessionTarget(sessionName);
  return `env -u HERDR_ENV -u HERDR_SESSION -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID ${HERDR_EXECUTABLE} --session ${quoteShellArgument(serverName)} ${arguments_.map(quoteShellArgument).join(" ")}`;
}

export function buildHerdrSessionCleanupCommand(sessionName: string): string {
  if (parseHerdrSessionTarget(sessionName).workspaceKey) throw new Error("공유 Herdr 서버는 task와 함께 종료할 수 없습니다.");
  return `${buildHerdrCommand(sessionName, ["session", "stop", sessionName])} >/dev/null 2>&1 || true; ${buildHerdrCommand(sessionName, ["session", "delete", sessionName])}`;
}

export function buildHerdrClientCommand(sessionName: string, socketPath: string): string {
  const { serverName } = parseHerdrSessionTarget(sessionName);
  // --session은 client socket override를 무시하므로 이 클라이언트의 env로만 지정한다.
  return `env -u HERDR_ENV -u HERDR_SOCKET_PATH -u HERDR_PANE_ID -u HERDR_WORKSPACE_ID -u HERDR_TAB_ID HERDR_SESSION=${quoteShellArgument(serverName)} HERDR_CLIENT_SOCKET_PATH=${quoteShellArgument(socketPath)} ${HERDR_EXECUTABLE} client`;
}

export async function readHerdrClientTabBarPosition(sshHost?: string): Promise<"top" | "bottom" | null> {
  try {
    const configPath = await execGit('printf "%s" "${HERDR_CONFIG_PATH:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr/config.toml}"', sshHost, { timeoutMs: HERDR_COMMAND_TIMEOUT_MS });
    if (!path.posix.isAbsolute(configPath)) return null;
    const target = quoteShellArgument(configPath);
    const content = await execGit(`if test -e ${target}; then cat ${target}; fi`, sshHost, { timeoutMs: HERDR_COMMAND_TIMEOUT_MS });
    let section = "";
    let position: "top" | "bottom" = "top";
    let foundPosition = false;
    for (const original of content.split(/\r?\n/)) {
      const line = original.trim();
      if (!line || line.startsWith("#")) continue;
      // 해석할 수 없는 TOML에서 위치를 추측하면 pane 밖을 덮을 수 있다.
      if (line.includes('"""') || line.includes("'''")) return null;
      if (line.startsWith("[")) {
        const table = /^\[([^\]]+)\]\s*(?:#.*)?$/.exec(line);
        if (table?.[1].includes("\\")) return null;
        section = table?.[1].trim().replace(/^(["'])(.*)\1$/, "$2") ?? "";
        continue;
      }
      if (!section && /^(?:ui|"ui"|'ui')\s*=/.test(line)) return null;
      if (section === "ui" && /^["'][^=]*\\/.test(line)) return null;
      const key = section === "ui"
        ? /^(?:tab_bar_position|"tab_bar_position"|'tab_bar_position')\s*=\s*(.*)$/.exec(line)
        : !section ? /^(?:ui|"ui"|'ui')\s*\.\s*(?:tab_bar_position|"tab_bar_position"|'tab_bar_position')\s*=\s*(.*)$/.exec(line) : null;
      if (!key) continue;
      const value = /^(?:"(top|bottom)"|'(top|bottom)')\s*(?:#.*)?$/.exec(key[1]);
      if (!value || foundPosition) return null;
      position = (value[1] ?? value[2]) as "top" | "bottom";
      foundPosition = true;
    }
    return position;
  } catch {
    return null;
  }
}

interface HerdrTab {
  tab_id: string;
  number: number;
  label: string;
  focused: boolean;
}

interface HerdrSession {
  name: string;
  running: boolean;
  socket_path: string;
  session_dir?: string;
}

function readHerdrResult<T>(output: string): T {
  const response = JSON.parse(output);
  if (response.error) {
    throw new Error(`Herdr: ${response.error.message ?? response.error.code}`);
  }
  if (!response.result) {
    throw new Error("Herdr 응답에 result가 없습니다.");
  }
  return response.result as T;
}

export async function runHerdrCommand<T>(sessionName: string, arguments_: string[], sshHost?: string | null): Promise<T> {
  const { workspaceKey } = parseHerdrSessionTarget(sessionName);
  if (workspaceKey) {
    const workspaceId = await getHerdrWorkspaceId(sessionName, sshHost);
    const paneIndex = arguments_.indexOf("--pane");
    const targetId = paneIndex >= 0 ? arguments_[paneIndex + 1] : arguments_[2];
    const workspaceIndex = arguments_.indexOf("--workspace");
    if ((targetId && /^w[0-9A-Za-z]+:(?:t|p)[0-9A-Za-z]+$/.test(targetId) && !targetId.startsWith(`${workspaceId}:`))
      || (workspaceIndex >= 0 && arguments_[workspaceIndex + 1] !== workspaceId)) {
      throw new Error("다른 Space의 터미널을 변경할 수 없습니다.");
    }
    if (arguments_[0] === "tab" && arguments_[1] === "list" && workspaceIndex < 0) {
      arguments_ = [...arguments_, "--workspace", workspaceId!];
    }
  }
  const output = await execGit(buildHerdrCommand(sessionName, arguments_), sshHost, { timeoutMs: HERDR_COMMAND_TIMEOUT_MS });
  // send-text는 전송 성공 시 본문을 출력하지 않는다.
  if (!output && arguments_[0] === "pane" && arguments_[1] === "send-text") return undefined as T;
  return readHerdrResult<T>(output);
}

export async function readHerdrTabs(sessionName: string, sshHost?: string | null): Promise<TerminalTab[]> {
  const result = await runHerdrCommand<{ tabs: HerdrTab[] }>(sessionName, ["tab", "list"], sshHost);
  return result.tabs.map((tab, index) => ({
    id: tab.tab_id,
    nativeIndex: index,
    name: tab.label,
    isActive: tab.focused,
  }));
}

interface HerdrPane {
  pane_id: string;
  tab_id: string;
  /** 작업 경로가 사라져 복원하지 못한 pane에만 있다. 이런 pane에는 터미널도 scroll도 없다 */
  restore_error?: string;
  scroll: { max_offset_from_bottom: number; viewport_rows: number };
}

interface HerdrRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface HerdrTabLayout {
  area: HerdrRect;
  focused_pane_id: string;
  zoomed: boolean;
  panes: { pane_id: string; rect: HerdrRect }[];
}

/**
 * 모바일이 비출 pane을 탭 순서대로 모은다.
 * `pane list`에는 좌표와 실행 명령이 없어 탭마다 `pane layout`, pane마다 `process-info`를 함께 읽는다.
 */
export async function listHerdrMirrorPanes(sessionName: string, sshHost?: string | null): Promise<MirrorPane[]> {
  const [{ panes }, tabs] = await Promise.all([
    runHerdrCommand<{ panes: HerdrPane[] }>(sessionName, ["pane", "list"], sshHost),
    readHerdrTabs(sessionName, sshHost),
  ]);
  const workspaceId = await getHerdrWorkspaceId(sessionName, sshHost);
  const panesByTab = await Promise.all(tabs.map(async (tab) => {
    const tabPanes = panes.filter((pane) => pane.tab_id === tab.id && !pane.restore_error && (!workspaceId || pane.pane_id.startsWith(`${workspaceId}:`)));
    if (tabPanes.length === 0) return [];
    const { layout } = await runHerdrCommand<{ layout: HerdrTabLayout }>(sessionName, ["pane", "layout", "--pane", tabPanes[0].pane_id], sshHost);
    return Promise.all(tabPanes.map(async (pane): Promise<MirrorPane> => {
      const rect = layout.panes.find((entry) => entry.pane_id === pane.pane_id)?.rect;
      /** 확대해도 layout의 pane 좌표는 그대로라, 확대된 pane의 너비는 탭 전체 영역에서 읽는다 */
      const isZoomedPane = layout.zoomed && layout.focused_pane_id === pane.pane_id;
      return {
        id: pane.pane_id,
        tabId: tab.id,
        tabName: tab.name,
        command: await readHerdrPaneCommand(sessionName, pane.pane_id, sshHost),
        left: rect?.x ?? 0,
        top: rect?.y ?? 0,
        width: isZoomedPane ? layout.area.width : rect?.width ?? layout.area.width,
        height: pane.scroll.viewport_rows,
      };
    }));
  }));
  return panesByTab.flat();
}

/** 실행 명령은 탭 이름 표시용이라 읽지 못해도 pane 목록을 막지 않는다 */
async function readHerdrPaneCommand(sessionName: string, paneId: string, sshHost?: string | null): Promise<string> {
  try {
    const { process_info: processInfo } = await runHerdrCommand<{ process_info: { foreground_processes?: { name?: string }[] } }>(
      sessionName, ["pane", "process-info", "--pane", paneId], sshHost,
    );
    return processInfo.foreground_processes?.at(-1)?.name ?? "";
  } catch {
    return "";
  }
}

export interface HerdrPaneExtent {
  historyRows: number;
  viewportRows: number;
}

/** 지난 출력 줄 수와 화면 높이. 모바일 화면을 다시 받아야 하는지 판단할 때 쓴다 */
export async function readHerdrPaneExtent(sessionName: string, paneId: string, sshHost?: string | null): Promise<HerdrPaneExtent> {
  const { pane } = await runHerdrCommand<{ pane: HerdrPane }>(sessionName, ["pane", "get", paneId], sshHost);
  return { historyRows: pane.scroll.max_offset_from_bottom, viewportRows: pane.scroll.viewport_rows };
}

function parseWorkspaceMapping(content: string): Record<string, string> {
  if (!content) return {};
  const parsed: unknown = JSON.parse(content);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)
    || Object.values(parsed).some((id) => typeof id !== "string" || !/^w[0-9A-Za-z]+$/.test(id))) {
    throw new Error("Herdr Space 연결 파일이 올바르지 않습니다.");
  }
  return parsed as Record<string, string>;
}

async function getWorkspaceMappingPath(sshHost?: string | null): Promise<string> {
  const key = sshHost ?? "";
  const cached = workspaceMappingPaths.get(key);
  if (cached) return cached;
  const shared = (await listHerdrSessions(sshHost)).find((session) => session.name === "kanvibe");
  if (!shared?.session_dir) throw new Error("공유 Herdr 세션 경로를 찾을 수 없습니다.");
  const mappingPath = path.posix.join(shared.session_dir, "kanvibe-workspaces.json");
  workspaceMappingPaths.set(key, mappingPath);
  return mappingPath;
}

export async function getHerdrWorkspaceId(sessionName: string, sshHost?: string | null): Promise<string | null> {
  const { workspaceKey } = parseHerdrSessionTarget(sessionName);
  if (!workspaceKey) return null;
  const mapping = parseWorkspaceMapping(await readTextFile(await getWorkspaceMappingPath(sshHost), sshHost));
  if (!Object.hasOwn(mapping, workspaceKey)) throw new Error("task에 연결된 Herdr Space가 없습니다.");
  return mapping[workspaceKey];
}

export async function getHerdrClientSocketPath(sessionName: string, sshHost?: string | null): Promise<string> {
  const { serverName } = parseHerdrSessionTarget(sessionName);
  const session = (await listHerdrSessions(sshHost)).find((entry) => entry.name === serverName && entry.running);
  if (!session) throw new Error("Herdr 세션을 찾을 수 없습니다.");
  return path.posix.join(path.posix.dirname(session.socket_path), "herdr-client.sock");
}

export async function isHerdrTaskAlive(sessionName: string, sshHost?: string | null): Promise<boolean> {
  try {
    const workspaceId = await getHerdrWorkspaceId(sessionName, sshHost);
    const { workspaces } = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(
      parseHerdrSessionTarget(sessionName).serverName, ["workspace", "list"], sshHost,
    );
    return workspaceId === null || workspaces.some((workspace) => workspace.workspace_id === workspaceId);
  } catch {
    return false;
  }
}

export async function closeHerdrTaskWorkspace(sessionName: string, sshHost?: string | null): Promise<void> {
  const { serverName, workspaceKey } = parseHerdrSessionTarget(sessionName);
  if (!workspaceKey) {
    await execGit(buildHerdrSessionCleanupCommand(sessionName), sshHost, { timeoutMs: HERDR_START_TIMEOUT_MS });
    return;
  }
  const mappingPath = await getWorkspaceMappingPath(sshHost);
  await updateTextFile(mappingPath, async (content) => {
    const mapping = parseWorkspaceMapping(content);
    if (!Object.hasOwn(mapping, workspaceKey)) return content;
    const { workspaces } = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(serverName, ["workspace", "list"], sshHost);
    if (workspaces.some((workspace) => workspace.workspace_id === mapping[workspaceKey])) {
      await runHerdrCommand(serverName, ["workspace", "close", mapping[workspaceKey]], sshHost);
    }
    delete mapping[workspaceKey];
    return JSON.stringify(mapping);
  }, sshHost, 0o600);
}

export function ensureHerdrSession(
  sessionName: string,
  cwd: string,
  layout?: TmuxPaneLayoutConfig | null,
  sshHost?: string | null,
): Promise<void> {
  const key = JSON.stringify([sshHost ?? null, sessionName]);
  const current = sessionBootstraps.get(key);
  if (current) return current;
  const sharedKey = sshHost ?? "";
  const previous = parseHerdrSessionTarget(sessionName).workspaceKey ? sharedBootstraps.get(sharedKey) : undefined;
  const bootstrap = (previous ? previous.catch(() => {}).then(() => bootstrapHerdrSession(sessionName, cwd, layout, sshHost))
    : bootstrapHerdrSession(sessionName, cwd, layout, sshHost)).finally(() => {
    sessionBootstraps.delete(key);
    if (sharedBootstraps.get(sharedKey) === bootstrap) sharedBootstraps.delete(sharedKey);
  });
  if (parseHerdrSessionTarget(sessionName).workspaceKey) sharedBootstraps.set(sharedKey, bootstrap);
  sessionBootstraps.set(key, bootstrap);
  return bootstrap;
}

export function ensureHerdrRemoteMachine(sessionName: string, sshHost: string): Promise<void> {
  const { serverName } = parseHerdrSessionTarget(sessionName);
  remoteMachineRegistration = remoteMachineRegistration.catch(() => {}).then(async () => {
    const machines: { target: string; session: string }[] = JSON.parse(await execGit(
      buildHerdrCommand(serverName, ["machine", "list", "--json"]), null, { timeoutMs: HERDR_COMMAND_TIMEOUT_MS },
    ));
    if (machines.some((machine) => machine.target === sshHost && machine.session === serverName)) return;
    await execGit(
      `${buildHerdrCommand(serverName, ["machine", "add", sshHost, "--remote-session", serverName])} </dev/null`,
      null, { timeoutMs: HERDR_START_TIMEOUT_MS },
    );
  });
  return remoteMachineRegistration;
}

async function startHerdrServer(sessionName: string, sshHost?: string | null): Promise<void> {
  const command = buildHerdrCommand(sessionName, ["server"]);
  if (sshHost) {
    await execGit(`nohup ${command} </dev/null >/dev/null 2>&1 &`, sshHost);
    return;
  }
  // nohup만 쓰면 앱의 프로세스 그룹에 남아 종료 신호를 함께 받는다.
  await new Promise<void>((resolve, reject) => {
    const server = spawn("/bin/sh", ["-c", `exec ${command}`], {
      detached: true,
      stdio: "ignore",
      env: createLocalShellEnvironment(),
    });
    server.once("error", reject);
    server.once("spawn", () => {
      server.unref();
      resolve();
    });
  });
}

async function bootstrapHerdrSession(sessionName: string, cwd: string, layout?: TmuxPaneLayoutConfig | null, sshHost?: string | null): Promise<void> {
  const { serverName, workspaceKey } = parseHerdrSessionTarget(sessionName);
  let workspaces: { workspace_id: string }[];
  try {
    ({ workspaces } = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(serverName, ["workspace", "list"], sshHost));
  } catch (error) {
    if (isSSHTransportError(error)) throw error;
    await startHerdrServer(serverName, sshHost);
    const deadline = Date.now() + HERDR_START_TIMEOUT_MS;
    for (;;) {
      try {
        ({ workspaces } = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(serverName, ["workspace", "list"], sshHost));
        break;
      } catch (startError) {
        if (isSSHTransportError(startError) || Date.now() >= deadline) throw startError;
        await delay(100);
      }
    }
  }
  if (!workspaceKey) {
    if (workspaces.length === 0) await createHerdrWorkspace(serverName, cwd, layout, null, sshHost);
    return;
  }
  const mappingPath = await getWorkspaceMappingPath(sshHost);
  parseWorkspaceMapping(await readTextFile(mappingPath, sshHost));
  let createdId: string | undefined;
  try {
    await updateTextFile(mappingPath, async (content) => {
      const mapping = parseWorkspaceMapping(content);
      const live = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(serverName, ["workspace", "list"], sshHost);
      if (Object.hasOwn(mapping, workspaceKey) && live.workspaces.some((space) => space.workspace_id === mapping[workspaceKey])) return content;
      createdId = await createHerdrWorkspace(serverName, cwd, layout, workspaceKey, sshHost);
      return JSON.stringify({ ...mapping, [workspaceKey]: createdId });
    }, sshHost, 0o600);
  } catch (error) {
    if (createdId) await runHerdrCommand(serverName, ["workspace", "close", createdId], sshHost);
    throw error;
  }
}

async function createHerdrWorkspace(serverName: string, cwd: string, layout: TmuxPaneLayoutConfig | null | undefined, workspaceKey: string | null, sshHost?: string | null): Promise<string> {
  const created = await runHerdrCommand<{ root_pane: { pane_id: string } }>(serverName, ["workspace", "create", "--cwd", cwd,
    ...(workspaceKey ? ["--label", workspaceKey, "--no-focus"] : ["--focus"])], sshHost);
  const workspaceId = created.root_pane.pane_id.split(":")[0];
  try {
    await applyHerdrLayout(serverName, created.root_pane.pane_id, cwd, layout, sshHost);
    return workspaceId;
  } catch (error) {
    if (workspaceKey) await runHerdrCommand(serverName, ["workspace", "close", workspaceId], sshHost);
    else await execGit(buildHerdrSessionCleanupCommand(serverName), sshHost, { timeoutMs: HERDR_START_TIMEOUT_MS });
    throw error;
  }
}

async function applyHerdrLayout(sessionName: string, rootPaneId: string, cwd: string, layout?: TmuxPaneLayoutConfig | null, sshHost?: string | null): Promise<void> {
  if (!layout) return;
  const split = async (paneId: string, direction: "right" | "down") => {
    const result = await runHerdrCommand<{ pane: { pane_id: string } }>(sessionName, ["pane", "split", paneId, "--direction", direction, "--cwd", cwd], sshHost);
    return result.pane.pane_id;
  };
  let paneIds = [rootPaneId];
  switch (layout.layoutType) {
    case PaneLayoutType.HORIZONTAL_2:
      paneIds.push(await split(rootPaneId, "down"));
      break;
    case PaneLayoutType.VERTICAL_2:
      paneIds.push(await split(rootPaneId, "right"));
      break;
    case PaneLayoutType.LEFT_RIGHT_TB: {
      const right = await split(rootPaneId, "right");
      paneIds = [rootPaneId, right, await split(right, "down")];
      break;
    }
    case PaneLayoutType.LEFT_TB_RIGHT: {
      const right = await split(rootPaneId, "right");
      paneIds = [rootPaneId, await split(rootPaneId, "down"), right];
      break;
    }
    case PaneLayoutType.QUAD: {
      const right = await split(rootPaneId, "right");
      paneIds = [rootPaneId, right, await split(rootPaneId, "down"), await split(right, "down")];
      break;
    }
  }
  for (const pane of layout.panes) {
    if (pane.command.trim() && paneIds[pane.position]) {
      // 초기 로그인 셸에는 bracketed-paste 대신 원문과 Enter를 한 번에 버퍼링한다.
      await runHerdrCommand(sessionName, ["pane", "send-text", paneIds[pane.position], `${pane.command}\r`], sshHost);
    }
  }
}

async function listHerdrSessions(sshHost?: string | null): Promise<HerdrSession[]> {
  const output = await execGit(buildHerdrCommand("default", ["session", "list", "--json"]), sshHost, { timeoutMs: HERDR_COMMAND_TIMEOUT_MS });
  const response = JSON.parse(output);
  if (!Array.isArray(response.sessions)) throw new Error("Herdr 세션 목록이 올바르지 않습니다.");
  return response.sessions;
}

function requestHerdrSocket(socketPath: string, method: string, params: Record<string, unknown>): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const socket = connect(socketPath);
    let buffer = "";
    socket.setEncoding("utf8");
    socket.setTimeout(HERDR_COMMAND_TIMEOUT_MS, () => socket.destroy(new Error("Herdr 소켓 응답 시간 초과")));
    socket.once("error", reject);
    socket.once("close", () => reject(new Error("Herdr 소켓 응답이 끊겼습니다.")));
    socket.once("connect", () => socket.write(`${JSON.stringify({ id: "kanvibe", method, params })}\n`));
    socket.on("data", (chunk) => {
      buffer += chunk;
      if (buffer.length > 1024 * 1024) {
        socket.destroy(new Error("Herdr 응답 크기 초과"));
        return;
      }
      const end = buffer.indexOf("\n");
      if (end < 0) return;
      try {
        const response = JSON.parse(buffer.slice(0, end));
        if (response.id !== "kanvibe") throw new Error("Herdr 소켓 응답 ID가 일치하지 않습니다.");
        resolve(readHerdrResult(buffer.slice(0, end)));
      } catch (error) {
        reject(error);
      } finally {
        socket.destroy();
      }
    });
  });
}

export async function moveHerdrTab(sessionName: string, tabId: string, targetIndex: number, sshHost?: string | null): Promise<void> {
  const session = (await listHerdrSessions(sshHost)).find((entry) => entry.name === parseHerdrSessionTarget(sessionName).serverName && entry.running);
  if (!session) throw new Error("Herdr 세션을 찾을 수 없습니다.");
  const workspaceId = await getHerdrWorkspaceId(sessionName, sshHost);
  if (workspaceId && !tabId.startsWith(`${workspaceId}:`)) throw new Error("다른 Space의 탭을 이동할 수 없습니다.");
  const params = { tab_id: tabId, insert_index: targetIndex };
  if (!sshHost) {
    await requestHerdrSocket(session.socket_path, "tab.move", params);
    return;
  }
  const host = (await parseSSHConfig()).find((entry) => entry.host === sshHost);
  if (!host) throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
  const directory = await mkdtemp(path.join(tmpdir(), "kvh-"));
  const localSocket = path.join(directory, "api");
  const sshArgs = buildSSHArgs(host, { disableTty: true, connectionHealth: getKanvibeSSHConnectionHealthOptions() });
  sshArgs.splice(sshArgs.length - 1, 0, "-N", "-o", "ExitOnForwardFailure=yes", "-o", "ControlMaster=no", "-o", "ControlPath=none", "-L", `${localSocket}:${session.socket_path}`);
  const tunnel = spawn("ssh", sshArgs, { env: createLocalShellEnvironment(), stdio: ["ignore", "ignore", "pipe"] });
  const tunnelClosed = new Promise<void>((resolve) => tunnel.once("close", () => resolve()));
  let tunnelError: Error | null = null;
  let stderr = "";
  tunnel.stderr.on("data", (chunk) => { stderr = (stderr + chunk.toString()).slice(-4096); });
  tunnel.once("error", (error) => { tunnelError = error; });
  tunnel.once("exit", () => { tunnelError ??= new Error(stderr || "Herdr SSH 소켓 연결이 종료됐습니다."); });
  try {
    const deadline = Date.now() + HERDR_START_TIMEOUT_MS;
    while (!(await stat(localSocket).catch(() => null))?.isSocket()) {
      if (tunnelError) throw tunnelError;
      if (Date.now() >= deadline) throw new Error("Herdr SSH 소켓 연결 시간 초과");
      await delay(25);
    }
    await requestHerdrSocket(localSocket, "tab.move", params);
  } finally {
    tunnel.kill();
    const forceKill = setTimeout(() => tunnel.kill("SIGKILL"), 1_000);
    await tunnelClosed.finally(() => clearTimeout(forceKill));
    await rm(directory, { recursive: true, force: true });
  }
}

export async function listHerdrRunningAgentPanes(sshHost?: string | null): Promise<RunningAgentPane[]> {
  const providers = new Set<AiSessionProvider>(["claude", "codex", "opencode", "gemini"]);
  const sessions = await listHerdrSessions(sshHost).catch(() => []);
  const panes = await Promise.all(sessions.filter((session) => session.running).map(async (session) => {
    try {
      const [result, tabs] = await Promise.all([
        runHerdrCommand<{ agents: { agent: string; pane_id: string; tab_id: string; cwd?: string; foreground_cwd?: string; agent_status: string }[] }>(session.name, ["agent", "list"], sshHost),
        readHerdrTabs(session.name, sshHost),
      ]);
      const mapping = session.name === "kanvibe"
        ? parseWorkspaceMapping(await readTextFile(await getWorkspaceMappingPath(sshHost), sshHost)) : {};
      return result.agents.filter((agent) => providers.has(agent.agent as AiSessionProvider) && (agent.foreground_cwd || agent.cwd)).map((agent) => {
        const workspaceKey = Object.entries(mapping).find(([, id]) => agent.pane_id.startsWith(`${id}:`))?.[0];
        return {
        provider: agent.agent as AiSessionProvider,
        worktreePath: (agent.foreground_cwd || agent.cwd)!,
        sessionName: workspaceKey ? `kanvibe:${workspaceKey}` : session.name,
        windowId: agent.tab_id,
        windowName: tabs.find((tab) => tab.id === agent.tab_id)?.name ?? agent.tab_id,
        };
      });
    } catch {
      return [];
    }
  }));
  return panes.flat();
}
