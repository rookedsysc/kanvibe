import { spawn, type ChildProcess, type ChildProcessWithoutNullStreams } from "node:child_process";
import { timingSafeEqual, X509Certificate } from "node:crypto";
import { createServer } from "node:https";
import { StringDecoder } from "node:string_decoder";
import type { Readable, Writable } from "node:stream";
import { WebSocketServer, WebSocket } from "ws";
import { createLocalShellEnvironment } from "./shellEnvironment";
import {
  decodeRemoteTerminalBytes, isRemoteTerminalSize, parseHerdrTerminalRecord, buildScreenRedraw, REMOTE_TERMINAL_COMMAND_TIMEOUT_MS,
  REMOTE_TERMINAL_ENVELOPE_BYTES, REMOTE_TERMINAL_FRAME_BYTES, REMOTE_TERMINAL_INPUT_BYTES, REMOTE_TERMINAL_INPUT_QUEUE_BYTES,
  REMOTE_TERMINAL_OWNER_LEASE_MS, REMOTE_TERMINAL_ADMISSION_MS, type RemoteTerminalWireGrant,
} from "./remoteTerminalProtocol";

interface Client {
  grant: Grant;
  send: (data: string) => void;
  close: (code: number, reason: string) => void;
  size: { cols: number; rows: number } | null;
  order: number;
  wrote: boolean;
  pendingBytes: number;
  ready: boolean;
}
interface Grant extends RemoteTerminalWireGrant { active?: Client; admissionUntil: number }
interface Group {
  child: ChildProcessWithoutNullStreams | null;
  clients: Set<Client>;
  grant: Grant;
  ready: Promise<void>;
  cancelInitial: () => void;
  history: string;
  initialized: boolean;
  ownsTerminal: boolean;
  original: { cols: number; rows: number } | null;
  size: { cols: number; rows: number };
  joiners: Map<Client, { history: string; resolve: () => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>;
  dead: boolean;
  order: number;
}

/** Runs only inside the dedicated SSH child, with all authority supplied over its private stdin. */
export async function startRemoteTerminalAgent(
  certificate: { cert: string; key: string }, input: Readable, output: Writable,
): Promise<{ port: number; stop: () => void }> {
  const grants = new Map<string, Grant>();
  const groups = new Map<string, Group>();
  const closingGroups = new Map<string, Promise<void>>();
  const relays = new Map<string, Client>();
  const queries = new Set<ChildProcess>();
  const wsServer = new WebSocketServer({ noServer: true, maxPayload: REMOTE_TERMINAL_INPUT_BYTES, perMessageDeflate: false });
  const server = createServer({ ...certificate, handshakeTimeout: REMOTE_TERMINAL_COMMAND_TIMEOUT_MS }, (_request, response) => {
    response.writeHead(404, { Connection: "close" }); response.end();
  });
  server.headersTimeout = REMOTE_TERMINAL_COMMAND_TIMEOUT_MS;
  server.requestTimeout = REMOTE_TERMINAL_COMMAND_TIMEOUT_MS;
  server.setTimeout(REMOTE_TERMINAL_COMMAND_TIMEOUT_MS, (socket) => socket.destroy());
  server.on("clientError", (_error, socket) => socket.destroy());
  let stopped = false;
  let lastOwnerHeartbeat = performance.now();
  let partial = "";
  const decoder = new StringDecoder("utf8");
  const emit = (message: object) => {
    if (stopped) return;
    if (output.writableLength > REMOTE_TERMINAL_ENVELOPE_BYTES) { stop(); return; }
    output.write(`${JSON.stringify(message)}\n`);
  };

  const sendController = (group: Group, record: object) => {
    const child = group.child;
    const line = `${JSON.stringify(record)}\n`;
    // stdin queues encoded NDJSON, so its budget must include base64 expansion, unlike raw input frames.
    if (group.dead || !child || child.stdin.writableLength + Buffer.byteLength(line) > REMOTE_TERMINAL_ENVELOPE_BYTES) throw new Error("Terminal input unavailable");
    child.stdin.write(line);
  };
  const closeGroup = (group: Group, code = 4000, reason = "stream-failed") => {
    if (group.dead) return;
    group.dead = true;
    group.cancelInitial();
    if (groups.get(group.grant.targetKey) === group) groups.delete(group.grant.targetKey);
    if (code !== 1000) for (const grant of grants.values()) if (grant.targetKey === group.grant.targetKey) grants.delete(grant.grantId);
    for (const joiner of group.joiners.values()) { clearTimeout(joiner.timer); joiner.reject(new Error("Terminal closed")); }
    group.joiners.clear();
    for (const client of [...group.clients]) client.close(code, reason);
    group.clients.clear();
    const child = group.child;
    if (!child) return;
    const closing = new Promise<void>((resolve) => {
      if (child.exitCode !== null || child.signalCode !== null) resolve(); else child.once("close", () => resolve());
    });
    closingGroups.set(group.grant.targetKey, closing);
    void closing.then(() => { if (closingGroups.get(group.grant.targetKey) === closing) closingGroups.delete(group.grant.targetKey); });
    const release = [{ type: "terminal.release" }];
    const restore = group.ownsTerminal && group.original && child.exitCode === null && child.signalCode === null
      ? [{ type: "terminal.resize", cols: group.original!.cols, rows: group.original!.rows }] : [];
    // Native release drops authority but keeps the last grid; restore through this still-owned pipe first.
    child.stdin.end([...restore, ...release].map((record) => JSON.stringify(record)).join("\n") + "\n");
    const killer = setTimeout(() => child.kill("SIGTERM"), REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);
    killer.unref();
    child.once("exit", () => clearTimeout(killer));
  };
  const detach = (client: Client) => {
    if (client.grant.active === client) client.grant.active = undefined;
    if (client.wrote) grants.delete(client.grant.grantId);
    const group = groups.get(client.grant.targetKey);
    if (!group || !group.clients.delete(client) || group.dead) return;
    const joiner = group.joiners.get(client);
    if (joiner) { group.joiners.delete(client); clearTimeout(joiner.timer); joiner.reject(new Error("Terminal client detached")); }
    if (group.clients.size === 0) { closeGroup(group, 1000, "detached"); return; }
    if (!group.child) return;
    const owner = [...group.clients].filter((candidate) => candidate.size).sort((a, b) => b.order - a.order)[0];
    const size = owner?.size ?? { cols: group.grant.cols, rows: group.grant.rows };
    group.size = size;
    try { sendController(group, { type: "terminal.resize", ...size }); }
    catch { closeGroup(group); }
  };

  const stopQuery = (child: ChildProcess) => {
    // Read-only CLI queries run in their own group; killing just a compound shell leaves its RPC alive.
    try {
      if (process.platform !== "win32" && child.pid) process.kill(-child.pid, "SIGKILL");
      else child.kill("SIGKILL");
    } catch { child.kill("SIGKILL"); }
  };

  const snapshot = (command: string) => new Promise<string>((resolve, reject) => {
    const child = spawn("sh", ["-lc", command], { env: createLocalShellEnvironment(), stdio: ["ignore", "pipe", "ignore"], detached: process.platform !== "win32" });
    queries.add(child);
    const chunks: Buffer[] = [];
    let length = 0;
    const timeout = setTimeout(() => { stopQuery(child); reject(new Error("Terminal snapshot timed out")); }, REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);
    child.stdout.on("data", (chunk: Buffer) => {
      length += chunk.length;
      if (length > REMOTE_TERMINAL_FRAME_BYTES) { stopQuery(child); reject(new Error("Terminal snapshot too large")); }
      else chunks.push(chunk);
    });
    child.once("error", reject);
    child.once("close", (code) => {
      queries.delete(child);
      clearTimeout(timeout);
      if (code === 0) resolve(Buffer.concat(chunks).toString("utf8"));
      else reject(new Error("Terminal snapshot unavailable"));
    });
  });

  const getGroup = async (grant: Grant, client: Client): Promise<Group> => {
    const closing = closingGroups.get(grant.targetKey);
    if (closing) await closing;
    if (!grants.has(grant.grantId) || grant.active !== client) throw new Error("Terminal grant revoked");
    const existing = groups.get(grant.targetKey);
    if (existing) { existing.clients.add(client); return existing; }
    const group: Group = { child: null, cancelInitial: () => {}, clients: new Set([client]), grant, ready: Promise.resolve(), history: "", initialized: false, ownsTerminal: false, original: null, size: { cols: grant.cols, rows: grant.rows }, joiners: new Map(), dead: false, order: 0 };
    groups.set(grant.targetKey, group);
    group.ready = (async () => {
      // The desktop's ticket can have been issued during another mobile viewport lease.
      // Read the native baseline on B immediately before claiming a new controller.
      const parts = (await snapshot(grant.geometryCommand)).split("\x1e");
      if (parts.length !== 2) throw new Error("Terminal geometry unavailable");
      const pane = JSON.parse(parts[0]).result?.pane;
      const layout = JSON.parse(parts[1]).result?.layout;
      const rect = layout?.panes?.find((entry: { pane_id: string }) => entry.pane_id === grant.paneId)?.rect;
      const cols = layout?.zoomed && layout.focused_pane_id === grant.paneId ? layout.area.width : rect?.width;
      const rows = pane?.scroll?.viewport_rows;
      if (!pane || pane.pane_id !== grant.paneId || pane.restore_error || !rect || ![cols, rows].every((size) => Number.isInteger(size) && size > 0)) throw new Error("Terminal geometry unavailable");
      if (group.dead) throw new Error("Terminal client detached before claim");
      const owner = [...group.clients].find((candidate) => grants.has(candidate.grant.grantId) && candidate.grant.active === candidate);
      if (!owner) throw new Error("Terminal grant revoked before claim");
      group.grant = owner.grant;
      group.size = { cols: owner.grant.cols, rows: owner.grant.rows };
      group.original = { cols, rows };
      const child = spawn("sh", ["-lc", `exec ${owner.grant.controlCommand}`], { env: createLocalShellEnvironment(), stdio: ["pipe", "pipe", "pipe"] });
      group.child = child;
      let resolveInitial!: () => void;
      let rejectInitial!: (error: Error) => void;
      const initialFrame = new Promise<void>((resolve, reject) => { resolveInitial = resolve; rejectInitial = reject; });
      const initialTimer = setTimeout(() => rejectInitial(new Error("Terminal initial frame timed out")), REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);
      group.cancelInitial = () => { clearTimeout(initialTimer); rejectInitial(new Error("Terminal closed before initial frame")); };
      let lines = "";
      let bufferedFrames = "";
      let bootstrapping = true;
      const frames = new StringDecoder("utf8");
      child.stdout.setEncoding("utf8");
      child.stdout.on("data", (chunk: string) => {
        if (group.dead) return;
        try {
          lines += chunk;
          if (Buffer.byteLength(lines) > REMOTE_TERMINAL_ENVELOPE_BYTES) throw new Error("Terminal record too large");
          let end: number;
          while ((end = lines.indexOf("\n")) >= 0) {
            const line = lines.slice(0, end); lines = lines.slice(end + 1);
            if (!line.trim()) continue;
            const record = parseHerdrTerminalRecord(line);
            if (record?.type === "invalid") throw new Error("Invalid terminal frame");
            if (record?.type === "closed") { group.ownsTerminal = false; closeGroup(group); return; }
            if (record?.type !== "frame") continue;
            const text = frames.write(record.bytes);
            const joined = new Set<Client>();
            if (record.isFullScreen) {
              group.ownsTerminal = true;
              clearTimeout(initialTimer); resolveInitial();
              for (const [joining, joiner] of group.joiners) {
                group.joiners.delete(joining); clearTimeout(joiner.timer);
                joining.send(joiner.history + text); joining.ready = true; joined.add(joining); joiner.resolve();
              }
            }
            if (bootstrapping) {
              bufferedFrames += text;
              if (Buffer.byteLength(bufferedFrames) > REMOTE_TERMINAL_FRAME_BYTES) throw new Error("Terminal bootstrap too large");
            } else for (const client of [...group.clients]) if (client.ready && !joined.has(client)) client.send(text);
          }
        } catch { closeGroup(group); }
      });
      child.stderr.resume();
      child.on("error", () => { group.ownsTerminal = false; closeGroup(group); });
      child.on("exit", () => { group.ownsTerminal = false; closeGroup(group); });
      child.stdin.on("error", () => { group.ownsTerminal = false; closeGroup(group); });
      const [history] = await Promise.all([snapshot(owner.grant.snapshotCommand), initialFrame]);
      if (group.dead) throw new Error("Terminal closed during bootstrap");
      group.history = buildScreenRedraw(history, null);
      group.initialized = true;
      bootstrapping = false;
      for (const client of group.clients) { client.send(group.history + bufferedFrames); client.ready = true; }
    })().catch((error: unknown) => { closeGroup(group); throw error; });
    // A direct client may leave while history is loading; keep the group failure observed.
    void group.ready.catch(() => {});
    return group;
  };

  const attach = async (grant: Grant, client: Client) => {
    if (grant.admissionUntil <= performance.now()) throw new Error("Expired terminal grant");
    if (grant.active?.wrote) throw new Error("Terminal grant already in use");
    const old = grant.active;
    grant.active = client;
    const existing = groups.get(grant.targetKey);
    const wasReady = existing?.initialized === true;
    const group = await getGroup(grant, client);
    old?.close(1000, "transport-replaced");
    await group.ready;
    if (group.dead || grant.active !== client || !grants.has(grant.grantId)) throw new Error("Terminal grant revoked");
    if (wasReady) {
      // Re-read history and request a current native full frame instead of caching unbounded deltas.
      const history = buildScreenRedraw(await snapshot(grant.snapshotCommand), null);
      if (group.dead || grant.active !== client || !grants.has(grant.grantId)) throw new Error("Terminal grant revoked");
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => { group.joiners.delete(client); reject(new Error("Terminal refresh timed out")); }, REMOTE_TERMINAL_COMMAND_TIMEOUT_MS);
        group.joiners.set(client, { history, resolve, reject, timer });
        try { sendController(group, { type: "terminal.resize", ...group.size }); }
        catch (error) { clearTimeout(timer); group.joiners.delete(client); reject(error); }
      });
    }
  };

  const receive = (client: Client, bytes: Buffer, binary: boolean) => {
    if (client.grant.active !== client || !grants.has(client.grant.grantId)) throw new Error("Terminal grant revoked");
    if (bytes.length > REMOTE_TERMINAL_INPUT_BYTES) throw new Error("Terminal input too large");
    const group = groups.get(client.grant.targetKey);
    if (!group || group.dead) throw new Error("Terminal unavailable");
    if (binary) {
      const record = JSON.parse(bytes.toString("utf8"));
      // 이력 갱신은 A의 미러에만 있는 요청이다. 직접 연결한 기기가 보내도 이 연결의 이력은 붙을 때 받은 것 그대로다.
      if (record.type === "refresh-history") return;
      if (record.type !== "resize" || !isRemoteTerminalSize(record.cols, record.rows)) throw new Error("Invalid terminal size");
      client.size = { cols: record.cols, rows: record.rows };
      group.size = client.size;
      client.order = ++group.order;
      sendController(group, { type: "terminal.resize", ...client.size });
    } else {
      if (!Buffer.from(bytes.toString("utf8"), "utf8").equals(bytes)) throw new Error("Invalid UTF-8 terminal input");
      client.wrote = true;
      sendController(group, { type: "terminal.input", bytes: bytes.toString("base64") });
    }
  };
  const revoke = (test: (grant: Grant) => boolean, code: number, reason: string) => {
    for (const grant of [...grants.values()]) if (test(grant)) {
      grants.delete(grant.grantId);
      grant.active?.close(code, reason);
    }
  };

  server.on("upgrade", (request, socket, head) => {
    const match = /^\/stream\/([a-zA-Z0-9-]+)$/.exec(request.url ?? "");
    const grant = match ? grants.get(match[1]) : undefined;
    const authorization = request.headers.authorization;
    const token = authorization?.startsWith("Bearer ") ? authorization.slice(7) : "";
    const expected = Buffer.from(grant?.token ?? "");
    const supplied = Buffer.from(token);
    if (!grant || grant.admissionUntil <= performance.now() || expected.length !== supplied.length || !timingSafeEqual(expected, supplied) || grant.active?.wrote) {
      socket.end("HTTP/1.1 401 Unauthorized\r\nConnection: close\r\n\r\n", () => socket.destroy()); return;
    }
    wsServer.handleUpgrade(request, socket, head, (ws) => {
      let closed = false;
      const client: Client = {
        grant, size: null, order: 0, wrote: false, pendingBytes: 0, ready: false,
        send: (data) => {
          if (closed || ws.readyState !== WebSocket.OPEN) return;
          if (ws.bufferedAmount > REMOTE_TERMINAL_FRAME_BYTES) { client.close(4000, "slow-client"); return; }
          ws.send(data);
        },
        close: (code, reason) => { if (closed) return; closed = true; ws.close(code, reason); detach(client); },
      };
      ws.on("pong", () => { (ws as WebSocket & { alive?: boolean }).alive = true; });
      ws.on("close", () => { closed = true; detach(client); });
      ws.on("error", () => client.close(4000, "stream-failed"));
      const ready = attach(grant, client);
      ws.on("message", (data, binary) => {
        const bytes = Buffer.from(data as Buffer);
        if (client.ready && client.pendingBytes === 0) {
          try { receive(client, bytes, binary); } catch { client.close(4000, "stream-failed"); }
          return;
        }
        client.pendingBytes += bytes.length;
        if (client.pendingBytes > REMOTE_TERMINAL_INPUT_QUEUE_BYTES) { client.close(4000, "input-too-large"); return; }
        void ready.then(() => receive(client, bytes, binary)).catch(() => client.close(4000, "stream-failed")).finally(() => { client.pendingBytes -= bytes.length; });
      });
      void ready.catch(() => client.close(4000, "stream-failed"));
    });
  });

  const handle = async (record: Record<string, unknown>) => {
    if (stopped) return;
    if (record.type === "heartbeat") { lastOwnerHeartbeat = performance.now(); return; }
    if (record.type === "shutdown") { stop(); return; }
    if (record.type === "detach") { relays.get(String(record.relayId))?.close(1000, "detached"); return; }
    if (record.type === "data") {
      const client = relays.get(String(record.relayId));
      if (!client) return;
      try { receive(client, decodeRemoteTerminalBytes(record.data, REMOTE_TERMINAL_INPUT_BYTES), record.binary === true); }
      catch { client.close(4000, "stream-failed"); }
      return;
    }
    if (record.type !== "request" || typeof record.id !== "string") throw new Error("Invalid terminal envelope");
    try {
      if (record.method === "grant") {
        const grant = record.grant as RemoteTerminalWireGrant;
        if (!grant || typeof grant.token !== "string" || grant.token.length < 32 || !isRemoteTerminalSize(grant.cols, grant.rows) || typeof grant.geometryCommand !== "string" || typeof grant.controlCommand !== "string" || typeof grant.snapshotCommand !== "string") throw new Error("Invalid terminal grant");
        grants.set(grant.grantId, { ...grant, admissionUntil: performance.now() + REMOTE_TERMINAL_ADMISSION_MS });
      } else if (record.method === "attach") {
        const grant = grants.get(String(record.grantId));
        if (!grant || grant.deviceId !== record.deviceId || grant.taskId !== record.taskId || grant.paneId !== record.paneId || typeof record.relayId !== "string") throw new Error("Invalid terminal scope");
        const relayId = record.relayId;
        const client: Client = {
          grant, size: null, order: 0, wrote: false, pendingBytes: 0, ready: false,
          send: (data) => emit({ type: "output", relayId, data: Buffer.from(data).toString("base64") }),
          close: (code, reason) => {
            if (relays.get(relayId) !== client) return;
            relays.delete(relayId); detach(client); emit({ type: "closed", relayId, code, reason });
          },
        };
        relays.set(relayId, client);
        await attach(grant, client).catch((error: unknown) => { client.close(4000, "stream-failed"); throw error; });
      } else if (record.method === "revokeDevice") revoke((grant) => grant.deviceId === record.deviceId, 4001, "device-unpaired");
      else if (record.method === "revokeTask") revoke((grant) => grant.taskId === record.taskId, 4002, "session-changed");
      else if (record.method === "revokeGrant") revoke((grant) => grant.grantId === record.grantId, 4000, "grant-revoked");
      else throw new Error("Unknown terminal request");
      emit({ type: "response", id: record.id, ok: true });
    } catch { emit({ type: "response", id: record.id, ok: false, error: "Terminal request rejected" }); }
  };
  const onInput = (chunk: Buffer | string) => {
    try {
      partial += typeof chunk === "string" ? chunk : decoder.write(chunk);
      if (Buffer.byteLength(partial) > REMOTE_TERMINAL_ENVELOPE_BYTES) throw new Error("Terminal envelope too large");
      let end: number;
      while ((end = partial.indexOf("\n")) >= 0) {
        const line = partial.slice(0, end); partial = partial.slice(end + 1);
        void handle(JSON.parse(line)).catch(stop);
      }
    } catch { stop(); }
  };
  input.on("data", onInput);
  input.once("end", stop);
  input.once("error", stop);
  output.once("error", stop);
  const ownerTimer = setInterval(() => { if (performance.now() - lastOwnerHeartbeat >= REMOTE_TERMINAL_OWNER_LEASE_MS) stop();
    for (const grant of grants.values()) if (!grant.active && grant.admissionUntil <= performance.now()) grants.delete(grant.grantId); }, REMOTE_TERMINAL_OWNER_LEASE_MS / 3);
  ownerTimer.unref();
  const streamHeartbeat = setInterval(() => {
    for (const ws of wsServer.clients) {
      if ((ws as WebSocket & { alive?: boolean }).alive === false) { ws.terminate(); continue; }
      (ws as WebSocket & { alive?: boolean }).alive = false; ws.ping();
    }
  }, REMOTE_TERMINAL_OWNER_LEASE_MS);
  streamHeartbeat.unref();

  function stop() {
    if (stopped) return;
    stopped = true;
    clearInterval(ownerTimer); clearInterval(streamHeartbeat);
    input.off("data", onInput);
    input.pause();
    if (input === process.stdin) input.destroy();
    for (const query of queries) stopQuery(query);
    for (const group of [...groups.values()]) closeGroup(group, 1001, "owner-disconnected");
    for (const ws of wsServer.clients) ws.terminate();
    grants.clear(); relays.clear(); wsServer.close(); server.close();
  }
  await new Promise<void>((resolve, reject) => { server.once("error", reject); server.listen(0, "0.0.0.0", resolve); });
  const port = (server.address() as { port: number }).port;
  emit({ type: "ready", port, certificateFingerprint: new X509Certificate(certificate.cert).fingerprint256.replaceAll(":", "").toLowerCase() });
  return { port, stop };
}
