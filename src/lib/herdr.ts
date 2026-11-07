import { connect } from "net";
import { spawn } from "child_process";
import { mkdtemp, rm, stat } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { setTimeout as delay } from "timers/promises";
import { execGit, isSSHTransportError } from "@/lib/gitOperations";
import { buildHerdrCommand, buildHerdrSessionCleanupCommand, type TmuxPaneLayoutConfig } from "@/lib/worktree";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig } from "@/lib/sshConfig";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import type { TerminalTab } from "@/desktop/shared/terminalTabs";
import type { AiSessionProvider, RunningAgentPane } from "@/lib/aiSessions/types";

const HERDR_COMMAND_TIMEOUT_MS = 5_000;
const HERDR_START_TIMEOUT_MS = 30_000;
const sessionBootstraps = new Map<string, Promise<void>>();

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

export function ensureHerdrSession(
  sessionName: string,
  cwd: string,
  layout?: TmuxPaneLayoutConfig | null,
  sshHost?: string | null,
): Promise<void> {
  const key = JSON.stringify([sshHost ?? null, sessionName]);
  const current = sessionBootstraps.get(key);
  if (current) return current;
  const bootstrap = bootstrapHerdrSession(sessionName, cwd, layout, sshHost).finally(() => sessionBootstraps.delete(key));
  sessionBootstraps.set(key, bootstrap);
  return bootstrap;
}

async function bootstrapHerdrSession(sessionName: string, cwd: string, layout?: TmuxPaneLayoutConfig | null, sshHost?: string | null): Promise<void> {
  let workspaces: { workspace_id: string }[];
  try {
    ({ workspaces } = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(sessionName, ["workspace", "list"], sshHost));
  } catch (error) {
    if (isSSHTransportError(error)) throw error;
    await execGit(`nohup ${buildHerdrCommand(sessionName, ["server"])} </dev/null >/dev/null 2>&1 &`, sshHost);
    const deadline = Date.now() + HERDR_START_TIMEOUT_MS;
    for (;;) {
      try {
        ({ workspaces } = await runHerdrCommand<{ workspaces: { workspace_id: string }[] }>(sessionName, ["workspace", "list"], sshHost));
        break;
      } catch (startError) {
        if (isSSHTransportError(startError) || Date.now() >= deadline) throw startError;
        await delay(100);
      }
    }
  }
  if (workspaces.length > 0) return;
  const created = await runHerdrCommand<{ root_pane: { pane_id: string } }>(sessionName, ["workspace", "create", "--cwd", cwd, "--focus"], sshHost);
  try {
    await applyHerdrLayout(sessionName, created.root_pane.pane_id, cwd, layout, sshHost);
  } catch (error) {
    await execGit(buildHerdrSessionCleanupCommand(sessionName), sshHost, { timeoutMs: HERDR_START_TIMEOUT_MS });
    throw error;
  }
}

async function applyHerdrLayout(sessionName: string, rootPaneId: string, cwd: string, layout?: TmuxPaneLayoutConfig | null, sshHost?: string | null): Promise<void> {
  if (!layout) return;
  const { PaneLayoutType } = await import("@/entities/PaneLayoutConfig");
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
  const session = (await listHerdrSessions(sshHost)).find((entry) => entry.name === sessionName && entry.running);
  if (!session) throw new Error("Herdr 세션을 찾을 수 없습니다.");
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
      return result.agents.filter((agent) => providers.has(agent.agent as AiSessionProvider) && (agent.foreground_cwd || agent.cwd)).map((agent) => ({
        provider: agent.agent as AiSessionProvider,
        worktreePath: (agent.foreground_cwd || agent.cwd)!,
        sessionName: session.name,
        windowId: agent.tab_id,
        windowName: tabs.find((tab) => tab.id === agent.tab_id)?.name ?? agent.tab_id,
      }));
    } catch {
      return [];
    }
  }));
  return panes.flat();
}
