import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import http, { type RequestListener } from "node:http";
import path from "node:path";
import { IsNull, Not } from "typeorm";
import { getOrCreateHookClientId } from "@/desktop/main/services/appSettingsService";
import { execGit } from "@/lib/gitOperations";
import { buildHookSocketFileName, buildRemoteHookSocketPrepareCommand } from "@/lib/hookSocketPath";
import { getProjectRepository, getTaskRepository } from "@/lib/database";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import { buildSSHArgs, parseSSHConfig, type SSHHostConfig } from "@/lib/sshConfig";
import { configureRemoteHostTransport, getRemoteHostConnection, getRemoteHostEndpointKey, getRemoteHostNamespace, stopRemoteHostConnections, type HostConnection } from "@/lib/remoteHostConnection";
import { decodeHostBytes, HOST_RECORD_BYTES, type HostRecord } from "@/lib/remoteHostProtocol";

/**
 * 원격 호스트의 hook·CLI가 평문 포트 대신 SSH 안으로 데스크탑에 돌아오도록 호스트별 `ssh -R` 터널을 유지한다.
 *
 * 원격 계정마다 로컬 소켓을 따로 두어, 요청이 어느 호스트에서 왔는지를 소켓만으로 알 수 있게 한다.
 * 같은 원격 계정을 가리키는 SSH 별칭들은 원격 소켓 경로가 같아 터널 하나를 함께 쓴다.
 * 별칭마다 터널을 열면 나중에 연 터널이 앞 터널의 소켓을 지워, 앞 별칭의 요청이 다른 별칭의 요청으로 들어온다.
 * 원격 hook과 CLI는 이 터널이 연결된 동안만 요청을 보낸다.
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
  endpointKey: string;
  /** 연결에 쓰는 별칭. 같은 원격 계정이라 어느 별칭으로 붙어도 소켓 위치는 같다 */
  connectHost: string;
  /** 이 터널로 들어온 요청이 바꿀 수 있는 task의 호스트. 호스트 범위 검사가 살아 있는 값을 보도록 같은 Set을 넘긴다 */
  sshHosts: Set<string>;
  localServer: http.Server;
  localSocketPath: string;
  sshProcess: ChildProcess | null;
  hostConnection?: HostConnection;
  reconnectTimer: NodeJS.Timeout | null;
  reconnectDelayMs: number;
  isClosed: boolean;
}

const tunnelsByEndpoint = new Map<string, HookTunnel>();
let createHostRequestListener: ((sshHosts: ReadonlySet<string>) => RequestListener) | null = null;
/** 별칭을 터널에 묶는 판단이 겹치면 같은 원격 계정에 터널이 둘 생기므로 순서대로 처리한다 */
let pendingTunnelOperation: Promise<void> = Promise.resolve();
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
    "-o", "StrictHostKeyChecking=yes",
    "-R", `${remoteSocketPath}:${localSocketPath}`,
    destination,
    /** 앱이 비정상 종료되면 stdin이 닫혀 cat이 끝나고, 그러면 ssh도 함께 끝나 터널이 남지 않는다 */
    "cat >/dev/null",
  ];
}

/** 같은 계정·주소·포트로 붙는 별칭은 원격의 같은 /tmp/kv-<uid>를 쓴다 */
function buildEndpointKey(hostConfig: SSHHostConfig): string {
  return `${hostConfig.username}@${hostConfig.hostname.toLowerCase()}:${hostConfig.port}`;
}

async function findSSHHostConfig(sshHost: string): Promise<SSHHostConfig> {
  const configs = await parseSSHConfig();
  const hostConfig = configs.find((config) => config.host === sshHost);
  if (!hostConfig) {
    throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
  }
  return hostConfig;
}

async function connectHookTunnel(tunnel: HookTunnel): Promise<void> {
  const hostConfig = await findSSHHostConfig(tunnel.connectHost);
  const hostConnection = await getRemoteHostConnection(tunnel.connectHost);
  if (tunnel.isClosed) return;
  if (hostConnection) {
    tunnel.hostConnection = hostConnection;
    hostConnection.setHookHandler((record) => forwardHostHookRequest(tunnel, record));
    void import("@/desktop/main/services/kanvibeTaskStateService").then(({ refreshHostTaskStatuses }) => (
      refreshHostTaskStatuses(tunnel.sshHosts, () => !tunnel.isClosed && tunnel.hostConnection === hostConnection)
    )).catch((error) => console.warn("[kanvibe] Host status refresh failed:", error));
    hostConnection.once("close", () => {
      if (tunnel.hostConnection === hostConnection) {
        tunnel.hostConnection = undefined;
        if (!tunnel.isClosed) startHookTunnelConnection(tunnel);
      }
    });
    return;
  }
  const clientId = await getOrCreateHookClientId();
  const remoteSocketPath = (await execGit(
    buildRemoteHookSocketPrepareCommand(clientId),
    tunnel.connectHost,
    { strictHostKeyChecking: true },
  )).trim();
  const socketDirectory = remoteSocketPath.slice(0, remoteSocketPath.lastIndexOf("/"));
  if (!/^\/tmp\/kv-\d+$/.test(socketDirectory)
    || remoteSocketPath !== `${socketDirectory}/${buildHookSocketFileName(clientId)}`) {
    throw new Error("원격 hook 소켓 경로가 설치한 client의 경로와 일치하지 않습니다.");
  }
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
    console.warn(`[kanvibe] Hook tunnel to ${tunnel.connectHost} closed (exit ${exitCode}): ${lastErrorOutput}`);
    scheduleHookTunnelReconnect(tunnel);
  });
}

function forwardHostHookRequest(tunnel: HookTunnel, record: HostRecord): Promise<{ status: number; headers: Record<string, string>; body: string }> {
  return new Promise((resolve, reject) => {
    if (tunnel.isClosed || typeof record.path !== "string" || typeof record.method !== "string") { reject(new Error("Invalid hook request")); return; }
    const request = http.request({ socketPath: tunnel.localSocketPath, path: record.path, method: record.method, headers: record.headers as http.OutgoingHttpHeaders }, (response) => {
      const chunks: Buffer[] = []; let size = 0;
      response.on("data", (chunk: Buffer) => { size += chunk.length; if (size > HOST_RECORD_BYTES / 2) { response.destroy(new Error("Hook response too large")); return; } chunks.push(chunk); });
      response.once("error", reject);
      response.once("end", () => {
        const headers: Record<string, string> = {};
        for (const [key, value] of Object.entries(response.headers)) if (typeof value === "string") headers[key] = value;
        resolve({ status: response.statusCode ?? 502, headers, body: Buffer.concat(chunks).toString("base64") });
      });
    });
    request.setTimeout(12_000, () => request.destroy(new Error("Hook request timed out")));
    request.once("error", reject);
    try { request.end(decodeHostBytes(record.body, HOST_RECORD_BYTES / 2)); } catch (error) { request.destroy(); reject(error); }
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
    console.warn(`[kanvibe] Hook tunnel to ${tunnel.connectHost} failed:`, error instanceof Error ? error.message : error);
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
  tunnelsByEndpoint.delete(tunnel.endpointKey);
}

/**
 * 터널로 들어온 요청이 이 task를 다룰 수 있는지 판정한다. 평문 포트의 요청(sourceSshHosts 없음)은 이 검사를 받지 않는다.
 * sshHost가 비어 있는 옛 task는 프로젝트의 호스트로 판정한다. 비워 둔 채 거부하면 그 task의 상태 변경이 조용히 사라진다.
 */
export function isTaskReachableFromTunnel(
  task: { sshHost: string | null },
  project: { sshHost: string | null } | null | undefined,
  sourceSshHosts: ReadonlySet<string> | undefined,
): boolean {
  if (!sourceSshHosts) {
    return true;
  }
  const taskHost = task.sshHost ?? project?.sshHost ?? null;
  return taskHost !== null && sourceSshHosts.has(taskHost);
}

function findTunnelForHost(sshHost: string): HookTunnel | undefined {
  return [...tunnelsByEndpoint.values()].find((tunnel) => tunnel.sshHosts.has(sshHost));
}

/** 호스트의 터널을 연다. 이미 있으면 그대로 두므로 hook을 설치할 때마다 불러도 된다 */
export function ensureHookTunnel(sshHost: string): void {
  pendingTunnelOperation = pendingTunnelOperation
    .then(() => openHookTunnel(sshHost))
    .catch((error) => {
      console.warn(`[kanvibe] Hook tunnel to ${sshHost} failed:`, error instanceof Error ? error.message : error);
    });
}

async function openHookTunnel(sshHost: string): Promise<void> {
  if (!createHostRequestListener || !localSocketDirectory || findTunnelForHost(sshHost)) {
    return;
  }

  const hostConfig = await findSSHHostConfig(sshHost);
  const endpointKey = getRemoteHostNamespace() ? await getRemoteHostEndpointKey(hostConfig) : buildEndpointKey(hostConfig);
  const sameEndpointTunnel = tunnelsByEndpoint.get(endpointKey);
  if (sameEndpointTunnel) {
    sameEndpointTunnel.sshHosts.add(sshHost);
    return;
  }
  if (!createHostRequestListener || !localSocketDirectory) {
    return;
  }

  const sshHosts = new Set([sshHost]);
  const localSocketPath = path.join(localSocketDirectory, `${tunnelsByEndpoint.size}-${Date.now()}.sock`);
  const localServer = http.createServer(createHostRequestListener(sshHosts));
  const tunnel: HookTunnel = {
    endpointKey,
    connectHost: sshHost,
    sshHosts,
    localServer,
    localSocketPath,
    sshProcess: null,
    reconnectTimer: null,
    reconnectDelayMs: TUNNEL_RECONNECT_MIN_DELAY_MS,
    isClosed: false,
  };
  tunnelsByEndpoint.set(endpointKey, tunnel);
  localServer.once("error", (error) => {
    console.warn(`[kanvibe] Hook tunnel socket for ${sshHost} failed:`, error.message);
    closeHookTunnel(tunnel);
  });
  localServer.listen(localSocketPath, () => startHookTunnelConnection(tunnel));
}

/**
 * 원격 task나 원격 프로젝트가 남아 있는 호스트의 터널을 유지한다.
 * 완료한 task도 worktree가 남아 있으면 hook과 CLI가 돌기 때문에 상태로 거르지 않는다.
 */
async function reconcileHookTunnels(): Promise<void> {
  const [remoteTasks, remoteProjects] = await Promise.all([
    (await getTaskRepository()).find({ select: { sshHost: true }, where: { sshHost: Not(IsNull()) } }),
    (await getProjectRepository()).find({ select: { sshHost: true }, where: { sshHost: Not(IsNull()) } }),
  ]);
  const activeHosts = new Set(
    [...remoteTasks, ...remoteProjects].flatMap((entity) => (entity.sshHost ? [entity.sshHost] : [])),
  );

  for (const sshHost of activeHosts) {
    ensureHookTunnel(sshHost);
  }
  await pendingTunnelOperation;
  for (const tunnel of [...tunnelsByEndpoint.values()]) {
    for (const sshHost of tunnel.sshHosts) {
      if (!activeHosts.has(sshHost)) {
        tunnel.sshHosts.delete(sshHost);
      }
    }
    if (tunnel.sshHosts.size === 0) {
      closeHookTunnel(tunnel);
    }
  }
}

function runHookTunnelReconcile(): void {
  reconcileHookTunnels().catch((error) => {
    console.warn("[kanvibe] Hook tunnel reconcile failed:", error instanceof Error ? error.message : error);
  });
}

export function startHookTunnels(requestListenerForHosts: (sshHosts: ReadonlySet<string>) => RequestListener, namespace?: string): void {
  /** Windows의 OpenSSH는 Unix 소켓 전달을 지원하지 않아 HTTP 경로만 쓴다 */
  if (process.platform === "win32" || createHostRequestListener) {
    return;
  }
  createHostRequestListener = requestListenerForHosts;
  /** mkdtemp는 추측할 수 없는 이름의 0700 디렉터리를 만들므로 같은 머신의 다른 사용자가 끼어들지 못한다 */
  localSocketDirectory = mkdtempSync(LOCAL_SOCKET_DIRECTORY_PREFIX);
  if (namespace) {
    void getOrCreateHookClientId().then((clientId) => {
      if (createHostRequestListener !== requestListenerForHosts) return;
      configureRemoteHostTransport({ namespace, clientId });
      runHookTunnelReconcile();
    }).catch((error) => console.warn("[kanvibe] Host transport configuration failed:", error));
  } else runHookTunnelReconcile();
  reconcileTimer = setInterval(runHookTunnelReconcile, TUNNEL_RECONCILE_INTERVAL_MS);
  reconcileTimer.unref();
}

/** 잠자기에서 깨면 끊긴 연결을 기다리지 않고 곧바로 다시 잇는다 */
export function reconnectHookTunnels(): void {
  for (const tunnel of tunnelsByEndpoint.values()) {
    tunnel.reconnectDelayMs = TUNNEL_RECONNECT_MIN_DELAY_MS;
    if (tunnel.hostConnection) {
      tunnel.hostConnection.close();
    } else if (tunnel.sshProcess) {
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
  for (const tunnel of [...tunnelsByEndpoint.values()]) {
    closeHookTunnel(tunnel);
  }
  if (localSocketDirectory) {
    rmSync(localSocketDirectory, { recursive: true, force: true });
    localSocketDirectory = null;
  }
  createHostRequestListener = null;
  stopRemoteHostConnections();
}
