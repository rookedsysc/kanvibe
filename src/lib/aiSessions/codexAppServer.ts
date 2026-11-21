import WebSocket from "ws";
import { spawn } from "node:child_process";
import { mkdtemp, realpath, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { getHomeDirectory } from "@/lib/hostFileAccess";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig } from "@/lib/sshConfig";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import type { AiSessionReaderContext } from "./types";

const CONNECTION_TIMEOUT_MS = 4_000;
const REQUEST_TIMEOUT_MS = 10_000;

export interface CodexAppServerConnection {
  codexHome: string;
  request<T>(method: string, params: unknown, onWrite?: () => void): Promise<T>;
}

async function withSocket<T>(socketPath: string, codexHome: string, operation: (connection: CodexAppServerConnection) => Promise<T>): Promise<T> {
  const socket = new WebSocket(`ws+unix://${socketPath}:/`, { handshakeTimeout: CONNECTION_TIMEOUT_MS, maxPayload: 1024 * 1024 });
  let sequence = 0;
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  const fail = (error: Error) => {
    for (const entry of pending.values()) { clearTimeout(entry.timer); entry.reject(error); }
    pending.clear();
  };
  socket.on("error", fail);
  socket.on("close", () => fail(new Error("Codex 연결이 종료되었습니다.")));
  socket.on("message", (data) => {
    try {
      const message = JSON.parse(data.toString());
      if (message.method || !pending.has(message.id)) return;
      const entry = pending.get(message.id)!;
      pending.delete(message.id);
      clearTimeout(entry.timer);
      if (message.error) entry.reject(new Error(`Codex 요청이 거절되었습니다 (${message.error.code}).`));
      else entry.resolve(message.result);
    } catch { fail(new Error("Codex 응답을 읽지 못했습니다.")); }
  });
  const request = <R>(method: string, params: unknown, onWrite?: () => void): Promise<R> => new Promise((resolve, reject) => {
    if (socket.readyState !== WebSocket.OPEN) { reject(new Error("Codex에 연결되지 않았습니다.")); return; }
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error("Codex 응답을 확인하지 못했습니다. 요청을 다시 보내지 않습니다.")); }, REQUEST_TIMEOUT_MS);
    pending.set(id, { resolve: (value) => resolve(value as R), reject, timer });
    try { onWrite?.(); socket.send(JSON.stringify({ id, method, params })); }
    catch (error) { clearTimeout(timer); pending.delete(id); reject(error); }
  });
  try {
    await new Promise<void>((resolve, reject) => { socket.once("open", resolve); socket.once("error", reject); });
    const initialized = await request<{ codexHome?: string }>("initialize", { clientInfo: { name: "kanvibe_pane_chat", version: "1" }, capabilities: { experimentalApi: true } });
    if (typeof initialized.codexHome !== "string" || path.posix.normalize(initialized.codexHome) !== codexHome) throw new Error("현재 CLI의 Codex 계정을 확인하지 못했습니다.");
    socket.send(JSON.stringify({ method: "initialized" }));
    return await operation({ codexHome, request });
  } finally {
    // 이 연결은 subscribe하지 않는다. 기존 TUI가 응답과 승인을 받으며 close는 이미 수락된 turn을 취소하지 않는다.
    socket.terminate();
    fail(new Error("Codex 연결이 종료되었습니다."));
  }
}

async function withHostSocket<T>(socketPath: string, codexHome: string, sshHost: string | null | undefined, operation: (connection: CodexAppServerConnection) => Promise<T>): Promise<T> {
  if (!sshHost) return withSocket(await realpath(socketPath), codexHome, operation);
  const host = (await parseSSHConfig()).find((entry) => entry.host === sshHost);
  if (!host) throw new Error("CLI의 SSH 호스트를 찾지 못했습니다.");
  const directory = await mkdtemp(path.join(tmpdir(), "kvc-"));
  const localSocket = path.join(directory, "api");
  const args = buildSSHArgs(host, { disableTty: true, connectionHealth: getKanvibeSSHConnectionHealthOptions() });
  args.splice(args.length - 1, 0, "-N", "-o", "ExitOnForwardFailure=yes", "-o", "ControlMaster=no", "-o", "ControlPath=none", "-L", `${localSocket}:${socketPath}`);
  const tunnel = spawn("ssh", args, { env: createLocalShellEnvironment(), stdio: ["ignore", "ignore", "pipe"] });
  let ended = false;
  const closed = new Promise<void>((resolve) => { tunnel.once("close", () => { ended = true; resolve(); }); tunnel.once("error", () => { ended = true; resolve(); }); });
  tunnel.stderr.resume();
  try {
    const deadline = Date.now() + CONNECTION_TIMEOUT_MS;
    while (!(await stat(localSocket).catch(() => null))?.isSocket()) {
      if (ended || Date.now() >= deadline) throw new Error("CLI의 Codex 소켓에 연결하지 못했습니다.");
      await delay(25);
    }
    return await withSocket(localSocket, codexHome, operation);
  } finally {
    if (!ended) tunnel.kill();
    const forceKill = setTimeout(() => tunnel.kill("SIGKILL"), 1_000);
    await closed.finally(() => clearTimeout(forceKill));
    await rm(directory, { recursive: true, force: true });
  }
}

/** 기존 계정의 서버에 연결해 전달된 작업을 수행한다. 연결을 위해 서버나 대화를 새로 만들지 않는다. */
export async function withCodexAppServer<T>(context: AiSessionReaderContext, explicitSocket: string | null | undefined, operation: (connection: CodexAppServerConnection) => Promise<T>): Promise<T> {
  if (explicitSocket === null) throw new Error("이 Codex 연결 방식에서는 대화 지정 입력을 지원하지 않습니다.");
  const codexHome = path.posix.normalize(path.posix.join(await getHomeDirectory(context.sshHost), ".codex"));
  const socketPath = explicitSocket ?? path.posix.join(codexHome, "app-server-control", "app-server-control.sock");
  if (!socketPath.startsWith("/") || socketPath.includes(":")) throw new Error("Codex 소켓 경로가 올바르지 않습니다.");
  return withHostSocket(socketPath, codexHome, context.sshHost, operation);
}
