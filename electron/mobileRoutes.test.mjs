/**
 * @vitest-environment node
 */
import { EventEmitter } from "node:events";
import http from "node:http";
import net from "node:net";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import WebSocket from "ws";

const require = createRequire(import.meta.url);
const { handleMobileRequest, attachMobileStreamServer } = require("./mobileRoutes");
const { MAX_BODY_BYTES, readJsonBody } = require("./httpBody");

const HOST = "127.0.0.1";

let bridge;
let server;
let port;
let openSockets;

function createBridge(overrides = {}) {
  return {
    authorizeMobileRequest: vi.fn(async (header) => header === "Bearer good"),
    pairMobileDevice: vi.fn(async () => "paired-token"),
    getMobileBoard: vi.fn(async () => ({ columns: [] })),
    getTaskSurfaces: vi.fn(async () => ({ panes: [] })),
    subscribeToPane: vi.fn(async () => () => {}),
    writeToPane: vi.fn(async () => {}),
    ...overrides,
  };
}

/** hookServer와 같은 방식으로 얹어야 "우리 경로가 아니면 false" 계약까지 같이 검증된다 */
function startServer() {
  return new Promise((resolve) => {
    openSockets = [];
    const created = http.createServer(async (request, response) => {
      if (await handleMobileRequest(request, response, { bridge, host: HOST, port })) {
        return;
      }
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end(JSON.stringify({ handledBy: "hook-fallthrough" }));
    });
    created.on("connection", (socket) => openSockets.push(socket));
    attachMobileStreamServer(created, { bridge, host: HOST, port: 0 });
    created.listen(0, HOST, () => {
      port = created.address().port;
      resolve(created);
    });
  });
}

function request(path, { method = "GET", headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const clientRequest = http.request({ host: HOST, port, path, method, headers }, (response) => {
      let raw = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => {
        raw += chunk;
      });
      response.on("end", () => resolve({ status: response.statusCode, body: raw ? JSON.parse(raw) : null }));
    });
    clientRequest.on("error", reject);
    if (body !== undefined) {
      clientRequest.write(body);
    }
    clientRequest.end();
  });
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function waitFor(predicate, timeoutMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) {
      return;
    }
    await sleep(10);
  }
  throw new Error("조건이 시간 안에 만족되지 않았다");
}

beforeEach(async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  bridge = createBridge();
  server = await startServer();
});

afterEach(async () => {
  for (const socket of openSockets) {
    socket.destroy();
  }
  await new Promise((resolve) => server.close(resolve));
  vi.restoreAllMocks();
});

describe("모바일 라우트 인증 경계", () => {
  it("페어링 요청은 토큰 없이도 지나간다", async () => {
    const response = await request("/api/mobile/pair", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code: "123456", deviceName: "iPhone" }),
    });

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, token: "paired-token" });
    expect(bridge.authorizeMobileRequest).not.toHaveBeenCalled();
  });

  it("보드와 pane 경로는 토큰 없이는 401로 막힌다", async () => {
    const board = await request("/api/mobile/board");
    const surfaces = await request("/api/mobile/tasks/task-1/surfaces");

    expect(board.status).toBe(401);
    expect(surfaces.status).toBe(401);
    expect(bridge.getMobileBoard).not.toHaveBeenCalled();
    expect(bridge.getTaskSurfaces).not.toHaveBeenCalled();
  });

  it("모바일 접두사 아래의 모르는 경로는 404를 준다", async () => {
    const response = await request("/api/mobile/unknown", { headers: { authorization: "Bearer good" } });

    expect(response.status).toBe(404);
    expect(response.body).toEqual({ success: false, error: "Not found" });
  });

  it("모바일 경로가 아니면 false를 돌려 hook 라우팅으로 넘긴다", async () => {
    const response = await request("/api/hooks/health");

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ handledBy: "hook-fallthrough" });
  });
});

describe("모바일 라우트 오류 응답", () => {
  it("세션이 없는 pane은 409와 이유를 준다", async () => {
    const unavailable = Object.assign(new Error("세션 없음"), {
      name: "SurfaceUnavailableError",
      reason: "session-missing",
    });
    bridge.getTaskSurfaces.mockRejectedValue(unavailable);

    const response = await request("/api/mobile/tasks/task-1/surfaces", {
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(409);
    expect(response.body).toEqual({ success: false, reason: "session-missing" });
  });

  it("그 밖의 예외는 내부 메시지를 감춘 500으로 나간다", async () => {
    bridge.getTaskSurfaces.mockRejectedValue(new Error("SSH 호스트를 찾을 수 없습니다: secret-alias"));

    const response = await request("/api/mobile/tasks/task-1/surfaces", {
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
    expect(JSON.stringify(response.body)).not.toContain("secret-alias");
  });

  it("보드 조회가 던져도 매달리지 않고 500을 돌려준다", async () => {
    bridge.getMobileBoard.mockRejectedValue(new Error("DB가 닫혔다"));

    const response = await request("/api/mobile/board", { headers: { authorization: "Bearer good" } });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });

  it("경로에 깨진 퍼센트 인코딩이 와도 매달리지 않고 500을 돌려준다", async () => {
    const response = await request("/api/mobile/tasks/%/surfaces", {
      headers: { authorization: "Bearer good" },
    });

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });

  it("인증 조회가 던져도 매달리지 않고 500을 돌려준다", async () => {
    bridge.authorizeMobileRequest.mockRejectedValue(new Error("DB가 닫혔다"));

    const response = await request("/api/mobile/board");

    expect(response.status).toBe(500);
    expect(response.body).toEqual({ success: false, error: "서버 오류" });
  });
});

describe("요청 본문 상한", () => {
  it("상한을 넘는 본문은 버퍼에 쌓지 않고 소켓을 끊으며 거절한다", async () => {
    const fakeRequest = new EventEmitter();
    fakeRequest.destroy = vi.fn();

    const rejected = readJsonBody(fakeRequest).then(
      () => "resolved",
      (error) => error,
    );

    const chunk = Buffer.alloc(16 * 1024, 0x61);
    for (let index = 0; index * chunk.length <= MAX_BODY_BYTES; index += 1) {
      fakeRequest.emit("data", chunk);
    }

    await expect(rejected).resolves.toBeInstanceOf(Error);
    expect(fakeRequest.destroy).toHaveBeenCalled();
  });

  it("상한을 넘는 페어링 본문은 400으로 거절한다", async () => {
    const fakeRequest = new EventEmitter();
    fakeRequest.url = "/api/mobile/pair";
    fakeRequest.method = "POST";
    fakeRequest.headers = {};
    fakeRequest.destroy = vi.fn();

    const fakeResponse = { writeHead: vi.fn(), end: vi.fn() };
    const handled = handleMobileRequest(fakeRequest, fakeResponse, { bridge, host: HOST, port });

    await sleep(0);
    const chunk = Buffer.alloc(16 * 1024, 0x61);
    for (let index = 0; index * chunk.length <= MAX_BODY_BYTES; index += 1) {
      fakeRequest.emit("data", chunk);
    }

    await expect(handled).resolves.toBe(true);
    expect(fakeResponse.writeHead).toHaveBeenCalledWith(400, { "Content-Type": "application/json" });
    expect(bridge.pairMobileDevice).not.toHaveBeenCalled();
  });
});

describe("pane 스트림 업그레이드", () => {
  it("스트림 경로가 아닌 업그레이드 요청은 소켓을 끊는다", async () => {
    const socket = net.connect(port, HOST);
    await new Promise((resolve, reject) => {
      socket.once("connect", resolve);
      socket.once("error", reject);
    });

    let closed = false;
    socket.on("close", () => {
      closed = true;
    });
    socket.write(`GET / HTTP/1.1\r\nHost: ${HOST}:${port}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`);

    await waitFor(() => closed);
    expect(closed).toBe(true);
    expect(bridge.authorizeMobileRequest).not.toHaveBeenCalled();
  });

  it("taskId나 paneId가 빠지면 400으로 끊는다", async () => {
    const socket = net.connect(port, HOST);
    await new Promise((resolve, reject) => {
      socket.once("connect", resolve);
      socket.once("error", reject);
    });

    let received = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => {
      received += chunk;
    });
    socket.write(
      `GET /api/mobile/stream?taskId=task-1 HTTP/1.1\r\nHost: ${HOST}:${port}\r\n` +
        `Authorization: Bearer good\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n`,
    );

    await waitFor(() => received.includes("400"));
    expect(received).toContain("400 Bad Request");
  });

  it("구독이 붙기 전에 기기가 나가면 구독 해제를 곧바로 부른다", async () => {
    let releaseSubscription;
    const stop = vi.fn();
    bridge.subscribeToPane.mockImplementation(
      () =>
        new Promise((resolve) => {
          releaseSubscription = () => resolve(stop);
        }),
    );

    const client = new WebSocket(`ws://${HOST}:${port}/api/mobile/stream?taskId=task-1&paneId=pane-1`, {
      headers: { authorization: "Bearer good" },
    });
    await new Promise((resolve, reject) => {
      client.once("open", resolve);
      client.once("error", reject);
    });
    await waitFor(() => Boolean(releaseSubscription));

    const serverSocket = openSockets[openSockets.length - 1];
    const serverSocketClosed = new Promise((resolve) => serverSocket.once("close", resolve));
    client.close();
    await serverSocketClosed;
    await sleep(50);

    releaseSubscription();
    await waitFor(() => stop.mock.calls.length > 0);
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
