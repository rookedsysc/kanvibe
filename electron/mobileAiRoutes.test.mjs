import { createRequire } from "node:module";
import http from "node:http";
import { EventEmitter } from "node:events";
import WebSocket, { WebSocketServer } from "ws";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const require = createRequire(import.meta.url);
const { handleMobileAiRequest, attachMobileAiLoginStream } = require("./mobileAiRoutes.js");
let bridge;
let server;
let origin;
beforeEach(async () => {
  bridge = {
    getMobileRunningAgentPanes: vi.fn(async () => []),
    getMobileLiveAiSessions: vi.fn(async () => ({ taskId: "task-1", sessions: [] })),
    getMobileAgentCallGraph: vi.fn(async () => null),
    listMobileAiAccounts: vi.fn(async () => [{ accountId: "a".repeat(64), provider: "claude" }]),
    addMobileAiAccount: vi.fn(async () => ({ accountId: "a".repeat(64) })),
    removeMobileAiAccount: vi.fn(async () => ({ outcome: "ok" })),
    getMobileAiUsageSnapshot: vi.fn(async () => ({ accounts: [], fetchedAt: "now" })),
    openMobileAiLogin: vi.fn(async () => {}), writeMobileAiLogin: vi.fn(), resizeMobileAiLogin: vi.fn(), closeMobileAiLogin: vi.fn(),
  };
  server = http.createServer(async (request, response) => {
    if (request.headers.authorization !== "Bearer paired") { response.writeHead(401).end(); return; }
    if (!await handleMobileAiRequest(request, response, { bridge, requestUrl: new URL(request.url, "http://127.0.0.1") })) response.writeHead(404).end();
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  origin = `http://127.0.0.1:${server.address().port}`;
});
afterEach(async () => { server.closeAllConnections(); await new Promise((resolve) => server.close(resolve)); });
async function api(route, method = "GET", body, authorization = "Bearer paired") {
  const response = await fetch(origin + route, { method, headers: { authorization, "Content-Type": "application/json" }, body: body === undefined ? undefined : typeof body === "string" ? body : JSON.stringify(body) });
  return { status: response.status, body: response.status === 401 || response.status === 404 ? null : await response.json() };
}
function socket() {
  return Object.assign(new EventEmitter(), { OPEN: 1, readyState: 1, bufferedAmount: 0, send: vi.fn(), close: vi.fn() });
}
function loginOptions(overrides = {}) { return { bridge, deviceId: "device-1", requestUrl: new URL(`http://local/api/mobile/ai/accounts/${"a".repeat(64)}/login?cols=80&rows=24`), ...overrides }; }

describe("mobile AI HTTP routes", () => {
  it("returns the real bridge response envelopes for live, graph, accounts and usage", async () => {
    expect(await api("/api/mobile/ai/running-panes")).toEqual({ status: 200, body: { success: true, panes: [] } });
    expect(await api("/api/mobile/tasks/task-1/live-ai-sessions")).toEqual({ status: 200, body: { success: true, liveSessions: { taskId: "task-1", sessions: [] } } });
    expect(await api("/api/mobile/tasks/task-1/agent-call-graph?provider=claude&sessionId=session-1")).toEqual({ status: 200, body: { success: true, graph: null } });
    expect(bridge.getMobileAgentCallGraph).toHaveBeenCalledWith("task-1", "claude", "session-1");
    expect((await api("/api/mobile/ai/accounts")).body.accounts).toHaveLength(1);
    expect(await api("/api/mobile/ai/usage")).toEqual({ status: 200, body: { success: true, usage: { accounts: [], fetchedAt: "now" } } });
  });
  it("creates and removes accounts only through explicit methods", async () => {
    expect(await api("/api/mobile/ai/accounts", "POST", { provider: "claude", accountName: "work" })).toEqual({ status: 201, body: { success: true, accountId: "a".repeat(64) } });
    expect(bridge.addMobileAiAccount).toHaveBeenCalledWith({ provider: "claude", accountName: "work" });
    expect(await api(`/api/mobile/ai/accounts/${"a".repeat(64)}`, "DELETE")).toEqual({ status: 200, body: { success: true, outcome: "ok" } });
  });
  it("keeps authentication ahead of every account mutation", async () => {
    expect((await api("/api/mobile/ai/accounts", "POST", {}, "Bearer wrong")).status).toBe(401);
    expect(bridge.addMobileAiAccount).not.toHaveBeenCalled();
  });
  it("rejects malformed JSON with 400 and preserves request errors", async () => {
    expect((await api("/api/mobile/ai/accounts", "POST", "{")).status).toBe(400);
    bridge.addMobileAiAccount.mockRejectedValue(Object.assign(new Error("중복된 계정"), { statusCode: 409 }));
    expect(await api("/api/mobile/ai/accounts", "POST", {})).toEqual({ status: 409, body: { success: false, error: "중복된 계정" } });
  });
  it("does not expose internal failures or dispatch unknown routes", async () => {
    bridge.listMobileAiAccounts.mockRejectedValue(new Error("secret filesystem path"));
    expect(await api("/api/mobile/ai/accounts")).toEqual({ status: 500, body: { success: false, error: "AI 기능 요청을 처리하지 못했습니다" } });
    expect((await api("/api/mobile/ai/accounts", "PATCH", {})).status).toBe(404);
    expect((await api("/api/mobile/unknown")).status).toBe(404);
  });
  it("rejects missing graph parameters and encoded-invalid identifiers before reader calls", async () => {
    expect((await api("/api/mobile/tasks/task-1/agent-call-graph")).status).toBe(400);
    expect((await api("/api/mobile/tasks/%ZZ/live-ai-sessions")).status).toBe(400);
    expect(bridge.getMobileAgentCallGraph).not.toHaveBeenCalled();
    expect(bridge.getMobileLiveAiSessions).not.toHaveBeenCalled();
  });
});

describe("mobile AI authenticated login stream", () => {
  it("routes output, input, resize and close to the generated device/socket owner", async () => {
    const webSocket = socket();
    await attachMobileAiLoginStream(webSocket, loginOptions());
    const [deviceId, socketId, accountId, cols, rows, callbacks] = bridge.openMobileAiLogin.mock.calls[0];
    expect([deviceId, accountId, cols, rows]).toEqual(["device-1", "a".repeat(64), 80, 24]);
    expect(socketId).toBeTypeOf("string");
    expect(webSocket.send).toHaveBeenCalledWith(JSON.stringify({ type: "ready" }));
    callbacks.onEvent({ type: "output", data: "OAuth code:" });
    expect(webSocket.send).toHaveBeenCalledWith(JSON.stringify({ type: "output", data: "OAuth code:" }));
    webSocket.emit("message", Buffer.from(JSON.stringify({ type: "input", data: "123\r" })), false);
    webSocket.emit("message", Buffer.from(JSON.stringify({ type: "resize", cols: 90, rows: 30 })), false);
    expect(bridge.writeMobileAiLogin).toHaveBeenCalledWith(deviceId, socketId, "123\r");
    expect(bridge.resizeMobileAiLogin).toHaveBeenCalledWith(deviceId, socketId, 90, 30);
    webSocket.emit("close");
    expect(bridge.closeMobileAiLogin).toHaveBeenCalledWith(deviceId, socketId);
  });
  it("does not send ready after a disconnect while account lookup is pending", async () => {
    const webSocket = socket();
    let complete;
    bridge.openMobileAiLogin.mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
    const opening = attachMobileAiLoginStream(webSocket, loginOptions());
    webSocket.readyState = 3;
    webSocket.emit("close");
    complete();
    await opening;
    expect(webSocket.send).not.toHaveBeenCalled();
    expect(bridge.openMobileAiLogin.mock.calls[0][5].isOpen()).toBe(false);
  });
  it("closes invalid, oversized and binary frames without executing terminal input", async () => {
    for (const frame of ["{", JSON.stringify({ type: "run-shell", command: "bad" }), JSON.stringify({ type: "resize", cols: -1, rows: 0 }), "x".repeat(65537)]) {
      const webSocket = socket();
      await attachMobileAiLoginStream(webSocket, loginOptions());
      webSocket.emit("message", Buffer.from(frame), false);
      expect(webSocket.close).toHaveBeenCalled();
    }
    const binary = socket();
    await attachMobileAiLoginStream(binary, loginOptions());
    binary.emit("message", Buffer.from('{"type":"input","data":"bad"}'), true);
    expect(binary.close).toHaveBeenCalled();
    expect(bridge.writeMobileAiLogin).not.toHaveBeenCalled();
  });
  it("cleans the process immediately for exit, timeout or backpressure", async () => {
    const webSocket = socket();
    await attachMobileAiLoginStream(webSocket, loginOptions());
    const callbacks = bridge.openMobileAiLogin.mock.calls[0][5];
    callbacks.onEvent({ type: "exit", exitCode: 0 });
    expect(webSocket.close).toHaveBeenCalledWith(1000, "login-finished");
    expect(bridge.closeMobileAiLogin).toHaveBeenCalled();
    const overloaded = socket();
    await attachMobileAiLoginStream(overloaded, loginOptions());
    overloaded.bufferedAmount = 2 * 1024 * 1024;
    bridge.openMobileAiLogin.mock.calls[1][5].onEvent({ type: "output", data: "too slow" });
    expect(overloaded.close).toHaveBeenCalledWith(1009, "login-output-overflow");
  });
});

describe("AI login over a real authenticated WebSocket", () => {
  it("streams terminal data bidirectionally and releases the owner on disconnect", async () => {
    const webSocketServer = new WebSocketServer({ noServer: true });
    const requests = [];
    server.on("upgrade", (request, socket, head) => {
      if (request.headers.authorization !== "Bearer paired") {
        socket.write("HTTP/1.1 401 Unauthorized\r\n\r\n");
        socket.destroy();
        return;
      }
      webSocketServer.handleUpgrade(request, socket, head, (webSocket) => {
        requests.push(webSocket);
        void attachMobileAiLoginStream(webSocket, loginOptions({ requestUrl: new URL(request.url, origin) }));
      });
    });
    const client = new WebSocket(origin.replace("http", "ws") + `/api/mobile/ai/accounts/${"a".repeat(64)}/login`, { headers: { authorization: "Bearer paired" } });
    const events = [];
    client.on("message", (data) => events.push(JSON.parse(data.toString())));
    try {
      await vi.waitFor(() => expect(events).toContainEqual({ type: "ready" }));
      const callbacks = bridge.openMobileAiLogin.mock.calls[0][5];
      callbacks.onEvent({ type: "output", data: "paste verification code" });
      await vi.waitFor(() => expect(events).toContainEqual({ type: "output", data: "paste verification code" }));
      client.send(JSON.stringify({ type: "input", data: "test-code\r" }));
      await vi.waitFor(() => expect(bridge.writeMobileAiLogin).toHaveBeenCalledWith("device-1", expect.any(String), "test-code\r"));
      client.close();
      await vi.waitFor(() => expect(bridge.closeMobileAiLogin).toHaveBeenCalled());
      const unauthorized = new WebSocket(origin.replace("http", "ws") + `/api/mobile/ai/accounts/${"a".repeat(64)}/login`);
      const unauthorizedFailure = await new Promise((resolve) => unauthorized.once("error", (error) => resolve(error.message)));
      expect(unauthorizedFailure).toContain("401");
      expect(bridge.openMobileAiLogin).toHaveBeenCalledTimes(1);
    } finally {
      client.terminate();
      for (const webSocket of requests) webSocket.terminate();
      webSocketServer.close();
    }
  });
});
