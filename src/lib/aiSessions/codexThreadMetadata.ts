import WebSocket from "ws";
import { spawn } from "node:child_process";
import { mkdtemp, realpath, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { getHomeDirectory, pathExists } from "@/lib/hostFileAccess";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig } from "@/lib/sshConfig";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import { determineMatchScope } from "./shared";
import type { AiSessionReaderContext } from "./types";

const METADATA_TIMEOUT_MS = 4_000;
const MAIN_SOURCES = new Set(["cli", "vscode", "exec", "appServer"]);

/** 새 대화는 첫 입력 전까지 JSONL이 없다. 이미 화면에서 확인한 ID의 메타데이터만 읽으며 대화에 구독하거나 설정을 바꾸지 않는다 */
function queryThread(socketPath: string, threadId: string): Promise<{ codexHome: string; thread: Record<string, unknown> } | null> {
  return new Promise((resolve) => {
    const socket = new WebSocket(`ws+unix://${socketPath}:/`, { maxPayload: 1024 * 1024 });
    let codexHome: string | null = null;
    let settled = false;
    const finish = (result: { codexHome: string; thread: Record<string, unknown> } | null) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.terminate();
      resolve(result);
    };
    const timer = setTimeout(() => finish(null), METADATA_TIMEOUT_MS);
    socket.once("open", () => socket.send(JSON.stringify({ id: 1, method: "initialize", params: { clientInfo: { name: "kanvibe_pane_metadata", version: "1" }, capabilities: { experimentalApi: true } } })));
    socket.on("message", (data) => {
      try {
        const message = JSON.parse(data.toString());
        if (message.error) { finish(null); return; }
        if (message.id === 1) {
          if (typeof message.result?.codexHome !== "string") { finish(null); return; }
          codexHome = message.result.codexHome;
          socket.send(JSON.stringify({ method: "initialized" }));
          socket.send(JSON.stringify({ id: 2, method: "thread/read", params: { threadId, includeTurns: false } }));
        } else if (message.id === 2) {
          const thread = message.result?.thread;
          finish(codexHome && thread && typeof thread === "object" ? { codexHome, thread } : null);
        }
      } catch { finish(null); }
    });
    socket.once("error", () => finish(null));
    socket.once("close", () => finish(null));
  });
}

async function queryHostSocket(socketPath: string, threadId: string, sshHost?: string | null) {
  if (!sshHost) return queryThread(await realpath(socketPath), threadId);
  const host = (await parseSSHConfig()).find((entry) => entry.host === sshHost);
  if (!host) return null;
  const directory = await mkdtemp(path.join(tmpdir(), "kvc-"));
  const localSocket = path.join(directory, "api");
  const args = buildSSHArgs(host, { disableTty: true, connectionHealth: getKanvibeSSHConnectionHealthOptions() });
  args.splice(args.length - 1, 0, "-N", "-o", "ExitOnForwardFailure=yes", "-o", "ControlMaster=no", "-o", "ControlPath=none", "-L", `${localSocket}:${socketPath}`);
  const tunnel = spawn("ssh", args, { env: createLocalShellEnvironment(), stdio: ["ignore", "ignore", "pipe"] });
  let ended = false;
  const closed = new Promise<void>((resolve) => { tunnel.once("close", () => { ended = true; resolve(); }); tunnel.once("error", () => { ended = true; resolve(); }); });
  tunnel.stderr.resume();
  try {
    const deadline = Date.now() + METADATA_TIMEOUT_MS;
    while (!(await stat(localSocket).catch(() => null))?.isSocket()) {
      if (ended || Date.now() >= deadline) return null;
      await delay(25);
    }
    return await queryThread(localSocket, threadId);
  } finally {
    if (!ended) tunnel.kill();
    const forceKill = setTimeout(() => tunnel.kill("SIGKILL"), 1_000);
    await closed.finally(() => clearTimeout(forceKill));
    await rm(directory, { recursive: true, force: true });
  }
}

export async function readCodexEmptyThread(context: AiSessionReaderContext, threadId: string, explicitSocket?: string | null): Promise<string | null> {
  if (explicitSocket === null || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(threadId)) return null;
  try {
    const home = path.posix.join(await getHomeDirectory(context.sshHost), ".codex");
    const socketPath = explicitSocket ?? path.posix.join(home, "app-server-control", "app-server-control.sock");
    if (!socketPath.startsWith("/") || socketPath.includes(":")) return null;
    const result = await queryHostSocket(socketPath, threadId, context.sshHost);
    if (!result || path.posix.normalize(result.codexHome) !== path.posix.normalize(home)) return null;
    const thread = result.thread;
    if (thread.id !== threadId || typeof thread.cwd !== "string" || !determineMatchScope(thread.cwd, context)
      || thread.parentThreadId !== null || !MAIN_SOURCES.has(String(thread.source)) || thread.ephemeral !== false
      || thread.canAcceptDirectInput !== true || thread.preview !== "" || typeof thread.path !== "string") return null;
    const source = path.posix.normalize(thread.path);
    if (!source.startsWith(`${home}/sessions/`) || !path.posix.basename(source).endsWith(`-${threadId}.jsonl`)
      || await pathExists(source, context.sshHost)) return null;
    return source;
  } catch { return null; }
}
