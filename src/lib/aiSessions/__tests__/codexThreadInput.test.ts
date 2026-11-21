import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createServer, type Server } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { WebSocketServer } from "ws";
const host = vi.hoisted(() => ({ home: "" }));
vi.mock("@/lib/hostFileAccess", async (load) => ({ ...await load<typeof import("@/lib/hostFileAccess")>(), getHomeDirectory: async () => host.home }));
import { sendCodexThreadInput, type CodexThreadInput } from "@/lib/aiSessions/codexThreadInput";
import { readCodexThreadSettings, updateCodexThreadSettings } from "@/lib/aiSessions/codexThreadSettings";

const id = "0190aaaa-0000-7000-8000-00000000000a";
let server: Server;
let clients: WebSocketServer;
let socket: string;
let home: string;
let thread: Record<string, unknown>;
let methods: { method: string; params: Record<string, unknown> }[];
let dropAfterInput: boolean;

function options(): CodexThreadInput {
  return { context: { worktreePath: path.join(host.home, "task"), repoPath: null, sshHost: null }, socket, threadId: id, sourceRef: thread.path as string,
    input: [{ type: "text", text: "첫 줄\n\nSecond line" }, { type: "localImage", path: path.join(host.home, "image.png") }],
    clientUserMessageId: "local-operation-id", beforeSend: vi.fn(async () => undefined), onWrite: vi.fn() };
}

beforeEach(async () => {
  host.home = await mkdtemp(path.join(tmpdir(), "kvi-"));
  home = path.join(host.home, ".codex");
  socket = path.join(host.home, "api");
  thread = { id, cwd: path.join(host.home, "task"), source: "cli", parentThreadId: null, ephemeral: false, canAcceptDirectInput: true,
    path: path.join(home, "sessions", `rollout-test-${id}.jsonl`), status: { type: "idle" } };
  methods = []; dropAfterInput = false;
  server = createServer(); clients = new WebSocketServer({ server });
  clients.on("connection", (client) => client.on("message", (data) => {
    const message = JSON.parse(data.toString()); methods.push(message);
    if (message.method === "initialize") client.send(JSON.stringify({ id: message.id, result: { codexHome: home } }));
    if (message.method === "thread/read") client.send(JSON.stringify({ id: message.id, result: { thread } }));
    if (message.method === "model/list") client.send(JSON.stringify({ id: message.id, result: { data: [
      { model: "model-a", displayName: "Model A", hidden: false, supportedReasoningEfforts: [{ reasoningEffort: "high", description: "High" }], serviceTiers: [{ id: "priority" }] },
      { model: "model-b", displayName: "Model B", hidden: false, supportedReasoningEfforts: [], serviceTiers: [] },
    ], nextCursor: null } }));
    if (message.method === "thread/settings/update") {
      if (message.params.model) thread.model = message.params.model;
      if (message.params.effort) thread.reasoningEffort = message.params.effort;
      client.send(JSON.stringify({ id: message.id, result: {} }));
    }
    if (message.method === "turn/start") {
      if (dropAfterInput) client.close();
      else client.send(JSON.stringify({ id: message.id, result: { turn: { id: "accepted-turn", status: "inProgress" } } }));
    }
  }));
  await new Promise<void>((resolve) => server.listen(socket, resolve));
});
afterEach(async () => {
  for (const client of clients.clients) client.terminate();
  await new Promise<void>((resolve) => clients.close(() => resolve()));
  await new Promise<void>((resolve) => server.close(() => resolve()));
  await rm(host.home, { recursive: true, force: true });
});

describe("addressed Codex input", () => {
  it("실제 모델 목록을 조회하고 해당 대화의 모델만 API로 바꾼다", async () => {
    thread.model = "model-a";
    thread.reasoningEffort = "high";
    const target = options();
    const beforeWrite = vi.fn(async () => undefined);
    expect(await readCodexThreadSettings(target)).toMatchObject({ model: "model-a", effort: "high", models: [{ id: "model-a" }, { id: "model-b" }] });
    await updateCodexThreadSettings({ ...target, beforeWrite }, { kind: "model", model: "model-b" });
    expect(methods.at(-1)).toMatchObject({ method: "thread/settings/update", params: { threadId: id, model: "model-b" } });
    expect(beforeWrite).toHaveBeenCalledOnce();
    expect((await readCodexThreadSettings(target)).model).toBe("model-b");
    expect(methods.some((item) => /turn\/|thread\/(start|resume|subscribe)/.test(item.method))).toBe(false);
  });

  it("모델 목록 밖의 값이나 잘못된 대화에는 설정 변경을 보내지 않는다", async () => {
    thread.model = "model-a";
    const target = options();
    await expect(updateCodexThreadSettings(target, { kind: "model", model: "unlisted" })).rejects.toThrow();
    thread.cwd = "/foreign";
    await expect(updateCodexThreadSettings(target, { kind: "effort", effort: "high" })).rejects.toThrow();
    expect(methods.some((item) => item.method === "thread/settings/update")).toBe(false);
  });
  it("sends exact text/images once without resuming, subscribing or changing auth/model/permissions", async () => {
    const request = options();
    await sendCodexThreadInput(request);
    expect(methods.map((m) => m.method)).toEqual(["initialize", "initialized", "thread/read", "turn/start"]);
    expect(methods.at(-1)?.params).toEqual({ threadId: id, input: request.input, clientUserMessageId: request.clientUserMessageId });
    expect(request.beforeSend).toHaveBeenCalledOnce();
    expect(request.onWrite).toHaveBeenCalledOnce();
  });

  it.each([
    { id: "0190bbbb-0000-7000-8000-00000000000b" }, { cwd: "/another-task" }, { source: { subAgent: {} } },
    { parentThreadId: "parent" }, { ephemeral: true }, { canAcceptDirectInput: false }, { path: "/another-account/record.jsonl" }, { status: { type: "notLoaded" } }, { status: { type: "systemError" } },
  ])("refuses a foreign/child/unavailable/busy target before writing input: %j", async (changes) => {
    const request = options(); Object.assign(thread, changes);
    await expect(sendCodexThreadInput(request)).rejects.toThrow();
    expect(methods.some((m) => m.method === "turn/start")).toBe(false);
    expect(request.onWrite).not.toHaveBeenCalled();
  });

  it("이미 진행 중인 대화에도 동일 threadId로 입력을 한 번 추가한다", async () => {
    thread.status = { type: "active", activeFlags: [] };
    const request = options();
    await sendCodexThreadInput(request);
    expect(methods.at(-1)).toMatchObject({ method: "turn/start", params: { threadId: id, input: request.input } });
    expect(request.onWrite).toHaveBeenCalledOnce();
  });

  it("rechecks the live lease after async metadata and refuses a changed target", async () => {
    const request = options(); request.beforeSend = vi.fn(async () => { throw new Error("target changed"); });
    await expect(sendCodexThreadInput(request)).rejects.toThrow("target changed");
    expect(methods.some((m) => m.method === "turn/start")).toBe(false);
    expect(request.onWrite).not.toHaveBeenCalled();
  });

  it("keeps a lost response uncertain and never automatically resends the user input", async () => {
    dropAfterInput = true;
    const request = options();
    await expect(sendCodexThreadInput(request)).rejects.toThrow();
    expect(methods.filter((m) => m.method === "turn/start")).toHaveLength(1);
    expect(request.onWrite).toHaveBeenCalledOnce();
  });

  it("rejects an account-home mismatch and unsupported socket without fallback", async () => {
    home = "/other-account/.codex";
    const request = options();
    await expect(sendCodexThreadInput(request)).rejects.toThrow();
    expect(methods.map((m) => m.method)).toEqual(["initialize"]);
    methods.length = 0;
    await expect(sendCodexThreadInput({ ...request, socket: null })).rejects.toThrow();
    expect(methods).toEqual([]);
    expect(request.onWrite).not.toHaveBeenCalled();
  });
});
