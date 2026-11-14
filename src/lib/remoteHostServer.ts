import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { chmod, lstat, mkdir, open, readFile, rm } from "node:fs/promises";
import http from "node:http";
import net, { type Socket } from "node:net";
import path from "node:path";
import type { Readable, Writable } from "node:stream";
import { createLocalShellEnvironment } from "./shellEnvironment";
import { RemoteHostPtyRegistry } from "./remoteHostPty";
import { openRemoteHostHookJournal, type HostHookJournal, type HostHookJournalRequest } from "./remoteHostHookJournal";
import {
  buildHostSocketPaths, createHostRecordDecoder, decodeHostBytes, encodeHostRecord,
  HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HOST_PROTOCOL_VERSION, HOST_RECORD_BYTES, isValidHostNamespace, type HostRecord,
} from "./remoteHostProtocol";

const MAX_ACTIVE_REQUESTS = 128;
const MAX_HOOK_BYTES = 512 * 1024;
const MAX_COMMAND_BYTES = 256 * 1024 * 1024;
const MAX_COMMAND_TEXT_BYTES = HOST_RECORD_BYTES / 2;
const MAX_COMMAND_TIMEOUT_MS = 24 * 60 * 60_000;
const HOOK_RESPONSE_TIMEOUT_MS = 120_000;
const HELLO_TIMEOUT_MS = 10_000;
const HOOK_PATHS = new Set(["/api/hooks/status", "/api/cli/tasks", "/api/cli/tasks/delete", "/api/cli/tasks/register"]);
const HOOK_READ_PATHS = new Set(["/api/hooks/health", "/api/install/hooks.sh"]);

export interface RemoteHostServerOptions { namespace: string; buildHash: string; rootDirectory?: string; ptyModulePath?: string; ptyArtifactHash?: string }

async function assertPrivateDirectory(directory: string): Promise<void> {
  const stat = await lstat(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink() || stat.uid !== process.getuid?.() || (stat.mode & 0o777) !== 0o700) throw new Error("Untrusted host private directory");
}

async function preparePaths(namespace: string, rootDirectory?: string) {
  const defaults = buildHostSocketPaths(namespace);
  const parent = rootDirectory ?? path.dirname(defaults.directory);
  await mkdir(parent, { mode: 0o700 }).catch((error: NodeJS.ErrnoException) => { if (error.code !== "EEXIST") throw error; });
  await assertPrivateDirectory(parent);
  const directory = path.join(parent, namespace);
  await mkdir(directory, { mode: 0o700 }).catch((error: NodeJS.ErrnoException) => { if (error.code !== "EEXIST") throw error; });
  await assertPrivateDirectory(directory);
  return { directory, rpcSocketPath: path.join(directory, "rpc-v1.sock"), hookSocketPath: path.join(directory, "hooks-v1.sock") };
}

async function acquireStartLock(directory: string): Promise<() => Promise<void>> {
  const lockPath = path.join(directory, "start.lock");
  try { await mkdir(lockPath, { mode: 0o700 }); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    await assertPrivateDirectory(lockPath);
    const before = await lstat(lockPath);
    const ownerPath = path.join(lockPath, "pid");
    const owner = await lstat(ownerPath);
    if (!owner.isFile() || owner.isSymbolicLink() || owner.uid !== process.getuid?.()) throw new Error("Untrusted host start lock");
    const pid = Number(await readFile(ownerPath, "utf8"));
    if (!Number.isSafeInteger(pid) || pid < 1) throw new Error("Invalid host start lock");
    try { process.kill(pid, 0); throw new Error("Host daemon is already starting or running"); }
    catch (signalError) { if ((signalError as NodeJS.ErrnoException).code !== "ESRCH") throw signalError; }
    if ((await lstat(lockPath)).ino !== before.ino) throw new Error("Host start lock changed");
    await rm(lockPath, { recursive: true });
    return acquireStartLock(directory);
  }
  const pidFile = await open(path.join(lockPath, "pid"), constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW, 0o600);
  await pidFile.writeFile(String(process.pid)); await pidFile.close();
  const owned = await lstat(lockPath);
  return async () => {
    try { if ((await lstat(lockPath)).ino === owned.ino) await rm(lockPath, { recursive: true }); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
  };
}

function connectSocket(socketPath: string): Promise<Socket> {
  return new Promise((resolve, reject) => {
    const socket = net.connect(socketPath);
    const timer = setTimeout(() => { socket.destroy(); reject(new Error("Host socket probe timed out")); }, 2_000);
    socket.once("connect", () => { clearTimeout(timer); socket.removeListener("error", fail); resolve(socket); });
    const fail = (error: Error) => { clearTimeout(timer); socket.destroy(); reject(error); };
    socket.once("error", fail);
  });
}

async function removeStaleSocket(socketPath: string): Promise<void> {
  let before;
  try { before = await lstat(socketPath); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return; throw error; }
  if (!before.isSocket() || before.isSymbolicLink() || before.uid !== process.getuid?.()) throw new Error("Untrusted host socket");
  try { const socket = await connectSocket(socketPath); socket.destroy(); throw new Error("Host daemon is already running"); }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== "ECONNREFUSED") throw error; }
  if ((await lstat(socketPath)).ino !== before.ino) throw new Error("Host socket changed during probe");
  await rm(socketPath);
}

function killCommand(child: ChildProcessWithoutNullStreams): void {
  if (!child.pid) return;
  try { process.kill(-child.pid, "SIGKILL"); }
  catch (error) { if ((error as NodeJS.ErrnoException).code !== "ESRCH") child.kill("SIGKILL"); }
}

function sendChunks(send: (record: HostRecord) => boolean, type: string, id: string, bytes: Buffer, extra: object = {}): boolean {
  let writable = true;
  for (let offset = 0; offset < bytes.length; offset += HOST_CHUNK_BYTES) {
    writable = send({ type, id, ...extra, data: bytes.subarray(offset, offset + HOST_CHUNK_BYTES).toString("base64") }) && writable;
  }
  return writable;
}

interface Peer {
  socket: Socket;
  clientId: string;
  send: (record: HostRecord) => boolean;
  hooks: Map<string, { response: http.ServerResponse; timer: NodeJS.Timeout }>;
}


interface JournalPeerState {
  subscribed: boolean;
  starting: boolean;
  acknowledging?: boolean;
  eventId?: string;
  timeout?: NodeJS.Timeout;
  retry?: NodeJS.Timeout;
}
function createHookJournalDelivery(journal: HostHookJournal, peers: Map<string, Peer>) {
  const states = new Map<Peer, JournalPeerState>();
  const admissions = new Map<http.ServerResponse, { persisting: boolean }>();
  const live = new Map<string, { clientId: string; response: http.ServerResponse; timer: NodeJS.Timeout; attempted: boolean }>();
  const finishLive = (id: string, status: number, headers: Record<string, string> = {}, bytes: Buffer = Buffer.alloc(0)) => {
    const pending = live.get(id);
    if (!pending) return;
    live.delete(id); clearTimeout(pending.timer);
    if (!pending.response.destroyed && !pending.response.writableEnded) pending.response.writeHead(status, headers).end(bytes);
  };
  const retry = (peer: Peer, state: JournalPeerState) => {
    if (states.get(peer) !== state || peer.socket.destroyed || state.retry) return;
    state.retry = setTimeout(() => { state.retry = undefined; pump(peer); }, 5_000);
    state.retry.unref();
  };
  const pump = (peer: Peer) => {
    const state = states.get(peer);
    if (!state?.subscribed || state.starting || state.acknowledging || state.eventId || state.retry || peer.socket.destroyed) return;
    state.starting = true;
    void journal.next(peer.clientId).then((event) => {
      if (!event || states.get(peer) !== state || peer.socket.destroyed) return;
      state.eventId = event.id;
      const pending = live.get(event.id);
      const replay = !pending || pending.attempted || pending.response.destroyed;
      if (pending) pending.attempted = true;
      state.timeout = setTimeout(() => {
        state.eventId = undefined; state.timeout = undefined;
        finishLive(event.id, 504);
        retry(peer, state);
      }, HOOK_RESPONSE_TIMEOUT_MS);
      state.timeout.unref();
      peer.send({ type: "hook.request", id: event.id, path: event.path, method: event.method, headers: event.headers, body: event.body, journal: { sequence: event.sequence, receivedAt: event.receivedAt, replay } });
    }).catch((error) => {
      console.warn("[kanvibe] Hook journal delivery failed:", error instanceof Error ? error.message : error);
      retry(peer, state);
    }).finally(() => { state.starting = false; });
  };
  return {
    admit(response: http.ServerResponse): boolean {
      if (admissions.size >= MAX_ACTIVE_REQUESTS) return false;
      const reservation = { persisting: false };
      admissions.set(response, reservation);
      const release = () => { if (!reservation.persisting) admissions.delete(response); };
      response.once("close", release); response.once("finish", release);
      return true;
    },
    subscribe(peer: Peer): void {
      let state = states.get(peer);
      if (!state) { state = { subscribed: true, starting: false }; states.set(peer, state); }
      state.subscribed = true;
      if (state.retry) { clearTimeout(state.retry); state.retry = undefined; }
      pump(peer);
    },
    async enqueue(clientId: string, request: HostHookJournalRequest, response: http.ServerResponse): Promise<void> {
      let event;
      const reservation = admissions.get(response);
      if (reservation) reservation.persisting = true;
      try { event = await journal.append(clientId, request); }
      catch (error) {
        const message = error instanceof Error ? error.message : "Hook journal unavailable";
        const status = /Invalid|mismatch|Unexpected|JSON/i.test(message) ? 400 : /bytes|size/i.test(message) && !/capacity/i.test(message) ? 413 : 503;
        if (!response.destroyed && !response.writableEnded) response.writeHead(status).end(message);
        return;
      } finally {
        if (reservation) reservation.persisting = false;
        if (response.destroyed || response.writableEnded) admissions.delete(response);
      }
      if (response.destroyed || response.writableEnded) { const peer = peers.get(clientId); if (peer) pump(peer); return; }
      const peer = peers.get(clientId);
      if (!peer || !states.get(peer)?.subscribed || peer.socket.destroyed) {
        response.writeHead(202, { "content-type": "application/json" }).end(JSON.stringify({ success: true, queued: true, eventId: event.id }));
      } else {
        const timer = setTimeout(() => finishLive(event.id, 504), HOOK_RESPONSE_TIMEOUT_MS); timer.unref();
        live.set(event.id, { clientId, response, timer, attempted: false });
        response.once("close", () => { const pending = live.get(event.id); if (pending) clearTimeout(pending.timer); live.delete(event.id); });
        pump(peer);
      }
    },
    receives(peer: Peer, id: string): boolean { const state = states.get(peer); return state?.eventId === id && !state.acknowledging; },
    async respond(peer: Peer, record: HostRecord): Promise<void> {
      const state = states.get(peer), id = String(record.id);
      if (!state || state.eventId !== id || state.acknowledging) return;
      state.acknowledging = true;
      if (state.timeout) clearTimeout(state.timeout);
      const status = Number(record.status);
      if (!Number.isInteger(status) || status < 100 || status > 599) { peer.socket.destroy(new Error("Invalid journal hook status")); return; }
      let bytes: Buffer, headers: Record<string, string>;
      try { bytes = decodeHostBytes(record.body, MAX_HOOK_BYTES); headers = filteredHeaders(record.headers); }
      catch { peer.socket.destroy(new Error("Invalid journal hook response")); return; }
      let application: { success?: unknown; delivery?: unknown } | undefined;
      try { application = JSON.parse(bytes.toString("utf8")); } catch { /* 기존 text 응답도 HTTP 성공 계약을 유지한다. */ }
      const success = status >= 200 && status < 300 && application?.success !== false;
      const rejected = status >= 400 && status < 500;
      try {
        if (success || rejected) await journal.acknowledge(peer.clientId, id, rejected ? "rejected" : application?.delivery === "ignored" ? "ignored" : "accepted");
        finishLive(id, status, headers, bytes);
      } catch (error) {
        finishLive(id, 503);
        console.warn("[kanvibe] Hook journal acknowledgement failed:", error instanceof Error ? error.message : error);
        state.eventId = undefined; state.acknowledging = false; retry(peer, state); return;
      }
      state.eventId = undefined; state.timeout = undefined; state.acknowledging = false;
      if (success || rejected) pump(peer); else retry(peer, state);
    },
    detach(peer: Peer): void {
      const state = states.get(peer);
      if (state?.timeout) clearTimeout(state.timeout);
      if (state?.retry) clearTimeout(state.retry);
      states.delete(peer);
      for (const [id, pending] of live) if (pending.clientId === peer.clientId) finishLive(id, 503);
    },
    status: async () => ({ ...await journal.status(), activeRequests: admissions.size }),
  };
}
type JournalDelivery = ReturnType<typeof createHookJournalDelivery>;

function attachPeer(socket: Socket, options: RemoteHostServerOptions, peers: Map<string, Peer>, identity: HostRecord, journalDelivery: JournalDelivery, ptys: RemoteHostPtyRegistry, stop: () => void): void {
  const commands = new Map<string, ChildProcessWithoutNullStreams>();
  const forwards = new Map<string, Socket>();
  const timers = new Set<NodeJS.Timeout>();
  const streamResumers = new Set<() => void>();
  const resumeStreams = () => streamResumers.forEach((resume) => resume());
  let peer: Peer | undefined;
  const send = (record: HostRecord): boolean => {
    if (socket.destroyed) return false;
    const bytes = encodeHostRecord(record);
    if (socket.writableLength + bytes.length > HOST_BUFFER_BYTES) { socket.destroy(new Error("Host output backpressure limit")); return false; }
    return socket.write(bytes);
  };
  const helloTimer = setTimeout(() => socket.destroy(), HELLO_TIMEOUT_MS);
  socket.on("drain", resumeStreams);
  socket.on("error", () => undefined);
  socket.on("close", () => {
    clearTimeout(helloTimer); timers.forEach(clearTimeout);
    commands.forEach(killCommand); forwards.forEach((forward) => forward.destroy());
    socket.off("drain", resumeStreams); streamResumers.clear();
    if (peer && peers.get(peer.clientId) === peer) peers.delete(peer.clientId);
    if (peer) journalDelivery.detach(peer);
    if (peer) ptys.detach(peer);
    peer?.hooks.forEach(({ response, timer }) => { clearTimeout(timer); response.writeHead(503).end("Desktop disconnected; request not replayed"); });
  });
  const fail = (error: unknown) => {
    send({ type: "error", error: error instanceof Error ? error.message : "Invalid host request" }); socket.end();
  };
  const openCommand = (record: HostRecord, id: string) => {
    if (commands.has(id) || forwards.has(id) || commands.size + forwards.size >= MAX_ACTIVE_REQUESTS) throw new Error("Host request capacity exceeded or duplicate id");
    if (typeof record.command !== "string" || !record.command.length || Buffer.byteLength(record.command) > MAX_COMMAND_TEXT_BYTES) throw new Error("Invalid host command");
    const timeout = Number(record.timeoutMs), limit = Number(record.maxBufferBytes);
    if (!Number.isSafeInteger(timeout) || timeout < 1 || timeout > MAX_COMMAND_TIMEOUT_MS || !Number.isSafeInteger(limit) || limit < 0 || limit > MAX_COMMAND_BYTES) throw new Error("Invalid host command bounds");
    if (record.interactive !== undefined && typeof record.interactive !== "boolean") throw new Error("Invalid host interactive mode");
    let child: ChildProcessWithoutNullStreams;
    try { child = spawn("/bin/sh", ["-lc", record.command], { env: createLocalShellEnvironment(), stdio: ["pipe", "pipe", "pipe"], detached: true }); }
    catch (error) { send({ type: "exec.exit", id, code: null, signal: null, error: error instanceof Error ? error.message : "Remote command could not start" }); return; }
    commands.set(id, child);
    let total = 0, commandError: string | undefined;
    const timer = setTimeout(() => { commandError = "Remote command timed out"; killCommand(child); }, timeout); timers.add(timer);
    const output = (stream: "stdout" | "stderr", bytes: Buffer) => {
      total += bytes.length;
      if (limit > 0 && total > limit) { commandError = "Remote command maxBuffer exceeded"; killCommand(child); return; }
      if (!sendChunks(send, "exec.data", id, bytes, { stream })) { child.stdout.pause(); child.stderr.pause(); }
    };
    child.stdout.on("data", (bytes: Buffer) => output("stdout", bytes)); child.stderr.on("data", (bytes: Buffer) => output("stderr", bytes));
    const resume = () => { child.stdout.resume(); child.stderr.resume(); }; streamResumers.add(resume);
    child.stdin.on("error", () => undefined);
    child.on("error", (error) => { commandError = error.message; });
    child.on("close", (code, signal) => {
      clearTimeout(timer); timers.delete(timer); commands.delete(id); streamResumers.delete(resume);
      send({ type: "exec.exit", id, code, signal, ...(commandError ? { error: commandError } : {}) });
    });
    if (record.stdin !== undefined) child.stdin.write(decodeHostBytes(record.stdin));
    if (record.interactive !== true) child.stdin.end();
  };
  const openForward = async (record: HostRecord, id: string) => {
    if (commands.has(id) || forwards.has(id) || commands.size + forwards.size >= MAX_ACTIVE_REQUESTS || typeof record.path !== "string" || !path.isAbsolute(record.path)) throw new Error("Invalid host socket request");
    const forward = new net.Socket(); forwards.set(id, forward);
    try {
      const stat = await lstat(record.path);
      if (!stat.isSocket() || stat.isSymbolicLink() || stat.uid !== process.getuid?.()) throw new Error("Untrusted upstream socket");
    } catch (error) { forwards.delete(id); forward.destroy(); throw error; }
    forward.on("connect", () => send({ type: "socket.ready", id }));
    forward.on("data", (bytes: Buffer) => { if (!sendChunks(send, "socket.data", id, bytes)) forward.pause(); });
    const resume = () => forward.resume(); streamResumers.add(resume);
    forward.on("drain", () => send({ type: "socket.drain", id }));
    forward.on("end", () => send({ type: "socket.end", id }));
    forward.on("error", (error) => send({ type: "socket.error", id, error: error.message }));
    forward.on("close", () => { streamResumers.delete(resume); forwards.delete(id); send({ type: "socket.close", id }); });
    if (socket.destroyed || forward.destroyed) forward.destroy(); else forward.connect(record.path);
  };
  const receive = (record: HostRecord) => {
    if (!peer) {
      if (record.type !== "hello" || record.version !== HOST_PROTOCOL_VERSION || record.namespace !== options.namespace || record.role !== "desktop" || record.buildHash !== options.buildHash || typeof record.clientId !== "string" || !/^[A-Za-z0-9-]{1,128}$/.test(record.clientId) || peers.has(record.clientId)) throw new Error("Host handshake namespace, role, version, build or client mismatch");
      peer = { socket, clientId: record.clientId, send, hooks: new Map() }; peers.set(peer.clientId, peer); clearTimeout(helloTimer);
      send(identity); return;
    }
    const id = record.id;
    if (typeof id !== "string" || !/^[A-Za-z0-9-]{1,128}$/.test(id)) throw new Error("Invalid host request id");
    switch (record.type) {
      case "pty.open": case "pty.list": case "pty.input": case "pty.resize": case "pty.ack": case "pty.detach": case "pty.close":
        try { ptys.receive(peer, record); }
        catch (error) { ptys.receive(peer, { type: "pty.detach", id }); send({ type: "pty.error", id, error: error instanceof Error ? error.message : "Invalid PTY request" }); }
        break;
      case "exec.open": openCommand(record, id); break;
      case "exec.stdin": {
        const child = commands.get(id); if (!child) break;
        const bytes = decodeHostBytes(record.data);
        if (child.stdin.writableLength + bytes.length > HOST_BUFFER_BYTES) { killCommand(child); break; }
        child.stdin.write(bytes, (error) => {
          if (error) killCommand(child);
          else if (commands.has(id)) send({ type: "exec.stdin-ready", id });
        });
        if (record.end === true) child.stdin.end(); break;
      }
      case "exec.cancel": { const child = commands.get(id); if (child) killCommand(child); break; }
      case "socket.open": void openForward(record, id).catch((error) => send({ type: "socket.error", id, error: error.message })); break;
      case "socket.data": {
        const forward = forwards.get(id); if (!forward) break;
        const bytes = decodeHostBytes(record.data);
        if (forward.writableLength + bytes.length > HOST_BUFFER_BYTES) forward.destroy();
        else forward.write(bytes, (error) => {
          if (error) forward.destroy();
          else if (forwards.has(id)) send({ type: "socket.data-ready", id });
        });
        break;
      }
      case "socket.end": forwards.get(id)?.end(); break;
      case "socket.close": forwards.get(id)?.destroy(); break;
      case "hook.subscribe": journalDelivery.subscribe(peer); break;
      case "hook.response": {
        if (journalDelivery.receives(peer, id)) { void journalDelivery.respond(peer, record); break; }
        const hook = peer.hooks.get(id); if (!hook) break;
        const status = Number(record.status);
        if (!Number.isInteger(status) || status < 100 || status > 599) throw new Error("Invalid hook status");
        const bytes = decodeHostBytes(record.body, MAX_HOOK_BYTES);
        const headers = filteredHeaders(record.headers);
        peer.hooks.delete(id); clearTimeout(hook.timer); hook.response.writeHead(status, headers).end(bytes); break;
      }
      case "status": void journalDelivery.status().then((journal) => send({ ...identity, type: "status", id, journal })).catch((error) => send({ type: "error", id, error: error.message })); break;
      case "stop": send({ type: "stopped", id }); setImmediate(stop); break;
      default: throw new Error("Unknown host operation");
    }
  };
  const decode = createHostRecordDecoder(receive);
  socket.on("data", (bytes: Buffer) => { try { decode(bytes); } catch (error) { fail(error); } });
}

function filteredHeaders(value: unknown): Record<string, string> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid hook headers");
  const result: Record<string, string> = {};
  for (const [key, entry] of Object.entries(value)) {
    const name = key.toLowerCase();
    if (["content-type", "authorization", "x-kanvibe-client-id", "x-kanvibe-install-token"].includes(name) && typeof entry === "string" && entry.length <= 8192 && !/[\r\n]/.test(entry)) result[name] = entry;
  }
  return result;
}

function serveHook(request: http.IncomingMessage, response: http.ServerResponse, peers: Map<string, Peer>, journalDelivery: JournalDelivery): void {
  if (request.headers.origin !== undefined) {
    response.writeHead(403, { "content-type": "application/json" }).end(JSON.stringify({ success: false, error: "브라우저 요청은 허용하지 않습니다" }));
    request.resume(); return;
  }
  const allowed = request.method === "POST" && HOOK_PATHS.has(request.url ?? "")
    || request.method === "GET" && HOOK_READ_PATHS.has(request.url ?? "");
  if (!allowed) { response.writeHead(403).end("Hook role cannot perform this operation"); request.resume(); return; }
  const clientId = request.headers["x-kanvibe-client-id"];
  const peer = typeof clientId === "string" ? peers.get(clientId) : undefined;
  const durable = request.method === "POST" && request.url === "/api/hooks/status";
  if (typeof clientId !== "string" || !/^[A-Za-z0-9-]{1,128}$/.test(clientId)) { response.writeHead(400).end("Invalid hook client"); request.resume(); return; }
  if (durable && !journalDelivery.admit(response)) { response.writeHead(503).end("Hook capacity exceeded"); request.resume(); return; }
  if (!durable && (!peer || peer.socket.destroyed)) { response.writeHead(503).end("Desktop offline; request not replayed"); request.resume(); return; }
  if (!durable && peer!.hooks.size >= MAX_ACTIVE_REQUESTS) { response.writeHead(503).end("Hook capacity exceeded"); request.resume(); return; }
  const chunks: Buffer[] = []; let length = 0;
  request.on("data", (chunk: Buffer) => {
    length += chunk.length;
    if (length > MAX_HOOK_BYTES) { chunks.length = 0; if (!response.writableEnded) response.writeHead(413).end("Hook body size exceeded"); } else chunks.push(chunk);
  });
  request.on("error", () => response.destroy());
  request.on("end", () => {
    if (response.destroyed || response.writableEnded) return;
    if (durable) {
      void journalDelivery.enqueue(clientId, { method: request.method!, path: request.url!, headers: filteredHeaders(request.headers), body: Buffer.concat(chunks).toString("base64") }, response);
      return;
    }
    if (!peer || peer.socket.destroyed) { response.writeHead(503).end("Desktop disconnected; request not replayed"); return; }
    const id = randomUUID();
    const timer = setTimeout(() => { peer.hooks.delete(id); response.writeHead(504).end("Desktop response timed out; request not replayed"); }, HOOK_RESPONSE_TIMEOUT_MS);
    peer.hooks.set(id, { response, timer });
    response.on("close", () => { clearTimeout(timer); peer.hooks.delete(id); });
    peer.send({ type: "hook.request", id, method: request.method, path: request.url, headers: filteredHeaders(request.headers), body: Buffer.concat(chunks).toString("base64") });
  });
}

export async function startRemoteHostServer(options: RemoteHostServerOptions) {
  if (process.platform === "win32" || !isValidHostNamespace(options.namespace) || !/^[a-f0-9]{64}$/.test(options.buildHash)) throw new Error("Unsupported host runtime or configuration");
  const paths = await preparePaths(options.namespace, options.rootDirectory);
  const releaseLock = await acquireStartLock(paths.directory);
  let journal: HostHookJournal;
  try { journal = await openRemoteHostHookJournal({ namespace: options.namespace, rootDirectory: options.rootDirectory ? path.join(options.rootDirectory, "state") : undefined }); }
  catch (error) { await releaseLock(); throw error; }
  const peers = new Map<string, Peer>();
  const journalDelivery = createHookJournalDelivery(journal, peers);
  const ptys = new RemoteHostPtyRegistry(options.ptyModulePath);
  const sockets = new Set<Socket>();
  const instanceId = randomUUID();
  const identity: HostRecord = { type: "ready", version: HOST_PROTOCOL_VERSION, namespace: options.namespace, instanceId, pid: process.pid, uid: process.getuid?.(), rpcSocketPath: paths.rpcSocketPath, hookSocketPath: paths.hookSocketPath, buildHash: options.buildHash, ptyArtifactHash: options.ptyArtifactHash, runtime: { platform: process.platform, arch: process.arch, node: process.versions.node, napi: process.versions.napi }, capabilities: ["exec", "socket", "hooks", "hook-journal", ...(options.ptyModulePath ? ["pty"] : [])] };
  let closed = false;
  const rpc = net.createServer((socket) => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); attachPeer(socket, options, peers, identity, journalDelivery, ptys, () => { void close(); }); });
  const hooks = http.createServer((request, response) => serveHook(request, response, peers, journalDelivery));
  hooks.maxHeadersCount = 32; hooks.requestTimeout = 15_000; hooks.headersTimeout = 10_000;
  hooks.on("connection", (socket) => { sockets.add(socket); socket.on("close", () => sockets.delete(socket)); });
  const close = async () => {
    if (closed) return; closed = true;
    ptys.close();
    sockets.forEach((socket) => socket.destroy());
    await Promise.all([rpc, hooks].map((server) => new Promise<void>((resolve) => server.close(() => resolve()))));
    await journal.flush();
    await releaseLock();
  };
  try {
    await removeStaleSocket(paths.rpcSocketPath); await removeStaleSocket(paths.hookSocketPath);
    await new Promise<void>((resolve, reject) => { rpc.once("error", reject); rpc.listen(paths.rpcSocketPath, resolve); });
    await chmod(paths.rpcSocketPath, 0o600);
    await new Promise<void>((resolve, reject) => { hooks.once("error", reject); hooks.listen(paths.hookSocketPath, resolve); });
    await chmod(paths.hookSocketPath, 0o600);
  } catch (error) { await close(); throw error; }
  return { paths, instanceId, pid: process.pid, close };
}

export async function startRemoteHostBridge(options: { namespace: string; sourcePath: string; buildHash: string; ptyModulePath?: string; ptyArtifactHash?: string }, input: Readable, output: Writable): Promise<void> {
  const paths = await preparePaths(options.namespace);
  const stat = await lstat(options.sourcePath);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.uid !== process.getuid?.() || path.dirname(options.sourcePath) !== paths.directory) throw new Error("Untrusted host bundle");
  let socket: Socket;
  try { socket = await connectSocket(paths.rpcSocketPath); }
  catch (error) {
    if (!["ENOENT", "ECONNREFUSED"].includes((error as NodeJS.ErrnoException).code ?? "")) throw error;
    const daemon = spawn(process.execPath, [options.sourcePath, "--serve", options.namespace, options.buildHash, options.ptyModulePath ?? "", options.ptyArtifactHash ?? ""], { detached: true, stdio: "ignore", env: createLocalShellEnvironment() });
    daemon.unref();
    const deadline = Date.now() + 8_000;
    while (true) {
      try { socket = await connectSocket(paths.rpcSocketPath); break; }
      catch (connectError) {
        if (Date.now() >= deadline || !["ENOENT", "ECONNREFUSED"].includes((connectError as NodeJS.ErrnoException).code ?? "")) throw connectError;
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
    }
  }
  output.write('{"type":"bridge-ready"}\n');
  socket.on("error", () => { input.unpipe(socket); output.end(); });
  input.on("error", () => socket.destroy()); output.on("error", () => socket.destroy());
  input.on("end", () => socket.end());
  socket.on("close", () => { input.unpipe(socket); input.destroy(); });
  input.pipe(socket); socket.pipe(output);
}

if (process.argv[2] === "--serve" && process.argv[3] && process.argv[4]) {
  void startRemoteHostServer({ namespace: process.argv[3], buildHash: process.argv[4], ptyModulePath: process.argv[5] || undefined, ptyArtifactHash: process.argv[6] || undefined }).then((server) => {
    const stop = () => { void server.close().then(() => process.exit(0)); };
    process.once("SIGTERM", stop); process.once("SIGINT", stop);
  }).catch(() => { process.exitCode = 1; });
}
