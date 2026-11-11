/**
 * @vitest-environment node
 */
import http from "node:http";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
let server;
let baseUrl;
let peerAddress;
let handled;

describe("hookTransportGuard", () => {
  beforeEach(async () => {
    server = undefined;
    const { rejectRemoteHookRequest } = require("./hookTransportGuard.js");
    peerAddress = "192.0.2.10";
    handled = 0;
    server = http.createServer((request, response) => {
      Object.defineProperty(request.socket, "remoteAddress", { value: peerAddress });
      if (rejectRemoteHookRequest(request, response)) return;
      handled += 1;
      response.end("handled");
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    baseUrl = `http://127.0.0.1:${server.address().port}`;
  });

  afterEach(async () => {
    if (server?.listening) {
      await new Promise((resolve) => server.close(resolve));
    }
  });

  it.each([
    "/api/hooks/status",
    "/api/hooks/health",
    "/api/install/hooks.sh",
    "/api/cli/tasks",
    "/api/cli/tasks/delete",
    "/api/cli/tasks/register",
  ])("원격 TCP 요청 %s 는 내부 처리 전에 거부한다", async (route) => {
    const response = await fetch(baseUrl + route, {
      headers: { Host: "localhost", "X-Forwarded-For": "127.0.0.1" },
    });

    expect(response.status).toBe(404);
    expect(await response.json()).toEqual({ success: false, error: "Not found" });
    expect(handled).toBe(0);
  });

  it.each(["127.0.0.1", "::1", "::ffff:127.0.0.1"])("로컬 peer %s 는 그대로 처리한다", async (address) => {
    peerAddress = address;

    const response = await fetch(baseUrl + "/api/hooks/health");

    expect(response.status).toBe(200);
    expect(await response.text()).toBe("handled");
    expect(handled).toBe(1);
  });
});
