/**
 * @vitest-environment node
 */
import http from "node:http";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const { rejectBrowserOriginRequest } = require("./browserOriginGuard");

let server;
let baseUrl;

describe("browserOriginGuard", () => {
  beforeEach(async () => {
    server = http.createServer((request, response) => {
      if (rejectBrowserOriginRequest(request, response)) {
        return;
      }
      response.writeHead(200);
      response.end("handled");
    });
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    baseUrl = `http://127.0.0.1:${server.address().port}`;
  });

  afterEach(async () => {
    await new Promise((resolve) => server.close(resolve));
  });

  it("Origin 헤더가 있는 요청은 처리 전에 403으로 거부한다", async () => {
    const response = await fetch(`${baseUrl}/api/hooks/status`, {
      method: "POST",
      headers: { Origin: "https://attacker.example", "Content-Type": "text/plain" },
      body: "{}",
    });

    expect(response.status).toBe(403);
  });

  it("Origin 헤더가 없는 hook·앱 요청은 그대로 통과시킨다", async () => {
    const response = await fetch(`${baseUrl}/api/hooks/status`, { method: "POST", body: "{}" });

    expect(response.status).toBe(200);
    await expect(response.text()).resolves.toBe("handled");
  });
});
