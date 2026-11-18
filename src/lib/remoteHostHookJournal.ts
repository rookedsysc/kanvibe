import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { lstat, mkdir, open, readdir, rename, rm } from "node:fs/promises";
import { homedir } from "node:os";
import path from "node:path";
import { decodeHostBytes, isValidHostNamespace } from "./remoteHostProtocol";

export const HOST_HOOK_JOURNAL_MAX_EVENTS = 4096;
export const HOST_HOOK_JOURNAL_MAX_BYTES = 16 * 1024 * 1024;
export const HOST_HOOK_JOURNAL_RETENTION_MS = 7 * 24 * 60 * 60_000;
const MAX_HOOK_BYTES = 512 * 1024;
const CLIENT_ID = /^[A-Za-z0-9-]{1,128}$/;
const STATUS = new Set(["todo", "progress", "pending", "review", "done"]);

export interface HostHookJournalRequest {
  path: string;
  method: string;
  headers: Record<string, string>;
  body: string;
}
export interface HostHookJournalEvent extends HostHookJournalRequest {
  id: string;
  sequence: number;
  receivedAt: number;
  clientId: string;
}
interface JournalSnapshot {
  version: 1;
  nextSequence: number;
  expired: number;
  rejected: number;
  ignored: number;
  events: HostHookJournalEvent[];
}
export type HostHookJournal = Awaited<ReturnType<typeof openRemoteHostHookJournal>>;

function validateRequest(clientId: string, request: HostHookJournalRequest): void {
  if (!CLIENT_ID.test(clientId) || request.method !== "POST" || request.path !== "/api/hooks/status") throw new Error("Invalid journal hook request");
  if (!request.headers || typeof request.headers !== "object" || Array.isArray(request.headers)) throw new Error("Invalid journal hook headers");
  for (const [key, value] of Object.entries(request.headers)) {
    if (!["content-type", "x-kanvibe-client-id"].includes(key) || typeof value !== "string" || value.length > 8192 || /[\r\n]/.test(value)) throw new Error("Invalid journal hook headers");
    if (key === "x-kanvibe-client-id" && value !== clientId) throw new Error("Journal hook client mismatch");
  }
  const input: unknown = JSON.parse(decodeHostBytes(request.body, MAX_HOOK_BYTES).toString("utf8"));
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("Invalid journal hook body");
  const status = input as { taskId?: unknown; status?: unknown };
  if (typeof status.taskId !== "string" || !status.taskId.length || status.taskId.length > 128 || typeof status.status !== "string" || !STATUS.has(status.status.toLowerCase())) throw new Error("Invalid journal hook status");
}

async function ensureDirectory(directory: string, privateMode: boolean): Promise<void> {
  await mkdir(directory, { mode: 0o700 }).catch((error: NodeJS.ErrnoException) => { if (error.code !== "EEXIST") throw error; });
  const stat = await lstat(directory);
  if (!stat.isDirectory() || stat.isSymbolicLink() || stat.uid !== process.getuid?.() || (privateMode ? (stat.mode & 0o777) !== 0o700 : (stat.mode & 0o022) !== 0)) throw new Error("Untrusted journal private directory");
}

async function prepareJournalDirectory(namespace: string, rootDirectory?: string): Promise<string> {
  if (!isValidHostNamespace(namespace) || !process.getuid) throw new Error("Invalid host journal namespace or runtime");
  let root = rootDirectory;
  if (!root) {
    const home = homedir();
    const homeStat = await lstat(home);
    if (!homeStat.isDirectory() || homeStat.isSymbolicLink() || homeStat.uid !== process.getuid() || (homeStat.mode & 0o022) !== 0) throw new Error("Untrusted journal home directory");
    let current = home;
    for (const component of [".local", "state", "kanvibe", "host"]) {
      current = path.join(current, component);
      await ensureDirectory(current, component === "kanvibe" || component === "host");
    }
    root = current;
  } else {
    if (!path.isAbsolute(root)) throw new Error("Invalid journal root");
    await ensureDirectory(root, true);
  }
  const directory = path.join(root, namespace);
  await ensureDirectory(directory, true);
  return directory;
}

function validateSnapshot(value: unknown): JournalSnapshot {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid journal snapshot");
  const snapshot = value as JournalSnapshot;
  if (snapshot.version !== 1 || !Number.isSafeInteger(snapshot.nextSequence) || snapshot.nextSequence < 1 || !Array.isArray(snapshot.events) || snapshot.events.length > HOST_HOOK_JOURNAL_MAX_EVENTS) throw new Error("Invalid journal snapshot");
  for (const count of [snapshot.expired, snapshot.rejected, snapshot.ignored]) if (!Number.isSafeInteger(count) || count < 0) throw new Error("Invalid journal counter");
  const ids = new Set<string>();
  let previousSequence = 0;
  for (const event of snapshot.events) {
    if (!event || typeof event !== "object" || typeof event.id !== "string" || !CLIENT_ID.test(event.id) || ids.has(event.id) || !Number.isSafeInteger(event.sequence) || event.sequence <= previousSequence || event.sequence >= snapshot.nextSequence || !Number.isSafeInteger(event.receivedAt) || event.receivedAt < 0) throw new Error("Invalid journal event");
    validateRequest(event.clientId, event);
    ids.add(event.id); previousSequence = event.sequence;
  }
  return snapshot;
}

async function readSnapshot(filePath: string): Promise<JournalSnapshot> {
  let file;
  try { file = await open(filePath, constants.O_RDONLY | constants.O_NOFOLLOW); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return { version: 1, nextSequence: 1, expired: 0, rejected: 0, ignored: 0, events: [] };
    throw new Error("Untrusted journal file", { cause: error });
  }
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.uid !== process.getuid?.() || (stat.mode & 0o777) !== 0o600 || stat.size > HOST_HOOK_JOURNAL_MAX_BYTES) throw new Error("Untrusted journal file");
    return validateSnapshot(JSON.parse(await file.readFile("utf8")));
  } finally { await file.close(); }
}

async function removeAbandonedPublications(directory: string): Promise<void> {
  for (const name of await readdir(directory)) {
    if (!/^\.hook-journal-[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}\.tmp$/.test(name)) continue;
    const filePath = path.join(directory, name);
    const stat = await lstat(filePath);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.uid !== process.getuid?.() || (stat.mode & 0o777) !== 0o600) throw new Error("Untrusted journal publication file");
    await rm(filePath);
  }
}

/** 호스트 시작 잠금을 가진 단일 writer만 열어 미완료 publication을 회수한다. */
export async function openRemoteHostHookJournal(options: { namespace: string; rootDirectory?: string }) {
  const directory = await prepareJournalDirectory(options.namespace, options.rootDirectory);
  const filePath = path.join(directory, "hook-journal-v1.json");
  let snapshot = await readSnapshot(filePath);
  await removeAbandonedPublications(directory);
  let operation: Promise<unknown> = Promise.resolve();

  const persist = async (next: JournalSnapshot): Promise<void> => {
    const text = JSON.stringify(next);
    if (Buffer.byteLength(text) > HOST_HOOK_JOURNAL_MAX_BYTES) throw new Error("Hook journal capacity exceeded");
    const pendingPath = path.join(directory, ".hook-journal-" + randomUUID() + ".tmp");
    try {
      const file = await open(pendingPath, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
      try { await file.writeFile(text); await file.sync(); } finally { await file.close(); }
      await rename(pendingPath, filePath);
      const folder = await open(directory, constants.O_RDONLY);
      try { await folder.sync(); } finally { await folder.close(); }
      snapshot = next;
    } finally { await rm(pendingPath, { force: true }); }
  };
  const serial = <T>(run: () => Promise<T>): Promise<T> => {
    const next = operation.then(run);
    operation = next.catch(() => undefined);
    return next;
  };
  const expire = async (): Promise<void> => {
    const cutoff = Date.now() - HOST_HOOK_JOURNAL_RETENTION_MS;
    const events = snapshot.events.filter((event) => event.receivedAt >= cutoff);
    if (events.length !== snapshot.events.length) await persist({ ...snapshot, events, expired: snapshot.expired + snapshot.events.length - events.length });
  };
  return {
    filePath,
    flush(): Promise<void> { return serial(async () => undefined); },
    append(clientId: string, request: HostHookJournalRequest): Promise<HostHookJournalEvent> {
      return serial(async () => {
        validateRequest(clientId, request);
        await expire();
        const event: HostHookJournalEvent = { ...request, headers: { ...request.headers }, clientId, id: randomUUID(), sequence: snapshot.nextSequence, receivedAt: Date.now() };
        const next: JournalSnapshot = { ...snapshot, nextSequence: snapshot.nextSequence + 1, events: [...snapshot.events, event] };
        if (!Number.isSafeInteger(next.nextSequence) || next.events.length > HOST_HOOK_JOURNAL_MAX_EVENTS || Buffer.byteLength(JSON.stringify(next)) > HOST_HOOK_JOURNAL_MAX_BYTES - 1024) {
          await persist({ ...snapshot, rejected: snapshot.rejected + 1 });
          throw new Error("Hook journal capacity exceeded");
        }
        await persist(next);
        return { ...event, headers: { ...event.headers } };
      });
    },
    next(clientId: string): Promise<HostHookJournalEvent | null> {
      return serial(async () => {
        if (!CLIENT_ID.test(clientId)) throw new Error("Invalid journal client");
        await expire();
        const event = snapshot.events.find((entry) => entry.clientId === clientId);
        return event ? { ...event, headers: { ...event.headers } } : null;
      });
    },
    acknowledge(clientId: string, eventId: string, outcome: "accepted" | "rejected" | "ignored"): Promise<boolean> {
      return serial(async () => {
        if (!CLIENT_ID.test(clientId) || !CLIENT_ID.test(eventId) || !["accepted", "rejected", "ignored"].includes(outcome)) throw new Error("Invalid journal acknowledgement");
        const event = snapshot.events.find((entry) => entry.clientId === clientId);
        if (!event || event.id !== eventId) return false;
        await persist({ ...snapshot, events: snapshot.events.filter((entry) => entry !== event), rejected: snapshot.rejected + Number(outcome === "rejected"), ignored: snapshot.ignored + Number(outcome === "ignored") });
        return true;
      });
    },
    status(): Promise<{ pending: number; bytes: number; expired: number; rejected: number; ignored: number }> {
      return serial(async () => {
        await expire();
        return { pending: snapshot.events.length, bytes: Buffer.byteLength(JSON.stringify(snapshot)), expired: snapshot.expired, rejected: snapshot.rejected, ignored: snapshot.ignored };
      });
    },
  };
}
