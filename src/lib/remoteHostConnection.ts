import { execFile, spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import { createHash, randomUUID } from "node:crypto";
import { readFile, mkdtemp, rm } from "node:fs/promises";
import { createServer, type Socket } from "node:net";
import { PassThrough, Writable, type Readable } from "node:stream";
import { StringDecoder } from "node:string_decoder";
import path from "node:path";
import { tmpdir } from "node:os";
import { promisify } from "node:util";
import { buildSSHArgs, getKanvibeSSHConnectionHealthOptions, parseSSHConfig, type SSHHostConfig } from "./sshConfig";
import { createLocalShellEnvironment } from "./shellEnvironment";
import { HOST_BOOTSTRAP_LOADER } from "./remoteHostBootstrap";
import { openRemoteHostPty, type RemotePtyOptions, type RemotePtySession, type RemoteHostPty } from "./remoteHostPtyConnection";
import { createHostRecordDecoder, decodeHostBytes, encodeHostRecord, HOST_BUFFER_BYTES, HOST_CHUNK_BYTES, HOST_PROTOCOL_VERSION, isValidHostNamespace, type HostRecord } from "./remoteHostProtocol";

export interface HostProcess extends EventEmitter {
  stdin: Writable;
  stdout: Readable;
  stderr: Readable;
  kill(signal?: NodeJS.Signals): boolean;
}
interface PtyArtifactFile { path: string; sha256: string; data: string; mode: number }
interface Bootstrap { source: string; hash: string; namespace: string; clientId: string; nativeArtifacts?: { common: PtyArtifactFile[]; platforms: Record<string, PtyArtifactFile[]> } }
interface ExecuteOptions {
  timeoutMs?: number;
  maxBufferBytes?: number;
  remoteStdin?: string;
  remoteStdinResponder?: { marker: string; respond: (payload: string) => Promise<string> };
}
export type HostHookHandler = (request: HostRecord) => Promise<{ status: number; headers: Record<string, string>; body: string }>;
const MAX_PENDING = 128;
const START_TIMEOUT_MS = 15_000;
const DEFAULT_TIMEOUT_MS = 45_000;
const DEFAULT_OUTPUT_BYTES = 10 * 1024 * 1024;
let configuration: { namespace: string; clientId: string } | undefined;
let generation = 0;
const connections = new Map<string, Promise<HostConnection | null>>();
const live = new Set<HostConnection>();
const endpointKeys = new Map<string, Promise<string>>();
const execFileAsync = promisify(execFile);

export function getRemoteHostEndpointKey(config: SSHHostConfig): Promise<string> {
  const configKey = JSON.stringify(config);
  const existing = endpointKeys.get(configKey);
  if (existing) return existing;
  const resolving = execFileAsync("ssh", ["-G", ...buildSSHArgs(config, { disableTty: true, strictHostKeyChecking: true })], {
    env: createLocalShellEnvironment(), timeout: 10_000, maxBuffer: 1024 * 1024,
  }).then(({ stdout }) => {
    const effective = stdout.split("\n").filter((line) => !line.startsWith("host ")).join("\n");
    const identity = /%[nk]/.test(effective) ? JSON.stringify([config.host, effective]) : effective;
    return createHash("sha256").update(identity).digest("hex");
  });
  endpointKeys.set(configKey, resolving);
  void resolving.catch(() => { if (endpointKeys.get(configKey) === resolving) endpointKeys.delete(configKey); });
  return resolving;
}

export function configureRemoteHostTransport(config: { namespace: string; clientId: string }): void {
  if (!isValidHostNamespace(config.namespace) || !/^[A-Za-z0-9-]+$/.test(config.clientId)) throw new Error("Invalid host transport identity");
  if (configuration?.namespace === config.namespace && configuration.clientId === config.clientId) return;
  stopRemoteHostConnections();
  configuration = { ...config };
}

export function getRemoteHostNamespace(): string | undefined { return configuration?.namespace; }

export function stopRemoteHostConnections(): void {
  generation += 1;
  configuration = undefined;
  connections.clear();
  endpointKeys.clear();
  for (const connection of live) connection.close();
  live.clear();
}

async function readHostSource(fileName = "host-server.cjs"): Promise<string> {
  let directory = __dirname;
  for (let depth = 0; depth < 9; depth += 1) {
    try { return await readFile(path.join(directory, "build", "remote", fileName), "utf8"); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }
  throw new Error("Host server bundle missing; run the desktop build");
}

function shellQuote(value: string): string { return `'${value.replaceAll("'", `'"'"'`)}'`; }

export async function getRemoteHostConnection(sshHost: string): Promise<HostConnection | null> {
  if (!configuration || process.platform === "win32") return null;
  const identity = configuration;
  const epoch = generation;
  const config = (await parseSSHConfig()).find((value) => value.host === sshHost);
  if (!config) throw new Error(`SSH 호스트를 찾을 수 없습니다: ${sshHost}`);
  if (epoch !== generation) throw new Error("Host preparation cancelled");
  const key = JSON.stringify([await getRemoteHostEndpointKey(config), identity.namespace]);
  if (epoch !== generation) throw new Error("Host preparation cancelled");
  const existing = connections.get(key);
  if (existing) return existing;
  const starting = startSSHHost(config, identity).then((connection) => {
    if (epoch !== generation) { connection?.close(); throw new Error("Host preparation cancelled"); }
    if (connection) {
      live.add(connection);
      connection.once("close", () => { live.delete(connection); endpointKeys.clear(); if (connections.get(key) === starting) connections.delete(key); });
    } else {
      const timer = setTimeout(() => { if (connections.get(key) === starting) connections.delete(key); }, 60_000);
      timer.unref();
    }
    return connection;
  });
  connections.set(key, starting);
  void starting.catch(() => { if (connections.get(key) === starting) connections.delete(key); });
  return starting;
}

async function startSSHHost(config: SSHHostConfig, identity: { namespace: string; clientId: string }): Promise<HostConnection | null> {
  const source = await readHostSource();
  const nativeArtifacts = JSON.parse(await readHostSource("host-pty-artifacts.json")) as Bootstrap["nativeArtifacts"];
  const hash = createHash("sha256").update(source).digest("hex");
  const command = `kanvibe_node=$(command -v node) || { printf '{"type":"unsupported"}\\n'; exit 0; }; "$kanvibe_node" -e 'process.exit(process.platform!=="win32" && Number(process.versions.node.split(".")[0])>=18?0:1)' || { printf '{"type":"unsupported"}\\n'; exit 0; }; exec "$kanvibe_node" -e ${shellQuote(HOST_BOOTSTRAP_LOADER)}`;
  const child = spawn("ssh", [
    ...buildSSHArgs(config, { disableTty: true, strictHostKeyChecking: true, connectionHealth: getKanvibeSSHConnectionHealthOptions() }),
    `sh -lc ${shellQuote(command)}`,
  ], { env: createLocalShellEnvironment(), stdio: ["pipe", "pipe", "pipe"] });
  return connectHostProcess(child, { source, hash, nativeArtifacts, ...identity });
}

export function connectHostProcess(child: HostProcess, bootstrap: Bootstrap): Promise<HostConnection | null> {
  return new Promise((resolve, reject) => {
    let stage: "bootstrap" | "bridge" | "framed" = "bootstrap";
    let preamble = Buffer.alloc(0);
    let settled = false;
    let expectedPtyHash: string | undefined;
    let stderr = "";
    const connection = new HostConnection(child);
    const timer = setTimeout(() => fail(new Error("Host bootstrap timed out")), START_TIMEOUT_MS);
    const fail = (error: Error) => {
      clearTimeout(timer);
      if (!settled) { settled = true; reject(error); }
      connection.close();
    };
    child.stderr.on("data", (chunk: Buffer) => { stderr = (stderr + chunk.toString()).slice(-4096); });
    const decode = createHostRecordDecoder((record) => {
      if (!settled) {
        if (record.type !== "ready" || record.version !== HOST_PROTOCOL_VERSION || record.namespace !== bootstrap.namespace || record.buildHash !== bootstrap.hash || typeof record.instanceId !== "string" || !Number.isSafeInteger(record.pid) || !Array.isArray(record.capabilities) || !["exec", "socket", "hooks"].every((capability) => (record.capabilities as unknown[]).includes(capability))) throw new Error("Host handshake mismatch");
        if (expectedPtyHash && (record.ptyArtifactHash !== expectedPtyHash || !(record.capabilities as unknown[]).includes("pty"))) throw new Error("Host PTY artifact handshake mismatch");
        settled = true; clearTimeout(timer); connection.instanceId = record.instanceId; connection.remotePid = record.pid as number; resolve(connection);
      } else connection.receive(record);
    });
    child.stdout.on("data", (chunk: Buffer) => {
      try {
        if (stage === "framed") return decode(chunk);
        preamble = Buffer.concat([preamble, chunk]);
        if (preamble.length > 4096) throw new Error("Invalid host bootstrap preamble");
        let newline: number;
        while (stage !== "framed" && (newline = preamble.indexOf(10)) >= 0) {
          const record = JSON.parse(preamble.subarray(0, newline).toString());
          preamble = preamble.subarray(newline + 1);
          if (record.type === "unsupported" && stage === "bootstrap") { settled = true; clearTimeout(timer); resolve(null); connection.close(); return; }
          if (stage === "bootstrap" && record.type === "bootstrap-ready") {
            stage = "bridge";
            const { nativeArtifacts, ...payload } = bootstrap;
            let pty;
            if (nativeArtifacts) {
              const platform = `${record.platform}-${record.arch}`;
              const nativeFiles = nativeArtifacts.platforms[platform];
              if (!nativeFiles || Number(record.napi) < 8) throw new Error(`Remote PTY capability unavailable for ${platform} / Node-API ${record.napi}`);
              const files = [...nativeArtifacts.common, ...nativeFiles];
              expectedPtyHash = createHash("sha256").update(JSON.stringify(files)).digest("hex");
              pty = { platform, files, hash: expectedPtyHash };
            }
            child.stdin.write(`${JSON.stringify({ ...payload, ...(pty ? { pty } : {}) })}\n`);
          }
          else if (stage === "bridge" && record.type === "bridge-ready") {
            stage = "framed";
            connection.send({ type: "hello", version: HOST_PROTOCOL_VERSION, namespace: bootstrap.namespace, clientId: bootstrap.clientId, role: "desktop", buildHash: bootstrap.hash });
          } else throw new Error("Invalid host bootstrap sequence");
        }
        if (stage === "framed" && preamble.length) { const rest = preamble; preamble = Buffer.alloc(0); decode(rest); }
      } catch (error) { fail(error instanceof Error ? error : new Error(String(error))); }
    });
    child.once("error", fail);
    child.once("close", () => fail(new Error(`SSH host disconnected; command outcome may be unknown${stderr ? `: ${stderr}` : ""}`)));
    child.stdin.on("error", fail);
  });
}

export class HostConnection extends EventEmitter {
  instanceId = "";
  remotePid = 0;
  private closed = false;
  private receivers = new Map<string, (record: HostRecord) => void>();
  private hookHandler?: HostHookHandler;
  constructor(private child: HostProcess) { super(); }

  openPty(options: RemotePtyOptions): Promise<RemoteHostPty> {
    if (this.receivers.size >= MAX_PENDING) return Promise.reject(new Error("Host request capacity exceeded"));
    return openRemoteHostPty(record => this.send(record), (id, receive) => {
      this.receivers.set(id, receive); return () => { this.receivers.delete(id); };
    }, options);
  }

  async listPtySessions(): Promise<RemotePtySession[]> {
    const record = await this.requestPty("pty.list", {}, "pty.sessions");
    if (!Array.isArray(record.sessions) || record.sessions.some(session => !session || typeof session.sessionId !== "string" || typeof session.key !== "string" || !Number.isSafeInteger(session.pid))) throw new Error("Invalid PTY session list");
    return record.sessions as RemotePtySession[];
  }

  async closePtySession(sessionId: string): Promise<void> { await this.requestPty("pty.close", { sessionId }, "pty.closed"); }

  private requestPty(type: string, fields: object, expected: string): Promise<HostRecord> {
    if (this.receivers.size >= MAX_PENDING) return Promise.reject(new Error("Host request capacity exceeded"));
    return new Promise((resolve, reject) => {
      const id = randomUUID();
      const finish = (error?: Error, record?: HostRecord) => { clearTimeout(timer); this.receivers.delete(id); if (error) reject(error); else resolve(record!); };
      const timer = setTimeout(() => finish(new Error("PTY request timed out; outcome may be unknown")), START_TIMEOUT_MS);
      this.receivers.set(id, record => {
        if (record.type === expected) finish(undefined, record);
        else if (record.type === "pty.error" || record.type === "disconnected") finish(new Error(String(record.error ?? "Host disconnected; PTY request outcome may be unknown")));
      });
      try { this.send({ type, id, ...fields }); } catch (error) { finish(error as Error); }
    });
  }

  send(record: HostRecord): boolean {
    if (this.closed || this.child.stdin.writableLength > HOST_BUFFER_BYTES) throw new Error("Host transport disconnected or backpressured");
    return this.child.stdin.write(encodeHostRecord(record));
  }

  receive(record: HostRecord): void {
    if (record.type === "hook.request") {
      void this.answerHook(record);
      return;
    }
    if (typeof record.id === "string") this.receivers.get(record.id)?.(record);
  }

  setHookHandler(handler: HostHookHandler): void {
    this.hookHandler = handler;
    this.send({ type: "hook.subscribe", id: randomUUID() });
  }

  private async answerHook(request: HostRecord): Promise<void> {
    try {
      const response = this.hookHandler ? await this.hookHandler(request) : { status: 503, headers: {}, body: "" };
      this.send({ type: "hook.response", id: request.id, ...response });
    } catch {
      if (!this.closed) {
        try { this.send({ type: "hook.response", id: request.id, status: 503, headers: {}, body: "" }); }
        catch { this.close(); }
      }
    }
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    for (const [id, receive] of this.receivers) receive({ type: "disconnected", id });
    this.receivers.clear();
    this.child.stdin.end(); this.child.kill("SIGTERM"); this.emit("close");
  }

  createProcess(command: string, options: ExecuteOptions = {}): HostProcess {
    if (this.receivers.size >= MAX_PENDING) throw new Error("Host request capacity exceeded");
    const id = randomUUID();
    const output = new PassThrough(), errors = new PassThrough();
    const child = new EventEmitter() as HostProcess;
    let ended = false;
    let acknowledgeInput: (() => void) | undefined;
    let rejectInput: ((error: Error) => void) | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const finish = (code: number | null, signal: string | null, error?: Error) => {
      if (ended) return;
      ended = true; clearTimeout(timer); this.receivers.delete(id);
      rejectInput?.(error ?? new Error("Host command stdin closed"));
      output.end(); errors.end();
      if (error && child.listenerCount("error")) child.emit("error", error);
      child.emit("close", code, signal);
    };
    const sendInput = async (bytes: Buffer) => {
      for (let offset = 0; offset < bytes.length; offset += HOST_CHUNK_BYTES) {
        if (ended) throw new Error("Host command stdin closed");
        await new Promise<void>((resolve, reject) => {
          acknowledgeInput = resolve; rejectInput = reject;
          this.send({ type: "exec.stdin", id, data: bytes.subarray(offset, offset + HOST_CHUNK_BYTES).toString("base64") });
        });
        acknowledgeInput = undefined; rejectInput = undefined;
      }
    };
    child.stdin = new Writable({
      write: (chunk, _encoding, done) => { void sendInput(chunk).then(() => done(), (error) => done(error as Error)); },
      final: (done) => { try { if (!ended) this.send({ type: "exec.stdin", id, data: "", end: true }); done(); } catch (error) { done(error as Error); } },
    });
    child.stdout = output; child.stderr = errors;
    child.kill = () => { if (!ended) { try { this.send({ type: "exec.cancel", id }); } catch { /* The connection already cancelled its attachments. */ } finish(null, "SIGTERM"); } return true; };
    child.stdin.on("error", () => child.kill());
    this.receivers.set(id, (record) => {
      if (record.type === "disconnected") finish(null, null, new Error("Host transport disconnected; command outcome may be unknown"));
      else if (record.type === "exec.stdin-ready") acknowledgeInput?.();
      else if (record.type === "exec.data") {
        const stream = record.stream === "stdout" ? output : errors;
        if (!stream.write(decodeHostBytes(record.data)) && stream.readableLength > HOST_BUFFER_BYTES) child.kill();
      } else if (record.type === "exec.exit") finish(record.code as number | null, record.signal as string | null, typeof record.error === "string" ? new Error(record.error) : undefined);
    });
    const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    timer = setTimeout(() => { child.kill(); }, timeoutMs);
    try { this.send({ type: "exec.open", id, command, timeoutMs, maxBufferBytes: options.maxBufferBytes ?? 0, interactive: true }); }
    catch (error) { this.receivers.delete(id); clearTimeout(timer); throw error; }
    return child;
  }

  execute(command: string, options: ExecuteOptions = {}): Promise<string> {
    return new Promise((resolve, reject) => {
      const child = this.createProcess(command, { ...options, maxBufferBytes: options.maxBufferBytes ?? DEFAULT_OUTPUT_BYTES });
      const stdout: Buffer[] = [], stderr: Buffer[] = [];
      const decoder = new StringDecoder("utf8");
      let bytes = 0, pendingLine = "", responded = false, responseComplete = false;
      let failure: Error | undefined;
      const responder = options.remoteStdinResponder;
      child.stdout.on("data", (chunk: Buffer) => {
        bytes += chunk.length; stdout.push(chunk);
        if (bytes > (options.maxBufferBytes ?? DEFAULT_OUTPUT_BYTES)) { failure = new Error("Host command output exceeded maxBuffer"); child.kill(); return; }
        if (!responder || responded) return;
        pendingLine += decoder.write(chunk);
        let newline: number;
        while (!responded && (newline = pendingLine.indexOf("\n")) >= 0) {
          const line = pendingLine.slice(0, newline); pendingLine = pendingLine.slice(newline + 1);
          if (!line.startsWith(`${responder.marker}\t`)) continue;
          responded = true;
          void responder.respond(line.slice(responder.marker.length + 1)).then((response) => {
            if (this.closed) throw new Error("Host disconnected during transaction");
            responseComplete = true; child.stdin.end(response);
          }).catch((error) => { failure = error instanceof Error ? error : new Error(String(error)); child.kill(); });
        }
      });
      child.stderr.on("data", (chunk: Buffer) => { bytes += chunk.length; stderr.push(chunk); if (bytes > (options.maxBufferBytes ?? DEFAULT_OUTPUT_BYTES)) { failure = new Error("Host command output exceeded maxBuffer"); child.kill(); } });
      child.once("error", (error: Error) => { failure = error; });
      child.once("close", (code: number | null, signal: string | null) => {
        const out = Buffer.concat(stdout).toString("utf8"), err = Buffer.concat(stderr).toString("utf8");
        if (failure || code !== 0 || (responder && (!responded || !responseComplete))) {
          reject(Object.assign(failure ?? new Error(err || `Remote command exited with ${signal ?? code ?? "unknown"}`), { stdout: out, stderr: err, code, signal, remoteCommandCompleted: code !== null }));
        } else {
          const visible = responder ? out.split("\n").filter((line) => !line.startsWith(`${responder.marker}\t`)).join("\n") : out;
          resolve(visible.trim());
        }
      });
      if (options.remoteStdin !== undefined) child.stdin.write(options.remoteStdin);
      if (!responder) child.stdin.end();
    });
  }

  async forwardSocket(remotePath: string): Promise<{ socketPath: string; close(): Promise<void> }> {
    const directory = await mkdtemp(path.join(tmpdir(), "kvhost-"));
    const sockets = new Set<Socket>();
    const server = createServer((socket) => {
      const id = randomUUID();
      let acknowledgeInput: (() => void) | undefined;
      let rejectInput: ((error: Error) => void) | undefined;
      let sending = Promise.resolve();
      if (this.receivers.size >= MAX_PENDING || this.closed) { socket.destroy(); return; }
      sockets.add(socket); socket.pause();
      this.receivers.set(id, (record) => {
        if (record.type === "socket.ready") socket.resume();
        else if (record.type === "socket.data-ready") acknowledgeInput?.();
        else if (record.type === "socket.data") { if (!socket.write(decodeHostBytes(record.data)) && socket.writableLength > HOST_BUFFER_BYTES) socket.destroy(); }
        else if (record.type === "socket.end") socket.end();
        else if (record.type === "socket.error" || record.type === "socket.close" || record.type === "disconnected") socket.destroy();
      });
      socket.on("data", (chunk: Buffer) => {
        socket.pause();
        sending = (async () => {
          for (let offset = 0; offset < chunk.length; offset += HOST_CHUNK_BYTES) {
            await new Promise<void>((resolve, reject) => {
              acknowledgeInput = resolve; rejectInput = reject;
              this.send({ type: "socket.data", id, data: chunk.subarray(offset, offset + HOST_CHUNK_BYTES).toString("base64") });
            });
          }
          acknowledgeInput = undefined; rejectInput = undefined; socket.resume();
        })();
        void sending.catch(() => socket.destroy());
      });
      socket.on("end", () => { void sending.then(() => this.send({ type: "socket.end", id })).catch(() => socket.destroy()); });
      socket.on("error", () => socket.destroy());
      socket.once("close", () => { rejectInput?.(new Error("Host socket closed")); sockets.delete(socket); this.receivers.delete(id); try { this.send({ type: "socket.close", id }); } catch { /* Detached connection owns no remote sockets. */ } });
      try { this.send({ type: "socket.open", id, path: remotePath }); } catch { socket.destroy(); }
    });
    const socketPath = path.join(directory, "upstream.sock");
    try { await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(socketPath, resolve); }); }
    catch (error) { await rm(directory, { recursive: true, force: true }); throw error; }
    return { socketPath, close: async () => { for (const socket of sockets) socket.destroy(); await new Promise<void>((resolve) => server.close(() => resolve())); await rm(directory, { recursive: true, force: true }); } };
  }
}
