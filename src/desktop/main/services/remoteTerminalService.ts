import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { randomBytes, randomUUID, X509Certificate } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";
import selfsigned from "selfsigned";
import { buildSSHArgs, parseSSHConfig, getKanvibeSSHConnectionHealthOptions, type SSHHostConfig } from "@/lib/sshConfig";
import { createLocalShellEnvironment } from "@/lib/shellEnvironment";
import { buildHerdrCommand } from "@/lib/herdr";
import { buildHerdrControlPaneCommand, buildHerdrRecentScreenCommand } from "@/lib/paneMirror";
import { quoteForPosixShell } from "@/lib/worktree";
import {
  decodeRemoteTerminalBytes, REMOTE_TERMINAL_ADMISSION_MS, REMOTE_TERMINAL_ENVELOPE_BYTES,
  REMOTE_TERMINAL_FRAME_BYTES, REMOTE_TERMINAL_INPUT_BYTES, REMOTE_TERMINAL_MAX_DIMENSION, REMOTE_TERMINAL_OWNER_HEARTBEAT_MS,
  type RemoteTerminalGrant, type RemoteTerminalGrantInput, type RemoteTerminalWireGrant,
} from "@/lib/remoteTerminalProtocol";

interface Relay {
  write: (data: string | Buffer, isBinary: boolean) => void;
  close: () => void;
}
interface RelayCallbacks { data: (data: string) => void; close: (code: number, reason: string) => void }
interface Helper {
  child: ChildProcessWithoutNullStreams;
  port: number;
  fingerprint: string;
  request: (method: string, payload: object) => Promise<void>;
  send: (record: object) => void;
  relays: Map<string, RelayCallbacks>;
  stop: () => void;
}
interface GrantRecord { descriptor: RemoteTerminalGrant; input: RemoteTerminalGrantInput; helper: Helper }
const helpers = new Map<string, Promise<Helper | null>>();
const liveHelpers = new Set<Helper>();
const ownedHelpers = new Set<Helper>();
const grants = new Map<string, GrantRecord>();
const deviceEpochs = new Map<string, number>();
const taskEpochs = new Map<string, number>();
let ownerEpoch = 0;
// Leave time for the mobile metadata request to handle an explicit startup failure.
const START_TIMEOUT_MS = 6_000;
// A warm join may need one history read and one native full-frame acknowledgement.
const REQUEST_TIMEOUT_MS = 12_000;
// Unsupported runtime probes are shared briefly across descriptor lookup and legacy fallback.
const UNSUPPORTED_RUNTIME_CACHE_MS = 60_000;
const CERTIFICATE_VALIDITY_MS = 24 * 60 * 60_000;
const MAX_PENDING_REQUESTS = 128;

// This loader consumes exactly one private bootstrap line before handing stdin to the agent.
// No source, certificate key, or grant capability appears in the remote process argv.
const AGENT_LOADER = `process.stdout.write('{"type":"bootstrap-ready"}\\n');let b='';const f=c=>{b+=c;if(Buffer.byteLength(b)>16777216)process.exit(1);const n=b.indexOf('\\n');if(n<0)return;process.stdin.off('data',f);process.stdin.pause();try{const p=JSON.parse(b.slice(0,n));const m={exports:{}};new Function('require','module','exports',p.source)(require,m,m.exports);m.exports.startRemoteTerminalAgent(p.certificate,process.stdin,process.stdout).then(()=>process.stdin.resume()).catch(()=>process.exit(1));}catch{process.exit(1)}};process.stdin.setEncoding('utf8');process.stdin.on('data',f);`;

async function readAgentSource(): Promise<string> {
  // Both source/dev main and packaged build/main resolve against the app root, not cwd.
  let directory = __dirname;
  for (let index = 0; index < 8; index += 1) {
    try { return await readFile(path.join(directory, "build", "remote", "terminal-agent.cjs"), "utf8"); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }
  throw new Error("Remote terminal agent bundle is missing; run the desktop build.");
}

async function startHelper(config: SSHHostConfig): Promise<Helper | null> {
  const epoch = ownerEpoch;
  const [source, certificate] = await Promise.all([
    readAgentSource(),
    selfsigned.generate([{ name: "commonName", value: "KanVibe Remote Terminal" }], { keySize: 2048, algorithm: "sha256", notAfterDate: new Date(Date.now() + CERTIFICATE_VALIDITY_MS) }),
  ]);
  if (epoch !== ownerEpoch) throw new Error("Remote terminal preparation cancelled");
  const fingerprint = new X509Certificate(certificate.cert).fingerprint256.replaceAll(":", "").toLowerCase();
  const command = [
    'kanvibe_node=$(command -v node) || { printf \'{"type":"unsupported"}\\n\'; exit 0; }',
    '"$kanvibe_node" -e \'process.exit(Number(process.versions.node.split(".")[0])>=18?0:1)\' || { printf \'{"type":"unsupported"}\\n\'; exit 0; }',
    `exec "$kanvibe_node" -e ${quoteForPosixShell(AGENT_LOADER)}`,
  ].join("; ");
  const args = [
    ...buildSSHArgs(config, { disableTty: true, strictHostKeyChecking: true, connectionHealth: getKanvibeSSHConnectionHealthOptions() }),
    `sh -lc ${quoteForPosixShell(command)}`,
  ];
  const child = spawn("ssh", args, { env: createLocalShellEnvironment(), stdio: ["pipe", "pipe", "pipe"] });
  const pending = new Map<string, { resolve: () => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  let resolveReady!: (value: Helper | null) => void;
  let rejectReady!: (error: Error) => void;
  const ready = new Promise<Helper | null>((resolve, reject) => { resolveReady = resolve; rejectReady = reject; });
  let stopped = false;
  let bootstrapped = false;
  let heartbeat: ReturnType<typeof setInterval> | undefined;
  let partial = "";
  const decoder = new StringDecoder("utf8");
  const send = (record: object) => {
    if (stopped || child.stdin.writableLength > REMOTE_TERMINAL_ENVELOPE_BYTES) throw new Error("Remote terminal transport unavailable");
    child.stdin.write(`${JSON.stringify(record)}\n`);
  };
  const helper: Helper = {
    child, port: 0, fingerprint, relays: new Map(), send,
    request: (method, payload) => new Promise<void>((resolve, reject) => {
      if (stopped || pending.size >= MAX_PENDING_REQUESTS) { reject(new Error("Remote terminal request unavailable")); return; }
      const id = randomUUID();
      const timer = setTimeout(() => { pending.delete(id); reject(new Error("Remote terminal request timed out")); }, REQUEST_TIMEOUT_MS);
      pending.set(id, { resolve, reject, timer });
      try { send({ type: "request", id, method, ...payload }); }
      catch (error) { clearTimeout(timer); pending.delete(id); reject(error); }
    }),
    stop: () => {
      if (stopped) return;
      stopped = true;
      clearTimeout(startTimer); if (heartbeat) clearInterval(heartbeat);
      liveHelpers.delete(helper); ownedHelpers.delete(helper);
      for (const [id, grant] of grants) if (grant.helper === helper) grants.delete(id);
      for (const request of pending.values()) { clearTimeout(request.timer); request.reject(new Error("Remote terminal disconnected")); }
      pending.clear();
      for (const relay of helper.relays.values()) relay.close(4000, "stream-failed");
      helper.relays.clear();
      child.stdin.end();
      child.kill("SIGTERM");
      rejectReady(new Error("Remote terminal helper disconnected"));
    },
  };
  const startTimer = setTimeout(() => helper.stop(), START_TIMEOUT_MS);
  ownedHelpers.add(helper);
  child.stdout.on("data", (chunk: Buffer) => {
    try {
      partial += decoder.write(chunk);
      if (Buffer.byteLength(partial) > REMOTE_TERMINAL_ENVELOPE_BYTES) throw new Error("Remote terminal record too large");
      let end: number;
      while ((end = partial.indexOf("\n")) >= 0) {
        const record = JSON.parse(partial.slice(0, end)); partial = partial.slice(end + 1);
        if (record.type === "bootstrap-ready") {
          if (bootstrapped) throw new Error("Duplicate remote terminal bootstrap");
          bootstrapped = true; send({ source, certificate: { cert: certificate.cert, key: certificate.private } });
        }
        else if (record.type === "unsupported") { clearTimeout(startTimer); resolveReady(null); helper.stop(); }
        else if (record.type === "ready") {
          if (!bootstrapped || helper.port !== 0 || !Number.isInteger(record.port) || record.port < 1 || record.port > 65535 || record.certificateFingerprint !== fingerprint) throw new Error("Invalid remote terminal endpoint");
          helper.port = record.port; clearTimeout(startTimer); liveHelpers.add(helper);
          heartbeat = setInterval(() => {
            for (const [id, record] of grants) if (record.descriptor.expiresAt <= Date.now()) grants.delete(id);
            try { send({ type: "heartbeat" }); } catch { helper.stop(); }
          }, REMOTE_TERMINAL_OWNER_HEARTBEAT_MS);
          heartbeat.unref(); resolveReady(helper);
        } else if (record.type === "response") {
          const request = pending.get(record.id);
          if (!request) continue;
          clearTimeout(request.timer); pending.delete(record.id);
          if (record.ok === true) request.resolve(); else request.reject(new Error("Remote terminal request rejected"));
        } else if (record.type === "output") helper.relays.get(record.relayId)?.data(decodeRemoteTerminalBytes(record.data, REMOTE_TERMINAL_FRAME_BYTES).toString("utf8"));
        else if (record.type === "closed") {
          const relay = helper.relays.get(record.relayId); helper.relays.delete(record.relayId);
          relay?.close(Number(record.code) || 4000, String(record.reason));
        }
      }
    } catch { helper.stop(); }
  });
  child.stderr.resume();
  child.on("error", () => helper.stop()); child.on("close", () => helper.stop()); child.stdin.on("error", () => helper.stop());
  return ready;
}

export async function prepareRemoteTerminalGrant(input: RemoteTerminalGrantInput): Promise<RemoteTerminalGrant | null> {
  const epochs = [ownerEpoch, deviceEpochs.get(input.deviceId) ?? 0, taskEpochs.get(input.taskId) ?? 0];
  const isCurrent = () => epochs[0] === ownerEpoch && epochs[1] === (deviceEpochs.get(input.deviceId) ?? 0) && epochs[2] === (taskEpochs.get(input.taskId) ?? 0);
  const config = (await parseSSHConfig()).find((host) => host.host === input.sshHost);
  if (!config) throw new Error("Remote terminal SSH host not found");
  const key = JSON.stringify(config);
  let starting = helpers.get(key);
  if (!starting) {
    starting = startHelper(config);
    helpers.set(key, starting);
    const owned = starting;
    void starting.then((helper) => {
      if (!helper) {
        const timer = setTimeout(() => { if (helpers.get(key) === owned) helpers.delete(key); }, UNSUPPORTED_RUNTIME_CACHE_MS);
        timer.unref();
      }
      helper?.child.once("close", () => { if (helpers.get(key) === owned) helpers.delete(key); });
    }, () => { if (helpers.get(key) === owned) helpers.delete(key); });
  }
  const helper = await starting;
  if (!helper) return null;
  if (!isCurrent()) throw new Error("Remote terminal grant revoked during preparation");
  const grantId = randomUUID();
  const token = randomBytes(32).toString("hex");
  const expiresAt = Date.now() + REMOTE_TERMINAL_ADMISSION_MS;
  const nativeCols = input.cols ?? 80; const nativeRows = input.rows ?? 24;
  if (![nativeCols, nativeRows].every((size) => Number.isInteger(size) && size > 0)) throw new Error("Invalid native terminal geometry");
  const cols = Math.min(nativeCols, REMOTE_TERMINAL_MAX_DIMENSION);
  const rows = Math.min(nativeRows, REMOTE_TERMINAL_MAX_DIMENSION);
  const grant: RemoteTerminalWireGrant = {
    grantId, token, expiresAt, deviceId: input.deviceId, taskId: input.taskId, paneId: input.paneId,
    targetKey: JSON.stringify([input.sessionName, input.paneId]),
    controlCommand: buildHerdrControlPaneCommand(input.sessionName, input.paneId, cols, rows),
    snapshotCommand: buildHerdrRecentScreenCommand(input.sessionName, input.paneId), cols, rows,
    geometryCommand: `${buildHerdrCommand(input.sessionName, ["pane", "get", input.paneId])} && printf '\\036' && ${buildHerdrCommand(input.sessionName, ["pane", "layout", "--pane", input.paneId])}`,
  };
  await helper.request("grant", { grant });
  if (!isCurrent()) { void helper.request("revokeGrant", { grantId }).catch(() => {}); throw new Error("Remote terminal grant revoked during preparation"); }
  const descriptor: RemoteTerminalGrant = { version: 1, grantId, expiresAt, direct: { host: config.hostname, port: helper.port, path: `/stream/${grantId}`, token, certificateFingerprint: helper.fingerprint } };
  grants.set(grantId, { descriptor, input: { ...input }, helper });
  return descriptor;
}

export async function attachRemoteTerminalRelay(
  grantId: string, deviceId: string, taskId: string, paneId: string,
  onData: (data: string) => void, onClose: (code: number, reason: string) => void,
): Promise<Relay> {
  const record = grants.get(grantId);
  if (!record || record.input.deviceId !== deviceId || record.input.taskId !== taskId || record.input.paneId !== paneId || record.descriptor.expiresAt <= Date.now()) throw new Error("Invalid remote terminal scope");
  const { helper } = record;
  const relayId = randomUUID();
  let closed = false;
  const close = () => {
    if (closed) return;
    closed = true; helper.relays.delete(relayId);
    try { helper.send({ type: "detach", relayId }); } catch { /* A lost owner already closes its helper. */ }
  };
  helper.relays.set(relayId, { data: onData, close: (code, reason) => { if (!closed) { closed = true; onClose(code, reason); } } });
  try { await helper.request("attach", { relayId, grantId, deviceId, taskId, paneId }); }
  catch (error) { close(); throw error; }
  return {
    write: (data, binary) => {
      if (closed) return;
      const bytes = Buffer.isBuffer(data) ? data : Buffer.from(data);
      if (bytes.length > REMOTE_TERMINAL_INPUT_BYTES) { close(); onClose(4000, "input-too-large"); return; }
      try { helper.send({ type: "data", relayId, binary, data: bytes.toString("base64") }); }
      catch { close(); onClose(4000, "stream-failed"); }
    }, close,
  };
}

/** Cancel one prepared/active route without revoking other panes belonging to the task. */
export function revokeRemoteTerminalGrant(grantId: string): void {
  const record = grants.get(grantId);
  grants.delete(grantId);
  // Admission records may have expired while their direct streams remain active.
  const targets = record ? [record.helper] : liveHelpers;
  for (const helper of targets) void helper.request("revokeGrant", { grantId }).catch(() => helper.stop());
}

export function revokeRemoteTerminalDevice(deviceId: string): void {
  deviceEpochs.set(deviceId, (deviceEpochs.get(deviceId) ?? 0) + 1);
  for (const [id, record] of grants) if (record.input.deviceId === deviceId) grants.delete(id);
  for (const helper of liveHelpers) void helper.request("revokeDevice", { deviceId }).catch(() => helper.stop());
}
export function revokeRemoteTerminalTask(taskId: string): void {
  taskEpochs.set(taskId, (taskEpochs.get(taskId) ?? 0) + 1);
  for (const [id, record] of grants) if (record.input.taskId === taskId) grants.delete(id);
  for (const helper of liveHelpers) void helper.request("revokeTask", { taskId }).catch(() => helper.stop());
}
export function stopRemoteTerminalAgents(): void {
  ownerEpoch += 1;
  for (const helper of ownedHelpers) helper.stop();
  for (const pending of helpers.values()) void pending.then((helper) => helper?.stop()).catch(() => {});
  helpers.clear(); grants.clear();
}
