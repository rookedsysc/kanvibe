/** @vitest-environment node */
import http from "node:http";
import { once } from "node:events";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebSocket from "ws";
const require = createRequire(import.meta.url);
const { handleMobileRequest, attachMobileStreamServer } = require("./mobileRoutes.js");
const accountId = "a".repeat(64);
let bridge, server, port, unpaired;
let clients;
beforeEach(async () => {
  clients = [];
  bridge = {
    authorizeMobileRequest: vi.fn(async (header) => ({ "Bearer one": "device-one", "Bearer two": "device-two" })[header] ?? null),
    onMobileDeviceUnpaired: (listener) => { unpaired = listener; return () => {}; },
    onMobileNotificationsChanged: () => () => {},
    getMobileLiveAiSessions: vi.fn(async () => ({ sessions: [] })),
    openMobileAiLogin: vi.fn(async () => {}),
    writeMobileAiLogin: vi.fn(), resizeMobileAiLogin: vi.fn(), closeMobileAiLogin: vi.fn(), closeMobileAiLoginsForDevice: vi.fn(), closeAllMobileAiLogins: vi.fn(),
  };
  server = http.createServer((request, response) => void handleMobileRequest(request, response, { bridge, host: "127.0.0.1", port: 4885 }));
  attachMobileStreamServer(server, { bridge, host: "127.0.0.1", port: 4885 });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  port = server.address().port;
});
afterEach(async () => {
  for (const client of clients) client.terminate();
  await new Promise((resolve) => server.close(resolve));
});
function client(token) {
  const socket = new WebSocket(`ws://127.0.0.1:${port}/api/mobile/ai/accounts/${accountId}/login?cols=80&rows=24`, { headers: { authorization: `Bearer ${token}` } });
  clients.push(socket);
  return socket;
}
async function ready(token) {
  const socket = client(token);
  await once(socket, "message");
  return socket;
}
describe("AI provider mounted behind the common mobile authentication boundary", () => {
  it("protects AI HTTP and login WebSocket endpoints before provider calls", async () => {
    const response = await fetch(`http://127.0.0.1:${port}/api/mobile/tasks/task/live-ai-sessions`);
    expect(response.status).toBe(401);
    expect(bridge.getMobileLiveAiSessions).not.toHaveBeenCalled();
    const rejected = client("bad");
    const status = await new Promise((resolve) => {
      rejected.on("unexpected-response", (_, response) => { resolve(response.statusCode); response.resume(); rejected.terminate(); });
      rejected.on("error", () => {});
    });
    expect(status).toBe(401);
    expect(bridge.openMobileAiLogin).not.toHaveBeenCalled();
  });
  it("passes the authenticated device identity and owns input, resize and close cleanup", async () => {
    const socket = await ready("one");
    expect(bridge.openMobileAiLogin).toHaveBeenCalledWith("device-one", expect.any(String), accountId, 80, 24, expect.any(Object));
    socket.send(JSON.stringify({ type: "input", data: "login\r" }));
    socket.send(JSON.stringify({ type: "resize", cols: 90, rows: 30 }));
    await vi.waitFor(() => expect(bridge.writeMobileAiLogin).toHaveBeenCalledOnce());
    const socketId = bridge.openMobileAiLogin.mock.calls[0][1];
    expect(bridge.writeMobileAiLogin).toHaveBeenCalledWith("device-one", socketId, "login\r");
    expect(bridge.resizeMobileAiLogin).toHaveBeenCalledWith("device-one", socketId, 90, 30);
    const closed = once(socket, "close");
    socket.close();
    await closed;
    await vi.waitFor(() => expect(bridge.closeMobileAiLogin).toHaveBeenCalledWith("device-one", socketId));
  });
  it("unpairing closes only that device while server shutdown closes all remaining logins", async () => {
    const first = await ready("one");
    const second = await ready("two");
    const firstClosed = once(first, "close");
    unpaired("device-one");
    expect((await firstClosed)[0]).toBe(4001);
    expect(bridge.closeMobileAiLoginsForDevice).toHaveBeenCalledWith("device-one");
    expect(second.readyState).toBe(WebSocket.OPEN);
    const secondClosed = once(second, "close");
    await new Promise((resolve) => server.close(resolve));
    expect((await secondClosed)[0]).toBe(1001);
    expect(bridge.closeAllMobileAiLogins).toHaveBeenCalledOnce();
  });
});
