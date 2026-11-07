import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import http, { type RequestListener } from "node:http";
import path from "node:path";
import { IsNull, Not } from "typeorm";
import { TaskStatus } from "@/entities/KanbanTask";
import { execGit } from "@/lib/gitOperations";
import { getRemoteHookServerUrl } from "@/lib/hookEndpoint";
import { buildRemoteHookSocketPrepareCommand } from "@/lib/hookSocketPath";
import { getTaskRepository } from "@/lib/database";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import { buildSSHArgs, parseSSHConfig } from "@/lib/sshConfig";

/**
 * 원격 호스트의 hook·CLI가 평문 포트 대신 SSH 안으로 데스크탑에 돌아오도록 호스트별 `ssh -R` 터널을 유지한다.
 *
 * 호스트마다 로컬 소켓을 따로 두어, 요청이 어느 호스트에서 왔는지를 소켓만으로 알 수 있게 한다.
 * 터널은 HTTP를 대체하는 최적 경로일 뿐이므로 실패해도 원격 스크립트는 HTTP로 넘어간다.
 */
const TUNNEL_RECONCILE_INTERVAL_MS = 5 * 60_000;
const TUNNEL_RECONNECT_MIN_DELAY_MS = 5_000;
const TUNNEL_RECONNECT_MAX_DELAY_MS = 5 * 60_000;
/** 이만큼 살아 있었으면 정상 연결로 보고 재연결 대기를 처음 값으로 되돌린다 */
const TUNNEL_STABLE_AFTER_MS = 30_000;
const TUNNEL_SERVER_ALIVE_INTERVAL_SECONDS = 15;
const TUNNEL_SERVER_ALIVE_COUNT_MAX = 2;
const TUNNEL_CONNECT_TIMEOUT_SECONDS = 10;
/** Unix 소켓 경로 길이 제한(약 104자)을 넘지 않도록 os.tmpdir() 대신 짧은 경로를 쓴다 */
const LOCAL_SOCKET_DIRECTORY_PREFIX = "/tmp/kvh-";

interface HookTunnel {
  sshHost: string;
  localServer: http.Server;
  localSocketPath: string;
  sshProcess: ChildProcess | null;
  reconnectTimer: NodeJS.Timeout | null;
  reconnectDelayMs: number;
  isClosed: boolean;
}

const tunnelsByHost = new Map<string, HookTunnel>();
let createHostRequestListener: ((sshHost: string) => RequestListener) | null = null;
let localSocketDirectory: string | null = null;
let reconcileTimer: NodeJS.Timeout | null = null;

export function buildHookTunnelSSHArgs(
  baseArgs: string[],
  remoteSocketPath: string,
  localSocketPath: string,
): string[] {
  const destination = baseArgs[baseArgs.length - 1];
  return [
    ...baseArgs.slice(0, -1),
    "-o", "ExitOnForwardFailure=yes",
    "-o", "StreamLocalBindUnlink=yes",
    /** 사용자 설정의 ControlMaster를 타면 터널의 수명이 이 프로세스와 분리된다 */
    "-o", "ControlMaster=no",
    "-o", "ControlPath=none",
    "-R", `${remoteSocketPath}:${localSocketPath}`,
    destination,
    /** 앱이 비정상 종료되면 stdin이 닫혀 cat이 끝나고, 그러면 ssh도 함께 끝나 터널이 남지 않는다 */
    "cat >/dev/null",
  ];
}

async function connectHookTunnel(tunnel: HookTunnel): Promise<void> {
  const configs = await parseSSHConfig();
  const hostConfig = configs.find((config) => config.host === tunnel.sshHost);
  if (!hostConfig) {
    throw new Error(`SSH 호스트를 찾을 수 없습니다: ${tunnel.sshHost}`);
  }

  const hookServerUrl = await getRemoteHookServerUrl(tunnel.sshHost);
  const remoteSocketPath = (await execGit(buildRemoteHookSocketPrepareCommand(hookServerUrl), tunnel.sshHost)).trim();
  if (tunnel.isClosed) {
    return;
  }

  const baseArgs = buildSSHArgs(hostConfig, {
    disableTty: true,
    connectionHealth: {
      connectTimeoutSeconds: TUNNEL_CONNECT_TIMEOUT_SECONDS,
      serverAliveIntervalSeconds: TUNNEL_SERVER_ALIVE_INTERVAL_SECONDS,
      serverAliveCountMax: TUNNEL_SERVER_ALIVE_COUNT_MAX,
    },
  });
  const sshProcess = spawn("ssh", buildHookTunnelSSHArgs(baseArgs, remoteSocketPath, tunnel.localSocketPath), {
    env: createLocalShellEnvironment(),
    stdio: ["pipe", "ignore", "pipe"],
  });
  tunnel.sshProcess = sshProcess;
  const startedAt = Date.now();
  let lastErrorOutput = "";
  sshProcess.stderr?.on("data", (chunk) => {
    lastErrorOutput = String(chunk).trim().slice(-300);
  });
  sshProcess.stdin?.on("error", () => undefined);
  sshProcess.once("error", (error) => {
    lastErrorOutput = error.message;
  });
  sshProcess.once("close", (exitCode) => {
    if (tunnel.sshProcess === sshProcess) {
      tunnel.sshProcess = null;
    }
    if (tunnel.isClosed) {
      return;
    }
    if (Date.now() - startedAt >= TUNNEL_STABLE_AFTER_MS) {
      tunnel.reconnectDelayMs = TUNNEL_RECONNECT_MIN_DELAY_MS;
    }
    console.warn(`[kanvibe] Hook tunnel to ${tunnel.sshHost} closed (exit ${exitCode}): ${lastErrorOutput}`);
    scheduleHookTunnelReconnect(tunnel);
  });
}

function scheduleHookTunnelReconnect(tunnel: HookTunnel): void {
  if (tunnel.isClosed || tunnel.reconnectTimer) {
    return;
  }
  const delayMs = tunnel.reconnectDelayMs;
  tunnel.reconnectDelayMs = Math.min(delayMs * 2, TUNNEL_RECONNECT_MAX_DELAY_MS);
  tunnel.reconnectTimer = setTimeout(() => {
    tunnel.reconnectTimer = null;
    startHookTunnelConnection(tunnel);
  }, delayMs);
  tunnel.reconnectTimer.unref();
}

function startHookTunnelConnection(tunnel: HookTunnel): void {
  connectHookTunnel(tunnel).catch((error) => {
    console.warn(`[kanvibe] Hook tunnel to ${tunnel.sshHost} failed:`, error instanceof Error ? error.message : error);
    scheduleHookTunnelReconnect(tunnel);
  });
}

function closeHookTunnel(tunnel: HookTunnel): void {
  tunnel.isClosed = true;
  if (tunnel.reconnectTimer) {
    clearTimeout(tunnel.reconnectTimer);
    tunnel.reconnectTimer = null;
  }
  tunnel.sshProcess?.kill();
  tunnel.localServer.close();
  tunnelsByHost.delete(tunnel.sshHost);
}

/** 호스트의 터널을 연다. 이미 있으면 그대로 두므로 hook을 설치할 때마다 불러도 된다 */
export function ensureHookTunnel(sshHost: string): void {
  if (!createHostRequestListener || !localSocketDirectory || tunnelsByHost.has(sshHost)) {
    return;
  }

  const localSocketPath = path.join(localSocketDirectory, `${tunnelsByHost.size}-${Date.now()}.sock`);
  const localServer = http.createServer(createHostRequestListener(sshHost));
  const tunnel: HookTunnel = {
    sshHost,
    localServer,
    localSocketPath,
    sshProcess: null,
    reconnectTimer: null,
    reconnectDelayMs: TUNNEL_RECONNECT_MIN_DELAY_MS,
    isClosed: false,
  };
  tunnelsByHost.set(sshHost, tunnel);
  localServer.once("error", (error) => {
    console.warn(`[kanvibe] Hook tunnel socket for ${sshHost} failed:`, error.message);
    closeHookTunnel(tunnel);
  });
  localServer.listen(localSocketPath, () => startHookTunnelConnection(tunnel));
}

/** 진행 중인 원격 task가 있는 호스트만 터널을 유지한다 */
async function reconcileHookTunnels(): Promise<void> {
  const taskRepository = await getTaskRepository();
  const activeTasks = await taskRepository.find({
    select: { sshHost: true },
    where: { sshHost: Not(IsNull()), status: Not(TaskStatus.DONE) },
  });
  const activeHosts = new Set(activeTasks.flatMap((task) => (task.sshHost ? [task.sshHost] : [])));

  for (const sshHost of activeHosts) {
    ensureHookTunnel(sshHost);
  }
  for (const tunnel of [...tunnelsByHost.values()]) {
    if (!activeHosts.has(tunnel.sshHost)) {
      closeHookTunnel(tunnel);
    }
  }
}

function runHookTunnelReconcile(): void {
  reconcileHookTunnels().catch((error) => {
    console.warn("[kanvibe] Hook tunnel reconcile failed:", error instanceof Error ? error.message : error);
  });
}

export function startHookTunnels(requestListenerForHost: (sshHost: string) => RequestListener): void {
  /** Windows의 OpenSSH는 Unix 소켓 전달을 지원하지 않아 HTTP 경로만 쓴다 */
  if (process.platform === "win32" || createHostRequestListener) {
    return;
  }
  createHostRequestListener = requestListenerForHost;
  /** mkdtemp는 추측할 수 없는 이름의 0700 디렉터리를 만들므로 같은 머신의 다른 사용자가 끼어들지 못한다 */
  localSocketDirectory = mkdtempSync(LOCAL_SOCKET_DIRECTORY_PREFIX);
  runHookTunnelReconcile();
  reconcileTimer = setInterval(runHookTunnelReconcile, TUNNEL_RECONCILE_INTERVAL_MS);
  reconcileTimer.unref();
}

/** 잠자기에서 깨면 끊긴 연결을 기다리지 않고 곧바로 다시 잇는다 */
export function reconnectHookTunnels(): void {
  for (const tunnel of tunnelsByHost.values()) {
    tunnel.reconnectDelayMs = TUNNEL_RECONNECT_MIN_DELAY_MS;
    if (tunnel.sshProcess) {
      tunnel.sshProcess.kill();
    } else if (tunnel.reconnectTimer) {
      clearTimeout(tunnel.reconnectTimer);
      tunnel.reconnectTimer = null;
      startHookTunnelConnection(tunnel);
    }
  }
}

export function stopHookTunnels(): void {
  if (reconcileTimer) {
    clearInterval(reconcileTimer);
    reconcileTimer = null;
  }
  for (const tunnel of [...tunnelsByHost.values()]) {
    closeHookTunnel(tunnel);
  }
  if (localSocketDirectory) {
    rmSync(localSocketDirectory, { recursive: true, force: true });
    localSocketDirectory = null;
  }
  createHostRequestListener = null;
}
