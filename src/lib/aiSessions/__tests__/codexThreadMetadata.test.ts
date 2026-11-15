import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createServer, type Server } from "node:http";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { WebSocketServer } from "ws";
const host = vi.hoisted(() => ({ home: "" }));
vi.mock("@/lib/hostFileAccess", async (load) => ({ ...await load<typeof import("@/lib/hostFileAccess")>(), getHomeDirectory: async () => host.home }));
import { readCodexEmptyThread } from "@/lib/aiSessions/codexThreadMetadata";

const id = "0190aaaa-0000-7000-8000-00000000000a";
let directory: string;
let socket: string;
let server: Server;
let clients: WebSocketServer;
let thread: Record<string, unknown>;
let codexHome: string;
let methods: { method: string; params?: Record<string, unknown> }[];
const context = () => ({ worktreePath: path.join(host.home, "task"), repoPath: null, sshHost: null });

beforeEach(async () => {
  directory = await mkdtemp(path.join(tmpdir(), "kvm-"));
  host.home = directory;
  socket = path.join(directory, "api");
  codexHome = path.join(host.home, ".codex");
  thread = { id, cwd: context().worktreePath, source: "cli", parentThreadId: null, ephemeral: false, canAcceptDirectInput: true, preview: "", path: path.join(codexHome, "sessions", `rollout-test-${id}.jsonl`) };
  methods = [];
  server = createServer();
  clients = new WebSocketServer({ server });
  clients.on("connection", (client) => client.on("message", (data) => {
    const request = JSON.parse(data.toString());
    methods.push(request);
    if (request.method === "initialize") client.send(JSON.stringify({ id: request.id, result: { codexHome } }));
    if (request.method === "thread/read") client.send(JSON.stringify({ id: request.id, result: { thread } }));
  }));
  await new Promise<void>((resolve) => server.listen(socket, resolve));
});
afterEach(async () => {
  for (const client of clients.clients) client.terminate();
  await new Promise<void>((resolve) => clients.close(() => resolve()));
  await new Promise<void>((resolve) => server.close(() => resolve()));
  await rm(directory, { recursive: true, force: true });
});

describe("unmaterialized Codex thread metadata", () => {
  it("reads only the exact thread without subscribing or changing either TUI", async () => {
    expect(await readCodexEmptyThread(context(), id, socket)).toBe(thread.path);
    expect(methods.map((request) => request.method)).toEqual(["initialize", "initialized", "thread/read"]);
    expect(methods.at(-1)?.params).toEqual({ threadId: id, includeTurns: false });
  });

  it.each([
    { id: "0190bbbb-0000-7000-8000-00000000000b" },
    { cwd: "/other-task" },
    { parentThreadId: "parent" },
    { source: { subAgent: { spawn: {} } } },
    { source: "unknown" },
    { ephemeral: true },
    { canAcceptDirectInput: false },
    { preview: "Existing conversation" },
    { path: "/outside-account/rollout.jsonl" },
  ])("rejects metadata that cannot establish an empty main thread: %j", async (override) => {
    Object.assign(thread, override);
    expect(await readCodexEmptyThread(context(), id, socket)).toBeNull();
  });

  it("does not override a materialized record rejected by the scoped file reader", async () => {
    await mkdir(path.dirname(thread.path as string), { recursive: true });
    await writeFile(thread.path as string, "untrusted or duplicate metadata");
    expect(await readCodexEmptyThread(context(), id, socket)).toBeNull();
  });

  it("rejects another provider home and never substitutes a default socket for a different transport", async () => {
    codexHome = "/other-account/.codex";
    expect(await readCodexEmptyThread(context(), id, socket)).toBeNull();
    methods.length = 0;
    expect(await readCodexEmptyThread(context(), id, null)).toBeNull();
    expect(await readCodexEmptyThread(context(), id.slice(0, 10), socket)).toBeNull();
    expect(methods).toEqual([]);
  });
});
